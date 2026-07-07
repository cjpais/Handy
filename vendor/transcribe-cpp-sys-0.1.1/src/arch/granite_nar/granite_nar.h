// arch/granite_nar/granite_nar.h - IBM Granite Speech NLE NAR variant.
//
// Family `granite_nar` covers granite-speech-4.1-2b-nar. The forward
// is non-autoregressive: a Conformer encoder + EncoderProjectorQFormer
// + bidirectional Granite-4 LM as a single-pass editor. There is no
// audio-token injection into the LM input_ids (the audio embeddings
// are concatenated to text embeddings BEFORE the LM forward) and no
// autoregressive decoding loop (the editing logits are produced in a
// single forward, then argmax+collapse yields the final text).

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
typedef struct ggml_backend *        ggml_backend_t;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::granite_nar {

void apply_family_invariants(transcribe_model & model);

struct GraniteNarModel final : public transcribe_model {
    Tokenizer         tok;
    GraniteNarHParams hparams;
    GraniteNarWeights weights;
    ggml_context *    ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Pre-fused BN (parakeet-style).
    ggml_context *        bn_fused_ctx    = nullptr;
    ggml_backend_buffer_t bn_fused_buffer = nullptr;

    std::optional<transcribe::MelFrontend> mel;

    GraniteNarModel() = default;
    ~GraniteNarModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct GraniteNarSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    // Encoder output buffered between encode and projector/LM.
    std::vector<float> mel_buf;
    std::vector<float> enc_cat_host;         // [T_enc, num_encoder_layers * enc_hidden]
    std::vector<float> ctc_logits_host;      // [T_enc, output_dim]
    std::vector<float> ctc_bpe_logits_host;  // [N_valid, bpe_output_dim] flat
    std::vector<float> proj_out_host;        // [n_audio_tokens, llm_dim]
    int32_t            t_enc          = 0;
    int32_t            n_audio_tokens = 0;

    bool encoder_use_flash = false;
    bool decoder_use_flash = false;  // bidirectional path doesn't use flash-attn KV cache

    GraniteNarSession() = default;
    ~GraniteNarSession() override;
};

}  // namespace transcribe::granite_nar
