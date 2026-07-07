// arch/canary/decoder.h - Canary autoregressive Transformer decoder graph builder.
//
// Untied LM head; per-sublayer dumps at layers {0, n_layers/2, n_layers-1}.

#pragma once

#include "ggml.h"

#include <cstdint>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::canary {

struct CanaryHParams;
struct CanaryWeights;
struct CanaryKvCache;

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;
    ggml_tensor * pos_emb         = nullptr;
    ggml_tensor * embed_norm      = nullptr;
    // For canary's per-sublayer dumps: indexed by layer, three slots each.
    // Slot 0 = post self_attn residual, slot 1 = post cross_attn residual,
    // slot 2 = post ffn residual (i.e. block out).
    ggml_tensor * sub_self[64]    = {};
    ggml_tensor * sub_cross[64]   = {};
    ggml_tensor * sub_ffn[64]     = {};
    ggml_tensor * out_before_head = nullptr;
    ggml_tensor * logits_raw      = nullptr;  // pre-softmax
    ggml_tensor * logits          = nullptr;  // post-softmax (when log_softmax fires)
};

struct DecoderBuild {
    ggml_tensor * token_ids_in   = nullptr;
    ggml_tensor * pos_ids_in     = nullptr;
    ggml_tensor * encoder_out_in = nullptr;

    ggml_tensor * out        = nullptr;  // logits [vocab, seq]
    ggml_tensor * argmax_out = nullptr;  // [seq] i32 when skip_log_softmax

    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a graph that computes cross-attention K/V for all decoder
// layers from the encoder output, writing them into the cross-attn
// KV cache.
DecoderBuild build_cross_kv_graph(ggml_context *        ctx,
                                  const CanaryWeights & w,
                                  const CanaryHParams & hp,
                                  CanaryKvCache &       kv_cache,
                                  int                   T_enc);

// Build a decoder graph that uses the KV cache. Works for both prompt
// pass (n_tokens > 1, n_past == 0) and step pass (n_tokens == 1).
//
// skip_log_softmax: when true, skip the log-softmax tail and add a GPU
// argmax over the last new-token position (saves a vocab-wide softmax
// per step). The pre-softmax logits remain available via dumps.logits_raw.
DecoderBuild build_decoder_graph_kv(ggml_context *        ctx,
                                    const CanaryWeights & w,
                                    const CanaryHParams & hp,
                                    CanaryKvCache &       kv_cache,
                                    int                   n_tokens,
                                    int                   n_past,
                                    int                   T_enc,
                                    bool                  skip_log_softmax = false,
                                    bool                  use_flash        = true);

// Static-topology single-token decoder graph. Built once per utterance
// after the prompt pass and reused for every step in the autoregressive
// loop. Compared to build_decoder_graph_kv with n_tokens=1, this graph
// is independent of n_past:
//   - KV cache writes go through ggml_set_rows with kv_idx_in as the
//     runtime row index (instead of ggml_cpy at a build-time-baked
//     view offset).
//   - Self-attn K/V reads span the full [0, max_n_kv) window; mask_in
//     gates which positions are valid each step.
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

StepBuild build_step_graph(ggml_context *        ctx,
                           const CanaryWeights & w,
                           const CanaryHParams & hp,
                           CanaryKvCache &       kv_cache,
                           int                   max_n_kv,
                           int                   T_enc,
                           bool                  use_flash = true);

// ---------------------------------------------------------------------------
// Offline batched decode (B utterances). Mirrors src/arch/cohere.
// ---------------------------------------------------------------------------

// Batched cross-attention K/V. encoder_out_in is [hidden, T_enc_max, B]
// (each utterance right-padded); per-layer K/V written to the batched cross
// cache slab (layer, b). Padded frames produce bias-only K/V discarded by
// the cross-pad mask at attention time.
DecoderBuild build_cross_kv_graph_batched(ggml_context *        ctx,
                                          const CanaryWeights & w,
                                          const CanaryHParams & hp,
                                          CanaryKvCache &       kv_cache,
                                          int                   T_enc_max,
                                          int                   n_batch);

// Static-topology batched single-step graph for B utterances, reused for
// the prompt feed and generation. Self-attn writes self-KV at kv_idx[b] and
// reads [0,max_n_kv) (self_mask gates valid slots); cross-attn reads
// [0,T_enc_max) (cross_mask gates each utterance's real frames).
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

StepBuildBatched build_step_graph_batched(ggml_context *        ctx,
                                          const CanaryWeights & w,
                                          const CanaryHParams & hp,
                                          CanaryKvCache &       kv_cache,
                                          int                   max_n_kv,
                                          int                   T_enc_max,
                                          int                   n_batch,
                                          bool                  use_flash = true);

}  // namespace transcribe::canary
