// arch/voxtral_realtime/encoder.cpp - causal RoPE encoder + projector graph.
//
// Reference: VoxtralRealtimeEncoder + VoxtralRealtimeMultiModalProjector.
//
//   mel [n_mels=128, T_mel]
//     -> transpose to [T_mel, n_mels]
//     -> LEFT-pad(2) + conv1 (k3 s1 p0) + bias + GELU      [T_mel, d_model]
//     -> LEFT-pad(1) + conv2 (k3 s2 p0) + bias + GELU      [T_enc, d_model]
//     -> transpose to [d_model, T_enc]                     -> enc.embedder.out
//     -> 32x pre-norm RMSNorm block (NEOX RoPE, sw-causal mask, SwiGLU)
//     -> final RMSNorm                                     -> enc.out
//   projector:
//     reshape [d_model,T_enc] -> [5120, n_audio] (group 4 frames, C-order)
//     -> Linear(5120->H) -> GELU -> Linear(H->H)           -> proj.out
//
// Attention is full MHA (n_kv_heads == n_heads): q/v/out carry bias, k does
// NOT. q-scale (head_dim**-0.5) is folded into the softmax.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>

namespace transcribe::voxtral_realtime {

namespace {

namespace conf = transcribe::conformer;
using conf::conv_1d_f32;
using conf::named;

ggml_tensor * rms_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * w, float eps) {
    return ggml_mul(ctx, ggml_rms_norm(ctx, x, eps), w);
}

// Reshape a 1D conv bias [Cout] into [1, Cout, 1, 1] so it broadcasts across T.
ggml_tensor * add_conv1d_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d) {
    if (bias_1d == nullptr) {
        return conv_out;
    }
    ggml_tensor * b4 = ggml_reshape_4d(ctx, bias_1d, 1, bias_1d->ne[0], 1, 1);
    return ggml_add(ctx, conv_out, b4);
}

// Left-pad the time axis (ne[0]) by `lp` zeros, then conv with padding=0:
// causal Conv1d (output position t sees only inputs <= t).
ggml_tensor * causal_conv(ggml_context * ctx, ggml_tensor * kernel, ggml_tensor * x, int stride, int left_pad) {
    ggml_tensor * xp = ggml_pad_ext(ctx, x, left_pad, 0, 0, 0, 0, 0, 0, 0);
    return conv_1d_f32(ctx, kernel, xp, stride, /*padding=*/0, /*dilation=*/1);
}

// Full-MHA self-attention with NEOX RoPE + host-prepared sliding-window-causal
// mask. x:[d_model, T]; q/v/out bias, k none.
ggml_tensor * mha(ggml_context *   ctx,
                  ggml_tensor *    x,
                  const EncBlock & b,
                  ggml_tensor *    positions,
                  ggml_tensor *    mask,
                  int              n_heads,
                  int              head_dim,
                  int              d_model,
                  float            rope_theta,
                  int              max_pos,
                  bool             use_flash) {
    const int64_t T     = x->ne[1];
    const int64_t q_dim = static_cast<int64_t>(n_heads) * head_dim;
    const float   scale = 1.0f / std::sqrt(static_cast<float>(head_dim));

    ggml_tensor * Q = ggml_mul_mat(ctx, b.attn_q_w, x);
    Q               = ggml_add(ctx, Q, b.attn_q_b);
    ggml_tensor * K = ggml_mul_mat(ctx, b.attn_k_w, x);  // no bias
    ggml_tensor * V = ggml_mul_mat(ctx, b.attn_v_w, x);
    V               = ggml_add(ctx, V, b.attn_v_b);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_heads, T, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_heads, T, 1);

    Q = ggml_rope_ext(ctx, Q, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);

    // [D, H, T, 1] -> [D, T, H, 1]
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));
    ggml_tensor * K_att = ggml_cont(ctx, ggml_permute(ctx, K, 0, 2, 1, 3));
    ggml_tensor * V_att = ggml_cont(ctx, ggml_permute(ctx, V, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, T);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_att, Q_att);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * V_t     = ggml_cont(ctx, ggml_permute(ctx, V_att, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, V_t, kq_soft);
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
        o                     = ggml_reshape_2d(ctx, o, q_dim, T);
    }

    o = ggml_mul_mat(ctx, b.attn_out_w, o);
    o = ggml_add(ctx, o, b.attn_out_b);
    return o;
}

// Batched full-MHA self-attention: x:[d_model, T, B]. `positions [T]` and the
// sw-causal `mask [T, T]` are SHARED across the batch (the encoder is causal, so
// right-pad rows are numerically isolated and need no per-row mask). Mirrors
// `mha` with the batch riding ne[2] of x (ne[3] of the per-head tensors).
ggml_tensor * mha_batched(ggml_context *   ctx,
                          ggml_tensor *    x,
                          const EncBlock & b,
                          ggml_tensor *    positions,
                          ggml_tensor *    mask,
                          int              n_heads,
                          int              head_dim,
                          int /*d_model*/,
                          float rope_theta,
                          int   max_pos,
                          bool  use_flash) {
    const int64_t T     = x->ne[1];
    const int64_t B     = x->ne[2];
    const int64_t q_dim = static_cast<int64_t>(n_heads) * head_dim;
    const float   scale = 1.0f / std::sqrt(static_cast<float>(head_dim));

    ggml_tensor * Q = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_q_w, x), b.attn_q_b);
    ggml_tensor * K = ggml_mul_mat(ctx, b.attn_k_w, x);  // no bias
    ggml_tensor * V = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_v_w, x), b.attn_v_b);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T, B);
    K = ggml_reshape_4d(ctx, K, head_dim, n_heads, T, B);
    V = ggml_reshape_4d(ctx, V, head_dim, n_heads, T, B);

    // positions[T] indexes ne[2]=T; the batch rides ne[3]=B (shared positions).
    Q = ggml_rope_ext(ctx, Q, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);

    // [D, H, T, B] -> [D, T, H, B]
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));
    ggml_tensor * K_att = ggml_cont(ctx, ggml_permute(ctx, K, 0, 2, 1, 3));
    ggml_tensor * V_att = ggml_cont(ctx, ggml_permute(ctx, V, 0, 2, 1, 3));

    // Both paths broadcast the shared [T,T] mask over heads (ne[2]) and batch
    // (ne[3]); the non-flash path is the CPU source-of-truth default.
    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_3d(ctx, o, q_dim, T, B);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_att, Q_att);                       // [T, T, H, B]
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * V_t     = ggml_cont(ctx, ggml_permute(ctx, V_att, 1, 0, 2, 3));  // [T, D, H, B]
        o                     = ggml_mul_mat(ctx, V_t, kq_soft);                       // [D, T, H, B]
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));      // [D, H, T, B]
        o                     = ggml_reshape_3d(ctx, o, q_dim, T, B);
    }

    o = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_out_w, o), b.attn_out_b);
    return o;
}

// SwiGLU MLP: down(silu(gate(x)) * up(x)) + down_bias. gate/up no bias.
ggml_tensor * mlp(ggml_context * ctx, ggml_tensor * x, const EncBlock & b) {
    ggml_tensor * g = ggml_mul_mat(ctx, b.ffn_gate_w, x);
    ggml_tensor * u = ggml_mul_mat(ctx, b.ffn_up_w, x);
    ggml_tensor * h = ggml_mul(ctx, ggml_silu(ctx, g), u);
    ggml_tensor * o = ggml_mul_mat(ctx, b.ffn_down_w, h);
    o               = ggml_add(ctx, o, b.ffn_down_b);
    return o;
}

// Incremental MHA against the encoder KV cache ring. `x`:[d_model, n_new] are
// the new query frames. Writes their K/V at cache rows [write_slot, write_slot +
// n_new), then attends Q[n_new] against the [read_start, read_start + read_len)
// window under the host sliding-window-causal `mask` [read_len, n_new] (built
// from ABSOLUTE positions by the caller). MHA (n_kv_heads==n_heads), so no GQA
// repeat. q/v/out carry bias; k none. NEOX RoPE at absolute positions.
ggml_tensor * mha_cached(ggml_context *                   ctx,
                         ggml_cgraph *                    gf,
                         ggml_tensor *                    x,
                         const EncBlock &                 b,
                         ggml_tensor *                    positions,
                         ggml_tensor *                    mask,
                         transcribe::causal_lm::KvCache & kv,
                         int                              layer,
                         int                              n_new,
                         int                              write_slot,
                         int                              read_start,
                         int                              read_len,
                         int                              n_heads,
                         int                              head_dim,
                         float                            rope_theta,
                         int                              max_pos,
                         bool                             use_flash) {
    const int64_t q_dim  = static_cast<int64_t>(n_heads) * head_dim;
    const int64_t kv_dim = static_cast<int64_t>(n_heads) * head_dim;  // MHA
    const int     n_ctx  = kv.n_ctx;
    const float   scale  = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t  k_elem = ggml_element_size(kv.self_k);
    const size_t  v_elem = ggml_element_size(kv.self_v);

    ggml_tensor * Q = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_q_w, x), b.attn_q_b);
    ggml_tensor * K = ggml_mul_mat(ctx, b.attn_k_w, x);  // no bias
    ggml_tensor * V = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_v_w, x), b.attn_v_b);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, n_new, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_heads, n_new, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_heads, n_new, 1);

    Q = ggml_rope_ext(ctx, Q, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, head_dim, GGML_ROPE_TYPE_NEOX, max_pos, rope_theta, 1.0f, 0.0f, 1.0f,
                      32.0f, 1.0f);

    // Write K/V into the cache ring at rows [write_slot, write_slot + n_new). K/V
    // memory order (fastest->slowest) is D,H,T == position-major within the slab.
    const size_t write_off = (static_cast<size_t>(layer) * n_ctx + static_cast<size_t>(write_slot)) * kv_dim;
    // The cache read views below are graph LEAVES (views of self_k/self_v), so
    // the cpy writes are not their ancestors — root them in the graph directly.
    {
        const size_t  n_elem = static_cast<size_t>(n_new) * kv_dim;
        ggml_tensor * k_dst  = ggml_view_1d(ctx, kv.self_k, n_elem, k_elem * write_off);
        ggml_tensor * v_dst  = ggml_view_1d(ctx, kv.self_v, n_elem, v_elem * write_off);
        ggml_build_forward_expand(gf, ggml_cpy(ctx, K, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, V, v_dst));
    }

    // Read the [read_start, read_start + read_len) window for this layer.
    const size_t  read_off = (static_cast<size_t>(layer) * n_ctx + static_cast<size_t>(read_start)) * kv_dim;
    ggml_tensor * K_att = ggml_view_3d(ctx, kv.self_k, head_dim, read_len, n_heads, k_elem * kv_dim, k_elem * head_dim,
                                       k_elem * read_off);
    ggml_tensor * V_att = ggml_view_3d(ctx, kv.self_v, head_dim, read_len, n_heads, v_elem * kv_dim, v_elem * head_dim,
                                       v_elem * read_off);

    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));  // [D, n_new, H]

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, n_new);
    } else {
        ggml_tensor * K_att_c = ggml_cont(ctx, K_att);
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_att_c, Q_att);                     // [n_kv, n_new, H]
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * V_t     = ggml_cont(ctx, ggml_permute(ctx, V_att, 1, 0, 2, 3));  // [n_kv, D, H]
        o                     = ggml_mul_mat(ctx, V_t, kq_soft);                       // [D, n_new, H]
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));      // [D, H, n_new]
        o                     = ggml_reshape_2d(ctx, o, q_dim, n_new);
    }

    o = ggml_add(ctx, ggml_mul_mat(ctx, b.attn_out_w, o), b.attn_out_b);
    return o;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *  ctx,
                                 const Weights & w,
                                 const HParams & hp,
                                 int             n_mel_frames,
                                 bool            use_flash) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 4) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime encoder: invalid n_mel_frames=%d", n_mel_frames);
        return eb;
    }

    const int d_model  = hp.enc_d_model;
    const int n_mels   = hp.enc_num_mel_bins;
    const int n_heads  = hp.enc_n_heads;
    const int head_dim = hp.enc_head_dim;
    // conv1 (k3 s1 causal): out == n_mel_frames; conv2 (k3 s2 causal lp1):
    //   T_enc = floor((n_mel_frames - 2) / 2) + 1
    const int T_enc    = (n_mel_frames - 2) / 2 + 1;
    const int n_audio  = T_enc / hp.proj_downsample;
    if (n_audio <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime encoder: T_enc=%d too small", T_enc);
        return eb;
    }
    eb.T_enc   = T_enc;
    eb.n_audio = n_audio;

    // ---- inputs ----
    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames);
    named(eb.mel_in, "enc.mel.in");
    ggml_set_input(eb.mel_in);

    eb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    named(eb.positions_in, "enc.positions");
    ggml_set_input(eb.positions_in);

    eb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_enc, T_enc);
    named(eb.mask_in, "enc.attn_mask");
    ggml_set_input(eb.mask_in);

    // ---- causal conv stem ----
    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));  // [T_mel, n_mels]
    x               = causal_conv(ctx, w.enc_stem.conv0_w, x, /*stride=*/1, /*left_pad=*/2);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x               = ggml_gelu_erf(ctx, x);  // [T_mel, d_model]
    x               = causal_conv(ctx, w.enc_stem.conv1_w, x, /*stride=*/2, /*left_pad=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x               = ggml_gelu_erf(ctx, x);                   // [T_enc, d_model]
    x               = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, T_enc]
    named(x, "enc.embedder.out");
    eb.dumps.embedder_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- transformer blocks ----
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    eb.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        const EncBlock & b = w.enc_blocks[i];
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_attn_w, hp.enc_rms_norm_eps);
            y               = mha(ctx, y, b, eb.positions_in, eb.mask_in, n_heads, head_dim, d_model, hp.enc_rope_theta,
                                  hp.enc_max_pos, use_flash);
            x               = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_ffn_w, hp.enc_rms_norm_eps);
            y               = mlp(ctx, y, b);
            x               = ggml_add(ctx, x, y);
        }
        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
        named(x, bname);
        transcribe::debug::mark_tensor_for_dump(x);
        eb.dumps.block_outs.push_back(x);
    }

    // ---- final RMSNorm -> enc.out ----
    x = rms_norm(ctx, x, w.enc_stem.final_norm_w, hp.enc_rms_norm_eps);
    named(x, "enc.out");
    eb.dumps.enc_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- projector: group 4 frames -> [proj_in, n_audio] ----
    // Truncate to n_audio*4 frames (drop the trailing T_enc%4) before reshape.
    ggml_tensor * xc = ggml_cont(ctx, x);  // [d_model, T_enc] contiguous
    if (T_enc != n_audio * hp.proj_downsample) {
        xc = ggml_view_2d(ctx, xc, d_model, static_cast<int64_t>(n_audio) * hp.proj_downsample,
                          ggml_element_size(xc) * d_model, 0);
        xc = ggml_cont(ctx, xc);
    }
    ggml_tensor * grouped = ggml_reshape_2d(ctx, xc, hp.proj_in, n_audio);
    ggml_tensor * p       = ggml_mul_mat(ctx, w.proj.linear_1_w, grouped);
    p                     = ggml_gelu_erf(ctx, p);
    p                     = ggml_mul_mat(ctx, w.proj.linear_2_w, p);
    named(p, "proj.out");
    eb.dumps.proj_out = p;
    transcribe::debug::mark_tensor_for_dump(p);

    eb.out = p;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    if (eb.dumps.embedder_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.embedder_out);
    }
    if (eb.dumps.enc_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.enc_out);
    }
    for (ggml_tensor * t : eb.dumps.block_outs) {
        ggml_build_forward_expand(eb.graph, t);
    }

    return eb;
}

EncoderBuildBatched build_encoder_graph_batched(ggml_context *  ctx,
                                                const Weights & w,
                                                const HParams & hp,
                                                int             n_mel_frames,
                                                int             n_batch,
                                                bool            use_flash) {
    EncoderBuildBatched eb{};

    if (ctx == nullptr || n_mel_frames <= 4 || n_batch <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "voxtral_realtime encoder(batched): invalid arg "
                "(n_mel_frames=%d, n_batch=%d)",
                n_mel_frames, n_batch);
        return eb;
    }

    const int d_model  = hp.enc_d_model;
    const int n_mels   = hp.enc_num_mel_bins;
    const int n_heads  = hp.enc_n_heads;
    const int head_dim = hp.enc_head_dim;
    const int T_enc    = (n_mel_frames - 2) / 2 + 1;
    const int n_audio  = T_enc / hp.proj_downsample;
    const int B        = n_batch;
    if (n_audio <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime encoder(batched): T_enc=%d too small", T_enc);
        return eb;
    }
    eb.T_enc   = T_enc;
    eb.n_audio = n_audio;
    eb.n_batch = B;

    // ---- inputs (positions + mask SHARED across the batch) ----
    eb.mel_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames, B);
    named(eb.mel_in, "enc.mel.in.batched");
    ggml_set_input(eb.mel_in);

    eb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    named(eb.positions_in, "enc.positions.batched");
    ggml_set_input(eb.positions_in);

    eb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_enc, T_enc);
    named(eb.mask_in, "enc.attn_mask.batched");
    ggml_set_input(eb.mask_in);

    // ---- causal conv stem (batch rides ne[2]; conv_1d_f32 N>1 path) ----
    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));  // [T_mel, n_mels, B]
    x               = causal_conv(ctx, w.enc_stem.conv0_w, x, /*stride=*/1, /*left_pad=*/2);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x               = ggml_gelu_erf(ctx, x);  // [T_mel, d_model, B]
    x               = causal_conv(ctx, w.enc_stem.conv1_w, x, /*stride=*/2, /*left_pad=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x               = ggml_gelu_erf(ctx, x);                   // [T_enc, d_model, B]
    x               = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, T_enc, B]

    // ---- transformer blocks ----
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const EncBlock & b = w.enc_blocks[i];
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_attn_w, hp.enc_rms_norm_eps);
            y = mha_batched(ctx, y, b, eb.positions_in, eb.mask_in, n_heads, head_dim, d_model, hp.enc_rope_theta,
                            hp.enc_max_pos, use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_ffn_w, hp.enc_rms_norm_eps);
            y               = mlp(ctx, y, b);
            x               = ggml_add(ctx, x, y);
        }
    }

    // ---- final RMSNorm ----
    x = rms_norm(ctx, x, w.enc_stem.final_norm_w, hp.enc_rms_norm_eps);  // [d_model, T_enc, B]

    // ---- projector: group 4 frames per row -> [proj_in, n_audio, B] ----
    // Drop the trailing T_enc%4 frames of each row before the group-of-4 reshape.
    ggml_tensor * xc = ggml_cont(ctx, x);
    if (T_enc != n_audio * hp.proj_downsample) {
        xc = ggml_view_3d(ctx, xc, d_model, static_cast<int64_t>(n_audio) * hp.proj_downsample, B, xc->nb[1], xc->nb[2],
                          0);
        xc = ggml_cont(ctx, xc);
    }
    ggml_tensor * grouped = ggml_reshape_3d(ctx, xc, hp.proj_in, n_audio, B);
    ggml_tensor * p       = ggml_mul_mat(ctx, w.proj.linear_1_w, grouped);  // [H, n_audio, B]
    p                     = ggml_gelu_erf(ctx, p);
    p                     = ggml_mul_mat(ctx, w.proj.linear_2_w, p);        // [dec_h, n_audio, B]
    named(p, "proj.out.batched");

    eb.out = p;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime encoder(batched): ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    return eb;
}

EmbedderBuild build_embedder_graph(ggml_context * ctx, const Weights & w, const HParams & hp, int n_mel_frames) {
    EmbedderBuild eb{};
    if (ctx == nullptr || n_mel_frames <= 4) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime embedder: invalid n_mel_frames=%d", n_mel_frames);
        return eb;
    }
    const int n_mels = hp.enc_num_mel_bins;
    eb.T_enc         = (n_mel_frames - 2) / 2 + 1;

    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames);
    named(eb.mel_in, "enc.mel.in");
    ggml_set_input(eb.mel_in);

    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));  // [T_mel, n_mels]
    x               = causal_conv(ctx, w.enc_stem.conv0_w, x, /*stride=*/1, /*left_pad=*/2);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x               = ggml_gelu_erf(ctx, x);
    x               = causal_conv(ctx, w.enc_stem.conv1_w, x, /*stride=*/2, /*left_pad=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x               = ggml_gelu_erf(ctx, x);
    x               = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, T_enc]
    named(x, "enc.embedder.out");
    eb.out = x;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/2048, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime embedder: graph alloc failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    return eb;
}

EmbedderChunkBuild build_embedder_chunk_graph(ggml_context *  ctx,
                                              const Weights & w,
                                              const HParams & hp,
                                              int             n_new_mel) {
    EmbedderChunkBuild eb{};
    if (ctx == nullptr || n_new_mel < 2 || (n_new_mel % 2) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime embedder chunk: invalid n_new_mel=%d", n_new_mel);
        return eb;
    }
    const int n_mels  = hp.enc_num_mel_bins;
    const int d_model = hp.enc_d_model;
    eb.M_emb          = n_new_mel / 2;

    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, n_new_mel);
    named(eb.mel_in, "enc.mel.chunk.in");
    ggml_set_input(eb.mel_in);
    eb.cache1_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, 2);  // conv0 left_pad
    named(eb.cache1_in, "enc.conv0.cache");
    ggml_set_input(eb.cache1_in);
    eb.cache2_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, 1);  // conv1 left_pad
    named(eb.cache2_in, "enc.conv1.cache");
    ggml_set_input(eb.cache2_in);

    // conv0 (k3 s1, left_pad 2): prepend the 2 cached mel frames instead of zeros.
    // mel layout [n_mels, frames]; concat along frames (ne[1]), then transpose to
    // time-major [frames, n_mels] for conv_1d_f32 (matches build_embedder_graph).
    ggml_tensor * mel_full = ggml_concat(ctx, eb.cache1_in, eb.mel_in, /*dim=*/1);             // [n_mels, 2+M]
    ggml_tensor * x        = ggml_cont(ctx, ggml_transpose(ctx, mel_full));                    // [2+M, n_mels]
    x = conv_1d_f32(ctx, w.enc_stem.conv0_w, x, /*stride=*/1, /*padding=*/0, /*dilation=*/1);  // [M, d_model]
    x = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x = ggml_gelu_erf(ctx, x);
    ggml_tensor * conv0_out = x;  // [M, d_model] time-major

    // Next conv1 cache = the chunk's last conv0-output frame (post-GELU), as [d_model,1].
    ggml_tensor * c0_last = ggml_view_2d(ctx, conv0_out, 1, d_model, conv0_out->nb[1],
                                         static_cast<size_t>(n_new_mel - 1) * conv0_out->nb[0]);  // [1, d_model]
    eb.cache2_out         = ggml_cont(ctx, ggml_transpose(ctx, c0_last));                         // [d_model, 1]
    named(eb.cache2_out, "enc.conv1.cache.next");
    ggml_set_output(eb.cache2_out);

    // conv1 (k3 s2, left_pad 1): prepend the 1 cached conv0-output frame.
    ggml_tensor * cache2_tm  = ggml_cont(ctx, ggml_transpose(ctx, eb.cache2_in));  // [1, d_model]
    ggml_tensor * conv0_full = ggml_concat(ctx, cache2_tm, conv0_out, /*dim=*/0);  // [1+M, d_model]
    x                        = conv_1d_f32(ctx, w.enc_stem.conv1_w, conv0_full, /*stride=*/2, /*padding=*/0,
                                           /*dilation=*/1);                        // [M/2, d_model]
    x                        = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x                        = ggml_gelu_erf(ctx, x);
    eb.out                   = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, M_emb]
    named(eb.out, "enc.embedder.chunk.out");
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/2048, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime embedder chunk: graph alloc failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    ggml_build_forward_expand(eb.graph, eb.cache2_out);
    return eb;
}

EncoderChunkBuild build_encoder_chunk_graph(ggml_context *                   ctx,
                                            const Weights &                  w,
                                            const HParams &                  hp,
                                            transcribe::causal_lm::KvCache & enc_kv,
                                            int                              n_new,
                                            int                              write_slot,
                                            int                              read_start,
                                            int                              read_len,
                                            bool                             use_flash) {
    EncoderChunkBuild cb{};
    const int         down = hp.proj_downsample;
    if (ctx == nullptr || n_new <= 0 || (n_new % down) != 0 || write_slot < 0 || read_start < 0 || read_len <= 0 ||
        read_start + read_len > enc_kv.n_ctx || write_slot + n_new > enc_kv.n_ctx || enc_kv.self_k == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "voxtral_realtime enc-chunk: invalid arg (n_new=%d write_slot=%d "
                "read_start=%d read_len=%d n_ctx=%d)",
                n_new, write_slot, read_start, read_len, enc_kv.n_ctx);
        return cb;
    }
    const int d_model  = hp.enc_d_model;
    const int n_heads  = hp.enc_n_heads;
    const int head_dim = hp.enc_head_dim;
    const int n_audio  = n_new / down;
    cb.n_new           = n_new;
    cb.read_len        = read_len;
    cb.n_audio_new     = n_audio;

    cb.embed_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, n_new);
    named(cb.embed_in, "enc.chunk.embed_in");
    ggml_set_input(cb.embed_in);
    cb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_new);
    named(cb.positions_in, "enc.chunk.positions");
    ggml_set_input(cb.positions_in);
    cb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, read_len, n_new);
    named(cb.mask_in, "enc.chunk.mask");
    ggml_set_input(cb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime enc-chunk: graph alloc failed");
        return cb;
    }
    cb.graph = gf;

    ggml_tensor * x        = cb.embed_in;  // [d_model, n_new]
    const int     n_blocks = static_cast<int>(w.enc_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const EncBlock & b = w.enc_blocks[i];
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_attn_w, hp.enc_rms_norm_eps);
            y = mha_cached(ctx, gf, y, b, cb.positions_in, cb.mask_in, enc_kv, i, n_new, write_slot, read_start,
                           read_len, n_heads, head_dim, hp.enc_rope_theta, hp.enc_max_pos, use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = rms_norm(ctx, x, b.norm_ffn_w, hp.enc_rms_norm_eps);
            y               = mlp(ctx, y, b);
            x               = ggml_add(ctx, x, y);
        }
    }

    x = rms_norm(ctx, x, w.enc_stem.final_norm_w, hp.enc_rms_norm_eps);  // [d_model, n_new]
    named(x, "enc.chunk.enc_out");
    cb.enc_out = x;

    // Projector: group `down` consecutive frames -> [proj_in, n_audio].
    ggml_tensor * xc      = ggml_cont(ctx, x);
    ggml_tensor * grouped = ggml_reshape_2d(ctx, xc, hp.proj_in, n_audio);
    ggml_tensor * p       = ggml_mul_mat(ctx, w.proj.linear_1_w, grouped);
    p                     = ggml_gelu_erf(ctx, p);
    p                     = ggml_mul_mat(ctx, w.proj.linear_2_w, p);
    named(p, "enc.chunk.proj_out");
    cb.out = p;
    ggml_set_output(cb.out);

    ggml_build_forward_expand(gf, cb.out);
    ggml_build_forward_expand(gf, cb.enc_out);
    return cb;
}

}  // namespace transcribe::voxtral_realtime
