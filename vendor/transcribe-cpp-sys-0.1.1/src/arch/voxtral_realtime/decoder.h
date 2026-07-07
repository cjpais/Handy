// arch/voxtral_realtime/decoder.h - Ministral LM prefill/step builders.
//
// The per-block math is the shared causal_lm module (GQA, NEOX RoPE, SwiGLU, KV
// cache) with null Q/K-norm and a per-layer FFN-branch scale (ffn_scale)
// carrying the delay-conditioned adaptive-norm `1 + ada(t_cond)`. This file owns
// graph allocation, ADDITIVE audio fusion, the TIED lm_head, and dump naming.
//
// Audio fusion: x[:, i] = embed_tokens(id[i]) + audio[:, i] for all i. The
// caller supplies exactly T audio embeddings ([dec_hidden, T]); each decode step
// adds the audio embed for its position.

#pragma once

#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "voxtral_realtime.h"
#include "weights.h"

struct ggml_context;
struct ggml_cgraph;
struct ggml_tensor;

namespace transcribe::voxtral_realtime {

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;
    ggml_tensor * audio_injected  = nullptr;
    ggml_tensor * block_0_out     = nullptr;
    ggml_tensor * block_mid_out   = nullptr;
    ggml_tensor * block_last_out  = nullptr;
    ggml_tensor * out_before_head = nullptr;
    ggml_tensor * logits_all      = nullptr;  // [vocab, T] when want_all_logits
};

struct PrefillBuild {
    ggml_tensor * input_ids_in = nullptr;  // [T] i32
    ggml_tensor * audio_in     = nullptr;  // [dec_hidden, T] f32 (overlay)
    ggml_tensor * positions_in = nullptr;  // [T] i32
    ggml_tensor * mask_in      = nullptr;  // [T, T] f16 causal
    ggml_tensor * out          = nullptr;  // logits ([vocab] last pos, or [vocab,T])
    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
    int           T     = 0;
};

// Teacher-forced / prompt prefill over T positions with additive audio overlay.
// want_all_logits=true exposes logits for every position (dump path: read
// gen0/gen8 columns); false slices the last position (autoregressive prefill).
PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const Weights &                  weights,
                                 const HParams &                  hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 ggml_tensor *                    ada_scale_all,
                                 int                              T,
                                 bool                             use_flash,
                                 bool                             want_all_logits);

struct StepBuild {
    ggml_tensor * input_id_in = nullptr;  // [1] i32
    ggml_tensor * audio_in    = nullptr;  // [dec_hidden, 1] f32 (overlay)
    ggml_tensor * position_in = nullptr;  // [1] i32
    ggml_tensor * kv_idx_in   = nullptr;  // [1] i64
    ggml_tensor * mask_in     = nullptr;  // [max_n_kv, 1] f16
    ggml_tensor * out         = nullptr;  // [1] i32 argmax
    ggml_tensor * logits      = nullptr;  // [vocab] f32
    ggml_cgraph * graph       = nullptr;
    int           max_n_kv    = 0;
};

StepBuild build_step_graph(ggml_context *                   ctx,
                           const Weights &                  weights,
                           const HParams &                  hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           ggml_tensor *                    ada_scale_all,
                           int                              max_n_kv,
                           bool                             use_flash);

// ---------------------------------------------------------------------------
// Multi-position verify (speculative decode)
// ---------------------------------------------------------------------------
//
// Runs the decoder on T_verify positions in one pass. Used by an n-gram
// speculative-decode driver: feed [next_tok, draft[0..K-1]] (T_verify = K+1),
// get back T_verify predicted tokens, accept the longest prefix where draft[i]
// matches the predicted token at column i. Rejected drafts' KV rows are simply
// overwritten on the next pass (positions are addressed by slot).
struct VerifyBuild {
    ggml_tensor * input_ids_in = nullptr;  // [T_verify] i32
    ggml_tensor * audio_in     = nullptr;  // [dec_hidden, T_verify] f32
    ggml_tensor * positions_in = nullptr;  // [T_verify] i32
    ggml_tensor * kv_idx_in    = nullptr;  // [T_verify] i64
    ggml_tensor * mask_in      = nullptr;  // [max_n_kv, T_verify] f16
    ggml_tensor * out          = nullptr;  // [T_verify] i32 argmax
    ggml_tensor * logits       = nullptr;  // [vocab, T_verify] f32
    ggml_cgraph * graph        = nullptr;
    int           T_verify     = 0;
    int           max_n_kv     = 0;
};

VerifyBuild build_verify_graph(ggml_context *                   ctx,
                               const Weights &                  weights,
                               const HParams &                  hp,
                               transcribe::causal_lm::KvCache & kv_cache,
                               ggml_tensor *                    ada_scale_all,
                               int                              T_verify,
                               int                              max_n_kv,
                               bool                             use_flash);

// ---------------------------------------------------------------------------
// Batched prefill / step (offline transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// Mirrors the single-utterance builders with the batch on ne[2], via the shared
// causal_lm::block_{prefill,step}_batched (flash-only, n_batch=B kv cache). The
// realtime specifics ride along: TIED lm_head, the per-layer ada ffn_scale view,
// and ADDITIVE audio fusion. The prompt is uniform length across the batch, so
// the prefill is rectangular [hidden, T_prompt, B] (last real position T_prompt-1
// for every row). Per-step audio injection is why the decode loop cannot reuse
// causal_lm::run_batched_step_loop.

struct PrefillBuildBatched {
    ggml_tensor * input_ids_in   = nullptr;  // [T_prompt, B] i32
    ggml_tensor * audio_dense_in = nullptr;  // [hidden, T_prompt*B] f32 (additive overlay)
    ggml_tensor * positions_in   = nullptr;  // [T_prompt] i32 (shared)
    ggml_tensor * mask_in        = nullptr;  // [T_prompt, T_prompt] f16 causal (shared)
    ggml_tensor * kv_idx_in      = nullptr;  // [T_prompt, B] i64 (idx[t,b] = t)
    ggml_tensor * last_idx_in    = nullptr;  // [1, B] i32 (= T_prompt-1 for every row)
    ggml_tensor * out            = nullptr;  // [B] i32 argmax (first generated token)
    ggml_tensor * logits         = nullptr;  // [vocab, B] f32
    ggml_cgraph * graph          = nullptr;
    int           T_prompt       = 0;
    int           n_batch        = 0;
};

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const Weights &                  weights,
                                                const HParams &                  hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                ggml_tensor *                    ada_scale_all,
                                                int                              T_prompt,
                                                int                              n_batch,
                                                bool                             use_flash);

struct StepBuildBatched {
    ggml_tensor * input_ids_in = nullptr;  // [B] i32
    ggml_tensor * audio_in     = nullptr;  // [hidden, B] f32 (per-step additive overlay)
    ggml_tensor * position_in  = nullptr;  // [B] i32
    ggml_tensor * kv_idx_in    = nullptr;  // [1, B] i64
    ggml_tensor * mask_in      = nullptr;  // [max_n_kv, 1, 1, B] f16
    ggml_tensor * out          = nullptr;  // [B] i32 argmax
    ggml_tensor * logits       = nullptr;  // [vocab, B] f32
    ggml_cgraph * graph        = nullptr;
    int           max_n_kv     = 0;
    int           n_batch      = 0;
};

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const Weights &                  weights,
                                          const HParams &                  hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          ggml_tensor *                    ada_scale_all,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash);

}  // namespace transcribe::voxtral_realtime
