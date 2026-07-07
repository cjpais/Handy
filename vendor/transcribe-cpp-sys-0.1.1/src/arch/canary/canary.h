// arch/canary/canary.h - Canary multitask AED model and context types.
//
// NVIDIA Canary: FastConformer encoder (parakeet shape but with biases on
// every linear) + autoregressive Transformer decoder (untied LM head;
// 180m-flash adds an enc->dec projection) with a 4/5-slot multitask prompt.

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

namespace transcribe::canary {

void apply_family_invariants(transcribe_model & model);

// ---------------------------------------------------------------------------
// KV cache for the autoregressive decoder. Same shape as cohere.
// ---------------------------------------------------------------------------

struct CanaryKvCache {
    ggml_tensor * self_k  = nullptr;
    ggml_tensor * self_v  = nullptr;
    ggml_tensor * cross_k = nullptr;
    ggml_tensor * cross_v = nullptr;

    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    int n_ctx = 0;
    int n     = 0;
    int head  = 0;
    int T_enc = 0;  // T_enc_max for a batched cache (shorter utts padded)

    // Utterance batch width. 1 for single-shot; > 1 for the offline batched
    // decoder. Self slab (layer, b) at (b + n_batch*layer)*n_ctx*hidden;
    // cross slab (layer, b) at (b + n_batch*layer)*T_enc*hidden.
    int n_batch = 1;

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

bool kv_cache_init(CanaryKvCache & cache,
                   ggml_backend_t  backend,
                   int             n_ctx,
                   int             T_enc,
                   int             n_state,
                   int             n_layer,
                   ggml_type       kv_type);

// Batched variant: self [n_state·n_ctx·n_batch·n_layer], cross
// [n_state·T_enc·n_batch·n_layer]. n_batch == 1 is layout-identical.
bool kv_cache_init_batched(CanaryKvCache & cache,
                           ggml_backend_t  backend,
                           int             n_ctx,
                           int             T_enc,
                           int             n_state,
                           int             n_layer,
                           int             n_batch,
                           ggml_type       kv_type);

struct CanaryModel final : public transcribe_model {
    Tokenizer      tok;
    CanaryHParams  hparams;
    CanaryWeights  weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Fused BN parameters (same as parakeet/cohere).
    ggml_context *        bn_fused_ctx    = nullptr;
    ggml_backend_buffer_t bn_fused_buffer = nullptr;

    // CPU-only F16 -> F32 promotion buffer for conformer 1x1 pointwise convs.
    ggml_context *        conv_pw_f32_ctx    = nullptr;
    ggml_backend_buffer_t conv_pw_f32_buffer = nullptr;

    std::optional<transcribe::MelFrontend> mel;

    CanaryModel() = default;
    ~CanaryModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct CanarySession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;
    ggml_tensor *        encoder_out = nullptr;

    CanaryKvCache kv_cache;

    std::vector<float> mel_buf;
    std::vector<float> pos_buf;
    std::vector<float> pos_div_term;
    std::vector<float> enc_host;

    // Per-stage flash-attention controls. ggml's Metal flash kernel lacks a
    // path for some encoder rel-pos MHSA dk values (96 for 180m-flash, 128
    // otherwise), so encoder flash defaults off on Metal; decoder defaults on.
    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    CanarySession() = default;
    ~CanarySession() override;
};

}  // namespace transcribe::canary
