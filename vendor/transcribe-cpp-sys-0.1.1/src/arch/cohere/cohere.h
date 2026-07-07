// arch/cohere/cohere.h - Cohere ASR model and context types.
//
// This header is INTERNAL to src/arch/cohere/. It defines the concrete
// classes that derive from transcribe_model / transcribe_session for
// the Cohere ASR family (encoder-decoder conformer + transformer).

#pragma once

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-backend.h"
#include "transcribe-mel.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <optional>
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

namespace transcribe::cohere {

void apply_family_invariants(transcribe_model & model);

// KV cache for the autoregressive decoder. Flat 1D K/V tensors (whisper.cpp
// pattern), per-layer slices via views. Self cache grows per step;
// cross cache is computed once from encoder output, then reused.
struct CohereKvCache {
    // Self-attention KV cache.
    // Flat tensors of size [n_state * n_layer * n_ctx].
    ggml_tensor * self_k = nullptr;
    ggml_tensor * self_v = nullptr;

    // Cross-attention KV cache.
    // Flat tensors of size [n_state * n_layer * T_enc].
    ggml_tensor * cross_k = nullptr;
    ggml_tensor * cross_v = nullptr;

    // ggml context that owns the cache tensor metadata.
    ggml_context * ctx = nullptr;

    // Backend buffer backing all cache tensors.
    ggml_backend_buffer_t buffer = nullptr;

    // Maximum sequence length for self-attention cache.
    int n_ctx = 0;

    // Current number of filled positions in self-attention cache.
    int n = 0;

    // The position at which the next token(s) will be written.
    int head = 0;

    // Number of encoder frames in cross-attention cache. For a batched
    // cache this is T_enc_max (the longest utterance; shorter ones are
    // right-padded and masked out in cross-attention).
    int T_enc = 0;

    // Utterance batch width. 1 for the single-shot path; > 1 for the
    // offline batched decoder. Self slab (layer, b) lives at flat offset
    // (b + n_batch*layer) * n_ctx * hidden; cross slab (layer, b) at
    // (b + n_batch*layer) * T_enc * hidden.
    int n_batch = 1;

    // Whether cross-attention cache has been populated.
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
        n_batch         = 1;
        cross_populated = false;
    }
};

// Initialize the KV cache. Must be called after backends are set up.
// n_ctx: max self-attention sequence length.
// T_enc: number of encoder frames (for cross-attention cache).
// n_state: decoder hidden dim.
// n_layer: number of decoder layers.
// kv_type: storage dtype for all four cache tensors (F16 or F32).
bool kv_cache_init(CohereKvCache & cache,
                   ggml_backend_t  backend,
                   int             n_ctx,
                   int             T_enc,
                   int             n_state,
                   int             n_layer,
                   ggml_type       kv_type);

// Batched variant: allocates self/cross caches with an extra utterance
// batch dimension (n_batch). Self tensors are [n_state·n_ctx·n_batch·n_layer],
// cross tensors [n_state·T_enc·n_batch·n_layer]. n_batch == 1 is layout-
// identical to kv_cache_init.
bool kv_cache_init_batched(CohereKvCache & cache,
                           ggml_backend_t  backend,
                           int             n_ctx,
                           int             T_enc,
                           int             n_state,
                           int             n_layer,
                           int             n_batch,
                           ggml_type       kv_type);

struct CohereModel final : public transcribe_model {
    Tokenizer      tok;
    CohereHParams  hparams;
    CohereWeights  weights;
    ggml_context * ctx_meta = nullptr;

    // Runtime backend plan — see transcribe-backend.h. scheduler_list
    // holds the backend handles plus a classified primary kind.
    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Fused BN parameters (same as Parakeet).
    ggml_context *        bn_fused_ctx    = nullptr;
    ggml_backend_buffer_t bn_fused_buffer = nullptr;

    // On CPU primary backend, the conformer 1×1 pointwise conv weights
    // are dequantized F16->F32 at load time (Zen 2 has no native F16
    // compute). Tensors live here; CohereBlock slots point at them.
    ggml_context *        conv_pw_f32_ctx    = nullptr;
    ggml_backend_buffer_t conv_pw_f32_buffer = nullptr;

    std::optional<transcribe::MelFrontend> mel;

    CohereModel() = default;
    ~CohereModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct CohereSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;
    ggml_tensor *        encoder_out = nullptr;

    // KV cache for the decoder.
    CohereKvCache kv_cache;

    std::vector<float> mel_buf;
    std::vector<float> pos_buf;
    std::vector<float> pos_div_term;
    std::vector<float> enc_host;

    // Flash-attn is per-stage: encoder dk=160 has no Metal flash_attn_ext
    // kernel (default OFF on Metal; manual path ties anyway); decoder
    // dk=128 works everywhere (default ON). TRANSCRIBE_NO_FLASH /
    // TRANSCRIBE_FORCE_FLASH apply to both stages at once.
    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    CohereSession() = default;
    ~CohereSession() override;
};

}  // namespace transcribe::cohere
