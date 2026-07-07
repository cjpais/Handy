// arch/gigaam/gigaam.h - GigaAM-family internal model and context types.
//
// INTERNAL to src/arch/gigaam/. GigaAM-v3 ships four ported variants
// (e2e_rnnt, e2e_ctc, rnnt, ctc), all sharing a 16-layer Conformer encoder
// with rotary attention. The e2e variants use a SentencePiece tokenizer
// (punctuation + Cyrillic casing); the non-e2e variants use a charwise
// 33-entry vocab. The upstream `v3_ssl` SSL checkpoint is out of scope
// (encoder-only, no head).

#pragma once

#include "decoder.h"
#include "mel.h"
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

namespace transcribe::gigaam {

// Family defaults — applied before read_capability_kv runs. Defined in
// capabilities.cpp. KV-driven overrides take precedence per the
// project-wide convention.
void apply_family_invariants(transcribe_model & model);

// Concrete model. Owns the ggml_context that holds every weight tensor's
// data buffer plus the host decoder mirror for the RNN-T/CTC greedy loop.
struct GigaamModel final : public transcribe_model {
    Tokenizer      tok;
    GigaamHParams  hparams;
    GigaamWeights  weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Family-specific mel frontend (HTK mel, power=2, log-clamp,
    // center=False, periodic Hann). See arch/gigaam/mel.{h,cpp}.
    GigaamMelFrontend mel;

    // Host mirror of predictor + joint (RNN-T variants) or CTC head
    // (CTC variants). Populated at load() time.
    HostDecoderWeights host_decoder;

    GigaamModel() = default;
    ~GigaamModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

// Concrete context. One scheduler + compute_ctx lifecycle per context,
// mirroring parakeet.
struct GigaamSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    ggml_tensor * encoder_out = nullptr;

    std::vector<float> mel_buf;
    std::vector<float> pos_buf;  // cos/sin rotary PE bank, host scratch
    std::vector<float> enc_host;

    GigaamSession() = default;
    ~GigaamSession() override;
};

}  // namespace transcribe::gigaam
