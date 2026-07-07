// src/granite_conformer/shaw_attn.cpp - shared Shaw block-local attention.
//
// See shaw_attn.h for the contract.

#include "shaw_attn.h"

#include "ggml.h"
#include "transcribe-log.h"

#include <cmath>
#include <cstdio>

namespace transcribe::granite_conformer {

namespace {

ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta, float eps) {
    ggml_tensor * y = ggml_norm(ctx, x, eps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

}  // namespace

ggml_tensor * shaw_block_attn(ggml_context *          ctx,
                              ggml_tensor *           x,
                              ggml_tensor *           zero_pad,
                              ggml_tensor *           dists,
                              ggml_tensor *           pad_mask_3d,
                              const ShawAttnWeights & w,
                              int                     n_heads,
                              int                     head_dim,
                              int                     context_size,
                              int                     num_blocks,
                              int                     T_enc,
                              float                   layer_norm_eps) {
    const int64_t inner_dim = static_cast<int64_t>(n_heads) * head_dim;
    const int64_t T_pad     = static_cast<int64_t>(context_size) * num_blocks;
    const float   scale     = 1.0f / std::sqrt(static_cast<float>(head_dim));

    // Offline batch: x ne=[d_model, T_enc, B]. Block-local attention treats
    // each time-block independently, so B utterances fold into the block
    // axis: num_blocks_eff = num_blocks * B (memory order [.., T_pad, B]
    // splits cleanly into (ctx, num_blocks*B)). The caller tiles the per-block
    // pad mask across B and sizes zero_pad with the batch.
    const int64_t B              = x->ne[2];
    const int64_t num_blocks_eff = static_cast<int64_t>(num_blocks) * B;

    // Pre-LayerNorm on the T_enc input.
    ggml_tensor * h = layer_norm(ctx, x, w.norm_attn_w, w.norm_attn_b, layer_norm_eps);

    // Zero-pad along the time axis when T_enc is not block-aligned. The
    // pad buffer is a caller-owned graph input; nullptr is a programmer
    // error.
    if (T_pad > T_enc) {
        if (zero_pad == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "granite_conformer::shaw_block_attn: "
                    "zero_pad is null but T_pad > T_enc");
            return nullptr;
        }
        h = ggml_concat(ctx, h, zero_pad, /*dim=*/1);
    }
    // h ne = [d_model, T_pad]

    // QKV projections. q has no bias; kv is fused (2*inner_dim) with no
    // bias; out has bias.
    ggml_tensor * q  = ggml_mul_mat(ctx, w.attn_q_w, h);   // [inner_dim, T_pad]
    ggml_tensor * kv = ggml_mul_mat(ctx, w.attn_kv_w, h);  // [2*inner_dim, T_pad]

    // Split kv into k, v along ne[0]. The mul_mat output is contiguous, so
    // views with a strided ne[0] are safe. ne[2] carries the utterance batch
    // (1 for single-shot, == B otherwise).
    ggml_tensor * k = ggml_view_3d(ctx, kv, inner_dim, kv->ne[1], kv->ne[2], kv->nb[1], kv->nb[2], /*offset=*/0);
    ggml_tensor * v =
        ggml_view_3d(ctx, kv, inner_dim, kv->ne[1], kv->ne[2], kv->nb[1], kv->nb[2], inner_dim * ggml_element_size(kv));
    k = ggml_cont(ctx, k);
    v = ggml_cont(ctx, v);

    // Reshape into block-local form. Target ne layout
    // [head_dim, context_size, n_heads, num_blocks_eff] so that
    // ggml_mul_mat(k, q) does per-(head, block, utterance) batched attention.
    auto reshape_qkv = [&](ggml_tensor * t) -> ggml_tensor * {
        ggml_tensor * r = ggml_reshape_4d(ctx, t, head_dim, n_heads, context_size, num_blocks_eff);
        r               = ggml_cont(ctx, ggml_permute(ctx, r, 0, 2, 1, 3));
        return r;
    };
    q = reshape_qkv(q);
    k = reshape_qkv(k);
    v = reshape_qkv(v);

    // Shaw positional bias. rel_pos_emb [head_dim, 2*max_pos_emb+1], dists
    // [context_size, context_size] int32. Materialise the per-(c, r) lookup
    // as [head_dim, context_size*context_size], then reshape to
    // [head_dim, r=context_size, c=context_size].
    ggml_tensor * dists_flat = ggml_reshape_1d(ctx, dists, static_cast<int64_t>(context_size) * context_size);
    ggml_tensor * rel_lookup = ggml_get_rows(ctx, w.attn_rel_pos_emb, dists_flat);
    rel_lookup               = ggml_reshape_3d(ctx, rel_lookup, head_dim, context_size, context_size);

    // pos_attn[h, b, c, r] = sum_d q[h, b, c, d] * rel_lookup[c, r, d].
    // Permute q to put c on ne[2] for batched mul_mat against
    // rel_lookup; rel_lookup's ne[3]=1 broadcasts across num_blocks.
    ggml_tensor * q_perm   = ggml_cont(ctx, ggml_permute(ctx, q, 0, 2, 1, 3));
    ggml_tensor * pos_attn = ggml_mul_mat(ctx, rel_lookup, q_perm);

    // QK^T: ggml_mul_mat(K, Q), both [head_dim, context_size, n_heads,
    // num_blocks] -> [k=context_size, q=context_size, n_heads, num_blocks].
    ggml_tensor * kq = ggml_mul_mat(ctx, k, q);

    // Align pos_attn's axis order to kq's before the add.
    pos_attn = ggml_cont(ctx, ggml_permute(ctx, pos_attn, 0, 2, 1, 3));

    // scores = (kq + pos_attn) * scale + pad_mask
    ggml_tensor * scores = ggml_add(ctx, kq, pos_attn);
    scores               = ggml_scale(ctx, scores, scale);

    // Broadcast the per-block additive mask across n_heads by giving it
    // ne[2]=1 in a 4D view. pad_mask_3d is [ctx, ctx, num_blocks_eff] — the
    // caller tiles the per-block pattern across the batch.
    ggml_tensor * pad_mask_4d = ggml_reshape_4d(ctx, pad_mask_3d, context_size, context_size, 1, num_blocks_eff);
    scores                    = ggml_add(ctx, scores, pad_mask_4d);

    // Softmax over the k axis (ne[0]).
    ggml_tensor * attn = ggml_soft_max(ctx, scores);

    // out = V^T @ attn → [head_dim, q, n_heads, num_blocks].
    ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
    ggml_tensor * out = ggml_mul_mat(ctx, v_t, attn);

    // Reshape back to [inner_dim, T_pad, B].
    out = ggml_cont(ctx, ggml_permute(ctx, out, 0, 2, 1, 3));
    out = ggml_reshape_3d(ctx, out, inner_dim, T_pad, B);

    // Slice off pad rows BEFORE out_proj. This mirrors the reference's
    // `out = self.to_out(out[:, :num_features, :])` order; it keeps
    // pad-row garbage from leaking through the post-attention depthwise
    // conv.
    if (T_pad > T_enc) {
        out = ggml_view_3d(ctx, out, inner_dim, T_enc, B, out->nb[1], out->nb[2], 0);
        out = ggml_cont(ctx, out);
    }

    // Final out_proj (with bias). Operates at T_enc.
    out = ggml_mul_mat(ctx, w.attn_out_w, out);
    if (w.attn_out_b != nullptr) {
        out = ggml_add(ctx, out, w.attn_out_b);
    }
    // out ne = [d_model, T_enc]
    return out;
}

}  // namespace transcribe::granite_conformer
