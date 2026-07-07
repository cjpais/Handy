// arch/qwen3_asr/model.cpp - Qwen3-ASR family handler.

#include "causal_lm/causal_lm.h"
#include "decoder.h"
#include "encoder.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "qwen3_asr.h"
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
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <unordered_map>
#include <vector>

namespace transcribe::qwen3_asr {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, QwenAsrModel>);
static_assert(std::is_base_of_v<transcribe_session, QwenAsrSession>);

QwenAsrSession::~QwenAsrSession() {
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

QwenAsrModel::~QwenAsrModel() {
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

constexpr const char k_default_variant[] = "qwen3-asr";

// Input-length contract (see docs/input-limits.md). Qwen3-ASR is a
// hard-context-cap family: audio tokens + chat prompt + generation share the
// Qwen3 decoder's context window (dec_max_position_embeddings), clamped to that
// ceiling. Over-length input is rejected with TRANSCRIBE_ERR_INPUT_TOO_LONG; a
// transcript that fills the generation budget before end-of-stream is flagged
// via transcribe_was_truncated().

// Per-run generation budget (matches the reference dumper default).
constexpr int k_max_new = 256;

int qwen3_max_new_tokens(const transcribe_run_params * params) {
    if (params != nullptr &&
        params->struct_size >= offsetof(transcribe_run_params, max_new_tokens) + sizeof(params->max_new_tokens) &&
        params->max_new_tokens > 0) {
        return params->max_new_tokens;
    }
    return k_max_new;
}

// Effective decoder context ceiling, in tokens: the model's trained maximum,
// optionally lowered — never raised — by the caller's session n_ctx knob.
int qwen3_context_ceiling(int32_t n_ctx_knob, const QwenAsrHParams & hp) {
    int ceiling = hp.dec_max_position_embeddings;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose
// audio tokens plus a representative prompt and the generation reserve fit
// the context ceiling. The audio encoder downsamples mel frames 8x (three
// stride-2 convs, see aftercnn_len); inverting that gives ms. Returns 0
// ("unknown / unbounded") if the rate constants are missing. Note: even
// within this bound a long transcript may truncate at the generation budget
// (transcribe_was_truncated) — max_audio_ms is the input bound.
int64_t qwen3_max_audio_ms(const QwenAsrHParams & hp) {
    if (hp.dec_max_position_embeddings <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0) {
        return 0;
    }
    constexpr int k_prompt_overhead = 48;  // chat affixes; advisory
    const int     max_audio_tokens  = hp.dec_max_position_embeddings - k_prompt_overhead - k_max_new;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    // audio_tokens ≈ mel_frames / 8 ; mel_frames = ms * sr / (hop * 1000)
    //   => ms ≈ audio_tokens * 8 * hop * 1000 / sr
    const int64_t mel_frames = static_cast<int64_t>(max_audio_tokens) * 8;
    return mel_frames * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
}

// Forward declarations for helpers defined further down in this file.
transcribe_status resolve_chat_tokens(const transcribe::Tokenizer & tok, ChatTokens & out);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<QwenAsrModel>();
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
    if (const transcribe_status st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Tokenizer (byte-level BPE; loader handles the "gpt2" model tag).
    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    // Chat template (read here so its absence surfaces at load, not mid-decode).
    (void) read_optional_string_kv(loader.gguf(), "tokenizer.chat_template", "qwen3_asr", "", m->chat_template);

    // Resolve chat-template special-token ids at load so a vocab drift surfaces
    // here instead of silently producing a wrong prompt at decode time.
    if (const transcribe_status st = resolve_chat_tokens(m->tok, m->chat_tokens); st != TRANSCRIBE_OK) {
        return st;
    }

    if (const transcribe_status st = read_qwen3_asr_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window
    // and frontend rate are known.
    m->caps.max_audio_ms = qwen3_max_audio_ms(m->hparams);

    // Basis for transcribe_session_get_limits: the same constants
    // qwen3_max_audio_ms uses, so the limit recomputes at a lowered n_ctx.
    if (m->hparams.dec_max_position_embeddings > 0 && m->hparams.fe_hop_length > 0 && m->hparams.fe_sample_rate > 0) {
        m->limits.has_context_cap    = true;
        m->limits.model_max_ctx      = m->hparams.dec_max_position_embeddings;
        m->limits.prompt_overhead    = 48;
        m->limits.gen_reserve        = k_max_new;
        // audio_tokens ≈ mel_frames / 8 ; mel_frames = ms*sr/(hop*1000)
        m->limits.ms_per_audio_token = 8.0 * m->hparams.fe_hop_length * 1000.0 / m->hparams.fe_sample_rate;
        m->limits.kv_elems_per_ctx_token =
            (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
    }

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();

    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: GGUF tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Mel frontend (Whisper-style 128-bin log-mel at 16 kHz, 30 s max).
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
        cfg.normalize    = m->hparams.fe_normalize;  // "per_utterance" (Whisper)

        // Optional filterbank + window buffers baked by the converter; if
        // present MelFrontend uses them instead of reconstructing from hparams.
        {
            using R               = transcribe::load_common::ReadF32Result;
            const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
            const auto   fb_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "qwen3_asr", cfg.filterbank);
            if (fb_rc != R::Ok && fb_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const size_t win_elems = static_cast<size_t>(cfg.win_length);
            const auto   win_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.window", win_elems, "qwen3_asr", cfg.window);
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

    if (const transcribe_status st = build_qwen3_asr_weights(m->ctx_meta, m->hparams, m->weights);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Backend plan.
    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "qwen3_asr", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "qwen3_asr");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Pack gate+up into a separate session + backend buffer so the FFN
    // can run a single mul_mat instead of two. ctx_meta is sized
    // exactly for GGUF file tensors with no headroom, so packed
    // tensors live in their own context owned by `causal_lm::pack_gate_up`.
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "qwen3_asr")) {
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

    auto cc       = std::make_unique<QwenAsrSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    cc->encoder_use_flash = false;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    // Pre-allocate KV cache at context creation so the first run
    // doesn't pay the allocation cost inside the decode phase.
    auto * cm = static_cast<QwenAsrModel *>(model);
    {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary,
                                            /*n_ctx=*/8192, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                            cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "qwen3_asr init_context: KV cache allocation failed "
                                "(n_ctx=8192, %d kv-heads x %d head-dim x %d layers) — "
                                "out of memory.",
                                cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim, cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    }

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Resolve the chat-template piece strings against the loaded tokenizer.
// Hard-fails with TRANSCRIBE_ERR_GGUF on a missing piece (a vocab reorder
// surfaces here at load). The newline is stored in its GPT-2 byte-level form
// (\n → U+010A "Ċ", \xC4\x8A); roles and <|im_*|> tokens are verbatim.
transcribe_status resolve_chat_tokens(const transcribe::Tokenizer & tok, ChatTokens & out) {
    struct PieceSlot {
        const char * piece;
        int32_t *    slot;
    };

    const PieceSlot pieces[] = {
        { "<|im_start|>", &out.im_start       },
        { "<|im_end|>",   &out.im_end         },
        { "\xC4\x8A",     &out.newline        }, // "Ċ" = byte-level \n
        { "system",       &out.role_system    },
        { "user",         &out.role_user      },
        { "assistant",    &out.role_assistant },
    };
    for (const auto & p : pieces) {
        const int id = tok.find(p.piece);
        if (id < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: chat-template piece \"%s\" not in tokenizer", p.piece);
            return TRANSCRIBE_ERR_GGUF;
        }
        *p.slot = id;
    }
    return TRANSCRIBE_OK;
}

// Build the prompt token sequence + audio-position list, mirroring the
// Qwen3-ASR chat template at the token level:
//
//   <|im_start|>system\n<|im_end|>\n
//   <|im_start|>user\n<|audio_start|><|audio_pad|>*T_enc<|audio_end|><|im_end|>\n
//   <|im_start|>assistant\n[language {Name}<asr_text>]?
//
// System prompt is empty. A non-null `lang_prefix_ids` (resolved via
// encode_language_prefix) is appended after the trailing newline to force an
// output language; kept out of here so this stays a pure token-id assembler.
void build_prompt_tokens(const QwenAsrHParams &       hp,
                         const ChatTokens &           ct,
                         int                          T_enc,
                         const std::vector<int32_t> * lang_prefix_ids,
                         std::vector<int32_t> &       out_ids,
                         std::vector<int64_t> &       out_audio_positions) {
    out_ids.clear();
    out_audio_positions.clear();

    out_ids.push_back(ct.im_start);
    out_ids.push_back(ct.role_system);
    out_ids.push_back(ct.newline);
    out_ids.push_back(ct.im_end);
    out_ids.push_back(ct.newline);

    out_ids.push_back(ct.im_start);
    out_ids.push_back(ct.role_user);
    out_ids.push_back(ct.newline);

    out_ids.push_back(hp.audio_start_token_id);
    const int64_t audio_start_pos = static_cast<int64_t>(out_ids.size());
    for (int i = 0; i < T_enc; ++i) {
        out_ids.push_back(hp.audio_token_id);
        out_audio_positions.push_back(audio_start_pos + i);
    }
    out_ids.push_back(hp.audio_end_token_id);

    out_ids.push_back(ct.im_end);
    out_ids.push_back(ct.newline);

    out_ids.push_back(ct.im_start);
    out_ids.push_back(ct.role_assistant);
    out_ids.push_back(ct.newline);

    if (lang_prefix_ids != nullptr && !lang_prefix_ids->empty()) {
        out_ids.insert(out_ids.end(), lang_prefix_ids->begin(), lang_prefix_ids->end());
    }
}

}  // namespace

// below; encode_language_prefix matches the qwen3_asr.h declaration.)

// BCP-47 → publisher canonical name ("English", "Chinese", ...), which the
// prompt renders instead of the code. Frozen per release
// (qwen_asr.inference.utils.SUPPORTED_LANGUAGES); this table is the update
// point for a future variant.
struct LangNameEntry {
    const char * bcp47;
    const char * pub_name;
};

constexpr LangNameEntry k_qwen3_asr_language_names[] = {
    { "zh",  "Chinese"    },
    { "en",  "English"    },
    { "yue", "Cantonese"  },
    { "ar",  "Arabic"     },
    { "de",  "German"     },
    { "fr",  "French"     },
    { "es",  "Spanish"    },
    { "pt",  "Portuguese" },
    { "id",  "Indonesian" },
    { "it",  "Italian"    },
    { "ko",  "Korean"     },
    { "ru",  "Russian"    },
    { "th",  "Thai"       },
    { "vi",  "Vietnamese" },
    { "ja",  "Japanese"   },
    { "tr",  "Turkish"    },
    { "hi",  "Hindi"      },
    { "ms",  "Malay"      },
    { "nl",  "Dutch"      },
    { "sv",  "Swedish"    },
    { "da",  "Danish"     },
    { "fi",  "Finnish"    },
    { "pl",  "Polish"     },
    { "cs",  "Czech"      },
    { "fil", "Filipino"   },
    { "fa",  "Persian"    },
    { "el",  "Greek"      },
    { "ro",  "Romanian"   },
    { "hu",  "Hungarian"  },
    { "mk",  "Macedonian" },
};

// Resolve a caller-supplied BCP-47 code to the token-id sequence the chat
// template expects: BPE("language {Name}") + the `<asr_text>` special id.
// encode() doesn't split special tokens out, so we look up the <asr_text> id
// directly from the vocab (never hardcoded) and append it by hand. The
// dispatcher validates `bcp47` against caps.languages first, so an unknown
// code here means converter/map drift — surface as UNSUPPORTED_LANGUAGE.
transcribe_status encode_language_prefix(const transcribe::Tokenizer & tok,
                                         const char *                  bcp47,
                                         std::vector<int32_t> &        out_ids) {
    out_ids.clear();
    if (bcp47 == nullptr || bcp47[0] == '\0') {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const char * pub_name = nullptr;
    for (const auto & e : k_qwen3_asr_language_names) {
        if (std::strcmp(e.bcp47, bcp47) == 0) {
            pub_name = e.pub_name;
            break;
        }
    }
    if (pub_name == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: no canonical publisher name for "
                "language=\"%s\"",
                bcp47);
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    if (!tok.has_encoder()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: tokenizer missing encoder (merges "
                "unavailable); cannot render language hint");
        return TRANSCRIBE_ERR_GGUF;
    }
    const int asr_text_id = tok.find("<asr_text>");
    if (asr_text_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: tokenizer vocab missing <asr_text> "
                "special token");
        return TRANSCRIBE_ERR_GGUF;
    }
    std::string text = "language ";
    text += pub_name;
    if (const transcribe_status st = tok.encode(text, out_ids); st != TRANSCRIBE_OK) {
        return st;
    }
    out_ids.push_back(asr_text_id);
    return TRANSCRIBE_OK;
}

namespace {  // reopen anon for the rest of the file's helpers.

// Host-side pack [n_mels, T_mel] mel into batched chunks
// [mel_per_chunk, n_mels, 1, n_chunks]. Chunks shorter than
// mel_per_chunk are zero-padded.
void pack_mel_chunks(const float *         mel,  // [n_mels, T_mel]
                     int                   n_mels,
                     int                   n_mel_frames,
                     const EncoderTiming & t,
                     std::vector<float> &  out) {
    const size_t per_chunk_elems = static_cast<size_t>(t.mel_per_chunk) * n_mels;
    out.assign(per_chunk_elems * t.n_chunks, 0.0f);

    for (int c = 0; c < t.n_chunks; ++c) {
        const int tail = (c == t.n_chunks - 1) ? t.last_chunk_real_mel : t.mel_per_chunk;
        for (int m = 0; m < n_mels; ++m) {
            const float * src = mel + static_cast<size_t>(m) * n_mel_frames + static_cast<size_t>(c) * t.mel_per_chunk;
            float *       dst =
                out.data() + static_cast<size_t>(c) * per_chunk_elems + static_cast<size_t>(m) * t.mel_per_chunk;
            std::memcpy(dst, src, tail * sizeof(float));
            // trailing frames in the tail chunk stay 0.0 (zero pad).
        }
    }
}

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * cc = static_cast<QwenAsrSession *>(session);
    auto * cm = static_cast<QwenAsrModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Pre-run abort check (Qwen3-ASR is single-shot, so this is the only
    // observation point).
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    // Language hint. Null/empty == auto-detect (the LM emits its own
    // "language X<asr_text>" prefix, stripped by the output parser below). A
    // non-null code is resolved to "language {Name}<asr_text>" tokens that seed
    // the assistant turn; a resolve failure surfaces as UNSUPPORTED_LANGUAGE.
    std::vector<int32_t>         lang_prefix_ids;
    const std::vector<int32_t> * lang_prefix_ptr = nullptr;
    if (params != nullptr && params->language != nullptr && params->language[0] != '\0') {
        if (const transcribe_status st = encode_language_prefix(cm->tok, params->language, lang_prefix_ids);
            st != TRANSCRIBE_OK) {
            return st;
        }
        lang_prefix_ptr = &lang_prefix_ids;
    }

    transcribe::debug::init();

    // Mel front-end.
    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (const transcribe_status mst =
            cm->mel->compute(pcm, static_cast<size_t>(n_samples), cc->mel_buf, mel_n_mels, mel_n_frames, cc->n_threads);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    // Dump the post-frontend mel in the reference's contract shape
    // [n_mels, T_mel]. The batched graph input is a reshaped view of
    // the same data, so the comparison point lives on the host.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { mel_n_mels, mel_n_frames };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "frontend.mel.norm");
    }

    // Compute encoder timing + reject unsupported shapes.
    EncoderTiming timing = compute_encoder_timing(mel_n_frames, cm->hparams);
    if (timing.n_chunks <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr run: encoder timing is degenerate "
                "(n_mel_frames=%d)",
                mel_n_frames);
        return TRANSCRIBE_ERR_GGUF;
    }

    // Reset per-call compute state. The phase timers below break out
    // per-run cost (graph build, sched alloc, uploads, prefill compute)
    // that the public transcribe_timings (mel/encode/decode) doesn't
    // expose, for the TRANSCRIBE_PERF_DEBUG breakdown.
    const int64_t t_enc_build_start    = ggml_time_us();
    int64_t       t_enc_build_us       = 0;
    int64_t       t_enc_d2h_us         = 0;
    int64_t       t_prefill_build_us   = 0;
    int64_t       t_prefill_compute_us = 0;
    int64_t       t_prefill_logits_us  = 0;
    int64_t       t_step_loop_us       = 0;
    int           n_steps              = 0;

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
        if (cc->compute_ctx == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: ggml_init for compute_ctx failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Build encoder graph.
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, timing, cc->encoder_use_flash);
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Allocate + compute encoder graph.
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "qwen3_asr run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    // Pack + upload mel.
    std::vector<float> mel_batched;
    pack_mel_chunks(cc->mel_buf.data(), mel_n_mels, mel_n_frames, timing, mel_batched);
    ggml_backend_tensor_set(eb.mel_in, mel_batched.data(), 0, mel_batched.size() * sizeof(float));

    // Positional embedding.
    {
        std::vector<float> pe = build_sinusoid_pe(cm->hparams.enc_d_model, timing.per_chunk_aftercnn);
        ggml_backend_tensor_set(eb.pos_emb_in, pe.data(), 0, pe.size() * sizeof(float));
    }

    // Attention mask (block-diagonal from cu_seqlens).
    {
        std::vector<float> mask = build_cu_seqlens_mask(timing, cm->hparams);
        if (cc->encoder_use_flash) {
            std::vector<ggml_fp16_t> mask_f16(mask.size());
            for (size_t i = 0; i < mask.size(); ++i) {
                mask_f16[i] = ggml_fp32_to_fp16(mask[i]);
            }
            ggml_backend_tensor_set(eb.mask_in, mask_f16.data(), 0, mask_f16.size() * sizeof(ggml_fp16_t));
        } else {
            ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(float));
        }
    }

    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    t_enc_build_us            = t_enc_start - t_enc_build_start;
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: encoder graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // Dump encoder intermediates.
    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.subsample.out", eb.dumps.subsample_out, "enc.subsample");
    try_dump("enc.pos_add.out", eb.dumps.pos_add_out, "enc.pos_add");
    try_dump("enc.block.0.out", eb.dumps.block_0_out, "enc.block.0");
    {
        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", cm->hparams.enc_n_layers - 1);
        try_dump(bname, eb.dumps.block_last_out, "enc.block.last");
    }
    try_dump("enc.ln_post.out", eb.dumps.ln_post_out, "enc.ln_post");
    try_dump("enc.proj.out", eb.dumps.proj_out, "enc.proj");

    // Read encoder output to host for the LM prefill. The graph already
    // dropped the aftercnn pad rows (see encoder.cpp), so eb.out is exactly
    // [d_enc, T_enc] — the reference's `padded_embed[padded_mask_after_cnn]`
    // shape.
    const int d_enc = static_cast<int>(eb.out->ne[0]);
    const int T_enc = static_cast<int>(eb.out->ne[1]);
    cc->enc_host.resize(static_cast<size_t>(d_enc) * static_cast<size_t>(T_enc));
    const int64_t t_d2h_start = ggml_time_us();
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
    t_enc_d2h_us = ggml_time_us() - t_d2h_start;

    // Decode phase begins. t_dec_start covers prompt + KV init + prefill
    // build/compute + step loop (prefill is part of "decode" to users).
    const int64_t t_dec_start           = ggml_time_us();
    const int64_t t_prefill_build_start = t_dec_start;

    // Prompt construction.
    std::vector<int32_t> prompt_ids;
    std::vector<int64_t> audio_positions;
    build_prompt_tokens(cm->hparams, cm->chat_tokens, T_enc, lang_prefix_ptr, prompt_ids, audio_positions);
    const int T_prompt   = static_cast<int>(prompt_ids.size());
    const int prefix_len = audio_positions.empty() ? 0 : static_cast<int>(audio_positions.front());
    const int suffix_len = T_prompt - prefix_len - T_enc;
    (void) audio_positions;

    // Input-length gate: audio + prompt + generation must fit the decoder
    // context window. Reject an over-length clip here, before prefill/decode.
    const int ceiling = qwen3_context_ceiling(cc->n_ctx, cm->hparams);
    const int max_new = qwen3_max_new_tokens(params);
    if (T_prompt + max_new > ceiling) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "qwen3_asr run: input too long — %d audio + %d prompt tokens "
                            "leave no room for output within the %d-token context (need %d). "
                            "Shorten the audio (see transcribe_capabilities.max_audio_ms) or "
                            "split it into segments.",
                            T_enc, prefix_len + suffix_len, ceiling, T_prompt + max_new);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // KV cache init (grow-to-fit, clamped to the context ceiling). Size to
    // hold prompt + generation budget, rounded up to a power of two (the step
    // graph's flash-attn path wants pow2 attention width). A pre-allocated
    // smaller cache is freed and re-allocated.
    int want_n_ctx = 1024;
    while (want_n_ctx < T_prompt + max_new) {
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
                                "qwen3_asr run: KV cache allocation failed (n_ctx=%d, "
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

    // Prefill graph. slice_last false: last block's FFN + final norm run on
    // every position (needed for dump parity). true: slice to just the final
    // position before the last FFN (llama.cpp's inp_out_ids trick, ~25 ms).
    const bool   dumps_on   = transcribe::debug::enabled();
    const bool   slice_last = !dumps_on;
    PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, T_prompt, T_enc,
                                          prefix_len, suffix_len,
                                          /*use_flash=*/cc->decoder_use_flash, slice_last);
    if (pb.graph == nullptr || pb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Allocate + compute prefill on the same scheduler.
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "qwen3_asr run: prefill graph allocation failed (T_prompt=%d) — "
                            "out of memory. Lower transcribe_session_params.n_ctx or shorten "
                            "the audio.",
                            T_prompt);
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload prefill inputs.
    ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
    ggml_backend_tensor_set(pb.enc_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    {
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }

    {
        // Causal mask in F16 (matches pb.mask_in): row r col c is 0 if c <= r,
        // else -inf, row-major. F16 upload avoids a per-layer ggml_cast.
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

    const int64_t t_prefill_compute_start = ggml_time_us();
    t_prefill_build_us                    = t_prefill_compute_start - t_prefill_build_start;
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr run: prefill graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    t_prefill_compute_us = ggml_time_us() - t_prefill_compute_start;

    cc->kv_cache.n    = T_prompt;
    cc->kv_cache.head = T_prompt;

    // Dump dec.* intermediates.
    try_dump("dec.token_emb", pb.dumps.token_emb, "dec.token_emb");
    try_dump("dec.audio_injected", pb.dumps.audio_injected, "dec.audio_injected");
    try_dump("dec.block.0.out", pb.dumps.block_0_out, "dec.block.0");
    {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers - 1);
        try_dump(nm, pb.dumps.block_last_out, "dec.block.last");
    }
    try_dump("dec.out_before_head", pb.dumps.out_before_head, "dec.out_before_head");
    try_dump("dec.logits_raw", pb.dumps.logits_raw, "dec.logits_raw");

    // Read prefill logits + first argmax.
    const int64_t      t_prefill_logits_start = ggml_time_us();
    const int          vocab                  = cm->hparams.dec_vocab_size;
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
    t_prefill_logits_us = ggml_time_us() - t_prefill_logits_start;

    // Step loop.
    const int32_t eos_id   = cm->hparams.eos_token_id;
    int           cur_past = T_prompt;

    // Build the step graph ONCE and reuse every step, sized for the actual
    // workload (T_prompt written + up to max_new generated). Metal's flash-attn
    // kernels dispatch ~30% faster (M4 Max) when K/V ne[1] is a power of 2, so
    // round up; floor of 1024 (smaller just hits the slow-misaligned branch).
    int max_n_kv = 1024;
    while (max_n_kv < T_prompt + max_new) {
        max_n_kv *= 2;
    }
    if (max_n_kv > cc->kv_cache.n_ctx) {
        max_n_kv = cc->kv_cache.n_ctx;
    }
    const int64_t t_step_build_start = ggml_time_us();
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
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "qwen3_asr step: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, max_n_kv,
                                    /*use_flash=*/cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "qwen3_asr step: decode graph allocation failed — out of memory. "
                            "Lower transcribe_session_params.n_ctx or shorten the audio.");
        return TRANSCRIBE_ERR_OOM;
    }
    const int64_t t_step_build_once_us = ggml_time_us() - t_step_build_start;

    // Mask buffer reused host-side across steps. Starts all -inf; per step we
    // zero positions [0, cur_past] (attend) and leave the rest -inf.
    const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> step_mask(max_n_kv, mask_neg_inf);

    // Per-step timers.
    int64_t       t_step_set_us     = 0;
    int64_t       t_step_comp_us    = 0;
    int64_t       t_step_get_us     = 0;
    const int64_t t_step_loop_start = ggml_time_us();
    while (next_tok != eos_id && static_cast<int32_t>(generated_ids.size()) < max_new && cur_past + 1 <= max_n_kv) {
        const int64_t t_set0 = ggml_time_us();

        ggml_backend_tensor_set(sb.input_id_in, &next_tok, 0, sizeof(int32_t));
        const int32_t pos_val = cur_past;
        ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
        const int64_t kv_idx_val = cur_past;
        ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx_val, 0, sizeof(int64_t));

        // Mask: mark the newly-added position as attendable. Positions
        // [0, cur_past) were zeroed in prior iterations; just set the
        // new one. (On iter 0, zero everything in [0, cur_past].)
        if (cur_past == T_prompt) {
            std::fill(step_mask.begin(), step_mask.begin() + cur_past + 1, mask_zero);
        } else {
            step_mask[cur_past] = mask_zero;
        }
        ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0, static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));

        const int64_t t_set1 = ggml_time_us();
        t_step_set_us += t_set1 - t_set0;

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr step: graph compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }
        const int64_t t_comp1 = ggml_time_us();
        t_step_comp_us += t_comp1 - t_set1;

        int32_t argmax_tok = 0;
        ggml_backend_tensor_get(sb.out, &argmax_tok, 0, sizeof(int32_t));
        next_tok = argmax_tok;
        generated_ids.push_back(next_tok);
        cur_past += 1;
        cc->kv_cache.n    = cur_past + 1;
        cc->kv_cache.head = cur_past + 1;
        t_step_get_us += ggml_time_us() - t_comp1;
    }
    t_step_loop_us = ggml_time_us() - t_step_loop_start;
    n_steps        = static_cast<int>(generated_ids.size()) - 1;

    // Decode stopped at EOS (complete) or the generation budget / context width
    // (truncated). Surface the latter via transcribe_was_truncated() + WARN.
    if (next_tok != eos_id) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "qwen3_asr run: output truncated at %d tokens — decode reached the "
                            "generation budget before end-of-stream; the transcript may be "
                            "incomplete.",
                            static_cast<int>(generated_ids.size()));
    }

    // Map granular counters to the debug-print shape. With graph reuse all
    // per-step overhead collapses to tensor_set; build/alloc/ctx_reset are
    // amortized in t_step_build_once_us.
    int64_t t_step_ctx_us   = 0;
    int64_t t_step_build_us = t_step_build_once_us;
    int64_t t_step_alloc_us = 0;

    // Strip trailing EOS if present (match the reference transcript).
    if (!generated_ids.empty() && generated_ids.back() == eos_id) {
        generated_ids.pop_back();
    }

    // Decode generated ids to text (Tokenizer::decode handles the "gpt2"
    // byte-level inversion natively).
    std::string raw_text = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));

    // Parse Qwen3-ASR output: auto-detect emits "language X<asr_text>text"
    // (strip prefix); forced emits "text" (we already seeded the prefix). The
    // split is unconditional — absent prefix leaves transcript_text = raw_text.
    std::string transcript_text = raw_text;
    if (auto sep = raw_text.find("<asr_text>"); sep != std::string::npos) {
        // Auto-detect path: surface the model-picked language name as
        // detected_language (reverse-mapped to BCP-47), but only when the
        // caller did NOT supply a hint (the field reports what the model told
        // us, not what we told it).
        if (lang_prefix_ptr == nullptr) {
            constexpr const char k_prefix[] = "language ";
            const size_t         name_start = raw_text.find(k_prefix);
            if (name_start != std::string::npos && name_start < sep) {
                const size_t ns   = name_start + (sizeof(k_prefix) - 1);
                std::string  name = raw_text.substr(ns, sep - ns);
                while (!name.empty() && (name.back() == ' ' || name.back() == '\t' || name.back() == '\n')) {
                    name.pop_back();
                }
                for (const auto & e : k_qwen3_asr_language_names) {
                    if (name == e.pub_name) {
                        cc->detected_language = e.bcp47;
                        break;
                    }
                }
            }
        }
        transcript_text = raw_text.substr(sep + std::strlen("<asr_text>"));
    }

    // Write full_text + a single segment (no timestamps; TIMESTAMPS_NONE).
    cc->t_decode_us = ggml_time_us() - t_dec_start;

    // Optional perf breakdown (finer split than the public mel/encode/decode
    // timings), gated on env var.
    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        const double ms     = 1.0 / 1000.0;
        const double sum_ms = (cc->t_mel_us + t_enc_build_us + cc->t_encode_us + t_enc_d2h_us + t_prefill_build_us +
                               t_prefill_compute_us + t_prefill_logits_us + t_step_loop_us) *
                              ms;
        const double per_step_ms = (n_steps > 0) ? (t_step_loop_us * ms / n_steps) : 0.0;
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "qwen3_asr perf breakdown:\n"
                "  mel              %8.2f ms\n"
                "  enc_build        %8.2f ms  (graph + sched + uploads)\n"
                "  enc_compute      %8.2f ms\n"
                "  enc_d2h          %8.2f ms  (%d floats)\n"
                "  prefill_build    %8.2f ms  (kv_init + prompt + graph + sched + uploads)\n"
                "  prefill_compute  %8.2f ms  (T_prompt=%d)\n"
                "  prefill_logits   %8.2f ms  (readback + argmax, vocab=%d)\n"
                "  step_loop        %8.2f ms  (%d steps, %.2f ms/step)\n"
                "    ctx_reset    %8.2f ms  (%.3f ms/step)\n"
                "    graph_build  %8.2f ms  (%.3f ms/step)\n"
                "    sched_alloc  %8.2f ms  (%.3f ms/step)\n"
                "    tensor_set   %8.2f ms  (%.3f ms/step)\n"
                "    compute      %8.2f ms  (%.3f ms/step)\n"
                "    tensor_get   %8.2f ms  (%.3f ms/step)\n"
                "  ---\n"
                "  sum              %8.2f ms",
                cc->t_mel_us * ms, t_enc_build_us * ms, cc->t_encode_us * ms, t_enc_d2h_us * ms,
                static_cast<int>(cc->enc_host.size()), t_prefill_build_us * ms, t_prefill_compute_us * ms, T_prompt,
                t_prefill_logits_us * ms, vocab, t_step_loop_us * ms, n_steps, per_step_ms, t_step_ctx_us * ms,
                (n_steps > 0) ? (t_step_ctx_us * ms / n_steps) : 0.0, t_step_build_us * ms,
                (n_steps > 0) ? (t_step_build_us * ms / n_steps) : 0.0, t_step_alloc_us * ms,
                (n_steps > 0) ? (t_step_alloc_us * ms / n_steps) : 0.0, t_step_set_us * ms,
                (n_steps > 0) ? (t_step_set_us * ms / n_steps) : 0.0, t_step_comp_us * ms,
                (n_steps > 0) ? (t_step_comp_us * ms / n_steps) : 0.0, t_step_get_us * ms,
                (n_steps > 0) ? (t_step_get_us * ms / n_steps) : 0.0, sum_ms);
    }

    cc->full_text   = transcript_text;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;
    transcribe_session::SegmentEntry seg{};
    seg.text  = transcript_text;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
    cc->segments.push_back(std::move(seg));

    // A truncated decode returns OUTPUT_TRUNCATED; the partial transcript above
    // stays readable (like an aborted run).
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

// ===========================================================================
// Offline batched decode (transcribe_run_batch)
// ===========================================================================
//
// Prefill each utterance serially into its OWN slab of a batched KV cache
// (byte-identical to single-shot prefill, so it inherits correctness), then
// batch only the autoregressive step loop. Encoder + prefill stay
// per-utterance; the batch-axis math lives in the batched step graph.

namespace {

// Apply the session thread count to every backend behind the scheduler. The
// setting persists across sched_reset, so callers only need this once.
void apply_sched_threads(QwenAsrSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

// Fresh per-utterance graph arena. Frees any prior compute_ctx and inits a
// no_alloc metadata context of `mb` MiB.
transcribe_status reset_compute_ctx(QwenAsrSession * cc, int mb) {
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

// Batched encoder: mel (parallel) + one encoder graph over all B utterances
// on the batch axis. Fills enc_hosts[b] = [d_enc, T_enc[b]], T_enc[b], valid[b].
// Real-row outputs are bit-identical to encode_one per utterance.
transcribe_status encode_all_batched(QwenAsrSession *                  cc,
                                     QwenAsrModel *                    cm,
                                     const float * const *             pcm,
                                     const int *                       n_samples,
                                     int                               n,
                                     std::vector<char> &               valid,
                                     std::vector<std::vector<float>> & enc_hosts,
                                     std::vector<int> &                T_enc_out,
                                     int64_t &                         mel_us,
                                     int64_t &                         enc_us) {
    const int n_mels = cm->hparams.enc_num_mel_bins;

    // Mel (parallel across utterances).
    std::vector<std::vector<float>> mels(n);
    std::vector<int>                mel_nf(n, 0);
    int                             n_threads = cc->n_threads;
    if (n_threads <= 0) {
        n_threads = transcribe::default_n_threads();
    }
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;  // valid stays 0
        }
        int nm = 0, nf = 0;
        if (cm->mel->compute(pcm[b], static_cast<size_t>(n_samples[b]), mels[b], nm, nf, /*n_threads=*/1) !=
            TRANSCRIBE_OK) {
            return true;  // leave invalid
        }
        mel_nf[b] = nf;
        valid[b]  = 1;
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;

    // Per-utterance timing + packing geometry.
    std::vector<EncoderTiming> timings(n);
    int                        n_chunks_max = 1;
    bool                       any          = false;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        timings[b] = compute_encoder_timing(mel_nf[b], cm->hparams);
        if (timings[b].n_chunks <= 0) {
            valid[b] = 0;
            continue;
        }
        n_chunks_max = std::max(n_chunks_max, timings[b].n_chunks);
        T_enc_out[b] = timings[b].T_enc;
        any          = true;
    }
    if (!any) {
        return TRANSCRIBE_OK;  // caller emits per-row errors
    }

    // Build batched encoder graph.
    if (const transcribe_status st = reset_compute_ctx(cc, 16); st != TRANSCRIBE_OK) {
        return st;
    }
    EncoderBuildBatched eb =
        build_encoder_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, n_chunks_max, n, cc->encoder_use_flash);
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

    const int    mel_per_chunk   = cm->hparams.enc_n_window * 2;
    const int    T_per_chunk     = eb.T_per_chunk;
    const int    T_pad_max       = eb.T_pad_max;
    const size_t per_chunk_elems = static_cast<size_t>(mel_per_chunk) * n_mels;

    // Pack mel: [mel_per_chunk, n_mels, 1, n*n_chunks_max]; utterance b's
    // chunk c at N = b*n_chunks_max + c, zero-padded beyond n_chunks[b].
    {
        std::vector<float> packed(per_chunk_elems * static_cast<size_t>(n) * n_chunks_max, 0.0f);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            const EncoderTiming & t = timings[b];
            for (int c = 0; c < t.n_chunks; ++c) {
                const int    tail = (c == t.n_chunks - 1) ? t.last_chunk_real_mel : mel_per_chunk;
                const size_t nidx = static_cast<size_t>(b) * n_chunks_max + c;
                for (int m = 0; m < n_mels; ++m) {
                    const float * src =
                        mels[b].data() + static_cast<size_t>(m) * mel_nf[b] + static_cast<size_t>(c) * mel_per_chunk;
                    float * dst = packed.data() + nidx * per_chunk_elems + static_cast<size_t>(m) * mel_per_chunk;
                    std::memcpy(dst, src, static_cast<size_t>(tail) * sizeof(float));
                }
            }
        }
        ggml_backend_tensor_set(eb.mel_in, packed.data(), 0, packed.size() * sizeof(float));
    }

    // Positional embedding (shared across chunks/utterances).
    {
        std::vector<float> pe = build_sinusoid_pe(cm->hparams.enc_d_model, T_per_chunk);
        ggml_backend_tensor_set(eb.pos_emb_in, pe.data(), 0, pe.size() * sizeof(float));
    }

    // Key-pad mask [T_pad_max, T_pad_max, 1, n]: row b attends keys k < T_enc[b].
    {
        const size_t plane = static_cast<size_t>(T_pad_max) * T_pad_max;
        if (cc->encoder_use_flash) {
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
            const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(plane * n, mn);
            for (int b = 0; b < n; ++b) {
                const int     real = valid[b] ? std::max(1, T_enc_out[b]) : 1;
                ggml_fp16_t * base = mask.data() + plane * b;
                for (int q = 0; q < T_pad_max; ++q) {
                    std::fill(base + static_cast<size_t>(q) * T_pad_max,
                              base + static_cast<size_t>(q) * T_pad_max + real, mz);
                }
            }
            ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        } else {
            const float        mn = -INFINITY;
            std::vector<float> mask(plane * n, mn);
            for (int b = 0; b < n; ++b) {
                const int real = valid[b] ? std::max(1, T_enc_out[b]) : 1;
                float *   base = mask.data() + plane * b;
                for (int q = 0; q < T_pad_max; ++q) {
                    std::fill(base + static_cast<size_t>(q) * T_pad_max,
                              base + static_cast<size_t>(q) * T_pad_max + real, 0.0f);
                }
            }
            ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(float));
        }
    }

    apply_sched_threads(cc);

    const int64_t t_enc0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc0;

    // Readback [d_enc, T_pad_max, n]; slice each utterance's first T_enc[b] rows.
    const int          d_enc = static_cast<int>(eb.out->ne[0]);
    std::vector<float> out_all(static_cast<size_t>(d_enc) * T_pad_max * n);
    ggml_backend_tensor_get(eb.out, out_all.data(), 0, out_all.size() * sizeof(float));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        const int te = T_enc_out[b];
        enc_hosts[b].resize(static_cast<size_t>(d_enc) * te);
        const float * src = out_all.data() + static_cast<size_t>(b) * T_pad_max * d_enc;
        std::memcpy(enc_hosts[b].data(), src, static_cast<size_t>(d_enc) * te * sizeof(float));
    }
    return TRANSCRIBE_OK;
}

// Batched prefill: one graph processes all B prompts, writing each utterance's
// KV into its slab and returning the first generated token per utterance. The
// caller must have allocated cc->kv_cache_batch with n_ctx >= max T_prompt and
// computed prompt_ids[b] / T_prompt[b] / prefix_len. Collapses B per-utterance
// prefill graph-builds + sched-allocs into one.
transcribe_status prefill_all_batched(QwenAsrSession *                          cc,
                                      QwenAsrModel *                            cm,
                                      const std::vector<char> &                 valid,
                                      const std::vector<std::vector<int32_t>> & prompt_ids,
                                      const std::vector<int> &                  T_prompt,
                                      int                                       prefix_len,
                                      const std::vector<std::vector<float>> &   enc_hosts,
                                      const std::vector<int> &                  T_enc,
                                      int                                       n,
                                      std::vector<int32_t> &                    first_tok_out) {
    int T_prompt_max = 0, T_enc_max = 0;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        T_prompt_max = std::max(T_prompt_max, T_prompt[b]);
        T_enc_max    = std::max(T_enc_max, T_enc[b]);
    }
    if (T_prompt_max == 0) {
        return TRANSCRIBE_OK;
    }
    T_enc_max = std::max(1, T_enc_max);

    if (const transcribe_status st = reset_compute_ctx(cc, 32); st != TRANSCRIBE_OK) {
        return st;
    }
    PrefillBuildBatched pb = build_prefill_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache_batch,
                                                         T_prompt_max, T_enc_max, n, cc->decoder_use_flash);
    if (pb.graph == nullptr || pb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }

    const int d_enc = cm->hparams.enc_output_dim;

    // input_ids [T_prompt_max, n] (pad 0).
    {
        std::vector<int32_t> ids(static_cast<size_t>(T_prompt_max) * n, 0);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            std::memcpy(ids.data() + static_cast<size_t>(b) * T_prompt_max, prompt_ids[b].data(),
                        static_cast<size_t>(T_prompt[b]) * sizeof(int32_t));
        }
        ggml_backend_tensor_set(pb.input_ids_in, ids.data(), 0, ids.size() * sizeof(int32_t));
    }
    // Audio injection by elementwise blend (see decoder.h): audio_dense holds
    // each utterance's enc_out embeds scattered into their prompt positions,
    // keep is 0 there and 1 elsewhere. (d_enc == dec_hidden, enforced.)
    {
        std::vector<float> audio_dense(static_cast<size_t>(d_enc) * T_prompt_max * n, 0.0f);
        std::vector<float> keep(static_cast<size_t>(T_prompt_max) * n, 1.0f);
        for (int b = 0; b < n; ++b) {
            const int te = valid[b] ? T_enc[b] : 0;
            // enc_hosts[b] is [d_enc, te] column-major; audio token j lands at
            // prompt position prefix_len+j, flat column b*T_prompt_max+pos.
            for (int j = 0; j < te; ++j) {
                const size_t dst_col = static_cast<size_t>(b) * T_prompt_max + (prefix_len + j);
                std::memcpy(audio_dense.data() + dst_col * d_enc, enc_hosts[b].data() + static_cast<size_t>(j) * d_enc,
                            static_cast<size_t>(d_enc) * sizeof(float));
                keep[dst_col] = 0.0f;
            }
        }
        ggml_backend_tensor_set(pb.audio_dense_in, audio_dense.data(), 0, audio_dense.size() * sizeof(float));
        ggml_backend_tensor_set(pb.keep_mask_in, keep.data(), 0, keep.size() * sizeof(float));
    }
    // positions [T_prompt_max] 0..T-1 (shared).
    {
        std::vector<int32_t> pos(T_prompt_max);
        for (int t = 0; t < T_prompt_max; ++t) {
            pos[t] = t;
        }
        ggml_backend_tensor_set(pb.positions_in, pos.data(), 0, pos.size() * sizeof(int32_t));
    }
    // causal mask [T_prompt_max, T_prompt_max] f16 (shared): m[q*T+k]=0 if k<=q.
    {
        const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
        const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt_max) * T_prompt_max, mn);
        for (int q = 0; q < T_prompt_max; ++q) {
            std::fill(mask.begin() + static_cast<size_t>(q) * T_prompt_max,
                      mask.begin() + static_cast<size_t>(q) * T_prompt_max + q + 1, mz);
        }
        ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
    }
    // kv_idx [T_prompt_max, n] i64: idx[t,b] = t.
    {
        std::vector<int64_t> kidx(static_cast<size_t>(T_prompt_max) * n);
        for (int b = 0; b < n; ++b) {
            for (int t = 0; t < T_prompt_max; ++t) {
                kidx[static_cast<size_t>(b) * T_prompt_max + t] = t;
            }
        }
        ggml_backend_tensor_set(pb.kv_idx_in, kidx.data(), 0, kidx.size() * sizeof(int64_t));
    }
    // last_idx [1, n] i32: each utterance's last real position.
    {
        std::vector<int32_t> lidx(n, 0);
        for (int b = 0; b < n; ++b) {
            lidx[b] = valid[b] ? (T_prompt[b] - 1) : 0;
        }
        ggml_backend_tensor_set(pb.last_idx_in, lidx.data(), 0, lidx.size() * sizeof(int32_t));
    }

    apply_sched_threads(cc);
    if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }

    std::vector<int32_t> amax(n, 0);
    ggml_backend_tensor_get(pb.out, amax.data(), 0, amax.size() * sizeof(int32_t));
    for (int b = 0; b < n; ++b) {
        first_tok_out[b] = amax[b];
    }
    return TRANSCRIBE_OK;
}

// Decode one utterance's generated token ids into a ResultSet (strip EOS,
// detokenize, parse the Qwen3-ASR "language X<asr_text>…" envelope). Mirrors
// run()'s output-parsing tail; `lang_prefix_ptr` non-null means the caller
// forced a language (so we don't surface a detected one).
transcribe_session::ResultSet finalize_utterance(QwenAsrModel *               cm,
                                                 std::vector<int32_t>         generated_ids,
                                                 const std::vector<int32_t> * lang_prefix_ptr,
                                                 int                          n_samples) {
    transcribe_session::ResultSet rs;
    const int32_t                 eos_id = cm->hparams.eos_token_id;
    if (!generated_ids.empty() && generated_ids.back() == eos_id) {
        generated_ids.pop_back();
    }

    std::string raw_text = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));

    std::string transcript_text = raw_text;
    if (auto sep = raw_text.find("<asr_text>"); sep != std::string::npos) {
        if (lang_prefix_ptr == nullptr) {
            constexpr const char k_prefix[] = "language ";
            const size_t         name_start = raw_text.find(k_prefix);
            if (name_start != std::string::npos && name_start < sep) {
                const size_t ns   = name_start + (sizeof(k_prefix) - 1);
                std::string  name = raw_text.substr(ns, sep - ns);
                while (!name.empty() && (name.back() == ' ' || name.back() == '\t' || name.back() == '\n')) {
                    name.pop_back();
                }
                for (const auto & e : k_qwen3_asr_language_names) {
                    if (name == e.pub_name) {
                        rs.detected_language = e.bcp47;
                        break;
                    }
                }
            }
        }
        transcript_text = raw_text.substr(sep + std::strlen("<asr_text>"));
    }

    rs.full_text   = transcript_text;
    rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    rs.has_result  = true;
    rs.status      = TRANSCRIBE_OK;
    transcribe_session::SegmentEntry seg{};
    seg.text  = transcript_text;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
    rs.segments.push_back(std::move(seg));
    return rs;
}

// Serial fallback: run() per utterance, capturing each into batch_results.
// Used when batched decode is unavailable (flash off) or as the safe path.
transcribe_status run_batch_serial(QwenAsrSession *              cc,
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

    auto * cc = static_cast<QwenAsrSession *>(session);
    auto * cm = static_cast<QwenAsrModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (!cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Batched decode requires the flash-attention step path and dump-free
    // operation. Fall back to the serial loop otherwise (same results).
    if (!cc->decoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();

    // Shared language hint (v1: one run_params across the batch).
    std::vector<int32_t>         lang_prefix_ids;
    const std::vector<int32_t> * lang_prefix_ptr = nullptr;
    if (params != nullptr && params->language != nullptr && params->language[0] != '\0') {
        if (encode_language_prefix(cm->tok, params->language, lang_prefix_ids) != TRANSCRIBE_OK) {
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
        lang_prefix_ptr = &lang_prefix_ids;
    }

    // Pass 1: per-utterance encoder + prefill into KV slabs.
    std::vector<std::vector<int32_t>> generated(n);
    std::vector<int>                  T_prompt(n, 0);
    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, 0);
    std::vector<char>                 valid(n, 0);  // utterance produced a usable prefill
    int64_t                           mel_us = 0, enc_us = 0;

    // Encode every utterance first so we can size the batched cache to the
    // real max prompt length before allocating it.
    std::vector<std::vector<float>> enc_hosts(n);
    std::vector<int>                T_enc(n, 0);
    const int64_t                   t_encpass0 = ggml_time_us();
    if (const transcribe_status st =
            encode_all_batched(cc, cm, pcm, n_samples, n, valid, enc_hosts, T_enc, mel_us, enc_us);
        st != TRANSCRIBE_OK) {
        return st;
    }
    const int64_t enc_pass_us = ggml_time_us() - t_encpass0;

    // Prompt length bound → max_n_kv and batched-cache n_ctx. Build and keep
    // each utterance's prompt token ids for the batched prefill.
    const int                         max_new      = qwen3_max_new_tokens(params);
    int                               max_T_prompt = 0;
    int                               prefix_len   = 0;
    // Per-utterance terminal status for rejected rows. Defaults to INVALID_ARG;
    // over-length rows below are upgraded to INPUT_TOO_LONG.
    const int                         ceiling      = qwen3_context_ceiling(cc->n_ctx, cm->hparams);
    std::vector<transcribe_status>    fail_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<int32_t>> prompt_ids(n);
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        std::vector<int64_t> ap;
        build_prompt_tokens(cm->hparams, cm->chat_tokens, T_enc[b], lang_prefix_ptr, prompt_ids[b], ap);
        T_prompt[b] = static_cast<int>(prompt_ids[b].size());
        prefix_len  = ap.empty() ? 0 : static_cast<int>(ap.front());
        // Same gate as single-shot run(); the rest of the batch still runs.
        if (T_prompt[b] + max_new > ceiling) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "qwen3_asr run_batch: utterance %d input too long — %d audio + "
                                "%d prompt tokens exceed the %d-token context. See "
                                "transcribe_capabilities.max_audio_ms.",
                                b, T_enc[b], T_prompt[b] - T_enc[b], ceiling);
            valid[b]       = 0;
            fail_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
            continue;
        }
        max_T_prompt = std::max(max_T_prompt, T_prompt[b]);
    }
    if (max_T_prompt == 0) {
        // No usable utterance — emit per-row errors and return.
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }
    int max_n_kv = 1024;
    while (max_n_kv < max_T_prompt + max_new) {
        max_n_kv *= 2;
    }
    // Clamp the pow2 round-up to the ceiling (the per-utterance gate guarantees
    // every valid row still fits).
    if (max_n_kv > ceiling) {
        max_n_kv = ceiling;
    }

    // Allocate / reuse the batched KV cache (n_ctx == max_n_kv, n slabs).
    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx != max_n_kv) {
        cc->kv_cache_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_cache_batch, cm->plan.primary, max_n_kv,
                                                    cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                                    cm->hparams.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "qwen3_asr run_batch: batched KV cache allocation failed "
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

    const int64_t t_prefpass0 = ggml_time_us();
    {
        std::vector<int32_t> first_tok(n, 0);
        if (const transcribe_status st =
                prefill_all_batched(cc, cm, valid, prompt_ids, T_prompt, prefix_len, enc_hosts, T_enc, n, first_tok);
            st != TRANSCRIBE_OK) {
            return st;
        }
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            n_past[b]   = T_prompt[b];
            next_tok[b] = first_tok[b];
            generated[b].push_back(first_tok[b]);
        }
    }
    const int64_t prefill_pass_us = ggml_time_us() - t_prefpass0;

    // Pass 2: batched step loop (shared causal_lm driver).
    const int32_t eos_id = cm->hparams.eos_token_id;

    if (const transcribe_status st = reset_compute_ctx(cc, 16); st != TRANSCRIBE_OK) {
        return st;
    }
    StepBuildBatched sb = build_step_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache_batch,
                                                   max_n_kv, n, cc->decoder_use_flash);
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
    const int     n_steps = step_stats.n_steps;

    // Capture per-utterance results.
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        transcribe_session::ResultSet rs = finalize_utterance(cm, generated[b], lang_prefix_ptr, n_samples[b]);
        // Per-utterance truncation parity with the single-shot path.
        if (b < static_cast<int>(truncated.size()) && truncated[b]) {
            cc->was_truncated = true;
            rs.status         = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = step_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }

    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "qwen3_asr run_batch: n=%d valid=%d max_n_kv=%d steps=%d (all phases batched x%d)\n"
                "  enc_pass=%.1fms (mel=%.1f parallel + enc_compute=%.1f, 1 graph)\n"
                "  prefill_pass=%.1fms (1 batched graph: build/sched/compute/readback)\n"
                "  step_loop=%.1fms (%.2fms/step)",
                n, valid_count, max_n_kv, n_steps, valid_count, enc_pass_us / 1000.0, mel_us / 1000.0, enc_us / 1000.0,
                prefill_pass_us / 1000.0, step_us / 1000.0, n_steps > 0 ? step_us / 1000.0 / n_steps : 0.0);
    }

    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "qwen3_asr",
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

}  // namespace transcribe::qwen3_asr
