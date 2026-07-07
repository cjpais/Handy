// src/conformer/conformer.h - shared Conformer encoder helpers.
//
// Family-agnostic helpers for the NeMo-style Conformer encoder shared by
// Parakeet, Cohere-ASR, and future variants. Block topology:
//
//   pre_encode (DwStridingSubsampling) ->
//   N x { macaron FF1 (0.5x) -> rel-pos MHSA -> conv module
//         (pw-GLU -> depthwise -> BN -> SiLU -> pw) -> macaron FF2 (0.5x)
//         -> per-block LayerNorm }
//
// Driven by three structs: PreEncodeView / BlockView (nullable-pointer
// projections; null bias = skip the add), ConvPolicy (per-family pointwise/
// depthwise conv dispatch), and BlockParams (scalar knobs). Sub-helpers are
// exposed at namespace scope for families that hand-build a block for
// debug-dump. Helpers name no intermediates except build_pre_encode (explicit
// prefix). The f32-friendly conv helpers (conv_1d_f32, conv_2d_dw_f32,
// conv_1d_dw_f32) work around ggml kernel limitations on Metal (see
// conv_2d_dw_f32 in conformer.cpp); use them for f32 kernels on Metal.

#pragma once

#include "ggml.h"

namespace transcribe::conformer {

// LayerNorm + BatchNorm epsilon. Both default to 1e-5 in NeMo and are NOT
// stored in the GGUF, so hardcode to match.
constexpr float kLayerNormEps = 1e-5f;

// Nullable-pointer projection over the pre_encode weights. The family
// fills this from its own weights struct at the call site.
struct PreEncodeView {
    ggml_tensor * conv0_w = nullptr;  // [KW=3, KH=3, IC=1,        OC=channels]
    ggml_tensor * conv0_b = nullptr;  // [channels]
    ggml_tensor * conv2_w = nullptr;  // depthwise [KW=3, KH=3, 1, OC=channels]
    ggml_tensor * conv2_b = nullptr;  // [channels]
    ggml_tensor * conv3_w = nullptr;  // pointwise [1, 1, channels, channels]
    ggml_tensor * conv3_b = nullptr;  // [channels]
    ggml_tensor * conv5_w = nullptr;  // depthwise
    ggml_tensor * conv5_b = nullptr;
    ggml_tensor * conv6_w = nullptr;  // pointwise
    ggml_tensor * conv6_b = nullptr;
    // Final projection from [channels * (num_mels / subsampling)] to d_model.
    ggml_tensor * out_w   = nullptr;  // [pre_encode_in, d_model]
    ggml_tensor * out_b   = nullptr;  // [d_model]
};

// Nullable-pointer projection over one Conformer block's weights.
// Every `_b` (bias) slot is optional — null = skip the add.
struct BlockView {
    // Macaron FF1.
    ggml_tensor * norm_ff1_w = nullptr;  // [d_model]
    ggml_tensor * norm_ff1_b = nullptr;  // [d_model]
    ggml_tensor * ff1_lin1_w = nullptr;  // [d_model, d_ff]
    ggml_tensor * ff1_lin1_b = nullptr;  // [d_ff], nullable
    ggml_tensor * ff1_lin2_w = nullptr;  // [d_ff, d_model]
    ggml_tensor * ff1_lin2_b = nullptr;  // [d_model], nullable

    // Self-attention with relative positional encoding.
    ggml_tensor * norm_attn_w = nullptr;  // [d_model]
    ggml_tensor * norm_attn_b = nullptr;  // [d_model]
    ggml_tensor * attn_q_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_q_b    = nullptr;  // [d_model], nullable
    ggml_tensor * attn_k_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_k_b    = nullptr;  // [d_model], nullable
    ggml_tensor * attn_v_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_v_b    = nullptr;  // [d_model], nullable
    ggml_tensor * attn_out_w  = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_out_b  = nullptr;  // [d_model], nullable
    ggml_tensor * attn_pos_w  = nullptr;  // [d_model, d_model] linear_pos (no bias)
    ggml_tensor * attn_pos_u  = nullptr;  // [n_head, head_dim]
    ggml_tensor * attn_pos_v  = nullptr;  // [n_head, head_dim]

    // Convolution module. Pointwise conv1 is 2*d_model for GLU.
    ggml_tensor * norm_conv_w         = nullptr;  // [d_model]
    ggml_tensor * norm_conv_b         = nullptr;  // [d_model]
    ggml_tensor * conv_pw1_w          = nullptr;  // [1, d_model, 2*d_model]
    ggml_tensor * conv_pw1_b          = nullptr;  // [2*d_model], nullable
    ggml_tensor * conv_dw_w           = nullptr;  // [k, 1, d_model]
    ggml_tensor * conv_dw_b           = nullptr;  // [d_model], nullable
    ggml_tensor * conv_pw2_w          = nullptr;  // [1, d_model, d_model]
    ggml_tensor * conv_pw2_b          = nullptr;  // [d_model], nullable
    // Post-depthwise normalisation. Exactly one pair is populated per block
    // per BlockParams::conv_norm_type: BatchNorm (conv_bn_fused_*, fused at
    // load) or LayerNorm (conv_ln_*, per-channel mean/std at inference).
    ggml_tensor * conv_bn_fused_scale = nullptr;  // [d_model]
    ggml_tensor * conv_bn_fused_bias  = nullptr;  // [d_model]
    ggml_tensor * conv_ln_w           = nullptr;  // [d_model]
    ggml_tensor * conv_ln_b           = nullptr;  // [d_model]

    // Macaron FF2.
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_lin1_w = nullptr;
    ggml_tensor * ff2_lin1_b = nullptr;  // nullable
    ggml_tensor * ff2_lin2_w = nullptr;
    ggml_tensor * ff2_lin2_b = nullptr;  // nullable

    // Final per-block layer norm.
    ggml_tensor * norm_out_w = nullptr;
    ggml_tensor * norm_out_b = nullptr;
};

// Per-family conv dispatch policy. direct_pw is shared (detect_direct_pw is
// the same for every family today); direct_dw splits between the block site
// (direct_dw_in_block: the conformer block's 1-D depthwise after GLU) and the
// pre_encode site (direct_dw_in_pre_encode: the stride-2 2-D depthwise), since
// the two have different shapes and per-family backend choices. Defaults are
// conservative: direct_pw true (pointwise is direct mul_mat everywhere), both
// direct_dw_* false (im2col), which is safe on Metal where the direct 2-D
// depthwise kernel is not implemented for all shapes.
struct ConvPolicy {
    bool direct_pw               = true;
    bool direct_dw_in_block      = false;
    bool direct_dw_in_pre_encode = false;

    // Causal pre_encode convolutions. NeMo's cache-aware streaming swaps
    // every Conv2d in ConvSubsampling for CausalConv2D, padding
    // (left=k-1, right=stride-1) on both spatial axes — for k=3/s=2 that
    // is (left=2, right=1), different from the offline (k-1)/2 symmetric
    // path, and shifts both the freq and time output dims. False on every
    // offline variant; true on nemotron-speech-streaming-en.
    bool causal_pre_encode = false;
};

// Resolve a conv-dispatch toggle from its env overrides. The DIRECT var forces
// true, the NO_DIRECT var forces false (DIRECT wins if both are set); otherwise
// `backend_default` is used. Centralizes the override parsing shared by the
// pointwise and the per-family depthwise dispatch helpers, so the env-var read
// has exactly one definition. `direct_env` / `no_direct_env` are env var names
// (e.g. "TRANSCRIBE_CONV_DIRECT_DW" / "TRANSCRIBE_CONV_NO_DIRECT_DW").
bool resolve_conv_direct(const char * direct_env, const char * no_direct_env, bool backend_default);

// Pointwise conv dispatch auto-detect (same answer for every family today).
// Env overrides: TRANSCRIBE_CONV_NO_DIRECT_PW forces im2col,
// TRANSCRIBE_CONV_DIRECT_PW forces direct. Vulkan defaults to im2col: on
// AMD Renoir, im2col + mul_mat measured ~200 ms/encode faster than direct for
// f32 weights. Metal and CPU prefer direct.
bool detect_direct_pw(const char * backend);

// Scalar knobs for the block forward. All fields required except the
// optional local-attention window (att_context_left / att_context_right).
struct BlockParams {
    int        d_model;
    int        n_head;
    int        conv_kernel;
    ggml_type  kv_type;    // GGML_TYPE_COUNT = auto (f16 if quant, f32 if f32)
    bool       use_flash;  // false -> manual mul_mat + soft_max + mul_mat
    ConvPolicy policy;

    // Local attention window. -1 / -1 = full attention (default).
    // Semantics depend on att_context_style below.
    int att_context_left  = -1;
    int att_context_right = -1;

    // Self-attention context style.
    //
    //   Regular         — when both att_context_left/right are >= 0,
    //     pos_emb is expected to have length (left+right+1) and
    //     rel_pos_mhsa pads matrix_bd with -inf rows so positions
    //     outside the band become -inf in the attention scores after
    //     softmax. Matches NeMo's LocalAttRelPositionalEncoding when
    //     T <= 2W+1 and remains correct (band-restricted) when T
    //     exceeds the window. Default for every offline variant.
    //
    //   ChunkedLimited  — pos_emb stays at the full 2T-1 length and
    //     the caller supplies a precomputed F16 mask of shape
    //     [T_k, T_q, 1, 1] in `attn_chunked_mask` that
    //     rel_pos_mhsa adds onto matrix_bd before flash_attn. Chunk
    //     size = right+1, left_chunks = left/chunk_size; the mask is
    //     0 inside the allowed [q_chunk - left_chunks, q_chunk] band
    //     and -INF elsewhere. NeMo cache-aware streaming.
    enum class AttContextStyle { Regular, ChunkedLimited };
    AttContextStyle att_context_style = AttContextStyle::Regular;

    // Optional precomputed mask for ChunkedLimited. The caller builds
    // this as a graph input shape [T_k, T_q, 1, 1] F16 (broadcasts
    // across heads) and uploads the host-computed pattern after the
    // compute buffer is allocated. Ignored for AttContextStyle::Regular.
    ggml_tensor * attn_chunked_mask = nullptr;

    // Optional key-padding mask for variable-length offline batching.
    // Shape [T_k, 1, 1, B] F32 (0 on real keys, -INF on padded keys);
    // broadcasts across queries (ne[1]) and heads (ne[2]) and is added
    // onto the attention score matrix in rel_pos_mhsa before flash /
    // soft_max, exactly like attn_chunked_mask. Null for single-shot and
    // same-length batches. Keeps a real query from attending to another
    // utterance's padded tail frames.
    ggml_tensor * attn_pad_mask = nullptr;

    // Depthwise conv padding. -1 / -1 means "centred (k-1)/2" on both
    // sides (every offline variant). Otherwise the depthwise conv
    // uses (conv_context_left, conv_context_right) directly. NeMo's
    // `conv_context_size = "causal"` emits (kernel-1, 0).
    int conv_context_left  = -1;
    int conv_context_right = -1;

    // Optional post-GLU valid-frame mask for the conv module. Values 1.0
    // for valid frames and 0.0 for padded overhang; multiplied onto x at
    // ne=[T, d_model, B, 1], so it broadcasts over d_model (ne[1]). Shape
    // is [T, 1, 1, 1] for the single-utterance / buffered-streaming case
    // and [T, 1, B, 1] for variable-length offline batching (the B axis
    // lands in ne[2]). NeMo applies pad_mask after pointwise_conv1+GLU and
    // before the depthwise convolution; null keeps the historical path.
    ggml_tensor * conv_pad_mask = nullptr;

    // Conv-module normalisation choice. BatchNorm uses fused scale +
    // bias precomputed at load time (BlockView::conv_bn_fused_*).
    // LayerNorm computes per-channel mean/std at inference and uses
    // BlockView::conv_ln_* as the affine scale/bias.
    enum class ConvNormType { BatchNorm, LayerNorm };
    ConvNormType conv_norm_type = ConvNormType::BatchNorm;

    // Streaming (cache-aware) inputs/outputs. All null in offline mode
    // (existing graph topology). When non-null the block runs the streaming
    // path with a "virtual T" attention window (T_cache + T_q_new):
    //   streaming_channel_in  [d_model, T_cache] cache_last_channel from the
    //     previous chunk, concat-prepended before Q/K/V projection.
    //   streaming_channel_out new cache slot (tail of x_norm, size T_q_new);
    //     the driver rotates it into the persistent buffer after compute.
    //   streaming_time_in     [k-1, d_model] cache_last_time replacing the
    //     depthwise conv's zero left-pad (causal pad is (k-1, 0)).
    //   streaming_time_out    last k-1 post-pw1+GLU frames -> next left pad.
    // pos_emb and attn_chunked_mask must be sized for the virtual T.
    ggml_tensor * streaming_channel_in  = nullptr;
    ggml_tensor * streaming_channel_out = nullptr;
    ggml_tensor * streaming_time_in     = nullptr;
    ggml_tensor * streaming_time_out    = nullptr;

    // When streaming, the number of new encoder frames produced this call
    // (slices the attention output and the cache-out source). Offline: 0.
    int streaming_T_q_new = 0;

    // Optional streaming KV cache (all four set together, or none):
    // persistent [d_model, T_cache] pre-projected keys/values. When set, K/V
    // are projected only for the new frames, the cache is prepended, and the
    // rotated cache is emitted into the *_out tensors. streaming_channel_in/
    // _out are ignored in this mode (same state, recompute-K/V form).
    ggml_tensor * streaming_kv_k_in  = nullptr;
    ggml_tensor * streaming_kv_v_in  = nullptr;
    ggml_tensor * streaming_kv_k_out = nullptr;
    ggml_tensor * streaming_kv_v_out = nullptr;

    // Optional precomputed rel-pos projection [head_dim, pos_len, n_head, 1]
    // from a prior chunk of identical geometry; when non-null rel_pos_mhsa
    // consumes it directly and ignores pos_emb. Streaming-only memoization.
    ggml_tensor * streaming_pos_proj_in = nullptr;

    // Graph the helpers forward-expand their cache-write cpy nodes onto. The
    // streaming cache outputs are SIDE outputs (not reachable from the
    // encoder's `out`), so they must be added explicitly. Required when
    // streaming_*_out is non-null.
    ggml_cgraph * streaming_graph = nullptr;
};

// Low-level helpers (exposed for family-specific hand-built blocks).

// Attach a debug name to a tensor. Safe on nullptr.
ggml_tensor * named(ggml_tensor * t, const char * name);

// LayerNorm with affine: y = gamma * (x - mean) / sqrt(var + eps) + beta.
// `gamma` is required; `beta` may be null for bias-free LN.
ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta);

// Two-linear FeedForward: y = Linear2(SiLU(Linear1(x))). Biases are
// optional — pass nullptr to skip the add.
ggml_tensor * feed_forward(ggml_context * ctx,
                           ggml_tensor *  x,
                           ggml_tensor *  lin1_w,
                           ggml_tensor *  lin1_b,
                           ggml_tensor *  lin2_w,
                           ggml_tensor *  lin2_b);

// Macaron half-residual FF: x + 0.5 * FeedForward(LayerNorm(x)).
ggml_tensor * macaron_ff_residual(ggml_context * ctx,
                                  ggml_tensor *  x,
                                  ggml_tensor *  norm_w,
                                  ggml_tensor *  norm_b,
                                  ggml_tensor *  lin1_w,
                                  ggml_tensor *  lin1_b,
                                  ggml_tensor *  lin2_w,
                                  ggml_tensor *  lin2_b);

// Shaw / Transformer-XL relative-position skew. Turns a
// [pos_len, T_q, H, 1] score matrix into a [pos_len, T_q, H, 1]
// matrix rotated so column k holds the score for relative offset k.
ggml_tensor * rel_shift(ggml_context * ctx, ggml_tensor * x);

// f32-friendly Conv1D. Use instead of ggml_conv_1d for fp32 kernels on
// Metal (see conv_2d_dw_f32 in conformer.cpp).
ggml_tensor * conv_1d_f32(ggml_context * ctx,
                          ggml_tensor *  kernel,
                          ggml_tensor *  data,
                          int            stride,
                          int            padding,
                          int            dilation);

// f32-friendly 2D depthwise conv. Use instead of ggml_conv_2d_dw /
// ggml_conv_2d_dw_direct for fp32 kernels on Metal (see conformer.cpp).
ggml_tensor * conv_2d_dw_f32(ggml_context * ctx,
                             ggml_tensor *  kernel,
                             ggml_tensor *  data,
                             int            s0,
                             int            s1,
                             int            p0,
                             int            p1,
                             int            d0,
                             int            d1);

// f32-friendly 1D depthwise conv. Same Metal reasoning.
ggml_tensor * conv_1d_dw_f32(ggml_context * ctx,
                             ggml_tensor *  kernel,
                             ggml_tensor *  data,
                             int            stride,
                             int            padding,
                             int            dilation);

// Fused BatchNorm: y = x * scale + bias, with 1-D scale/bias
// reshaped for broadcast across the time axis of [T, C, 1, 1].
// Both scale_1d and bias_1d are REQUIRED (not nullable) — they come
// from the load-time BN fusion and must always be present. Callers
// that need a no-op path should skip the call entirely rather than
// pass null.
ggml_tensor * fused_batch_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * scale_1d, ggml_tensor * bias_1d);

// Add a 1-D bias [C] to a 4-D conv output [W, H, C, N] via broadcast.
// Nullable: passing bias_1d == nullptr is a no-op that returns
// conv_out unchanged.
ggml_tensor * add_conv_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d);

// Sub-blocks (exposed for families that hand-build a block).

// Convolution sub-block: pointwise -> GLU -> optional valid-frame mask
// -> depthwise -> BN/LN -> SiLU -> pointwise. Operates on post-LayerNorm
// input; the LN is applied by the caller. Reads
// BlockParams::conv_context_{left,right} (depthwise padding),
// BlockParams::conv_pad_mask, and BlockParams::conv_norm_type
// (BN vs LN).
ggml_tensor * conv_module(ggml_context * ctx, ggml_tensor * x, const BlockView & b, const BlockParams & params);

// Relative-position multi-head self-attention. Flash attention when
// use_flash is true; manual mul_mat + soft_max_ext + mul_mat fallback
// otherwise. Reads attention-context fields from BlockParams:
//
//   Regular        + att_context_{left,right} >= 0 -> local sliding
//     window. pos_emb has length (left+right+1); helper pads matrix_bd
//     with -inf rows so keys outside the band drop out of softmax.
//
//   ChunkedLimited                                  -> caller supplies
//     a precomputed [T_k, T_q] F16 mask in BlockParams::attn_chunked_mask
//     and pos_emb has full 2T-1 length. The mask is added onto
//     matrix_bd before flash_attn / soft_max_ext.
//
// Otherwise (Regular + att_context_* == -1) -> unrestricted attention.
// x_q (optional): query-only activation [d_model, T_q, B] with T_q !=
// x->ne[1]. When non-null, K/V (and the keys of the score matrix) come
// from `x` while queries come from `x_q` — the cache-aware streaming
// case where only the new frames need attention output but the cached
// frames still serve as keys/values. Requirements in this mode:
//   - pos_emb length must be T_q + T_k - 1 with the zero-offset row at
//     index T_k - 1 (row i holds relative offset (T_k - 1) - i)
//   - attn_chunked_mask (if any) must be [T_k, T_q]
//   - the flash path is bypassed (manual mul_mat + soft_max only)
// Null (default) keeps the historical self-attention behavior.
//
// k_full / v_full (optional, set together): pre-projected keys/values
// [d_model, T_k, 1, 1] (bias already applied). When set, the helper
// skips its own K/V projections entirely, queries come from `x`, and
// the same rectangular requirements as x_q apply. Streaming KV-cache
// path; mutually exclusive with x_q.
ggml_tensor * rel_pos_mhsa(ggml_context *      ctx,
                           ggml_tensor *       x,
                           ggml_tensor *       pos_emb,
                           const BlockView &   b,
                           const BlockParams & params,
                           ggml_tensor *       x_q    = nullptr,
                           ggml_tensor *       k_full = nullptr,
                           ggml_tensor *       v_full = nullptr);

// Top-level block + pre-encode.

// Optional per-block observer hook. `build_conformer_block` calls `on_point`
// at five canonical residual / post-LN points (compile-time `tag`):
//   "after_ff1", "after_attn", "after_conv", "after_ff2", "out".
// Callers attach a ggml name, stash the tensor, and/or mark it for graph
// output. Pointer-of-struct + user pointer (not std::function) keeps the
// header C-ABI-friendly. Pass nullptr to skip observation.
struct BlockObserver {
    void (*on_point)(void * user, const char * tag, ggml_tensor * t) = nullptr;
    void * user                                                      = nullptr;
};

// Full Conformer block forward: FF1 -> attn -> conv -> FF2 -> LN_out.
// `pos_emb` is shared across every block in an encoder — the caller
// computes it once and threads it through. `obs` may be null; when
// non-null its `on_point` callback fires at each of the five
// canonical points listed on BlockObserver.
ggml_tensor * build_conformer_block(ggml_context *        ctx,
                                    ggml_tensor *         x,
                                    ggml_tensor *         pos_emb,
                                    const BlockView &     b,
                                    const BlockParams &   params,
                                    const BlockObserver * obs = nullptr);

// Per-stage valid-frame masks for variable-length offline batching of the
// pre_encode conv stem. Zero-padding to a common length is NOT transparent
// through this stack: convs carry bias + ReLU, so the padded region becomes
// non-zero after the first conv and then leaks into each utterance's last
// VALID frame via the next stride-2 conv's receptive field (flips trailing
// CTC tokens). NeMo's masked subsampling zeros each conv intermediate's
// padded time region after every ReLU. When PreEncodeValidMasks is non-null,
// build_pre_encode allocates three mask graph inputs ne=[1, H_stage, 1, B]
// (one per ReLU stage), applies them, and writes the handles back for the
// driver to fill. Offline (non-causal) pre_encode only.
struct PreEncodeValidMasks {
    ggml_tensor * mask_s1 = nullptr;  // after relu0
    ggml_tensor * mask_s2 = nullptr;  // after relu3
    ggml_tensor * mask_s3 = nullptr;  // after relu6
};

// DwStridingSubsampling pre_encode stack. Returns the final
// [d_model, T_enc, 1, 1] tensor or nullptr on a shape-sanity failure.
// `name_prefix` (if non-null) names every intermediate "<prefix>.conv0"
// etc. Uses pe.out_w->ne[0] as a d_model sanity check; `error_tag` names the
// family in the diagnostic. valid_masks (optional) enables the
// masked-subsampling path above and is filled with the mask input handles.
ggml_tensor * build_pre_encode(ggml_context *        ctx,
                               const PreEncodeView & pe,
                               ggml_tensor *         mel_in,
                               const ConvPolicy &    policy,
                               const char *          name_prefix,
                               const char *          error_tag,
                               PreEncodeValidMasks * valid_masks = nullptr);

}  // namespace transcribe::conformer
