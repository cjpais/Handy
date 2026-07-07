// arch/parakeet/model.cpp - Parakeet family handler.
//
// load() binds a runtime backend, opens the GGUF with no_alloc=true so
// ggml builds the tensor catalog without touching the data section,
// allocates a backend buffer for every tensor, and streams the data
// section into it; afterward every borrowed ggml_tensor* in `weights`
// points at backend memory. The persistent model state is ctx_meta +
// backend_buffer + backends, freed in that order in the destructor.
// run() computes the mel front-end, runs the encoder graph, then
// host-decodes (predictor + joint + TDT).

#include "decoder.h"
#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "parakeet.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "transcribe/parakeet.h"
#include "weights.h"

// No backend-specific #includes: ggml's device registry discovers
// Metal/Vulkan/CUDA/BLAS at runtime.

#include <algorithm>
#include <cassert>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <ios>
#include <limits>
#include <memory>
#include <set>
#include <string>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

namespace transcribe::parakeet {

// Forward declaration; full definition at the bottom of the file.
extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, ParakeetModel>);
static_assert(std::is_base_of_v<transcribe_session, ParakeetSession>);

ParakeetSession::~ParakeetSession() {
    // Tear down per-call compute state before the model's backend plan
    // (which outlives the context): scheduler, then context.
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
    encoder_out = nullptr;

    // Streaming cache tensors live in their own ggml_context + backend
    // buffer. Free buffer first (may hold a backend ref), then the ctx.
    if (stream_caches.buffer != nullptr) {
        safe_buffer_free(stream_caches.buffer);
        stream_caches.buffer = nullptr;
    }
    if (stream_caches.ctx != nullptr) {
        ggml_free(stream_caches.ctx);
        stream_caches.ctx = nullptr;
    }
    // Rel-pos projection memo (own ctx + buffer; geometry-keyed).
    if (stream_caches.pos_proj_buf != nullptr) {
        safe_buffer_free(stream_caches.pos_proj_buf);
        stream_caches.pos_proj_buf = nullptr;
    }
    if (stream_caches.pos_proj_ctx != nullptr) {
        ggml_free(stream_caches.pos_proj_ctx);
        stream_caches.pos_proj_ctx = nullptr;
    }
    stream_caches.pos_proj.clear();
    stream_caches.pos_proj_len = -1;
    stream_caches.last_channel.clear();
    stream_caches.last_time.clear();
    stream_caches.last_k.clear();
    stream_caches.last_v.clear();
    stream_caches.initialized = false;
}

ParakeetModel::~ParakeetModel() {
    // Teardown order: ctx_meta → backend_buffer → plan backends. The
    // buffer must be freed before the backends (it holds a backend ref);
    // backends freed in reverse init order.
    if (bn_fused_ctx != nullptr) {
        ggml_free(bn_fused_ctx);
        bn_fused_ctx = nullptr;
    }
    if (bn_fused_buffer != nullptr) {
        safe_buffer_free(bn_fused_buffer);
        bn_fused_buffer = nullptr;
    }
    // CPU conv_pw F32 promotion ctx + buffer (non-null only when
    // promote_conv_pw_to_f32_on_cpu ran).
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

// Resolve the runtime language hint to a prompt-table index for the
// multilingual prompt-conditioned variants. The dictionary carries both
// BCP-47 codes ("en-US") and short aliases ("en"); exact-string lookup.
// Empty/null hint maps to the dictionary's auto slot (prompt.auto_id);
// unknown hints return -1 (caller surfaces UNSUPPORTED_LANGUAGE).
int32_t resolve_prompt_id(const ParakeetHParams & hp, const char * language_hint) {
    if (!hp.has_prompt) {
        return -1;
    }
    const bool empty_hint = (language_hint == nullptr) || (*language_hint == '\0');
    if (empty_hint) {
        // Auto-language slot; fall back to the dictionary's first entry
        // when there is no explicit auto_id.
        if (hp.prompt_auto_id >= 0) {
            return hp.prompt_auto_id;
        }
        if (!hp.prompt_dictionary_indices.empty()) {
            return hp.prompt_dictionary_indices.front();
        }
        return -1;
    }
    for (size_t i = 0; i < hp.prompt_dictionary_locales.size(); ++i) {
        if (hp.prompt_dictionary_locales[i] == language_hint) {
            return hp.prompt_dictionary_indices[i];
        }
    }
    return -1;
}

// Fill a [num_prompts, T_enc, 1, n_batch] buffer with a one-hot at
// column prompt_id of each utterance, replicated across T_enc (one id
// per utterance in prompt_ids). Host-side replication keeps the in-graph
// step a single concat + two matmuls. Returns false if any id is OOR.
bool fill_prompt_one_hot(std::vector<float> &         out,
                         int                          num_prompts,
                         int                          T_enc,
                         int                          n_batch,
                         const std::vector<int32_t> & prompt_ids) {
    const size_t total = static_cast<size_t>(num_prompts) * static_cast<size_t>(T_enc) * static_cast<size_t>(n_batch);
    out.assign(total, 0.0f);
    for (int b = 0; b < n_batch; ++b) {
        const int32_t pid = (b < static_cast<int>(prompt_ids.size())) ? prompt_ids[static_cast<size_t>(b)] :
                                                                        (prompt_ids.empty() ? -1 : prompt_ids.front());
        if (pid < 0 || pid >= num_prompts) {
            return false;
        }
        const size_t per_utt = static_cast<size_t>(num_prompts) * static_cast<size_t>(T_enc);
        const size_t utt_off = static_cast<size_t>(b) * per_utt;
        for (int t = 0; t < T_enc; ++t) {
            const size_t row_off = utt_off + static_cast<size_t>(t) * static_cast<size_t>(num_prompts);
            out[row_off + static_cast<size_t>(pid)] = 1.0f;
        }
    }
    return true;
}

// Fuse inference-time BatchNorm into scale + bias tensors:
//   BN eval: y = (x - mean) / sqrt(var + eps) * weight + bias
//   fused:   y = x * scale + shift,  scale = weight / sqrt(var + eps),
//            shift = bias - mean * scale
// Fused tensors live in a separate ggml ctx + CPU buffer, computed once
// at load and referenced by the encoder graph in place of the raw BN.
transcribe_status fuse_batch_norm(ParakeetModel & m) {
    const size_t n_blocks = m.weights.blocks.size();
    if (n_blocks == 0) {
        return TRANSCRIBE_OK;
    }

    const int64_t d            = m.hparams.enc_d_model;
    const size_t  tensor_bytes = static_cast<size_t>(d) * sizeof(float);

    const size_t     ctx_size = n_blocks * 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params   = { ctx_size, nullptr, /*no_alloc=*/true };
    m.bn_fused_ctx            = ggml_init(params);
    if (m.bn_fused_ctx == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    // Create all tensors first, then allocate a buffer.
    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b              = m.weights.blocks[i];
        b.conv_bn_fused_scale = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
        b.conv_bn_fused_bias  = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
    }

    // Allocate on the CPU backend (always last in the scheduler list).
    m.bn_fused_buffer = ggml_backend_alloc_ctx_tensors(m.bn_fused_ctx, m.plan.scheduler_list.back());
    if (m.bn_fused_buffer == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    // Compute fused values from the raw BN tensors.
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

// On a CPU primary backend, dequantize the conformer 1×1 pointwise conv
// weights (pw1, pw2) from F16 to F32: CPUs without native F16 arithmetic
// pay a per-matmul F16→F32 upconvert that erases the bandwidth win. The
// machinery lives in transcribe::load_common; this is the parakeet-
// specific weight walk.
transcribe_status promote_conv_pw_to_f32_on_cpu(ParakeetModel & m) {
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
    return load_common::promote_conv_pw_f16_to_f32_on_cpu(m.plan, slots, "parakeet", &m.conv_pw_f32_ctx,
                                                          &m.conv_pw_f32_buffer);
}

// Default variant string when the GGUF did not carry stt.variant.
constexpr const char k_default_variant[] = "tdt-0.6b-v2";

// Allocate the streaming encoder caches (cache_last_channel,
// cache_last_time), lazily on the first stream_begin for ChunkedLimited
// variants. Layout matches NeMo's get_initial_cache_state (see
// ParakeetStreamingCaches in parakeet.h). The tensors live in their own
// ggml_context + backend buffer on the primary backend (freed in the
// dtor). Idempotent; zero_streaming_caches clears contents separately.
transcribe_status init_streaming_caches(ParakeetSession * pc, ParakeetModel * pm) {
    if (pc->stream_caches.initialized) {
        return TRANSCRIBE_OK;
    }

    const auto & hp        = pm->hparams;
    const int    n_layer   = static_cast<int>(pm->weights.blocks.size());
    const int    d_model   = hp.enc_d_model;
    const int    T_cache   = hp.enc_att_context_left;
    const int    k_minus_1 = hp.enc_conv_context_left >= 0 ? hp.enc_conv_context_left : (hp.enc_conv_kernel - 1);

    if (n_layer <= 0 || d_model <= 0 || T_cache <= 0 || k_minus_1 <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet stream init_caches: degenerate sizes "
                "(n_layer=%d, d_model=%d, T_cache=%d, k-1=%d)",
                n_layer, d_model, T_cache, k_minus_1);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // 4 tensors per layer (last_channel/time/k/v) plus graph-build headroom.
    const size_t     ctx_size = static_cast<size_t>(4 * n_layer + 8) * ggml_tensor_overhead();
    ggml_init_params ip{};
    ip.mem_size           = ctx_size;
    ip.mem_buffer         = nullptr;
    ip.no_alloc           = true;
    pc->stream_caches.ctx = ggml_init(ip);
    if (pc->stream_caches.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream init_caches: ggml_init failed");
        return TRANSCRIBE_ERR_OOM;
    }

    pc->stream_caches.last_channel.assign(n_layer, nullptr);
    pc->stream_caches.last_time.assign(n_layer, nullptr);
    pc->stream_caches.last_k.assign(n_layer, nullptr);
    pc->stream_caches.last_v.assign(n_layer, nullptr);
    for (int i = 0; i < n_layer; ++i) {
        ggml_tensor * lc = ggml_new_tensor_2d(pc->stream_caches.ctx, GGML_TYPE_F32, d_model, T_cache);
        ggml_tensor * lt = ggml_new_tensor_2d(pc->stream_caches.ctx, GGML_TYPE_F32, k_minus_1, d_model);
        ggml_tensor * lk = ggml_new_tensor_2d(pc->stream_caches.ctx, GGML_TYPE_F32, d_model, T_cache);
        ggml_tensor * lv = ggml_new_tensor_2d(pc->stream_caches.ctx, GGML_TYPE_F32, d_model, T_cache);
        if (lc == nullptr || lt == nullptr || lk == nullptr || lv == nullptr) {
            ggml_free(pc->stream_caches.ctx);
            pc->stream_caches.ctx = nullptr;
            return TRANSCRIBE_ERR_OOM;
        }
        char name[64];
        std::snprintf(name, sizeof(name), "stream.cache.last_channel.%d", i);
        ggml_set_name(lc, name);
        std::snprintf(name, sizeof(name), "stream.cache.last_time.%d", i);
        ggml_set_name(lt, name);
        std::snprintf(name, sizeof(name), "stream.cache.last_k.%d", i);
        ggml_set_name(lk, name);
        std::snprintf(name, sizeof(name), "stream.cache.last_v.%d", i);
        ggml_set_name(lv, name);
        pc->stream_caches.last_channel[i] = lc;
        pc->stream_caches.last_time[i]    = lt;
        pc->stream_caches.last_k[i]       = lk;
        pc->stream_caches.last_v[i]       = lv;
    }

    pc->stream_caches.buffer = ggml_backend_alloc_ctx_tensors(pc->stream_caches.ctx, pm->plan.primary);
    if (pc->stream_caches.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream init_caches: backend buffer alloc failed");
        ggml_free(pc->stream_caches.ctx);
        pc->stream_caches.ctx = nullptr;
        return TRANSCRIBE_ERR_BACKEND;
    }

    pc->stream_caches.channel_len         = 0;
    pc->stream_caches.mel_frames_consumed = 0;

    pc->stream_caches.initialized = true;
    return TRANSCRIBE_OK;
}

// Zero the streaming caches and reset cursors at each stream_begin
// (NeMo's get_initial_cache_state returns zeros every time). The backend
// buffer survives; only the contents are cleared.
void zero_streaming_caches(ParakeetSession * pc) {
    if (pc->stream_caches.buffer != nullptr) {
        ggml_backend_buffer_clear(pc->stream_caches.buffer, 0);
    }
    pc->stream_caches.channel_len         = 0;
    pc->stream_caches.mel_frames_consumed = 0;
    pc->stream_caches.pcm_start_sample    = 0;
}

// Reset the streaming decoder state (LSTM h/c, prev token, frame offset)
// to the model's predictor sizing.
void reset_streaming_decoder_state(ParakeetSession * pc, const ParakeetModel * pm) {
    const auto & pred        = pm->host_decoder.predictor;
    const int    n_layers    = static_cast<int>(pred.lstm.size());
    const int    pred_hidden = pred.pred_hidden;

    pc->stream_dec_state.lstm_state.reset(n_layers, pred_hidden);
    pc->stream_dec_state.prev_token_id = -1;
    pc->stream_dec_state.frame_offset  = 0;
    pc->stream_dec_state.initialized   = true;
}

// Forward declarations for the Arch trait below.
extern transcribe_status load(Loader &, const transcribe_model_load_params *, transcribe_model **);
extern transcribe_status init_context(transcribe_model *, const transcribe_session_params *, transcribe_session **);
extern transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    // The dispatcher has verified out_model is non-null and cleared it,
    // and the loader has a valid gguf_context.

    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<ParakeetModel>();
    m->arch      = &arch;
    m->t_load_us = 0;

    // Variant defaulting belongs in the family handler. The variant
    // string is descriptive metadata (surfaced via
    // transcribe_model_variant_string) and drives no behavior decision;
    // capability differences are expressed as stt.capability.* /
    // general.languages KV, read below.
    if (loader.variant().empty()) {
        m->variant = k_default_variant;
    } else {
        m->variant = loader.variant();
    }

    // Empty until the runtime backend is bound below (public ABI: "no
    // backend currently bound").
    m->backend.clear();

    // Capability resolution, KV-driven. apply_family_invariants
    // populates the family defaults; read_capability_kv then overlays any
    // stt.capability.* KV (absent leaves the default; wrong-type
    // propagates as TRANSCRIBE_ERR_GGUF). n_languages=0 / languages=null
    // is the "we don't know" state until read_languages_kv overwrites it
    // from general.languages.
    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (const transcribe_status st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }

    // Languages from general.languages (copied into the model, so the
    // loader's gguf_context can be freed afterward).
    if (const transcribe_status st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Tokenizer ingest: copies tokenizer.ggml.* into the Tokenizer's own
    // storage (no borrow into the gguf_context).
    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    // Architecture KV (encoder / predictor / joint / frontend dims).
    // read_parakeet_hparams enforces cross-field invariants so the shapes
    // we validate against are consistent.
    if (const transcribe_status st = read_parakeet_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Derive supports_streaming from hparams:
    //   ChunkedLimited + (L, R) >= 0 — cache-aware streaming
    //     (nemotron-speech-streaming-en-0.6b).
    //   ChunkedLimitedWithRc + non-empty (L, C, R) menu — buffered
    //     streaming (parakeet-unified-en-0.6b).
    // Offline variants stay non-streaming.
    const bool cache_aware_streaming =
        (m->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited) &&
        m->hparams.enc_att_context_left >= 0 && m->hparams.enc_att_context_right >= 0;
    const bool buffered_streaming =
        (m->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc) &&
        !m->hparams.enc_att_chunk_left_choices.empty() && !m->hparams.enc_att_chunk_chunk_choices.empty() &&
        !m->hparams.enc_att_chunk_right_choices.empty();
    // supports_streaming is the generic gate; the configurable geometry
    // is reached via the parakeet stream extensions, not flat caps.
    if (buffered_streaming || cache_aware_streaming) {
        m->caps.supports_streaming = true;
    }

    // Construct the mel front-end (precomputes Hann window + Slaney mel
    // filterbank); amortized across every run on every context.
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
        cfg.normalize    = m->hparams.fe_normalize;
        // NeMo's FilterbankFeatures.stft uses pad_mode="constant" (the
        // cpp MelConfig default is "reflect"). The two differ in the
        // first/last few STFT frames; offline the residual washes out but
        // streaming sees it every chunk, amplified by per-feature normalize.
        cfg.pad_mode     = "constant";
        m->mel.emplace(cfg);
    }

    // Reopen the file with no_alloc=true + ctx=&ctx_meta: ggml builds the
    // tensor catalog (metadata only), then we bind a backend, allocate a
    // buffer for every tensor, and stream the data section into it. The
    // second gguf_context (gguf_data) is freed before load() returns.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        // The first-stage open succeeded, so this is unexpected (file
        // changed between opens, or a permissions transition).
        return TRANSCRIBE_ERR_GGUF;
    }

    // Validate every tensor against the canonical catalog (type + shape
    // only, so it works before the backend buffer is bound).
    if (const transcribe_status st = build_parakeet_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Resolve the backend plan via ggml's device registry (runtime, no
    // compile-time backend guards). See transcribe-load-common.h.
    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;

    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "parakeet", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    // Allocate a backend buffer for every tensor in ctx_meta on the
    // primary backend; the weight bytes are streamed in below.
    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    // Stream tensor data from the GGUF into the backend buffer slots
    // (shared loop in transcribe-load-common.h; works on host-memory
    // backends and discrete GPUs).
    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "parakeet");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    gguf_free(gguf_data);

    // Fuse BatchNorm into scale + bias (replaces 4 elementwise ops per
    // block with 2). Skipped for LayerNorm conv-module variants (no
    // running stats to fuse against).
    if (m->hparams.enc_conv_norm_type == ParakeetHParams::ConvNormType::BatchNorm) {
        if (const transcribe_status st = fuse_batch_norm(*m); st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // On CPU primary backend, dequantize conv pointwise weights to F32
    // (see promote_conv_pw_to_f32_on_cpu). No-op on GPU backends.
    if (const transcribe_status st = promote_conv_pw_to_f32_on_cpu(*m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Build the host mirror of the predictor + joint weights (one-shot
    // backend → host copy; the decoder runs on host, see decoder.h).
    if (const transcribe_status st = build_host_decoder_weights(*m, m->host_decoder); st != TRANSCRIBE_OK) {
        return st;
    }

    m->t_load_us = ggml_time_us() - t_load_start;

    // The caller now owns the model and must call transcribe_model_free.
    *out_model = m.release();
    return TRANSCRIBE_OK;
}

transcribe_status init_context(transcribe_model *                model,
                               const transcribe_session_params * params,
                               transcribe_session **             out_ctx) {
    // The dispatcher has null-checked model/params/out_ctx; verify the
    // model is actually a ParakeetModel (arch is the discriminator).
    if (model->arch != &arch) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto pc       = std::make_unique<ParakeetSession>();
    pc->model     = model;
    pc->n_threads = params->n_threads;
    pc->kv_type   = params->kv_type;

    *out_ctx = pc.release();
    return TRANSCRIBE_OK;
}

// True for a multilingual language-tag SentencePiece piece "<ll-RR>"
// (2-3 letter language, '-', 2-4 letter region), e.g. "<en-US>". The
// nemotron-3.5 vocab emits one per segment to mark the language; we drop
// them from the public result by default (clean transcript, uncorrupted
// word/timestamp aggregation). Requiring '-REGION' avoids matching
// control pieces like "<unk>". No-op for the English parakeets.
static bool is_lang_tag_piece(const std::string & p) {
    const size_t n = p.size();
    if (n < 7 || p.front() != '<' || p.back() != '>') {
        return false;  // min "<aa-AA>"
    }
    size_t       i     = 1;
    const size_t end   = n - 1;
    const size_t lang0 = i;
    while (i < end && p[i] >= 'a' && p[i] <= 'z') {
        ++i;
    }
    const size_t lang_len = i - lang0;
    if (lang_len < 2 || lang_len > 3) {
        return false;
    }
    if (i >= end || p[i] != '-') {
        return false;
    }
    ++i;
    const size_t reg0 = i;
    while (i < end && std::isalpha(static_cast<unsigned char>(p[i]))) {
        ++i;
    }
    const size_t reg_len = i - reg0;
    if (reg_len < 2 || reg_len > 4) {
        return false;
    }
    return i == end;  // interior consumed exactly up to '>'
}

// Drop this piece from the public result when keep_special_tags is off:
// stripped if CONTROL-typed or matching the <ll-RR> locale-tag pattern
// (transitional fallback). Shared by the offline and streaming builders.
static bool is_strippable_special(const transcribe::Tokenizer & tok, int id) {
    return tok.is_control(id) || is_lang_tag_piece(tok.token(id));
}

// Collapse runs of ASCII spaces to one and trim both ends — cleans up the
// double/edge spaces a stripped tag leaves behind. Shared by both builders.
static void normalize_transcript_whitespace(std::string & s) {
    std::string norm;
    norm.reserve(s.size());
    bool prev_space = false;
    for (char ch : s) {
        const bool is_space = (ch == ' ');
        if (is_space && prev_space) {
            continue;
        }
        norm.push_back(ch);
        prev_space = is_space;
    }
    while (!norm.empty() && norm.front() == ' ') {
        norm.erase(norm.begin());
    }
    while (!norm.empty() && norm.back() == ' ') {
        norm.pop_back();
    }
    s.swap(norm);
}

// Milliseconds per encoder frame (NeMo: subsampling_factor * hop_length /
// sample_rate). Returned as a double — not pre-rounded — so a non-integral
// stride doesn't quantize before the call-site multiply. Shared by both
// result builders.
static double parakeet_ms_per_enc_frame(const ParakeetHParams & hp) {
    return 1000.0 * static_cast<double>(hp.enc_subsampling_factor) * static_cast<double>(hp.fe_hop_length) /
           static_cast<double>(hp.fe_sample_rate);
}

// Host-side TDT/RNNT/CTC decode + public result-hierarchy build for one
// utterance's encoder output. `enc` is the row-major [T_enc, d_enc]
// activation for ONE utterance; utt_index >= 0 tags the per-utterance
// dump (-1 for single-shot). Writes the session result slot (tokens /
// words / segments / full_text / result_kind / has_result).
static transcribe_status decode_and_populate(ParakeetSession *             pc,
                                             ParakeetModel *               pm,
                                             const transcribe_run_params * params,
                                             const float *                 enc,
                                             int                           T_enc,
                                             int                           d_enc,
                                             int                           utt_index,
                                             const char *                  enc_dump_name_override = nullptr) {
    // Default dump name is "dec.enc_out"; the prompt-conditioned path
    // overrides to "dec.enc_out_prompted" so the comparator sees the
    // post-prompt tensor under its expected filename.
    std::string dump_name = (enc_dump_name_override != nullptr && *enc_dump_name_override != '\0') ?
                                std::string(enc_dump_name_override) :
                                std::string("dec.enc_out");
    if (utt_index >= 0) {
        dump_name += ".b" + std::to_string(utt_index);
    }
    // Optional dump of the encoder output as the decoder sees it.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { T_enc, d_enc };
        const char *    stage    = (enc_dump_name_override != nullptr && *enc_dump_name_override != '\0') ?
                                       "decoder.enc_out_prompted" :
                                       "decoder.enc_out";
        transcribe::debug::dump_host_f32(
            dump_name.c_str(), enc, static_cast<long long>(T_enc) * static_cast<long long>(d_enc), shape, 2, stage);
    }

    pc->raw_tokens.clear();
    const int64_t t_dec_start = ggml_time_us();
    {
        transcribe_status st = TRANSCRIBE_OK;
        switch (pm->host_decoder.head_kind) {
            case HostHeadKind::TDT:
                st = decode_tdt_greedy(pm->host_decoder, enc, T_enc, d_enc, pc->n_threads, pc->raw_tokens);
                break;
            case HostHeadKind::RNNT:
                st = decode_rnnt_greedy(pm->host_decoder, enc, T_enc, d_enc, pc->n_threads, pc->raw_tokens);
                break;
            case HostHeadKind::CTC:
                st = decode_ctc_greedy(pm->host_decoder, enc, T_enc, d_enc, pc->n_threads, pc->raw_tokens);
                break;
        }
        if (st != TRANSCRIBE_OK) {
            return st;
        }
    }
    pc->t_decode_us = ggml_time_us() - t_dec_start;

    // ----- Build the public result hierarchy -----
    //
    // step_at_emit is an encoder frame index; one frame is
    // subsampling_factor * hop_length / sample_rate seconds (80 ms on
    // v2/v3). Shared with the streaming builder.
    const double frame_to_ms = parakeet_ms_per_enc_frame(pm->hparams);

    // SentencePiece word-boundary marker U+2581 ("▁", UTF-8 E2 96 81): a
    // raw piece starting with it opens a new word. We use the raw NeMo
    // piece (its leading 3 bytes are unambiguous), not the post-decode
    // "▁ → space" form.
    constexpr const char k_sp_marker[]   = "\xE2\x96\x81";
    constexpr int        k_sp_marker_len = 3;

    const transcribe::Tokenizer & tok = pm->tok;

    pc->tokens.reserve(pc->raw_tokens.size());
    // Strip multilingual <ll-RR> language tags by default (gated on
    // keep_special_tags / CLI --raw-tokens). Detection is
    // Tokenizer::is_control (CONTROL token_type); is_lang_tag_piece is a
    // transitional fallback for GGUFs predating that converter change.
    const bool strip_tags = (params == nullptr) ? true : !params->keep_special_tags;
    for (const TdtToken & rt : pc->raw_tokens) {
        if (strip_tags && is_strippable_special(tok, rt.id)) {
            continue;
        }
        transcribe_session::TokenEntry te;
        te.id                  = rt.id;
        te.p                   = rt.p;
        te.t0_ms               = static_cast<int64_t>(std::llround(frame_to_ms * static_cast<double>(rt.step_at_emit)));
        // duration_frames=0 yields a zero-width "point in time" token.
        const double end_frame = static_cast<double>(rt.step_at_emit) + static_cast<double>(rt.duration_frames);
        te.t1_ms               = static_cast<int64_t>(std::llround(frame_to_ms * end_frame));
        te.text                = tok.decode(&rt.id, 1);
        pc->tokens.push_back(std::move(te));
    }

    // Word + segment construction. v1 produces a single segment over the
    // whole clip; words split on SentencePiece marker boundaries. Empty
    // result → no segment/words/text (the accessors' safe-sentinel state).
    if (!pc->tokens.empty()) {
        transcribe_session::SegmentEntry seg;
        seg.t0_ms       = pc->tokens.front().t0_ms;
        seg.t1_ms       = pc->tokens.back().t1_ms;
        seg.first_token = 0;
        seg.n_tokens    = static_cast<int>(pc->tokens.size());
        seg.first_word  = 0;

        // First token opens word 0; a raw piece beginning with the
        // marker opens a new word, others extend the current word.
        transcribe_session::WordEntry cur_word;
        bool                          cur_word_open = false;

        auto open_new_word = [&](int token_index, const transcribe_session::TokenEntry & tk) {
            if (cur_word_open) {
                cur_word.t1_ms    = pc->tokens[static_cast<size_t>(token_index - 1)].t1_ms;
                cur_word.n_tokens = token_index - cur_word.first_token;
                pc->words.push_back(std::move(cur_word));
                cur_word = transcribe_session::WordEntry{};
            }
            cur_word.t0_ms       = tk.t0_ms;
            cur_word.first_token = token_index;
            cur_word.seg_index   = 0;
            cur_word_open        = true;
        };

        for (size_t i = 0; i < pc->tokens.size(); ++i) {
            const auto & tk          = pc->tokens[i];
            const auto & raw_piece   = tok.token(tk.id);  // empty if id OOR
            const bool   starts_word = (i == 0) || (raw_piece.size() >= static_cast<size_t>(k_sp_marker_len) &&
                                                    std::memcmp(raw_piece.data(), k_sp_marker, k_sp_marker_len) == 0);
            if (starts_word) {
                open_new_word(static_cast<int>(i), tk);
            }
            // seg/word back-pointers.
            pc->tokens[i].seg_index  = 0;
            pc->tokens[i].word_index = static_cast<int>(pc->words.size());
        }
        // Close out the trailing word.
        if (cur_word_open) {
            cur_word.t1_ms    = pc->tokens.back().t1_ms;
            cur_word.n_tokens = static_cast<int>(pc->tokens.size()) - cur_word.first_token;
            pc->words.push_back(std::move(cur_word));
        }

        // Materialize each word's text via the tokenizer (so "▁ → space"
        // runs over the whole span), trimming the word-opener's leading
        // space.
        std::vector<int> id_buf;
        for (auto & wd : pc->words) {
            id_buf.clear();
            id_buf.reserve(static_cast<size_t>(wd.n_tokens));
            for (int j = 0; j < wd.n_tokens; ++j) {
                id_buf.push_back(pc->tokens[static_cast<size_t>(wd.first_token + j)].id);
            }
            std::string text = tok.decode(id_buf.data(), wd.n_tokens);
            if (!text.empty() && text.front() == ' ') {
                text.erase(text.begin());
            }
            wd.text = std::move(text);
        }

        seg.n_words = static_cast<int>(pc->words.size());

        // Build the full text for the segment via the tokenizer
        // (one decode call over every id, so the SentencePiece
        // substitution sees the whole sequence).
        std::vector<int> all_ids;
        all_ids.reserve(pc->tokens.size());
        for (const auto & tk : pc->tokens) {
            all_ids.push_back(tk.id);
        }
        std::string full = tok.decode(all_ids.data(), static_cast<int>(all_ids.size()));
        normalize_transcript_whitespace(full);
        seg.text      = full;
        pc->full_text = std::move(full);
        pc->segments.push_back(std::move(seg));

        // Clamp to the caller's requested ceiling. AUTO resolves to the
        // family max (TOKEN); a coarser request elides the finer levels.
        // The dispatcher has already rejected any request finer than
        // TOKEN, so eff ends up in {NONE, SEGMENT, WORD, TOKEN}.
        transcribe_timestamp_kind eff = params->timestamps;
        if (eff == TRANSCRIBE_TIMESTAMPS_AUTO) {
            eff = pm->caps.max_timestamp_kind;  // = TOKEN for parakeet
        }
        if (eff == TRANSCRIBE_TIMESTAMPS_NONE) {
            // Keep the segment + text, drop all alignment data.
            pc->tokens.clear();
            pc->words.clear();
            auto & s      = pc->segments.back();
            s.t0_ms       = 0;
            s.t1_ms       = 0;
            s.first_word  = 0;
            s.n_words     = 0;
            s.first_token = 0;
            s.n_tokens    = 0;
        } else if (eff == TRANSCRIBE_TIMESTAMPS_SEGMENT) {
            // Keep segment timings, drop token + word tables.
            pc->tokens.clear();
            pc->words.clear();
            auto & s      = pc->segments.back();
            s.first_word  = 0;
            s.n_words     = 0;
            s.first_token = 0;
            s.n_tokens    = 0;
        } else if (eff == TRANSCRIBE_TIMESTAMPS_WORD) {
            // Keep segment + word timings, drop the token table. Zero the
            // back-references so word/segment accessors return safe
            // sentinels (n_tokens == 0) rather than indexing it.
            pc->tokens.clear();
            for (auto & w : pc->words) {
                w.first_token = 0;
                w.n_tokens    = 0;
            }
            auto & s      = pc->segments.back();
            s.first_token = 0;
            s.n_tokens    = 0;
        }
        // TRANSCRIBE_TIMESTAMPS_TOKEN: nothing to elide.

        pc->result_kind = eff;
        pc->has_result  = true;
    }

    return TRANSCRIBE_OK;
}

static transcribe_status run_one_shot_inner(ParakeetSession *             pc,
                                            ParakeetModel *               pm,
                                            const float *                 pcm,
                                            int                           n_samples,
                                            const transcribe_run_params * params) {
    // Pre-run abort check (the single observation point on this path).
    if (pc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    // ----- Mel front-end -----
    if (!pm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: model has no MelFrontend (load skipped?)");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (const transcribe_status mst =
            pm->mel->compute(pcm, static_cast<size_t>(n_samples), pc->mel_buf, mel_n_mels, mel_n_frames);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    pc->t_mel_us = ggml_time_us() - t_mel_start;

    // ----- Reset per-call compute state -----
    // Free the previous run's compute_ctx; encoder_out is invalidated.
    if (pc->compute_ctx != nullptr) {
        ggml_free(pc->compute_ctx);
        pc->compute_ctx = nullptr;
    }
    pc->encoder_out = nullptr;

    // ----- Build the compute context + encoder graph -----
    //
    // compute_ctx holds tensor metadata + the cgraph (data is allocated
    // by the scheduler below). The 24-block encoder graph uses ~1.23 MB
    // of metadata arena; 4 MB gives ~3x headroom.
    {
        ggml_init_params init_params{};
        init_params.mem_size   = 4 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        pc->compute_ctx        = ggml_init(init_params);
        if (pc->compute_ctx == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: ggml_init for compute_ctx failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // GGML_TYPE_COUNT is the encoder's "auto" sentinel.
    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    EncoderBuild eb =
        build_encoder_graph(pc->compute_ctx, pm->weights, pm->hparams, mel_n_frames, resolved_kv, pm->backend.c_str());
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;  // build_encoder_graph logged
    }

    // ----- Allocate compute tensors via the multi-backend scheduler.
    // Created lazily, persists across calls. -----
    if (pc->sched == nullptr) {
        pc->sched = ggml_backend_sched_new(pm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(pm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (pc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(pc->sched);
    if (!ggml_backend_sched_alloc_graph(pc->sched, eb.graph)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: ggml_backend_sched_alloc_graph failed");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Upload the mel; the row-major [num_mels, n_frames] buffer is
    // byte-identical to the ggml ne=[n_frames, num_mels, 1, 1] input.
    ggml_backend_tensor_set(eb.mel_in, pc->mel_buf.data(), 0, pc->mel_buf.size() * sizeof(float));

    transcribe::debug::dump_tensor("enc.mel.in", eb.mel_in, "encoder.mel");

    // Prompt one-hot input (multilingual variants only): fill
    // prompt_one_hot_in with the resolved language's one-hot replicated
    // across T_enc frames (see resolve_prompt_id).
    if (eb.prompt_one_hot_in != nullptr) {
        const int     P         = static_cast<int>(eb.prompt_one_hot_in->ne[0]);
        const int     T_oh      = static_cast<int>(eb.prompt_one_hot_in->ne[1]);
        const char *  lang_hint = (params != nullptr) ? params->language : nullptr;
        const int32_t pid       = resolve_prompt_id(pm->hparams, lang_hint);
        if (pid < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet run: language %s%s%s not in prompt "
                    "dictionary",
                    lang_hint ? "\"" : "", lang_hint ? lang_hint : "<null>", lang_hint ? "\"" : "");
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
        std::vector<float> one_hot_buf;
        if (!fill_prompt_one_hot(one_hot_buf, P, T_oh, /*n_batch=*/1, { pid })) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet run: prompt_id %d out of range "
                    "[0, %d)",
                    pid, P);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_set(eb.prompt_one_hot_in, one_hot_buf.data(), 0, one_hot_buf.size() * sizeof(float));
    }

    // ----- Host-side sinusoidal positional embedding -----
    //
    // Fill eb.pos_emb_in (ne=[d_model, pos_len, 1, 1]). Positions:
    //   full attention: positions[i] = (T_enc - 1) - i, i in [0, 2T-1)
    //   local attention: positions[i] = W_left - i, i in [0, W_left+W_right+1)
    // Per-row sinusoid (identical in both):
    //   div_term[k]  = exp(2k * -ln(10000) / d_model)
    //   pe[i, 2k]    = sin(positions[i] * div_term[k])
    //   pe[i, 2k+1]  = cos(positions[i] * div_term[k])
    if (eb.pos_emb_in != nullptr) {
        const int d_model = pm->hparams.enc_d_model;
        const int pos_len = static_cast<int>(eb.pos_emb_in->ne[1]);

        // Position of "relative offset 0" in the buffer: (pos_len-1)/2 for
        // full / chunked_limited; W_left for Regular local attention.
        const bool is_chunked = (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);
        const bool is_local_pe =
            (!is_chunked) && (pm->hparams.enc_att_context_left >= 0 && pm->hparams.enc_att_context_right >= 0);
        const int zero_index = is_local_pe ? pm->hparams.enc_att_context_left : (pos_len - 1) / 2;

        pc->pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);

        pc->pos_div_term.resize(static_cast<size_t>(d_model / 2));
        const float ln_10000 = std::log(10000.0f);
        for (int k = 0; k < d_model / 2; ++k) {
            pc->pos_div_term[static_cast<size_t>(k)] =
                std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
        }

        for (int i = 0; i < pos_len; ++i) {
            const float pos = static_cast<float>(zero_index - i);
            float *     row = pc->pos_buf.data() + static_cast<size_t>(i) * d_model;
            for (int k = 0; k < d_model / 2; ++k) {
                const float div = pc->pos_div_term[static_cast<size_t>(k)];
                row[2 * k]      = std::sin(pos * div);
                row[2 * k + 1]  = std::cos(pos * div);
            }
        }

        ggml_backend_tensor_set(eb.pos_emb_in, pc->pos_buf.data(), 0, pc->pos_buf.size() * sizeof(float));

        transcribe::debug::dump_tensor("enc.pos_emb", eb.pos_emb_in, "encoder.pos_emb");
    }

    // ChunkedLimited attention mask (streaming variants): 0 on (q, k)
    // pairs whose chunk indices are in [q_chunk - left_chunks, q_chunk],
    // -INF outside. NeMo: chunk_size = att_context_right + 1,
    // left_chunks = att_context_left / chunk_size.
    if (eb.chunked_mask_in != nullptr) {
        const int T_enc       = static_cast<int>(eb.chunked_mask_in->ne[0]);
        const int chunk_size  = pm->hparams.enc_att_context_right + 1;
        const int left_chunks = (chunk_size > 0) ? (pm->hparams.enc_att_context_left / chunk_size) : 0;

        std::vector<float> mask_buf(static_cast<size_t>(T_enc) * static_cast<size_t>(T_enc));
        // ggml ne = [T_k, T_q, 1, 1], row-major in the host buffer is
        // contiguous along T_k (ne[0]). Indexing: mask[q, k] lives at
        // offset (q * T_enc + k).
        for (int q = 0; q < T_enc; ++q) {
            const int q_chunk     = (chunk_size > 0) ? (q / chunk_size) : 0;
            const int k_min_chunk = (q_chunk - left_chunks > 0) ? (q_chunk - left_chunks) : 0;
            const int k_min       = k_min_chunk * chunk_size;
            const int k_max       = (q_chunk + 1) * chunk_size;  // exclusive
            float *   row         = mask_buf.data() + static_cast<size_t>(q) * T_enc;
            for (int k = 0; k < T_enc; ++k) {
                row[k] = (k >= k_min && k < k_max) ? 0.0f : -std::numeric_limits<float>::infinity();
            }
        }

        ggml_backend_tensor_set(eb.chunked_mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(float));
        transcribe::debug::dump_tensor("enc.attn.chunked_mask", eb.chunked_mask_in, "encoder.attn.chunked_mask");
    }

    // Set n_threads on the CPU/BLAS backends (GPU backends ignore it).
    transcribe::configure_sched_n_threads(pc->sched, pc->n_threads);

    // ----- Compute -----
    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(pc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run: ggml_backend_sched_graph_compute failed (%d)",
                static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    pc->t_encode_us = ggml_time_us() - t_enc_start;
    pc->t_decode_us = 0;

    // ----- Dump intermediates (debug only; nullptr fields skipped) -----
    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.pre_encode.out", eb.dumps.pre_encode_out, "encoder.pre_encode");
    try_dump("enc.block.0.ff1", eb.dumps.block0_after_ff1, "encoder.block0.ff1");
    try_dump("enc.block.0.attn", eb.dumps.block0_after_attn, "encoder.block0.attn");
    try_dump("enc.block.0.conv", eb.dumps.block0_after_conv, "encoder.block0.conv");
    try_dump("enc.block.0.ff2", eb.dumps.block0_after_ff2, "encoder.block0.ff2");
    try_dump("enc.block.0.out", eb.dumps.block0_out, "encoder.block0.out");
    // Mid- and last-block spot-check dumps. File name encodes the actual
    // block index (scales with n_layers). last_block_out aliases
    // final_out; dumped under both its block name and "enc.final".
    if (eb.dumps.mid_block_out != nullptr && eb.dumps.mid_block_idx >= 0) {
        char name[64];
        std::snprintf(name, sizeof(name), "enc.block.%d.out", eb.dumps.mid_block_idx);
        transcribe::debug::dump_tensor(name, eb.dumps.mid_block_out, "encoder.block.mid.out");
    }
    if (eb.dumps.last_block_out != nullptr && eb.dumps.last_block_idx >= 0) {
        char name[64];
        std::snprintf(name, sizeof(name), "enc.block.%d.out", eb.dumps.last_block_idx);
        transcribe::debug::dump_tensor(name, eb.dumps.last_block_out, "encoder.block.last.out");
    }
    // Per-block dump for the layer-by-layer divergence bisect (gated by
    // TRANSCRIBE_DUMP_ALL_BLOCKS).
    if (transcribe::debug::dump_all_blocks_requested()) {
        for (size_t i = 0; i < eb.dumps.all_block_outs.size(); ++i) {
            ggml_tensor * t = eb.dumps.all_block_outs[i];
            if (t == nullptr) {
                continue;
            }
            char name[64];
            std::snprintf(name, sizeof(name), "enc.block.%zu.out", i);
            transcribe::debug::dump_tensor(name, t, "encoder.block.bisect");
        }
    }
    // Sub-block intermediates for TRANSCRIBE_DUMP_SUB_BLOCKS (populated
    // by the sub-block observer in encoder.cpp).
    for (const auto & p : eb.dumps.sub_block_dumps) {
        if (p.second == nullptr) {
            continue;
        }
        transcribe::debug::dump_tensor(p.first.c_str(), p.second, "encoder.block.subblock");
    }
    try_dump("enc.final", eb.dumps.final_out, "encoder.final");
    try_dump("enc.prompted", eb.dumps.prompted_out, "encoder.prompted");

    pc->encoder_out = eb.out;

    // ----- TDT decode -----
    //
    // Read the encoder output to host, decode, build the result
    // hierarchy. The activation ne=[d_enc, T_enc, 1, 1] is row-major
    // [T_enc, d_enc] from the host's POV — what the decoder expects. The
    // caller cleared the result snapshot; re-clearing here would reset
    // the cursors stream_finalize is about to commit.
    const int d_enc = static_cast<int>(eb.out->ne[0]);
    const int T_enc = static_cast<int>(eb.out->ne[1]);
    if (d_enc <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet run: encoder output has degenerate shape "
                "[%d, %d]",
                d_enc, T_enc);
        return TRANSCRIBE_ERR_GGUF;
    }
    pc->enc_host.resize(static_cast<size_t>(d_enc) * static_cast<size_t>(T_enc));
    ggml_backend_tensor_get(eb.out, pc->enc_host.data(), 0, pc->enc_host.size() * sizeof(float));

    // Prompt-conditioned models: eb.out is the post-prompt tensor
    // ("dec.enc_out_prompted"); also read back the pre-prompt final_out
    // as "dec.enc_out" so both reference dump names match.
    if (pm->hparams.has_prompt && eb.dumps.final_out != nullptr && transcribe::debug::enabled()) {
        std::vector<float> unprompted(pc->enc_host.size());
        ggml_backend_tensor_get(eb.dumps.final_out, unprompted.data(), 0, unprompted.size() * sizeof(float));
        const long long shape[2] = { T_enc, d_enc };
        transcribe::debug::dump_host_f32("dec.enc_out", unprompted.data(),
                                         static_cast<long long>(T_enc) * static_cast<long long>(d_enc), shape, 2,
                                         "decoder.enc_out");
    }

    const char * enc_dump_name = pm->hparams.has_prompt ? "dec.enc_out_prompted" : nullptr;
    return decode_and_populate(pc, pm, params, pc->enc_host.data(), T_enc, d_enc, /*utt_index=*/-1, enc_dump_name);
}

// One-shot entry point. Validates session/pm, clears the previous result
// snapshot, then forwards to the shared inference helper (also reused by
// the streaming hooks).
transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    // The dispatcher has already enum-validated params and rejected
    // TRANSLATE / sub-TOKEN timestamps; we only resolve AUTO and downcast
    // when the result is built.
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * pc = static_cast<ParakeetSession *>(session);
    auto * pm = static_cast<ParakeetModel *>(pc->model);
    if (pm == nullptr || pm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    pc->clear_result();
    return run_one_shot_inner(pc, pm, pcm, n_samples, params);
}

// ---------------------------------------------------------------------------
// Offline batch (transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// Batches B utterances through ONE encoder graph (batch on the
// activation's ne[2]) in a single device dispatch, then host-decodes each
// slice; mel / decode / result-build are identical to single-shot. Mels
// pack into [T_max, n_mels, 1, n], zero-padded to T_max. Variable-length
// batches build attn key-padding + conv valid-frame masks so a padded
// tail can't corrupt real frames and decode each at its own T_enc;
// same-length batches skip the masks and are bit-identical to single-shot.
static int pre_encode_t_out(int in) {
    // One stride-2, kernel-3, pad-1 conv: floor((in + 2 - 3)/2) + 1.
    return (in - 1) / 2 + 1;
}

static transcribe_status run_batch_encode(ParakeetSession *                       pc,
                                          ParakeetModel *                         pm,
                                          const std::vector<std::vector<float>> & mels,
                                          const std::vector<int> &                nf,
                                          int                                     n_mels,
                                          int                                     T_max,
                                          int64_t                                 total_mel_us,
                                          const transcribe_run_params *           params) {
    const int n       = static_cast<int>(mels.size());
    bool      var_len = false;
    for (int b = 0; b < n; ++b) {
        if (nf[b] != T_max) {
            var_len = true;
            break;
        }
    }

    // Pack mels into [T_max, n_mels, 1, n], zero-padding each along time
    // (channel-major source [n_mels, nf[b]] -> per-utterance [T_max, n_mels]).
    transcribe::pack_pad_channel_major(pc->mel_buf, mels, nf, n_mels, T_max);

    if (pc->compute_ctx != nullptr) {
        ggml_free(pc->compute_ctx);
        pc->compute_ctx = nullptr;
    }
    pc->encoder_out = nullptr;
    {
        ggml_init_params init_params{};
        init_params.mem_size   = 4 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        pc->compute_ctx        = ggml_init(init_params);
        if (pc->compute_ctx == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    EncoderBuild eb = build_encoder_graph(pc->compute_ctx, pm->weights, pm->hparams, T_max, resolved_kv,
                                          pm->backend.c_str(), /*buf_mask=*/nullptr,
                                          /*n_batch=*/n, /*batch_var_len=*/var_len);
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (pc->sched == nullptr) {
        pc->sched = ggml_backend_sched_new(pm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(pm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (pc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(pc->sched);
    if (!ggml_backend_sched_alloc_graph(pc->sched, eb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_tensor_set(eb.mel_in, pc->mel_buf.data(), 0, pc->mel_buf.size() * sizeof(float));

    const int d_enc = static_cast<int>(eb.out->ne[0]);
    const int T_enc = static_cast<int>(eb.out->ne[1]);
    if (d_enc <= 0 || T_enc <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Per-utterance valid encoder-frame count (the same subsample the conv
    // stack applies: 3 stride-2 convs on the time axis).
    std::vector<int> real_tenc(static_cast<size_t>(n), T_enc);
    if (var_len) {
        for (int b = 0; b < n; ++b) {
            int t        = nf[b];
            t            = pre_encode_t_out(t);
            t            = pre_encode_t_out(t);
            t            = pre_encode_t_out(t);
            real_tenc[b] = std::min(t, T_enc);
        }
        // Attention key-padding mask [T_enc, 1, 1, n] (0 real / -INF padded)
        // and conv valid-frame mask [T_enc, 1, n, 1] (1 real / 0 padded).
        transcribe::fill_keypad_mask(eb.attn_pad_mask_in, real_tenc, T_enc, n);
        transcribe::fill_valid_frame_mask(eb.conv_pad_mask_in, real_tenc, T_enc, n);

        // Pre-encode valid-frame masks (masked subsampling). One per ReLU
        // stage; the valid time length at stage k is the per-utterance mel
        // length downsampled k times by the (in-1)/2+1 conv formula. Each
        // mask is ne=[1, H_stage, 1, n] -> host index b*H_stage + h.
        auto fill_pe_mask = [&](ggml_tensor * mask, int n_down) {
            if (mask == nullptr) {
                return;
            }
            const int          H = static_cast<int>(mask->ne[1]);
            std::vector<float> mb(static_cast<size_t>(H) * n, 0.0f);
            for (int b = 0; b < n; ++b) {
                int v = nf[b];
                for (int d = 0; d < n_down; ++d) {
                    v = pre_encode_t_out(v);
                }
                if (v > H) {
                    v = H;
                }
                for (int h = 0; h < v; ++h) {
                    mb[static_cast<size_t>(b) * H + h] = 1.0f;
                }
            }
            ggml_backend_tensor_set(mask, mb.data(), 0, mb.size() * sizeof(float));
        };
        fill_pe_mask(eb.pre_encode_mask_s1_in, 1);  // after relu0
        fill_pe_mask(eb.pre_encode_mask_s2_in, 2);  // after relu3
        fill_pe_mask(eb.pre_encode_mask_s3_in, 3);  // after relu6
    }

    // Positional embedding (batch-independent; depends only on T_enc).
    if (eb.pos_emb_in != nullptr) {
        const int  d_model    = pm->hparams.enc_d_model;
        const int  pos_len    = static_cast<int>(eb.pos_emb_in->ne[1]);
        const bool is_chunked = (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);
        const bool is_local_pe =
            (!is_chunked) && (pm->hparams.enc_att_context_left >= 0 && pm->hparams.enc_att_context_right >= 0);
        const int zero_index = is_local_pe ? pm->hparams.enc_att_context_left : (pos_len - 1) / 2;
        pc->pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);
        pc->pos_div_term.resize(static_cast<size_t>(d_model / 2));
        const float ln_10000 = std::log(10000.0f);
        for (int k = 0; k < d_model / 2; ++k) {
            pc->pos_div_term[static_cast<size_t>(k)] =
                std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
        }
        for (int i = 0; i < pos_len; ++i) {
            const float pos = static_cast<float>(zero_index - i);
            float *     row = pc->pos_buf.data() + static_cast<size_t>(i) * d_model;
            for (int k = 0; k < d_model / 2; ++k) {
                const float div = pc->pos_div_term[static_cast<size_t>(k)];
                row[2 * k]      = std::sin(pos * div);
                row[2 * k + 1]  = std::cos(pos * div);
            }
        }
        ggml_backend_tensor_set(eb.pos_emb_in, pc->pos_buf.data(), 0, pc->pos_buf.size() * sizeof(float));
    }

    // Prompt one-hot upload (multilingual variants). Resolves
    // params->language once for the whole batch (no per-utterance language
    // array in the ABI) and replicates across all (T_enc, B) slots.
    if (eb.prompt_one_hot_in != nullptr) {
        const int     P         = static_cast<int>(eb.prompt_one_hot_in->ne[0]);
        const int     T_oh      = static_cast<int>(eb.prompt_one_hot_in->ne[1]);
        const char *  lang_hint = (params != nullptr) ? params->language : nullptr;
        const int32_t pid       = resolve_prompt_id(pm->hparams, lang_hint);
        if (pid < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet run_batch: language %s%s%s not in prompt "
                    "dictionary",
                    lang_hint ? "\"" : "", lang_hint ? lang_hint : "<null>", lang_hint ? "\"" : "");
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
        std::vector<int32_t> pids(static_cast<size_t>(n), pid);
        std::vector<float>   one_hot_buf;
        if (!fill_prompt_one_hot(one_hot_buf, P, T_oh, /*n_batch=*/n, pids)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet run_batch: prompt_id %d out of range "
                    "[0, %d)",
                    pid, P);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_set(eb.prompt_one_hot_in, one_hot_buf.data(), 0, one_hot_buf.size() * sizeof(float));
    }

    // ChunkedLimited attention mask (cache-aware variants), same as
    // run_one_shot_inner. The pattern depends only on T_enc, shared
    // across the batch, so one mask broadcasts.
    if (eb.chunked_mask_in != nullptr) {
        const int          Tk          = static_cast<int>(eb.chunked_mask_in->ne[0]);
        const int          chunk_size  = pm->hparams.enc_att_context_right + 1;
        const int          left_chunks = (chunk_size > 0) ? (pm->hparams.enc_att_context_left / chunk_size) : 0;
        std::vector<float> mask_buf(static_cast<size_t>(Tk) * Tk);
        for (int q = 0; q < Tk; ++q) {
            const int q_chunk     = (chunk_size > 0) ? (q / chunk_size) : 0;
            const int k_min_chunk = (q_chunk - left_chunks > 0) ? (q_chunk - left_chunks) : 0;
            const int k_min       = k_min_chunk * chunk_size;
            const int k_max       = (q_chunk + 1) * chunk_size;
            float *   row         = mask_buf.data() + static_cast<size_t>(q) * Tk;
            for (int k = 0; k < Tk; ++k) {
                row[k] = (k >= k_min && k < k_max) ? 0.0f : -std::numeric_limits<float>::infinity();
            }
        }
        ggml_backend_tensor_set(eb.chunked_mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(float));
    }

    transcribe::configure_sched_n_threads(pc->sched, pc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(pc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet run_batch: graph_compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    pc->t_encode_us = ggml_time_us() - t_enc_start;

    // Bisect dump (debug only): the batched encoder intermediates as full
    // [d_model, T_max, n] tensors, so a batched-vs-single divergence can be
    // located per stage. Gated on TRANSCRIBE_DUMP_ALL_BLOCKS.
    if (transcribe::debug::enabled() && transcribe::debug::dump_all_blocks_requested()) {
        if (eb.dumps.pre_encode_out != nullptr) {
            transcribe::debug::dump_tensor("enc.pre_encode.out", eb.dumps.pre_encode_out, "encoder.pre_encode");
        }
        for (size_t i = 0; i < eb.dumps.all_block_outs.size(); ++i) {
            ggml_tensor * t = eb.dumps.all_block_outs[i];
            if (t == nullptr) {
                continue;
            }
            char name[64];
            std::snprintf(name, sizeof(name), "enc.block.%zu.out", i);
            transcribe::debug::dump_tensor(name, t, "encoder.block.bisect");
        }
    }

    const size_t utt_elems = static_cast<size_t>(d_enc) * static_cast<size_t>(T_enc);
    pc->enc_host.resize(utt_elems * static_cast<size_t>(n));
    ggml_backend_tensor_get(eb.out, pc->enc_host.data(), 0, pc->enc_host.size() * sizeof(float));

    // Prompt-conditioned variants: dump the PRE-prompt final_out per
    // utterance as "dec.enc_out.b{i}" (the post-prompt eb.out is dumped
    // via decode_and_populate's override). Mirrors single-shot.
    const bool         has_prompt = pm->hparams.has_prompt && eb.dumps.final_out != nullptr;
    std::vector<float> unprompted_host;
    if (has_prompt && transcribe::debug::enabled()) {
        unprompted_host.resize(pc->enc_host.size());
        ggml_backend_tensor_get(eb.dumps.final_out, unprompted_host.data(), 0, unprompted_host.size() * sizeof(float));
        for (int b = 0; b < n; ++b) {
            const long long shape[2] = { real_tenc[b], d_enc };
            char            namebuf[64];
            std::snprintf(namebuf, sizeof(namebuf), "dec.enc_out.b%d", b);
            transcribe::debug::dump_host_f32(namebuf, unprompted_host.data() + static_cast<size_t>(b) * utt_elems,
                                             static_cast<long long>(real_tenc[b]) * static_cast<long long>(d_enc),
                                             shape, 2, "decoder.enc_out");
        }
    }
    const char * enc_dump_name = has_prompt ? "dec.enc_out_prompted" : nullptr;

    // Host-slice the shared encoder output and decode each utterance.
    // decode_batch_slices amortizes the one shared encoder + total mel
    // cost across the batch; decode is genuinely per-utterance.
    return transcribe::decode_batch_slices(
        pc, n, pc->enc_host.data(), utt_elems, pc->t_encode_us, total_mel_us, [&](int b, const float * enc_b) {
            return decode_and_populate(pc, pm, params, enc_b, real_tenc[b], d_enc, /*utt_index=*/b, enc_dump_name);
        });
}

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * pc = static_cast<ParakeetSession *>(session);
    auto * pm = static_cast<ParakeetModel *>(pc->model);
    if (pm == nullptr || pm->plan.scheduler_list.empty() || !pm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    transcribe::debug::init();

    // Compute each utterance's mel in parallel (pure host, no
    // cross-utterance state). A malformed utterance falls the whole call
    // back to the per-utterance path (keeps the batch tensor rectangular).
    // n_mels collected per-index to avoid a shared write.
    std::vector<std::vector<float>> mels(static_cast<size_t>(n));
    std::vector<int>                nf(static_cast<size_t>(n), 0);
    std::vector<int>                n_mels_per(static_cast<size_t>(n), 0);
    const int64_t                   t_mel_start  = ggml_time_us();
    const bool                      all_ok       = transcribe::parallel_for_all(n, pc->n_threads, [&](int i) -> bool {
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            return false;
        }
        int                     this_mels = 0, this_frames = 0;
        const transcribe_status st =
            pm->mel->compute(pcm[i], static_cast<size_t>(n_samples[i]), mels[i], this_mels, this_frames);
        if (st != TRANSCRIBE_OK || this_frames <= 0) {
            return false;
        }
        nf[i]         = this_frames;
        n_mels_per[i] = this_mels;
        return true;
    });
    const int64_t                   total_mel_us = ggml_time_us() - t_mel_start;

    if (all_ok) {
        int T_max = 0, n_mels = 0;
        for (int i = 0; i < n; ++i) {
            T_max  = std::max(T_max, nf[i]);
            n_mels = std::max(n_mels, n_mels_per[i]);
        }
        return run_batch_encode(pc, pm, mels, nf, n_mels, T_max, total_mel_us, params);
    }

    // Per-utterance fallback (also the malformed-input path).
    for (int i = 0; i < n; ++i) {
        if (pc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            transcribe_session::ResultSet rs;
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            pc->batch_results.push_back(std::move(rs));
            continue;
        }
        pc->clear_result();
        const transcribe_status st = run_one_shot_inner(pc, pm, pcm[i], n_samples[i], params);
        pc->batch_results.push_back(pc->capture_result(st));
    }
    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Streaming hooks
// ---------------------------------------------------------------------------
//
// Only streaming variants reach these hooks (load() leaves
// supports_streaming = false otherwise; the gate in stream_begin is
// defense in depth). The dispatcher handles the lifecycle and the
// result-snapshot counters; the family hooks own the per-utterance audio
// scratch.

constexpr int64_t k_sample_rate_hz = 16000;

static int64_t samples_to_us(int64_t n_samples) {
    return (n_samples * 1000000) / k_sample_rate_hz;
}

static int64_t us_to_ms(int64_t us) {
    return us / 1000;
}

// ---------------------------------------------------------------------------
// Streaming-encoder helpers
// ---------------------------------------------------------------------------
//
// Chunk geometry lives on ParakeetStreamingCaches, resolved per stream
// from ParakeetHParams + the caller-selected att_context_right.

// Fill the sinusoidal pos_emb buffer for a streaming chunk. Same layout
// as the offline ChunkedLimited path: pos_len = 2*T_virtual - 1,
// positions[i] = (T_virtual - 1) - i, div_term[k] = exp(2k * -ln(10000)
// / d_model). pos_emb_in is ne=[d_model, pos_len, 1, 1] f32. Resizes
// pc->pos_buf / pc->pos_div_term in place.
void fill_streaming_pos_emb(ParakeetSession * pc, ggml_tensor * pos_emb_in, int d_model, int zero_index) {
    const int pos_len = static_cast<int>(pos_emb_in->ne[1]);

    pc->pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);
    pc->pos_div_term.resize(static_cast<size_t>(d_model / 2));
    const float ln_10000 = std::log(10000.0f);
    for (int k = 0; k < d_model / 2; ++k) {
        pc->pos_div_term[static_cast<size_t>(k)] =
            std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
    }
    for (int i = 0; i < pos_len; ++i) {
        const float pos = static_cast<float>(zero_index - i);
        float *     row = pc->pos_buf.data() + static_cast<size_t>(i) * d_model;
        for (int k = 0; k < d_model / 2; ++k) {
            const float div = pc->pos_div_term[static_cast<size_t>(k)];
            row[2 * k]      = std::sin(pos * div);
            row[2 * k + 1]  = std::cos(pos * div);
        }
    }
    ggml_backend_tensor_set(pos_emb_in, pc->pos_buf.data(), 0, pc->pos_buf.size() * sizeof(float));
}

// Fill the streaming attention mask. Square shape [T_virtual,T_virtual]
// in ggml ne (broadcasts across n_heads inside rel_pos_mhsa). Rules:
//   - Within the chunked-limited band (k in [q-att_left..q+att_right] in
//     chunk-aligned terms): 0
//   - Outside the band: -INF
//   - Additionally for any query: cache-prefix keys whose slot has not
//     been written yet (k < T_cache - channel_len): -INF (NeMo's
//     cache_last_channel_len-driven masking).
void fill_streaming_chunked_mask(ggml_tensor * mask_in,
                                 int           T_virtual,
                                 int           T_cache,
                                 int           channel_len,
                                 int           att_context_left,
                                 int           att_context_right) {
    const int n_q_rows                = static_cast<int>(mask_in->ne[1]);
    const int q_first                 = T_virtual - n_q_rows;
    const int chunk_size              = att_context_right + 1;
    const int left_chunks             = (chunk_size > 0) ? (att_context_left / chunk_size) : 0;
    const int invalid_cache_threshold = T_cache - channel_len;

    std::vector<float> mask_buf(static_cast<size_t>(n_q_rows) * static_cast<size_t>(T_virtual));
    for (int q = q_first; q < T_virtual; ++q) {
        const int q_chunk     = (chunk_size > 0) ? (q / chunk_size) : 0;
        const int k_min_chunk = (q_chunk - left_chunks > 0) ? (q_chunk - left_chunks) : 0;
        const int k_min       = k_min_chunk * chunk_size;
        const int k_max       = (q_chunk + 1) * chunk_size;  // exclusive
        float *   row         = mask_buf.data() + static_cast<size_t>(q - q_first) * T_virtual;
        for (int k = 0; k < T_virtual; ++k) {
            const bool in_band        = (k >= k_min && k < k_max);
            const bool cache_unfilled = (k < invalid_cache_threshold);
            row[k]                    = (in_band && !cache_unfilled) ? 0.0f : -std::numeric_limits<float>::infinity();
        }
    }
    ggml_backend_tensor_set(mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(float));
}

// Rebuild the per-layer rel-pos projection cache for `pos_len` (see
// ParakeetStreamingCaches::pos_proj). Runs a one-off graph (n_layers
// mul_mats + layout massaging) through the session scheduler, reading
// pos_emb from pc->pos_buf. Called only on a geometry change — twice per
// stream in practice (first-chunk, then steady state).
transcribe_status ensure_pos_proj_cache(ParakeetSession * pc, ParakeetModel * pm, int pos_len) {
    const auto & hp       = pm->hparams;
    const int    n_layers = static_cast<int>(pm->weights.blocks.size());
    const int    d_model  = hp.enc_d_model;
    const int    n_head   = hp.enc_n_heads;
    const int    head_dim = d_model / n_head;

    if (pc->pos_buf.size() < static_cast<size_t>(pos_len) * static_cast<size_t>(d_model)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet stream: pos_proj cache fill without pos_buf "
                "(%zu < %d*%d)",
                pc->pos_buf.size(), pos_len, d_model);
        return TRANSCRIBE_ERR_BACKEND;
    }

    auto & sc = pc->stream_caches;
    if (sc.pos_proj_buf != nullptr) {
        safe_buffer_free(sc.pos_proj_buf);
        sc.pos_proj_buf = nullptr;
    }
    if (sc.pos_proj_ctx != nullptr) {
        ggml_free(sc.pos_proj_ctx);
        sc.pos_proj_ctx = nullptr;
    }
    sc.pos_proj.assign(static_cast<size_t>(n_layers), nullptr);
    sc.pos_proj_len = -1;

    ggml_init_params pip{};
    pip.mem_size    = (static_cast<size_t>(n_layers) + 2) * ggml_tensor_overhead();
    pip.mem_buffer  = nullptr;
    pip.no_alloc    = true;
    sc.pos_proj_ctx = ggml_init(pip);
    if (sc.pos_proj_ctx == nullptr) {
        return TRANSCRIBE_ERR_OOM;
    }

    for (int i = 0; i < n_layers; ++i) {
        ggml_tensor * t = ggml_new_tensor_3d(sc.pos_proj_ctx, GGML_TYPE_F32, head_dim, pos_len, n_head);
        if (t == nullptr) {
            return TRANSCRIBE_ERR_OOM;
        }
        char nm[64];
        std::snprintf(nm, sizeof(nm), "stream.pos_proj.%d", i);
        ggml_set_name(t, nm);
        sc.pos_proj[static_cast<size_t>(i)] = t;
    }
    sc.pos_proj_buf = ggml_backend_alloc_ctx_tensors(sc.pos_proj_ctx, pm->plan.primary);
    if (sc.pos_proj_buf == nullptr) {
        return TRANSCRIBE_ERR_OOM;
    }

    // One-off projection graph.
    ggml_init_params gip{};
    gip.mem_size        = 4 * 1024 * 1024;
    gip.mem_buffer      = nullptr;
    gip.no_alloc        = true;
    ggml_context * gctx = ggml_init(gip);
    if (gctx == nullptr) {
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_cgraph * graph  = ggml_new_graph_custom(gctx, 512, false);
    ggml_tensor * pos_in = ggml_new_tensor_2d(gctx, GGML_TYPE_F32, d_model, pos_len);
    ggml_set_name(pos_in, "pos_proj.pos_emb.in");
    ggml_set_input(pos_in);
    for (int i = 0; i < n_layers; ++i) {
        ggml_tensor * p   = ggml_mul_mat(gctx, pm->weights.blocks[static_cast<size_t>(i)].attn_pos_w, pos_in);
        p                 = ggml_reshape_4d(gctx, p, head_dim, n_head, pos_len, 1);
        p                 = ggml_cont(gctx, ggml_permute(gctx, p, 0, 2, 1, 3));
        ggml_tensor * cpy = ggml_cpy(gctx, p, sc.pos_proj[static_cast<size_t>(i)]);
        ggml_build_forward_expand(graph, cpy);
    }

    transcribe_status st = TRANSCRIBE_OK;
    ggml_backend_sched_reset(pc->sched);
    if (!ggml_backend_sched_alloc_graph(pc->sched, graph)) {
        st = TRANSCRIBE_ERR_BACKEND;
    } else {
        ggml_backend_tensor_set(pos_in, pc->pos_buf.data(), 0, static_cast<size_t>(pos_len) * d_model * sizeof(float));
        if (ggml_backend_sched_graph_compute(pc->sched, graph) != GGML_STATUS_SUCCESS) {
            st = TRANSCRIBE_ERR_BACKEND;
        }
    }
    ggml_free(gctx);
    if (st == TRANSCRIBE_OK) {
        sc.pos_proj_len = pos_len;
    }
    return st;
}

}  // namespace

// Declared in parakeet.h (non-anonymous) so unit tests and the buffered
// driver can link against it.
void compute_chunked_limited_with_rc_mask(float * out_buf,
                                          int     T,
                                          int     left_context_frames,
                                          int     chunk_size_frames,
                                          int     right_context_frames,
                                          int     pad_length) {
    assert(out_buf != nullptr);
    assert(T >= 1);
    assert(chunk_size_frames >= 1);
    assert(left_context_frames >= 0);
    assert(right_context_frames >= 0);
    assert(pad_length >= 0);

    const int L = left_context_frames;
    const int C = chunk_size_frames;
    const int R = right_context_frames;
    // Clamp pad_length to [0, T]. pad_length >= T means "no pad mask" —
    // every frame is valid.
    const int P = pad_length > T ? T : pad_length;

    for (int q = 0; q < T; ++q) {
        const int  c_q                    = q / C;
        const int  window_start_unclamped = c_q * C - L;
        const int  window_end_unclamped   = c_q * C + C - 1 + R;
        const int  window_start           = window_start_unclamped > 0 ? window_start_unclamped : 0;
        const int  window_end             = window_end_unclamped < (T - 1) ? window_end_unclamped : (T - 1);
        const bool q_padded               = q >= P;

        float * row = out_buf + static_cast<size_t>(q) * static_cast<size_t>(T);
        for (int k = 0; k < T; ++k) {
            const bool k_padded = k >= P;
            const bool allowed  = (k >= window_start && k <= window_end) && !q_padded && !k_padded;
            row[k]              = allowed ? 0.0f : -std::numeric_limits<float>::infinity();
        }
    }
}

namespace {

// Build, run, and post-process a single streaming encoder chunk.
//
//   mel_chunk_data         row-major [n_mels, n_mel_chunk_frames] f32.
//   n_mel_chunk_frames     mel frames in this chunk (history prepend +
//                          new).
//   drop_extra_pre_encoded 0 for first chunk, else drop_extra_subsequent.
//   mel_frames_advance     mel frames this chunk consumes from the input
//                          (NOT n_mel_chunk_frames when a cache prepend
//                          is in play).
// On success, appends TdtTokens to pc->raw_tokens and advances the cache
// + decoder state.
transcribe_status emit_streaming_chunk(ParakeetSession * pc,
                                       ParakeetModel *   pm,
                                       const float *     mel_chunk_data,
                                       int               n_mel_chunk_frames,
                                       int               drop_extra_pre_encoded,
                                       int               mel_frames_advance) {
    const auto & hp       = pm->hparams;
    const int    n_layers = static_cast<int>(pm->weights.blocks.size());

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    // One compute_ctx per chunk (matches the offline run() lifecycle).
    if (pc->compute_ctx != nullptr) {
        ggml_free(pc->compute_ctx);
        pc->compute_ctx = nullptr;
    }
    ggml_init_params ip{};
    ip.mem_size     = 16 * 1024 * 1024;
    ip.mem_buffer   = nullptr;
    ip.no_alloc     = true;
    pc->compute_ctx = ggml_init(ip);
    if (pc->compute_ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream: ggml_init compute_ctx failed");
        return TRANSCRIBE_ERR_OOM;
    }

    const int  step_num = pc->stream_caches.chunk_step;
    const bool dump_on  = transcribe::debug::enabled();

    // Per-step dump layer selection: match the Python dumper's default
    // ({0, n/2, n-1}). The selected layers are read AFTER graph_compute
    // for cache_out and BEFORE for cache_in.
    auto sel_layers = [&]() {
        std::vector<int> v;
        if (n_layers <= 0) {
            return v;
        }
        std::set<int> s = { 0, n_layers / 2, n_layers - 1 };
        for (int i : s) {
            if (i >= 0 && i < n_layers) {
                v.push_back(i);
            }
        }
        std::sort(v.begin(), v.end());
        return v;
    }();

    // Dump mel_in + cache_in (snapshot of inputs to this chunk).
    if (dump_on) {
        char namebuf[128];
        std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.mel_in", step_num);
        const long long mel_shape[2] = { hp.fe_num_mels, n_mel_chunk_frames };
        transcribe::debug::dump_host_f32(namebuf, mel_chunk_data,
                                         static_cast<long long>(hp.fe_num_mels) * n_mel_chunk_frames, mel_shape, 2,
                                         "streaming.mel_in");
        for (int L : sel_layers) {
            std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.cache_lc_in_%d", step_num, L);
            transcribe::debug::dump_tensor(namebuf, pc->stream_caches.last_channel[L], "streaming.cache_in");
            std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.cache_lt_in_%d", step_num, L);
            transcribe::debug::dump_tensor(namebuf, pc->stream_caches.last_time[L], "streaming.cache_in");
        }
    }

    StreamingEncoderCacheIO cache_io;
    cache_io.channel_in   = pc->stream_caches.last_channel;
    cache_io.time_in      = pc->stream_caches.last_time;
    cache_io.k_in         = pc->stream_caches.last_k;
    cache_io.v_in         = pc->stream_caches.last_v;
    cache_io.pos_proj     = pc->stream_caches.pos_proj;
    cache_io.pos_proj_len = pc->stream_caches.pos_proj_len;

    EncoderBuild eb = build_encoder_graph_streaming(pc->compute_ctx, pm->weights, hp, n_mel_chunk_frames,
                                                    drop_extra_pre_encoded, cache_io, resolved_kv, pm->backend.c_str());
    if (eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (pc->sched == nullptr) {
        pc->sched = ggml_backend_sched_new(pm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(pm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (pc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream: sched_new failed");
            return TRANSCRIBE_ERR_BACKEND;
        }
    }
    ggml_backend_sched_reset(pc->sched);
    if (!ggml_backend_sched_alloc_graph(pc->sched, eb.graph)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream: alloc_graph failed");
        return TRANSCRIBE_ERR_BACKEND;
    }

    // Upload mel chunk. Row-major [n_mels, n_mel_chunk_frames] is
    // byte-identical to ggml ne=[n_mel_chunk_frames, n_mels, 1, 1].
    ggml_backend_tensor_set(
        eb.mel_in, mel_chunk_data, 0,
        static_cast<size_t>(n_mel_chunk_frames) * static_cast<size_t>(hp.fe_num_mels) * sizeof(float));

    const int T_virtual = static_cast<int>(eb.chunked_mask_in->ne[0]);
    const int T_cache   = hp.enc_att_context_left;
    const int T_q_new   = T_virtual - T_cache;

    // Build & upload pos_emb. The zero-offset row sits at T_virtual - 1.
    // Null when every block consumes the memoized rel-pos projection.
    if (eb.pos_emb_in != nullptr) {
        fill_streaming_pos_emb(pc, eb.pos_emb_in, hp.enc_d_model,
                               /*zero_index=*/T_virtual - 1);
    }

    // Build & upload the chunked mask with cache-unfilled prefix masking.
    // The band uses the (left, right) RESOLVED for this stream, not the
    // model-default hparams (correct by construction even if a future
    // variant's menu breaks today's coincidence).
    fill_streaming_chunked_mask(eb.chunked_mask_in, T_virtual, T_cache, pc->stream_caches.channel_len,
                                pc->stream_caches.att_context_left, pc->stream_caches.att_context_right);

    // Prompt one-hot upload (multilingual variants only). Same shape as
    // the offline path; the language hint lives in pc->stream_run_params.
    if (eb.prompt_one_hot_in != nullptr) {
        const int     P         = static_cast<int>(eb.prompt_one_hot_in->ne[0]);
        const int     T_oh      = static_cast<int>(eb.prompt_one_hot_in->ne[1]);
        const char *  lang_hint = pc->stream_run_params.language;
        const int32_t pid       = resolve_prompt_id(pm->hparams, lang_hint);
        if (pid < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet stream: language %s%s%s not in prompt "
                    "dictionary",
                    lang_hint ? "\"" : "", lang_hint ? lang_hint : "<null>", lang_hint ? "\"" : "");
            return TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE;
        }
        std::vector<float> one_hot_buf;
        if (!fill_prompt_one_hot(one_hot_buf, P, T_oh, /*n_batch=*/1, { pid })) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet stream: prompt_id %d out of range "
                    "[0, %d)",
                    pid, P);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_set(eb.prompt_one_hot_in, one_hot_buf.data(), 0, one_hot_buf.size() * sizeof(float));
    }

    // Thread count (same recipe as offline run()).
    transcribe::configure_sched_n_threads(pc->sched, pc->n_threads);

    if (const ggml_status gs = ggml_backend_sched_graph_compute(pc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet stream: graph_compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }

    // Read encoder output back to host.
    const int d_enc = static_cast<int>(eb.out->ne[0]);
    pc->enc_host.resize(static_cast<size_t>(d_enc) * static_cast<size_t>(T_q_new));
    ggml_backend_tensor_get(eb.out, pc->enc_host.data(), 0, pc->enc_host.size() * sizeof(float));

    // Dump enc_out + cache_out, BEFORE the cache rotation so the
    // cache_out tensors are still the freshly-computed ones.
    if (dump_on) {
        char namebuf[128];
        std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.enc_out", step_num);
        // Match the Python dumper's squeeze: T_q_new == 1 (R=0) drops the
        // leading size-1 dim so the on-disk shape is [d_enc] not [1, d_enc].
        if (T_q_new == 1) {
            const long long enc_shape[1] = { d_enc };
            transcribe::debug::dump_host_f32(namebuf, pc->enc_host.data(), d_enc, enc_shape, 1, "streaming.enc_out");
        } else {
            const long long enc_shape[2] = { T_q_new, d_enc };
            transcribe::debug::dump_host_f32(namebuf, pc->enc_host.data(), static_cast<long long>(T_q_new) * d_enc,
                                             enc_shape, 2, "streaming.enc_out");
        }
        for (int L : sel_layers) {
            std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.cache_lc_out_%d", step_num, L);
            transcribe::debug::dump_tensor(namebuf, cache_io.channel_out[L], "streaming.cache_out");
            std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.cache_lt_out_%d", step_num, L);
            transcribe::debug::dump_tensor(namebuf, cache_io.time_out[L], "streaming.cache_out");
        }
    }

    // Rotate per-layer caches: cache_out → persistent cache_in
    // (backend-side copy, no host roundtrip). KV-cache mode rotates
    // last_k/last_v (channel_out is null); the recompute path rotates
    // last_channel. The time (conv) cache rotates in both. An unallocated
    // cache_out here is a builder bug.
    const bool kv_mode = !cache_io.k_out.empty();
    for (int i = 0; i < n_layers; ++i) {
        ggml_tensor * ch      = cache_io.channel_out[i];
        ggml_tensor * tm      = cache_io.time_out[i];
        ggml_tensor * ko      = kv_mode ? cache_io.k_out[i] : nullptr;
        ggml_tensor * vo      = kv_mode ? cache_io.v_out[i] : nullptr;
        const bool    attn_ok = kv_mode ?
                                    (ko != nullptr && ko->buffer != nullptr && vo != nullptr && vo->buffer != nullptr) :
                                    (ch != nullptr && ch->buffer != nullptr);
        if (!attn_ok || tm == nullptr || tm->buffer == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet stream: cache_out unallocated at layer %d "
                    "(att_context_right=%d) — builder bug",
                    i, pc->stream_caches.att_context_right);
            return TRANSCRIBE_ERR_BACKEND;
        }
        if (kv_mode) {
            ggml_backend_tensor_copy(ko, pc->stream_caches.last_k[i]);
            ggml_backend_tensor_copy(vo, pc->stream_caches.last_v[i]);
        } else {
            ggml_backend_tensor_copy(ch, pc->stream_caches.last_channel[i]);
        }
        ggml_backend_tensor_copy(tm, pc->stream_caches.last_time[i]);
    }
    pc->stream_caches.channel_len = std::min(T_cache, pc->stream_caches.channel_len + T_q_new);

    // Refill the rel-pos projection memo on geometry change. Must come
    // after the cache rotation: ensure_pos_proj_cache resets the
    // scheduler, releasing this chunk's compute buffers (cache_out lives
    // there). A failure only loses the memoization, so it logs and degrades.
    if (eb.pos_emb_in != nullptr) {
        const int pos_len_cur = static_cast<int>(eb.pos_emb_in->ne[1]);
        if (pos_len_cur != pc->stream_caches.pos_proj_len) {
            if (const transcribe_status st = ensure_pos_proj_cache(pc, pm, pos_len_cur); st != TRANSCRIBE_OK) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet stream: pos_proj cache fill failed (%s) — "
                        "continuing without memoization",
                        transcribe_status_string(st));
            }
        }
    }

    if (dump_on) {
        char namebuf[128];
        std::snprintf(namebuf, sizeof(namebuf), "stream.chunk.%d.channel_len", step_num);
        const float     channel_len_f = static_cast<float>(pc->stream_caches.channel_len);
        const long long len_shape[1]  = { 1 };
        transcribe::debug::dump_host_f32(namebuf, &channel_len_f, 1, len_shape, 1, "streaming.channel_len");
    }
    pc->stream_caches.chunk_step += 1;

    // Run streaming RNN-T decoder on the new encoder frames.
    if (const transcribe_status st = decode_rnnt_greedy_streaming(
            pm->host_decoder, pc->enc_host.data(), T_q_new, d_enc, pc->stream_dec_state.lstm_state,
            pc->stream_dec_state.prev_token_id, static_cast<int>(pc->stream_dec_state.frame_offset), pc->n_threads,
            pc->raw_tokens);
        st != TRANSCRIBE_OK) {
        return st;
    }

    pc->stream_dec_state.frame_offset += T_q_new;
    // Cursor advances by mel_frames_advance (NeMo's shift_size),
    // INDEPENDENT of the fed chunk (which may include a cache prepend).
    pc->stream_caches.mel_frames_consumed += mel_frames_advance;
    return TRANSCRIBE_OK;
}

// Rebuild the public result vectors (tokens, full_text) from the
// committed raw_tokens; idempotent. Words/segments are NOT populated
// during streaming (deferred to finalize). result_kind is TOKEN whenever
// tokens are present — each gets real t0/t1 from step_at_emit, the
// strongest honest answer at this granularity.
void rebuild_streaming_result_text(ParakeetSession * pc, const ParakeetModel * pm) {
    pc->tokens.clear();
    pc->words.clear();
    pc->segments.clear();
    pc->full_text.clear();

    if (pc->raw_tokens.empty()) {
        pc->has_result  = false;
        pc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        return;
    }

    const double frame_to_ms = parakeet_ms_per_enc_frame(pm->hparams);

    // Strip CONTROL / <ll-RR> tag pieces from the public result, mirroring
    // decode_and_populate (gated on keep_special_tags). Only the public
    // projection is filtered; pc->raw_tokens stays whole.
    const transcribe::Tokenizer & tok        = pm->tok;
    const bool                    strip_tags = !pc->stream_run_params.keep_special_tags;

    pc->tokens.reserve(pc->raw_tokens.size());
    std::vector<int32_t> all_ids;
    all_ids.reserve(pc->raw_tokens.size());
    for (const auto & tk : pc->raw_tokens) {
        if (strip_tags && is_strippable_special(tok, tk.id)) {
            continue;
        }
        transcribe_session::TokenEntry te;
        te.id    = tk.id;
        te.p     = tk.p;
        te.t0_ms = static_cast<int64_t>(std::llround(frame_to_ms * static_cast<double>(tk.step_at_emit)));
        te.t1_ms =
            static_cast<int64_t>(std::llround(frame_to_ms * static_cast<double>(tk.step_at_emit + tk.duration_frames)));
        te.seg_index  = 0;
        te.word_index = -1;
        te.text       = tok.decode(&tk.id, 1);
        pc->tokens.push_back(std::move(te));
        all_ids.push_back(tk.id);
    }
    pc->full_text = tok.decode(all_ids.data(), static_cast<int>(all_ids.size()));
    normalize_transcript_whitespace(pc->full_text);
    pc->has_result  = true;
    pc->result_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;
}

// ----- Buffered streaming (parakeet-unified-en-0.6b) -----
//
// Mirrors NeMo's speech_to_text_streaming_infer_rnnt.py. Variable-stride
// per step: step 0 num_new = samples_chunk + samples_right; steady state
// num_new = samples_chunk; the final step (finalize) consumes the rest
// and folds the right slot into chunk. Each step updates the buffer's
// ContextSize (buf_ctx_*), slices the encoder window, computes mel,
// builds the graph with a BufferedStreamMaskOverride, then slices off the
// ctx_left frames and decodes ctx_chunk (or all remaining on last) with a
// carried RNN-T LstmState.
static void buf_ctx_add_frames(ParakeetSession * pc, int64_t num_new, bool is_last) {
    pc->buf_ctx_left += pc->buf_ctx_chunk;
    pc->buf_ctx_chunk = 0;
    pc->buf_ctx_right += num_new;
    if (is_last) {
        pc->buf_ctx_chunk = pc->buf_ctx_right;
        pc->buf_ctx_right = 0;
    } else {
        pc->buf_ctx_chunk = pc->buf_samples_chunk;
        pc->buf_ctx_right -= static_cast<int64_t>(pc->buf_samples_chunk);
    }
    const int64_t total_now = pc->buf_ctx_left + pc->buf_ctx_chunk + pc->buf_ctx_right;
    const int64_t expected  = static_cast<int64_t>(pc->buf_samples_left) + static_cast<int64_t>(pc->buf_samples_chunk) +
                              static_cast<int64_t>(pc->buf_samples_right);
    const int64_t extra     = std::max<int64_t>(total_now - expected, 0);
    pc->buf_ctx_left -= extra;
}

transcribe_status emit_buffered_chunk(ParakeetSession * pc,
                                      ParakeetModel *   pm,
                                      int64_t           num_new_samples,
                                      bool              is_last_chunk) {
    if (pc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    const auto & hp = pm->hparams;
    if (!pm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet buffered: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int samples_per_frame = hp.enc_subsampling_factor * hp.fe_hop_length;
    if (samples_per_frame <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // ----- Update buffer ContextSize (mirrors NeMo's add_frames_get_removed_) -----
    buf_ctx_add_frames(pc, num_new_samples, is_last_chunk);

    // ----- Build the [left | chunk | right] PCM window from absolute coords -----
    const int64_t end_abs     = pc->buf_next_audio_read + num_new_samples;
    const int64_t total_now   = pc->buf_ctx_left + pc->buf_ctx_chunk + pc->buf_ctx_right;
    const int     effective_T = static_cast<int>(total_now / samples_per_frame);
    const int64_t start_abs   = end_abs - total_now;

    std::vector<float> window_pcm(static_cast<size_t>(total_now), 0.0f);
    const int64_t      buf_size = static_cast<int64_t>(pc->stream_pcm_buffer.size());
    for (int64_t i = 0; i < total_now; ++i) {
        const int64_t src = start_abs + i;
        if (src >= 0 && src < buf_size) {
            window_pcm[static_cast<size_t>(i)] = pc->stream_pcm_buffer[static_cast<size_t>(src)];
        }
    }

    // Push a `stream.chunk.<N>.` dump prefix so per-chunk block outputs
    // don't overwrite across chunks; popped at function exit.
    const int chunk_step = pc->buf_chunk_step;
    char      chunk_prefix[64];
    std::snprintf(chunk_prefix, sizeof(chunk_prefix), "stream.chunk.%d.", chunk_step);
    transcribe::debug::push_name_prefix(chunk_prefix);

    struct PrefixPopGuard {
        ~PrefixPopGuard() { transcribe::debug::pop_name_prefix(); }
    } _prefix_pop_guard;

    if (transcribe::debug::enabled()) {
        const long long shape[1] = { static_cast<long long>(window_pcm.size()) };
        transcribe::debug::dump_host_f32("audio_in", window_pcm.data(), static_cast<long long>(window_pcm.size()),
                                         shape, 1, "buffered_streaming.audio_in");
    }

    // ----- Mel -----
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (const transcribe_status mst =
            pm->mel->compute(window_pcm.data(), window_pcm.size(), pc->mel_buf, mel_n_mels, mel_n_frames);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet buffered: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    pc->t_mel_us += ggml_time_us() - t_mel_start;

    // ----- Reset per-chunk compute state -----
    if (pc->compute_ctx != nullptr) {
        ggml_free(pc->compute_ctx);
        pc->compute_ctx = nullptr;
    }
    pc->encoder_out = nullptr;
    {
        ggml_init_params init_params{};
        init_params.mem_size   = 4 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        pc->compute_ctx        = ggml_init(init_params);
        if (pc->compute_ctx == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet buffered: ggml_init for compute_ctx failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (pc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    BufferedStreamMaskOverride buf_mask{};
    buf_mask.left_frames  = pc->buf_left_frames;
    buf_mask.chunk_frames = pc->buf_chunk_frames;
    buf_mask.right_frames = pc->buf_right_frames;
    buf_mask.valid_frames = effective_T;

    EncoderBuild eb = build_encoder_graph(pc->compute_ctx, pm->weights, pm->hparams, mel_n_frames, resolved_kv,
                                          pm->backend.c_str(), &buf_mask);
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- Scheduler alloc + mel upload -----
    if (pc->sched == nullptr) {
        pc->sched = ggml_backend_sched_new(pm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(pm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (pc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(pc->sched);
    if (!ggml_backend_sched_alloc_graph(pc->sched, eb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_tensor_set(eb.mel_in, pc->mel_buf.data(), 0, pc->mel_buf.size() * sizeof(float));

    // ----- Pos_emb fill (full 2T-1 layout; matches offline ChunkedLimited path) -----
    if (eb.pos_emb_in != nullptr) {
        const int d_model    = hp.enc_d_model;
        const int pos_len    = static_cast<int>(eb.pos_emb_in->ne[1]);
        const int zero_index = (pos_len - 1) / 2;

        pc->pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);
        pc->pos_div_term.resize(static_cast<size_t>(d_model / 2));
        const float ln_10000 = std::log(10000.0f);
        for (int k = 0; k < d_model / 2; ++k) {
            pc->pos_div_term[static_cast<size_t>(k)] =
                std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
        }
        for (int i = 0; i < pos_len; ++i) {
            const float pos = static_cast<float>(zero_index - i);
            float *     row = pc->pos_buf.data() + static_cast<size_t>(i) * d_model;
            for (int k = 0; k < d_model / 2; ++k) {
                const float div = pc->pos_div_term[static_cast<size_t>(k)];
                row[2 * k]      = std::sin(pos * div);
                row[2 * k + 1]  = std::cos(pos * div);
            }
        }
        ggml_backend_tensor_set(eb.pos_emb_in, pc->pos_buf.data(), 0, pc->pos_buf.size() * sizeof(float));
    }

    // Conv pad mask: zeros post-GLU activations on pre_encode overhang
    // frames so they don't leak backward through the depthwise conv's
    // right context (the attention mask already excludes them from MHA).
    if (eb.conv_pad_mask_in != nullptr) {
        const int          T_enc = static_cast<int>(eb.conv_pad_mask_in->ne[0]);
        const int          P     = std::max(0, std::min(effective_T, T_enc));
        std::vector<float> mask_buf(static_cast<size_t>(T_enc), 1.0f);
        for (int t = P; t < T_enc; ++t) {
            mask_buf[static_cast<size_t>(t)] = 0.0f;
        }
        ggml_backend_tensor_set(eb.conv_pad_mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(float));
        transcribe::debug::dump_tensor("enc.conv.pad_mask", eb.conv_pad_mask_in, "encoder.conv.pad_mask");
    }

    // Chunked_limited_with_rc mask. effective_T folds in NeMo's
    // conv-overhang pad_mask: the subsampling can emit T_enc >
    // session.total/samples_per_frame and those trailing frames would
    // otherwise contaminate every frame's attention scores (critical at
    // low-C/low-R).
    if (eb.chunked_mask_in != nullptr) {
        const int          T_enc = static_cast<int>(eb.chunked_mask_in->ne[0]);
        std::vector<float> mask_buf(static_cast<size_t>(T_enc) * static_cast<size_t>(T_enc));
        compute_chunked_limited_with_rc_mask(mask_buf.data(), T_enc, pc->buf_left_frames, pc->buf_chunk_frames,
                                             pc->buf_right_frames, effective_T);
        ggml_backend_tensor_set(eb.chunked_mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(float));
    }

    // ----- Threading -----
    transcribe::configure_sched_n_threads(pc->sched, pc->n_threads);

    // ----- Compute -----
    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(pc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet buffered: sched_graph_compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    pc->t_encode_us += ggml_time_us() - t_enc_start;

    pc->encoder_out = eb.out;

    if (transcribe::debug::enabled()) {
        transcribe::debug::dump_tensor("enc.mel.in", eb.mel_in, "encoder.mel");
    }

    // ----- Per-chunk intermediate dumps (debug only) -----
    // Same set as the offline run() path, scoped by the active per-chunk
    // name prefix.
    if (transcribe::debug::enabled()) {
        auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
            if (t != nullptr) {
                transcribe::debug::dump_tensor(name, t, stage);
            }
        };
        try_dump("enc.pre_encode.out", eb.dumps.pre_encode_out, "encoder.pre_encode");
        try_dump("enc.block.0.ff1", eb.dumps.block0_after_ff1, "encoder.block0.ff1");
        try_dump("enc.block.0.attn", eb.dumps.block0_after_attn, "encoder.block0.attn");
        try_dump("enc.block.0.conv", eb.dumps.block0_after_conv, "encoder.block0.conv");
        try_dump("enc.block.0.ff2", eb.dumps.block0_after_ff2, "encoder.block0.ff2");
        try_dump("enc.block.0.out", eb.dumps.block0_out, "encoder.block0.out");
        if (eb.dumps.mid_block_out != nullptr && eb.dumps.mid_block_idx >= 0) {
            char name[64];
            std::snprintf(name, sizeof(name), "enc.block.%d.out", eb.dumps.mid_block_idx);
            transcribe::debug::dump_tensor(name, eb.dumps.mid_block_out, "encoder.block.mid.out");
        }
        if (eb.dumps.last_block_out != nullptr && eb.dumps.last_block_idx >= 0) {
            char name[64];
            std::snprintf(name, sizeof(name), "enc.block.%d.out", eb.dumps.last_block_idx);
            transcribe::debug::dump_tensor(name, eb.dumps.last_block_out, "encoder.block.last.out");
        }
        if (transcribe::debug::dump_all_blocks_requested()) {
            for (size_t i = 0; i < eb.dumps.all_block_outs.size(); ++i) {
                ggml_tensor * t = eb.dumps.all_block_outs[i];
                if (t == nullptr) {
                    continue;
                }
                char name[64];
                std::snprintf(name, sizeof(name), "enc.block.%zu.out", i);
                transcribe::debug::dump_tensor(name, t, "encoder.block.bisect");
            }
        }
    }

    // ----- Readback encoder output, slice off left frames -----
    const int d_enc      = static_cast<int>(eb.out->ne[0]);
    const int T_enc_full = static_cast<int>(eb.out->ne[1]);
    if (T_enc_full <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet buffered: encoder produced %d frames", T_enc_full);
        return TRANSCRIBE_ERR_GGUF;
    }
    pc->enc_host.resize(static_cast<size_t>(d_enc) * static_cast<size_t>(T_enc_full));
    ggml_backend_tensor_get(eb.out, pc->enc_host.data(), 0, pc->enc_host.size() * sizeof(float));

    transcribe::debug::dump_tensor("enc.final", eb.out, "encoder.final");

    // Subsample session into encoder frames — mirrors NeMo's
    // buffer.context_size.subsample(factor=encoder_frame2audio_samples).
    const int ctx_left_frames  = static_cast<int>(pc->buf_ctx_left / samples_per_frame);
    const int ctx_chunk_frames = static_cast<int>(pc->buf_ctx_chunk / samples_per_frame);

    auto advance_cursor = [&]() {
        pc->buf_next_audio_read += num_new_samples;
        pc->buf_chunk_step += 1;
        pc->buf_initialized = true;
    };

    if (T_enc_full <= ctx_left_frames) {
        // Window too small for any chunk-region frames; skip emission.
        advance_cursor();
        return TRANSCRIBE_OK;
    }

    // Decode length matches NeMo's dispatch:
    //   non-final:   encoder_context_batch.chunk
    //   final:       encoder_output_len - encoder_context_batch.left
    const int T_chunk_avail = T_enc_full - ctx_left_frames;
    const int T_to_decode   = is_last_chunk ? T_chunk_avail : std::min(ctx_chunk_frames, T_chunk_avail);
    if (T_to_decode <= 0) {
        advance_cursor();
        return TRANSCRIBE_OK;
    }

    const float * enc_chunk = pc->enc_host.data() + static_cast<size_t>(ctx_left_frames) * static_cast<size_t>(d_enc);

    // Per-chunk encoder output dump: the FULL post-slice output (chunk +
    // right context), not just the T_to_decode frames, so the parity
    // harness sees both the decoded and lookahead portions.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = {
            static_cast<long long>(T_chunk_avail),
            static_cast<long long>(d_enc),
        };
        transcribe::debug::dump_host_f32("enc_out", enc_chunk,
                                         static_cast<long long>(T_chunk_avail) * static_cast<long long>(d_enc), shape,
                                         2, "buffered_streaming.enc_out");
    }

    // ----- Decode with carried RNN-T state -----
    const int64_t t_dec_start = ggml_time_us();
    if (const transcribe_status st = decode_rnnt_greedy_streaming(
            pm->host_decoder, enc_chunk, T_to_decode, d_enc, pc->stream_dec_state.lstm_state,
            pc->stream_dec_state.prev_token_id, static_cast<int>(pc->stream_dec_state.frame_offset), pc->n_threads,
            pc->raw_tokens);
        st != TRANSCRIBE_OK) {
        return st;
    }
    pc->t_decode_us += ggml_time_us() - t_dec_start;

    pc->stream_dec_state.frame_offset += T_to_decode;
    advance_cursor();
    return TRANSCRIBE_OK;
}

// Pure validation of the buffered-stream extension: resolves (L, C, R)
// to encoder frames. Does NOT touch pc->buf_*; out pointers written only
// on TRANSCRIBE_OK.
transcribe_status resolve_buffered_stream_geom(const ParakeetModel *            pm,
                                               const transcribe_stream_params * stream_params,
                                               int *                            out_L_frames,
                                               int *                            out_C_frames,
                                               int *                            out_R_frames) {
    const int frame_ms = (pm->hparams.enc_subsampling_factor * pm->hparams.fe_hop_length * 1000) /
                         std::max(pm->hparams.fe_sample_rate, 1);

    auto max_in = [](const std::vector<int32_t> & v) -> int32_t {
        int32_t best = 0;
        for (auto x : v) {
            if (x > best) {
                best = x;
            }
        }
        return best;
    };
    const int default_L = static_cast<int>(max_in(pm->hparams.enc_att_chunk_left_choices));
    const int default_C = static_cast<int>(max_in(pm->hparams.enc_att_chunk_chunk_choices));
    const int default_R = static_cast<int>(max_in(pm->hparams.enc_att_chunk_right_choices));

    int                    req_L_ms = -1, req_C_ms = -1, req_R_ms = -1;
    const transcribe_ext * family = stream_params != nullptr ? stream_params->family : nullptr;
    if (const transcribe_status st = transcribe_ext_check(family, TRANSCRIBE_EXT_KIND_PARAKEET_BUFFERED_STREAM,
                                                          sizeof(struct transcribe_parakeet_buffered_stream_ext));
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (family != nullptr) {
        const auto * px = reinterpret_cast<const transcribe_parakeet_buffered_stream_ext *>(family);
        req_L_ms        = px->left_ms;
        req_C_ms        = px->chunk_ms;
        req_R_ms        = px->right_ms;
    }
    const int safe_frame_ms      = std::max(frame_ms, 1);
    auto      ms_to_exact_frames = [&](int req_ms, int default_frames, int * out_frames) -> bool {
        if (req_ms == -1) {
            *out_frames = default_frames;
            return true;
        }
        if (req_ms < -1) {
            return false;
        }
        if (req_ms % safe_frame_ms != 0) {
            return false;
        }
        *out_frames = req_ms / safe_frame_ms;
        return true;
    };
    int L_frames = 0, C_frames = 0, R_frames = 0;
    if (!ms_to_exact_frames(req_L_ms, default_L, &L_frames) || !ms_to_exact_frames(req_C_ms, default_C, &C_frames) ||
        !ms_to_exact_frames(req_R_ms, default_R, &R_frames)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet buffered: requested (L, C, R)_ms = "
                "(%d, %d, %d) is invalid. Use -1 on any field to "
                "select the model default; otherwise the value "
                "must be 0 or a positive exact multiple of the "
                "%d ms encoder frame.",
                req_L_ms, req_C_ms, req_R_ms, safe_frame_ms);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto contains = [](const std::vector<int32_t> & v, int x) -> bool {
        for (auto y : v) {
            if (y == x) {
                return true;
            }
        }
        return false;
    };
    if (!contains(pm->hparams.enc_att_chunk_left_choices, L_frames) ||
        !contains(pm->hparams.enc_att_chunk_chunk_choices, C_frames) ||
        !contains(pm->hparams.enc_att_chunk_right_choices, R_frames)) {
        std::string allowed = "L=";
        for (auto v : pm->hparams.enc_att_chunk_left_choices) {
            allowed += std::to_string(v) + ",";
        }
        allowed += " C=";
        for (auto v : pm->hparams.enc_att_chunk_chunk_choices) {
            allowed += std::to_string(v) + ",";
        }
        allowed += " R=";
        for (auto v : pm->hparams.enc_att_chunk_right_choices) {
            allowed += std::to_string(v) + ",";
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet buffered: requested (L, C, R) = (%d, %d, %d) "
                "encoder frames not in model menu. Allowed %s",
                L_frames, C_frames, R_frames, allowed.c_str());
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (C_frames < 1) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    *out_L_frames = L_frames;
    *out_C_frames = C_frames;
    *out_R_frames = R_frames;
    return TRANSCRIBE_OK;
}

// Pure validation of the cache-aware-stream extension: resolves the
// active (att_context_left, att_context_right) from the training menu.
// Does NOT touch pc->*; out pointers written only on TRANSCRIBE_OK.
transcribe_status resolve_cache_aware_stream_geom(const ParakeetModel *            pm,
                                                  const transcribe_stream_params * stream_params,
                                                  int *                            out_chosen_left,
                                                  int *                            out_chosen_right) {
    int chosen_right = pm->hparams.enc_att_context_right;
    int chosen_left  = pm->hparams.enc_att_context_left;

    const transcribe_ext * family = stream_params != nullptr ? stream_params->family : nullptr;
    if (const transcribe_status st = transcribe_ext_check(family, TRANSCRIBE_EXT_KIND_PARAKEET_STREAM,
                                                          sizeof(struct transcribe_parakeet_stream_ext));
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (family != nullptr) {
        const auto * px        = reinterpret_cast<const transcribe_parakeet_stream_ext *>(family);
        const int    requested = px->att_context_right;
        if (requested < -1) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: att_context_right=%d is invalid; "
                    "use -1 for the model default or >=0 to pick "
                    "an entry from the model's training menu",
                    requested);
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        if (requested >= 0) {
            bool matched = false;
            for (const auto & p : pm->hparams.enc_att_context_size_choices) {
                if (p.second == requested) {
                    chosen_right = p.second;
                    chosen_left  = p.first;
                    matched      = true;
                    break;
                }
            }
            if (!matched) {
                std::string available;
                for (const auto & p : pm->hparams.enc_att_context_size_choices) {
                    available += std::to_string(p.second) + " ";
                }
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: requested att_context_right=%d "
                        "not in model's training menu; available: %s",
                        requested, available.c_str());
                return TRANSCRIBE_ERR_INVALID_ARG;
            }
        }
    }

    *out_chosen_left  = chosen_left;
    *out_chosen_right = chosen_right;
    return TRANSCRIBE_OK;
}

// Pre-flight: validate caller extension fields without mutating state.
// Called by the dispatcher before clear_result, so a rejection leaves the
// previous snapshot intact. stream_begin re-runs the same resolvers.
transcribe_status stream_validate(const transcribe_session * session,
                                  const transcribe_run_params * /*run_params*/,
                                  const transcribe_stream_params * stream_params) {
    const auto * pc = static_cast<const ParakeetSession *>(session);
    const auto * pm = static_cast<const ParakeetModel *>(pc->model);
    if (pm == nullptr || pm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const bool is_chunked_limited =
        (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);
    const bool is_chunked_with_rc =
        (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc);
    if (!is_chunked_limited && !is_chunked_with_rc) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    if (is_chunked_with_rc) {
        int L = 0, C = 0, R = 0;
        return resolve_buffered_stream_geom(pm, stream_params, &L, &C, &R);
    }

    int left = 0, right = 0;
    return resolve_cache_aware_stream_geom(pm, stream_params, &left, &right);
}

transcribe_status stream_begin(transcribe_session *             session,
                               const transcribe_run_params *    run_params,
                               const transcribe_stream_params * stream_params) {
    auto * pc = static_cast<ParakeetSession *>(session);
    auto * pm = static_cast<ParakeetModel *>(pc->model);
    if (pm == nullptr || pm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Streaming may run without going through run(), so init the dumper here.
    transcribe::debug::init();

    // Defense in depth (the dispatcher already gates on supports_streaming).
    const bool is_chunked_limited =
        (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);
    const bool is_chunked_with_rc =
        (pm->hparams.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc);
    if (!is_chunked_limited && !is_chunked_with_rc) {
        return TRANSCRIBE_ERR_NOT_IMPLEMENTED;
    }

    // -------- Buffered streaming path (parakeet-unified-en-0.6b) --------
    //
    // chunked_limited_with_rc with a 3-tuple training menu. Re-runs the
    // offline encoder on a sliding [left | chunk | right] PCM window per
    // chunk (no per-layer cache); the only new state is the runtime
    // (L, C, R) geometry and the LSTM carry.
    if (is_chunked_with_rc) {
        // Resolve (L, C, R) in encoder frames (defense in depth; also
        // produces the parsed values we configure state from).
        int L_frames = 0, C_frames = 0, R_frames = 0;
        if (const transcribe_status st =
                resolve_buffered_stream_geom(pm, stream_params, &L_frames, &C_frames, &R_frames);
            st != TRANSCRIBE_OK) {
            return st;
        }

        const int subsampling_factor = pm->hparams.enc_subsampling_factor;
        const int hop                = pm->hparams.fe_hop_length;
        const int samples_per_frame  = subsampling_factor * hop;

        pc->buf_left_frames     = L_frames;
        pc->buf_chunk_frames    = C_frames;
        pc->buf_right_frames    = R_frames;
        pc->buf_samples_left    = L_frames * samples_per_frame;
        pc->buf_samples_chunk   = C_frames * samples_per_frame;
        pc->buf_samples_right   = R_frames * samples_per_frame;
        pc->buf_next_audio_read = 0;
        pc->buf_ctx_left        = 0;
        pc->buf_ctx_chunk       = 0;
        pc->buf_ctx_right       = 0;
        pc->buf_initialized     = false;
        pc->buf_chunk_step      = 0;
        pc->buf_active          = true;

        pc->stream_pcm_buffer.clear();
        pc->raw_tokens.clear();
        pc->stream_run_params = *run_params;
        reset_streaming_decoder_state(pc, pm);
        // frame_offset starts at 0, advanced per-chunk by T_to_decode.
        pc->stream_dec_state.frame_offset = 0;

        return TRANSCRIBE_OK;
    }

    // -------- Cache-aware streaming path (nemotron-speech-streaming) --------
    pc->buf_active = false;

    // Resolve the active (att_context_left, att_context_right) (defense in
    // depth; also extracts the values we configure the caches from).
    int chosen_left = 0, chosen_right = 0;
    if (const transcribe_status st = resolve_cache_aware_stream_geom(pm, stream_params, &chosen_left, &chosen_right);
        st != TRANSCRIBE_OK) {
        return st;
    }

    pc->stream_pcm_buffer.clear();
    pc->raw_tokens.clear();  // parakeet-internal, accumulates per-chunk;
                             // clear_result doesn't touch it, so reset here
                             // or a back-to-back stream_begin carries tokens.
    pc->stream_run_params = *run_params;

    // Allocate streaming caches on first stream_begin (idempotent); zero
    // contents and reset cursors on every begin.
    if (const transcribe_status st = init_streaming_caches(pc, pm); st != TRANSCRIBE_OK) {
        return st;
    }
    zero_streaming_caches(pc);
    reset_streaming_decoder_state(pc, pm);

    // Resolve chunking geometry (NeMo's setup_streaming_params):
    //   chunk_size_subsequent = subsampling_factor * (1 + R)
    //   chunk_size_first      = sampling_frames_first + subsampling_factor * R
    //   mel_fed_first         = chunk_size_first
    //   mel_fed_subsequent    = chunk_size_subsequent + pre_encode_cache_size
    //   drop_extra_first      = 0
    //   drop_extra_subsequent = streaming_cfg.drop_extra_pre_encoded
    // Fallbacks below cover legacy GGUFs lacking the streaming.* KVs
    // (sampling_frames_first defaults to subsampling_factor, collapsing
    // chunk_size_first == chunk_size_subsequent — no first-chunk case).
    const int subsampling_factor    = pm->hparams.enc_subsampling_factor;
    int       pre_encode_cache_size = pm->hparams.enc_stream_pre_encode_cache_size;
    if (pre_encode_cache_size <= 0) {
        pre_encode_cache_size = subsampling_factor + 1;
    }
    int drop_extra_subsequent = pm->hparams.enc_stream_drop_extra_pre_encoded;
    if (drop_extra_subsequent <= 0 && pre_encode_cache_size >= 1) {
        drop_extra_subsequent = 1 + (pre_encode_cache_size - 1) / std::max(subsampling_factor, 1);
    }
    int sampling_frames_first = pm->hparams.enc_stream_sampling_frames_first;
    if (sampling_frames_first <= 0) {
        sampling_frames_first = subsampling_factor;
    }

    const int chunk_size_subsequent = subsampling_factor * (1 + chosen_right);
    const int chunk_size_first      = sampling_frames_first + subsampling_factor * chosen_right;

    pc->stream_caches.att_context_right     = chosen_right;
    pc->stream_caches.att_context_left      = chosen_left;
    pc->stream_caches.chunk_size_first      = chunk_size_first;
    pc->stream_caches.chunk_size_subsequent = chunk_size_subsequent;
    pc->stream_caches.mel_fed_first         = chunk_size_first;
    pc->stream_caches.mel_fed_subsequent    = chunk_size_subsequent + pre_encode_cache_size;
    pc->stream_caches.drop_extra_first      = 0;
    pc->stream_caches.drop_extra_subsequent = drop_extra_subsequent;
    pc->stream_caches.is_first_chunk        = true;
    pc->stream_caches.chunk_step            = 0;

    return TRANSCRIBE_OK;
}

transcribe_status stream_feed(transcribe_session *       session,
                              const float *              pcm,
                              int                        n_samples,
                              transcribe_stream_update * update) {
    auto * pc = static_cast<ParakeetSession *>(session);
    auto * pm = static_cast<ParakeetModel *>(pc->model);
    if (pc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    pc->stream_pcm_buffer.insert(pc->stream_pcm_buffer.end(), pcm, pcm + n_samples);
    pc->stream_audio_input_us += samples_to_us(n_samples);

    const int prev_n_tokens = static_cast<int>(pc->raw_tokens.size());

    if (pm == nullptr || pm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (!pm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // -------- Buffered streaming path --------
    //
    // Emit non-last chunks while there's enough buffered audio for the
    // next [chunk | right] window (the last chunk is stream_finalize's
    // job). Step 0 needs samples_chunk + samples_right; steady-state
    // needs samples_chunk.
    if (pc->buf_active) {
        while (true) {
            if (pc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            const int64_t num_new  = pc->buf_initialized ? static_cast<int64_t>(pc->buf_samples_chunk) :
                                                           static_cast<int64_t>(pc->buf_samples_chunk) +
                                                               static_cast<int64_t>(pc->buf_samples_right);
            const int64_t need_end = pc->buf_next_audio_read + num_new;
            if (need_end > static_cast<int64_t>(pc->stream_pcm_buffer.size())) {
                break;
            }
            if (const transcribe_status st = emit_buffered_chunk(pc, pm, num_new, /*is_last_chunk=*/false);
                st != TRANSCRIBE_OK) {
                return st;
            }
        }
        const bool tokens_changed = static_cast<int>(pc->raw_tokens.size()) != prev_n_tokens;
        if (tokens_changed) {
            rebuild_streaming_result_text(pc, pm);
            pc->n_committed_tokens   = static_cast<int>(pc->tokens.size());
            pc->n_committed_words    = 0;
            pc->n_committed_segments = 0;
            pc->stream_revision += 1;
            pc->stream_audio_committed_us =
                pc->buf_next_audio_read * 1000000LL / std::max<int64_t>(pm->hparams.fe_sample_rate, 1);
        }
        if (update != nullptr) {
            update->result_changed     = tokens_changed;
            update->revision           = pc->stream_revision;
            update->input_received_ms  = us_to_ms(pc->stream_audio_input_us);
            update->audio_committed_ms = us_to_ms(pc->stream_audio_committed_us);
            update->buffered_ms        = us_to_ms(pc->stream_audio_input_us - pc->stream_audio_committed_us);
        }
        return TRANSCRIBE_OK;
    }

    // -------- Cache-aware streaming path --------
    //
    // Recompute mel from the sliding PCM buffer (trimmed after each emit
    // to keep per-feed mel cost bounded). pcm_start_sample is the absolute
    // hop-aligned index of buffer[0], so absolute → buffer-relative
    // mel-frame is pcm_start_sample / hop. The first few buffer-relative
    // frames are reflect-pad garbage but correspond to already-emitted
    // absolute frames and are never read.
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (const transcribe_status mst = pm->mel->compute(pc->stream_pcm_buffer.data(), pc->stream_pcm_buffer.size(),
                                                       pc->mel_buf, mel_n_mels, mel_n_frames, pc->n_threads);
        mst != TRANSCRIBE_OK) {
        // For very short buffers compute returns INVALID_ARG — "not
        // enough audio yet", a no-op feed, not a fatal error.
        if (mst == TRANSCRIBE_ERR_INVALID_ARG) {
            if (update != nullptr) {
                update->result_changed     = false;
                update->revision           = pc->stream_revision;
                update->input_received_ms  = us_to_ms(pc->stream_audio_input_us);
                update->audio_committed_ms = us_to_ms(pc->stream_audio_committed_us);
                update->buffered_ms        = us_to_ms(pc->stream_audio_input_us - pc->stream_audio_committed_us);
            }
            return TRANSCRIBE_OK;
        }
        return mst;
    }
    pc->t_mel_us += ggml_time_us() - t_mel_start;

    // Emit as many chunks as we have new mel frames for. First chunk:
    // chunk_size_first new frames, no prepend, drop_extra=0. Subsequent:
    // chunk_size_subsequent new + pre_encode_cache_size prepend,
    // drop_extra=drop_extra_subsequent.
    const int     pre_encode_cache_size = pm->hparams.enc_stream_pre_encode_cache_size > 0 ?
                                              pm->hparams.enc_stream_pre_encode_cache_size :
                                              pm->hparams.enc_subsampling_factor + 1;
    // pcm_start_frame is the absolute mel-frame index of buffer-relative
    // frame 0. Hop-aligned trim keeps this an integer.
    const int     hop                   = pm->hparams.fe_hop_length;
    const int64_t pcm_start_frame       = pc->stream_caches.pcm_start_sample / hop;
    while (true) {
        if (pc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }

        const int64_t consumed = pc->stream_caches.mel_frames_consumed;
        const int64_t avail    = pcm_start_frame + static_cast<int64_t>(mel_n_frames);
        const bool    is_first = pc->stream_caches.is_first_chunk;

        const int chunk_advance =
            is_first ? pc->stream_caches.chunk_size_first : pc->stream_caches.chunk_size_subsequent;
        const int mel_fed    = is_first ? pc->stream_caches.mel_fed_first : pc->stream_caches.mel_fed_subsequent;
        const int drop_extra = is_first ? pc->stream_caches.drop_extra_first : pc->stream_caches.drop_extra_subsequent;
        const int prepend_frames = mel_fed - chunk_advance;

        // Hold back the last ceil(n_fft/2 / hop) mel frames: their STFT
        // windows are reflect-padded against the buffer end and stay
        // slightly wrong (never matching NeMo's full-audio mel) until more
        // audio arrives. stream_finalize skips this margin since the
        // right-edge pad is then real (end of stream).
        const int right_edge_margin =
            (pm->hparams.fe_n_fft / 2 + pm->hparams.fe_hop_length - 1) / pm->hparams.fe_hop_length;
        if (avail - consumed < chunk_advance + right_edge_margin) {
            break;
        }

        std::vector<float> chunk(static_cast<size_t>(mel_n_mels) * static_cast<size_t>(mel_fed));

        // mel_buf layout: row-major [n_mels, mel_n_frames]; entry
        // for absolute frame `abs_t` at offset
        // (m * mel_n_frames + (abs_t - pcm_start_frame)).
        auto mel_at = [&](int m, int64_t abs_t) -> float {
            const int64_t rel = abs_t - pcm_start_frame;
            return pc->mel_buf[static_cast<size_t>(m) * mel_n_frames + static_cast<size_t>(rel)];
        };

        for (int m = 0; m < mel_n_mels; ++m) {
            // Pre-encode-cache prepend: [consumed - prepend_frames,
            // consumed). prepend_frames == 0 on the first chunk.
            for (int h = 0; h < prepend_frames; ++h) {
                const int64_t frame_idx                     = consumed - prepend_frames + h;
                const float   val                           = (frame_idx < 0) ? 0.0f : mel_at(m, frame_idx);
                chunk[static_cast<size_t>(m) * mel_fed + h] = val;
            }
            // New frames: [consumed, consumed + chunk_advance).
            for (int n = 0; n < chunk_advance; ++n) {
                const float val                                              = mel_at(m, consumed + n);
                chunk[static_cast<size_t>(m) * mel_fed + prepend_frames + n] = val;
            }
        }

        // Flip is_first_chunk before the emit (the emit uses the params
        // we already extracted).
        pc->stream_caches.is_first_chunk = false;
        (void) pre_encode_cache_size;

        const int64_t t_enc_start = ggml_time_us();
        if (const transcribe_status st = emit_streaming_chunk(pc, pm, chunk.data(), mel_fed, drop_extra, chunk_advance);
            st != TRANSCRIBE_OK) {
            return st;
        }
        pc->t_encode_us += ggml_time_us() - t_enc_start;
    }

    // Rebuild the partial transcript from committed tokens.
    const bool tokens_changed = static_cast<int>(pc->raw_tokens.size()) != prev_n_tokens;
    if (tokens_changed) {
        rebuild_streaming_result_text(pc, pm);
        pc->n_committed_tokens   = static_cast<int>(pc->tokens.size());
        pc->n_committed_words    = 0;
        pc->n_committed_segments = 0;
        pc->stream_revision += 1;
    }

    // Trim already-consumed PCM, keeping the prepend_max history frames
    // AND each mel window's pad = n_fft/2 left margin — drop more and the
    // prepend frames reflect-pad against a truncated start, corrupting the
    // cache input (observed as chunk-boundary deletions in WER). Drop is
    // hop-aligned so pcm_start_sample stays at mel-frame 0. First feed
    // (mel_frames_consumed == 0): keep_from is negative, guard rejects.
    {
        const int     pad         = pm->hparams.fe_n_fft / 2;
        const int     prepend_max = pc->stream_caches.mel_fed_subsequent - pc->stream_caches.chunk_size_subsequent;
        const int64_t earliest_frame_needed = pc->stream_caches.mel_frames_consumed - static_cast<int64_t>(prepend_max);
        const int64_t earliest_sample_needed = earliest_frame_needed * static_cast<int64_t>(hop);
        const int64_t keep_from              = earliest_sample_needed - static_cast<int64_t>(pad);
        if (keep_from > pc->stream_caches.pcm_start_sample) {
            const int64_t drop_raw     = keep_from - pc->stream_caches.pcm_start_sample;
            const int64_t drop_aligned = (drop_raw / static_cast<int64_t>(hop)) * static_cast<int64_t>(hop);
            if (drop_aligned > 0 && drop_aligned <= static_cast<int64_t>(pc->stream_pcm_buffer.size())) {
                pc->stream_pcm_buffer.erase(pc->stream_pcm_buffer.begin(),
                                            pc->stream_pcm_buffer.begin() + static_cast<ptrdiff_t>(drop_aligned));
                pc->stream_caches.pcm_start_sample += drop_aligned;
            }
        }
    }

    // Audio "committed" cursor. mel_frames_consumed (vs encoder output
    // frames) avoids the right-context overshoot that would make
    // "encoder frames * 80 ms" exceed the real audio time per chunk.
    pc->stream_audio_committed_us = pc->stream_caches.mel_frames_consumed *
                                    static_cast<int64_t>(pm->hparams.fe_hop_length) * 1000000 /
                                    static_cast<int64_t>(pm->hparams.fe_sample_rate);

    if (update != nullptr) {
        update->result_changed     = tokens_changed;
        update->revision           = pc->stream_revision;
        update->input_received_ms  = us_to_ms(pc->stream_audio_input_us);
        update->audio_committed_ms = us_to_ms(pc->stream_audio_committed_us);
        update->buffered_ms        = us_to_ms(pc->stream_audio_input_us - pc->stream_audio_committed_us);
    }
    return TRANSCRIBE_OK;
}

transcribe_status stream_finalize(transcribe_session * session, transcribe_stream_update * update) {
    auto * pc = static_cast<ParakeetSession *>(session);
    auto * pm = static_cast<ParakeetModel *>(pc->model);
    if (pm == nullptr || pm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (!pm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int prev_n_tokens = static_cast<int>(pc->raw_tokens.size());

    // -------- Buffered streaming finalize --------
    //
    // One final emit consuming all remaining audio with is_last_chunk=true;
    // add_frames_get_removed_ folds the right slot + this num_new into the
    // chunk slot so the decoder gets every frame past ctx_left (no zero-pad).
    if (pc->buf_active) {
        const int64_t total = static_cast<int64_t>(pc->stream_pcm_buffer.size());
        if (pc->buf_next_audio_read < total) {
            if (pc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            const int64_t num_new = total - pc->buf_next_audio_read;
            if (const transcribe_status st = emit_buffered_chunk(pc, pm, num_new, /*is_last_chunk=*/true);
                st != TRANSCRIBE_OK) {
                return st;
            }
        }
        const bool tokens_changed = static_cast<int>(pc->raw_tokens.size()) != prev_n_tokens;
        if (tokens_changed || !pc->has_result) {
            rebuild_streaming_result_text(pc, pm);
        }
        pc->stream_audio_committed_us = pc->stream_audio_input_us;
        pc->n_committed_tokens        = static_cast<int>(pc->tokens.size());
        pc->n_committed_words         = static_cast<int>(pc->words.size());
        pc->n_committed_segments      = static_cast<int>(pc->segments.size());
        pc->stream_revision += 1;
        if (update != nullptr) {
            update->result_changed     = pc->has_result;
            update->revision           = pc->stream_revision;
            update->input_received_ms  = us_to_ms(pc->stream_audio_input_us);
            update->audio_committed_ms = us_to_ms(pc->stream_audio_committed_us);
            update->buffered_ms        = 0;
        }
        return TRANSCRIBE_OK;
    }

    // -------- Cache-aware streaming finalize --------
    //
    // Recompute mel one last time; if the stream ended mid-chunk, run a
    // final partial chunk so the tail audio isn't dropped (the encoder
    // graph handles arbitrary mel chunk sizes).
    if (!pc->stream_pcm_buffer.empty()) {
        int                     mel_n_mels   = 0;
        int                     mel_n_frames = 0;
        const transcribe_status mst = pm->mel->compute(pc->stream_pcm_buffer.data(), pc->stream_pcm_buffer.size(),
                                                       pc->mel_buf, mel_n_mels, mel_n_frames, pc->n_threads);
        if (mst != TRANSCRIBE_OK && mst != TRANSCRIBE_ERR_INVALID_ARG) {
            return mst;
        }

        if (mst == TRANSCRIBE_OK) {
            const int     hop             = pm->hparams.fe_hop_length;
            const int64_t pcm_start_frame = pc->stream_caches.pcm_start_sample / hop;
            const int64_t consumed        = pc->stream_caches.mel_frames_consumed;
            const int64_t avail           = pcm_start_frame + static_cast<int64_t>(mel_n_frames);
            const int64_t remaining       = avail - consumed;
            if (remaining > 0) {
                const bool is_first = pc->stream_caches.is_first_chunk;
                const int  drop_extra =
                    is_first ? pc->stream_caches.drop_extra_first : pc->stream_caches.drop_extra_subsequent;
                const int mel_fed_full =
                    is_first ? pc->stream_caches.mel_fed_first : pc->stream_caches.mel_fed_subsequent;
                const int chunk_advance_full =
                    is_first ? pc->stream_caches.chunk_size_first : pc->stream_caches.chunk_size_subsequent;
                const int          prepend_frames = mel_fed_full - chunk_advance_full;
                // Feed the natural partial-chunk size, no silence pad
                // (NeMo's last-chunk behavior), so trailing-audio tokens
                // survive instead of being masked by zero-pad frames.
                const int          new_take       = static_cast<int>(remaining);
                const int          mel_fed        = prepend_frames + new_take;
                std::vector<float> chunk(static_cast<size_t>(mel_n_mels) * static_cast<size_t>(mel_fed), 0.0f);
                // mel_buf indexed by ABSOLUTE frame; translate to the
                // buffer-relative offset via pcm_start_frame.
                auto               mel_at = [&](int m, int64_t abs_t) -> float {
                    const int64_t rel = abs_t - pcm_start_frame;
                    return pc->mel_buf[static_cast<size_t>(m) * mel_n_frames + static_cast<size_t>(rel)];
                };
                for (int m = 0; m < mel_n_mels; ++m) {
                    for (int h = 0; h < prepend_frames; ++h) {
                        const int64_t frame_idx                     = consumed - prepend_frames + h;
                        const float   val                           = (frame_idx < 0) ? 0.0f : mel_at(m, frame_idx);
                        chunk[static_cast<size_t>(m) * mel_fed + h] = val;
                    }
                    for (int n = 0; n < new_take; ++n) {
                        const float val                                              = mel_at(m, consumed + n);
                        chunk[static_cast<size_t>(m) * mel_fed + prepend_frames + n] = val;
                    }
                }
                pc->stream_caches.is_first_chunk = false;
                if (const transcribe_status st =
                        emit_streaming_chunk(pc, pm, chunk.data(), mel_fed, drop_extra, new_take);
                    st != TRANSCRIBE_OK) {
                    return st;
                }
            }
        }
    }

    const bool tokens_changed = static_cast<int>(pc->raw_tokens.size()) != prev_n_tokens;
    if (tokens_changed || !pc->has_result) {
        rebuild_streaming_result_text(pc, pm);
    }

    // Commit everything: at finalize all emitted tokens are final.
    pc->stream_audio_committed_us = pc->stream_audio_input_us;
    pc->n_committed_tokens        = static_cast<int>(pc->tokens.size());
    pc->n_committed_words         = static_cast<int>(pc->words.size());
    pc->n_committed_segments      = static_cast<int>(pc->segments.size());
    pc->stream_revision += 1;

    if (update != nullptr) {
        update->result_changed     = pc->has_result;
        update->revision           = pc->stream_revision;
        update->input_received_ms  = us_to_ms(pc->stream_audio_input_us);
        update->audio_committed_ms = us_to_ms(pc->stream_audio_committed_us);
        update->buffered_ms        = 0;
    }
    return TRANSCRIBE_OK;
}

void stream_reset(transcribe_session * session) {
    auto * pc = static_cast<ParakeetSession *>(session);
    pc->stream_pcm_buffer.clear();  // keep the allocation
}

// Kind+slot probe. No run-slot extensions (always false on _RUN). On
// _STREAM: ChunkedLimited takes PARAKEET_STREAM, ChunkedLimitedWithRc
// takes PARAKEET_BUFFERED_STREAM, offline variants neither.
bool accepts_ext_kind(const transcribe_model * model, transcribe_ext_slot slot, uint32_t kind) {
    if (model == nullptr) {
        return false;
    }
    if (slot != TRANSCRIBE_EXT_SLOT_STREAM) {
        return false;
    }
    const auto * pm = static_cast<const ParakeetModel *>(model);
    switch (pm->hparams.enc_att_context_style) {
        case ParakeetHParams::AttContextStyle::ChunkedLimited:
            return kind == TRANSCRIBE_EXT_KIND_PARAKEET_STREAM;
        case ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc:
            return kind == TRANSCRIBE_EXT_KIND_PARAKEET_BUFFERED_STREAM;
        case ParakeetHParams::AttContextStyle::Regular:
            return false;
    }
    return false;
}

}  // namespace

// `extern const` forces external linkage (a namespace-scope const object
// is internal-linkage in C++ otherwise).
extern const Arch arch = {
    /* .name             = */ "parakeet",
    /* .load             = */ load,
    /* .init_context     = */ init_context,
    /* .run              = */ run,
    /* .run_batch        = */ run_batch,
    /* .stream_validate  = */ stream_validate,
    /* .stream_begin     = */ stream_begin,
    /* .stream_feed      = */ stream_feed,
    /* .stream_finalize  = */ stream_finalize,
    /* .stream_reset     = */ stream_reset,
    /* .accepts_ext_kind = */ accepts_ext_kind,
};

}  // namespace transcribe::parakeet

// ---------------------------------------------------------------------------
// Public parakeet extension init functions (global scope, C linkage).
// Defined here so transcribe.cpp stays family-agnostic; each stamps the
// transcribe_ext header (size + kind) and the field defaults.
// ---------------------------------------------------------------------------

extern "C" void transcribe_parakeet_stream_ext_init(struct transcribe_parakeet_stream_ext * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->ext.size          = sizeof(*p);
    p->ext.kind          = TRANSCRIBE_EXT_KIND_PARAKEET_STREAM;
    p->att_context_right = -1;  // model default (max accuracy / max latency)
}

extern "C" void transcribe_parakeet_buffered_stream_ext_init(struct transcribe_parakeet_buffered_stream_ext * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->ext.size = sizeof(*p);
    p->ext.kind = TRANSCRIBE_EXT_KIND_PARAKEET_BUFFERED_STREAM;
    p->left_ms  = -1;  // model default
    p->chunk_ms = -1;
    p->right_ms = -1;
}
