// arch/granite/model.cpp - Granite Speech family handler.

#include "decoder.h"
#include "encoder.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "granite.h"
#include "projector.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-env.h"
#include "transcribe-flash-policy.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::granite {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, GraniteModel>);
static_assert(std::is_base_of_v<transcribe_session, GraniteSession>);

GraniteSession::~GraniteSession() {
    kv.free();
    kv_batch.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

GraniteModel::~GraniteModel() {
    if (bn_fused_ctx != nullptr) {
        ggml_free(bn_fused_ctx);
        bn_fused_ctx = nullptr;
    }
    if (bn_fused_buffer != nullptr) {
        safe_buffer_free(bn_fused_buffer);
        bn_fused_buffer = nullptr;
    }
    packed_gate_up.free();
    if (ctx_meta != nullptr) {
        ggml_free(ctx_meta);
        ctx_meta = nullptr;
    }
    if (backend_buffer != nullptr) {
        safe_buffer_free(backend_buffer);
        backend_buffer = nullptr;
    }
    for (auto it = plan.scheduler_list.rbegin(); it != plan.scheduler_list.rend(); ++it) {
        safe_backend_free(*it);
    }
    plan.scheduler_list.clear();
    plan.primary      = nullptr;
    plan.primary_kind = transcribe::BackendKind::Unknown;
}

namespace {

constexpr const char k_default_variant[] = "granite-speech";
constexpr float      kBnEps              = 1e-5f;

// Input-length contract (see docs/input-limits.md). Granite is a
// hard-context-cap family: audio tokens + prompt + generation share the LLM
// decoder's context window (dec_max_position_embeddings, typically 4096).
// Over-length input is rejected up front with TRANSCRIBE_ERR_INPUT_TOO_LONG
// rather than silently aliasing RoPE past the trained range.

// Generation budget reserved per run. Also the KV grow-to-fit step budget,
// so an accepted clip always has room for up to this many output tokens.
constexpr int k_gen_budget = 256;

// Effective decoder context ceiling, in tokens: the model's trained maximum,
// optionally lowered — never raised — by the caller's session n_ctx knob.
int granite_context_ceiling(int32_t n_ctx_knob, const GraniteHParams & hp) {
    int ceiling = hp.dec_max_position_embeddings;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Q-Former audio tokens per encoder window: window_size / downsample_rate
// (== num_queries, 15/5 = 3 for shipped granite variants).
int granite_num_queries(const GraniteHParams & hp) {
    if (hp.downsample_rate > 0 && hp.window_size > 0) {
        return hp.window_size / hp.downsample_rate;
    }
    return 3;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose
// audio tokens, a representative prompt, and the generation reserve still fit
// the context ceiling. This is the input bound the gate enforces; transcripts
// of long-but-fitting audio may still truncate (transcribe_was_truncated)
// because the per-run output is bounded by k_gen_budget. Returns 0 ("unknown
// / unbounded") if the rate constants are missing, so a misconfigured model
// is never advertised with a wrong finite number.
int64_t granite_max_audio_ms(const GraniteHParams & hp) {
    const int num_queries = granite_num_queries(hp);
    if (num_queries <= 0 || hp.window_size <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0 ||
        hp.dec_max_position_embeddings <= 0) {
        return 0;
    }
    // Representative non-audio prompt overhead (chat affixes); advisory.
    constexpr int k_prompt_overhead = 64;
    const int     max_audio_tokens  = hp.dec_max_position_embeddings - k_prompt_overhead - k_gen_budget;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    // Invert the encoder/projector rate:
    //   tokens = nblocks * num_queries ; t_enc <= nblocks * window_size
    //   mel_frames = t_enc * 2 ; ms = mel_frames * hop * 1000 / sample_rate
    const int64_t nblocks = max_audio_tokens / num_queries;
    const int64_t t_enc   = nblocks * hp.window_size;
    const int64_t frames  = t_enc * 2;
    return frames * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
}

// Pre-fuse BatchNorm scale/bias per encoder block (mirrors parakeet's
// fuse_batch_norm): scale = gamma / sqrt(var + eps), bias = beta - mean * scale.
transcribe_status fuse_batch_norm(GraniteModel & m) {
    const size_t n_blocks = m.weights.enc_blocks.size();
    if (n_blocks == 0) {
        return TRANSCRIBE_OK;
    }

    const int64_t inner        = static_cast<int64_t>(m.hparams.enc_hidden) * m.hparams.enc_conv_expansion;
    const size_t  tensor_bytes = static_cast<size_t>(inner) * sizeof(float);

    const size_t     ctx_size = n_blocks * 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params   = { ctx_size, nullptr, /*no_alloc=*/true };
    m.bn_fused_ctx            = ggml_init(params);
    if (m.bn_fused_ctx == nullptr) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite: BatchNorm-fusion context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b              = m.weights.enc_blocks[i];
        b.conv_bn_fused_scale = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, inner);
        b.conv_bn_fused_bias  = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, inner);
    }

    // The plan's scheduler_list always has CPU last as fallback. We
    // could pin to the primary (Metal/Vulkan) but the fused buffers
    // are tiny and live the whole life of the model — putting them on
    // CPU keeps GPU memory free for KV cache + activations.
    m.bn_fused_buffer = ggml_backend_alloc_ctx_tensors(m.bn_fused_ctx, m.plan.scheduler_list.back());
    if (m.bn_fused_buffer == nullptr) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite: BatchNorm-fusion buffer allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    std::vector<float> bn_w(inner), bn_b(inner), rm(inner), rv(inner);
    std::vector<float> fused_s(inner), fused_b(inner);
    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b = m.weights.enc_blocks[i];
        ggml_backend_tensor_get(b.conv_bn_w, bn_w.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_b, bn_b.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_mean, rm.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_var, rv.data(), 0, tensor_bytes);
        for (int64_t c = 0; c < inner; ++c) {
            const float s = bn_w[c] / std::sqrt(rv[c] + kBnEps);
            fused_s[c]    = s;
            fused_b[c]    = bn_b[c] - rm[c] * s;
        }
        ggml_backend_tensor_set(b.conv_bn_fused_scale, fused_s.data(), 0, tensor_bytes);
        ggml_backend_tensor_set(b.conv_bn_fused_bias, fused_b.data(), 0, tensor_bytes);
    }

    return TRANSCRIBE_OK;
}

// Resolve chat-template pieces across both templates (bare-USER/ASSISTANT for
// 1b/2b, granite-4 system-role for -plus). audio exists on every variant;
// start_of_role / end_of_role only on -plus, so they may be absent. Hard-fails
// for the must-haves (audio, end_of_text, pad) so vocab drift surfaces at load.
transcribe_status resolve_chat_tokens(const transcribe::Tokenizer & tok, ChatTokens & out) {
    // Must-have on every granite variant.
    const struct {
        const char * piece;
        int32_t *    slot;
    } required[] = {
        { "<|audio|>",       &out.audio       },
        { "<|end_of_text|>", &out.end_of_text },
        { "<|pad|>",         &out.pad         },
    };

    for (const auto & p : required) {
        const int id = tok.find(p.piece);
        if (id < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: tokenizer missing required piece \"%s\"", p.piece);
            return TRANSCRIBE_ERR_GGUF;
        }
        *p.slot = id;
    }
    // Optional: present in the -plus vocab, absent in 1b/2b. We resolve
    // anyway so the plus prompt builder can use them without re-looking.
    out.start_of_role = tok.find("<|start_of_role|>");  // may be -1
    out.end_of_role   = tok.find("<|end_of_role|>");    // may be -1
    return TRANSCRIBE_OK;
}

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<GraniteModel>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = loader.variant().empty() ? k_default_variant : loader.variant();
    m->backend.clear();

    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (const transcribe_status st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }

    // Only -plus exposes word timestamps; lower the WORD family default to NONE
    // when the GGUF does not advertise stt.capability.word_timestamps.
    {
        bool word_ts = false;
        if (const transcribe_status st =
                read_optional_bool_kv(loader.gguf(), "stt.capability.word_timestamps", "granite", false, word_ts);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (!word_ts) {
            m->caps.max_timestamp_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        }
    }

    if (const transcribe_status st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Tokenizer (byte-level BPE; loader handles the "gpt2" model tag).
    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    // Chat template (required at run time but the runtime accepts an
    // empty string today and the prompt builder reports a clear error
    // later). We read it eagerly so a converter regression that drops
    // the template surfaces at load.
    (void) read_optional_string_kv(loader.gguf(), "tokenizer.chat_template", "granite", "", m->chat_template);

    if (const transcribe_status st = resolve_chat_tokens(m->tok, m->chat_tokens); st != TRANSCRIBE_OK) {
        return st;
    }

    // Hparams.
    if (const transcribe_status st = read_granite_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window
    // and frontend rate are known (apply_family_invariants ran before the
    // hparams were read, so it could not set this).
    m->caps.max_audio_ms = granite_max_audio_ms(m->hparams);

    // Basis for the session-level limits query (transcribe_session_get_limits):
    // the same rate constants granite_max_audio_ms uses, kept so the effective
    // limit can be recomputed at a lowered session n_ctx. The guard mirrors
    // granite_max_audio_ms's: every rate field must be present, or we leave the
    // basis unset (zero model_max_ctx => the query reports "unbounded").
    {
        const int num_queries = granite_num_queries(m->hparams);
        if (num_queries > 0 && m->hparams.window_size > 0 && m->hparams.fe_hop_length > 0 &&
            m->hparams.fe_sample_rate > 0 && m->hparams.dec_max_position_embeddings > 0) {
            m->limits.has_context_cap    = true;
            m->limits.model_max_ctx      = m->hparams.dec_max_position_embeddings;
            m->limits.prompt_overhead    = 64;  // match granite_max_audio_ms's k_prompt_overhead
            m->limits.gen_reserve        = k_gen_budget;
            // ms per audio token: granite emits num_queries tokens per
            // window_size encoder frames; t_enc = mel_frames/2;
            // mel_frames = ms*sr/(hop*1000). Inverting granite_max_audio_ms's
            // forward rate gives ms-per-audio-token below.
            m->limits.ms_per_audio_token = (static_cast<double>(m->hparams.window_size) / num_queries) * 2.0 *
                                           m->hparams.fe_hop_length * 1000.0 / m->hparams.fe_sample_rate;
            m->limits.kv_elems_per_ctx_token =
                (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
        }
    }

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();
    // The Tokenizer class doesn't track pad; chat_tokens.pad was
    // resolved above by direct piece lookup.
    m->hparams.pad_token_id = m->chat_tokens.pad;

    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Mel frontend: configured from the granite KV.
    {
        transcribe::MelConfig cfg{};
        cfg.sample_rate  = m->hparams.fe_sample_rate;
        cfg.num_mels     = m->hparams.fe_num_mels;
        cfg.n_fft        = m->hparams.fe_n_fft;
        cfg.win_length   = m->hparams.fe_win_length;
        cfg.hop_length   = m->hparams.fe_hop_length;
        cfg.pre_emphasis = m->hparams.fe_pre_emphasis;
        cfg.f_min        = m->hparams.fe_f_min;
        cfg.f_max        = m->hparams.fe_f_max;
        cfg.pad_mode     = m->hparams.fe_pad_mode;
        cfg.window_type  = m->hparams.fe_window;     // "hann_periodic"
        cfg.normalize    = m->hparams.fe_normalize;  // "none"

        // Frontend buffers baked by the converter: librosa htk mel
        // filterbank + periodic Hann window. We pick them up here so
        // the frontend uses bit-identical values.
        {
            using R               = transcribe::load_common::ReadF32Result;
            const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
            const auto   fb_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "granite", cfg.filterbank);
            if (fb_rc != R::Ok && fb_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const size_t win_elems = static_cast<size_t>(cfg.win_length);
            const auto   win_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.window", win_elems, "granite", cfg.window);
            if (win_rc != R::Ok && win_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
        }

        m->mel.emplace(cfg);
    }

    // Reopen with no_alloc to build the tensor catalog.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = build_granite_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Backend plan.
    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "granite", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "granite");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // BatchNorm fusion. Computed once at load using the streamed-in
    // gamma/beta + running_mean/var; the encoder graph references the
    // fused [inner_dim] scale/bias tensors per block.
    if (const transcribe_status st = fuse_batch_norm(*m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Gate+Up fusion: shared causal_lm packer. Drops one FFN mul_mat per
    // block and lets the graph use ggml_swiglu(gate_up) instead of
    // explicit silu(gate)*up.
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "granite")) {
            m->packed_gate_up.free();
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    m->t_load_us = ggml_time_us() - t_load_start;
    *out_model   = m.release();
    return TRANSCRIBE_OK;
}

transcribe_status init_context(transcribe_model *                model,
                               const transcribe_session_params * params,
                               transcribe_session **             out_ctx) {
    if (model->arch != &arch) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto cc       = std::make_unique<GraniteSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    // Encoder uses manual mul_mat + soft_max (no flash) because the
    // Shaw bias requires a per-(head, block) additive term that
    // flash_attn_ext doesn't broadcast cleanly yet.
    cc->encoder_use_flash = false;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Map a BCP-47 language code or English name to the language name the
// granite-speech instruction expects. Returns nullptr for unsupported.
// granite-speech base AR variants advertise fr/de/es/pt/ja plus translate-only
// targets it/zh; -plus is ASR-only, so this helper is not used for it.
static const char * granite_target_language_name(const char * code_or_name) {
    if (code_or_name == nullptr || *code_or_name == '\0') {
        return nullptr;
    }
    std::string s = code_or_name;
    for (auto & c : s) {
        c = static_cast<char>(std::tolower(c));
    }
    if (s == "de" || s == "ger" || s == "deu" || s == "german") {
        return "German";
    }
    if (s == "fr" || s == "fra" || s == "fre" || s == "french") {
        return "French";
    }
    if (s == "es" || s == "spa" || s == "spanish") {
        return "Spanish";
    }
    if (s == "pt" || s == "por" || s == "portuguese") {
        return "Portuguese";
    }
    if (s == "ja" || s == "jpn" || s == "japanese") {
        return "Japanese";
    }
    if (s == "it" || s == "ita" || s == "italian") {
        return "Italian";
    }
    if (s == "zh" || s == "zh-cn" || s == "cmn" || s == "mandarin" || s == "chinese") {
        return "Mandarin";
    }
    if (s == "en" || s == "eng" || s == "english") {
        return "English";
    }
    return nullptr;
}

// Build the prompt prefix/suffix token-id lists from the shared run params and
// model variant (the audio tokens splice in between). Single source of truth
// for run() and run_batch().
static transcribe_status build_granite_affixes(GraniteModel *                cm,
                                               const transcribe_run_params * params,
                                               std::vector<int32_t> &        prefix_ids,
                                               std::vector<int32_t> &        suffix_ids) {
    std::string instruction;
    if (cm->hparams.variant == "granite-speech-4.1-2b") {
        instruction = "transcribe the speech with proper punctuation and capitalization.";
    } else if (cm->hparams.variant == "granite-speech-4.1-2b-plus") {
        instruction = " can you transcribe the speech into a written format?";
    } else {
        instruction = "can you transcribe the speech into a written format?";
    }
    if (params != nullptr) {
        if (params->task == TRANSCRIBE_TASK_TRANSLATE) {
            if (params->target_language == nullptr || params->target_language[0] == '\0') {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: translate task requires --target-language");
                return TRANSCRIBE_ERR_INVALID_ARG;
            }
            const char * lang_name = granite_target_language_name(params->target_language);
            if (lang_name == nullptr) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: target_language '%s' is not advertised",
                        params->target_language);
                return TRANSCRIBE_ERR_INVALID_ARG;
            }
            instruction = std::string("can you translate the speech into ") + lang_name + "?";
        } else if (params->timestamps == TRANSCRIBE_TIMESTAMPS_WORD) {
            // -plus only (1b/2b advertise NONE, gated out upstream). AUTO does
            // NOT request timestamps. IBM's verbatim prompt; the model emits
            // per-word "[T:N]" centisecond markers (parsed in run()).
            instruction =
                " Timestamps: Transcribe the speech. After each word, add a timestamp tag "
                "showing the end time in centiseconds, e.g. hello [T:45] world [T:82]";
        }
    }

    const bool use_granite4_chat = cm->chat_template.find("<|start_of_role|>") != std::string::npos &&
                                   cm->chat_tokens.start_of_role >= 0 && cm->chat_tokens.end_of_role >= 0;
    if (use_granite4_chat) {
        const char *         system_content = (cm->hparams.variant == "granite-speech-4.1-2b-plus") ?
                                                  "Knowledge Cutoff Date: April 2024.\n"
                                                  "Today's Date: December 19, 2024.\n"
                                                  "You are Granite, developed by IBM. You are a helpful AI assistant" :
                                                  "You are a helpful assistant. Please ensure responses "
                                                  "are professional, accurate, and safe.";
        std::vector<int32_t> text_a, text_b;
        if (const transcribe_status st = cm->tok.encode("system", text_a); st != TRANSCRIBE_OK) {
            return st;
        }
        if (const transcribe_status st = cm->tok.encode(system_content, text_b); st != TRANSCRIBE_OK) {
            return st;
        }
        prefix_ids.push_back(cm->chat_tokens.start_of_role);
        prefix_ids.insert(prefix_ids.end(), text_a.begin(), text_a.end());
        prefix_ids.push_back(cm->chat_tokens.end_of_role);
        prefix_ids.insert(prefix_ids.end(), text_b.begin(), text_b.end());
        prefix_ids.push_back(cm->chat_tokens.end_of_text);

        std::vector<int32_t> nl_ids, user_ids;
        if (const transcribe_status st = cm->tok.encode("\n", nl_ids); st != TRANSCRIBE_OK) {
            return st;
        }
        if (const transcribe_status st = cm->tok.encode("user", user_ids); st != TRANSCRIBE_OK) {
            return st;
        }
        prefix_ids.insert(prefix_ids.end(), nl_ids.begin(), nl_ids.end());
        prefix_ids.push_back(cm->chat_tokens.start_of_role);
        prefix_ids.insert(prefix_ids.end(), user_ids.begin(), user_ids.end());
        prefix_ids.push_back(cm->chat_tokens.end_of_role);

        if (const transcribe_status st = cm->tok.encode(instruction, suffix_ids); st != TRANSCRIBE_OK) {
            return st;
        }
        suffix_ids.push_back(cm->chat_tokens.end_of_text);
        std::vector<int32_t> nl_ids2;
        if (const transcribe_status st = cm->tok.encode("\n", nl_ids2); st != TRANSCRIBE_OK) {
            return st;
        }
        suffix_ids.insert(suffix_ids.end(), nl_ids2.begin(), nl_ids2.end());
        suffix_ids.push_back(cm->chat_tokens.start_of_role);
        std::vector<int32_t> asst_ids;
        if (const transcribe_status st = cm->tok.encode("assistant", asst_ids); st != TRANSCRIBE_OK) {
            return st;
        }
        suffix_ids.insert(suffix_ids.end(), asst_ids.begin(), asst_ids.end());
        suffix_ids.push_back(cm->chat_tokens.end_of_role);
    } else {
        const std::string prefix_text = "USER: ";
        const std::string suffix_text = instruction + "\n ASSISTANT:";
        if (const transcribe_status st = cm->tok.encode(prefix_text, prefix_ids); st != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: tokenize(prefix) failed");
            return st;
        }
        if (const transcribe_status st = cm->tok.encode(suffix_text, suffix_ids); st != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: tokenize(suffix) failed");
            return st;
        }
    }
    return TRANSCRIBE_OK;
}

// Parse -plus inline "<word> [T:N]" markers (N = preceding word's end in
// centiseconds, mod 1000 / 10 s rollover) into structured words + one segment.
// "_" items are silence placeholders (advance the clock, not emitted). Returns
// the clean transcript; leaves the vectors empty on marker-free output.
static std::string parse_granite_word_timestamps(const std::string &                             raw,
                                                 int64_t                                         audio_ms,
                                                 std::vector<transcribe_session::WordEntry> &    out_words,
                                                 std::vector<transcribe_session::SegmentEntry> & out_segments) {
    auto is_space = [](char c) {
        return c == ' ' || c == '\t' || c == '\n' || c == '\r';
    };
    auto trim = [&](const std::string & s) -> std::string {
        size_t a = 0, b = s.size();
        while (a < b && is_space(s[a])) {
            ++a;
        }
        while (b > a && is_space(s[b - 1])) {
            --b;
        }
        return s.substr(a, b - a);
    };

    std::string pending;
    int         rollover    = 0;   // count of 10 s (1000 cs) wraps seen so far
    long        last_raw    = -1;  // previous raw centisecond value (pre-unwrap)
    int64_t     prev_end_ms = 0;   // end of the previous marker (word or "_")

    const size_t n = raw.size();
    size_t       i = 0;
    while (i < n) {
        // Recognize a "[T:<digits>]" marker (tolerate spaces around the digits).
        if (raw[i] == '[' && i + 2 < n && raw[i + 1] == 'T' && raw[i + 2] == ':') {
            size_t j = i + 3;
            while (j < n && is_space(raw[j])) {
                ++j;
            }
            long val       = 0;
            bool has_digit = false;
            while (j < n && raw[j] >= '0' && raw[j] <= '9') {
                val       = val * 10 + (raw[j] - '0');
                has_digit = true;
                ++j;
            }
            while (j < n && is_space(raw[j])) {
                ++j;
            }
            if (has_digit && j < n && raw[j] == ']') {
                ++j;  // consume ']'
                if (last_raw >= 0 && val < last_raw) {
                    ++rollover;
                }
                last_raw                 = val;
                const int64_t     end_ms = (static_cast<int64_t>(val) + static_cast<int64_t>(rollover) * 1000) * 10;
                const std::string w      = trim(pending);
                if (!w.empty() && w != "_") {
                    transcribe_session::WordEntry we;
                    we.text        = w;
                    we.t0_ms       = prev_end_ms;
                    we.t1_ms       = end_ms;
                    we.seg_index   = 0;
                    we.first_token = 0;
                    we.n_tokens    = 0;
                    out_words.push_back(std::move(we));
                }
                prev_end_ms = end_ms;
                pending.clear();
                i = j;
                continue;
            }
            // Not a well-formed marker — fall through and treat '[' as text.
        }
        pending.push_back(raw[i]);
        ++i;
    }

    // A trailing word with no marker keeps its text but has no model end time;
    // bound it by the audio duration.
    if (const std::string w = trim(pending); !w.empty() && w != "_") {
        transcribe_session::WordEntry we;
        we.text        = w;
        we.t0_ms       = prev_end_ms;
        we.t1_ms       = audio_ms > prev_end_ms ? audio_ms : prev_end_ms;
        we.seg_index   = 0;
        we.first_token = 0;
        we.n_tokens    = 0;
        out_words.push_back(std::move(we));
    }

    std::string clean;
    for (size_t k = 0; k < out_words.size(); ++k) {
        if (k != 0) {
            clean.push_back(' ');
        }
        clean += out_words[k].text;
    }

    if (!out_words.empty()) {
        transcribe_session::SegmentEntry seg;
        seg.text        = clean;
        seg.t0_ms       = out_words.front().t0_ms;
        seg.t1_ms       = out_words.back().t1_ms;
        seg.first_word  = 0;
        seg.n_words     = static_cast<int>(out_words.size());
        seg.first_token = 0;
        seg.n_tokens    = 0;
        out_segments.push_back(std::move(seg));
    }

    return clean;
}

transcribe_status run(transcribe_session *          ctx_base,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (ctx_base == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<GraniteSession *>(ctx_base);
    auto * cm = static_cast<GraniteModel *>(cc->model);

    transcribe::debug::init();

    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Mel + 2-frame stack (host side).
    const int64_t t_mel_start = ggml_time_us();
    int           t_enc       = 0;
    if (const transcribe_status mst =
            compute_mel_encoder_input(*cm->mel, pcm, n_samples, cc->n_threads, cc->mel_buf, t_enc);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: mel/stack failed (%s)", transcribe_status_string(mst));
        return mst;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    // Dump enc.mel.in in the reference's [T_enc, input_dim] row-major
    // shape. The graph input is the same data, so this is the natural
    // comparison point on the host.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { t_enc, cm->hparams.enc_input_dim };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "encoder");
    }

    // Reset compute state.
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        // 16 conformer blocks * many ops. 32 MB is generous headroom.
        ip.mem_size     = 32 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // Build encoder graph.
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, t_enc, cc->encoder_use_flash);
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Encoder operates at T_enc throughout (no host-side padding of
    // the mel buffer required). The Shaw-attention helper pads to
    // T_pad internally using the eb.zero_pad graph input.

    // Scheduler.
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 32768, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload inputs.
    // mel_in: contiguous [T_enc, input_dim] row-major matches ggml ne
    // [input_dim, T_enc] (ne[0]=input_dim is innermost).
    ggml_backend_tensor_set(eb.mel_in, cc->mel_buf.data(), 0, cc->mel_buf.size() * sizeof(float));

    // Shaw attention_dists. Row-major over (c, r) with int32 indices.
    std::vector<int32_t> dists = precompute_attention_dists(cm->hparams.enc_context_size, cm->hparams.enc_max_pos_emb);
    ggml_backend_tensor_set(eb.attention_dists, dists.data(), 0, dists.size() * sizeof(int32_t));

    // last_block_mask: [context_size, context_size, n_blocks_local].
    // All zeros except the last slice when t_enc is not a multiple of
    // context_size.
    {
        const int          ctx_size = cm->hparams.enc_context_size;
        const size_t       plane    = static_cast<size_t>(ctx_size) * ctx_size;
        std::vector<float> mask(plane * eb.n_blocks_local, 0.0f);
        const int          rem = eb.last_block_rem;
        if (rem > 0 && rem < ctx_size) {
            std::vector<float> last = precompute_last_block_mask(ctx_size, rem);
            // Last slice index = n_blocks_local - 1. ne[2] is the slice
            // axis (row-major plane order). Offset = (n_blocks_local-1)*plane.
            std::memcpy(mask.data() + plane * (eb.n_blocks_local - 1), last.data(), plane * sizeof(float));
        }
        ggml_backend_tensor_set(eb.last_block_mask, mask.data(), 0, mask.size() * sizeof(float));
    }

    // zero_pad: only present when t_enc is not aligned to context_size.
    if (eb.zero_pad != nullptr) {
        const size_t       n_elems = static_cast<size_t>(eb.zero_pad->ne[0]) * eb.zero_pad->ne[1];
        std::vector<float> zeros(n_elems, 0.0f);
        ggml_backend_tensor_set(eb.zero_pad, zeros.data(), 0, zeros.size() * sizeof(float));
    }

    // Thread count.
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);

    // Compute.
    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: encoder graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // Dump intermediates.
    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.input_linear.out", eb.dumps.input_linear_out, "encoder");
    try_dump("enc.block.0.out", eb.dumps.block_0_out, "encoder");
    {
        const int mid_idx = cm->hparams.enc_n_layers / 2;
        char      bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", mid_idx);
        try_dump(bname, eb.dumps.block_mid_out, "encoder");
    }
    {
        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", cm->hparams.enc_n_layers - 1);
        try_dump(bname, eb.dumps.block_last_out, "encoder");
    }
    try_dump("enc.out", eb.dumps.out_named, "encoder");

    // Read encoder output to host for the projector pass. The projector
    // uses a separate compute graph so its dumps compare without encoder
    // graph state lingering.
    const int64_t      enc_hidden_runtime = eb.out->ne[0];
    const int64_t      enc_T_runtime      = eb.out->ne[1];
    std::vector<float> enc_host(static_cast<size_t>(enc_hidden_runtime) * enc_T_runtime);
    ggml_backend_tensor_get(eb.out, enc_host.data(), 0, enc_host.size() * sizeof(float));

    // Cat-hidden-layers handling (-plus variant): the -plus checkpoint sets
    // enc.cat_hidden_layers = [3]; the encoder graph channel-concatenates
    // block[K-1].out with the final hidden along dim=0 before returning eb.out,
    // so the projector input dimension widens from 1024 to 2048 automatically.

    // Projector graph.
    ggml_context * proj_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size   = 16 * 1024 * 1024;  // 2 Q-Former layers — small graph
        ip.mem_buffer = nullptr;
        ip.no_alloc   = true;
        proj_ctx      = ggml_init(ip);
        if (proj_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run: projector compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ProjectorBuild pb = build_projector_graph(proj_ctx, cm->weights, cm->hparams, static_cast<int>(enc_T_runtime));
    if (pb.graph == nullptr || pb.out == nullptr) {
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run: projector graph allocation failed — out of memory.");
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload encoder output to the projector's enc_in input.
    ggml_backend_tensor_set(pb.enc_in, enc_host.data(), 0, enc_host.size() * sizeof(float));
    if (pb.enc_pad != nullptr) {
        const size_t       pad_bytes = static_cast<size_t>(pb.enc_pad->ne[0]) * pb.enc_pad->ne[1] * sizeof(float);
        std::vector<float> pad(pad_bytes / sizeof(float), 0.0f);
        ggml_backend_tensor_set(pb.enc_pad, pad.data(), 0, pad_bytes);
    }

    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: projector graph compute failed (%d)", static_cast<int>(gs));
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    try_dump("proj.qformer.out", pb.dumps.qformer_out, "projector");
    try_dump("proj.out", pb.dumps.proj_out, "projector");

    // Read projector output (the audio tokens in LM hidden space).
    cc->n_audio_tokens = pb.n_audio_tokens;
    cc->audio_tokens_host.resize(static_cast<size_t>(pb.out->ne[0]) * pb.out->ne[1]);
    ggml_backend_tensor_get(pb.out, cc->audio_tokens_host.data(), 0, cc->audio_tokens_host.size() * sizeof(float));

    ggml_free(proj_ctx);

    // Decoder prefill pass.
    // Build the prompt as `prefix_text | <|audio|>×n | suffix_text`.
    // Two templates ship today:
    //
    //   1b / 2b  (chat_tokens.start_of_role < 0):
    //     "USER: <|audio|>{instruction}\n ASSISTANT:"
    //   -plus    (chat_tokens.start_of_role >= 0):
    //     granite-4 system-role chat. The system message and assistant
    //     start-of-role are appended via special-token id splices so the
    //     BPE encoder only sees plain text fragments — special tokens
    //     are emitted directly rather than tokenized from their literal
    //     piece strings.
    //
    // Granite-speech instructions. Each variant's published WER on
    // LibriSpeech test-clean was measured with a specific prompt; using
    // a different prompt produces a measurable WER drift (typically
    // 0.02-0.04pp). Match the model card per variant.
    //
    //   granite-4.0-1b-speech            : "can you transcribe the speech into a written format?"
    //   granite-speech-4.1-2b            : "transcribe the speech with proper punctuation and capitalization."
    //   granite-speech-4.1-2b-plus       : " can you transcribe the speech into a written format?"
    //                                       (leading space matters: BPE token IDs differ)
    //                                       PLUS a Granite-specific system prompt (see use_granite4_chat below).
    //   word-timestamps task (-plus)     : IBM's verbatim timestamps prompt; the model emits
    //                                       per-word "[T:N]" centisecond markers (1b/2b: NONE).
    //   translate task                   : "can you translate the speech into <Language>?"
    std::vector<int32_t> prefix_ids;
    std::vector<int32_t> suffix_ids;
    if (const transcribe_status st = build_granite_affixes(cm, params, prefix_ids, suffix_ids); st != TRANSCRIBE_OK) {
        return st;
    }

    const int n_audio_tokens = cc->n_audio_tokens;
    const int prefix_len     = static_cast<int>(prefix_ids.size());
    const int suffix_len     = static_cast<int>(suffix_ids.size());
    const int T_prompt       = prefix_len + n_audio_tokens + suffix_len;

    // Reference quirk: HF replaces audio_token_id with 0 before the
    // embed_tokens lookup (those rows are overwritten by the audio scatter
    // moments later). We mirror the masking so dec.token_emb matches at the
    // audio positions; the forward result is identical either way.
    std::vector<int32_t> input_ids;
    input_ids.reserve(T_prompt);
    input_ids.insert(input_ids.end(), prefix_ids.begin(), prefix_ids.end());
    for (int i = 0; i < n_audio_tokens; ++i) {
        input_ids.push_back(0);
    }
    input_ids.insert(input_ids.end(), suffix_ids.begin(), suffix_ids.end());

    // Input-length gate. The decoder context window is the binding limit:
    // audio tokens + prompt + generation must fit dec_max_position_embeddings
    // (optionally lowered by the caller's n_ctx knob). The audio-token count is
    // fixed by the input length, so reject an over-length clip here, before the
    // autoregressive decode, instead of growing the cache unboundedly and
    // aliasing RoPE past the trained range. Reserving the full generation
    // budget means an accepted clip always has room for a real transcript.
    const int ceiling = granite_context_ceiling(cc->n_ctx, cm->hparams);
    if (T_prompt + k_gen_budget > ceiling) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run: input too long — %d audio + %d prompt tokens leave "
                            "no room for output within the %d-token context (need %d). "
                            "Shorten the audio (see transcribe_capabilities.max_audio_ms) or "
                            "split it into segments.",
                            n_audio_tokens, prefix_len + suffix_len, ceiling, T_prompt + k_gen_budget);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // Size the KV cache dynamically: T_prompt + room for the longest
    // generation we'll emit, clamped to the context ceiling. Matches the
    // HF reference's DynamicCache semantics (grows as needed) without
    // paying for a worst-case pre-alloc. If the existing cache is too small
    // for this prompt, free and re-allocate. Round up to a 256-row bucket
    // so back-to-back runs of similar audio lengths don't keep
    // re-allocating.
    constexpr int kKvBucket    = 256;
    const int     needed_raw   = std::min(T_prompt + k_gen_budget, ceiling);
    const int     needed_n_ctx = ((needed_raw + kKvBucket - 1) / kKvBucket) * kKvBucket;

    if (cc->kv.self_k != nullptr && cc->kv.n_ctx < needed_n_ctx) {
        cc->kv.free();
    }
    if (cc->kv.self_k == nullptr) {
        ggml_type kv_t = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_t = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv, cm->plan.primary, needed_n_ctx, cm->hparams.dec_n_kv_heads,
                                            cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kv_t)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run: KV cache allocation failed (n_ctx=%d, "
                                "%d kv-heads x %d head-dim x %d layers) — out of memory. "
                                "Lower transcribe_session_params.n_ctx or shorten the audio.",
                                needed_n_ctx, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    }
    // After the gate + dynamic sizing this should be unreachable; keep as a
    // belt-and-braces invariant check.
    if (T_prompt > cc->kv.n_ctx) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: T_prompt=%d exceeds kv.n_ctx=%d", T_prompt,
                            cc->kv.n_ctx);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // Prefill graph in its own compute context.
    ggml_context * dec_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size   = 64 * 1024 * 1024;  // 40 layers — generous headroom
        ip.mem_buffer = nullptr;
        ip.no_alloc   = true;
        dec_ctx       = ggml_init(ip);
        if (dec_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run: decoder compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    PrefillBuild dec = build_prefill_graph(dec_ctx, cm->weights, cm->hparams, cc->kv, T_prompt, n_audio_tokens,
                                           prefix_len, suffix_len, cc->decoder_use_flash, /*slice_last=*/false);
    if (dec.graph == nullptr || dec.out == nullptr) {
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, dec.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run: decoder prefill graph allocation failed — "
                            "out of memory.");
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload decoder inputs.
    ggml_backend_tensor_set(dec.input_ids_in, input_ids.data(), 0, input_ids.size() * sizeof(int32_t));
    ggml_backend_tensor_set(dec.audio_feats_in, cc->audio_tokens_host.data(), 0,
                            cc->audio_tokens_host.size() * sizeof(float));

    {
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(dec.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }

    // Causal mask in F16: zero on/below diagonal, -inf above. f16 -inf
    // is the bit pattern 0xFC00.
    {
        const uint16_t        f16_zero    = 0x0000;
        const uint16_t        f16_neg_inf = 0xFC00;
        std::vector<uint16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, f16_zero);
        for (int q = 0; q < T_prompt; ++q) {
            for (int k = q + 1; k < T_prompt; ++k) {
                mask[static_cast<size_t>(q) * T_prompt + k] = f16_neg_inf;
            }
        }
        ggml_backend_tensor_set(dec.mask_in, mask.data(), 0, mask.size() * sizeof(uint16_t));
    }

    const int64_t t_dec_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, dec.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: decoder prefill compute failed (%d)", static_cast<int>(gs));
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_decode_us = ggml_time_us() - t_dec_start;

    cc->kv.n    = T_prompt;
    cc->kv.head = T_prompt;

    try_dump("dec.token_emb", dec.dumps.token_emb, "decoder");
    try_dump("dec.audio_injected", dec.dumps.audio_injected, "decoder");
    try_dump("dec.block.0.out", dec.dumps.block_0_out, "decoder");
    if (dec.dumps.block_mid_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers / 2);
        try_dump(nm, dec.dumps.block_mid_out, "decoder");
    }
    if (dec.dumps.block_last_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers - 1);
        try_dump(nm, dec.dumps.block_last_out, "decoder");
    }
    try_dump("dec.out_before_head", dec.dumps.out_before_head, "decoder");
    try_dump("dec.logits_raw", dec.dumps.logits_raw, "decoder");

    // Read final-position logits to compute the first generated token.
    std::vector<float> last_logits(cm->hparams.dec_vocab_size);
    ggml_backend_tensor_get(dec.out, last_logits.data(), 0, last_logits.size() * sizeof(float));
    ggml_free(dec_ctx);

    auto argmax_logits = [](const std::vector<float> & v) -> int32_t {
        int32_t best   = 0;
        float   best_v = v[0];
        for (int32_t i = 1; i < static_cast<int32_t>(v.size()); ++i) {
            if (v[i] > best_v) {
                best_v = v[i];
                best   = i;
            }
        }
        return best;
    };

    int32_t next_id = argmax_logits(last_logits);

    // Greedy step loop.
    // Build one reusable step graph sized for the whole run. The
    // n_ctx of the KV cache bounds the max generation length we can
    // attend over.
    const int max_n_kv  = cc->kv.n_ctx;
    // Bound generation by the step budget, the allocated cache, AND the
    // context ceiling (the gate guarantees ceiling - T_prompt >= k_gen_budget,
    // so for in-spec input this stays k_gen_budget and decode is unchanged).
    const int max_steps = std::min({ k_gen_budget, max_n_kv - T_prompt, ceiling - T_prompt });

    ggml_context * step_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size   = 32 * 1024 * 1024;
        ip.mem_buffer = nullptr;
        ip.no_alloc   = true;
        step_ctx      = ggml_init(ip);
    }
    StepBuild step = build_step_graph(step_ctx, cm->weights, cm->hparams, cc->kv, max_n_kv, cc->decoder_use_flash);
    if (step.graph == nullptr) {
        ggml_free(step_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, step.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run: decode step graph allocation failed — "
                            "out of memory.");
        ggml_free(step_ctx);
        return TRANSCRIBE_ERR_OOM;
    }

    std::vector<int32_t> gen_ids;
    gen_ids.reserve(max_steps);

    const int32_t         eos_id = cm->hparams.eos_token_id;
    // Step-loop mask sized for max_n_kv. We pre-fill -inf and let
    // valid positions get zeroed per step.
    std::vector<uint16_t> step_mask(max_n_kv, 0xFC00);

    for (int step_i = 0; step_i < max_steps; ++step_i) {
        if (next_id == eos_id) {
            break;
        }
        gen_ids.push_back(next_id);

        const int32_t pos      = T_prompt + step_i;  // RoPE position
        const int64_t kv_idx   = pos;                // KV write row
        const int32_t input_id = next_id;

        // Zero mask up through `pos` (inclusive); the new K/V row sits
        // at index `pos`, so attention can see it.
        std::fill(step_mask.begin(), step_mask.end(), static_cast<uint16_t>(0xFC00));
        for (int i = 0; i <= pos; ++i) {
            step_mask[i] = 0x0000;
        }

        ggml_backend_tensor_set(step.input_id_in, &input_id, 0, sizeof(int32_t));
        ggml_backend_tensor_set(step.position_in, &pos, 0, sizeof(int32_t));
        ggml_backend_tensor_set(step.kv_idx_in, &kv_idx, 0, sizeof(int64_t));
        ggml_backend_tensor_set(step.mask_in, step_mask.data(), 0, step_mask.size() * sizeof(uint16_t));

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, step.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite run: step compute failed (%d)", static_cast<int>(gs));
            ggml_free(step_ctx);
            return TRANSCRIBE_ERR_GGUF;
        }

        int32_t amax = 0;
        ggml_backend_tensor_get(step.out, &amax, 0, sizeof(int32_t));
        cc->kv.n    = pos + 1;
        cc->kv.head = pos + 1;
        next_id     = amax;
    }

    ggml_free(step_ctx);

    // The decode stopped either at EOS (complete) or at the generation
    // budget / context ceiling (truncated). Surface the latter via
    // transcribe_was_truncated() and a WARN rather than handing back a
    // silently shortened transcript.
    if (next_id != eos_id) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "granite run: output truncated at %d tokens — decode reached the "
                            "generation budget before end-of-stream; the transcript may be "
                            "incomplete.",
                            static_cast<int>(gen_ids.size()));
    }

    // Detokenize.
    const std::string raw_text = cm->tok.decode(gen_ids.data(), static_cast<int>(gen_ids.size()));
    const int64_t audio_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);

    cc->has_result = true;
    // -plus word timestamps: parse "[T:N]" markers into words, else plain text.
    if (params != nullptr && params->timestamps == TRANSCRIBE_TIMESTAMPS_WORD) {
        cc->full_text = parse_granite_word_timestamps(raw_text, audio_ms, cc->words, cc->segments);
        if (!cc->words.empty()) {
            cc->result_kind = TRANSCRIBE_TIMESTAMPS_WORD;
        }
    }
    if (cc->words.empty()) {
        cc->full_text   = raw_text;
        cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        transcribe_session::SegmentEntry seg{};
        seg.text  = cc->full_text;
        seg.t0_ms = 0;
        seg.t1_ms = audio_ms;
        cc->segments.push_back(std::move(seg));
    }

    // Output truncation (decode hit the generation budget / context ceiling
    // before EOS) is a hard status, not a silent success: surface it so the
    // caller can distinguish a complete transcript from one cut short. The
    // partial transcript is still attached above for inspection.
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

// Offline batched decode (transcribe_run_batch). Serial mel + Conformer
// encoder + Q-Former projector per utterance produce each one's audio embedding
// [hidden, n_audio_tokens]; then prefill + step are batched via granite's
// batched block builders. Same recipe as the causal_lm families, with
// Granite-specific block math.

transcribe_status reset_ctx_g(GraniteSession * cc, int mb) {
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    ggml_init_params ip{};
    ip.mem_size     = static_cast<size_t>(mb) * 1024 * 1024;
    ip.mem_buffer   = nullptr;
    ip.no_alloc     = true;
    cc->compute_ctx = ggml_init(ip);
    return cc->compute_ctx != nullptr ? TRANSCRIBE_OK : TRANSCRIBE_ERR_GGUF;
}

void apply_threads_g(GraniteSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

// encoder + projector for one utterance from a PRECOMPUTED mel buffer →
// [hidden, n_audio]. The encoder runs PER-UTTERANCE (serial) deliberately: the
// 2B conformer is compute-bound, so a batched single-graph encoder is a
// measured wash on Metal and L40S and strictly worse for mixed-length batches
// (it pads to the longest). Only the decode and host-side mel benefit from
// batching/threading.
transcribe_status encode_one(GraniteSession *           cc,
                             GraniteModel *             cm,
                             const std::vector<float> & mel_buf,
                             int                        t_enc,
                             std::vector<float> &       audio_out,
                             int &                      n_audio_out,
                             int64_t &                  enc_us) {
    const auto & hp = cm->hparams;
    if (t_enc <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (reset_ctx_g(cc, 32) != TRANSCRIBE_OK) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite encode_one: compute context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, t_enc, cc->encoder_use_flash);
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 32768, false, true);
        if (cc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite encode_one: encoder graph allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, mel_buf.data(), 0, mel_buf.size() * sizeof(float));
    std::vector<int32_t> dists = precompute_attention_dists(hp.enc_context_size, hp.enc_max_pos_emb);
    ggml_backend_tensor_set(eb.attention_dists, dists.data(), 0, dists.size() * sizeof(int32_t));
    {
        const int          ctx_size = hp.enc_context_size;
        const size_t       plane    = static_cast<size_t>(ctx_size) * ctx_size;
        std::vector<float> mask(plane * eb.n_blocks_local, 0.0f);
        const int          rem = eb.last_block_rem;
        if (rem > 0 && rem < ctx_size) {
            std::vector<float> last = precompute_last_block_mask(ctx_size, rem);
            std::memcpy(mask.data() + plane * (eb.n_blocks_local - 1), last.data(), plane * sizeof(float));
        }
        ggml_backend_tensor_set(eb.last_block_mask, mask.data(), 0, mask.size() * sizeof(float));
    }
    if (eb.zero_pad != nullptr) {
        const size_t       n_elems = static_cast<size_t>(eb.zero_pad->ne[0]) * eb.zero_pad->ne[1];
        std::vector<float> zeros(n_elems, 0.0f);
        ggml_backend_tensor_set(eb.zero_pad, zeros.data(), 0, zeros.size() * sizeof(float));
    }
    apply_threads_g(cc);

    const int64_t t_enc0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc0;

    const int64_t      enc_hidden = eb.out->ne[0];
    const int64_t      enc_T      = eb.out->ne[1];
    std::vector<float> enc_host(static_cast<size_t>(enc_hidden) * enc_T);
    ggml_backend_tensor_get(eb.out, enc_host.data(), 0, enc_host.size() * sizeof(float));

    ggml_context * proj_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size = 16 * 1024 * 1024;
        ip.no_alloc = true;
        proj_ctx    = ggml_init(ip);
        if (proj_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite encode_one: projector compute context allocation "
                                "failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ProjectorBuild pb = build_projector_graph(proj_ctx, cm->weights, hp, static_cast<int>(enc_T));
    if (pb.graph == nullptr || pb.out == nullptr) {
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite encode_one: projector graph allocation failed — "
                            "out of memory.");
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_OOM;
    }
    ggml_backend_tensor_set(pb.enc_in, enc_host.data(), 0, enc_host.size() * sizeof(float));
    if (pb.enc_pad != nullptr) {
        const size_t       n = static_cast<size_t>(pb.enc_pad->ne[0]) * pb.enc_pad->ne[1];
        std::vector<float> pad(n, 0.0f);
        ggml_backend_tensor_set(pb.enc_pad, pad.data(), 0, n * sizeof(float));
    }
    const int64_t t_enc1 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc1;
    n_audio_out = pb.n_audio_tokens;
    audio_out.resize(static_cast<size_t>(pb.out->ne[0]) * pb.out->ne[1]);
    ggml_backend_tensor_get(pb.out, audio_out.data(), 0, audio_out.size() * sizeof(float));
    ggml_free(proj_ctx);
    return TRANSCRIBE_OK;
}

transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status run_batch_serial(GraniteSession *              cc,
                                   const float * const *         pcm,
                                   const int *                   n_samples,
                                   int                           n,
                                   const transcribe_run_params * params) {
    for (int i = 0; i < n; ++i) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        const transcribe_status st = (pcm[i] == nullptr || n_samples[i] <= 0) ? TRANSCRIBE_ERR_INVALID_ARG :
                                                                                run(cc, pcm[i], n_samples[i], params);
        if (st == TRANSCRIBE_OK) {
            cc->batch_results.push_back(cc->capture_result(st));
        } else {
            transcribe_session::ResultSet rs;
            rs.status = st;
            cc->batch_results.push_back(std::move(rs));
        }
    }
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<GraniteSession *>(session);
    auto * cm = static_cast<GraniteModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (!cc->decoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto & hp = cm->hparams;

    // Shared prompt affixes (one run_params across the batch).
    std::vector<int32_t> prefix_ids, suffix_ids;
    if (build_granite_affixes(cm, params, prefix_ids, suffix_ids) != TRANSCRIBE_OK) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int prefix_len = static_cast<int>(prefix_ids.size());

    // Pass 1: per-utterance mel + encoder + projector (serial).
    std::vector<char>                 valid(n, 0);
    // Per-utterance failure status for the result capture below. Defaults to
    // INVALID_ARG (bad pcm / mel / encode); the input-length gate upgrades it
    // to INPUT_TOO_LONG for over-length clips.
    std::vector<transcribe_status>    fail_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<float>>   audio_hosts(n);
    std::vector<int>                  n_audio(n, 0);
    std::vector<std::vector<int32_t>> prompt_ids(n);
    std::vector<int>                  T_prompt(n, 0);
    int64_t                           mel_us = 0, enc_us = 0;

    // Decoder context ceiling for the per-utterance input-length gate. Same
    // value the single-utterance run() enforces; an over-length utterance is
    // rejected with TRANSCRIBE_ERR_INPUT_TOO_LONG rather than growing the cache
    // unboundedly.
    const int ceiling = granite_context_ceiling(cc->n_ctx, cm->hparams);

    // Pass 0: parallel mel + 2-frame stack (host-side, thread-safe).
    std::vector<std::vector<float>> mel_bufs(n);
    std::vector<int>                mel_t_enc(n, 0);
    int                             n_mel_threads = cc->n_threads;
    if (n_mel_threads <= 0) {
        n_mel_threads = transcribe::default_n_threads();
    }
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_mel_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;
        }
        int t = 0;
        if (compute_mel_encoder_input(*cm->mel, pcm[b], n_samples[b],
                                      /*n_threads=*/1, mel_bufs[b], t) == TRANSCRIBE_OK) {
            mel_t_enc[b] = t;
        }
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;

    // Pass 1: per-utterance encoder + projector (serial — see encode_one's
    // note). Only the mel (Pass 0) is parallelized; the decode is batched below.
    for (int b = 0; b < n; ++b) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (mel_t_enc[b] <= 0) {
            continue;
        }
        if (encode_one(cc, cm, mel_bufs[b], mel_t_enc[b], audio_hosts[b], n_audio[b], enc_us) != TRANSCRIBE_OK) {
            continue;
        }
        if (n_audio[b] <= 0) {
            continue;
        }
        prompt_ids[b] = prefix_ids;
        for (int i = 0; i < n_audio[b]; ++i) {
            prompt_ids[b].push_back(0);  // placeholder
        }
        prompt_ids[b].insert(prompt_ids[b].end(), suffix_ids.begin(), suffix_ids.end());
        T_prompt[b] = static_cast<int>(prompt_ids[b].size());

        // Input-length gate, mirroring the single-shot run() gate.
        if (T_prompt[b] + k_gen_budget > ceiling) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run_batch: utterance %d input too long — %d audio + %d "
                                "prompt tokens leave no room for output within the %d-token "
                                "context (need %d). Shorten the audio (see "
                                "transcribe_capabilities.max_audio_ms) or split it.",
                                b, n_audio[b], T_prompt[b] - n_audio[b], ceiling, T_prompt[b] + k_gen_budget);
            fail_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
            continue;
        }
        valid[b] = 1;
    }

    int max_T_prompt = 0, n_audio_max = 0;
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            max_T_prompt = std::max(max_T_prompt, T_prompt[b]);
            n_audio_max  = std::max(n_audio_max, n_audio[b]);
        }
    }
    if (max_T_prompt == 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }
    n_audio_max        = std::max(1, n_audio_max);
    const int max_new  = 256;
    int       max_n_kv = 1024;
    while (max_n_kv < max_T_prompt + max_new) {
        max_n_kv *= 2;
    }
    // Honor the session context cap: clamp the pow2-rounded width to the
    // ceiling (the per-utterance gate guarantees every valid row fits).
    if (max_n_kv > ceiling) {
        max_n_kv = ceiling;
    }

    // Allocate batched KV cache.
    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx != max_n_kv) {
        cc->kv_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_batch, cm->plan.primary, max_n_kv, hp.dec_n_kv_heads,
                                                    hp.dec_head_dim, hp.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run_batch: batched KV cache allocation failed "
                                "(n_ctx=%d, batch=%d) — out of memory.",
                                max_n_kv, n);
            return TRANSCRIBE_ERR_OOM;
        }
        cc->kv_batch_cap   = n;
        cc->kv_batch_n_ctx = max_n_kv;
    } else if (cc->kv_batch.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_batch.buffer, 0);
    }

    // Pass 2: batched prefill.
    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, 0);
    std::vector<std::vector<int32_t>> generated(n);
    const int64_t                     t_pref0 = ggml_time_us();
    {
        if (reset_ctx_g(cc, 64) != TRANSCRIBE_OK) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run_batch: prefill compute context allocation failed "
                                "— out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        PrefillBuildBatched pb = build_prefill_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_batch,
                                                             max_T_prompt, n_audio_max, n, cc->decoder_use_flash);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite run_batch: batched prefill graph allocation failed "
                                "— out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        const int            hidden = hp.dec_hidden;
        std::vector<int32_t> ids(static_cast<size_t>(max_T_prompt) * n, 0);
        // Audio injection by elementwise blend: audio_dense holds each
        // utterance's audio embeds scattered into their prompt positions (zero
        // elsewhere), keep is 0 there and 1 elsewhere. x = x*keep + audio_dense.
        std::vector<float>   audio_dense(static_cast<size_t>(hidden) * max_T_prompt * n, 0.0f);
        std::vector<float>   keep(static_cast<size_t>(max_T_prompt) * n, 1.0f);
        std::vector<int64_t> kidx(static_cast<size_t>(max_T_prompt) * n);
        std::vector<int32_t> lidx(n, 0);
        for (int b = 0; b < n; ++b) {
            const int na = valid[b] ? n_audio[b] : 0;
            const int tp = valid[b] ? T_prompt[b] : 0;
            if (valid[b]) {
                std::memcpy(ids.data() + static_cast<size_t>(b) * max_T_prompt, prompt_ids[b].data(),
                            static_cast<size_t>(tp) * sizeof(int32_t));
                // audio_hosts[b] is [hidden, na] column-major; audio token j lands
                // at prompt position prefix_len+j, flat column b*max_T_prompt+pos.
                for (int j = 0; j < na; ++j) {
                    const size_t dst_col = static_cast<size_t>(b) * max_T_prompt + (prefix_len + j);
                    std::memcpy(audio_dense.data() + dst_col * hidden,
                                audio_hosts[b].data() + static_cast<size_t>(j) * hidden,
                                static_cast<size_t>(hidden) * sizeof(float));
                    keep[dst_col] = 0.0f;
                }
            }
            for (int t = 0; t < max_T_prompt; ++t) {
                kidx[static_cast<size_t>(b) * max_T_prompt + t] = t;
            }
            lidx[b] = valid[b] ? (tp - 1) : 0;
        }
        ggml_backend_tensor_set(pb.input_ids_in, ids.data(), 0, ids.size() * sizeof(int32_t));
        ggml_backend_tensor_set(pb.audio_dense_in, audio_dense.data(), 0, audio_dense.size() * sizeof(float));
        ggml_backend_tensor_set(pb.keep_mask_in, keep.data(), 0, keep.size() * sizeof(float));
        {
            std::vector<int32_t> pos(max_T_prompt);
            for (int t = 0; t < max_T_prompt; ++t) {
                pos[t] = t;
            }
            ggml_backend_tensor_set(pb.positions_in, pos.data(), 0, pos.size() * sizeof(int32_t));
        }
        {
            const uint16_t        f16_zero = 0x0000, f16_ninf = 0xFC00;
            std::vector<uint16_t> mask(static_cast<size_t>(max_T_prompt) * max_T_prompt, f16_zero);
            for (int q = 0; q < max_T_prompt; ++q) {
                for (int k = q + 1; k < max_T_prompt; ++k) {
                    mask[static_cast<size_t>(q) * max_T_prompt + k] = f16_ninf;
                }
            }
            ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(uint16_t));
        }
        ggml_backend_tensor_set(pb.kv_idx_in, kidx.data(), 0, kidx.size() * sizeof(int64_t));
        ggml_backend_tensor_set(pb.last_idx_in, lidx.data(), 0, lidx.size() * sizeof(int32_t));
        apply_threads_g(cc);
        if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        std::vector<int32_t> amax(n, 0);
        ggml_backend_tensor_get(pb.out, amax.data(), 0, amax.size() * sizeof(int32_t));
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            n_past[b]   = T_prompt[b];
            next_tok[b] = amax[b];
            generated[b].push_back(amax[b]);
        }
    }

    const int64_t prefill_us = ggml_time_us() - t_pref0;

    // Pass 3: batched step loop (shared causal_lm driver).
    const int32_t eos_id = cm->hparams.eos_token_id;

    if (reset_ctx_g(cc, 32) != TRANSCRIBE_OK) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run_batch: step compute context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }
    StepBuildBatched sb =
        build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_batch, max_n_kv, n, cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite run_batch: batched step graph allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    transcribe::causal_lm::StepBatchedIO io{};
    io.input_ids = sb.input_ids_in;
    io.positions = sb.position_in;
    io.kv_idx    = sb.kv_idx_in;
    io.mask      = sb.mask_in;
    io.argmax    = sb.out;
    io.graph     = sb.graph;

    transcribe::causal_lm::StepBatchedState step_state;
    step_state.valid    = valid;
    step_state.next_tok = next_tok;
    step_state.n_past   = n_past;

    transcribe::causal_lm::StepLoopStats step_stats;
    std::vector<char>                    truncated;
    if (const transcribe_status st = transcribe::causal_lm::run_batched_step_loop(
            cc, cc->sched, io, n, max_n_kv, eos_id, max_new, step_state, generated, &step_stats, &truncated);
        st != TRANSCRIBE_OK) {
        return st;
    }
    const int64_t step_us = step_stats.step_us;

    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        int total_steps = 0;
        for (int b = 0; b < n; ++b) {
            total_steps = std::max<int>(total_steps, static_cast<int>(generated[b].size()));
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "granite run_batch: n=%d max_n_kv=%d steps=%d max_T_prompt=%d n_audio_max=%d\n"
                "  mel=%.1fms enc+proj=%.1fms (serial x%d)  prefill=%.1fms (batched)  step_loop=%.1fms (%.2fms/step, "
                "batched)",
                n, max_n_kv, total_steps, max_T_prompt, n_audio_max, mel_us / 1000.0, enc_us / 1000.0, n,
                prefill_us / 1000.0, step_us / 1000.0, total_steps > 0 ? step_us / 1000.0 / total_steps : 0.0);
    }

    // Capture results.
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::vector<int32_t> gen = generated[b];
        if (!gen.empty() && gen.back() == eos_id) {
            gen.pop_back();
        }
        const std::string transcript = cm->tok.decode(gen.data(), static_cast<int>(gen.size()));
        const int64_t audio_ms = static_cast<int64_t>(n_samples[b]) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
        transcribe_session::ResultSet rs;
        rs.has_result = true;
        rs.status     = TRANSCRIBE_OK;
        // Same as run(): parse -plus "[T:N]" word markers, else plain-text segment.
        if (params != nullptr && params->timestamps == TRANSCRIBE_TIMESTAMPS_WORD) {
            rs.full_text = parse_granite_word_timestamps(transcript, audio_ms, rs.words, rs.segments);
            if (!rs.words.empty()) {
                rs.result_kind = TRANSCRIBE_TIMESTAMPS_WORD;
            }
        }
        if (rs.words.empty()) {
            rs.full_text   = transcript;
            rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
            transcribe_session::SegmentEntry seg{};
            seg.text  = transcript;
            seg.t0_ms = 0;
            seg.t1_ms = audio_ms;
            rs.segments.push_back(std::move(seg));
        }
        // Per-utterance truncation parity with single-shot run(): a row cut at
        // the generation budget / KV window before eos reports
        // TRANSCRIBE_ERR_OUTPUT_TRUNCATED (partial transcript retained). Only
        // override a TRANSCRIBE_OK status, never a worse one.
        if (b < static_cast<int>(truncated.size()) && truncated[b] && rs.status == TRANSCRIBE_OK) {
            cc->was_truncated = true;
            rs.status         = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = step_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }
    return TRANSCRIBE_OK;
}

extern const Arch arch = {
    /* .name             = */ "granite_speech",
    /* .load             = */ load,
    /* .init_context     = */ init_context,
    /* .run              = */ run,
    /* .run_batch        = */ run_batch,
    /* .stream_validate  = */ nullptr,
    /* .stream_begin     = */ nullptr,
    /* .stream_feed      = */ nullptr,
    /* .stream_finalize  = */ nullptr,
    /* .stream_reset     = */ nullptr,
    /* .accepts_ext_kind = */ nullptr,
};

}  // namespace transcribe::granite
