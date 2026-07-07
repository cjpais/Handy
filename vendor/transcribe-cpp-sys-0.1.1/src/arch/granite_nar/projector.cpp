// arch/granite_nar/projector.cpp - NLE EncoderProjectorQFormer.
//
// Reference: EncoderProjectorQFormer in modeling_nle.py. Strictly
// simpler than the AR BLIP-2 Q-Former: no self-attention sublayer;
// the layer is pre-LN -> cross-attn -> add residual -> pre-LN -> FFN
// -> add residual.

#include "projector.h"

#include "ggml.h"
#include "granite_nar.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::granite_nar {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta, float eps) {
    ggml_tensor * y = ggml_norm(ctx, x, eps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

ggml_tensor * linear(ggml_context * ctx, ggml_tensor * x, ggml_tensor * w, ggml_tensor * b) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (b != nullptr) {
        y = ggml_add(ctx, y, b);
    }
    return y;
}

// Cross-attention with explicit Q/K/V already projected. Inputs have
// ne = [hidden, seq, batch]. Output ne = [hidden, q_seq, batch].
ggml_tensor * cross_attn(ggml_context * ctx,
                         ggml_tensor *  q_in,
                         ggml_tensor *  k_in,
                         ggml_tensor *  v_in,
                         int            n_heads,
                         int            head_dim) {
    const int64_t q_seq = q_in->ne[1];
    const int64_t k_seq = k_in->ne[1];
    const int64_t batch = q_in->ne[2];
    const float   scale = 1.0f / std::sqrt(static_cast<float>(head_dim));

    auto split = [&](ggml_tensor * t, int64_t seq) {
        ggml_tensor * r = ggml_reshape_4d(ctx, t, head_dim, n_heads, seq, batch);
        r               = ggml_cont(ctx, ggml_permute(ctx, r, 0, 2, 1, 3));
        return r;
    };
    ggml_tensor * q = split(q_in, q_seq);
    ggml_tensor * k = split(k_in, k_seq);
    ggml_tensor * v = split(v_in, k_seq);

    ggml_tensor * kq   = ggml_mul_mat(ctx, k, q);
    kq                 = ggml_scale(ctx, kq, scale);
    ggml_tensor * attn = ggml_soft_max(ctx, kq);

    ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
    ggml_tensor * out = ggml_mul_mat(ctx, v_t, attn);
    out               = ggml_cont(ctx, ggml_permute(ctx, out, 0, 2, 1, 3));
    out               = ggml_reshape_3d(ctx, out, static_cast<int64_t>(head_dim) * n_heads, q_seq, batch);
    return out;
}

}  // namespace

ProjectorBuild build_projector_graph(ggml_context *            ctx,
                                     const GraniteNarWeights & weights,
                                     const GraniteNarHParams & hp,
                                     int                       T_enc) {
    ProjectorBuild pb{};

    const int   block_size = hp.prj_block_size;          // 15
    const int   downsample = hp.prj_downsample_rate;     // 5
    const int   n_query    = block_size / downsample;    // 3
    const int   n_heads    = hp.prj_n_heads;             // 32
    const int   prj_hidden = hp.prj_hidden;              // 2048
    const int   enc_layers = hp.prj_num_encoder_layers;  // 4
    const int   enc_hidden = hp.prj_encoder_dim;         // 1024
    const int   cat_in     = enc_layers * enc_hidden;    // 4096
    const float ln_eps     = hp.prj_layernorm_eps;

    if (prj_hidden % n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar projector: prj_hidden (%d) %% n_heads (%d) != 0", prj_hidden,
                n_heads);
        return pb;
    }
    const int head_dim = prj_hidden / n_heads;  // 64

    pb.nblocks        = (T_enc + block_size - 1) / block_size;
    const int T_pad   = pb.nblocks * block_size;
    pb.t_enc_pad      = T_pad - T_enc;
    pb.n_audio_tokens = pb.nblocks * n_query;

    pb.enc_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, cat_in, T_enc);
    named(pb.enc_in, "proj.enc_in");
    ggml_set_input(pb.enc_in);

    // Per-encoder-layer LayerNorms.
    // Slice enc_in along the channel axis into `enc_layers` chunks of
    // size `enc_hidden`, LN each chunk with its own gamma/beta, then
    // concat back.
    ggml_tensor * normed       = nullptr;
    const size_t  cat_in_bytes = ggml_element_size(pb.enc_in) * enc_hidden;
    for (int j = 0; j < enc_layers; ++j) {
        ggml_tensor * slc = ggml_view_2d(ctx, pb.enc_in, enc_hidden, T_enc, pb.enc_in->nb[1], cat_in_bytes * j);
        slc               = ggml_cont(ctx, slc);
        slc = layer_norm(ctx, slc, weights.proj_top.layer_norms_w[j], weights.proj_top.layer_norms_b[j], ln_eps);
        if (normed == nullptr) {
            normed = slc;
        } else {
            normed = ggml_concat(ctx, normed, slc, /*dim=*/0);
        }
    }
    // normed ne = [cat_in, T_enc]

    // Linear projector (cat_in -> prj_hidden) + GELU.
    ggml_tensor * proj_lp = linear(ctx, normed, weights.proj_top.layer_projector_w, weights.proj_top.layer_projector_b);
    proj_lp               = ggml_gelu(ctx, proj_lp);
    // proj_lp ne = [prj_hidden, T_enc]

    // Pad along T to nblocks * block_size.
    // The pad is allocated at prj_hidden width (post-GELU) so the
    // caller only uploads a small zero buffer once per encode. The
    // reference does the pad on the same axis, post-layer_projector,
    // before the windowed pool.
    ggml_tensor * full = proj_lp;
    if (pb.t_enc_pad > 0) {
        pb.enc_pad = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, prj_hidden, pb.t_enc_pad);
        named(pb.enc_pad, "proj.t_pad_zero");
        ggml_set_input(pb.enc_pad);
        full = ggml_concat(ctx, proj_lp, pb.enc_pad, /*dim=*/1);
    }
    // full ne = [prj_hidden, T_pad]

    // Reshape to [prj_hidden, block_size, nblocks].
    ggml_tensor * windowed = ggml_reshape_3d(ctx, full, prj_hidden, block_size, pb.nblocks);

    // K/V: x + window_positions.
    // window_positions tensor ne = [prj_hidden, block_size, 1]. We add
    // it to `windowed` which is [prj_hidden, block_size, nblocks]; ggml
    // broadcasts the ne[2]=1 across the nblocks dim.
    ggml_tensor * wpos = ggml_cast(ctx, weights.proj_top.window_positions, GGML_TYPE_F32);
    ggml_tensor * kv   = ggml_add(ctx, windowed, wpos);

    // Mean-pool windowed -> [prj_hidden, n_query, nblocks].
    // Reference reshapes the windowed tensor (B*nblocks, block_size, h) as
    // (B*nblocks, n_query, downsample, h) and means over the downsample
    // axis. In ggml ne order, this is reshape [h, block_size, nblocks] ->
    // [h, downsample, n_query, nblocks] then mean over ne[1].
    ggml_tensor * mp_view   = ggml_reshape_4d(ctx, windowed, prj_hidden, downsample, n_query, pb.nblocks);
    // ggml_mean reduces over ne[0] only. Permute so downsample becomes
    // ne[0]: (h, downsample, n_query, nblocks) -> (downsample, h, n_query, nblocks)
    // via permute(1, 0, 2, 3). Then mean reduces ne[0] = downsample,
    // yielding (1, h, n_query, nblocks). Squeeze ne[0]=1 with reshape.
    ggml_tensor * mp_perm   = ggml_cont(ctx, ggml_permute(ctx, mp_view, 1, 0, 2, 3));
    ggml_tensor * mp_mean   = ggml_mean(ctx, mp_perm);
    // mp_mean ne = [1, h, n_query, nblocks]; reshape to [h, n_query, nblocks].
    ggml_tensor * mean_pool = ggml_reshape_3d(ctx, mp_mean, prj_hidden, n_query, pb.nblocks);

    // Query: learned [prj_hidden, n_query, 1] + mean_pool.
    ggml_tensor * query = ggml_cast(ctx, weights.proj_top.query, GGML_TYPE_F32);
    // Broadcast query over the nblocks axis when adding mean_pool. ggml_add
    // broadcasts dims of size 1, so the [h, n_query, 1] query expands
    // naturally to match mean_pool's [h, n_query, nblocks].
    query               = ggml_add(ctx, mean_pool, query);

    // Q-Former layers (no self-attention).
    for (int i = 0; i < hp.prj_n_layers; ++i) {
        const auto & b = weights.proj_blocks[i];

        // Pre-LN cross-attention.
        ggml_tensor * q_norm = layer_norm(ctx, query, b.norm_attn_w, b.norm_attn_b, ln_eps);
        ggml_tensor * q_proj = linear(ctx, q_norm, b.cross_attn_q_w, b.cross_attn_q_b);
        ggml_tensor * k_proj = linear(ctx, kv, b.cross_attn_k_w, b.cross_attn_k_b);
        ggml_tensor * v_proj = linear(ctx, kv, b.cross_attn_v_w, b.cross_attn_v_b);
        ggml_tensor * attn   = cross_attn(ctx, q_proj, k_proj, v_proj, n_heads, head_dim);
        attn                 = linear(ctx, attn, b.cross_attn_o_w, b.cross_attn_o_b);
        query                = ggml_add(ctx, query, attn);

        // Pre-LN FFN.
        ggml_tensor * f_norm = layer_norm(ctx, query, b.norm_ffn_w, b.norm_ffn_b, ln_eps);
        ggml_tensor * mid    = linear(ctx, f_norm, b.ffn_fc1_w, b.ffn_fc1_b);
        mid                  = ggml_silu(ctx, mid);
        ggml_tensor * f_out  = linear(ctx, mid, b.ffn_fc2_w, b.ffn_fc2_b);
        query                = ggml_add(ctx, query, f_out);
    }

    // qformer.out dump tap (HF emits this BEFORE out_norm + out_linear).
    named(query, "proj.qformer.out");
    pb.dumps.qformer_out = query;
    transcribe::debug::mark_tensor_for_dump(query);

    // out_norm + out_linear (LLM-space).
    ggml_tensor * onorm = layer_norm(ctx, query, weights.proj_top.out_norm_w, weights.proj_top.out_norm_b, ln_eps);

    // Reshape to [prj_hidden, n_query * nblocks].
    ggml_tensor * tokens = ggml_reshape_2d(ctx, onorm, prj_hidden, static_cast<int64_t>(n_query) * pb.nblocks);
    ggml_tensor * out    = ggml_mul_mat(ctx, weights.proj_top.out_linear_w, tokens);
    out                  = ggml_add(ctx, out, weights.proj_top.out_linear_b);
    named(out, "proj.out");
    pb.out            = out;
    pb.dumps.proj_out = out;
    ggml_set_output(out);
    transcribe::debug::mark_tensor_for_dump(out);

    pb.graph = ggml_new_graph_custom(ctx, /*size=*/4096, /*grads=*/false);
    if (pb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar projector: ggml_new_graph_custom failed");
        return pb;
    }
    ggml_build_forward_expand(pb.graph, pb.out);
    if (pb.dumps.qformer_out) {
        ggml_build_forward_expand(pb.graph, pb.dumps.qformer_out);
    }

    return pb;
}

}  // namespace transcribe::granite_nar
