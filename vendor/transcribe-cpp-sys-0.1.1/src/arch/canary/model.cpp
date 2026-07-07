// arch/canary/model.cpp - Canary multitask AED family handler.
//
// Load / init_context / run lifecycle for the four canary variants
// (180m-flash, 1b-flash, 1b-v2, 1b): FastConformer encoder + autoregressive
// Transformer decoder, driven by a 4/5-slot multitask prompt.

#include "canary.h"
#include "decoder.h"
#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
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
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

namespace transcribe::canary {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, CanaryModel>);
static_assert(std::is_base_of_v<transcribe_session, CanarySession>);

CanarySession::~CanarySession() {
    kv_cache.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
    encoder_out = nullptr;
}

bool kv_cache_init(CanaryKvCache & cache,
                   ggml_backend_t  backend,
                   int             n_ctx,
                   int             T_enc,
                   int             n_state,
                   int             n_layer,
                   ggml_type       kv_type) {
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary kv_cache: unsupported kv_type=%d", static_cast<int>(kv_type));
        return false;
    }

    const size_t ctx_size = 4 * ggml_tensor_overhead() + 256;

    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary kv_cache: ggml_init failed");
        return false;
    }

    const int64_t self_elements  = static_cast<int64_t>(n_state) * n_layer * n_ctx;
    const int64_t cross_elements = static_cast<int64_t>(n_state) * n_layer * T_enc;

    cache.self_k  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.self_v  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.cross_k = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    cache.cross_v = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);

    ggml_set_name(cache.self_k, "kv_self_k");
    ggml_set_name(cache.self_v, "kv_self_v");
    ggml_set_name(cache.cross_k, "kv_cross_k");
    ggml_set_name(cache.cross_v, "kv_cross_v");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary kv_cache: buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }

    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx           = n_ctx;
    cache.T_enc           = T_enc;
    cache.n               = 0;
    cache.head            = 0;
    cache.cross_populated = false;

    const size_t total_bytes =
        ggml_nbytes(cache.self_k) + ggml_nbytes(cache.self_v) + ggml_nbytes(cache.cross_k) + ggml_nbytes(cache.cross_v);
    log_msg(TRANSCRIBE_LOG_LEVEL_INFO,
            "canary kv_cache: allocated %.1f MB (%s) "
            "(self: %d session x %d layers, cross: %d T_enc x %d layers)",
            static_cast<double>(total_bytes) / (1024.0 * 1024.0), ggml_type_name(kv_type), n_ctx, n_layer, T_enc,
            n_layer);

    return true;
}

bool kv_cache_init_batched(CanaryKvCache & cache,
                           ggml_backend_t  backend,
                           int             n_ctx,
                           int             T_enc,
                           int             n_state,
                           int             n_layer,
                           int             n_batch,
                           ggml_type       kv_type) {
    if (n_batch <= 1) {
        if (!kv_cache_init(cache, backend, n_ctx, T_enc, n_state, n_layer, kv_type)) {
            return false;
        }
        cache.n_batch = 1;
        return true;
    }
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary kv_cache(batched): unsupported kv_type");
        return false;
    }
    const size_t     ctx_size = 4 * ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;
    cache.ctx         = ggml_init(params);
    if (cache.ctx == nullptr) {
        return false;
    }

    const int64_t self_elements  = static_cast<int64_t>(n_state) * n_layer * n_ctx * n_batch;
    const int64_t cross_elements = static_cast<int64_t>(n_state) * n_layer * T_enc * n_batch;
    cache.self_k                 = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.self_v                 = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.cross_k                = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    cache.cross_v                = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    ggml_set_name(cache.self_k, "kv_self_k");
    ggml_set_name(cache.self_v, "kv_self_v");
    ggml_set_name(cache.cross_k, "kv_cross_k");
    ggml_set_name(cache.cross_v, "kv_cross_v");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);
    cache.n_ctx           = n_ctx;
    cache.T_enc           = T_enc;
    cache.n               = 0;
    cache.head            = 0;
    cache.n_batch         = n_batch;
    cache.cross_populated = false;
    return true;
}

CanaryModel::~CanaryModel() {
    if (bn_fused_ctx != nullptr) {
        ggml_free(bn_fused_ctx);
        bn_fused_ctx = nullptr;
    }
    if (bn_fused_buffer != nullptr) {
        safe_buffer_free(bn_fused_buffer);
        bn_fused_buffer = nullptr;
    }
    if (conv_pw_f32_ctx != nullptr) {
        ggml_free(conv_pw_f32_ctx);
        conv_pw_f32_ctx = nullptr;
    }
    if (conv_pw_f32_buffer != nullptr) {
        safe_buffer_free(conv_pw_f32_buffer);
        conv_pw_f32_buffer = nullptr;
    }
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

constexpr float kBnEps = 1e-5f;
constexpr int   k_default_max_new = 512;

int canary_max_new_tokens(const transcribe_run_params * params) {
    if (params != nullptr &&
        params->struct_size >= offsetof(transcribe_run_params, max_new_tokens) + sizeof(params->max_new_tokens) &&
        params->max_new_tokens > 0) {
        return params->max_new_tokens;
    }
    return k_default_max_new;
}

// Input-length contract (see docs/input-limits.md). Two limits:
//   (a) INPUT — the encoder rel-pos table (enc_pos_emb_max_len, ~400 s).
//       T_enc must stay within it or the runtime table aliases past the
//       trained range; gated up front. Drives max_audio_ms.
//   (b) DECODER self-KV (dec_max_position) + 512 max-new cap bound the
//       OUTPUT length; an overrun is kept as a partial and flagged via
//       transcribe_was_truncated(), not rejected.

// Predicted encoder frame count T_enc for a given mel frame count. The
// FastConformer pre-encode downsamples time via stride-2, kernel-3, pad-1
// convs; each stage maps T_in -> floor((T_in-1)/2)+1. We fold that exact
// per-stage recurrence so the prediction matches the graph's T_enc.
int canary_predict_t_enc(int mel_n_frames, int subsampling_factor) {
    if (mel_n_frames <= 0 || subsampling_factor <= 0) {
        return 0;
    }
    // subsampling_factor 8 == three stride-2 stages. Derive the stage
    // count from the factor (log2) so a future geometry change stays
    // honest; fall back to floor(T_mel / factor) if it is not a power of two.
    int stages = 0;
    for (int f = subsampling_factor; f > 1; f >>= 1) {
        ++stages;
    }
    if ((1 << stages) != subsampling_factor) {
        return mel_n_frames / subsampling_factor;
    }
    int t = mel_n_frames;
    for (int s = 0; s < stages; ++s) {
        t = (t - 1) / 2 + 1;  // floor((T_in - 1)/2) + 1, k=3 s=2 p=1
        if (t <= 0) {
            return 0;
        }
    }
    return t;
}

// Advisory max_audio_ms: longest audio whose T_enc still fits
// enc_pos_emb_max_len, inverting the rate
//   ms = T_enc * subsampling_factor * hop_length * 1000 / sr
// at T_enc == enc_pos_emb_max_len. Returns 0 (unknown) if any rate hparam
// is missing, so a misconfigured model is never advertised with a wrong number.
int64_t canary_max_audio_ms(const CanaryHParams & hp) {
    if (hp.enc_pos_emb_max_len <= 0 || hp.enc_subsampling_factor <= 0 || hp.fe_hop_length <= 0 ||
        hp.fe_sample_rate <= 0) {
        return 0;
    }
    const int64_t mel_frames = static_cast<int64_t>(hp.enc_pos_emb_max_len) * hp.enc_subsampling_factor;
    return mel_frames * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
}

// Effective decoder self-KV ceiling, in tokens: dec_max_position, optionally
// lowered — never raised — by the caller's session n_ctx knob.
int canary_context_ceiling(int32_t n_ctx_knob, const CanaryHParams & hp) {
    int ceiling = hp.dec_max_position > 0 ? hp.dec_max_position : 1024;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Fuse inference-time BatchNorm into precomputed scale + bias. Same as
// parakeet/cohere — see those files for the math.
transcribe_status fuse_batch_norm(CanaryModel & m) {
    const size_t n_blocks = m.weights.blocks.size();
    if (n_blocks == 0) {
        return TRANSCRIBE_OK;
    }

    const int64_t d            = m.hparams.enc_d_model;
    const size_t  tensor_bytes = static_cast<size_t>(d) * sizeof(float);

    const size_t     ctx_size = n_blocks * 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params   = { ctx_size, nullptr, true };
    m.bn_fused_ctx            = ggml_init(params);
    if (m.bn_fused_ctx == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b              = m.weights.blocks[i];
        b.conv_bn_fused_scale = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
        b.conv_bn_fused_bias  = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
    }

    m.bn_fused_buffer = ggml_backend_alloc_ctx_tensors(m.bn_fused_ctx, m.plan.scheduler_list.back());
    if (m.bn_fused_buffer == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    std::vector<float> bn_w(d), bn_b(d), rm(d), rv(d);
    std::vector<float> fused_s(d), fused_b(d);

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b = m.weights.blocks[i];
        ggml_backend_tensor_get(b.conv_bn_w, bn_w.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_b, bn_b.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_rm, rm.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_rv, rv.data(), 0, tensor_bytes);

        for (int64_t c = 0; c < d; ++c) {
            const float s = bn_w[c] / std::sqrt(rv[c] + kBnEps);
            fused_s[c]    = s;
            fused_b[c]    = bn_b[c] - rm[c] * s;
        }

        ggml_backend_tensor_set(b.conv_bn_fused_scale, fused_s.data(), 0, tensor_bytes);
        ggml_backend_tensor_set(b.conv_bn_fused_bias, fused_b.data(), 0, tensor_bytes);
    }

    return TRANSCRIBE_OK;
}

// On CPU primary backend, dequantize 1x1 conformer pointwise convs
// from F16 to F32. Same rationale as parakeet/cohere.
transcribe_status promote_conv_pw_to_f32_on_cpu(CanaryModel & m) {
    std::vector<load_common::ConvPwF32Slot> slots;
    slots.reserve(m.weights.blocks.size() * 2);
    for (auto & b : m.weights.blocks) {
        if (b.conv_pw1_w != nullptr && b.conv_pw1_w->type == GGML_TYPE_F16) {
            slots.push_back({ &b.conv_pw1_w, b.conv_pw1_w });
        }
        if (b.conv_pw2_w != nullptr && b.conv_pw2_w->type == GGML_TYPE_F16) {
            slots.push_back({ &b.conv_pw2_w, b.conv_pw2_w });
        }
    }
    return load_common::promote_conv_pw_f16_to_f32_on_cpu(m.plan, slots, "canary", &m.conv_pw_f32_ctx,
                                                          &m.conv_pw_f32_buffer);
}

constexpr const char k_default_variant[] = "canary";

extern transcribe_status load(Loader &, const transcribe_model_load_params *, transcribe_model **);
extern transcribe_status init_context(transcribe_model *, const transcribe_session_params *, transcribe_session **);
extern transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<CanaryModel>();
    m->arch      = &arch;
    m->t_load_us = 0;

    if (loader.variant().empty()) {
        m->variant = k_default_variant;
    } else {
        m->variant = loader.variant();
    }
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

    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    if (const transcribe_status st = read_canary_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the encoder positional span
    // and frontend rate are known (apply_family_invariants ran before the
    // hparams were read). The encoder is the binding INPUT limit for canary.
    m->caps.max_audio_ms = canary_max_audio_ms(m->hparams);

    // Basis for transcribe_session_get_limits. audio_from_caps = true pins
    // effective_max_audio_ms to the encoder bound regardless of n_ctx; the
    // decoder self-KV (which n_ctx does lower) only bounds transcript length.
    if (m->hparams.dec_max_position > 0) {
        m->limits.has_context_cap        = true;
        m->limits.audio_from_caps        = true;
        m->limits.model_max_ctx          = m->hparams.dec_max_position;
        m->limits.gen_reserve            = 512;  // run()'s max-new-tokens cap
        // Whisper-style decoder self-KV: dec_d_model per layer, K and V, no GQA.
        m->limits.kv_elems_per_ctx_token = (int64_t) m->hparams.dec_d_model * m->hparams.dec_n_layers * 2;
    }

    // Cross-check tokenizer and KV-declared vocab sizes match.
    {
        const int tok_vocab = m->tok.n_tokens();
        if (tok_vocab > 0 && tok_vocab != m->hparams.dec_vocab_size) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: tokenizer vocab (%d) != stt.canary.decoder.vocab_size (%d)",
                    tok_vocab, m->hparams.dec_vocab_size);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();
    if (m->hparams.eos_token_id < 0) {
        // Fall back to canary's named special: <|endoftext|>.
        m->hparams.eos_token_id = m->hparams.endoftext_id;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary: GGUF tokenizer has no eos_token_id and no "
                "stt.canary.special.endoftext_id — regenerate GGUF");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Mel frontend.
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
        // window_type defaults to "hann_symmetric" (NeMo behavior); the
        // GGUF stt.frontend.window value is currently always "hann"
        // and is treated as the family of Hann window only — the
        // periodic/symmetric distinction is not carried in the GGUF
        // today and the reference uses the symmetric variant.

        {
            using R = load_common::ReadF32Result;

            const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
            const auto   fb_rc    = load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "canary", cfg.filterbank);
            if (fb_rc != R::Ok && fb_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }

            const size_t win_elems = static_cast<size_t>(cfg.win_length);
            const auto   win_rc = load_common::read_f32_tensor_checked(loader.gguf(), loader.path(), "frontend.window",
                                                                       win_elems, "canary", cfg.window);
            if (win_rc != R::Ok && win_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
        }

        m->mel.emplace(cfg);
    }

    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = build_canary_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;

    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "canary", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "canary");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    gguf_free(gguf_data);

    if (const transcribe_status st = fuse_batch_norm(*m); st != TRANSCRIBE_OK) {
        return st;
    }
    if (const transcribe_status st = promote_conv_pw_to_f32_on_cpu(*m); st != TRANSCRIBE_OK) {
        return st;
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

    auto cc       = std::make_unique<CanarySession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    // Cache the caller's n_ctx knob. For canary this lowers the decoder
    // self-KV ceiling only; it does not affect the encoder input limit.
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    auto *     cm       = static_cast<CanaryModel *>(model);
    const bool is_metal = (cm->plan.primary_kind == transcribe::BackendKind::Metal);

    cc->encoder_use_flash = !is_metal;
    cc->decoder_use_flash = true;

    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Build the multitask prompt token sequence. Each slot's value comes from the
// GGUF's stt.canary.special.*_id catalog; returns {} if a required slot is
// missing. itn / timestamp / diarize are hardwired off; only pnc is toggleable.

// canary-1 (4-slot), task token explicit:
//   <|startoftranscript|> <|src_lang|> <|task|> <|target_lang|> <|pnc|>
// <|task|> is <|translate|> when src != tgt (and available), else <|transcribe|>.
std::vector<int32_t> build_prompt_canary(const CanaryHParams & hp,
                                         int                   src_lang_id,
                                         int                   tgt_lang_id,
                                         const char *          task,
                                         bool                  pnc) {
    std::vector<int32_t> ids;
    ids.reserve(8);

    if (hp.startoftranscript_id < 0 || src_lang_id < 0 || tgt_lang_id < 0) {
        return {};
    }
    ids.push_back(hp.startoftranscript_id);
    ids.push_back(src_lang_id);

    const bool is_translate = (task != nullptr && std::strcmp(task, "translate") == 0) || (src_lang_id != tgt_lang_id);
    int        task_id      = -1;
    if (is_translate && hp.translate_id >= 0) {
        task_id = hp.translate_id;
    } else if (hp.transcribe_id >= 0) {
        task_id = hp.transcribe_id;
    } else {
        return {};
    }
    ids.push_back(task_id);
    ids.push_back(tgt_lang_id);

    const int pnc_slot = pnc ? hp.pnc_id : hp.nopnc_id;
    if (pnc_slot < 0) {
        return {};
    }
    ids.push_back(pnc_slot);
    return ids;
}

std::vector<int32_t> build_prompt_canary2(const CanaryModel &   cm,
                                          const CanaryHParams & hp,
                                          int                   src_lang_id,
                                          int                   tgt_lang_id,
                                          const char * /*task*/,
                                          bool pnc) {
    // canary2 prompt template:
    //   <|startofcontext|> [decodercontext] <|startoftranscript|>
    //   <|emo:?|> <|src_lang|> <|tgt_lang|> <|pnc|> <|itn|> <|timestamp|> <|diarize|>
    // ASR with empty decoder context realizes 9 tokens.
    std::vector<int32_t> ids;
    ids.reserve(9);

    if (hp.startofcontext_id < 0 || hp.startoftranscript_id < 0 || src_lang_id < 0 || tgt_lang_id < 0) {
        return {};
    }

    // Emotion is not in the named-special KV catalog; look it up via
    // the tokenizer (every canary2 SP vocab carries <|emo:undefined|>).
    const int emo_id = cm.tok.find("<|emo:undefined|>");
    if (emo_id < 0) {
        return {};
    }

    // Single-SP canary2 (canary-1b-v2): NeMo's prompt formatter renders
    // the empty `decodercontext` slot as one SP whitespace marker (`▁`).
    // Aggregate-tokenizer variants drop the empty slot entirely.
    if (hp.tokenizer_single_sp) {
        const int sp_space_id = cm.tok.find("\xe2\x96\x81");  // U+2581 LOWER ONE EIGHTH BLOCK = ▁
        if (sp_space_id < 0) {
            return {};
        }
        ids.push_back(sp_space_id);
    }

    ids.push_back(hp.startofcontext_id);
    ids.push_back(hp.startoftranscript_id);
    ids.push_back(emo_id);
    ids.push_back(src_lang_id);
    ids.push_back(tgt_lang_id);

    const int pnc_slot = pnc ? hp.pnc_id : hp.nopnc_id;
    if (pnc_slot < 0) {
        return {};
    }
    ids.push_back(pnc_slot);

    if (hp.noitn_id < 0) {
        return {};
    }
    ids.push_back(hp.noitn_id);

    if (hp.notimestamp_id < 0) {
        return {};
    }
    ids.push_back(hp.notimestamp_id);

    if (hp.nodiarize_id < 0) {
        return {};
    }
    ids.push_back(hp.nodiarize_id);

    return ids;
}

int find_language_id(const CanaryHParams & hp, const char * lang) {
    if (lang == nullptr) {
        return -1;
    }
    for (size_t i = 0; i < hp.languages.size() && i < hp.language_ids.size(); ++i) {
        if (hp.languages[i] == lang) {
            return hp.language_ids[i];
        }
    }
    return -1;
}

bool translation_pair_allowed(const CanaryHParams & hp, const char * src, const char * dst) {
    if (hp.translation_pairs.empty()) {
        return true;
    }
    if (src == nullptr || dst == nullptr || src[0] == '\0' || dst[0] == '\0') {
        return false;
    }
    const std::string pair = std::string(src) + ">" + dst;
    for (const auto & allowed : hp.translation_pairs) {
        if (allowed == pair) {
            return true;
        }
    }
    return false;
}

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * cc = static_cast<CanarySession *>(session);
    auto * cm = static_cast<CanaryModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    // Mel front-end.
    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (const transcribe_status mst =
            cm->mel->compute(pcm, static_cast<size_t>(n_samples), cc->mel_buf, mel_n_mels, mel_n_frames);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    // Input-length gate: T_enc must stay within enc_pos_emb_max_len or the
    // runtime pos table aliases past the trained range. T_enc is a
    // deterministic function of mel_n_frames, so reject an over-length clip
    // here, before building the encoder graph.
    if (cm->hparams.enc_pos_emb_max_len > 0) {
        const int t_enc_pred = canary_predict_t_enc(mel_n_frames, cm->hparams.enc_subsampling_factor);
        if (t_enc_pred > cm->hparams.enc_pos_emb_max_len) {
            const double max_s = static_cast<double>(cm->caps.max_audio_ms) / 1000.0;
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: input too long — %d encoder frames exceed the %d "
                                "the model supports (~%.0f s max). See "
                                "transcribe_capabilities.max_audio_ms.",
                                t_enc_pred, cm->hparams.enc_pos_emb_max_len, max_s);
            return TRANSCRIBE_ERR_INPUT_TOO_LONG;
        }
    }

    // Reset compute state.
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    cc->encoder_out = nullptr;

    // Build encoder graph.
    {
        ggml_init_params init_params{};
        init_params.mem_size   = 8 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        cc->compute_ctx        = ggml_init(init_params);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: compute context allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, mel_n_frames, resolved_kv,
                                          cc->encoder_use_flash, cm->backend.c_str());
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, cc->mel_buf.data(), 0, cc->mel_buf.size() * sizeof(float));
    transcribe::debug::dump_tensor("enc.mel.in", eb.mel_in, "encoder.mel");

    // Sinusoidal relative positional embedding.
    if (eb.pos_emb_in != nullptr) {
        const int d_model = cm->hparams.enc_d_model;
        const int pos_len = static_cast<int>(eb.pos_emb_in->ne[1]);
        const int T_enc   = (pos_len + 1) / 2;

        cc->pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);

        cc->pos_div_term.resize(static_cast<size_t>(d_model / 2));
        const float ln_10000 = std::log(10000.0f);
        for (int k = 0; k < d_model / 2; ++k) {
            cc->pos_div_term[static_cast<size_t>(k)] =
                std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
        }

        for (int i = 0; i < pos_len; ++i) {
            const float pos = static_cast<float>((T_enc - 1) - i);
            float *     row = cc->pos_buf.data() + static_cast<size_t>(i) * d_model;
            for (int k = 0; k < d_model / 2; ++k) {
                const float div = cc->pos_div_term[static_cast<size_t>(k)];
                row[2 * k]      = std::sin(pos * div);
                row[2 * k + 1]  = std::cos(pos * div);
            }
        }

        ggml_backend_tensor_set(eb.pos_emb_in, cc->pos_buf.data(), 0, cc->pos_buf.size() * sizeof(float));
        transcribe::debug::dump_tensor("enc.pos_emb", eb.pos_emb_in, "encoder.pos_emb");
    }

    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: encoder compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };

    try_dump("enc.pre_encode.out", eb.dumps.pre_encode_out, "encoder.pre_encode");
    try_dump("enc.block.0.out", eb.dumps.block0_out, "encoder.block0.out");
    {
        char      bname[64], stage[64];
        const int mid = cm->hparams.enc_n_layers / 2;
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", mid);
        std::snprintf(stage, sizeof(stage), "encoder.block%d.out", mid);
        try_dump(bname, eb.dumps.block_mid_out, stage);
    }
    {
        char      bname[64], stage[64];
        const int last = cm->hparams.enc_n_layers - 1;
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", last);
        std::snprintf(stage, sizeof(stage), "encoder.block%d.out", last);
        try_dump(bname, eb.dumps.block_last_out, stage);
    }
    // Native (pre-projection) and final (post-projection) encoder outs.
    try_dump("enc.native", eb.dumps.native_out, "encoder.native_out");
    try_dump("enc.final", eb.dumps.final_out, "encoder.final");

    cc->encoder_out = eb.out;

    // Pull encoder output to host so the cross-attn graph can consume it.
    const int d_dec = static_cast<int>(eb.out->ne[0]);
    const int T_enc = static_cast<int>(eb.out->ne[1]);
    if (d_dec <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: encoder output has degenerate shape [%d, %d]", d_dec, T_enc);
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->enc_host.resize(static_cast<size_t>(d_dec) * static_cast<size_t>(T_enc));
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    // Build multitask prompt.
    const char * lang         = (params && params->language) ? params->language : "en";
    const bool   is_translate = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE);
    const char * tgt_lang     = lang;
    if (is_translate && params && params->target_language) {
        tgt_lang = params->target_language;
    }
    const char * task = is_translate ? "translate" : "asr";

    const int src_id = find_language_id(cm->hparams, lang);
    const int tgt_id = find_language_id(cm->hparams, tgt_lang);
    if (src_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary run: source language '%s' has no token id "
                "(model supports %zu langs)",
                lang, cm->hparams.languages.size());
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    if (tgt_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary run: target language '%s' has no token id "
                "(model supports %zu langs)",
                tgt_lang, cm->hparams.languages.size());
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    if (is_translate && !translation_pair_allowed(cm->hparams, lang, tgt_lang)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: translation pair '%s>%s' is not advertised", lang, tgt_lang);
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }

    // Generic transcribe_run_params::pnc routes here. DEFAULT maps to the
    // model's shipped behavior (pnc=on; matches the upstream model card's
    // published WER numbers). OFF / ON override explicitly. Non-DEFAULT
    // requests have already passed the dispatcher's advisory-warn gate
    // (which only warns when transcribe_model_supports(model,
    // TRANSCRIBE_FEATURE_PNC) is false; canary sets TRANSCRIBE_FEATURE_PNC
    // so the probe returns true and no warn fires here).
    bool pnc = true;
    if (params != nullptr) {
        switch (params->pnc) {
            case TRANSCRIBE_PNC_MODE_DEFAULT:
                pnc = true;
                break;
            case TRANSCRIBE_PNC_MODE_OFF:
                pnc = false;
                break;
            case TRANSCRIBE_PNC_MODE_ON:
                pnc = true;
                break;
        }
    }

    std::vector<int32_t> prompt_ids;
    if (cm->hparams.prompt_format == "canary2") {
        prompt_ids = build_prompt_canary2(*cm, cm->hparams, src_id, tgt_id, task, pnc);
    } else if (cm->hparams.prompt_format == "canary") {
        prompt_ids = build_prompt_canary(cm->hparams, src_id, tgt_id, task, pnc);
    }
    if (prompt_ids.empty()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: failed to build prompt for format='%s'",
                cm->hparams.prompt_format.c_str());
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int prompt_len = static_cast<int>(prompt_ids.size());

    // Init KV cache.
    {
        if (cc->kv_cache.buffer != nullptr && cc->kv_cache.T_enc != T_enc) {
            cc->kv_cache.free();
        }
        if (cc->kv_cache.buffer == nullptr) {
            // Decoder self-KV ceiling: the model's trained dec_max_position,
            // optionally lowered (never raised) by the caller's n_ctx knob.
            // With the default knob (0) this is exactly dec_max_position, so
            // in-spec decode is unchanged.
            const int n_ctx      = canary_context_ceiling(cc->n_ctx, cm->hparams);
            ggml_type cache_type = resolved_kv;
            if (cache_type == GGML_TYPE_COUNT) {
                cache_type = GGML_TYPE_F16;
            }
            if (!kv_cache_init(cc->kv_cache, cm->plan.primary, n_ctx, T_enc, cm->hparams.dec_d_model,
                               cm->hparams.dec_n_layers, cache_type)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "canary run: KV cache allocation failed (n_ctx=%d, T_enc=%d, "
                                    "%d d_model x %d layers) — out of memory. Lower "
                                    "transcribe_session_params.n_ctx or shorten the audio.",
                                    n_ctx, T_enc, cm->hparams.dec_d_model, cm->hparams.dec_n_layers);
                return TRANSCRIBE_ERR_OOM;
            }
        } else {
            cc->kv_cache.n               = 0;
            cc->kv_cache.head            = 0;
            cc->kv_cache.cross_populated = false;
        }
    }

    auto new_compute_ctx = [&](size_t mem_size) -> bool {
        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        cc->encoder_out = nullptr;
        ggml_init_params ip{};
        ip.mem_size     = mem_size;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        return cc->compute_ctx != nullptr;
    };

    auto find_mask_input = [&]() -> ggml_tensor * {
        for (ggml_tensor * t = ggml_get_first_tensor(cc->compute_ctx); t != nullptr;
             t               = ggml_get_next_tensor(cc->compute_ctx, t)) {
            if (std::strcmp(t->name, "dec.causal_mask") == 0 && t->type == GGML_TYPE_F32) {
                return t;
            }
        }
        return nullptr;
    };

    // Cross-attention KV pre-compute.
    const int64_t t_dec_start = ggml_time_us();
    {
        if (!new_compute_ctx(4 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: cross-KV compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        DecoderBuild cross_db = build_cross_kv_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, T_enc);
        if (cross_db.graph == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: build_cross_kv_graph failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, cross_db.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: cross-KV graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        ggml_backend_tensor_set(cross_db.encoder_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, cross_db.graph);
            gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: cross_kv compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.cross_populated = true;
    }

    // Prompt pass + autoregressive decode.
    {
        if (!new_compute_ctx(4 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: decoder-prompt compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        // When debug-dumping, we need full softmax'd logits for the
        // pre-head dump pipeline (matches cohere's pattern). Otherwise
        // skip log_softmax and read a GPU argmax over the last position.
        const bool   prompt_skip_softmax = !transcribe::debug::enabled();
        DecoderBuild db = build_decoder_graph_kv(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, prompt_len,
                                                 /*n_past=*/0, T_enc,
                                                 /*skip_log_softmax=*/prompt_skip_softmax, cc->decoder_use_flash);
        if (db.out == nullptr || db.graph == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: build_decoder_graph_kv (prompt) failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, db.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run: decoder-prompt graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        ggml_backend_tensor_set(db.token_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
        std::vector<int32_t> pos_ids(prompt_len);
        for (int i = 0; i < prompt_len; ++i) {
            pos_ids[i] = i;
        }
        ggml_backend_tensor_set(db.pos_ids_in, pos_ids.data(), 0, pos_ids.size() * sizeof(int32_t));

        if (prompt_len > 1) {
            ggml_tensor * mask_input = find_mask_input();
            if (mask_input != nullptr) {
                const int          n_kv = prompt_len;
                std::vector<float> mask_data(static_cast<size_t>(n_kv) * prompt_len);
                for (int q = 0; q < prompt_len; ++q) {
                    for (int k = 0; k < n_kv; ++k) {
                        mask_data[static_cast<size_t>(q) * n_kv + k] = (k <= q) ? 0.0f : -1e9f;
                    }
                }
                ggml_backend_tensor_set(mask_input, mask_data.data(), 0, mask_data.size() * sizeof(float));
            }
        }

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, db.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: decoder prompt compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }

        // Per-sublayer dumps at layers {0, n_layers/2, n_layers-1}.
        for (int i = 0; i < cm->hparams.dec_n_layers; ++i) {
            const bool dump_this =
                (i == 0) || (i == cm->hparams.dec_n_layers / 2) || (i == cm->hparams.dec_n_layers - 1);
            if (!dump_this || i >= 64) {
                continue;
            }
            char bname[64], stage[64];
            std::snprintf(bname, sizeof(bname), "dec.layer.%d.self_attn.out", i);
            std::snprintf(stage, sizeof(stage), "decoder.layer%d.self_attn", i);
            try_dump(bname, db.dumps.sub_self[i], stage);
            std::snprintf(bname, sizeof(bname), "dec.layer.%d.cross_attn.out", i);
            std::snprintf(stage, sizeof(stage), "decoder.layer%d.cross_attn", i);
            try_dump(bname, db.dumps.sub_cross[i], stage);
            std::snprintf(bname, sizeof(bname), "dec.layer.%d.ffn.out", i);
            std::snprintf(stage, sizeof(stage), "decoder.layer%d.ffn", i);
            try_dump(bname, db.dumps.sub_ffn[i], stage);
        }

        cc->kv_cache.n    = prompt_len;
        cc->kv_cache.head = prompt_len;

        cc->clear_result();

        const int eos_id     = cm->hparams.eos_token_id;
        const int max_tokens = std::min(512, cc->kv_cache.n_ctx - prompt_len);

        int next_token = 0;
        if (prompt_skip_softmax && db.argmax_out != nullptr) {
            int32_t argmax_id = 0;
            ggml_backend_tensor_get(db.argmax_out, &argmax_id, 0, sizeof(int32_t));
            next_token = argmax_id;
        } else {
            const int64_t      vocab_size = db.out->ne[0];
            std::vector<float> logits_host(static_cast<size_t>(vocab_size) * prompt_len);
            ggml_backend_tensor_get(db.out, logits_host.data(), 0, logits_host.size() * sizeof(float));
            const float * last_logits = logits_host.data() + static_cast<size_t>(prompt_len - 1) * vocab_size;
            float         best        = last_logits[0];
            for (int j = 1; j < static_cast<int>(vocab_size); ++j) {
                if (last_logits[j] > best) {
                    best       = last_logits[j];
                    next_token = j;
                }
            }
        }

        std::vector<int> generated_ids;
        if (next_token != eos_id) {
            generated_ids.push_back(next_token);
        }

        int n_past = prompt_len;

        // Commit accumulated generated_ids as the run's segment + full
        // text. Called both on normal loop exit and on abort so the
        // public contract (partial result on TRANSCRIBE_ERR_ABORTED)
        // holds.
        auto commit_result = [&]() {
            cc->t_decode_us = ggml_time_us() - t_dec_start;

            if (generated_ids.empty()) {
                return;
            }

            const transcribe::Tokenizer & tok = cm->tok;

            // keep_special_tags (default false → strip): canary's vocab
            // carries language/task/PNC control tokens at CONTROL type.
            // They shouldn't appear after the prompt is consumed, but the
            // strip is defensive — and only applied when the caller wants
            // clean text. --raw-tokens / keep_special_tags=true exposes
            // whatever the decoder emitted.
            const bool       strip = (params == nullptr) ? true : !params->keep_special_tags;
            std::vector<int> text_ids;
            if (strip) {
                text_ids.reserve(generated_ids.size());
                for (int id : generated_ids) {
                    if (tok.is_control(id)) {
                        continue;
                    }
                    text_ids.push_back(id);
                }
            } else {
                text_ids.assign(generated_ids.begin(), generated_ids.end());
            }

            std::string full = tok.decode(text_ids.data(), static_cast<int>(text_ids.size()));
            if (!full.empty() && full.front() == ' ') {
                full.erase(full.begin());
            }

            transcribe_session::SegmentEntry seg;
            seg.t0_ms       = 0;
            seg.t1_ms       = 0;
            seg.first_token = 0;
            seg.n_tokens    = 0;
            seg.first_word  = 0;
            seg.n_words     = 0;
            seg.text        = full;

            cc->segments.push_back(std::move(seg));
            cc->full_text   = std::move(full);
            cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
            cc->has_result  = true;
        };

        // Two decoder loop variants, picked by primary backend kind:
        //   GPU: build_step_graph — one static-topology graph reused for the
        //     whole utterance, amortizing per-step graph_build + sched_alloc
        //     dispatch overhead.
        //   CPU: build_decoder_graph_kv per step — n_kv grows with n_past, so
        //     no dispatch overhead to amortize and no static-graph bandwidth tax.
        const bool primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                    cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                    cm->plan.primary_kind != transcribe::BackendKind::Unknown;

        if (primary_is_gpu) {
            // Static-graph step path (GPU). max_n_kv: pad to next power of two
            // with a 1024 floor — Vulkan/Metal flash-attn dispatches ~30%
            // faster on pow2 ne[1].
            int max_n_kv = 1024;
            while (max_n_kv < prompt_len + max_tokens) {
                max_n_kv *= 2;
            }
            if (max_n_kv > cc->kv_cache.n_ctx) {
                max_n_kv = cc->kv_cache.n_ctx;
            }

            if (!new_compute_ctx(8 * 1024 * 1024)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "canary run: step compute context allocation failed — "
                                    "out of memory.");
                commit_result();
                return TRANSCRIBE_ERR_OOM;
            }
            StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, max_n_kv, T_enc,
                                            cc->decoder_use_flash);
            if (sb.graph == nullptr || sb.argmax_out == nullptr) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary run: build_step_graph failed");
                commit_result();
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "canary run: step graph allocation failed — out of memory.");
                commit_result();
                return TRANSCRIBE_ERR_OOM;
            }

            // Mask buffer: full max_n_kv span, reused host-side. Positions
            // already populated by the prompt pass [0, prompt_len) start
            // attendable; remaining slots are -inf until each step flips
            // its newly-written position to attendable.
            const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
            const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> step_mask(max_n_kv, mask_neg_inf);
            for (int p = 0; p < prompt_len; ++p) {
                step_mask[p] = mask_zero;
            }

            for (int step = 1; step < max_tokens && next_token != eos_id; ++step) {
                if (cc->poll_abort()) {
                    commit_result();
                    return TRANSCRIBE_ERR_ABORTED;
                }
                if (n_past + 1 > max_n_kv) {
                    transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "canary run: hit max_n_kv=%d at n_past=%d", max_n_kv,
                                        n_past);
                    break;
                }

                int32_t token_val = next_token;
                int32_t pos_val   = n_past;
                int64_t kv_val    = n_past;
                ggml_backend_tensor_set(sb.token_id_in, &token_val, 0, sizeof(int32_t));
                ggml_backend_tensor_set(sb.pos_id_in, &pos_val, 0, sizeof(int32_t));
                ggml_backend_tensor_set(sb.kv_idx_in, &kv_val, 0, sizeof(int64_t));

                step_mask[n_past] = mask_zero;
                ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0,
                                        static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));

                if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph);
                    gs != GGML_STATUS_SUCCESS) {
                    break;
                }

                n_past += 1;
                cc->kv_cache.n    = n_past;
                cc->kv_cache.head = n_past;

                int32_t argmax_id = 0;
                ggml_backend_tensor_get(sb.argmax_out, &argmax_id, 0, sizeof(int32_t));
                next_token = argmax_id;

                if (next_token != eos_id) {
                    generated_ids.push_back(next_token);
                }
            }
        } else {
            // Dynamic-graph step path (CPU). Reserve the worst-case step graph
            // so per-step alloc_graph doesn't grow the scheduler's memory pool.
            if (new_compute_ctx(4 * 1024 * 1024)) {
                const int    worst_n_past = cc->kv_cache.n_ctx - 1;
                DecoderBuild db_reserve =
                    build_decoder_graph_kv(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                           /*n_tokens=*/1, worst_n_past, T_enc,
                                           /*skip_log_softmax=*/true, cc->decoder_use_flash);
                if (db_reserve.graph != nullptr) {
                    ggml_backend_sched_reserve(cc->sched, db_reserve.graph);
                }
            }

            for (int step = 1; step < max_tokens && next_token != eos_id; ++step) {
                if (cc->poll_abort()) {
                    commit_result();
                    return TRANSCRIBE_ERR_ABORTED;
                }
                if (n_past + 1 > cc->kv_cache.n_ctx) {
                    transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "canary run: KV cache full at n_past=%d", n_past);
                    break;
                }

                if (!new_compute_ctx(4 * 1024 * 1024)) {
                    break;
                }

                DecoderBuild db_step = build_decoder_graph_kv(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                                              /*n_tokens=*/1, n_past, T_enc,
                                                              /*skip_log_softmax=*/true, cc->decoder_use_flash);
                if (db_step.out == nullptr || db_step.graph == nullptr) {
                    break;
                }

                ggml_backend_sched_reset(cc->sched);
                if (!ggml_backend_sched_alloc_graph(cc->sched, db_step.graph)) {
                    break;
                }

                int32_t token_id = next_token;
                int32_t pos_id   = n_past;
                ggml_backend_tensor_set(db_step.token_ids_in, &token_id, 0, sizeof(int32_t));
                ggml_backend_tensor_set(db_step.pos_ids_in, &pos_id, 0, sizeof(int32_t));

                if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, db_step.graph);
                    gs != GGML_STATUS_SUCCESS) {
                    break;
                }

                n_past += 1;
                cc->kv_cache.n    = n_past;
                cc->kv_cache.head = n_past;

                int32_t argmax_id = 0;
                ggml_backend_tensor_get(db_step.argmax_out, &argmax_id, 0, sizeof(int32_t));
                next_token = argmax_id;

                if (next_token != eos_id) {
                    generated_ids.push_back(next_token);
                }
            }
        }

        // A post-loop next_token != eos_id means the decode hit max_tokens /
        // KV-full / a compute break without end-of-stream: flag truncation and
        // WARN rather than silently shortening. (Abort paths return early and
        // intentionally do NOT set the flag — abort is not a length truncation.)
        if (next_token != eos_id) {
            cc->was_truncated = true;
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                "canary run: output truncated at %d tokens — decode reached the "
                                "generation budget / decoder context (%d) before end-of-stream; "
                                "the transcript may be incomplete.",
                                static_cast<int>(generated_ids.size()), cc->kv_cache.n_ctx);
        }

        commit_result();
    }

    // Partial transcript committed above; a truncated decode returns the hard
    // OUTPUT_TRUNCATED status (the result stays readable, like an aborted run).
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

// ===========================================================================
// Offline batched decode (transcribe_run_batch). Encoder stays serial per
// utterance (compute-bound); mel parallel; the autoregressive decode is batched.
// ===========================================================================

// Encoder for one utterance from a precomputed mel buffer → host [hidden, T_enc].
transcribe_status encode_one_to_host(CanarySession *            cc,
                                     CanaryModel *              cm,
                                     const std::vector<float> & mel_buf,
                                     int                        mel_n_frames,
                                     std::vector<float> &       enc_host_out,
                                     int &                      T_enc_out,
                                     int64_t &                  enc_us) {
    if (mel_n_frames <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    cc->encoder_out = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size     = 8 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary encode_one_to_host: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, mel_n_frames, resolved_kv,
                                          cc->encoder_use_flash, cm->backend.c_str());
    if (eb.out == nullptr || eb.graph == nullptr) {
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
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "canary encode_one_to_host: encoder graph allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, mel_buf.data(), 0, mel_buf.size() * sizeof(float));

    if (eb.pos_emb_in != nullptr) {
        const int          d_model = cm->hparams.enc_d_model;
        const int          pos_len = static_cast<int>(eb.pos_emb_in->ne[1]);
        const int          T_enc   = (pos_len + 1) / 2;
        std::vector<float> pos_buf(static_cast<size_t>(pos_len) * d_model, 0.0f);
        const float        ln_10000 = std::log(10000.0f);
        std::vector<float> div_term(static_cast<size_t>(d_model / 2));
        for (int k = 0; k < d_model / 2; ++k) {
            div_term[k] = std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
        }
        for (int i = 0; i < pos_len; ++i) {
            const float p   = static_cast<float>((T_enc - 1) - i);
            float *     row = pos_buf.data() + static_cast<size_t>(i) * d_model;
            for (int k = 0; k < d_model / 2; ++k) {
                row[2 * k]     = std::sin(p * div_term[k]);
                row[2 * k + 1] = std::cos(p * div_term[k]);
            }
        }
        ggml_backend_tensor_set(eb.pos_emb_in, pos_buf.data(), 0, pos_buf.size() * sizeof(float));
    }

    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);

    const int64_t t0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t0;

    const int d_dec = static_cast<int>(eb.out->ne[0]);
    const int T_enc = static_cast<int>(eb.out->ne[1]);
    if (d_dec <= 0 || T_enc <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_host_out.resize(static_cast<size_t>(d_dec) * T_enc);
    ggml_backend_tensor_get(eb.out, enc_host_out.data(), 0, enc_host_out.size() * sizeof(float));
    T_enc_out = T_enc;
    return TRANSCRIBE_OK;
}

transcribe_status run_batch_serial(CanarySession *               cc,
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

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<CanarySession *>(session);
    auto * cm = static_cast<CanaryModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const bool primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                cm->plan.primary_kind != transcribe::BackendKind::Unknown;
    if (n == 1 || !cc->decoder_use_flash || !primary_is_gpu || transcribe::debug::enabled()) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto & hp      = cm->hparams;
    const int    hidden  = hp.dec_d_model;
    const int    n_layer = hp.dec_n_layers;

    // Shared multitask prompt (identical across the batch).
    const char * lang         = (params && params->language) ? params->language : "en";
    const bool   is_translate = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE);
    const char * tgt_lang     = lang;
    if (is_translate && params && params->target_language) {
        tgt_lang = params->target_language;
    }
    const char * task   = is_translate ? "translate" : "asr";
    const int    src_id = find_language_id(hp, lang);
    const int    tgt_id = find_language_id(hp, tgt_lang);
    if (src_id < 0 || tgt_id < 0) {
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    if (is_translate && !translation_pair_allowed(hp, lang, tgt_lang)) {
        return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
    }
    bool pnc = true;
    if (params != nullptr && params->pnc == TRANSCRIBE_PNC_MODE_OFF) {
        pnc = false;
    }
    std::vector<int32_t> prompt_ids;
    if (hp.prompt_format == "canary2") {
        prompt_ids = build_prompt_canary2(*cm, hp, src_id, tgt_id, task, pnc);
    } else if (hp.prompt_format == "canary") {
        prompt_ids = build_prompt_canary(hp, src_id, tgt_id, task, pnc);
    }
    if (prompt_ids.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int prompt_len = static_cast<int>(prompt_ids.size());

    // Pass 0: parallel mel.
    std::vector<char>               valid(n, 0);
    std::vector<std::vector<float>> mel_bufs(n);
    std::vector<int>                mel_nf(n, 0);
    int                             n_threads = cc->n_threads;
    if (n_threads <= 0) {
        n_threads = transcribe::default_n_threads();
    }
    int64_t       mel_us = 0, enc_us = 0;
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;
        }
        int nm = 0, nf = 0;
        if (cm->mel->compute(pcm[b], static_cast<size_t>(n_samples[b]), mel_bufs[b], nm, nf, /*n_threads=*/1) ==
                TRANSCRIBE_OK &&
            nf > 0) {
            mel_nf[b] = nf;
            valid[b]  = 1;
        }
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;

    // Pass 1: serial per-utterance encoder. Per-utterance input-length gate
    // (same enc_pos_emb_max_len limit as run()); an over-length utterance is
    // marked invalid with INPUT_TOO_LONG so the rest of the batch still decodes.
    std::vector<transcribe_status>  reject_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<float>> enc_hosts(n);
    std::vector<int>                T_enc(n, 0);
    int                             T_enc_max = 0;
    for (int b = 0; b < n; ++b) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (!valid[b]) {
            continue;
        }
        if (cm->hparams.enc_pos_emb_max_len > 0) {
            const int t_enc_pred = canary_predict_t_enc(mel_nf[b], cm->hparams.enc_subsampling_factor);
            if (t_enc_pred > cm->hparams.enc_pos_emb_max_len) {
                const double max_s = static_cast<double>(cm->caps.max_audio_ms) / 1000.0;
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "canary run_batch: utterance %d too long — %d encoder frames "
                                    "exceed the %d the model supports (~%.0f s max). See "
                                    "transcribe_capabilities.max_audio_ms.",
                                    b, t_enc_pred, cm->hparams.enc_pos_emb_max_len, max_s);
                reject_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
                valid[b]         = 0;
                continue;
            }
        }
        if (encode_one_to_host(cc, cm, mel_bufs[b], mel_nf[b], enc_hosts[b], T_enc[b], enc_us) != TRANSCRIBE_OK ||
            T_enc[b] <= 0) {
            valid[b] = 0;
            continue;
        }
        T_enc_max = std::max(T_enc_max, T_enc[b]);
    }
    if (T_enc_max <= 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = reject_status[b];
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }

    // Batched KV cache.
    const int max_new  = canary_max_new_tokens(params);
    int       max_n_kv = 1024;
    while (max_n_kv < prompt_len + max_new) {
        max_n_kv *= 2;
    }
    // Decoder self-KV ceiling: dec_max_position, optionally lowered (never
    // raised) by the caller's n_ctx knob. Default knob (0) leaves it at
    // dec_max_position, so in-spec batched decode is unchanged.
    const int n_ctx_cap = canary_context_ceiling(cc->n_ctx, hp);
    if (max_n_kv > n_ctx_cap) {
        max_n_kv = n_ctx_cap;
    }

    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache.buffer != nullptr &&
        (cc->kv_cache.n_batch != n || cc->kv_cache.T_enc != T_enc_max || cc->kv_cache.n_ctx != max_n_kv)) {
        cc->kv_cache.free();
    }
    if (cc->kv_cache.buffer == nullptr) {
        if (!kv_cache_init_batched(cc->kv_cache, cm->plan.primary, max_n_kv, T_enc_max, hidden, n_layer, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run_batch: batched KV cache allocation failed "
                                "(n_ctx=%d, T_enc=%d, %d d_model x %d layers x %d batch) — "
                                "out of memory.",
                                max_n_kv, T_enc_max, hidden, n_layer, n);
            return TRANSCRIBE_ERR_OOM;
        }
    } else {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        cc->kv_cache.n               = 0;
        cc->kv_cache.head            = 0;
        cc->kv_cache.cross_populated = false;
    }

    auto new_compute_ctx = [&](size_t mem) -> bool {
        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        cc->encoder_out = nullptr;
        ggml_init_params ip{};
        ip.mem_size     = mem;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        return cc->compute_ctx != nullptr;
    };

    const int64_t t_dec0 = ggml_time_us();

    // Batched cross-attention K/V.
    {
        if (!new_compute_ctx(8 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run_batch: cross-KV compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        DecoderBuild cross = build_cross_kv_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_enc_max, n);
        if (cross.graph == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, cross.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary run_batch: cross-KV graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        std::vector<float> packed(static_cast<size_t>(hidden) * T_enc_max * n, 0.0f);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            std::memcpy(packed.data() + static_cast<size_t>(b) * T_enc_max * hidden, enc_hosts[b].data(),
                        static_cast<size_t>(hidden) * T_enc[b] * sizeof(float));
        }
        ggml_backend_tensor_set(cross.encoder_out_in, packed.data(), 0, packed.size() * sizeof(float));
        if (ggml_backend_sched_graph_compute(cc->sched, cross.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.cross_populated = true;
    }

    // Batched step graph with a GROWING self-attention window. Self-KV holds
    // only the short prompt + transcript (audio is in cross-KV), so start the
    // read window at 64 and double it as n_past advances (rebuild graph + widen
    // mask, O(log) rebuilds; KV cache persists, capacity max_n_kv unchanged).
    const ggml_fp16_t f16_zero = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t f16_ninf = ggml_fp32_to_fp16(-INFINITY);
    const int32_t     eos_id   = hp.eos_token_id;

    std::vector<ggml_fp16_t> cmask(static_cast<size_t>(T_enc_max) * n, f16_ninf);
    for (int b = 0; b < n; ++b) {
        const int     real = valid[b] ? T_enc[b] : 1;
        ggml_fp16_t * base = cmask.data() + static_cast<size_t>(b) * T_enc_max;
        std::fill(base, base + std::min(real, T_enc_max), f16_zero);
    }

    int init_window = 64;
    while (init_window > max_n_kv) {
        init_window /= 2;
    }
    if (init_window < 1) {
        init_window = max_n_kv;
    }

    StepBuildBatched sb{};
    auto             rebuild = [&](int win, transcribe::EncDecStepIO & io) -> bool {
        if (!new_compute_ctx(16 * 1024 * 1024)) {
            return false;
        }
        sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, win, T_enc_max, n,
                                      cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.argmax_out == nullptr) {
            return false;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            return false;
        }
        ggml_backend_tensor_set(sb.cross_mask_in, cmask.data(), 0, cmask.size() * sizeof(ggml_fp16_t));
        io.token_ids = sb.token_ids_in;
        io.pos_ids   = sb.pos_ids_in;
        io.kv_idx    = sb.kv_idx_in;
        io.self_mask = sb.self_mask_in;
        io.argmax    = sb.argmax_out;
        io.graph     = sb.graph;
        return true;
    };

    std::vector<std::vector<int32_t>> generated(n);
    std::vector<char>                 truncated;
    if (const transcribe_status st = transcribe::run_batched_encdec_step_loop(
            cc, cc->sched, rebuild, prompt_ids, prompt_len, init_window, max_new, max_n_kv, eos_id, n, valid, generated,
            /*n_steps_out=*/nullptr, &truncated);
        st != TRANSCRIBE_OK) {
        return st;
    }
    const int64_t dec_us = ggml_time_us() - t_dec0;

    // Capture (strip control tokens like serial commit_result).
    const bool strip       = (params == nullptr) ? true : !params->keep_special_tags;
    const int  valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        transcribe_session::ResultSet rs;
        if (!valid[b]) {
            rs.status = reject_status[b];
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::vector<int> text_ids;
        if (strip) {
            for (int id : generated[b]) {
                if (!cm->tok.is_control(id)) {
                    text_ids.push_back(id);
                }
            }
        } else {
            text_ids.assign(generated[b].begin(), generated[b].end());
        }
        std::string full = cm->tok.decode(text_ids.data(), static_cast<int>(text_ids.size()));
        if (!full.empty() && full.front() == ' ') {
            full.erase(full.begin());
        }
        transcribe_session::SegmentEntry seg{};
        seg.t0_ms = 0;
        seg.t1_ms = 0;
        seg.text  = full;
        rs.segments.push_back(std::move(seg));
        rs.full_text   = full;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        rs.status      = TRANSCRIBE_OK;
        // Per-utterance truncation parity with the single-shot path: a valid row
        // that hit the generation budget / context window before eos reports
        // TRANSCRIBE_ERR_OUTPUT_TRUNCATED (partial transcript retained). Only
        // override an otherwise-OK status — never a worse one.
        if (rs.status == TRANSCRIBE_OK && b < static_cast<int>(truncated.size()) && truncated[b]) {
            cc->was_truncated = true;
            rs.status         = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = dec_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }

    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "canary run_batch: n=%d T_enc_max=%d kv_cap=%d prompt=%d\n"
                "  mel=%.1fms (parallel)  enc=%.1fms (serial x%d)  decode=%.1fms (batched)",
                n, T_enc_max, max_n_kv, prompt_len, mel_us / 1000.0, enc_us / 1000.0, n, dec_us / 1000.0);
    }
    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "canary",
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

}  // namespace transcribe::canary
