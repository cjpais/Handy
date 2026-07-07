// src/sanm/sanm.h - shared SAN-M encoder helpers + sinusoidal PE.
//
// SenseVoice (SenseVoiceEncoderSmall) and Fun-ASR-Nano (FunASRNano.encoder)
// ship identical SAN-M block code: a fused QKV projection with a parallel
// FSMN depthwise-conv1d branch on V, SDPA over the QKV split, and a 2-layer
// ReLU FFN. Per-block primitives follow the conformer-style view +
// free-function pattern (src/conformer/conformer.h); sub-blocks are exposed
// for families that instrument dump points. build_sinusoidal_pe is the
// host-side PE-table companion both families add before the first block.
//
// LayerNorm epsilon is 1e-12 (FunASR's transformer.layer_norm.LayerNorm),
// NOT the conformer-shared 1e-5.

#pragma once

#include <vector>

struct ggml_context;
struct ggml_tensor;

namespace transcribe::sanm {

// LayerNorm epsilon (FunASR's transformer.layer_norm.LayerNorm); hardcoded
// to match the reference.
constexpr float kLayerNormEps = 1e-12f;

// Nullable-pointer projection over one SAN-M block's weights. Field names
// match SenseVoice's SenseVoiceBlock and Fun-ASR-Nano's EncBlock.
//
// Tensor shapes (ggml channel-innermost layout):
//   norm_attn_w/b   [d_in]
//   attn_qkv_w      [d_in, 3·d_model]   fused QKV
//   attn_qkv_b      [3·d_model]
//   attn_out_w      [d_model, d_model]
//   attn_out_b      [d_model]
//   attn_fsmn_w     [kernel, 1, d_model]   depthwise conv1d
//   norm_ffn_w/b    [d_model]
//   ffn_fc1_w       [d_model, d_ff]
//   ffn_fc1_b       [d_ff]
//   ffn_fc2_w       [d_ff, d_model]
//   ffn_fc2_b       [d_model]
struct SanmBlockView {
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_qkv_w  = nullptr;
    ggml_tensor * attn_qkv_b  = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;
    ggml_tensor * attn_fsmn_w = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;
    ggml_tensor * norm_ffn_b  = nullptr;
    ggml_tensor * ffn_fc1_w   = nullptr;
    ggml_tensor * ffn_fc1_b   = nullptr;
    ggml_tensor * ffn_fc2_w   = nullptr;
    ggml_tensor * ffn_fc2_b   = nullptr;
};

// Per-call shape for the SAN-M block. The batch axis rides ne[2] (B derived
// from x->ne[2]). Offline batching with mismatched lengths supplies two
// padding masks so a padded tail never perturbs a real frame:
//   - attn_pad_mask : additive key-padding mask ne=[T_k, 1, 1, B] f32
//       (0 on real keys, -INF on padded), added onto the score matrix; its
//       presence forces the manual SDPA path.
//   - conv_pad_mask : FSMN valid-frame mask ne=[1, T, B] f32 (1 real, 0 pad),
//       multiplies V before the depthwise conv.
// Same-length batches leave both null and run the flash path.
struct SanmBlockParams {
    int n_heads = 0;
    int d_model = 0;
    int kernel  = 0;  // FSMN depthwise kernel width (sanm_shift=0)

    ggml_tensor * attn_pad_mask = nullptr;
    ggml_tensor * conv_pad_mask = nullptr;
    bool          use_flash     = true;
};

// LayerNorm with kLayerNormEps. `beta` is optional (may be nullptr).
ggml_tensor * sv_layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta);

// FSMN parallel branch: depthwise conv1d on V (kernel-symmetric padding)
// + V residual. Input layout `v_pre` ne=[d_model, T, B]; output same shape
// (B derived from v_pre->ne[2]). `conv_pad_mask` (nullable, ne=[1, T, B])
// zeros padded frames before the conv for variable-length batches.
ggml_tensor * fsmn_branch(ggml_context * ctx,
                          ggml_tensor *  v_pre,
                          ggml_tensor *  fsmn_w,
                          int            kernel,
                          ggml_tensor *  conv_pad_mask = nullptr);

// SAN-M attention sub-block: fused QKV, FSMN parallel branch on V,
// SDPA over the QKV split. Returns ne=[d_model, T, B] (post-projection
// SDPA + post-FSMN add). Input `x` ne=[d_in, T, B] (d_in == d_model for
// residual blocks, d_input for the projection block; B == x->ne[2]).
//
// Variable-length batches supply p.attn_pad_mask / p.conv_pad_mask; a
// non-null attn_pad_mask forces the manual SDPA path. Same-length batches
// (and single-shot) run the flash path with no mask.
ggml_tensor * sanm_attention(ggml_context * ctx, ggml_tensor * x, const SanmBlockView & b, const SanmBlockParams & p);

// 2-layer ReLU FFN (no residual; caller adds it).
ggml_tensor * sanm_ffn(ggml_context * ctx, ggml_tensor * x, const SanmBlockView & b);

// Residual SAN-M block (in==out width). Used by encoders[*] and
// tp_encoders[*]. Adds attn-branch residual, then ffn residual.
ggml_tensor * sanm_block_residual(ggml_context *          ctx,
                                  ggml_tensor *           x,
                                  const SanmBlockView &   b,
                                  const SanmBlockParams & p);

// Projection SAN-M block (in!=out width, d_input → d_model). Used by
// encoders0[0] only. Skips the attn-branch residual because the channel
// count changes; the projection is itself the residual stream. The FFN
// residual still adds.
ggml_tensor * sanm_block_projection(ggml_context *          ctx,
                                    ggml_tensor *           x,
                                    const SanmBlockView &   b,
                                    const SanmBlockParams & p);

// Sinusoidal positional encoding table:
//   row-major [T, depth] f32, 1-based positions, split sin/cos halves
//   (NOT interleaved):
//     positions[i]    = i + 1                          (i = 0..T-1)
//     log_increment   = log(10000) / (depth/2 - 1)
//     inv_ts[k]       = exp(k · -log_increment)        (k = 0..half-1)
//     pe[i, k]        = sin(positions[i] · inv_ts[k])
//     pe[i, half + k] = cos(positions[i] · inv_ts[k])
//
//   Matches funasr.SinusoidalPositionEncoder.encode (Vaswani-style
//   additive PE with `torch.cat([sin, cos], dim=2)`).
void build_sinusoidal_pe(std::vector<float> & out, int depth, int T);

}  // namespace transcribe::sanm
