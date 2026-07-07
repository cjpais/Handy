// arch/moonshine_streaming/encoder.cpp - Moonshine-Streaming encoder
// graph builder.
//
// Time-domain frontend: frame reshape [frame_len, T_frames] -> CMVN
// (ggml_norm eps=1e-6, no affine) -> asinh(exp(log_k)·x) -> linear + SiLU ->
// 2× causal Conv1d (left-pad k-1, stride 2; conv1 +bias +SiLU, conv2 +bias
// only). Then n_layers transformer blocks (per-layer sliding-window mask, no
// RoPE) and a final bias-less LN.
//
// Encoder LayerNorm note: every MoonshineStreamingLayerNorm has
// unit_offset=True (effective gain = γ + 1.0). The converter pre-folds the
// +1.0 into the tensor so we use the plain `ggml_norm * scale` pattern.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "moonshine_streaming.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <limits>

namespace transcribe::moonshine_streaming {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

constexpr float kLayerNormEps = 1e-5f;

ggml_tensor * add_conv1d_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d) {
    if (bias_1d == nullptr) {
        return conv_out;
    }
    const int64_t channels = bias_1d->ne[0];
    ggml_tensor * bias_4d  = ggml_reshape_4d(ctx, bias_1d, 1, channels, 1, 1);
    return ggml_add(ctx, conv_out, bias_4d);
}

// asinh(z) = log(z + sqrt(z^2 + 1)).
// PCM range and `exp(log_k)` ≈ 0.5 keep z in roughly [-0.5, 0.5] so the
// stability tricks PyTorch uses (recasting for large |z|) aren't needed.
ggml_tensor * asinh_op(ggml_context * ctx, ggml_tensor * z) {
    ggml_tensor * z2          = ggml_sqr(ctx, z);
    ggml_tensor * z2_plus_one = ggml_scale_bias(ctx, z2, /*s=*/1.0f, /*b=*/1.0f);
    ggml_tensor * s           = ggml_sqrt(ctx, z2_plus_one);
    ggml_tensor * z_plus_s    = ggml_add(ctx, z, s);
    return ggml_log(ctx, z_plus_s);
}

// Encoder MHSA without RoPE, with a per-layer sliding-window mask.
//
// x:        [d_model, T_enc]            (residual stream dim = enc_d_model)
// mask:     [T_enc, T_enc] f32 — uploaded by caller, cast to F16 inside graph
// Returns:  [d_model, T_enc]
//
// q_w/k_w/v_w have ggml ne [d_model, attn_dim]; out_w has ne
// [attn_dim, d_model]. attn_dim = n_heads * head_dim. attn_dim may be
// smaller than d_model (small/medium); for tiny they are equal.
ggml_tensor * mha_encoder_swa(ggml_context *                    ctx,
                              ggml_tensor *                     x,
                              ggml_tensor *                     mask_f32,
                              ggml_tensor *                     q_w,
                              ggml_tensor *                     k_w,
                              ggml_tensor *                     v_w,
                              ggml_tensor *                     out_w,
                              const MoonshineStreamingHParams & hp,
                              int                               n_heads,
                              int                               head_dim,
                              bool                              use_flash) {
    const int     attn_dim     = n_heads * head_dim;
    const int     head_dim_pad = hp.enc_head_dim_padded();
    const int     pad          = head_dim_pad - head_dim;
    // HF reference uses unpadded head_dim for the scale.
    const float   scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T            = x->ne[1];

    // After projections: ne0 = attn_dim (not d_model when non-square).
    ggml_tensor * Q = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * K = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * V = ggml_mul_mat(ctx, v_w, x);

    // [attn_dim, T] → [head_dim, n_heads, T, 1]
    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_heads, T, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_heads, T, 1);

    // Permute to [head_dim, T, n_heads, 1].
    auto to_attn_layout = [&](ggml_tensor * t) -> ggml_tensor * {
        t = ggml_permute(ctx, t, 0, 2, 1, 3);
        t = ggml_cont(ctx, t);
        if (pad > 0) {
            t = ggml_pad(ctx, t, pad, 0, 0, 0);
        }
        return t;
    };
    Q = to_attn_layout(Q);
    K = to_attn_layout(K);
    V = to_attn_layout(V);

    // ggml_soft_max_ext / ggml_flash_attn_ext require F16 mask.
    ggml_tensor * mask_f16 = ggml_cast(ctx, mask_f32, GGML_TYPE_F16);

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask_f16, scale, 0.0f, 0.0f);
        // FA output: [head_dim_pad, n_heads, T, 1] → [head_dim_pad, T, n_heads, 1]
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask_f16, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        // o ne: [head_dim_pad, T, n_heads, 1]
    }

    if (pad > 0) {
        o = ggml_view_3d(ctx, o, head_dim, T, n_heads, o->nb[1], o->nb[2], 0);
        o = ggml_cont(ctx, o);
    }

    // Merge heads: [head_dim, T, n_heads] → [head_dim, n_heads, T] → [attn_dim, T].
    // The output projection out_w (ne [attn_dim, d_model]) lifts back to d_model.
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, attn_dim, T);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

// Encoder MLP: fc1 (with bias) → GELU(erf) → fc2 (with bias).
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

ggml_tensor * build_encoder_block(ggml_context *                     ctx,
                                  ggml_tensor *                      x,
                                  ggml_tensor *                      mask_f32,
                                  const MoonshineStreamingEncBlock & b,
                                  const MoonshineStreamingHParams &  hp,
                                  int                                n_heads,
                                  int                                head_dim,
                                  bool                               use_flash) {
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, /*beta=*/nullptr);
        y = mha_encoder_swa(ctx, y, mask_f32, b.attn_q_w, b.attn_k_w, b.attn_v_w, b.attn_out_w, hp, n_heads, head_dim,
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

// Output time of one streaming Conv1d (left-pad k-1, stride 2):
//   T_out = floor((T_in + (k-1) - k) / s) + 1 = floor((T_in - 1) / s) + 1
//         = ceil(T_in / s)
int causal_conv1d_t_out(int T_in, int /*k*/, int s) {
    if (T_in <= 0 || s <= 0) {
        return 0;
    }
    return (T_in + s - 1) / s;
}

}  // namespace

bool dump_block_index(int i, int n_layers) {
    if (n_layers <= 8) {
        return true;
    }
    const int last     = n_layers - 1;
    auto      round_he = [](double x) -> int {
        return static_cast<int>(std::nearbyint(x));
    };
    if (i == 0) {
        return true;
    }
    if (i == last) {
        return true;
    }
    if (i == round_he(last / 4.0)) {
        return true;
    }
    if (i == round_he(last / 2.0)) {
        return true;
    }
    if (i == round_he(3.0 * last / 4.0)) {
        return true;
    }
    return false;
}

int encoder_t_enc(const MoonshineStreamingHParams & hp, int n_samples) {
    if (n_samples <= 0 || hp.enc_frame_len <= 0) {
        return 0;
    }
    const int T_frames = n_samples / hp.enc_frame_len;
    if (T_frames <= 0) {
        return 0;
    }
    // Two strided causal conv layers with stride 2 each.
    const int T1    = causal_conv1d_t_out(T_frames, /*k=*/5, /*s=*/2);
    const int T_enc = causal_conv1d_t_out(T1, /*k=*/5, /*s=*/2);
    return T_enc;
}

void build_sliding_window_mask(int T_enc, int left_window, int right_window, float * out_mask) {
    constexpr float NEG_INF = -std::numeric_limits<float>::infinity();
    // mask[q, k]: q is row (n_q axis), k is col (n_kv axis = innermost).
    // ggml mask layout convention: ne0 = n_kv, ne1 = n_q. Row-major
    // memory has q as outer index, k as inner.
    for (int q = 0; q < T_enc; ++q) {
        for (int k = 0; k < T_enc; ++k) {
            const int  dist                              = q - k;
            const bool left_ok                           = (dist >= 0) && (dist < left_window);
            const bool right_ok                          = (dist < 0) && (-dist < right_window);
            out_mask[static_cast<size_t>(q) * T_enc + k] = (left_ok || right_ok) ? 0.0f : NEG_INF;
        }
    }
}

EncoderBuild build_encoder_graph(ggml_context *                    ctx,
                                 const MoonshineStreamingWeights & w,
                                 const MoonshineStreamingHParams & hp,
                                 int                               n_samples,
                                 bool                              use_flash) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_samples <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming encoder: invalid arg (ctx=%p, n_samples=%d)",
                static_cast<void *>(ctx), n_samples);
        return eb;
    }

    const int frame_len = hp.enc_frame_len;
    const int hidden    = hp.enc_d_model;
    const int hidden2   = 2 * hidden;
    const int n_heads   = hp.enc_n_heads;
    const int T_enc     = encoder_t_enc(hp, n_samples);

    if (T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming encoder: input too short (n_samples=%d → T_enc=0)",
                n_samples);
        return eb;
    }

    // Audio must be a multiple of frame_len so the reshape is well-defined.
    // Reference processor right-pads to multiples of 80 with attention_mask
    // marking the pad slots; here we require the caller to pass an exact
    // multiple (callers right-pad before calling).
    if (n_samples % frame_len != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming encoder: n_samples=%d not a multiple of "
                "frame_len=%d (caller must right-pad before calling)",
                n_samples, frame_len);
        return eb;
    }
    const int T_frames = n_samples / frame_len;

    // ---- audio input ----
    eb.audio_in = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, n_samples);
    if (eb.audio_in == nullptr) {
        return eb;
    }
    named(eb.audio_in, "enc.audio.in");
    ggml_set_input(eb.audio_in);
    eb.dumps.audio_in = eb.audio_in;
    transcribe::debug::mark_tensor_for_dump(eb.audio_in);

    // ---- frame reshape: [n_samples] → [frame_len, T_frames] ----
    ggml_tensor * x = ggml_reshape_2d(ctx, eb.audio_in, frame_len, T_frames);

    // ---- CMVN: layer-norm-style, no affine ----
    x = ggml_norm(ctx, x, hp.cmvn_eps);
    named(x, "enc.embedder.cmvn.out");
    eb.dumps.cmvn_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- asinh compression ----
    ggml_tensor * exp_logk = ggml_exp(ctx, w.embedder.comp_log_k);  // [1]
    ggml_tensor * z        = ggml_mul(ctx, x, exp_logk);            // broadcast [1] over [80, T_frames]
    x                      = asinh_op(ctx, z);
    named(x, "enc.embedder.comp.out");
    eb.dumps.comp_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- linear (no bias) + SiLU ----
    x = ggml_mul_mat(ctx, w.embedder.linear_w, x);  // [hidden, T_frames]
    x = ggml_silu(ctx, x);
    named(x, "enc.embedder.linear.out");
    eb.dumps.linear_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- conv1: causal, stride 2, hidden → 2·hidden ----
    // Transpose [hidden, T_frames] → [T_frames, hidden] for conv_1d_f32.
    {
        x                = ggml_cont(ctx, ggml_transpose(ctx, x));
        // → ne=[T_frames, hidden]
        x                = ggml_reshape_3d(ctx, x, T_frames, hidden, 1);
        // Left-pad ne[0] by k-1=4 (causal).
        x                = ggml_pad_ext(ctx, x,
                                        /*lp0=*/4, /*rp0=*/0,
                                        /*lp1=*/0, /*rp1=*/0,
                                        /*lp2=*/0, /*rp2=*/0,
                                        /*lp3=*/0, /*rp3=*/0);
        x                = conf::conv_1d_f32(ctx, w.embedder.conv1_w, x,
                                             /*stride=*/2, /*padding=*/0, /*dilation=*/1);
        // x ne=[T1, hidden2, 1]
        x                = add_conv1d_bias(ctx, x, w.embedder.conv1_b);
        x                = ggml_silu(ctx, x);
        // Reshape to 2-D [T1, hidden2] then transpose to [hidden2, T1] for dump.
        const int64_t T1 = x->ne[0];
        x                = ggml_reshape_2d(ctx, x, T1, hidden2);
        x                = ggml_cont(ctx, ggml_transpose(ctx, x));
    }
    named(x, "enc.embedder.conv1.out");
    eb.dumps.conv1_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- conv2: causal, stride 2, 2·hidden → hidden (NO SiLU) ----
    {
        // x ne=[hidden2, T1]. Transpose to [T1, hidden2].
        x                          = ggml_cont(ctx, ggml_transpose(ctx, x));
        const int64_t T1           = x->ne[0];
        x                          = ggml_reshape_3d(ctx, x, T1, hidden2, 1);
        x                          = ggml_pad_ext(ctx, x,
                                                  /*lp0=*/4, /*rp0=*/0,
                                                  /*lp1=*/0, /*rp1=*/0,
                                                  /*lp2=*/0, /*rp2=*/0,
                                                  /*lp3=*/0, /*rp3=*/0);
        x                          = conf::conv_1d_f32(ctx, w.embedder.conv2_w, x,
                                                       /*stride=*/2, /*padding=*/0, /*dilation=*/1);
        // x ne=[T_enc, hidden, 1]
        x                          = add_conv1d_bias(ctx, x, w.embedder.conv2_b);
        // NO SiLU after conv2.
        const int64_t T_enc_actual = x->ne[0];
        x                          = ggml_reshape_2d(ctx, x, T_enc_actual, hidden);
        x                          = ggml_cont(ctx, ggml_transpose(ctx, x));
        // x ne=[hidden, T_enc]
    }
    named(x, "enc.embedder.conv2.out");
    eb.dumps.conv2_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- per-layer sliding-window masks (input tensors) ----
    eb.per_layer_masks.assign(hp.enc_n_layers, nullptr);
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        ggml_tensor * mask = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, T_enc, T_enc);
        char          mname[64];
        std::snprintf(mname, sizeof(mname), "enc.swa_mask.%d", i);
        named(mask, mname);
        ggml_set_input(mask);
        eb.per_layer_masks[i] = mask;
    }

    // ---- transformer blocks ----
    eb.dumps.block_outs.reserve(static_cast<size_t>(hp.enc_n_layers));
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        x = build_encoder_block(ctx, x, eb.per_layer_masks[i], w.enc_blocks[i], hp, n_heads, hp.enc_head_dim,
                                use_flash);

        if (dump_block_index(i, hp.enc_n_layers)) {
            char bname[64];
            std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
            named(x, bname);
            transcribe::debug::mark_tensor_for_dump(x);
        }
        eb.dumps.block_outs.push_back(x);
    }

    // ---- final LN (no bias; scale already includes +1.0) ----
    x = layer_norm(ctx, x, w.enc_top.final_norm_w, /*beta=*/nullptr);
    named(x, "enc.final");
    eb.dumps.final_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    eb.out   = x;
    eb.T_enc = T_enc;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    (void) kLayerNormEps;
    return eb;
}

}  // namespace transcribe::moonshine_streaming
