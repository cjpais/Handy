// src/sanm/sanm.cpp - shared SAN-M encoder helpers + sinusoidal PE.
//
// See src/sanm/sanm.h for the high-level contract.

#include "sanm/sanm.h"

#include "conformer/conformer.h"
#include "ggml.h"

#include <cmath>

namespace transcribe::sanm {

// Force F32 accumulation for F16 weights (see mul_mat_f32acc in
// causal_lm.cpp for the CUDA COMPUTE_16F saturation rationale). SAN-M scales
// activations by sqrt(d_model), so its internal activations run large and
// overflow F16 on CUDA without this.
static ggml_tensor * mul_mat_f32acc(ggml_context * ctx, ggml_tensor * w, ggml_tensor * x) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (w->type == GGML_TYPE_F16) {
        ggml_mul_mat_set_prec(y, GGML_PREC_F32);
    }
    return y;
}

ggml_tensor * sv_layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta) {
    ggml_tensor * y = ggml_norm(ctx, x, kLayerNormEps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

ggml_tensor * fsmn_branch(ggml_context * ctx,
                          ggml_tensor *  v_pre,          // [d_model, T, B]
                          ggml_tensor *  fsmn_w,         // ne=[K, 1, d_model]
                          int            kernel,
                          ggml_tensor *  conv_pad_mask)  // [1, T, B] or null
{
    namespace conf = transcribe::conformer;

    const int64_t B       = v_pre->ne[2];
    const int64_t d_model = v_pre->ne[0];
    const int     padding = (kernel - 1) / 2;  // sanm_shift=0

    // Variable-length batch: zero padded frames before the conv so a padded
    // tail contributes nothing to a real frame's depthwise window. Broadcasts
    // over the channel axis (mask ne[0] == 1).
    ggml_tensor * v_in = v_pre;
    if (conv_pad_mask != nullptr) {
        v_in = ggml_mul(ctx, v_pre, conv_pad_mask);
    }

    // The reference op is depthwise Conv1d on a channels-first tensor
    // ([B, C, L]). Our layout is channels-innermost. Transpose to
    // [T, d_model, B] (the [W, C, N] layout the conv ops expect), then
    // transpose back.
    ggml_tensor * v_t = ggml_cont(ctx, ggml_transpose(ctx, v_in));  // [T, d_model, B]

    ggml_tensor * fsmn;
    if (B > 1) {
        // Batched depthwise conv. conv_1d_dw_f32 (im2col) collapses the
        // batch axis, so use the direct depthwise-2D op (W=T, H=1, C=d_model,
        // N=B), which threads the utterance batch at ne[3]. Kernel
        // [K, 1, d_model] -> [K, 1, 1, d_model].
        ggml_tensor * knl  = ggml_reshape_4d(ctx, fsmn_w, kernel, 1, 1, d_model);
        ggml_tensor * data = ggml_reshape_4d(ctx, v_t, v_t->ne[0], 1, d_model, B);
        fsmn               = ggml_conv_2d_dw_direct(ctx, knl, data,
                                                    /*s0=*/1, /*s1=*/1,
                                                    /*p0=*/padding, /*p1=*/0,
                                                    /*d0=*/1, /*d1=*/1);
        // [T, 1, d_model, B] -> [T, d_model, B].
        fsmn               = ggml_reshape_3d(ctx, fsmn, fsmn->ne[0], d_model, B);
    } else {
        // Single-shot: im2col path.
        fsmn = conf::conv_1d_dw_f32(ctx, fsmn_w, v_t,
                                    /*stride=*/1, /*padding=*/padding,
                                    /*dilation=*/1);
        // fsmn ne=[T, d_model, 1]. Drop the singleton batch.
        fsmn = ggml_reshape_2d(ctx, fsmn, fsmn->ne[0], fsmn->ne[1]);  // [T, d_model]
    }
    fsmn = ggml_cont(ctx, ggml_transpose(ctx, fsmn));                 // [d_model, T, B]

    // Residual within the FSMN: x_fsmn += masked_v. (Padded frames carry
    // unmasked residual here, but they are masked out of attention and
    // never decoded, so the residual on them is irrelevant.)
    fsmn = ggml_add(ctx, fsmn, v_pre);
    return fsmn;
}

ggml_tensor * sanm_attention(ggml_context *          ctx,
                             ggml_tensor *           x,  // [d_in, T, B]
                             const SanmBlockView &   b,
                             const SanmBlockParams & p) {
    const int     d_model  = p.d_model;
    const int     n_heads  = p.n_heads;
    const int     kernel   = p.kernel;
    const int     head_dim = d_model / n_heads;
    const int64_t T        = x->ne[1];
    const int64_t B        = x->ne[2];
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));

    // Fused QKV projection. W_qkv ne=[d_in, 3*d_model] (PyTorch
    // [3*d_model, d_in] stored ggml-style as [d_in, 3*d_model]).
    ggml_tensor * qkv = mul_mat_f32acc(ctx, b.attn_qkv_w, x);  // [3*d_model, T, B]
    qkv               = ggml_add(ctx, qkv, b.attn_qkv_b);

    // Split QKV along the channel axis (ne[0]). Each view is contiguous
    // in time (ne[1]) and batch (ne[2]) but the channel slices live at
    // offsets 0, d_model, 2*d_model. The utterance batch rides ne[2].
    const size_t qkv_nb1   = qkv->nb[1];
    const size_t qkv_nb2   = qkv->nb[2];
    auto         split_qkv = [&](size_t channel_offset) {
        return ggml_view_3d(ctx, qkv, d_model, T, B, qkv_nb1, qkv_nb2, channel_offset * sizeof(float));
    };
    ggml_tensor * q = split_qkv(0);
    ggml_tensor * k = split_qkv(static_cast<size_t>(d_model));
    ggml_tensor * v = split_qkv(static_cast<size_t>(2 * d_model));

    // The FSMN consumes V *before* head reshape, in the [d_model, T, B]
    // layout. Make a contiguous copy so the post-FSMN add does not see
    // a strided alias of V.
    ggml_tensor * v_pre = ggml_cont(ctx, v);

    // FSMN branch (parallel to SDPA).
    ggml_tensor * fsmn = fsmn_branch(ctx, v_pre, b.attn_fsmn_w, kernel, p.conv_pad_mask);

    // SDPA. Reshape Q,K,V to [head_dim, n_heads, T, B]. The fused QKV split
    // is non-contiguous along the channel axis, so cont each before reshape.
    auto split_heads = [&](ggml_tensor * t) {
        t = ggml_cont(ctx, t);
        return ggml_reshape_4d(ctx, t, head_dim, n_heads, T, B);
    };
    ggml_tensor * qh = split_heads(q);
    ggml_tensor * kh = split_heads(k);
    ggml_tensor * vh = split_heads(v);

    // Attention layout: ne=[head_dim, T, n_heads, B] (head_dim innermost,
    // time at ne[1], heads at ne[2], utterance batch at ne[3]).
    auto to_attn_layout = [&](ggml_tensor * t) {
        t = ggml_permute(ctx, t, 0, 2, 1, 3);  // [head_dim, T, n_heads, B]
        return ggml_cont(ctx, t);
    };
    qh = to_attn_layout(qh);
    kh = to_attn_layout(kh);
    vh = to_attn_layout(vh);

    // A variable-length batch supplies an additive key-padding mask; route
    // through the manual mul_mat + soft_max path, which broadcasts the
    // [T_k, 1, 1, B] mask cleanly over queries and heads. Same-length batches
    // (and single-shot) take the flash path.
    ggml_tensor * o;
    if (p.use_flash && p.attn_pad_mask == nullptr) {
        o = ggml_flash_attn_ext(ctx, qh, kh, vh, /*mask=*/nullptr,
                                /*scale=*/scale, /*max_bias=*/0.0f, /*logit_softcap=*/0.0f);
        // ggml_flash_attn_ext returns ne=[head_dim, n_heads, T, B] — already
        // heads-concatenated in memory. Reshape directly to [d_model, T, B].
        o = ggml_cont(ctx, o);
        o = ggml_reshape_3d(ctx, o, d_model, T, B);
    } else {
        // Manual SDPA: kq = qh @ kh^T -> [T_k, T_q, n_heads, B].
        ggml_tensor * kq = ggml_mul_mat(ctx, kh, qh);
        if (p.attn_pad_mask != nullptr) {
            kq = ggml_add(ctx, kq, p.attn_pad_mask);
        }
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale, /*max_bias=*/0.0f);
        // V^T: [T, head_dim, n_heads, B].
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, vh, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);                   // [head_dim, T, n_heads, B]
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));  // [head_dim, n_heads, T, B]
        o                     = ggml_reshape_3d(ctx, o, d_model, T, B);
    }

    o = mul_mat_f32acc(ctx, b.attn_out_w, o);
    o = ggml_add(ctx, o, b.attn_out_b);

    // SAN-M attention output: SDPA + FSMN.
    return ggml_add(ctx, o, fsmn);
}

ggml_tensor * sanm_ffn(ggml_context * ctx, ggml_tensor * x, const SanmBlockView & b) {
    ggml_tensor * h = mul_mat_f32acc(ctx, b.ffn_fc1_w, x);
    h               = ggml_add(ctx, h, b.ffn_fc1_b);
    h               = ggml_relu(ctx, h);
    h               = mul_mat_f32acc(ctx, b.ffn_fc2_w, h);
    h               = ggml_add(ctx, h, b.ffn_fc2_b);
    return h;
}

ggml_tensor * sanm_block_residual(ggml_context *          ctx,
                                  ggml_tensor *           x,
                                  const SanmBlockView &   b,
                                  const SanmBlockParams & p) {
    ggml_tensor * y = sv_layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
    y               = sanm_attention(ctx, y, b, p);
    x               = ggml_add(ctx, x, y);

    ggml_tensor * z = sv_layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
    z               = sanm_ffn(ctx, z, b);
    return ggml_add(ctx, x, z);
}

ggml_tensor * sanm_block_projection(ggml_context *          ctx,
                                    ggml_tensor *           x,  // [d_input, T]
                                    const SanmBlockView &   b,
                                    const SanmBlockParams & p) {
    ggml_tensor * y = sv_layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
    y               = sanm_attention(ctx, y, b, p);
    // No attention residual — the channel count changes (e.g. 560 → 512).
    x               = y;

    ggml_tensor * z = sv_layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
    z               = sanm_ffn(ctx, z, b);
    return ggml_add(ctx, x, z);
}

void build_sinusoidal_pe(std::vector<float> & out, int depth, int T) {
    out.assign(static_cast<size_t>(T) * depth, 0.0f);
    if (depth <= 1 || T <= 0) {
        return;
    }

    const int half = depth / 2;
    if (half <= 1) {
        return;
    }

    const double log_increment = std::log(10000.0) / static_cast<double>(half - 1);

    std::vector<double> inv_ts(static_cast<size_t>(half));
    for (int k = 0; k < half; ++k) {
        inv_ts[static_cast<size_t>(k)] = std::exp(static_cast<double>(k) * (-log_increment));
    }

    for (int i = 0; i < T; ++i) {
        const double pos = static_cast<double>(i + 1);  // 1-based
        float *      row = out.data() + static_cast<size_t>(i) * depth;
        for (int k = 0; k < half; ++k) {
            const double s = pos * inv_ts[static_cast<size_t>(k)];
            row[k]         = static_cast<float>(std::sin(s));
            row[half + k]  = static_cast<float>(std::cos(s));
        }
    }
}

}  // namespace transcribe::sanm
