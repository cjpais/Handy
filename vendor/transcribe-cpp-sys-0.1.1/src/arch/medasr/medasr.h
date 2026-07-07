// arch/medasr/medasr.h - MedASR family internal model and context types.
//
// INTERNAL to src/arch/medasr/. Defines the concrete classes that derive
// from transcribe_model / transcribe_session.

#pragma once

#include "transcribe-backend.h"
#include "transcribe-mel.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <optional>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::medasr {

// Family defaults — applied before read_capability_kv runs. Defined in
// capabilities.cpp.
void apply_family_invariants(transcribe_model & model);

struct MedAsrModel final : public transcribe_model {
    Tokenizer      tok;
    MedAsrHParams  hparams;
    MedAsrWeights  weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    std::optional<transcribe::MelFrontend> mel;

    MedAsrModel() = default;
    ~MedAsrModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct MedAsrSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    ggml_tensor * encoder_out = nullptr;

    std::vector<float> mel_buf;
    std::vector<float> enc_host;    // [d_enc * T_enc] f32 readback
    std::vector<float> logits_buf;  // [vocab * T_enc] f32 readback

    MedAsrSession() = default;
    ~MedAsrSession() override;
};

}  // namespace transcribe::medasr
