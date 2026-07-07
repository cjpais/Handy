// arch/moonshine_streaming/decoder.h - Moonshine-Streaming decoder graph
// builders. Mirrors moonshine's decoder; the differences are:
//
//   - Untied lm_head: a separate `dec.lm_head.weight` tensor is used for
//     the final logits projection (moonshine ties to dec.token_embd.weight).
//   - LayerNorms have no unit_offset trick.
//   - Adapter (encoder + pos_emb [+ proj]) is applied OUTSIDE the layer
//     loop in a separate `build_adapter_graph` so the in-place
//     mutation in HF's `decoder.forward` cannot bite us.

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::moonshine_streaming {

struct MoonshineStreamingHParams;
struct MoonshineStreamingWeights;
struct MoonshineStreamingKvCache;

struct DecoderDumps {
    ggml_tensor *              adapter_pos_emb = nullptr;  // adapter.pos_emb (slice)
    ggml_tensor *              adapter_out     = nullptr;  // adapter.out (post pos_emb add + proj if present)
    ggml_tensor *              token_emb       = nullptr;  // dec.token_emb
    ggml_tensor *              embed_sum       = nullptr;  // dec.embed_sum (= token_emb)
    std::vector<ggml_tensor *> block_outs;                 // dec.block.{i}.out
    ggml_tensor *              out_before_head = nullptr;  // dec.out_before_head
    ggml_tensor *              logits_raw      = nullptr;  // dec.logits_raw
    ggml_tensor *              logits          = nullptr;  // dec.logits (log_softmax)
};

struct AdapterBuild {
    ggml_tensor * encoder_out_in = nullptr;  // [enc_hidden, T_enc] f32 input
    ggml_tensor * pos_ids_in     = nullptr;  // [T_enc] i32 input
    ggml_tensor * out            = nullptr;  // [dec_hidden, T_enc]
    ggml_tensor * pos_emb_out    = nullptr;  // adapter.pos_emb dump tensor
    ggml_cgraph * graph          = nullptr;
};

struct DecoderBuild {
    ggml_tensor * token_ids_in   = nullptr;  // [n_tokens] i32
    ggml_tensor * pos_ids_in     = nullptr;  // [n_tokens] i32
    ggml_tensor * encoder_out_in = nullptr;  // [dec_hidden, T_enc] f32 (cross_kv graph only)
    ggml_tensor * causal_mask_in = nullptr;  // [n_kv, n_tokens] f32 (n_tokens>1 only)

    ggml_tensor * out        = nullptr;      // logits or log-softmax depending on flag
    ggml_tensor * argmax_out = nullptr;      // [n_tokens] i32, set when skip_log_softmax

    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a one-shot graph that applies the adapter to the encoder output:
//   x = encoder_out + pos_emb[arange(T_enc)]
//   x = (proj when adapter_has_proj else identity) (x)
// Output is host-read so the cross_kv graph can re-upload it into a
// fresh compute_ctx.
AdapterBuild build_adapter_graph(ggml_context *                    compute_ctx,
                                 const MoonshineStreamingWeights & weights,
                                 const MoonshineStreamingHParams & hp,
                                 int                               T_enc);

// Build a graph that computes cross-attn K/V for every decoder layer
// from an already-adapted encoder hidden state, writing into the cross
// cache. Reads from `encoder_out_in` (= adapter.out).
DecoderBuild build_cross_kv_graph(ggml_context *                    compute_ctx,
                                  const MoonshineStreamingWeights & weights,
                                  const MoonshineStreamingHParams & hp,
                                  MoonshineStreamingKvCache &       kv_cache,
                                  int                               T_enc);

// Build a graph that computes cross-attn K/V projections for every
// decoder layer over an [dec_d_model, n_frames] adapter slice, leaving
// the per-layer K and V tensors readable to host. Does NOT touch the
// persistent KV cache — the caller accumulates K/V into host buffers
// across feeds and pushes them into the cache once at finalize via
// build_cross_kv_commit_graph. This keeps the per-feed work bounded
// and decouples the cache allocation (which requires the final T_enc)
// from per-feed computation.
struct CrossKVProjectionBuild {
    ggml_tensor *              encoder_out_in = nullptr;  // [dec_d_model, n_frames] f32
    std::vector<ggml_tensor *> per_layer_k;               // [dec_d_model, n_frames] f32 each
    std::vector<ggml_tensor *> per_layer_v;
    ggml_cgraph *              graph = nullptr;
};

CrossKVProjectionBuild build_cross_kv_projection_graph(ggml_context *                    compute_ctx,
                                                       const MoonshineStreamingWeights & weights,
                                                       const MoonshineStreamingHParams & hp,
                                                       int                               n_frames);

// Build a graph that uploads per-layer K and V host buffers (one
// pair per decoder layer, each [dec_d_model, T_enc] f32) into the
// kv_cache.cross_k / cross_v slots via ggml_cpy. Routing the upload
// through ggml_cpy lets the backend handle any F32→F16 conversion
// that the cache's storage dtype requires.
struct CrossKVCommitBuild {
    std::vector<ggml_tensor *> per_layer_k_in;  // [dec_d_model, T_enc] f32 input each
    std::vector<ggml_tensor *> per_layer_v_in;
    ggml_cgraph *              graph = nullptr;
};

CrossKVCommitBuild build_cross_kv_commit_graph(ggml_context *                    compute_ctx,
                                               const MoonshineStreamingHParams & hp,
                                               MoonshineStreamingKvCache &       kv_cache,
                                               int                               T_enc);

// Build a KV-cached decoder graph for the prompt or step pass.
DecoderBuild build_decoder_graph_kv(ggml_context *                    compute_ctx,
                                    const MoonshineStreamingWeights & weights,
                                    const MoonshineStreamingHParams & hp,
                                    MoonshineStreamingKvCache &       kv_cache,
                                    int                               n_tokens,
                                    int                               n_past,
                                    int                               T_enc,
                                    bool                              skip_log_softmax = false,
                                    bool                              use_flash        = true);

// ---------------------------------------------------------------------------
// Offline batched decode (B utterances). Mirrors src/arch/moonshine, with
// the streaming-specific untied lm_head. The adapter is applied per
// utterance (serial) before the batched cross-KV, so the cross input here
// is the already-adapted [dec_d_model, T_enc_max, B] hidden state.
// ---------------------------------------------------------------------------

DecoderBuild build_cross_kv_graph_batched(ggml_context *                    ctx,
                                          const MoonshineStreamingWeights & w,
                                          const MoonshineStreamingHParams & hp,
                                          MoonshineStreamingKvCache &       kv_cache,
                                          int                               T_enc_max,
                                          int                               n_batch);

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

StepBuildBatched build_step_graph_batched(ggml_context *                    ctx,
                                          const MoonshineStreamingWeights & w,
                                          const MoonshineStreamingHParams & hp,
                                          MoonshineStreamingKvCache &       kv_cache,
                                          int                               max_n_kv,
                                          int                               T_enc_max,
                                          int                               n_batch,
                                          bool                              use_flash = true);

}  // namespace transcribe::moonshine_streaming
