// arch/funasr_nano/model.cpp - FunASR-Nano family handler.

#include "adaptor.h"
#include "causal_lm/causal_lm.h"
#include "decoder.h"
#include "encoder.h"
#include "funasr_nano.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "sanm/sanm.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-flash-policy.h"
#include "transcribe-kaldi-fbank.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::funasr_nano {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, FunAsrNanoModel>);
static_assert(std::is_base_of_v<transcribe_session, FunAsrNanoSession>);

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

FunAsrNanoSession::~FunAsrNanoSession() {
    kv_cache.free();
    kv_cache_batch.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

FunAsrNanoModel::~FunAsrNanoModel() {
    if (ctx_meta != nullptr) {
        ggml_free(ctx_meta);
        ctx_meta = nullptr;
    }
    if (backend_buffer != nullptr) {
        safe_buffer_free(backend_buffer);
        backend_buffer = nullptr;
    }
    packed_gate_up.free();
    for (auto it = plan.scheduler_list.rbegin(); it != plan.scheduler_list.rend(); ++it) {
        safe_backend_free(*it);
    }
    plan.scheduler_list.clear();
    plan.primary      = nullptr;
    plan.primary_kind = transcribe::BackendKind::Unknown;
}

namespace {

constexpr const char k_default_variant[] = "fun-asr-nano-2512";

// ---------------------------------------------------------------------------
// Input-length contract (see docs/input-limits.md). Fun-ASR-Nano is a
// hard-context-cap family: audio tokens + chat prompt + generation share the
// Qwen3 decoder's context window (dec_max_position_embeddings). The KV cache
// grows to fit the prompt, clamped to that ceiling. Over-length input is
// rejected up front with TRANSCRIBE_ERR_INPUT_TOO_LONG; a transcript that
// fills the per-run generation budget before end-of-stream is flagged via
// transcribe_was_truncated().
// ---------------------------------------------------------------------------

// Per-run generation budget.
constexpr int k_max_new = 256;

// Effective decoder context ceiling, in tokens: the model's trained maximum,
// optionally lowered — never raised — by the caller's session n_ctx knob.
int funasr_nano_context_ceiling(int32_t n_ctx_knob, const FunAsrNanoHParams & hp) {
    int ceiling = hp.dec_max_position_embeddings;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose
// audio tokens plus a representative prompt and the generation reserve fit
// the context ceiling. audio_tokens ≈ N / (hop * lfr_n * folds), so
// ms ≈ audio_tokens * folds * lfr_n * hop * 1000 / sr. Returns 0
// ("unknown / unbounded") if the rate constants are missing.
int64_t funasr_nano_max_audio_ms(const FunAsrNanoHParams & hp) {
    if (hp.dec_max_position_embeddings <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0 || hp.fe_lfr_n <= 0) {
        return 0;
    }
    constexpr int k_prompt_overhead = 48;  // chat affixes; advisory
    const int     max_audio_tokens  = hp.dec_max_position_embeddings - k_prompt_overhead - k_max_new;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    const int     folds   = hp.adaptor_use_low_frame_rate ? 8 : 1;
    const int64_t samples = static_cast<int64_t>(max_audio_tokens) * folds * hp.fe_lfr_n * hp.fe_hop_length;
    return samples * 1000 / hp.fe_sample_rate;
}

// Resolve chat-template special-token ids at load time.
transcribe_status resolve_chat_tokens(const transcribe::Tokenizer & tok, ChatTokens & out) {
    struct PieceSlot {
        const char * piece;
        int32_t *    slot;
    };

    const PieceSlot pieces[] = {
        { "<|im_start|>", &out.im_start       },
        { "<|im_end|>",   &out.im_end         },
        { "\xC4\x8A",     &out.newline        }, // byte-level "\n"
        { "system",       &out.role_system    },
        { "user",         &out.role_user      },
        { "assistant",    &out.role_assistant },
    };
    for (const auto & p : pieces) {
        const int id = tok.find(p.piece);
        if (id < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: chat-template piece \"%s\" not in tokenizer", p.piece);
            return TRANSCRIBE_ERR_GGUF;
        }
        *p.slot = id;
    }
    return TRANSCRIBE_OK;
}

// Build the language/itn prompt text that the reference's
// FunASRNano.get_prompt produces. hotwords-empty path only.
std::string build_funasr_prompt_text(const char * lang, bool use_itn) {
    std::string out;
    if (lang != nullptr && lang[0] != '\0') {
        // 语音转写成 = "transcribe to" / "transcribe into"
        out = "\xE8\xAF\xAD\xE9\x9F\xB3\xE8\xBD\xAC\xE5\x86\x99\xE6\x88\x90";
        out += lang;
    } else {
        out = "\xE8\xAF\xAD\xE9\x9F\xB3\xE8\xBD\xAC\xE5\x86\x99";
    }
    if (!use_itn) {
        // ，不进行文本规整 = "; do not apply text normalization"
        out +=
            "\xEF\xBC\x8C\xE4\xB8\x8D\xE8\xBF\x9B\xE8\xA1\x8C"
            "\xE6\x96\x87\xE6\x9C\xAC\xE8\xA7\x84\xE6\x95\xB4";
    }
    out += "\xEF\xBC\x9A";  // 全角冒号 = "："
    return out;
}

// Encode a chat-template text fragment, pushing the literal id for
// `<|im_start|>` / `<|im_end|>` markers and BPE-encoding the rest. The
// reference Python `tokenizer.encode(...)` recognizes these as added
// tokens; our internal Tokenizer::encode does not, so we split here.
transcribe_status encode_with_chat_specials(const transcribe::Tokenizer & tok,
                                            const ChatTokens &            ct,
                                            const std::string &           text,
                                            std::vector<int32_t> &        out_ids) {
    static const std::string k_im_start = "<|im_start|>";
    static const std::string k_im_end   = "<|im_end|>";

    size_t cursor = 0;
    while (cursor < text.size()) {
        size_t s_idx = text.find(k_im_start, cursor);
        size_t e_idx = text.find(k_im_end, cursor);
        size_t next  = std::min(s_idx, e_idx);
        if (next == std::string::npos) {
            // Tail: BPE-encode whatever remains.
            const std::string tail = text.substr(cursor);
            if (!tail.empty()) {
                std::vector<int32_t> ids;
                if (const transcribe_status st = tok.encode(tail, ids); st != TRANSCRIBE_OK) {
                    return st;
                }
                out_ids.insert(out_ids.end(), ids.begin(), ids.end());
            }
            return TRANSCRIBE_OK;
        }
        if (next > cursor) {
            const std::string    seg = text.substr(cursor, next - cursor);
            std::vector<int32_t> ids;
            if (const transcribe_status st = tok.encode(seg, ids); st != TRANSCRIBE_OK) {
                return st;
            }
            out_ids.insert(out_ids.end(), ids.begin(), ids.end());
        }
        if (next == s_idx) {
            out_ids.push_back(ct.im_start);
            cursor = next + k_im_start.size();
        } else {
            out_ids.push_back(ct.im_end);
            cursor = next + k_im_end.size();
        }
    }
    return TRANSCRIBE_OK;
}

// Build the FunASRNano prompt mirroring data_load_speech.
//
// source_input pattern (i=0, sys_prompt=True, do_think=True default):
//   <|im_start|>system\nYou are a helpful assistant.<|im_end|>\n
//   <|im_start|>user\n{prompt}<|startofspeech|>!!<|endofspeech|><|im_end|>\n
//   <|im_start|>assistant\n
//
// The reference's pattern.split() splits at <|startofspeech|>...<|endofspeech|>;
// for each text segment it calls tokenizer.encode(...). We mirror that
// boundary exactly; encode_with_chat_specials handles the
// <|im_start|>/<|im_end|> within each segment.
transcribe_status build_funasr_nano_prompt(const transcribe::Tokenizer & tok,
                                           const ChatTokens &            ct,
                                           const char *                  language,
                                           bool                          use_itn,
                                           int                           fake_token_len,
                                           std::vector<int32_t> &        out_ids,
                                           int &                         out_fbank_beg) {
    out_ids.clear();
    out_fbank_beg = 0;

    const std::string prompt_text = build_funasr_prompt_text(language, use_itn);

    std::string seg_a =
        "<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n"
        "<|im_start|>user\n";
    seg_a += prompt_text;
    if (const transcribe_status st = encode_with_chat_specials(tok, ct, seg_a, out_ids); st != TRANSCRIBE_OK) {
        return st;
    }

    out_fbank_beg = static_cast<int>(out_ids.size());
    for (int i = 0; i < fake_token_len; ++i) {
        out_ids.push_back(0);
    }

    const std::string seg_c = "<|im_end|>\n<|im_start|>assistant\n";
    if (const transcribe_status st = encode_with_chat_specials(tok, ct, seg_c, out_ids); st != TRANSCRIBE_OK) {
        return st;
    }

    return TRANSCRIBE_OK;
}

void apply_thread_policy(FunAsrNanoSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

// ---------------------------------------------------------------------------
// load
// ---------------------------------------------------------------------------

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<FunAsrNanoModel>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = loader.variant().empty() ? k_default_variant : loader.variant();
    m->backend.clear();

    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (auto st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Tokenizer (gpt2 byte-level BPE; pretokenizer "qwen2").
    if (auto st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    // Chat template — required; absence is a hard fail because the
    // prompt builder needs the structure.
    (void) read_optional_string_kv(loader.gguf(), "tokenizer.chat_template", "funasr_nano", "", m->chat_template);

    // Resolve special token ids.
    if (auto st = resolve_chat_tokens(m->tok, m->chat_tokens); st != TRANSCRIBE_OK) {
        return st;
    }

    if (auto st = read_funasr_nano_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window
    // and frontend rate are known. See docs/input-limits.md.
    m->caps.max_audio_ms = funasr_nano_max_audio_ms(m->hparams);

    // Basis for the session-level limits query (transcribe_session_get_limits):
    // the same constants funasr_nano_max_audio_ms uses, kept so the effective
    // limit can be recomputed at a lowered session n_ctx.
    if (m->hparams.dec_max_position_embeddings > 0 && m->hparams.fe_hop_length > 0 && m->hparams.fe_sample_rate > 0 &&
        m->hparams.fe_lfr_n > 0) {
        const int folds              = m->hparams.adaptor_use_low_frame_rate ? 8 : 1;
        m->limits.has_context_cap    = true;
        m->limits.model_max_ctx      = m->hparams.dec_max_position_embeddings;
        m->limits.prompt_overhead    = 48;
        m->limits.gen_reserve        = k_max_new;
        m->limits.ms_per_audio_token = static_cast<double>(folds) * m->hparams.fe_lfr_n * m->hparams.fe_hop_length *
                                       1000.0 / m->hparams.fe_sample_rate;
        m->limits.kv_elems_per_ctx_token =
            (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
    }

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();

    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: GGUF tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Reopen with no_alloc to bind the tensor catalog.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_funasr_nano_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "funasr_nano", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "funasr_nano");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Pack gate+up into one tensor per layer so the FFN runs as a
    // single mul_mat. Owned by causal_lm::pack_gate_up.
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "funasr_nano")) {
            m->packed_gate_up.free();
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    {
        const auto &                 hp_ = m->hparams;
        transcribe::KaldiFbankParams fe_params;
        fe_params.n_mels          = hp_.fe_num_mels;
        fe_params.sample_rate     = hp_.fe_sample_rate;
        fe_params.win_length      = hp_.fe_win_length;
        fe_params.hop_length      = hp_.fe_hop_length;
        fe_params.lfr_m           = hp_.fe_lfr_m;
        fe_params.lfr_n           = hp_.fe_lfr_n;
        fe_params.d_input         = hp_.enc_d_input;
        fe_params.upscale_samples = hp_.fe_upscale_samples;
        fe_params.apply_cmvn      = hp_.fe_apply_cmvn;
        // Fun-ASR-Nano trains on raw LFR features (apply_cmvn=false), so
        // the cmvn_shift / cmvn_scale buffers are intentionally empty.
        m->frontend               = std::make_unique<transcribe::KaldiFbankFrontend>(std::move(fe_params));
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

    auto cc       = std::make_unique<FunAsrNanoSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    cc->encoder_use_flash = true;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    auto * cm = static_cast<FunAsrNanoModel *>(model);
    {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary,
                                            /*n_ctx=*/2048, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                            cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "funasr_nano init_context: KV cache allocation failed "
                                "(n_ctx=2048, %d kv-heads x %d head-dim x %d layers) — "
                                "out of memory.",
                                cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim, cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    }

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * cc = static_cast<FunAsrNanoSession *>(session);
    auto * cm = static_cast<FunAsrNanoModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || cm->frontend == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();
    cc->clear_result();

    const auto & hp = cm->hparams;

    // ---- Frontend (host-side) ----
    const int64_t t_mel_start = ggml_time_us();
    const int     T_lfr       = cm->frontend->compute(pcm, static_cast<size_t>(n_samples), cc->frontend_buf);
    cc->t_mel_us              = ggml_time_us() - t_mel_start;

    if (T_lfr <= 0) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: input too short (n_samples=%d -> T_lfr=0)",
                            n_samples);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (transcribe::debug::enabled()) {
        // dump_host_f32's `shape` arg is numpy/row-major (slow-to-fast).
        // The frontend buffer is [T_lfr, d_input] row-major (T outer).
        const long long shape[2] = { T_lfr, hp.enc_d_input };
        transcribe::debug::dump_host_f32("frontend.fbank.lfr.cmvn.out", cc->frontend_buf.data(),
                                         static_cast<long long>(cc->frontend_buf.size()), shape, 2,
                                         "frontend.lfr.cmvn");
    }

    // ---- Reset compute context ----
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 32 * 1024 * 1024;  // generous for 70 SAN-M blocks + adaptor + 28 LM blocks
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: ggml_init for compute_ctx failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ---- Build encoder graph ----
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, T_lfr);
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, /*parallel=*/false,
                                           /*op_offload=*/true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "funasr_nano run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload frontend output and PE.
    ggml_backend_tensor_set(eb.frontend_in, cc->frontend_buf.data(), 0, cc->frontend_buf.size() * sizeof(float));
    transcribe::sanm::build_sinusoidal_pe(cc->pe_buf, hp.enc_d_input, T_lfr);
    ggml_backend_tensor_set(eb.pe_in, cc->pe_buf.data(), 0, cc->pe_buf.size() * sizeof(float));

    apply_thread_policy(cc);

    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: encoder graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // Dump encoder intermediates.
    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.embed.out", eb.dumps.embed_out, "encoder.embed.pos_added");
    try_dump("enc.encoders0.0.out", eb.dumps.encoders0_0_out, "encoder.encoders0.0");
    if (eb.dumps.encoders_first) {
        try_dump("enc.encoders.0.out", eb.dumps.encoders_first, "encoder.encoders.0");
    }
    if (eb.dumps.encoders_mid) {
        const char * nm = eb.dumps.encoders_mid->name;
        try_dump(nm, eb.dumps.encoders_mid, "encoder.encoders.mid");
    }
    if (eb.dumps.encoders_last) {
        const char * nm = eb.dumps.encoders_last->name;
        try_dump(nm, eb.dumps.encoders_last, "encoder.encoders.last");
    }
    try_dump("enc.after_norm.out", eb.dumps.after_norm_out, "encoder.after_norm");
    if (eb.dumps.tp_encoders_first) {
        try_dump("enc.tp_encoders.0.out", eb.dumps.tp_encoders_first, "encoder.tp_encoders.0");
    }
    if (eb.dumps.tp_encoders_mid) {
        const char * nm = eb.dumps.tp_encoders_mid->name;
        try_dump(nm, eb.dumps.tp_encoders_mid, "encoder.tp_encoders.mid");
    }
    if (eb.dumps.tp_encoders_last) {
        const char * nm = eb.dumps.tp_encoders_last->name;
        try_dump(nm, eb.dumps.tp_encoders_last, "encoder.tp_encoders.last");
    }
    try_dump("enc.tp_norm.out", eb.dumps.tp_norm_out, "encoder.tp_norm");

    // Read encoder output to host.
    cc->enc_host.resize(static_cast<size_t>(hp.enc_d_model) * static_cast<size_t>(T_lfr));
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    // ---- Build adaptor graph ----
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 8 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
    }

    AdaptorBuild ab = build_adaptor_graph(cc->compute_ctx, cm->weights, hp, T_lfr);
    if (ab.graph == nullptr || ab.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, ab.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "funasr_nano run: adaptor graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }
    ggml_backend_tensor_set(ab.enc_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, ab.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: adaptor graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }

    try_dump("adaptor.linear1.out", ab.dumps.linear1_out, "adaptor.linear1");
    try_dump("adaptor.linear2.out", ab.dumps.linear2_out, "adaptor.linear2");
    try_dump("adaptor.blocks.0.out", ab.dumps.block0_out, "adaptor.blocks.0");
    try_dump("adaptor.out", ab.dumps.adaptor_out, "adaptor.out");

    cc->adaptor_host.resize(static_cast<size_t>(hp.adaptor_llm_dim) * static_cast<size_t>(T_lfr));
    ggml_backend_tensor_get(ab.out, cc->adaptor_host.data(), 0, cc->adaptor_host.size() * sizeof(float));

    // ---- Build prompt + audio splice ----
    const int fake_token_len = compute_fake_token_len(T_lfr, hp.adaptor_use_low_frame_rate);

    const char * lang    = (params != nullptr) ? params->language : nullptr;
    // Generic transcribe_run_params::itn routes here. DEFAULT maps to the
    // family's shipping default (use_itn=false; matches the upstream
    // `itn=False` Python path). OFF / ON override explicitly. The
    // dispatcher's advisory WARN only fires when transcribe_model_supports(
    // model, TRANSCRIBE_FEATURE_ITN) is false; funasr_nano sets
    // TRANSCRIBE_FEATURE_ITN so the probe returns true and no WARN here.
    bool         use_itn = false;
    if (params != nullptr) {
        switch (params->itn) {
            case TRANSCRIBE_ITN_MODE_DEFAULT:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_OFF:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_ON:
                use_itn = true;
                break;
        }
    }

    std::vector<int32_t> prompt_ids;
    int                  fbank_beg = 0;
    if (const transcribe_status st =
            build_funasr_nano_prompt(cm->tok, cm->chat_tokens, lang, use_itn, fake_token_len, prompt_ids, fbank_beg);
        st != TRANSCRIBE_OK) {
        return st;
    }

    const int T_prompt   = static_cast<int>(prompt_ids.size());
    const int T_audio    = fake_token_len;
    const int prefix_len = fbank_beg;
    const int suffix_len = T_prompt - prefix_len - T_audio;

    // ---- Input-length gate (see docs/input-limits.md) ----
    // audio tokens + prompt + generation must fit the decoder context window
    // (optionally lowered by the caller's n_ctx). The audio-token count is
    // fixed by the input length, so reject an over-length clip here, before
    // KV alloc / prefill / decode, instead of walling at a fixed size.
    const int ceiling = funasr_nano_context_ceiling(cc->n_ctx, hp);
    if (T_prompt + k_max_new > ceiling) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "funasr_nano run: input too long — %d audio + %d prompt tokens "
                            "leave no room for output within the %d-token context (need %d). "
                            "Shorten the audio (see transcribe_capabilities.max_audio_ms) or "
                            "split it into segments.",
                            T_audio, prefix_len + suffix_len, ceiling, T_prompt + k_max_new);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // ---- KV cache init (grow-to-fit, clamped to the context ceiling) ----
    // Size to hold the prompt plus the generation budget, rounded up to a
    // power of two (the step graph's attention width wants pow2 for the fast
    // flash-attn path). The cache grows across runs as audio length demands;
    // a pre-allocated smaller cache is freed and re-allocated.
    int want_n_ctx = 1024;
    while (want_n_ctx < T_prompt + k_max_new) {
        want_n_ctx *= 2;
    }
    if (want_n_ctx > ceiling) {
        want_n_ctx = ceiling;
    }
    if (cc->kv_cache.ctx != nullptr && cc->kv_cache.n_ctx < want_n_ctx) {
        cc->kv_cache.free();
    }
    if (cc->kv_cache.ctx == nullptr) {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, want_n_ctx, cm->hparams.dec_n_kv_heads,
                                            cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "funasr_nano run: KV cache allocation failed (n_ctx=%d, "
                                "%d kv-heads x %d head-dim x %d layers) — out of memory. "
                                "Lower transcribe_session_params.n_ctx or shorten the audio.",
                                want_n_ctx, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    } else {
        // Clear stale positions for a fresh prefill.
        if (cc->kv_cache.buffer != nullptr) {
            ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        }
        cc->kv_cache.n    = 0;
        cc->kv_cache.head = 0;
    }

    const int64_t t_dec_start = ggml_time_us();

    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 16 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
    }

    const bool   dumps_on   = transcribe::debug::enabled();
    const bool   slice_last = !dumps_on;
    PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_prompt, T_audio, prefix_len,
                                          suffix_len, cc->decoder_use_flash, slice_last);
    if (pb.graph == nullptr || pb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "funasr_nano run: prefill graph allocation failed (T_prompt=%d) — "
                            "out of memory. Lower transcribe_session_params.n_ctx or shorten "
                            "the audio.",
                            T_prompt);
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));

    if (T_audio > 0 && pb.audio_in != nullptr) {
        // Upload first fake_token_len rows of adaptor_out. adaptor_host
        // is row-major [T_lfr, llm_dim]; upload llm_dim * fake_token_len
        // floats.
        const size_t bytes = static_cast<size_t>(T_audio) * hp.adaptor_llm_dim * sizeof(float);
        ggml_backend_tensor_set(pb.audio_in, cc->adaptor_host.data(), 0, bytes);
    }

    {
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }
    {
        const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
        const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, mask_neg_inf);
        for (int r = 0; r < T_prompt; ++r) {
            for (int c = 0; c <= r; ++c) {
                mask[static_cast<size_t>(r) * T_prompt + c] = mask_zero;
            }
        }
        ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
    }

    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: prefill graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }

    cc->kv_cache.n    = T_prompt;
    cc->kv_cache.head = T_prompt;

    try_dump("dec.token_emb", pb.dumps.token_emb, "dec.token_emb");
    try_dump("dec.inputs_embeds.with_audio", pb.dumps.audio_injected, "dec.audio_injected");
    try_dump("dec.block.0.out", pb.dumps.block_0_out, "dec.block.0");
    if (pb.dumps.block_last_out) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", hp.dec_n_layers - 1);
        try_dump(nm, pb.dumps.block_last_out, "dec.block.last");
    }
    try_dump("dec.out_before_head", pb.dumps.out_before_head, "dec.out_before_head");
    try_dump("dec.logits_raw.prefill", pb.dumps.logits_raw, "decoder.logits.prefill");

    const int          vocab = hp.dec_vocab_size;
    std::vector<float> logits(vocab);
    ggml_backend_tensor_get(pb.out, logits.data(), 0, logits.size() * sizeof(float));

    auto argmax = [&](const std::vector<float> & v) -> int32_t {
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

    std::vector<int32_t> generated_ids;
    int32_t              next_tok = argmax(logits);
    generated_ids.push_back(next_tok);

    // ---- Step loop ----
    const int32_t eos_id   = hp.eos_token_id;
    const int     max_new  = k_max_new;
    int           cur_past = T_prompt;

    // Static step-graph shape: T_prompt prefilled + up to max_new generated.
    // Clamp to cc->kv_cache.n_ctx (the grown cache size) so the attention
    // width never exceeds the allocation.
    int max_n_kv = 1024;
    while (max_n_kv < T_prompt + max_new) {
        max_n_kv *= 2;
    }
    if (max_n_kv > cc->kv_cache.n_ctx) {
        max_n_kv = cc->kv_cache.n_ctx;
    }

    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 8 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
    }
    StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, max_n_kv, cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "funasr_nano step: decode graph allocation failed — out of memory. "
                            "Lower transcribe_session_params.n_ctx or shorten the audio.");
        return TRANSCRIBE_ERR_OOM;
    }

    const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> step_mask(max_n_kv, mask_neg_inf);

    // Mid-generation logits dump. The reference captures the 9th lm_head call
    // (prefill = 1st call, iter K = (K+2)th call), so dump when n_steps == 7.
    const int gen_dump_step = 7;
    int       n_steps       = 0;
    while (next_tok != eos_id && static_cast<int32_t>(generated_ids.size()) < max_new && cur_past + 1 <= max_n_kv) {
        ggml_backend_tensor_set(sb.input_id_in, &next_tok, 0, sizeof(int32_t));
        const int32_t pos_val = cur_past;
        ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
        const int64_t kv_idx_val = cur_past;
        ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx_val, 0, sizeof(int64_t));

        if (cur_past == T_prompt) {
            std::fill(step_mask.begin(), step_mask.begin() + cur_past + 1, mask_zero);
        } else {
            step_mask[cur_past] = mask_zero;
        }
        ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0, static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano run: step graph compute failed (%d)",
                    static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }

        int32_t argmax_tok = 0;
        ggml_backend_tensor_get(sb.out, &argmax_tok, 0, sizeof(int32_t));
        next_tok = argmax_tok;
        generated_ids.push_back(next_tok);

        if (n_steps == gen_dump_step && transcribe::debug::enabled()) {
            try_dump("dec.logits_raw.gen8", sb.logits, "decoder.logits.gen8");
        }

        cur_past += 1;
        n_steps += 1;
    }
    (void) n_steps;

    // The decode stopped at EOS (complete) or at the generation budget /
    // context width (truncated). Surface the latter via
    // transcribe_was_truncated() and a WARN rather than returning a silently
    // shortened transcript. See docs/input-limits.md.
    if (next_tok != eos_id) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "funasr_nano run: output truncated at %d tokens — decode reached "
                            "the generation budget before end-of-stream; the transcript may be "
                            "incomplete.",
                            static_cast<int>(generated_ids.size()));
    }

    if (!generated_ids.empty() && generated_ids.back() == eos_id) {
        generated_ids.pop_back();
    }

    std::string transcript = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));

    cc->t_decode_us = ggml_time_us() - t_dec_start;

    cc->full_text   = transcript;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;

    transcribe_session::SegmentEntry seg{};
    seg.text  = transcript;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
    cc->segments.push_back(std::move(seg));

    // The partial transcript is fully populated above; a truncated decode
    // returns the hard OUTPUT_TRUNCATED status (the result stays readable,
    // like an aborted run). See docs/input-limits.md.
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

}  // namespace

// ===========================================================================
// Offline batched decode (transcribe_run_batch)
// ===========================================================================
// Serial frontend+encoder+adaptor per utterance produce each one's audio
// embedding (adaptor output, [hidden, T_audio]); then the prefill and the
// autoregressive step loop are batched via the shared causal_lm primitives —
// the same recipe as arch/qwen3_asr.

namespace {

transcribe_status reset_ctx(FunAsrNanoSession * cc, int mb) {
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

// encoder + adaptor for one utterance from a PRECOMPUTED frontend buffer
// (the mel/fbank is computed in parallel by the caller) → audio embedding
// [hidden, T_audio]. Mirrors run()'s encoder/adaptor section (serial).
transcribe_status audio_embed_one(FunAsrNanoSession *        cc,
                                  FunAsrNanoModel *          cm,
                                  const std::vector<float> & frontend_buf,
                                  int                        T_lfr,
                                  std::vector<float> &       audio_host,
                                  int &                      T_audio_out,
                                  int64_t &                  enc_us) {
    const auto & hp = cm->hparams;
    if (T_lfr <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (const transcribe_status st = reset_ctx(cc, 32); st != TRANSCRIBE_OK) {
        return st;
    }
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, T_lfr);
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_tensor_set(eb.frontend_in, frontend_buf.data(), 0, frontend_buf.size() * sizeof(float));
    transcribe::sanm::build_sinusoidal_pe(cc->pe_buf, hp.enc_d_input, T_lfr);
    ggml_backend_tensor_set(eb.pe_in, cc->pe_buf.data(), 0, cc->pe_buf.size() * sizeof(float));
    apply_thread_policy(cc);
    const int64_t t_enc0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc0;
    cc->enc_host.resize(static_cast<size_t>(hp.enc_d_model) * T_lfr);
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    // Adaptor.
    if (const transcribe_status st = reset_ctx(cc, 8); st != TRANSCRIBE_OK) {
        return st;
    }
    AdaptorBuild ab = build_adaptor_graph(cc->compute_ctx, cm->weights, hp, T_lfr);
    if (ab.graph == nullptr || ab.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, ab.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_tensor_set(ab.enc_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
    const int64_t t_enc1 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, ab.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc1;
    cc->adaptor_host.resize(static_cast<size_t>(hp.adaptor_llm_dim) * T_lfr);
    ggml_backend_tensor_get(ab.out, cc->adaptor_host.data(), 0, cc->adaptor_host.size() * sizeof(float));

    const int fake_token_len = compute_fake_token_len(T_lfr, hp.adaptor_use_low_frame_rate);
    T_audio_out              = fake_token_len;
    audio_host.assign(cc->adaptor_host.begin(),
                      cc->adaptor_host.begin() + static_cast<size_t>(fake_token_len) * hp.adaptor_llm_dim);
    return TRANSCRIBE_OK;
}

transcribe_status run_batch_serial(FunAsrNanoSession *           cc,
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
    auto * cc = static_cast<FunAsrNanoSession *>(session);
    auto * cm = static_cast<FunAsrNanoModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || cm->frontend == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (!cc->decoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto & hp = cm->hparams;

    // ---- Pass 1: per-utterance frontend + encoder + adaptor (serial) ----
    std::vector<char>                 valid(n, 0);
    // Per-utterance failure status for the result capture below. Defaults to
    // INVALID_ARG (bad pcm / frontend / encode / prompt); the input-length gate
    // upgrades it to INPUT_TOO_LONG for over-length clips.
    std::vector<transcribe_status>    fail_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<float>>   audio_hosts(n);
    std::vector<int>                  T_audio(n, 0);
    std::vector<std::vector<int32_t>> prompt_ids(n);
    std::vector<int>                  T_prompt(n, 0);
    int                               prefix_len = 0;
    int64_t                           mel_us = 0, enc_us = 0;

    // Decoder context ceiling for the per-utterance input-length gate (see
    // docs/input-limits.md). Same value the single-utterance run() enforces;
    // an over-length utterance is rejected with TRANSCRIBE_ERR_INPUT_TOO_LONG
    // rather than walling at a fixed KV size.
    const int ceiling = funasr_nano_context_ceiling(cc->n_ctx, hp);

    const char * lang    = (params != nullptr) ? params->language : nullptr;
    bool         use_itn = (params != nullptr && params->itn == TRANSCRIBE_ITN_MODE_ON);

    // ---- Pass 0: parallel frontend (kaldi-fbank, host-side, thread-safe) ----
    std::vector<std::vector<float>> fbufs(n);
    std::vector<int>                T_lfr(n, 0);
    int                             n_mel_threads = cc->n_threads;
    if (n_mel_threads <= 0) {
        n_mel_threads = transcribe::default_n_threads();
    }
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_mel_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;
        }
        const int t = cm->frontend->compute(pcm[b], static_cast<size_t>(n_samples[b]), fbufs[b]);
        if (t > 0) {
            T_lfr[b] = t;
        }
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;

    // ---- Pass 1: per-utterance encoder + adaptor (serial) ----
    for (int b = 0; b < n; ++b) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (T_lfr[b] <= 0) {
            continue;
        }
        if (audio_embed_one(cc, cm, fbufs[b], T_lfr[b], audio_hosts[b], T_audio[b], enc_us) != TRANSCRIBE_OK) {
            continue;
        }
        int fbank_beg = 0;
        if (build_funasr_nano_prompt(cm->tok, cm->chat_tokens, lang, use_itn, T_audio[b], prompt_ids[b], fbank_beg) !=
            TRANSCRIBE_OK) {
            continue;
        }
        T_prompt[b] = static_cast<int>(prompt_ids[b].size());
        prefix_len  = fbank_beg;

        // Input-length gate (see docs/input-limits.md). Audio tokens + prompt +
        // generation must fit the decoder context window; reject an over-length
        // utterance here instead of walling at a fixed KV size. Mirrors the
        // single-shot run() gate (T_prompt + k_max_new > ceiling).
        if (T_prompt[b] + k_max_new > ceiling) {
            const int suffix = T_prompt[b] - fbank_beg - T_audio[b];
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "funasr_nano run_batch: utterance %d input too long — %d audio + "
                                "%d prompt tokens leave no room for output within the %d-token "
                                "context (need %d). Shorten the audio (see "
                                "transcribe_capabilities.max_audio_ms) or split it.",
                                b, T_audio[b], fbank_beg + suffix, ceiling, T_prompt[b] + k_max_new);
            fail_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
            continue;
        }
        valid[b] = 1;
    }

    int max_T_prompt = 0;
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            max_T_prompt = std::max(max_T_prompt, T_prompt[b]);
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

    // ---- Allocate batched KV cache ----
    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx != max_n_kv) {
        cc->kv_cache_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_cache_batch, cm->plan.primary, max_n_kv, hp.dec_n_kv_heads,
                                                    hp.dec_head_dim, hp.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "funasr_nano run_batch: batched KV cache allocation failed "
                                "(n_ctx=%d x %d utterances) — out of memory. Lower "
                                "transcribe_session_params.n_ctx or the batch size.",
                                max_n_kv, n);
            return TRANSCRIBE_ERR_OOM;
        }
        cc->kv_batch_cap   = n;
        cc->kv_batch_n_ctx = max_n_kv;
    } else if (cc->kv_cache_batch.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache_batch.buffer, 0);
    }

    // ---- Pass 2: batched prefill ----
    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, 0);
    std::vector<std::vector<int32_t>> generated(n);
    {
        int T_audio_max = 0;
        for (int b = 0; b < n; ++b) {
            if (valid[b]) {
                T_audio_max = std::max(T_audio_max, T_audio[b]);
            }
        }
        T_audio_max = std::max(1, T_audio_max);
        if (reset_ctx(cc, 32) != TRANSCRIBE_OK) {
            return TRANSCRIBE_ERR_GGUF;
        }
        PrefillBuildBatched pb = build_prefill_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch,
                                                             max_T_prompt, T_audio_max, n, cc->decoder_use_flash);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            return TRANSCRIBE_ERR_GGUF;
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
            const int ta = valid[b] ? T_audio[b] : 0;
            const int tp = valid[b] ? T_prompt[b] : 0;
            if (valid[b]) {
                std::memcpy(ids.data() + static_cast<size_t>(b) * max_T_prompt, prompt_ids[b].data(),
                            static_cast<size_t>(tp) * sizeof(int32_t));
                // audio_hosts[b] is [hidden, ta] column-major; audio token j lands
                // at prompt position prefix_len+j, flat column b*max_T_prompt+pos.
                for (int j = 0; j < ta; ++j) {
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
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
            const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(max_T_prompt) * max_T_prompt, mn);
            for (int q = 0; q < max_T_prompt; ++q) {
                std::fill(mask.begin() + static_cast<size_t>(q) * max_T_prompt,
                          mask.begin() + static_cast<size_t>(q) * max_T_prompt + q + 1, mz);
            }
            ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        ggml_backend_tensor_set(pb.kv_idx_in, kidx.data(), 0, kidx.size() * sizeof(int64_t));
        ggml_backend_tensor_set(pb.last_idx_in, lidx.data(), 0, lidx.size() * sizeof(int32_t));
        apply_thread_policy(cc);
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

    // ---- Pass 3: batched step loop (shared causal_lm driver) ----
    const int32_t eos_id = cm->hparams.eos_token_id;

    if (reset_ctx(cc, 16) != TRANSCRIBE_OK) {
        return TRANSCRIBE_ERR_GGUF;
    }
    StepBuildBatched sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch, max_n_kv, n,
                                                   cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
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

    // ---- Capture results ----
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
        std::string                   transcript = cm->tok.decode(gen.data(), static_cast<int>(gen.size()));
        transcribe_session::ResultSet rs;
        rs.full_text   = transcript;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        rs.status      = TRANSCRIBE_OK;
        transcribe_session::SegmentEntry seg{};
        seg.text  = transcript;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(n_samples[b]) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
        rs.segments.push_back(std::move(seg));
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
    /* .name             = */ "funasr_nano",
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

}  // namespace transcribe::funasr_nano
