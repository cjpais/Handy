// arch/moonshine_streaming/moonshine_streaming.h - Moonshine-Streaming
// model and context types.
//
// This header is INTERNAL to src/arch/moonshine_streaming/. It mirrors
// src/arch/moonshine/moonshine.h since the encoder-decoder + cross-attn
// KV cache shape is shared across both. Differences:
//
//   - The adapter add+proj is applied exactly once per session into a host
//     buffer; cross-KV precompute reads that buffer, not the encoder output.

#pragma once

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-backend.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <deque>
#include <string>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_backend;
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend *        ggml_backend_t;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::moonshine_streaming {

void apply_family_invariants(transcribe_model & model);

// ---------------------------------------------------------------------------
// KV cache for the autoregressive decoder.  Same shape as moonshine /
// cohere / whisper — dual cache: self over [d_model, n_ctx], cross over
// [d_model, T_enc].
// ---------------------------------------------------------------------------

struct MoonshineStreamingKvCache {
    ggml_tensor * self_k  = nullptr;
    ggml_tensor * self_v  = nullptr;
    ggml_tensor * cross_k = nullptr;
    ggml_tensor * cross_v = nullptr;

    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    int n_ctx   = 0;
    int n       = 0;
    int head    = 0;
    int T_enc   = 0;
    int n_batch = 1;  // utterance batch width (>1 for the offline batched decoder)

    bool cross_populated = false;

    void free() {
        if (buffer != nullptr) {
            safe_buffer_free(buffer);
            buffer = nullptr;
        }
        if (ctx != nullptr) {
            ggml_free(ctx);
            ctx = nullptr;
        }
        self_k          = nullptr;
        self_v          = nullptr;
        cross_k         = nullptr;
        cross_v         = nullptr;
        n               = 0;
        head            = 0;
        T_enc           = 0;
        n_batch         = 1;
        cross_populated = false;
    }
};

bool kv_cache_init(MoonshineStreamingKvCache & cache,
                   ggml_backend_t              backend,
                   int                         n_ctx,
                   int                         T_enc,
                   int                         d_model,
                   int                         n_layer,
                   ggml_type                   kv_type);

// Batched variant: self [d_model·n_ctx·n_batch·n_layer], cross
// [d_model·T_enc·n_batch·n_layer]. n_batch == 1 is layout-identical.
bool kv_cache_init_batched(MoonshineStreamingKvCache & cache,
                           ggml_backend_t              backend,
                           int                         n_ctx,
                           int                         T_enc,
                           int                         d_model,
                           int                         n_layer,
                           int                         n_batch,
                           ggml_type                   kv_type);

// ---------------------------------------------------------------------------
// Model / context
// ---------------------------------------------------------------------------

struct MoonshineStreamingModel final : public transcribe_model {
    Tokenizer                 tok;
    MoonshineStreamingHParams hparams;
    MoonshineStreamingWeights weights;
    ggml_context *            ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    MoonshineStreamingModel() = default;
    ~MoonshineStreamingModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct MoonshineStreamingSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    // Host-side mirror of the post-adapter encoder hidden. The adapter
    // pos_emb add (and proj when present) is applied once per session;
    // this host buffer feeds the cross_kv precompute graph.
    std::vector<float> adapter_host;
    int                enc_T = 0;  // T_enc

    MoonshineStreamingKvCache kv_cache;

    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    // ---- incremental streaming state ----
    //
    // Each feed extends host-side committed buffers in lockstep:
    //   stream_adapter_committed  - post-adapter encoder hidden
    //                               [dec_d_model, T_emitted].
    //   stream_cross_k/v_committed - per decoder layer, [dec_d_model,
    //                               T_emitted]; uploaded into the persistent
    //                               kv_cache on each partial decode (per-feed,
    //                               when the throttle allows) and again at
    //                               the finalize decode.
    //
    // Per-feed slicing is numerically equivalent to a one-shot pass because
    // the encoder is ergodic (no positional encoding) with per-layer
    // sliding-window attention + causal-conv frontend (output frame t depends
    // only on conv-stack frames [t - L_total, t + R_total]), the adapter
    // pos_emb is an absolute-frame get_rows, and the cross-KV projection is
    // per-frame linear.
    //
    // PCM trimming: stream_pcm_buffer drops samples older than
    // (T_emitted - L_total - frontend_pad) encoder frames (the left context
    // any future window still needs). stream_pcm_start_sample tracks the
    // absolute index of stream_pcm_buffer[0].
    //
    // These fields are NOT touched by clear_result — the family owns its
    // per-utterance audio + encoder scratch.
    std::vector<float>               stream_pcm_buffer;
    int64_t                          stream_pcm_start_sample = 0;
    std::vector<float>               stream_adapter_committed;
    std::vector<std::vector<float>>  stream_cross_k_committed;
    std::vector<std::vector<float>>  stream_cross_v_committed;
    int32_t                          stream_T_emitted      = 0;
    // T_emitted captured at the last successful partial decode; lets
    // stream_feed / stream_finalize skip a redundant decode when no
    // new encoder frames have been committed since the previous one.
    int32_t                          stream_last_decoded_T = 0;
    // Recent partial-decode token id sequences, used to compute the
    // longest common prefix that's safe to mark committed across feeds.
    // Reset at stream_begin; updated after every successful partial decode.
    std::deque<std::vector<int32_t>> stream_token_id_history;
    // Geometry, resolved at stream_begin (constant per stream).
    int32_t                          stream_L_total_frames        = 0;
    int32_t                          stream_R_total_frames        = 0;
    int32_t                          stream_frontend_pad_frames   = 0;
    int32_t                          stream_samples_per_enc_frame = 0;
    // Minimum gap (encoder frames) between per-feed AR decode runs, from the
    // family extension. 0 = decode every advance; finalize always decodes once.
    int32_t                          stream_min_decode_frames     = 0;
    transcribe_run_params            stream_run_params{};

    MoonshineStreamingSession() = default;
    ~MoonshineStreamingSession() override;
};

}  // namespace transcribe::moonshine_streaming
