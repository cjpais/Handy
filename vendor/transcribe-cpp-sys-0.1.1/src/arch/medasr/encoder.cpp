// arch/medasr/encoder.cpp - MedASR (Google LASR-CTC) Conformer encoder
// graph builder.
//
// mel.in ne=[n_mels=128, T_mel, 1, B] -> subsampling (dense + 2x
// conv1d(s=2,p=0) + dense) [d_model=512, T_enc, 1, B] -> 17 x conformer
// block {macaron FF1 [1.5,0.5] | RoPE self-attn | conv (BatchNorm) [2.0,1.0]
// | macaron FF2 [1.5,0.5] | norm_out} -> enc.out_norm -> ctc head
// (Conv1d k=1) [vocab=512, T_enc, 1, B]. All encoder LayerNorms are
// bias=false with eps=1e-6 (local `lasr_layer_norm`, not conf's 1e-5);
// RoPE is applied to Q/K after projection.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-flash-policy.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

namespace transcribe::medasr {

namespace {

namespace conf = transcribe::conformer;

// Weight-matmul forcing F32 accumulation for F16 weights. On CUDA, an F16
// weight takes the cuBLAS COMPUTE_16F path whose accumulator saturates at
// ~6.5e4 — and medasr's scaled-residual stream (macaron [1.5,0.5] + conv
// [2.0,1.0]) pushes activations to ~2e6 between blocks, so F16 accum
// overflows to NaN and every transcript comes back empty. GGML_PREC_F32
// matches CPU/Metal's always-F32 accumulate. No-op on CPU/Metal and on
// non-F16 weights (BF16 already COMPUTE_32F; quantized routes through MMQ).
ggml_tensor * mul_mat_f32acc(ggml_context * ctx, ggml_tensor * w, ggml_tensor * x) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (w->type == GGML_TYPE_F16) {
        ggml_mul_mat_set_prec(y, GGML_PREC_F32);
    }
    return y;
}

// LayerNorm with affine scale only (no bias). LASR uses
// `nn.LayerNorm(hidden_size, layer_norm_eps, bias=False)` on every
// encoder LN; eps is loaded from the GGUF (1e-6 for medasr) and is
// distinct from the conformer helper's hardcoded 1e-5.
ggml_tensor * lasr_layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * scale, float eps) {
    x = ggml_norm(ctx, x, eps);
    if (scale != nullptr) {
        x = ggml_mul(ctx, x, scale);
    }
    return x;
}

// Two-linear feed forward with no biases: y = Linear2(SiLU(Linear1(x))).
// Matches LASR's `LasrEncoderFeedForward` (SiLU activation via hidden_act).
ggml_tensor * lasr_feed_forward(ggml_context * ctx, ggml_tensor * x, ggml_tensor * up_w, ggml_tensor * down_w) {
    x = mul_mat_f32acc(ctx, up_w, x);
    x = ggml_silu(ctx, x);
    x = mul_mat_f32acc(ctx, down_w, x);
    return x;
}

// Scaled residual: out = w0 * residual + w1 * branch. Used for FF1/FF2
// macaron (w0=1.5, w1=0.5) and conv (w0=2.0, w1=1.0). For the standard
// w0=1.0 residual we just ggml_add directly.
ggml_tensor * scaled_residual(ggml_context * ctx, ggml_tensor * residual, ggml_tensor * branch, float w0, float w1) {
    ggml_tensor * a = (w0 == 1.0f) ? residual : ggml_scale(ctx, residual, w0);
    ggml_tensor * b = (w1 == 1.0f) ? branch : ggml_scale(ctx, branch, w1);
    return ggml_add(ctx, a, b);
}

// ---------------------------------------------------------------------------
// Subsampling: dense_0 -> relu -> conv_0(s=2) -> relu -> conv_1(s=2) ->
// relu -> dense_1 (no relu). The running tensor stays "channel-fast"
// [C, T, B] for the dense layers (mul_mat) and "time-fast" [T, C, B] for
// the convs (conv_1d_f32); one permute on each boundary. Per-step ne is
// noted inline below.
// ---------------------------------------------------------------------------
ggml_tensor * build_subsampling(ggml_context *            ctx,
                                const MedAsrSubsampling & ss,
                                ggml_tensor *             mel_in,
                                const MedAsrHParams &     hp,
                                const conf::ConvPolicy & /*policy*/,
                                EncoderDumps * dumps = nullptr) {
    const int sub_k = hp.enc_sub_kernel;

    // mel_in ne=[n_mels, T_mel, 1, B]. Squeeze the singleton ne[2] so
    // dense_0's mul_mat sees a clean [n_mels, T_mel, B].
    ggml_tensor * x = ggml_reshape_3d(ctx, mel_in, mel_in->ne[0], mel_in->ne[1], mel_in->ne[3]);

    // dense_0: Linear(n_mels -> d_model). mul_mat broadcasts over the
    // batch axis (ne[2]).
    x = mul_mat_f32acc(ctx, ss.dense0_w, x);
    if (ss.dense0_b != nullptr) {
        x = ggml_add(ctx, x, ss.dense0_b);
    }
    x = ggml_relu(ctx, x);
    if (dumps != nullptr) {
        dumps->sub_after_dense0 = x;
        transcribe::debug::mark_tensor_for_dump(x);
    }
    // x ne=[d_model, T_mel, B].

    // Transpose to [T_mel, d_model, B] for conv_1d_f32 (which expects
    // data ne=[T, C, B] with kernel ne=[k, C_in, C_out]).
    x = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));

    // conv_0: Conv1d(d_model -> d_model, k=5, s=2, p=0). Output T1.
    x = conf::conv_1d_f32(ctx, ss.conv0_w, x, /*stride=*/sub_k > 0 ? hp.enc_sub_stride : 2,
                          /*padding=*/0, /*dilation=*/1);
    if (ss.conv0_b != nullptr) {
        ggml_tensor * bias_r = ggml_reshape_3d(ctx, ss.conv0_b, 1, x->ne[1], 1);
        x                    = ggml_add(ctx, x, bias_r);
    }
    x = ggml_relu(ctx, x);
    if (dumps != nullptr) {
        dumps->sub_after_conv0 = x;
        transcribe::debug::mark_tensor_for_dump(x);
    }
    // x ne=[T1, d_model, B].

    // conv_1: Conv1d(d_model -> sub_channels, k=5, s=2, p=0). Output T_enc.
    x = conf::conv_1d_f32(ctx, ss.conv1_w, x, /*stride=*/hp.enc_sub_stride,
                          /*padding=*/0, /*dilation=*/1);
    if (ss.conv1_b != nullptr) {
        ggml_tensor * bias_r = ggml_reshape_3d(ctx, ss.conv1_b, 1, x->ne[1], 1);
        x                    = ggml_add(ctx, x, bias_r);
    }
    x = ggml_relu(ctx, x);
    if (dumps != nullptr) {
        dumps->sub_after_conv1 = x;
        transcribe::debug::mark_tensor_for_dump(x);
    }
    // x ne=[T_enc, sub_channels, B].

    // Transpose to [sub_channels, T_enc, B] so dense_1's mul_mat sees
    // sub_channels at ne[0].
    x = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));

    // dense_1: Linear(sub_channels -> d_model). NO RELU after this dense.
    //
    // Q4_K MMQ on CUDA overflows to +Inf for this specific tensor when
    // the activation magnitudes reach the ~3e4 range produced by the
    // ReLU-conv-ReLU-conv-ReLU stack above (see the per-stage stats in
    // the family-doc CUDA-quant debug section). The input dim here is
    // exactly 256 — one Q4_K super-block — and that shape + large
    // activations trip a CUDA-specific Q4_K MMQ bug. Workaround: cast
    // the dense_1 weight to F32 at graph build time on quants. The
    // tensor is small (256*512 = 128 KiB at F32) and runs once per
    // call, so the cost is negligible. F16/F32 GGUFs hit the cast
    // branch as a no-op (the type check skips them).
    ggml_tensor * dense1_w = ss.dense1_w;
    if (dense1_w->type != GGML_TYPE_F32 && dense1_w->type != GGML_TYPE_F16) {
        dense1_w = ggml_cast(ctx, dense1_w, GGML_TYPE_F32);
    }
    x = mul_mat_f32acc(ctx, dense1_w, x);
    if (ss.dense1_b != nullptr) {
        x = ggml_add(ctx, x, ss.dense1_b);
    }
    // x ne=[d_model, T_enc, B].
    return x;
}

// ---------------------------------------------------------------------------
// Conv module (BatchNorm path, asymmetric pad for k=32 "same")
// ---------------------------------------------------------------------------
//
// Adapts the conformer::conv_module shape but uses host-fused BN
// scale/bias passed in as input tensors (one pair per call). PyTorch's
// `padding="same"` with even k=32 and stride=1 splits total pad k-1=31
// as (left=15, right=16). conformer::conv_module supports asymmetric pad
// natively via params.conv_context_{left,right}.
ggml_tensor * build_conv_module(ggml_context *           ctx,
                                ggml_tensor *            x,
                                const MedAsrBlock &      b,
                                ggml_tensor *            bn_scale,
                                ggml_tensor *            bn_bias,
                                int                      d_model,
                                int                      conv_kernel,
                                const conf::ConvPolicy & policy,
                                ggml_tensor *            conv_pad_mask) {
    conf::BlockView bv{};
    bv.conv_pw1_w          = b.conv_pw1_w;
    bv.conv_pw1_b          = nullptr;  // bias=False on LASR conv layers
    bv.conv_dw_w           = b.conv_dw_w;
    bv.conv_dw_b           = nullptr;
    bv.conv_pw2_w          = b.conv_pw2_w;
    bv.conv_pw2_b          = nullptr;
    bv.conv_bn_fused_scale = bn_scale;
    bv.conv_bn_fused_bias  = bn_bias;

    conf::BlockParams bp{};
    bp.d_model            = d_model;
    bp.n_head             = 0;  // unused in conv_module
    bp.conv_kernel        = conv_kernel;
    bp.kv_type            = GGML_TYPE_F32;
    bp.use_flash          = false;                                   // irrelevant for conv_module
    bp.policy             = policy;
    bp.conv_context_left  = (conv_kernel - 1) / 2;                   // 15 for k=32
    bp.conv_context_right = conv_kernel - 1 - bp.conv_context_left;  // 16
    bp.conv_norm_type     = conf::BlockParams::ConvNormType::BatchNorm;
    bp.conv_pad_mask      = conv_pad_mask;

    return conf::conv_module(ctx, x, bv, bp);
}

// ---------------------------------------------------------------------------
// RoPE self-attention
// ---------------------------------------------------------------------------
//
// Standard convention (matches LasrEncoderAttention.forward):
//   q = Wq @ x; k = Wk @ x; v = Wv @ x      (all no-bias)
//   reshape to [head_dim, n_head, T, B]
//   apply RoPE to q and k (positions tensor)
//   permute to [head_dim, T, n_head, B]
//   SDPA(q, k, v, mask=None)                 (scaling = 1/sqrt(head_dim))
//   permute + reshape to [d_model, T, B]
//   y = Wo @ ...                             (no bias)
//
// The full attention path matches gigaam::build_rotary_attn — the only
// medasr-specific deltas are:
//   - LASR rotates AFTER Wq/Wk projection (standard); gigaam rotates
//     BEFORE projection.
//   - rope_theta is 10000.0 (from hparams), not 5000.0.
ggml_tensor * build_rope_attn(ggml_context *      ctx,
                              ggml_tensor *       x,
                              ggml_tensor *       positions,
                              const MedAsrBlock & b,
                              int                 d_model,
                              int                 n_head,
                              float               rope_theta,
                              int                 rope_max_pos,
                              bool                use_flash,
                              ggml_tensor *       attn_pad_mask = nullptr) {
    const int     head_dim = d_model / n_head;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T        = x->ne[1];
    const int64_t Bb       = x->ne[2];

    // Per-utterance key-padding mask is content/length dependent — force
    // the manual SDPA path so the additive mask lands cleanly on the
    // score matrix before soft_max_ext's internal scale.
    const bool flash = use_flash && (attn_pad_mask == nullptr);

    // Q/K/V projections (no bias). Outputs [d_model, T, B].
    ggml_tensor * q = mul_mat_f32acc(ctx, b.attn_q_w, x);
    ggml_tensor * k = mul_mat_f32acc(ctx, b.attn_k_w, x);
    ggml_tensor * v = mul_mat_f32acc(ctx, b.attn_v_w, x);

    // Reshape Q/K to [head_dim, n_head, T, B] for RoPE rotation. ggml_rope_ext
    // rotates ne[0] (= head_dim) using position lookups on ne[2] (= T).
    q = ggml_reshape_4d(ctx, q, head_dim, n_head, T, Bb);
    k = ggml_reshape_4d(ctx, k, head_dim, n_head, T, Bb);

    q = ggml_rope_ext(ctx, q, positions, /*c=*/nullptr,
                      /*n_dims=*/head_dim, GGML_ROPE_TYPE_NEOX,
                      /*n_ctx_orig=*/rope_max_pos, rope_theta,
                      /*freq_scale=*/1.0f, /*ext_factor=*/0.0f,
                      /*attn_factor=*/1.0f, /*beta_fast=*/32.0f,
                      /*beta_slow=*/1.0f);
    k = ggml_rope_ext(ctx, k, positions, /*c=*/nullptr,
                      /*n_dims=*/head_dim, GGML_ROPE_TYPE_NEOX,
                      /*n_ctx_orig=*/rope_max_pos, rope_theta,
                      /*freq_scale=*/1.0f, /*ext_factor=*/0.0f,
                      /*attn_factor=*/1.0f, /*beta_fast=*/32.0f,
                      /*beta_slow=*/1.0f);

    // Reshape V to [head_dim, n_head, T, B]. No rotation on V.
    v = ggml_reshape_4d(ctx, v, head_dim, n_head, T, Bb);

    // SDPA target layout: [head_dim, T, n_head, B] so the attention
    // matmuls batch over (head, utterance) along ne[2..3].
    auto to_attn = [&](ggml_tensor * t) -> ggml_tensor * {
        t = ggml_permute(ctx, t, 0, 2, 1, 3);
        return ggml_cont(ctx, t);
    };
    q = to_attn(q);
    k = to_attn(k);
    v = to_attn(v);

    ggml_tensor * o;
    if (flash) {
        o = ggml_flash_attn_ext(ctx, q, k, v, /*mask=*/nullptr, scale, 0.0f, 0.0f);
        // ggml_flash_attn_ext returns [head_dim, n_head, T, B].
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq = ggml_mul_mat(ctx, k, q);  // [T_k, T_q, n_head, B]
        if (attn_pad_mask != nullptr) {
            kq = ggml_add(ctx, kq, attn_pad_mask);
        }
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
    }
    // o: [head_dim, T, n_head, B] -> [head_dim, n_head, T, B] -> [d_model, T, B]
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_3d(ctx, o, d_model, T, Bb);

    // Output projection (no bias).
    o = mul_mat_f32acc(ctx, b.attn_o_w, o);
    return o;
}

// ---------------------------------------------------------------------------
// Single Conformer block
// ---------------------------------------------------------------------------
struct BlockObs {
    ggml_tensor * post_ff1  = nullptr;
    ggml_tensor * post_attn = nullptr;
    ggml_tensor * post_conv = nullptr;
    ggml_tensor * post_ff2  = nullptr;
};

ggml_tensor * build_block(ggml_context *           ctx,
                          ggml_tensor *            x,
                          ggml_tensor *            positions,
                          const MedAsrBlock &      b,
                          ggml_tensor *            bn_scale,
                          ggml_tensor *            bn_bias,
                          const MedAsrHParams &    hp,
                          const conf::ConvPolicy & policy,
                          bool                     use_flash,
                          ggml_tensor *            attn_pad_mask,
                          ggml_tensor *            conv_pad_mask,
                          BlockObs *               obs) {
    const int   d_model  = hp.enc_hidden;
    const int   n_head   = hp.enc_n_heads;
    const int   conv_k   = hp.enc_conv_kernel;
    const float ln_eps   = hp.enc_layer_norm_eps;
    const float rope_th  = hp.enc_rope_theta;
    const int   rope_max = hp.enc_max_pos_emb;
    const float ff_w0    = hp.enc_ff_resid_w0;    // 1.5
    const float ff_w1    = hp.enc_ff_resid_w1;    // 0.5
    const float conv_w0  = hp.enc_conv_resid_w0;  // 2.0
    const float conv_w1  = hp.enc_conv_resid_w1;  // 1.0

    // Macaron FF1.
    {
        ggml_tensor * y = lasr_layer_norm(ctx, x, b.norm_ff1_w, ln_eps);
        y               = lasr_feed_forward(ctx, y, b.ff1_up_w, b.ff1_down_w);
        x               = scaled_residual(ctx, x, y, ff_w0, ff_w1);
    }
    if (obs != nullptr) {
        obs->post_ff1 = x;
    }

    // Self-attention.
    {
        ggml_tensor * y = lasr_layer_norm(ctx, x, b.norm_attn_w, ln_eps);
        y = build_rope_attn(ctx, y, positions, b, d_model, n_head, rope_th, rope_max, use_flash, attn_pad_mask);
        x = ggml_add(ctx, x, y);
    }
    if (obs != nullptr) {
        obs->post_attn = x;
    }

    // Conv module.
    {
        ggml_tensor * y = lasr_layer_norm(ctx, x, b.norm_conv_w, ln_eps);
        y               = build_conv_module(ctx, y, b, bn_scale, bn_bias, d_model, conv_k, policy, conv_pad_mask);
        x               = scaled_residual(ctx, x, y, conv_w0, conv_w1);
    }
    if (obs != nullptr) {
        obs->post_conv = x;
    }

    // Macaron FF2.
    {
        ggml_tensor * y = lasr_layer_norm(ctx, x, b.norm_ff2_w, ln_eps);
        y               = lasr_feed_forward(ctx, y, b.ff2_up_w, b.ff2_down_w);
        x               = scaled_residual(ctx, x, y, ff_w0, ff_w1);
    }
    if (obs != nullptr) {
        obs->post_ff2 = x;
    }

    // Final per-block LN (= block output).
    x = lasr_layer_norm(ctx, x, b.norm_post_w, ln_eps);
    return x;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *        ctx,
                                 const MedAsrWeights & w,
                                 const MedAsrHParams & hp,
                                 int                   n_mel_frames,
                                 const char *          backend_name,
                                 int                   n_batch,
                                 bool                  batch_var_len) {
    EncoderBuild eb{};
    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr encoder: invalid arg "
                "(ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }
    if (n_batch < 1) {
        n_batch = 1;
    }
    const bool var_len_masks = batch_var_len && n_batch > 1;

    const int d_model  = hp.enc_hidden;
    const int n_layers = hp.enc_n_layers;

    // Mel input. The LASR reference dumps `mel.in` as numpy [T_mel, n_mels]
    // row-major (input_features shape after batch-squeeze). For ggml ne to
    // be byte-equivalent we need fast=n_mels, slow=T_mel — i.e.
    // ne=[n_mels, T_mel, 1, B]. The C++ MelFrontend writes the same
    // [T_mel, n_mels] row-major layout so the same upload byte stream
    // works in both modes.
    eb.mel_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, hp.fe_num_mels, n_mel_frames, 1, n_batch);
    if (eb.mel_in == nullptr) {
        return eb;
    }
    ggml_set_name(eb.mel_in, "mel.in");
    ggml_set_input(eb.mel_in);
    transcribe::debug::mark_tensor_for_dump(eb.mel_in);

    // Conv policy. medasr's depthwise k=32 is too large for the direct
    // 2-D depthwise on some Metal kernels; use im2col im2col_dw to stay safe
    // until empirically profiled. Pointwise is always direct mul_mat.
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    policy.direct_dw_in_block      = true;
    policy.direct_dw_in_pre_encode = false;
    policy.causal_pre_encode       = false;

    // Build subsampling (passes dumps for stage-by-stage diagnostic).
    ggml_tensor * x = build_subsampling(ctx, w.subsampling, eb.mel_in, hp, policy, &eb.dumps);
    if (x == nullptr) {
        return eb;
    }
    ggml_set_name(x, "enc.subsampling.out");
    eb.dumps.subsampling_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    if (n_layers <= 0) {
        eb.out   = x;
        eb.graph = ggml_new_graph(ctx);
        ggml_build_forward_expand(eb.graph, x);
        return eb;
    }

    const int64_t T_enc = x->ne[1];

    // Positions tensor for RoPE.
    eb.positions = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    ggml_set_name(eb.positions, "enc.positions");
    ggml_set_input(eb.positions);

    // Variable-length batch masks.
    if (var_len_masks) {
        eb.attn_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, 1, n_batch);
        eb.conv_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, n_batch, 1);
        ggml_set_name(eb.attn_pad_mask_in, "enc.attn_pad_mask");
        ggml_set_name(eb.conv_pad_mask_in, "enc.conv_pad_mask");
        ggml_set_input(eb.attn_pad_mask_in);
        ggml_set_input(eb.conv_pad_mask_in);
    }

    // Per-layer fused-BN scale/bias inputs. The driver uploads from
    // MedAsrWeights::fused_bn_*_storage after sched_alloc_graph.
    eb.bn_scale_inputs.assign(n_layers, nullptr);
    eb.bn_bias_inputs.assign(n_layers, nullptr);
    for (int i = 0; i < n_layers; ++i) {
        ggml_tensor * s  = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, d_model);
        ggml_tensor * bt = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, d_model);
        if (s == nullptr || bt == nullptr) {
            return eb;
        }
        char nm[64];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.conv_bn.scale", i);
        ggml_set_name(s, nm);
        ggml_set_input(s);
        std::snprintf(nm, sizeof(nm), "enc.block.%d.conv_bn.bias", i);
        ggml_set_name(bt, nm);
        ggml_set_input(bt);
        eb.bn_scale_inputs[i] = s;
        eb.bn_bias_inputs[i]  = bt;
    }

    // Flash attention defaults on. Forced off when the manual SDPA path is
    // required (variable-length key-padding mask, single-shot/same-length
    // batches keep flash on). TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH
    // override; MedASR is CTC-only so the decoder flag is a throwaway.
    bool enc_use_flash = true, dec_use_flash = false;
    transcribe::flash::apply_env_overrides(enc_use_flash, dec_use_flash);
    const bool use_flash = enc_use_flash;

    eb.dumps.all_block_outs.assign(n_layers, nullptr);

    for (int i = 0; i < n_layers; ++i) {
        BlockObs   obs{};
        const bool with_obs = (i == 0);
        x = build_block(ctx, x, eb.positions, w.blocks[i], eb.bn_scale_inputs[i], eb.bn_bias_inputs[i], hp, policy,
                        use_flash, eb.attn_pad_mask_in, eb.conv_pad_mask_in, with_obs ? &obs : nullptr);

        if (with_obs) {
            // Block 0 sub-steps.
            const auto mark = [&](ggml_tensor * t, const char * nm, ggml_tensor *& slot) {
                if (t == nullptr) {
                    return;
                }
                ggml_set_name(t, nm);
                transcribe::debug::mark_tensor_for_dump(t);
                slot = t;
            };
            mark(obs.post_ff1, "enc.block.0.post_ff1", eb.dumps.block0_post_ff1);
            mark(obs.post_attn, "enc.block.0.post_attn", eb.dumps.block0_post_attn);
            mark(obs.post_conv, "enc.block.0.post_conv", eb.dumps.block0_post_conv);
            mark(obs.post_ff2, "enc.block.0.post_ff2", eb.dumps.block0_post_ff2);
        }

        char nm[32];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", i);
        ggml_set_name(x, nm);
        eb.dumps.all_block_outs[i] = x;
        // Mark every block output for dump; the dump pass only emits files for
        // the names referenced by the manifest, so marking extras is cheap.
        transcribe::debug::mark_tensor_for_dump(x);
    }

    // Top-level out norm.
    x = lasr_layer_norm(ctx, x, w.enc_out_norm_w, hp.enc_layer_norm_eps);
    ggml_set_name(x, "enc.out_norm.out");
    eb.dumps.out_norm_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // CTC head: Conv1d(d_model -> vocab, k=1) = mul_mat. The converter
    // emits the weight as a k=1 conv with ne=[1, d_model, vocab]; reshape
    // to [d_model, vocab] for mul_mat (output ne=[vocab, T_enc, B], which
    // is byte-equivalent to numpy [T_enc, vocab] — handy for the decode
    // loop's per-frame argmax).
    {
        ggml_tensor * proj_w = ggml_reshape_2d(ctx, w.ctc_head.proj_w, d_model, hp.ctc_vocab_size);
        x                    = mul_mat_f32acc(ctx, proj_w, x);
        if (w.ctc_head.proj_b != nullptr) {
            x = ggml_add(ctx, x, w.ctc_head.proj_b);
        }
    }
    ggml_set_name(x, "enc.ctc_logits.raw");
    eb.dumps.ctc_logits = x;

    // Transposed view for the reference-comparable dump (numpy [vocab,
    // T_enc] order). The decode loop never reads this; only the dump pass.
    {
        ggml_tensor * logits_t = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));
        ggml_set_name(logits_t, "enc.ctc_logits");
        eb.dumps.ctc_logits_for_dump = logits_t;
        transcribe::debug::mark_tensor_for_dump(logits_t);
    }

    eb.out = x;
    ggml_set_output(eb.out);
    if (eb.dumps.ctc_logits_for_dump != nullptr) {
        ggml_set_output(eb.dumps.ctc_logits_for_dump);
    }

    // Generous node budget. 17 blocks * ~50 ops/block + subsampling +
    // CTC head ~ 1000 nodes; 8192 leaves ample headroom.
    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    // The transposed ctc_logits dump tensor is a SIDE output (reads from
    // eb.out but eb.out doesn't read from it), so it needs an explicit
    // forward-expand or the scheduler will skip it.
    if (eb.dumps.ctc_logits_for_dump != nullptr) {
        ggml_build_forward_expand(eb.graph, eb.dumps.ctc_logits_for_dump);
    }
    return eb;
}

}  // namespace transcribe::medasr
