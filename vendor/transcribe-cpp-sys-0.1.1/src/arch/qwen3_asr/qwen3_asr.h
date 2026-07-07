// arch/qwen3_asr/qwen3_asr.h - Qwen3-ASR model and context types.
//
// INTERNAL to src/arch/qwen3_asr/. Concrete transcribe_model /
// transcribe_session subclasses for the Qwen3-ASR family (audio-LLM: audio
// encoder + Qwen3 causal LM with audio-token injection).

#pragma once

#include "causal_lm/causal_lm.h"
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

namespace transcribe::qwen3_asr {

void apply_family_invariants(transcribe_model & model);

// Encode "language {Name}<asr_text>" for the given BCP-47 code: the token-id
// sequence the chat template seeds the assistant turn with on a forced hint.
// Declared here (not in model.cpp's anon namespace) so the BPE parity test can
// verify the full prefix against the HF reference.
//
// Returns:
//   TRANSCRIBE_OK                     on success, out_ids populated.
//   TRANSCRIBE_ERR_INVALID_ARG        bcp47 null or empty.
//   TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE  bcp47 not in the publisher's
//                                     support list (the static map
//                                     in model.cpp is the source of
//                                     truth; drift vs. caps.languages
//                                     surfaces here).
//   TRANSCRIBE_ERR_GGUF               tokenizer has no encoder
//                                     (missing merges) or vocab
//                                     missing <asr_text> special.
transcribe_status encode_language_prefix(const transcribe::Tokenizer & tok,
                                         const char *                  bcp47,
                                         std::vector<int32_t> &        out_ids);

// ---------------------------------------------------------------------------
// Model / Context
// ---------------------------------------------------------------------------

// Qwen3 chat-template special-token ids, resolved through the loaded tokenizer
// at load time so a future vocab reorder fails loudly. Reference ids on the
// 0.6B/1.7B vocab are stable but never hardcoded at runtime.
struct ChatTokens {
    int32_t im_start       = -1;
    int32_t im_end         = -1;
    int32_t newline        = -1;
    int32_t role_system    = -1;
    int32_t role_user      = -1;
    int32_t role_assistant = -1;
};

struct QwenAsrModel final : public transcribe_model {
    Tokenizer      tok;
    QwenAsrHParams hparams;
    QwenAsrWeights weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan                    plan;
    ggml_backend_buffer_t                      backend_buffer = nullptr;
    transcribe::causal_lm::PackedGateUpHandles packed_gate_up;

    std::optional<transcribe::MelFrontend> mel;

    // Jinja chat template string from the GGUF KV (empty == none).
    std::string chat_template;

    // Resolved chat-template token ids (see resolve_chat_tokens in model.cpp).
    ChatTokens chat_tokens;

    QwenAsrModel() = default;
    ~QwenAsrModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct QwenAsrSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    transcribe::causal_lm::KvCache kv_cache;

    // Batched KV cache for offline transcribe_run_batch (n_batch slabs).
    // Allocated/resized lazily by run_batch; freed in the destructor.
    transcribe::causal_lm::KvCache kv_cache_batch;
    int                            kv_batch_cap   = 0;  // allocated n_batch
    int                            kv_batch_n_ctx = 0;  // allocated n_ctx

    std::vector<float> mel_buf;
    std::vector<float> enc_host;  // audio encoder output, pre-injection

    bool encoder_use_flash = true;
    bool decoder_use_flash = true;

    QwenAsrSession() = default;
    ~QwenAsrSession() override;
};

}  // namespace transcribe::qwen3_asr
