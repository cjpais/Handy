// arch/voxtral_realtime/voxtral_realtime.h - model and context types.
//
// INTERNAL to src/arch/voxtral_realtime/. Streaming audio-LLM: causal RoPE
// sliding-window audio encoder + 4x frame-group projector + Ministral causal
// LM with ADDITIVE audio fusion and per-layer delay-conditioned adaptive-norm.

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
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::voxtral_realtime {

void apply_family_invariants(transcribe_model & model);

// Streaming prompt control tokens. The prompt is
//   [BOS] [STREAMING_PAD] * (n_left_pad + num_delay_tokens)
// with the projector audio embeddings ADDED onto every position.
struct PromptSpecials {
    int32_t bos           = -1;
    int32_t streaming_pad = -1;  // additive-fusion placeholder (id 32)
    int32_t eos           = -1;
    int32_t n_left_pad    = 32;  // streaming_n_left_pad_tokens (tekken)
};

struct Model final : public transcribe_model {
    Tokenizer      tok;
    HParams        hparams;
    Weights        weights;
    ggml_context * ctx_meta = nullptr;

    transcribe::BackendPlan                    plan;
    ggml_backend_buffer_t                      backend_buffer = nullptr;
    transcribe::causal_lm::PackedGateUpHandles packed_gate_up;

    std::optional<transcribe::MelFrontend> mel;

    PromptSpecials specials;

    Model() = default;
    ~Model() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

struct Session final : public transcribe_session {
    ggml_context *       compute_ctx = nullptr;
    ggml_backend_sched_t sched       = nullptr;

    transcribe::causal_lm::KvCache kv_cache;

    // Offline batched decode (transcribe_run_batch): a batched KV cache with one
    // slab per utterance, reused across calls, re-allocated only when the batch
    // size or context window grows. Mirrors VoxtralSession.
    transcribe::causal_lm::KvCache kv_cache_batch;
    int                            kv_batch_cap   = 0;  // slabs allocated (== n_batch of last alloc)
    int                            kv_batch_n_ctx = 0;  // n_ctx of last alloc

    std::vector<float> mel_buf;
    std::vector<float> enc_host;  // projector output (audio embeds), [hidden, n_audio]

    // Per-layer adaptive-norm FFN scale (1 + ada(t_cond)), precomputed for the
    // active num_delay_tokens. One [dec_hidden] tensor per decoder layer, all
    // packed into ada_scale_all [dec_hidden, n_layers] in ada_buffer.
    ggml_context *             ada_ctx       = nullptr;
    ggml_backend_buffer_t      ada_buffer    = nullptr;
    ggml_tensor *              ada_scale_all = nullptr;  // [dec_hidden, n_layers]
    std::vector<ggml_tensor *> ada_scale;                // per-layer views
    int                        ada_num_delay = -1;

    bool encoder_use_flash = false;
    bool decoder_use_flash = true;

    // ---- Incremental streaming state (reference StaticCache mechanism) -------
    // The encoder transformer runs INCREMENTALLY: new embedder frames attend to
    // an encoder KV cache (StaticCache, sliding_window=750) under a windowed
    // mask, each frame processed exactly once. The decoder prefills the prompt
    // once then steps one token per audio frame against its persistent KV.
    std::vector<float> stream_pcm;                       // accumulated raw PCM (no pad)
    int                stream_num_delay           = -1;  // active num_delay_tokens
    int                stream_min_decode_ms       = -1;  // partial-decode throttle
    int64_t            stream_last_decode_samples = 0;

    transcribe::causal_lm::KvCache enc_kv;                      // encoder StaticCache (ring)
    int                            stream_enc_slot        = 0;  // ring write head (slot units)
    int                            stream_enc_abs_base    = 0;  // absolute frame index of slot 0
    int                            stream_n_enc_committed = 0;  // enc frames committed (absolute)
    // Conv-stem padding cache: feed only new mel frames, carrying the conv
    // left-context. conv0 (k3 s1) ← last 2 mel frames; conv1 (k3 s2) ← last 1
    // conv0-output frame. Zeros on the first chunk == whole-buffer left-pad.
    std::vector<float>             stream_conv0_cache;          // [n_mels, 2] (mel-major, 2 frames)
    std::vector<float>             stream_conv1_cache;          // [d_model, 1] (last conv0-out frame)
    int                            stream_n_mel_committed = 0;  // mel frames fed through the conv stem
    // Frontend memory bounding (match the reference's bounded streaming): drop PCM
    // and audio-embeds that no future feed/step can read. Offsets keep absolute
    // indexing correct across the physical drops (batched to stay amortized O(1)).
    int64_t                        stream_pcm_drop        = 0;  // samples erased from stream_pcm front
    int                            stream_audio_base      = 0;  // absolute index of stream_audio_embeds[0]
    std::vector<float>             stream_audio_embeds;         // [dec_hidden, n_tok_ready]
    int                            stream_n_tok_ready = 0;      // audio embeds computed
    bool                           stream_prompt_done = false;  // decoder prompt prefilled
    int                            stream_dec_pos     = 0;      // next decoder position
    int32_t                        stream_next_tok    = 0;      // last emitted token (step input)
    bool                           stream_eos         = false;
    std::vector<int32_t>           stream_generated;            // emitted token ids
    int                            stream_n_audio_clamp = -1;   // ceil(mel_frames/8) decode cap

    // Numerical-validation captures (only when TRANSCRIBE_DUMP_DIR is set):
    // stream.logits_raw (first generated step) and .gen8 (ninth step).
    std::vector<float> stream_gen0_logits;
    std::vector<float> stream_gen8_logits;

    // Per-component wall-time accumulators (us), printed at finalize when
    // TRANSCRIBE_VOXTRAL_REALTIME_STREAM_TIMING is set. Diagnostic only.
    int64_t stream_t_mel_us         = 0;  // CPU log-mel (windowed compute)
    int64_t stream_t_conv_us        = 0;  // conv stem (cached chunk graph)
    int64_t stream_t_enc_us         = 0;  // encoder ring transformer (build + alloc + compute)
    int64_t stream_t_enc_compute_us = 0;  // ... of which pure graph_compute (rest = per-feed overhead)
    int64_t stream_t_dec_us         = 0;  // decoder prefill + step loop

    Session() = default;
    ~Session() override;
};

}  // namespace transcribe::voxtral_realtime
