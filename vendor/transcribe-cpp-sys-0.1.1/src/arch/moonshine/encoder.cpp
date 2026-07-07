// arch/moonshine/encoder.cpp - Moonshine encoder graph builder.
//
// 3-conv stem (conv0 k=127 s=64 + tanh + GroupNorm; conv1 k=7 s=3 + gelu;
// conv2 k=3 s=2 + gelu) on raw 16 kHz PCM, then n_layers pre-LN blocks
// (bidirectional partial-RoPE MHSA + GELU MLP) and a final bias-less LN.
//
// HF uses exact-erf gelu (ggml_gelu_erf); the tanh approx drifts ~1e-4/elem.
//
// q/k/v are pre-padded to head_dim_padded = round_up(head_dim,
// pad_head_dim_multiple); the padding rows of attn_output are sliced off
// before o_proj (matching HF MoonshineAttention).

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "moonshine.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>

namespace transcribe::moonshine {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

ggml_tensor * add_conv1d_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d) {
    if (bias_1d == nullptr) {
        return conv_out;
    }
    const int64_t channels = bias_1d->ne[0];
    ggml_tensor * bias_4d  = ggml_reshape_4d(ctx, bias_1d, 1, channels, 1, 1);
    return ggml_add(ctx, conv_out, bias_4d);
}

// Partial RoPE on the leading `head_dim_rot` of dim-0 (x ne =
// [head_dim, n_heads, T, 1]). Moonshine uses GPT-J / interleaved RoPE
// (rotate_half slices x[..., 0::2] / x[..., 1::2]) = GGML_ROPE_TYPE_NORMAL,
// NOT NEOX.
ggml_tensor * apply_partial_rope(ggml_context *           ctx,
                                 ggml_tensor *            x,
                                 ggml_tensor *            positions,
                                 const MoonshineHParams & hp,
                                 int                      head_dim_rot,
                                 int                      n_ctx_orig) {
    return ggml_rope_ext(ctx, x, positions, /*c=*/nullptr, head_dim_rot, GGML_ROPE_TYPE_NORMAL, n_ctx_orig,
                         hp.rope_theta,
                         /*freq_scale=*/1.0f,
                         /*ext_factor=*/0.0f,
                         /*attn_factor=*/1.0f,
                         /*beta_fast=*/32.0f,
                         /*beta_slow=*/1.0f);
}

// Encoder MHSA with partial RoPE on q/k and head_dim padding.
//
// x:        [d_model, T]
// pos_ids:  [T] i32, encoder positions 0..T-1
// Returns:  [d_model, T]
ggml_tensor * mha_encoder(ggml_context *           ctx,
                          ggml_tensor *            x,
                          ggml_tensor *            pos_ids,
                          ggml_tensor *            q_w,
                          ggml_tensor *            k_w,
                          ggml_tensor *            v_w,
                          ggml_tensor *            out_w,
                          const MoonshineHParams & hp,
                          int                      n_heads,
                          int                      d_model,
                          bool                     use_flash) {
    const int     head_dim     = d_model / n_heads;
    const int     head_dim_pad = hp.enc_head_dim_padded();
    const int     head_dim_rot = hp.enc_head_dim_rot();
    const int     pad          = head_dim_pad - head_dim;
    // Unpadded scale: HF uses head_dim**-0.5; padded slots are
    // zero-extension and don't change the dot product.
    const float   scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T            = x->ne[1];

    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);  // [d_model, T]
    ggml_tensor * k = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * v = ggml_mul_mat(ctx, v_w, x);

    // Reshape to [head_dim, n_heads, T, 1]. ggml_rope_ext expects
    // positions to match `a->ne[2]`, so we keep T at ne[2] for the
    // rotation step.
    q = ggml_reshape_4d(ctx, q, head_dim, n_heads, T, 1);
    k = ggml_reshape_4d(ctx, k, head_dim, n_heads, T, 1);
    v = ggml_reshape_4d(ctx, v, head_dim, n_heads, T, 1);

    // Partial RoPE on q / k (v passes through).
    q = apply_partial_rope(ctx, q, pos_ids, hp, head_dim_rot,
                           /*n_ctx_orig=*/0);
    k = apply_partial_rope(ctx, k, pos_ids, hp, head_dim_rot,
                           /*n_ctx_orig=*/0);

    // Permute to [head_dim, T, n_heads, 1] then make contiguous and
    // pad axis 0 (head_dim) to head_dim_padded if required.
    auto to_attn_layout = [&](ggml_tensor * t) -> ggml_tensor * {
        t = ggml_permute(ctx, t, 0, 2, 1, 3);  // [head_dim, T, n_heads, 1]
        t = ggml_cont(ctx, t);
        if (pad > 0) {
            t = ggml_pad(ctx, t, pad, 0, 0, 0);
        }
        return t;
    };
    q = to_attn_layout(q);
    k = to_attn_layout(k);
    v = to_attn_layout(v);

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, q, k, v, /*mask=*/nullptr, scale, 0.0f, 0.0f);
        // ggml_flash_attn_ext output: [head_dim_pad, n_heads, T, 1]
        // (reduces along the K sequence axis). Rearrange to
        // [head_dim_pad, T, n_heads, 1] for the slice-and-merge below.
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, k, q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        // o shape: [head_dim_pad, T, n_heads, 1]
    }

    // Slice off head_dim padding before merging heads (HF slices
    // [..., :head_dim] before the n_heads*head_dim merge).
    if (pad > 0) {
        o = ggml_view_3d(ctx, o, head_dim, T, n_heads, o->nb[1], o->nb[2], 0);
        o = ggml_cont(ctx, o);
    }

    // Merge heads: [head_dim, T, n_heads] -> [head_dim, n_heads, T] -> [d_model, T].
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, d_model, T);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

// Encoder MLP: fc1 (with bias) -> GELU(erf) -> fc2 (with bias).
ggml_tensor * ffn_encoder(ggml_context * ctx,
                          ggml_tensor *  x,
                          ggml_tensor *  fc1_w,
                          ggml_tensor *  fc1_b,
                          ggml_tensor *  fc2_w,
                          ggml_tensor *  fc2_b) {
    ggml_tensor * h = ggml_mul_mat(ctx, fc1_w, x);
    if (fc1_b != nullptr) {
        h = ggml_add(ctx, h, fc1_b);
    }
    h               = ggml_gelu_erf(ctx, h);
    ggml_tensor * o = ggml_mul_mat(ctx, fc2_w, h);
    if (fc2_b != nullptr) {
        o = ggml_add(ctx, o, fc2_b);
    }
    return o;
}

ggml_tensor * build_block(ggml_context *            ctx,
                          ggml_tensor *             x,
                          ggml_tensor *             pos_ids,
                          const MoonshineEncBlock & b,
                          const MoonshineHParams &  hp,
                          int                       n_heads,
                          int                       d_model,
                          bool                      use_flash) {
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, /*beta=*/nullptr);
        y = mha_encoder(ctx, y, pos_ids, b.attn_q_w, b.attn_k_w, b.attn_v_w, b.attn_out_w, hp, n_heads, d_model,
                        use_flash);
        x = ggml_add(ctx, x, y);
    }
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, /*beta=*/nullptr);
        y               = ffn_encoder(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
        x               = ggml_add(ctx, x, y);
    }
    return x;
}

// Number of frames after a single Conv1d layer with no padding:
//   T_out = floor((T_in - K) / stride) + 1
int conv1d_t_out(int T_in, int K, int stride) {
    if (T_in < K) {
        return 0;
    }
    return (T_in - K) / stride + 1;
}

}  // namespace

int encoder_t_enc(const MoonshineHParams & hp, int n_samples) {
    if (n_samples <= 0 || hp.conv_kernel_sizes.size() < 3 || hp.conv_strides.size() < 3) {
        return 0;
    }
    const int t1 = conv1d_t_out(n_samples, hp.conv_kernel_sizes[0], hp.conv_strides[0]);
    if (t1 <= 0) {
        return 0;
    }
    const int t2 = conv1d_t_out(t1, hp.conv_kernel_sizes[1], hp.conv_strides[1]);
    if (t2 <= 0) {
        return 0;
    }
    const int t3 = conv1d_t_out(t2, hp.conv_kernel_sizes[2], hp.conv_strides[2]);
    return t3;
}

EncoderBuild build_encoder_graph(ggml_context *           ctx,
                                 const MoonshineWeights & w,
                                 const MoonshineHParams & hp,
                                 int                      n_samples,
                                 bool                     use_flash) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_samples <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine encoder: invalid arg (ctx=%p, n_samples=%d)",
                static_cast<void *>(ctx), n_samples);
        return eb;
    }

    const int d_model = hp.enc_d_model;
    const int n_heads = hp.enc_n_heads;
    const int T_enc   = encoder_t_enc(hp, n_samples);

    if (T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine encoder: input too short (n_samples=%d "
                "produces T_enc<=0)",
                n_samples);
        return eb;
    }

    // ---- audio input ----
    eb.audio_in = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, n_samples);
    if (eb.audio_in == nullptr) {
        return eb;
    }
    named(eb.audio_in, "enc.audio.in");
    ggml_set_input(eb.audio_in);
    eb.dumps.audio_in = eb.audio_in;
    transcribe::debug::mark_tensor_for_dump(eb.audio_in);

    // ---- position ids for partial RoPE ----
    eb.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    named(eb.pos_ids_in, "enc.pos_ids");
    ggml_set_input(eb.pos_ids_in);

    // ---- conv stem ----
    // ggml_conv_1d / conv_1d_f32 want input ne=[T, in_channels] (T
    // innermost). PCM is 1 channel: reshape [n_samples] -> [n_samples, 1].
    ggml_tensor * x = ggml_reshape_2d(ctx, eb.audio_in, n_samples, 1);

    x = conf::conv_1d_f32(ctx, w.enc_stem.conv0_w, x,
                          /*s=*/hp.conv_strides[0],
                          /*p=*/0,
                          /*d=*/1);
    // After conv: ne=[T1, d_model, 1, 1]
    x = ggml_tanh(ctx, x);

    // Transpose to [d_model, T1] for the LayerNorm-style affine
    // (and to match the dump layout).
    x = ggml_cont(ctx, ggml_transpose(ctx, x));
    named(x, "enc.conv1.out");
    eb.dumps.conv1_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // GroupNorm num_groups=1: PyTorch normalizes jointly over (C, T) per
    // batch (not just over C). ggml_group_norm matches (standardization
    // only; affine * gn_w + gn_b applied separately below), but reads
    // ne[2] as the channel axis, so reshape [C, T1] -> [C, T1, 1, 1].
    {
        const int64_t C  = w.enc_stem.gn_w->ne[0];
        const int64_t T1 = x->ne[1];
        x                = ggml_reshape_4d(ctx, x, /*ne0=*/C, /*ne1=*/T1, /*ne2=*/1, /*ne3=*/1);
        x                = ggml_group_norm(ctx, x, /*n_groups=*/hp.conv_groupnorm_num_groups,
                                           /*eps=*/hp.conv_groupnorm_eps);
        x                = ggml_reshape_2d(ctx, x, C, T1);
    }
    x = ggml_mul(ctx, x, w.enc_stem.gn_w);
    x = ggml_add(ctx, x, w.enc_stem.gn_b);
    named(x, "enc.groupnorm.out");
    eb.dumps.groupnorm_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // Transpose back to [T1, d_model] for conv1.
    x = ggml_cont(ctx, ggml_transpose(ctx, x));

    x = conf::conv_1d_f32(ctx, w.enc_stem.conv1_w, x,
                          /*s=*/hp.conv_strides[1],
                          /*p=*/0,
                          /*d=*/1);
    x = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x = ggml_gelu_erf(ctx, x);
    // -> [T2, 2·d_model]; transpose to [2·d_model, T2] for dump.
    x = ggml_cont(ctx, ggml_transpose(ctx, x));
    named(x, "enc.conv2.out");
    eb.dumps.conv2_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // Back to [T2, 2·d_model] for conv2.
    x = ggml_cont(ctx, ggml_transpose(ctx, x));

    x = conf::conv_1d_f32(ctx, w.enc_stem.conv2_w, x,
                          /*s=*/hp.conv_strides[2],
                          /*p=*/0,
                          /*d=*/1);
    x = add_conv1d_bias(ctx, x, w.enc_stem.conv2_b);
    x = ggml_gelu_erf(ctx, x);
    // -> [T_enc, d_model]; transpose to [d_model, T_enc] for the rest of the encoder.
    x = ggml_cont(ctx, ggml_transpose(ctx, x));
    named(x, "enc.conv3.out");
    eb.dumps.conv3_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- transformer blocks ----
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    eb.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        x = build_block(ctx, x, eb.pos_ids_in, w.enc_blocks[i], hp, n_heads, d_model, use_flash);

        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
        named(x, bname);
        transcribe::debug::mark_tensor_for_dump(x);
        eb.dumps.block_outs.push_back(x);
    }

    // ---- final LN (no bias) ----
    x = layer_norm(ctx, x, w.enc_top.final_norm_w, /*beta=*/nullptr);
    named(x, "enc.final");
    eb.dumps.final_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    eb.out   = x;
    eb.T_enc = T_enc;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    return eb;
}

}  // namespace transcribe::moonshine
