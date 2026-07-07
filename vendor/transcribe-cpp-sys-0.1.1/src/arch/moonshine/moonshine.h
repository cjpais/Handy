// arch/moonshine/moonshine.h - Moonshine ASR model and context types.
//
// This header is INTERNAL to src/arch/moonshine/. It defines the concrete
// classes that derive from transcribe_model / transcribe_session for the
// Moonshine ASR family (encoder-decoder transformer over raw 16 kHz PCM).
//
// Closest in-tree analog: src/arch/whisper/whisper.h. The two families
// share the encoder-decoder + cross-attn KV cache shape; moonshine
// differs in the frontend (raw PCM, no mel) and uses partial RoPE on
// self-attention (whisper uses learned positional embeddings).

#pragma once

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-backend.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
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

namespace transcribe::moonshine {

void apply_family_invariants(transcribe_model & model);

// ---------------------------------------------------------------------------
// KV cache for the autoregressive decoder.
//
// Same dual-cache layout as src/arch/cohere/ and src/arch/whisper/:
//   self_k / self_v   - one [d_model, n_ctx] slab per layer; grows with
//                       each decode step.
//   cross_k / cross_v - one [d_model, T_enc] slab per layer; precomputed
//                       once per session from the encoder output.
// ---------------------------------------------------------------------------

struct MoonshineKvCache {
    ggml_tensor * self_k  = nullptr;
    ggml_tensor * self_v  = nullptr;
    ggml_tensor * cross_k = nullptr;
    ggml_tensor * cross_v = nullptr;

    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    int n_ctx   = 0;  // max self-attn sequence length
    int n       = 0;  // current self-attn fill
    int head    = 0;  // write head for next step
    int T_enc   = 0;  // encoder frame count in cross cache (T_enc_max if batched)
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

bool kv_cache_init(MoonshineKvCache & cache,
                   ggml_backend_t     backend,
                   int                n_ctx,
                   int                T_enc,
                   int                d_model,
                   int                n_layer,
                   ggml_type          kv_type);

// Batched variant: self [d_model·n_ctx·n_batch·n_layer], cross
// [d_model·T_enc·n_batch·n_layer]. n_batch == 1 is layout-identical.
bool kv_cache_init_batched(MoonshineKvCache & cache,
                           ggml_backend_t     backend,
                           int                n_ctx,
                           int                T_enc,
                           int                d_model,
                           int                n_layer,
                           int                n_batch,
                           ggml_type          kv_type);

// ---------------------------------------------------------------------------
// Model / context
// ---------------------------------------------------------------------------

struct MoonshineModel final : public transcribe_model {
    Tokenizer        tok;
    MoonshineHParams hparams;
    MoonshineWeights weights;
    ggml_context *   ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    MoonshineModel() = default;
    ~MoonshineModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct MoonshineSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    // Host-side mirror of the encoder output. Required because the
    // cross-KV graph runs in a fresh compute_ctx that does not share
    // tensor handles with the encoder graph.
    std::vector<float> enc_host;
    int                enc_T = 0;  // number of encoder frames

    MoonshineKvCache kv_cache;

    // Flash-attention defaults — finalized per-backend in init_context.
    // Encoder: ON everywhere (~2x on long audio). Decoder: ON except Metal
    // (Vulkan ~15% faster: jfk q8_0 tiny 112→98, base 165→143 ms). Metal
    // is the outlier: base's head_dim_padded=56 is not in Metal's FA
    // head-size set, so FA spills per-step to CPU and is ~2x slower than the
    // explicit kq->softmax->v·kq path (tiny's 40 is supported only via the
    // tile kernel, a wash). TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH apply on top.
    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    MoonshineSession() = default;
    ~MoonshineSession() override;
};

}  // namespace transcribe::moonshine
