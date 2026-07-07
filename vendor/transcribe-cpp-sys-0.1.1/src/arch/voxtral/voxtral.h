// arch/voxtral/voxtral.h - Voxtral (2507) model and context types.
//
// INTERNAL to src/arch/voxtral/. Concrete transcribe_model /
// transcribe_session classes for the Voxtral audio-LLM family (Whisper-large-v3
// encoder + 4x frame-group projector + Llama/Ministral causal LM with
// audio-token injection). Two prompt modes — ASR transcription and a
// mistral-common instruct path (translation / free-text) — share the same
// encoder/projector/decoder.

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

namespace transcribe::voxtral {

void apply_family_invariants(transcribe_model & model);

// mistral-common transcription/instruct control tokens, resolved through the
// loaded tokenizer at load time (a vocab reorder fails loudly here).
//
// Transcription template:
//   [BOS] [INST] [BEGIN_AUDIO] [AUDIO]*N [/INST] (lang:<l>)? [TRANSCRIBE]
// Instruct template (translate / prompt):
//   [BOS] [INST] [BEGIN_AUDIO] [AUDIO]*N BPE(instruction) [/INST]
struct PromptSpecials {
    int32_t bos         = -1;
    int32_t inst        = -1;  // [INST]
    int32_t begin_audio = -1;  // [BEGIN_AUDIO]
    int32_t end_inst    = -1;  // [/INST]
    int32_t transcribe  = -1;  // [TRANSCRIBE]
    int32_t eos         = -1;
};

struct VoxtralModel final : public transcribe_model {
    Tokenizer      tok;
    VoxtralHParams hparams;
    VoxtralWeights weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan                    plan;
    ggml_backend_buffer_t                      backend_buffer = nullptr;
    transcribe::causal_lm::PackedGateUpHandles packed_gate_up;

    std::optional<transcribe::MelFrontend> mel;

    PromptSpecials specials;

    VoxtralModel() = default;
    ~VoxtralModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct VoxtralSession final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    transcribe::causal_lm::KvCache kv_cache;

    // Offline batched decode (transcribe_run_batch): a batched KV cache with
    // one slab per utterance, reused across calls and re-allocated only when
    // the batch size or context window grows.
    transcribe::causal_lm::KvCache kv_cache_batch;
    int                            kv_batch_cap   = 0;  // slabs allocated (== n_batch of last alloc)
    int                            kv_batch_n_ctx = 0;  // n_ctx of last alloc

    std::vector<float> mel_buf;
    std::vector<float> enc_host;  // projector output (audio embeds), all chunks

    bool encoder_use_flash = false;
    bool decoder_use_flash = true;

    VoxtralSession() = default;
    ~VoxtralSession() override;
};

}  // namespace transcribe::voxtral
