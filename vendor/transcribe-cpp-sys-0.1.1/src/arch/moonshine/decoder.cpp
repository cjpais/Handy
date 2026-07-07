// arch/moonshine/decoder.cpp - Moonshine decoder graph builders.
//
// Closest in-tree analog: arch/whisper/decoder.cpp (same encoder-decoder
// + cross-attn KV cache pattern). Moonshine differs in:
//
//   - Self-attn applies partial RoPE 0.9 to q/k (whisper has none).
//   - Cross-attn applies NO RoPE.
//   - All q/k/v/o projections are bias-less (whisper has bias on q/v/o).
//   - All LayerNorms are bias-less (whisper has bias on every LN).
//   - Decoder MLP is SwiGLU (whisper is plain GELU).
//   - HF MoonshineAttention pads head_dim to a multiple of 8 before the
//     matmul, slices the pad off after o_proj. The C++ port mirrors:
//     pad q/k/v on dim-0 (head_dim) to head_dim_padded, attend, then
//     view dim-0 back to head_dim before merging heads. Pad regions
//     never participate in RoPE (they sit past the rotated range).
//   - The K/V cache is stored UNPADDED (at d_model, not d_model_padded)
//     to match the natural layout of cohere/whisper caches and to
//     simplify graph construction. Padding is reapplied per-step on the
//     cache view before attention.

#include "decoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "moonshine.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdio>

namespace transcribe::moonshine {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

// Pad axis 0 (head_dim) of a [head_dim, ...] tensor to head_dim_padded.
// No-op when pad == 0.
ggml_tensor * pad_head_dim(ggml_context * ctx, ggml_tensor * t, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_pad(ctx, t, /*p0=*/pad, /*p1=*/0, /*p2=*/0, /*p3=*/0);
}

// View a [head_dim_padded, T, n_heads] tensor as [head_dim, T, n_heads]
// (slice off trailing head_dim_padding rows on dim 0). The result is
// non-contiguous; caller cont's it before downstream ops that need
// contiguity.
ggml_tensor * unpad_head_dim(ggml_context * ctx, ggml_tensor * t, int head_dim, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_view_3d(ctx, t, head_dim, t->ne[1], t->ne[2], t->nb[1], t->nb[2], 0);
}

// GPT-J / interleaved RoPE (rotate_half slices x[..., 0::2] / x[..., 1::2])
// = GGML_ROPE_TYPE_NORMAL, not NEOX. See encoder.cpp apply_partial_rope.
ggml_tensor * apply_partial_rope(ggml_context *           ctx,
                                 ggml_tensor *            x,
                                 ggml_tensor *            positions,
                                 const MoonshineHParams & hp,
                                 int                      head_dim_rot) {
    return ggml_rope_ext(ctx, x, positions, /*c=*/nullptr, head_dim_rot, GGML_ROPE_TYPE_NORMAL,
                         /*n_ctx_orig=*/0, hp.rope_theta,
                         /*freq_scale=*/1.0f,
                         /*ext_factor=*/0.0f,
                         /*attn_factor=*/1.0f,
                         /*beta_fast=*/32.0f,
                         /*beta_slow=*/1.0f);
}

// SwiGLU MLP: fc1 hidden->2·intermediate, split into [x_proj, gate],
// y = silu(gate) * x_proj, fc2 intermediate->hidden. HF chunk(2, dim=-1)
// (modeling_moonshine.py) is the innermost dim = ggml ne[0]: first
// ne[0]/2 is x_proj, second is gate.
ggml_tensor * ffn_decoder_swiglu(ggml_context * ctx,
                                 ggml_tensor *  x,
                                 ggml_tensor *  fc1_w,
                                 ggml_tensor *  fc1_b,
                                 ggml_tensor *  fc2_w,
                                 ggml_tensor *  fc2_b,
                                 int            ffn_dim) {
    ggml_tensor * h = ggml_mul_mat(ctx, fc1_w, x);
    if (fc1_b != nullptr) {
        h = ggml_add(ctx, h, fc1_b);
    }
    // h ne = [2·ffn_dim, T] (innermost is the channel dim).
    const int64_t T          = h->ne[1];
    const size_t  el         = ggml_element_size(h);
    const size_t  half_bytes = static_cast<size_t>(ffn_dim) * el;

    // First half: x_proj = h[..., :ffn_dim]  -> [ffn_dim, T]
    ggml_tensor * x_proj = ggml_view_2d(ctx, h, ffn_dim, T, h->nb[1], 0);
    // Second half: gate   = h[..., ffn_dim:]
    ggml_tensor * gate   = ggml_view_2d(ctx, h, ffn_dim, T, h->nb[1], half_bytes);

    ggml_tensor * y = ggml_mul(ctx, ggml_silu(ctx, ggml_cont(ctx, gate)), ggml_cont(ctx, x_proj));
    ggml_tensor * o = ggml_mul_mat(ctx, fc2_w, y);
    if (fc2_b != nullptr) {
        o = ggml_add(ctx, o, fc2_b);
    }
    return o;
}

// Self-attention reading/writing the self-attn KV cache.
//
// x:        [d_model, n_tokens] new tokens for this step
// pos_ids:  [n_tokens] i32 absolute positions (n_past + [0..n_tokens))
// mask:     [n_kv, n_tokens] f16 causal mask (n_kv = n_past + n_tokens),
//           or nullptr when n_tokens == 1 and no padding.
// Returns:  [d_model, n_tokens]
//
// Cache layout: per-layer slab of [d_model, n_ctx]. New K/V written to
// rows [n_past, n_past+n_tokens) at offset il*n_ctx*d_model + n_past*d_model
// (in elements). Read full [0..n_kv) for attention.
ggml_tensor * mha_self_cached(ggml_context *           ctx,
                              ggml_cgraph *            gf,
                              ggml_tensor *            x,
                              MoonshineKvCache &       kv_cache,
                              ggml_tensor *            pos_ids,
                              ggml_tensor *            mask,
                              ggml_tensor *            q_w,
                              ggml_tensor *            k_w,
                              ggml_tensor *            v_w,
                              ggml_tensor *            out_w,
                              const MoonshineHParams & hp,
                              int                      n_heads,
                              int                      d_model,
                              int                      il,
                              int                      n_past,
                              int                      n_tokens,
                              int                      n_kv,
                              bool                     use_flash) {
    const int   head_dim     = d_model / n_heads;
    const int   head_dim_pad = hp.dec_head_dim_padded();
    const int   head_dim_rot = hp.dec_head_dim_rot();
    const int   pad          = head_dim_pad - head_dim;
    const int   n_ctx        = kv_cache.n_ctx;
    // Unpadded scale: HF uses head_dim**-0.5; padded slots are
    // zero-extension and don't change the dot product.
    const float scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);

    // Reshape to [head_dim, n_heads, n_tokens, 1]. RoPE expects
    // positions to match ne[2] = n_tokens.
    Qcur = ggml_reshape_4d(ctx, Qcur, head_dim, n_heads, n_tokens, 1);
    Kcur = ggml_reshape_4d(ctx, Kcur, head_dim, n_heads, n_tokens, 1);
    Vcur = ggml_reshape_4d(ctx, Vcur, head_dim, n_heads, n_tokens, 1);

    // Partial RoPE on Q and K (V passes through).
    ggml_tensor * Q_rope = apply_partial_rope(ctx, Qcur, pos_ids, hp, head_dim_rot);
    ggml_tensor * K_rope = apply_partial_rope(ctx, Kcur, pos_ids, hp, head_dim_rot);

    // Q needs [head_dim, n_tokens, n_heads, 1] for attention. K/V are
    // already in [head_dim, n_heads, n_tokens] cache memory order after
    // reshape + (optional) RoPE, so they're written straight to the cache.
    ggml_tensor * Q_unpad = ggml_cont(ctx, ggml_permute(ctx, Q_rope, 0, 2, 1, 3));

    // Write K_rope / Vcur into the cache. The cache stores
    // [d_model, n_ctx] per layer; offset for (il, n_past) is
    //   il * n_ctx * d_model + n_past * d_model  (in elements)
    {
        const size_t k_elem = ggml_element_size(kv_cache.self_k);
        const size_t v_elem = ggml_element_size(kv_cache.self_v);

        ggml_tensor * k_dst = ggml_view_1d(ctx, kv_cache.self_k, static_cast<int64_t>(n_tokens) * d_model,
                                           k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model +
                                                                        static_cast<int64_t>(n_past) * d_model));
        ggml_tensor * v_dst = ggml_view_1d(ctx, kv_cache.self_v, static_cast<int64_t>(n_tokens) * d_model,
                                           v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model +
                                                                        static_cast<int64_t>(n_past) * d_model));

        ggml_build_forward_expand(gf, ggml_cpy(ctx, K_rope, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, Vcur, v_dst));
    }

    // Read [head_dim, n_kv, n_heads] views over the cache for attention.
    const size_t  k_elem = ggml_element_size(kv_cache.self_k);
    ggml_tensor * K      = ggml_view_3d(ctx, kv_cache.self_k, head_dim, n_kv, n_heads,
                                        k_elem * d_model,   // nb1: stride between positions (full d_model row)
                                        k_elem * head_dim,  // nb2: stride between heads inside a position
                                        k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.self_v);
    ggml_tensor * V = ggml_view_3d(ctx, kv_cache.self_v, head_dim, n_kv, n_heads, v_elem * d_model, v_elem * head_dim,
                                   v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    // Pad head_dim on Q, K, V before attention (HF behavior).
    ggml_tensor * Q   = pad_head_dim(ctx, Q_unpad, pad);
    ggml_tensor * K_p = pad_head_dim(ctx, ggml_cont(ctx, K), pad);
    ggml_tensor * V_p = pad_head_dim(ctx, ggml_cont(ctx, V), pad);

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K_p, V_p, mask, scale, 0.0f, 0.0f);
        // FA output: [head_dim_pad, n_heads, n_tokens, 1] — permute to
        // [head_dim_pad, n_tokens, n_heads, 1] for the slice path.
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_p, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V_p, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        // o ne: [head_dim_pad, n_tokens, n_heads, 1]
    }

    // Slice off head_dim padding before merging heads (HF order).
    if (pad > 0) {
        o = unpad_head_dim(ctx, o, head_dim, pad);
        o = ggml_cont(ctx, o);
    }

    // Merge heads: [head_dim, n_tokens, n_heads] -> [head_dim, n_heads, n_tokens]
    // -> cont -> [d_model, n_tokens].
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, d_model, n_tokens);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

// Self-attention for the static-topology step graph. KV cache writes
// go through ggml_set_rows at runtime row index `kv_idx`; reads span
// the full [0, max_n_kv) window with `mask` gating valid positions.
// n_tokens is implicitly 1.
//
// Partial RoPE: the new Q's RoPE is computed at runtime position
// `pos_ids` ([1] i32). The cache stores K already-rotated at its
// original position, so only the new K row needs rotation here.
ggml_tensor * mha_self_step(ggml_context *           ctx,
                            ggml_cgraph *            gf,
                            ggml_tensor *            x,
                            MoonshineKvCache &       kv_cache,
                            ggml_tensor *            pos_ids,
                            ggml_tensor *            kv_idx,
                            ggml_tensor *            mask,
                            ggml_tensor *            q_w,
                            ggml_tensor *            k_w,
                            ggml_tensor *            v_w,
                            ggml_tensor *            out_w,
                            const MoonshineHParams & hp,
                            int                      n_heads,
                            int                      d_model,
                            int                      il,
                            int                      max_n_kv,
                            bool                     use_flash) {
    const int   head_dim     = d_model / n_heads;
    const int   head_dim_pad = hp.dec_head_dim_padded();
    const int   head_dim_rot = hp.dec_head_dim_rot();
    const int   pad          = head_dim_pad - head_dim;
    const int   n_ctx        = kv_cache.n_ctx;
    const float scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);

    // Reshape to [head_dim, n_heads, 1, 1]. RoPE expects ne[2]==pos_count.
    Qcur = ggml_reshape_4d(ctx, Qcur, head_dim, n_heads, 1, 1);
    Kcur = ggml_reshape_4d(ctx, Kcur, head_dim, n_heads, 1, 1);

    ggml_tensor * Q_rope = apply_partial_rope(ctx, Qcur, pos_ids, hp, head_dim_rot);
    ggml_tensor * K_rope = apply_partial_rope(ctx, Kcur, pos_ids, hp, head_dim_rot);

    // Q for attention: [head_dim, 1, n_heads, 1]
    ggml_tensor * Q_unpad = ggml_cont(ctx, ggml_permute(ctx, Q_rope, 0, 2, 1, 3));

    // KV cache write via ggml_set_rows. Per-layer view spans [d_model, n_ctx];
    // we write one [d_model, 1] row at the kv_idx position. K_rope is the
    // post-RoPE K; V is the unrotated projection.
    {
        const size_t k_elem      = ggml_element_size(kv_cache.self_k);
        const size_t v_elem      = ggml_element_size(kv_cache.self_v);
        const size_t layer_off_k = k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model);
        const size_t layer_off_v = v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model);

        ggml_tensor * k_layer = ggml_view_2d(ctx, kv_cache.self_k, d_model, n_ctx, k_elem * d_model, layer_off_k);
        ggml_tensor * v_layer = ggml_view_2d(ctx, kv_cache.self_v, d_model, n_ctx, v_elem * d_model, layer_off_v);

        // K_rope is [head_dim, n_heads, 1, 1] contiguous → reshape to [d_model, 1].
        ggml_tensor * K_row = ggml_reshape_2d(ctx, ggml_cont(ctx, K_rope), d_model, 1);
        ggml_tensor * V_row = ggml_reshape_2d(ctx, Vcur, d_model, 1);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    // Static read across [0, max_n_kv); mask zeros valid positions and
    // -inf's the rest, so flash-attn ignores empty/future slots.
    const size_t  k_elem = ggml_element_size(kv_cache.self_k);
    ggml_tensor * K =
        ggml_view_3d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, k_elem * d_model, k_elem * head_dim,
                     k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.self_v);
    ggml_tensor * V =
        ggml_view_3d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, v_elem * d_model, v_elem * head_dim,
                     v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    // Pad head_dim on Q, K, V before attention (HF behavior).
    ggml_tensor * Q   = pad_head_dim(ctx, Q_unpad, pad);
    ggml_tensor * K_p = pad_head_dim(ctx, ggml_cont(ctx, K), pad);
    ggml_tensor * V_p = pad_head_dim(ctx, ggml_cont(ctx, V), pad);

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K_p, V_p, mask, scale, 0.0f, 0.0f);
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_p, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V_p, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
    }

    if (pad > 0) {
        o = unpad_head_dim(ctx, o, head_dim, pad);
        o = ggml_cont(ctx, o);
    }
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, d_model, 1);
    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

// Cross-attention reading the pre-populated cross KV cache. No RoPE on
// either side. Same head_dim padding policy as self-attn.
ggml_tensor * mha_cross_cached(ggml_context *           ctx,
                               ggml_tensor *            x,
                               MoonshineKvCache &       kv_cache,
                               ggml_tensor *            q_w,
                               ggml_tensor *            out_w,
                               const MoonshineHParams & hp,
                               int                      n_heads,
                               int                      d_model,
                               int                      il,
                               int                      T_enc,
                               bool                     use_flash) {
    const int     head_dim     = d_model / n_heads;
    const int     head_dim_pad = hp.dec_head_dim_padded();
    const int     pad          = head_dim_pad - head_dim;
    // Unpadded scale (see comment in mha_self_cached).
    const float   scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t n_tokens     = x->ne[1];

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * Q    = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q                  = ggml_permute(ctx, Q, 0, 2, 1, 3);
    Q                  = ggml_cont(ctx, Q);
    Q                  = pad_head_dim(ctx, Q, pad);

    const size_t  k_elem = ggml_element_size(kv_cache.cross_k);
    ggml_tensor * K = ggml_view_3d(ctx, kv_cache.cross_k, head_dim, T_enc, n_heads, k_elem * d_model, k_elem * head_dim,
                                   k_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.cross_v);
    ggml_tensor * V = ggml_view_3d(ctx, kv_cache.cross_v, head_dim, T_enc, n_heads, v_elem * d_model, v_elem * head_dim,
                                   v_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));

    ggml_tensor * K_p = pad_head_dim(ctx, ggml_cont(ctx, K), pad);
    ggml_tensor * V_p = pad_head_dim(ctx, ggml_cont(ctx, V), pad);

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K_p, V_p, /*mask=*/nullptr, scale, 0.0f, 0.0f);
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K_p, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V_p, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
    }

    if (pad > 0) {
        o = unpad_head_dim(ctx, o, head_dim, pad);
        o = ggml_cont(ctx, o);
    }
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, d_model, n_tokens);
    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

}  // namespace

// Cross-KV precompute graph.
DecoderBuild build_cross_kv_graph(ggml_context *           ctx,
                                  const MoonshineWeights & w,
                                  const MoonshineHParams & hp,
                                  MoonshineKvCache &       kv_cache,
                                  int                      T_enc) {
    DecoderBuild db{};

    if (ctx == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine cross_kv: invalid arg (ctx=%p, T_enc=%d)",
                static_cast<void *>(ctx), T_enc);
        return db;
    }

    const int d_model = hp.dec_d_model;

    // Encoder hidden state input: [d_model, T_enc] f32. Caller uploads
    // host-side post-encoder output via ggml_backend_tensor_set.
    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_enc);
    named(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine cross_kv: ggml_new_graph_custom failed");
        return db;
    }

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    for (int il = 0; il < n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        // K_cross = encoder_out · Wk_c, V_cross = encoder_out · Wv_c
        // (both bias-less). Output: [d_model, T_enc].
        ggml_tensor * Kcross = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        ggml_tensor * Vcross = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);

        const size_t k_elem = ggml_element_size(kv_cache.cross_k);
        const size_t v_elem = ggml_element_size(kv_cache.cross_v);

        ggml_tensor * k_dst = ggml_view_1d(ctx, kv_cache.cross_k, static_cast<int64_t>(T_enc) * d_model,
                                           k_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));
        ggml_tensor * v_dst = ggml_view_1d(ctx, kv_cache.cross_v, static_cast<int64_t>(T_enc) * d_model,
                                           v_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));

        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Kcross, k_dst));
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Vcross, v_dst));
    }

    return db;
}

// KV-cached decoder graph (prompt + step).
DecoderBuild build_decoder_graph_kv(ggml_context *           ctx,
                                    const MoonshineWeights & w,
                                    const MoonshineHParams & hp,
                                    MoonshineKvCache &       kv_cache,
                                    int                      n_tokens,
                                    int                      n_past,
                                    int                      T_enc,
                                    bool                     skip_log_softmax,
                                    bool                     use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || n_tokens <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine decoder_kv: invalid arg "
                "(ctx=%p, n_tokens=%d, T_enc=%d)",
                static_cast<void *>(ctx), n_tokens, T_enc);
        return db;
    }
    const int n_kv = n_past + n_tokens;
    if (n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine decoder_kv: n_kv=%d exceeds n_ctx=%d", n_kv, kv_cache.n_ctx);
        return db;
    }

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;

    // ---- Inputs ----
    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    db.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(db.pos_ids_in, "dec.pos_ids");
    ggml_set_input(db.pos_ids_in);

    // Self-attn causal mask. Required when n_tokens > 1 (multi-token
    // prompt pass needs the causal triangle). For n_tokens == 1 a
    // single token attends to its full live cache window with no
    // masking needed.
    ggml_tensor * causal_mask = nullptr;
    const bool    need_mask   = (n_tokens > 1);
    if (need_mask) {
        db.causal_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_kv, n_tokens);
        named(db.causal_mask_in, "dec.causal_mask");
        ggml_set_input(db.causal_mask_in);
        causal_mask = ggml_cast(ctx, db.causal_mask_in, GGML_TYPE_F16);
    }

    // ---- Token embedding ----
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, db.token_ids_in);
    named(tok_emb, "dec.token_emb");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(tok_emb);
        db.dumps.token_emb = tok_emb;
    }

    // Moonshine has no additive positional embedding; embed_sum is
    // identical to token_emb, still emitted under "embed_sum" for dump
    // parity with whisper / cohere.
    ggml_tensor * x = tok_emb;
    {
        ggml_tensor * embed_sum = ggml_scale(ctx, tok_emb, 1.0f);
        named(embed_sum, "dec.embed_sum");
        if (n_past == 0) {
            transcribe::debug::mark_tensor_for_dump(embed_sum);
            db.dumps.embed_sum = embed_sum;
        }
        x = embed_sum;
    }

    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine decoder_kv: ggml_new_graph_custom failed");
        return db;
    }

    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    db.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];

        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, /*beta=*/nullptr);
            y = mha_self_cached(ctx, db.graph, y, kv_cache, db.pos_ids_in, causal_mask, b.self_q_w, b.self_k_w,
                                b.self_v_w, b.self_out_w, hp, n_heads, d_model, i, n_past, n_tokens, n_kv, use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, /*beta=*/nullptr);
            y = mha_cross_cached(ctx, y, kv_cache, b.cross_q_w, b.cross_out_w, hp, n_heads, d_model, i, T_enc,
                                 use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, /*beta=*/nullptr);
            y = ffn_decoder_swiglu(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b, hp.dec_ffn_dim);
            x = ggml_add(ctx, x, y);
        }

        char bname[64];
        std::snprintf(bname, sizeof(bname), "dec.block.%d.out", i);
        named(x, bname);
        if (n_past == 0) {
            transcribe::debug::mark_tensor_for_dump(x);
            db.dumps.block_outs.push_back(x);
        }
    }

    // Final LN (no bias).
    x = layer_norm(ctx, x, w.dec_top.final_norm_w, /*beta=*/nullptr);
    named(x, "dec.out_before_head");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(x);
        db.dumps.out_before_head = x;
    }

    // Tied lm_head (reuses dec.token_embd.weight): W ne=[d_model, vocab],
    // x ne=[d_model, n_tokens] -> logits ne=[vocab, n_tokens].
    ggml_tensor * logits_raw = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);
    named(logits_raw, "dec.logits_raw");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(logits_raw);
        db.dumps.logits_raw = logits_raw;
    }

    ggml_tensor * logits = logits_raw;
    if (!skip_log_softmax) {
        logits = ggml_log(ctx, ggml_soft_max(ctx, logits_raw));
        named(logits, "dec.logits");
        if (n_past == 0) {
            transcribe::debug::mark_tensor_for_dump(logits);
            db.dumps.logits = logits;
        }
    }

    db.out = logits;
    ggml_set_output(db.out);

    // When skipping log_softmax (every step pass after the dump-emitting
    // prompt pass), append argmax over the last position so we download a
    // single int32 instead of the full vocab logits.
    if (skip_log_softmax) {
        ggml_tensor * last_logits = logits;
        if (n_tokens > 1) {
            const int64_t vocab     = logits->ne[0];
            const size_t  row_bytes = ggml_element_size(logits) * static_cast<size_t>(vocab);
            last_logits =
                ggml_view_2d(ctx, logits, vocab, /*n=*/1, row_bytes, row_bytes * static_cast<size_t>(n_tokens - 1));
            last_logits = ggml_cont(ctx, last_logits);
        }
        db.argmax_out = ggml_argmax(ctx, last_logits);
        ggml_set_name(db.argmax_out, "dec.argmax");
        ggml_set_output(db.argmax_out);
        ggml_build_forward_expand(db.graph, db.argmax_out);
    } else {
        ggml_build_forward_expand(db.graph, db.out);
    }

    return db;
}

// Static-topology single-token step graph (GPU dispatch path).
StepBuild build_step_graph(ggml_context *           ctx,
                           const MoonshineWeights & w,
                           const MoonshineHParams & hp,
                           MoonshineKvCache &       kv_cache,
                           int                      max_n_kv,
                           int                      T_enc,
                           bool                     use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine step: invalid arg (max_n_kv=%d, T_enc=%d)", max_n_kv, T_enc);
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;

    sb.token_id_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.token_id_in, "step.token_id");
    ggml_set_input(sb.token_id_in);

    sb.pos_id_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.pos_id_in, "step.pos_id");
    ggml_set_input(sb.pos_id_in);

    sb.kv_idx_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I64, 1);
    ggml_set_name(sb.kv_idx_in, "step.kv_idx");
    ggml_set_input(sb.kv_idx_in);

    sb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, max_n_kv, 1);
    ggml_set_name(sb.mask_in, "step.mask");
    ggml_set_input(sb.mask_in);

    sb.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (sb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine step: ggml_new_graph_custom failed");
        return sb;
    }

    // Token embedding (no additive positional embedding).
    ggml_tensor * x = ggml_get_rows(ctx, w.dec_top.token_embd_w, sb.token_id_in);

    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];

        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, /*beta=*/nullptr);
            y = mha_self_step(ctx, sb.graph, y, kv_cache, sb.pos_id_in, sb.kv_idx_in, sb.mask_in, b.self_q_w,
                              b.self_k_w, b.self_v_w, b.self_out_w, hp, n_heads, d_model, i, max_n_kv, use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, /*beta=*/nullptr);
            y = mha_cross_cached(ctx, y, kv_cache, b.cross_q_w, b.cross_out_w, hp, n_heads, d_model, i, T_enc,
                                 use_flash);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, /*beta=*/nullptr);
            y = ffn_decoder_swiglu(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b, hp.dec_ffn_dim);
            x = ggml_add(ctx, x, y);
        }
    }

    x = layer_norm(ctx, x, w.dec_top.final_norm_w, /*beta=*/nullptr);

    // Tied lm_head.
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);

    sb.argmax_out = ggml_argmax(ctx, logits);
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);

    return sb;
}

// Offline batched decode (B utterances).
namespace {

// Slice off head-dim padding on a 4D tensor [head_dim_pad, a, b, c] → keep B.
ggml_tensor * unpad_head_dim_4d(ggml_context * ctx, ggml_tensor * t, int head_dim, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_view_4d(ctx, t, head_dim, t->ne[1], t->ne[2], t->ne[3], t->nb[1], t->nb[2], t->nb[3], 0);
}

// Batched self-attention step (partial RoPE + head-dim pad). x: [d_model, B].
ggml_tensor * mha_self_step_batched(ggml_context *           ctx,
                                    ggml_cgraph *            gf,
                                    ggml_tensor *            x,
                                    MoonshineKvCache &       kv_cache,
                                    ggml_tensor *            pos_ids,
                                    ggml_tensor *            kv_idx,
                                    ggml_tensor *            mask,
                                    ggml_tensor *            q_w,
                                    ggml_tensor *            k_w,
                                    ggml_tensor *            v_w,
                                    ggml_tensor *            out_w,
                                    const MoonshineHParams & hp,
                                    int                      n_heads,
                                    int                      d_model,
                                    int                      il,
                                    int                      max_n_kv,
                                    int                      B) {
    const int     head_dim     = d_model / n_heads;
    const int     head_dim_pad = hp.dec_head_dim_padded();
    const int     head_dim_rot = hp.dec_head_dim_rot();
    const int     pad          = head_dim_pad - head_dim;
    const int64_t n_ctx        = kv_cache.n_ctx;
    const float   scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t  k_elem       = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem       = ggml_element_size(kv_cache.self_v);

    ggml_tensor * Q = ggml_mul_mat(ctx, q_w, x);  // [d_model, B]
    ggml_tensor * K = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * V = ggml_mul_mat(ctx, v_w, x);

    // Batch on ne[2] so RoPE rotates each utterance by its own position.
    Q = ggml_reshape_3d(ctx, Q, head_dim, n_heads, B);
    K = ggml_reshape_3d(ctx, K, head_dim, n_heads, B);
    V = ggml_reshape_3d(ctx, V, head_dim, n_heads, B);
    Q = apply_partial_rope(ctx, Q, pos_ids, hp, head_dim_rot);
    K = apply_partial_rope(ctx, K, pos_ids, hp, head_dim_rot);

    // Batched KV write: slab [d_model, n_ctx, B].
    {
        const size_t  off_k = k_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
        const size_t  off_v = v_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, d_model, n_ctx, B, k_elem * d_model, k_elem * d_model * n_ctx, off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, d_model, n_ctx, B, v_elem * d_model, v_elem * d_model * n_ctx, off_v);
        ggml_tensor * K_row = ggml_reshape_3d(ctx, ggml_cont(ctx, K), d_model, 1, B);
        ggml_tensor * V_row = ggml_reshape_3d(ctx, ggml_cont(ctx, V), d_model, 1, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    const size_t  off_k = k_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
    const size_t  off_v = v_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
    ggml_tensor * K_att = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, B, k_elem * d_model,
                                       k_elem * head_dim, k_elem * d_model * n_ctx, off_k);
    ggml_tensor * V_att = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, B, v_elem * d_model,
                                       v_elem * head_dim, v_elem * d_model * n_ctx, off_v);

    // Q for flash: [head_dim, n_heads, B] -> [head_dim, 1, n_heads, B].
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * Qp    = pad_head_dim(ctx, Q_att, pad);
    ggml_tensor * Kp    = pad_head_dim(ctx, ggml_cont(ctx, K_att), pad);
    ggml_tensor * Vp    = pad_head_dim(ctx, ggml_cont(ctx, V_att), pad);

    ggml_tensor * o = ggml_flash_attn_ext(ctx, Qp, Kp, Vp, mask, scale, 0.0f, 0.0f);
    // o = [head_dim_pad, n_heads, 1, B] -> unpad -> [head_dim, n_heads, 1, B].
    o               = ggml_cont(ctx, unpad_head_dim_4d(ctx, o, head_dim, pad));
    o               = ggml_reshape_2d(ctx, o, d_model, B);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

// Batched cross-attention step (no RoPE, head-dim pad, cross-pad mask).
ggml_tensor * mha_cross_step_batched(ggml_context *           ctx,
                                     ggml_tensor *            x,
                                     MoonshineKvCache &       kv_cache,
                                     ggml_tensor *            cross_mask,
                                     ggml_tensor *            q_w,
                                     ggml_tensor *            out_w,
                                     const MoonshineHParams & hp,
                                     int                      n_heads,
                                     int                      d_model,
                                     int                      il,
                                     int                      T_enc_max,
                                     int                      B) {
    const int    head_dim     = d_model / n_heads;
    const int    head_dim_pad = hp.dec_head_dim_padded();
    const int    pad          = head_dim_pad - head_dim;
    const float  scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t k_elem       = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem       = ggml_element_size(kv_cache.cross_v);

    ggml_tensor * Q     = ggml_mul_mat(ctx, q_w, x);  // [d_model, B]
    Q                   = ggml_reshape_3d(ctx, Q, head_dim, n_heads, B);
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * Qp    = pad_head_dim(ctx, Q_att, pad);

    const size_t  off_k = k_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
    const size_t  off_v = v_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
    ggml_tensor * K     = ggml_view_4d(ctx, kv_cache.cross_k, head_dim, T_enc_max, n_heads, B, k_elem * d_model,
                                       k_elem * head_dim, k_elem * d_model * T_enc_max, off_k);
    ggml_tensor * V     = ggml_view_4d(ctx, kv_cache.cross_v, head_dim, T_enc_max, n_heads, B, v_elem * d_model,
                                       v_elem * head_dim, v_elem * d_model * T_enc_max, off_v);
    ggml_tensor * Kp    = pad_head_dim(ctx, ggml_cont(ctx, K), pad);
    ggml_tensor * Vp    = pad_head_dim(ctx, ggml_cont(ctx, V), pad);

    ggml_tensor * o = ggml_flash_attn_ext(ctx, Qp, Kp, Vp, cross_mask, scale, 0.0f, 0.0f);
    o               = ggml_cont(ctx, unpad_head_dim_4d(ctx, o, head_dim, pad));
    o               = ggml_reshape_2d(ctx, o, d_model, B);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

}  // namespace

DecoderBuild build_cross_kv_graph_batched(ggml_context *           ctx,
                                          const MoonshineWeights & w,
                                          const MoonshineHParams & hp,
                                          MoonshineKvCache &       kv_cache,
                                          int                      T_enc_max,
                                          int                      n_batch) {
    DecoderBuild db{};
    if (ctx == nullptr || T_enc_max <= 0 || n_batch <= 0) {
        return db;
    }

    const int d_model = hp.dec_d_model;
    const int B       = n_batch;

    db.encoder_out_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, d_model, T_enc_max, B);
    named(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        return db;
    }

    const size_t k_elem = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem = ggml_element_size(kv_cache.cross_v);

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    for (int il = 0; il < n_layers; ++il) {
        const auto &  blk   = w.dec_blocks[il];
        ggml_tensor * Kc    = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        ggml_tensor * Vc    = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);
        // [d_model, T_enc_max, B].
        const size_t  off_k = k_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
        const size_t  off_v = v_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
        ggml_tensor * k_dst = ggml_view_3d(ctx, kv_cache.cross_k, d_model, T_enc_max, B, k_elem * d_model,
                                           k_elem * d_model * T_enc_max, off_k);
        ggml_tensor * v_dst = ggml_view_3d(ctx, kv_cache.cross_v, d_model, T_enc_max, B, v_elem * d_model,
                                           v_elem * d_model * T_enc_max, off_v);
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Kc, k_dst));
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Vc, v_dst));
    }
    return db;
}

StepBuildBatched build_step_graph_batched(ggml_context *           ctx,
                                          const MoonshineWeights & w,
                                          const MoonshineHParams & hp,
                                          MoonshineKvCache &       kv_cache,
                                          int                      max_n_kv,
                                          int                      T_enc_max,
                                          int                      n_batch,
                                          bool                     use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || T_enc_max <= 0 || n_batch <= 0) {
        return sb;
    }
    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine step(batched): requires flash path");
        return sb;
    }

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;
    const int B       = n_batch;

    sb.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_name(sb.token_ids_in, "step.token_ids");
    ggml_set_input(sb.token_ids_in);
    sb.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_name(sb.pos_ids_in, "step.pos_ids");
    ggml_set_input(sb.pos_ids_in);
    sb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, 1, B);
    ggml_set_name(sb.kv_idx_in, "step.kv_idx");
    ggml_set_input(sb.kv_idx_in);
    sb.self_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F16, max_n_kv, 1, 1, B);
    ggml_set_name(sb.self_mask_in, "step.self_mask");
    ggml_set_input(sb.self_mask_in);
    sb.cross_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F16, T_enc_max, 1, 1, B);
    ggml_set_name(sb.cross_mask_in, "step.cross_mask");
    ggml_set_input(sb.cross_mask_in);

    sb.graph = ggml_new_graph_custom(ctx, 16384, false);
    if (sb.graph == nullptr) {
        return sb;
    }

    ggml_tensor * x = ggml_get_rows(ctx, w.dec_top.token_embd_w, sb.token_ids_in);

    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, /*beta=*/nullptr);
            y = mha_self_step_batched(ctx, sb.graph, y, kv_cache, sb.pos_ids_in, sb.kv_idx_in, sb.self_mask_in,
                                      b.self_q_w, b.self_k_w, b.self_v_w, b.self_out_w, hp, n_heads, d_model, i,
                                      max_n_kv, B);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, /*beta=*/nullptr);
            y = mha_cross_step_batched(ctx, y, kv_cache, sb.cross_mask_in, b.cross_q_w, b.cross_out_w, hp, n_heads,
                                       d_model, i, T_enc_max, B);
            x = ggml_add(ctx, x, y);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, /*beta=*/nullptr);
            y = ffn_decoder_swiglu(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b, hp.dec_ffn_dim);
            x = ggml_add(ctx, x, y);
        }
    }

    x                    = layer_norm(ctx, x, w.dec_top.final_norm_w, /*beta=*/nullptr);
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);  // tied head

    sb.argmax_out = ggml_argmax(ctx, logits);                             // [B]
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);
    return sb;
}

}  // namespace transcribe::moonshine
