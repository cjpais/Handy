// arch/moonshine/decoder.h - Moonshine decoder graph builders.
//
// Two graph families:
//
//   1. build_cross_kv_graph() - precomputes cross-attn K/V for every
//      decoder layer from the encoder output and writes them into the
//      cross cache. Run once per session (T_enc may vary across calls
//      because moonshine consumes variable-length raw audio).
//
//   2. build_decoder_graph_kv() - the autoregressive prompt/step path.
//      Handles both the prompt pass (n_tokens=1, n_past=0 — moonshine's
//      prompt is a single bos token) and steady-state step passes
//      (n_tokens=1, n_past=current). All dec.* dump points are wired
//      on the prompt pass.
//
// All attention projections (q/k/v/o) are bias-less (attention_bias=False).
// Self-attn applies partial RoPE 0.9 to q/k; cross-attn does NOT rotate.
// MLP is SwiGLU: fc1 hidden→2·intermediate, split [x_proj, gate],
// silu(gate)⊙x_proj, fc2 intermediate→hidden.

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::moonshine {

struct MoonshineHParams;
struct MoonshineWeights;
struct MoonshineKvCache;

struct DecoderDumps {
    ggml_tensor *              token_emb = nullptr;        // dec.token_emb
    ggml_tensor *              embed_sum = nullptr;        // dec.embed_sum (= token_emb)
    std::vector<ggml_tensor *> block_outs;                 // dec.block.{i}.out
    ggml_tensor *              out_before_head = nullptr;  // dec.out_before_head
    ggml_tensor *              logits_raw      = nullptr;  // dec.logits_raw
    ggml_tensor *              logits          = nullptr;  // dec.logits (log_softmax)
};

struct DecoderBuild {
    ggml_tensor * token_ids_in   = nullptr;  // [n_tokens] i32
    ggml_tensor * pos_ids_in     = nullptr;  // [n_tokens] i32  (absolute positions)
    ggml_tensor * encoder_out_in = nullptr;  // [d_model, T_enc] f32 (cross_kv graph only)
    ggml_tensor * causal_mask_in = nullptr;  // [n_kv, n_tokens] f32 (n_tokens>1 only)

    ggml_tensor * out        = nullptr;      // logits or log-softmax depending on flag
    ggml_tensor * argmax_out = nullptr;      // [n_tokens] i32, set when skip_log_softmax

    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a graph that computes cross-attention K/V for every decoder
// layer from the encoder output and writes them into the cross-attn
// KV cache. Reads encoder_out [d_model, T_enc] (must already be allocated
// in the same backend the cache lives on). Writes into kv_cache.cross_k /
// .cross_v.
DecoderBuild build_cross_kv_graph(ggml_context *           compute_ctx,
                                  const MoonshineWeights & weights,
                                  const MoonshineHParams & hp,
                                  MoonshineKvCache &       kv_cache,
                                  int                      T_enc);

// Build a KV-cached decoder graph covering both the prompt pass
// (n_past=0) and per-step generation (n_past>=1). For moonshine the
// prompt pass always has n_tokens=1 (just the bos token), but the
// builder accepts n_tokens > 1 for completeness.
//
// `skip_log_softmax=true` outputs raw pre-softmax logits (argmax-
// invariant; cheaper readback). `skip_log_softmax=false` matches the
// reference dump's `dec.logits` tensor (log_softmax of logits_raw).
DecoderBuild build_decoder_graph_kv(ggml_context *           compute_ctx,
                                    const MoonshineWeights & weights,
                                    const MoonshineHParams & hp,
                                    MoonshineKvCache &       kv_cache,
                                    int                      n_tokens,
                                    int                      n_past,
                                    int                      T_enc,
                                    bool                     skip_log_softmax = false,
                                    bool                     use_flash        = true);

// Static-topology single-token decoder graph. Built once per utterance
// after the prompt pass and reused for every step in the autoregressive
// loop. Independent of n_past:
//   - KV cache writes go through ggml_set_rows with kv_idx_in as the
//     runtime row index (post-RoPE K / unrotated V).
//   - Self-attn K/V reads span the full [0, max_n_kv) window; mask_in
//     gates which positions are valid each step.
//   - RoPE position and KV write index come from pos_id_in / kv_idx_in
//     so the graph topology never changes.
//
// Caller pre-allocates the graph + sched once and only updates the
// runtime inputs (token_id_in, pos_id_in, kv_idx_in, mask_in) per step.
struct StepBuild {
    ggml_tensor * token_id_in = nullptr;  // i32 [1]
    ggml_tensor * pos_id_in   = nullptr;  // i32 [1]
    ggml_tensor * kv_idx_in   = nullptr;  // i64 [1]
    ggml_tensor * mask_in     = nullptr;  // f16 [max_n_kv, 1]
    ggml_tensor * argmax_out  = nullptr;  // i32 [1]
    int           max_n_kv    = 0;
    ggml_cgraph * graph       = nullptr;
};

StepBuild build_step_graph(ggml_context *           compute_ctx,
                           const MoonshineWeights & weights,
                           const MoonshineHParams & hp,
                           MoonshineKvCache &       kv_cache,
                           int                      max_n_kv,
                           int                      T_enc,
                           bool                     use_flash = true);

// Offline batched decode (B utterances). Mirrors src/arch/cohere + canary,
// with moonshine's partial RoPE on self-attn and head-dim padding.

// Batched cross-attention K/V from packed encoder outputs [d_model, T_enc_max, B].
DecoderBuild build_cross_kv_graph_batched(ggml_context *           ctx,
                                          const MoonshineWeights & w,
                                          const MoonshineHParams & hp,
                                          MoonshineKvCache &       kv_cache,
                                          int                      T_enc_max,
                                          int                      n_batch);

// Static-topology batched single-step graph for B utterances, reused for the
// single-token prompt feed and generation. Requires flash.
struct StepBuildBatched {
    ggml_tensor * token_ids_in  = nullptr;  // i32 [B]
    ggml_tensor * pos_ids_in    = nullptr;  // i32 [B]
    ggml_tensor * kv_idx_in     = nullptr;  // i64 [1, B]
    ggml_tensor * self_mask_in  = nullptr;  // f16 [max_n_kv, 1, 1, B]
    ggml_tensor * cross_mask_in = nullptr;  // f16 [T_enc_max, 1, 1, B]
    ggml_tensor * argmax_out    = nullptr;  // i32 [B]
    int           max_n_kv      = 0;
    int           n_batch       = 0;
    ggml_cgraph * graph         = nullptr;
};

StepBuildBatched build_step_graph_batched(ggml_context *           ctx,
                                          const MoonshineWeights & w,
                                          const MoonshineHParams & hp,
                                          MoonshineKvCache &       kv_cache,
                                          int                      max_n_kv,
                                          int                      T_enc_max,
                                          int                      n_batch,
                                          bool                     use_flash = true);

}  // namespace transcribe::moonshine
