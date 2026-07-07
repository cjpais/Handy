// arch/whisper/decoder.h - Whisper decoder graph builder.
//
// Autoregressive Transformer: token embedding + learned positional embedding
// (NO post-embed LayerNorm), N pre-LN layers (self-attn + cross-attn + FFN),
// final LayerNorm, linear head tied to token embedding (NO bias) + log-softmax.
//
// Two graph families:
//   1. build_decoder_prefill_graph() — non-cached, used only for language
//      detection (SOT-only forward on the first chunk before the cross-KV
//      cache exists); cross-K/V recomputed inside the graph.
//   2. build_cross_kv_graph() + build_decoder_graph_kv() — KV-cached
//      production path. Cross-K/V precomputed once per chunk; self-K/V written
//      per step. One graph handles both prompt pass (n_tokens>1, n_past=0) and
//      per-step generation (n_tokens=1, n_past=current).
//
// Attention quirk (as encoder.cpp): q/v/out carry bias, k does NOT.

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::whisper {

struct WhisperHParams;
struct WhisperWeights;
struct WhisperKvCache;

struct DecoderDumps {
    ggml_tensor *              token_emb = nullptr;
    ggml_tensor *              pos_emb   = nullptr;
    ggml_tensor *              embed_sum = nullptr;
    std::vector<ggml_tensor *> block_outs;                 // per-layer residual stream
    ggml_tensor *              out_before_head = nullptr;  // post final LN
    ggml_tensor *              logits_raw      = nullptr;  // pre log-softmax logits
    ggml_tensor *              logits          = nullptr;  // log-softmax(logits_raw)
};

struct DecoderBuild {
    // Inputs (caller fills via ggml_backend_tensor_set after alloc).
    ggml_tensor * token_ids_in   = nullptr;  // [seq_len] i32
    ggml_tensor * encoder_out_in = nullptr;  // [d_model, T_enc] f32 (lang-det prefill + cross_kv graph)
    ggml_tensor * causal_mask_in = nullptr;  // [n_kv, seq_len] f32 (cast to f16 inside)

    // Cross-attention mask, only present when the cross K/V cache
    // is padded (T_enc_pad > T_enc). Shape [T_enc_pad, n_tokens] f32
    // (cast to f16 inside). Caller populates with 0 for k in [0,
    // T_enc) and -inf for k in [T_enc, T_enc_pad), so the unmasked
    // padded slots do not contribute to the cross-attn output.
    ggml_tensor * cross_mask_in = nullptr;

    // Output: log-softmax logits [vocab_size, seq_len].
    ggml_tensor * out = nullptr;

    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a decoder prefill graph in compute_ctx (no KV cache). Used
// only by the language-detection forward pass on the first chunk;
// every other decoder forward goes through build_decoder_graph_kv.
DecoderBuild build_decoder_prefill_graph(ggml_context *         compute_ctx,
                                         const WhisperWeights & weights,
                                         const WhisperHParams & hp,
                                         int                    seq_len,
                                         int                    T_enc,
                                         bool                   use_flash = true);

// Build a graph that computes cross-attention K/V for every decoder
// layer from the encoder output and writes them into the cross-attn
// KV cache. Run once per chunk (long-form audio runs the encoder per
// 30 s window and rebuilds the cross cache each time).
//
// encoder_out is the backend-resident persistent tensor populated by
// the encoder graph (shape [d_model, T_enc]). The cross-KV graph
// reads it via a view in compute_ctx — no host roundtrip required.
// Caller calls ggml_backend_sched_graph_compute after alloc_graph.
DecoderBuild build_cross_kv_graph(ggml_context *         compute_ctx,
                                  const WhisperWeights & weights,
                                  const WhisperHParams & hp,
                                  WhisperKvCache &       kv_cache,
                                  ggml_tensor *          encoder_out,
                                  int                    T_enc);

// Build a KV-cached decoder graph. Works for both prompt pass
// (n_tokens > 1, n_past = 0) and step pass (n_tokens = 1, n_past =
// prev total). Self-K/V are written into the self cache at position
// n_past; the full [0..n_past+n_tokens) window is read back for
// attention. Cross-attention reads from the pre-populated cross cache.
// The prompt pass wires every dec.* dump; step passes omit them.
//
// skip_log_softmax=true outputs pre-softmax logits (argmax-invariant,
// cheaper readback). skip_log_softmax=false matches the reference dump.
//
// kv_pad is the active-KV padding multiple for the FA kernel
// (whisper.cpp: 32 on Metal+FA, 1 elsewhere). When kv_pad > 1 the
// effective n_kv is rounded up to a multiple of kv_pad, and the
// caller-supplied causal_mask_in covers the trailing padded slots
// with -inf so they do not contribute to the FA output.
DecoderBuild build_decoder_graph_kv(ggml_context *         compute_ctx,
                                    const WhisperWeights & weights,
                                    const WhisperHParams & hp,
                                    WhisperKvCache &       kv_cache,
                                    int                    n_tokens,
                                    int                    n_past,
                                    int                    T_enc,
                                    int                    kv_pad           = 1,
                                    bool                   skip_log_softmax = false,
                                    bool                   use_flash        = true);

// Static-topology single-token decoder graph. Built once per tier
// after the prompt pass and reused for every step in the tier's
// autoregressive loop. Independent of n_past:
//   - KV cache writes go through ggml_set_rows with kv_idx_in as the
//     runtime row index (instead of ggml_cpy at a build-time-baked
//     view offset).
//   - Self-attn K/V reads span the full [0, max_n_kv) window; mask_in
//     gates which positions are valid each step.
//
// Logits-out variant: the graph emits raw pre-softmax logits, not an
// in-graph argmax. Whisper's host loop needs full logits for the
// suppress_tokens / timestamp-rule masks and temperature sampling, so
// the in-graph argmax shortcut from canary/cohere/moonshine doesn't
// apply.
struct StepBuild {
    ggml_tensor * token_id_in   = nullptr;  // i32 [1]
    ggml_tensor * pos_id_in     = nullptr;  // i32 [1]
    ggml_tensor * kv_idx_in     = nullptr;  // i64 [1]
    ggml_tensor * mask_in       = nullptr;  // f16 [max_n_kv, 1]
    ggml_tensor * cross_mask_in = nullptr;  // f32 [T_enc_pad, 1], may be null
    ggml_tensor * logits_out    = nullptr;  // f32 [vocab, 1] raw logits
    int           max_n_kv      = 0;
    ggml_cgraph * graph         = nullptr;
};

StepBuild build_step_graph(ggml_context *         compute_ctx,
                           const WhisperWeights & weights,
                           const WhisperHParams & hp,
                           WhisperKvCache &       kv_cache,
                           int                    max_n_kv,
                           int                    T_enc,
                           bool                   use_flash = true);

// Offline batched decode (B utterances in lockstep). Mirrors the single-shot
// cross-KV + step graphs but threads the batch on the trailing flash axis
// (ne[3]). Emits RAW logits [vocab, B] so the host applies whisper's per-
// utterance rules before sampling. Flash-only (the batched view layout
// requires flash_attn_ext).

// Build a batched cross-attention K/V precompute graph. encoder_out_in is a
// [d_model, T_enc_max, B] f32 input the caller fills with zero-padded
// per-utterance encoder outputs. Writes into the batched cross cache.
DecoderBuild build_cross_kv_graph_batched(ggml_context *         compute_ctx,
                                          const WhisperWeights & weights,
                                          const WhisperHParams & hp,
                                          WhisperKvCache &       kv_cache,
                                          int                    T_enc_max,
                                          int                    n_batch);

struct StepBuildBatched {
    ggml_tensor * token_ids_in  = nullptr;  // i32 [B]
    ggml_tensor * pos_ids_in    = nullptr;  // i32 [B]
    ggml_tensor * kv_idx_in     = nullptr;  // i64 [1, B]
    ggml_tensor * self_mask_in  = nullptr;  // f16 [max_n_kv, 1, 1, B]
    ggml_tensor * cross_mask_in = nullptr;  // f16 [T_enc_max, 1, 1, B]
    ggml_tensor * logits_out    = nullptr;  // f32 [vocab, B] raw logits
    int           max_n_kv      = 0;
    int           n_batch       = 0;
    ggml_cgraph * graph         = nullptr;
};

StepBuildBatched build_step_graph_batched(ggml_context *         compute_ctx,
                                          const WhisperWeights & weights,
                                          const WhisperHParams & hp,
                                          WhisperKvCache &       kv_cache,
                                          int                    max_n_kv,
                                          int                    T_enc_max,
                                          int                    n_batch,
                                          bool                   use_flash = true);

}  // namespace transcribe::whisper
