// arch/sensevoice/sensevoice.h - SenseVoice family internal model and
// context types.
//
// INTERNAL to src/arch/sensevoice/. Defines the concrete classes that
// derive from transcribe_model / transcribe_session.

#pragma once

#include "transcribe-backend.h"
#include "transcribe-kaldi-fbank.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <memory>
#include <vector>

struct ggml_context;
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::sensevoice {

void apply_family_invariants(transcribe_model & model);

struct SenseVoiceModel final : public transcribe_model {
    transcribe::Tokenizer tok;
    SenseVoiceHParams     hparams;
    SenseVoiceWeights     weights;
    ggml_context *        ctx_meta = nullptr;

    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Constructed once at load() time; const-after-construction so
    // every context that derives from this model can share the cached
    // hamming window + HTK mel filterbank + CMVN buffers.
    std::unique_ptr<transcribe::KaldiFbankFrontend> frontend;

    SenseVoiceModel() = default;
    ~SenseVoiceModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct SenseVoiceSession final : public transcribe_session {
    // Per-call compute state. Reset at the top of every run() call.
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    // Reusable host scratch.
    std::vector<float>   frontend_buf;  // [T_lfr, d_input]
    std::vector<float>   pe_buf;        // [T, d_input]
    std::vector<float>   logits_buf;    // [T, vocab] for greedy CTC
    std::vector<int32_t> token_ids;     // post-collapse / post-blank-strip

    SenseVoiceSession() = default;
    ~SenseVoiceSession() override;
};

}  // namespace transcribe::sensevoice
