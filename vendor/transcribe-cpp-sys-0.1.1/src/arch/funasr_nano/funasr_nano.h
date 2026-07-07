// arch/funasr_nano/funasr_nano.h - FunASR-Nano family internal model and
// context types.
//
// INTERNAL to src/arch/funasr_nano/.

#pragma once

#include "causal_lm/causal_lm.h"
#include "transcribe-backend.h"
#include "transcribe-kaldi-fbank.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::funasr_nano {

void apply_family_invariants(transcribe_model & model);

// ---------------------------------------------------------------------------
// Resolved chat-template special-token ids (filled at load time).
// ---------------------------------------------------------------------------

struct ChatTokens {
    int32_t im_start       = -1;
    int32_t im_end         = -1;
    int32_t newline        = -1;
    int32_t role_system    = -1;
    int32_t role_user      = -1;
    int32_t role_assistant = -1;
};

// ---------------------------------------------------------------------------
// Model / Context
// ---------------------------------------------------------------------------

struct FunAsrNanoModel final : public transcribe_model {
    Tokenizer         tok;
    FunAsrNanoHParams hparams;
    FunAsrNanoWeights weights;
    ggml_context *    ctx_meta = nullptr;

    transcribe::BackendPlan                    plan;
    ggml_backend_buffer_t                      backend_buffer = nullptr;
    transcribe::causal_lm::PackedGateUpHandles packed_gate_up;

    // Constructed once at load() time; const-after-construction. Kaldi
    // HTK fbank + LFR; CMVN dropped (fe_apply_cmvn=false).
    std::unique_ptr<transcribe::KaldiFbankFrontend> frontend;

    // Jinja chat template string (verbatim from GGUF KV).
    std::string chat_template;

    // Resolved chat-template token ids.
    ChatTokens chat_tokens;

    FunAsrNanoModel() = default;
    ~FunAsrNanoModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct FunAsrNanoSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    transcribe::causal_lm::KvCache kv_cache;

    // Batched KV cache for offline transcribe_run_batch (n_batch slabs).
    transcribe::causal_lm::KvCache kv_cache_batch;
    int                            kv_batch_cap   = 0;
    int                            kv_batch_n_ctx = 0;

    // Reusable host scratch.
    std::vector<float> frontend_buf;  // [T_lfr, d_input]
    std::vector<float> pe_buf;        // [T_lfr, d_input]  sinusoidal PE
    std::vector<float> enc_host;      // encoder output [d_model, T_lfr]
    std::vector<float> adaptor_host;  // adaptor output [llm_dim, T_lfr]

    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    FunAsrNanoSession() = default;
    ~FunAsrNanoSession() override;
};

}  // namespace transcribe::funasr_nano
