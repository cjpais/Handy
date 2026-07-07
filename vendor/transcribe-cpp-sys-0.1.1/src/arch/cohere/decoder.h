// arch/cohere/decoder.h - Cohere ASR Transformer decoder graph builder.
//
// The decoder is an autoregressive Transformer with:
//   - Token embedding + learned positional embedding + LayerNorm
//   - N decoder layers (self-attn + cross-attn + FFN)
//   - Final LayerNorm
//   - Linear head (tied to token embedding) + log-softmax
//
// Two graph modes are supported:
//   1. Prompt pass: process N tokens, write KV cache for all layers.
//   2. Step pass: process 1 token, read+extend KV cache.
//
// Cross-attention K/V are computed in a separate graph
// (build_cross_kv_graph) and stored in the cross-attention cache.
// Both prompt and step passes read from this pre-computed cache.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::cohere {

struct CohereHParams;
struct CohereWeights;
struct CohereKvCache;

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;
    ggml_tensor * pos_emb         = nullptr;
    ggml_tensor * embed_norm      = nullptr;
    ggml_tensor * block_out[64]   = {};       // per-layer outputs (up to 64 layers)
    ggml_tensor * out_before_head = nullptr;
    ggml_tensor * logits_raw      = nullptr;  // pre-log-softmax logits
    ggml_tensor * logits          = nullptr;  // post-log-softmax logits
};

struct DecoderBuild {
    // Input handles.
    ggml_tensor * token_ids_in   = nullptr;  // [seq_len] i32
    ggml_tensor * pos_ids_in     = nullptr;  // [seq_len] i32
    ggml_tensor * encoder_out_in = nullptr;  // [dec_hidden, T_enc] f32

    // Output.
    ggml_tensor * out        = nullptr;  // logits [vocab_size, seq_len]
    ggml_tensor * argmax_out = nullptr;  // [seq_len] i32, set when skip_log_softmax

    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a decoder prompt-pass graph (no KV cache): processes the full
// prompt sequence at once with a causal self-attention mask.
//   seq_len:   number of prompt tokens.
//   T_enc:     number of encoder frames (after enc-dec projection).
//   use_flash: fused ggml_flash_attn_ext vs manual mul_mat path.
DecoderBuild build_decoder_graph(ggml_context *        compute_ctx,
                                 const CohereWeights & weights,
                                 const CohereHParams & hp,
                                 int                   seq_len,
                                 int                   T_enc,
                                 bool                  use_flash = true);

// Compute cross-attention K/V for all decoder layers from encoder_out
// (an input) into kv_cache.cross_k / cross_v via ggml_cpy. Run once per
// utterance. Returns a DecoderBuild with only .encoder_out_in and .graph set.
DecoderBuild build_cross_kv_graph(ggml_context *        compute_ctx,
                                  const CohereWeights & weights,
                                  const CohereHParams & hp,
                                  CohereKvCache &       kv_cache,
                                  int                   T_enc);

// KV-cached decoder graph for both prompt pass (n_tokens > 1) and step
// pass (n_tokens = 1). Writes new self-K/V at position n_past via ggml_cpy,
// reads the full cache [0..n_past+n_tokens) plus the pre-populated cross
// K/V, outputs logits for the new tokens only.
//   n_past: tokens already in the self-attention KV cache (0 for prompt).
//   T_enc:  number of encoder frames (cross-attention cache shape).
DecoderBuild build_decoder_graph_kv(ggml_context *        compute_ctx,
                                    const CohereWeights & weights,
                                    const CohereHParams & hp,
                                    CohereKvCache &       kv_cache,
                                    int                   n_tokens,
                                    int                   n_past,
                                    int                   T_enc,
                                    bool                  skip_log_softmax = false,
                                    bool                  use_flash        = true);

// Static-topology single-token decoder graph, built once per utterance and
// reused for every step. Independent of n_past: KV writes go via
// ggml_set_rows at runtime row index kv_idx_in; self-K/V reads span the full
// [0, max_n_kv) window with mask_in gating valid positions. Caller only
// updates the runtime inputs (token_id_in, pos_id_in, kv_idx_in, mask_in).
struct StepBuild {
    ggml_tensor * token_id_in = nullptr;  // i32 [1]
    ggml_tensor * pos_id_in   = nullptr;  // i32 [1]
    ggml_tensor * kv_idx_in   = nullptr;  // i64 [1]
    ggml_tensor * mask_in     = nullptr;  // f16 [max_n_kv, 1]
    ggml_tensor * argmax_out  = nullptr;  // i32 [1]
    int           max_n_kv    = 0;
    ggml_cgraph * graph       = nullptr;
};

StepBuild build_step_graph(ggml_context *        compute_ctx,
                           const CohereWeights & weights,
                           const CohereHParams & hp,
                           CohereKvCache &       kv_cache,
                           int                   max_n_kv,
                           int                   T_enc,
                           bool                  use_flash = true);

// ---------------------------------------------------------------------------
// Offline batched decode (B utterances at once).
// ---------------------------------------------------------------------------

// Batched cross-attention K/V. encoder_out_in is [hidden, T_enc_max, B]
// (each utterance right-padded to T_enc_max); per-layer K/V written into
// the batched cross cache slab (layer, b). Padded frames produce bias-only
// K/V that the cross-pad mask discards at attention time.
DecoderBuild build_cross_kv_graph_batched(ggml_context *        compute_ctx,
                                          const CohereWeights & weights,
                                          const CohereHParams & hp,
                                          CohereKvCache &       kv_cache,
                                          int                   T_enc_max,
                                          int                   n_batch);

// Static-topology batched single-step graph for B utterances, reused for
// both the prompt feed and generation: each call consumes one token per
// utterance, writes self-KV at kv_idx[b], reads self-KV [0,max_n_kv)
// (self_mask) + cross-KV [0,T_enc_max) (cross_mask).
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

StepBuildBatched build_step_graph_batched(ggml_context *        compute_ctx,
                                          const CohereWeights & weights,
                                          const CohereHParams & hp,
                                          CohereKvCache &       kv_cache,
                                          int                   max_n_kv,
                                          int                   T_enc_max,
                                          int                   n_batch,
                                          bool                  use_flash = true);

}  // namespace transcribe::cohere
