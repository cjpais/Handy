// arch/moonshine_streaming/decoder.cpp - Moonshine-Streaming decoder
// graph builders.
//
// Closest in-tree analog: src/arch/moonshine/decoder.cpp. Differences:
//
//   - Adapter:  pos_emb (and optional proj) applied in a SEPARATE graph
//     (build_adapter_graph) before the cross_kv precompute graph runs.
//     The HF reference does this inside `decoder.forward` IN-PLACE; we
//     pull it out so the per-step graphs never re-apply it.
//   - Untied lm_head: final logits projection uses dec.lm_head.weight,
//     not dec.token_embd.weight.
//   - Decoder LayerNorms are vanilla `nn.LayerNorm(bias=False)` — no
//     unit_offset folding.
//   - RoPE: GPT-J / interleaved (= GGML_ROPE_TYPE_NORMAL, not NEOX), as moonshine.

#include "decoder.h"

#include "conformer/conformer.h"
#include "encoder.h"
#include "ggml.h"
#include "moonshine_streaming.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>

namespace transcribe::moonshine_streaming {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

ggml_tensor * pad_head_dim(ggml_context * ctx, ggml_tensor * t, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_pad(ctx, t, /*p0=*/pad, /*p1=*/0, /*p2=*/0, /*p3=*/0);
}

ggml_tensor * unpad_head_dim(ggml_context * ctx, ggml_tensor * t, int head_dim, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_view_3d(ctx, t, head_dim, t->ne[1], t->ne[2], t->nb[1], t->nb[2], 0);
}

// Decoder uses GPT-J / interleaved RoPE (`rotate_half` slices
// `x[..., 0::2]` / `x[..., 1::2]`); ggml mode = GGML_ROPE_TYPE_NORMAL.
ggml_tensor * apply_partial_rope(ggml_context *                    ctx,
                                 ggml_tensor *                     x,
                                 ggml_tensor *                     positions,
                                 const MoonshineStreamingHParams & hp,
                                 int                               head_dim_rot) {
    return ggml_rope_ext(ctx, x, positions, /*c=*/nullptr, head_dim_rot, GGML_ROPE_TYPE_NORMAL,
                         /*n_ctx_orig=*/0, hp.rope_theta,
                         /*freq_scale=*/1.0f,
                         /*ext_factor=*/0.0f,
                         /*attn_factor=*/1.0f,
                         /*beta_fast=*/32.0f,
                         /*beta_slow=*/1.0f);
}

// SwiGLU MLP: fc1 -> 2·ffn_dim, chunk(2, dim=-1) into [x_proj, gate]
// (innermost dim = ggml ne[0]), out = silu(gate) * x_proj, fc2.
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
    const int64_t T          = h->ne[1];
    const size_t  el         = ggml_element_size(h);
    const size_t  half_bytes = static_cast<size_t>(ffn_dim) * el;

    ggml_tensor * x_proj = ggml_view_2d(ctx, h, ffn_dim, T, h->nb[1], 0);
    ggml_tensor * gate   = ggml_view_2d(ctx, h, ffn_dim, T, h->nb[1], half_bytes);

    ggml_tensor * y = ggml_mul(ctx, ggml_silu(ctx, ggml_cont(ctx, gate)), ggml_cont(ctx, x_proj));
    ggml_tensor * o = ggml_mul_mat(ctx, fc2_w, y);
    if (fc2_b != nullptr) {
        o = ggml_add(ctx, o, fc2_b);
    }
    return o;
}

ggml_tensor * mha_self_cached(ggml_context *                    ctx,
                              ggml_cgraph *                     gf,
                              ggml_tensor *                     x,
                              MoonshineStreamingKvCache &       kv_cache,
                              ggml_tensor *                     pos_ids,
                              ggml_tensor *                     mask,
                              ggml_tensor *                     q_w,
                              ggml_tensor *                     k_w,
                              ggml_tensor *                     v_w,
                              ggml_tensor *                     out_w,
                              const MoonshineStreamingHParams & hp,
                              int                               n_heads,
                              int                               d_model,
                              int                               il,
                              int                               n_past,
                              int                               n_tokens,
                              int                               n_kv,
                              bool                              use_flash) {
    const int   head_dim     = d_model / n_heads;
    const int   head_dim_pad = hp.dec_head_dim_padded();
    const int   head_dim_rot = hp.dec_head_dim_rot();
    const int   pad          = head_dim_pad - head_dim;
    const int   n_ctx        = kv_cache.n_ctx;
    const float scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);

    Qcur = ggml_reshape_4d(ctx, Qcur, head_dim, n_heads, n_tokens, 1);
    Kcur = ggml_reshape_4d(ctx, Kcur, head_dim, n_heads, n_tokens, 1);
    Vcur = ggml_reshape_4d(ctx, Vcur, head_dim, n_heads, n_tokens, 1);

    ggml_tensor * Q_rope = apply_partial_rope(ctx, Qcur, pos_ids, hp, head_dim_rot);
    ggml_tensor * K_rope = apply_partial_rope(ctx, Kcur, pos_ids, hp, head_dim_rot);

    // Q needs [head_dim, n_tokens, n_heads, 1] for attention. K/V are
    // already in cache memory order [head_dim, n_heads, n_tokens, 1] after
    // RoPE (or reshape, for V), so they're written straight to the cache.
    ggml_tensor * Q_unpad = ggml_cont(ctx, ggml_permute(ctx, Q_rope, 0, 2, 1, 3));

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

    const size_t  k_elem = ggml_element_size(kv_cache.self_k);
    ggml_tensor * K = ggml_view_3d(ctx, kv_cache.self_k, head_dim, n_kv, n_heads, k_elem * d_model, k_elem * head_dim,
                                   k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.self_v);
    ggml_tensor * V = ggml_view_3d(ctx, kv_cache.self_v, head_dim, n_kv, n_heads, v_elem * d_model, v_elem * head_dim,
                                   v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

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
    o = ggml_reshape_2d(ctx, o, d_model, n_tokens);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

ggml_tensor * mha_cross_cached(ggml_context *                    ctx,
                               ggml_tensor *                     x,
                               MoonshineStreamingKvCache &       kv_cache,
                               ggml_tensor *                     q_w,
                               ggml_tensor *                     out_w,
                               const MoonshineStreamingHParams & hp,
                               int                               n_heads,
                               int                               d_model,
                               int                               il,
                               int                               T_enc,
                               bool                              use_flash) {
    const int     head_dim     = d_model / n_heads;
    const int     head_dim_pad = hp.dec_head_dim_padded();
    const int     pad          = head_dim_pad - head_dim;
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

// Adapter graph.
AdapterBuild build_adapter_graph(ggml_context *                    ctx,
                                 const MoonshineStreamingWeights & w,
                                 const MoonshineStreamingHParams & hp,
                                 int                               T_enc) {
    AdapterBuild ab{};

    if (ctx == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming adapter: invalid arg (ctx=%p, T_enc=%d)",
                static_cast<void *>(ctx), T_enc);
        return ab;
    }

    const int enc_h = hp.enc_d_model;
    const int dec_h = hp.dec_d_model;

    ab.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, enc_h, T_enc);
    named(ab.encoder_out_in, "adapter.encoder_in");
    ggml_set_input(ab.encoder_out_in);

    ab.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    named(ab.pos_ids_in, "adapter.pos_ids");
    ggml_set_input(ab.pos_ids_in);

    // pos_emb slice: ggml_get_rows over [0..T_enc) yields ne=[enc_h, T_enc].
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.adapter.pos_emb_w, ab.pos_ids_in);
    named(pos_emb, "adapter.pos_emb");
    ab.pos_emb_out = pos_emb;
    transcribe::debug::mark_tensor_for_dump(pos_emb);

    ggml_tensor * y = ggml_add(ctx, ab.encoder_out_in, pos_emb);
    if (hp.adapter_has_proj) {
        // proj_w ne=[enc_h, dec_h]; result ne=[dec_h, T_enc]
        y = ggml_mul_mat(ctx, w.adapter.proj_w, y);
    } else if (enc_h != dec_h) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming adapter: enc_h (%d) != dec_h (%d) but adapter_has_proj=false", enc_h, dec_h);
        return ab;
    }
    named(y, "adapter.out");
    transcribe::debug::mark_tensor_for_dump(y);
    ab.out = y;
    ggml_set_output(ab.out);

    ab.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (ab.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming adapter: ggml_new_graph_custom failed");
        return ab;
    }
    // Build forward through both pos_emb (for the dump) and out.
    ggml_build_forward_expand(ab.graph, pos_emb);
    ggml_build_forward_expand(ab.graph, ab.out);

    return ab;
}

// Cross-KV precompute graph.
DecoderBuild build_cross_kv_graph(ggml_context *                    ctx,
                                  const MoonshineStreamingWeights & w,
                                  const MoonshineStreamingHParams & hp,
                                  MoonshineStreamingKvCache &       kv_cache,
                                  int                               T_enc) {
    DecoderBuild db{};

    if (ctx == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming cross_kv: invalid arg (ctx=%p, T_enc=%d)",
                static_cast<void *>(ctx), T_enc);
        return db;
    }

    const int d_model = hp.dec_d_model;

    // Reads the post-adapter encoder hidden, NOT the raw encoder output.
    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_enc);
    named(db.encoder_out_in, "dec.encoder_in_adapted");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming cross_kv: ggml_new_graph_custom failed");
        return db;
    }

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    for (int il = 0; il < n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

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

// Cross-KV projection (incremental streaming).
CrossKVProjectionBuild build_cross_kv_projection_graph(ggml_context *                    ctx,
                                                       const MoonshineStreamingWeights & w,
                                                       const MoonshineStreamingHParams & hp,
                                                       int                               n_frames) {
    CrossKVProjectionBuild pb{};

    if (ctx == nullptr || n_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming cross_kv_proj: invalid arg "
                "(ctx=%p, n_frames=%d)",
                static_cast<void *>(ctx), n_frames);
        return pb;
    }

    const int d_model = hp.dec_d_model;

    pb.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, n_frames);
    named(pb.encoder_out_in, "dec.encoder_in_adapted.proj");
    ggml_set_input(pb.encoder_out_in);

    pb.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (pb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming cross_kv_proj: ggml_new_graph_custom failed");
        return pb;
    }

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    pb.per_layer_k.reserve(static_cast<size_t>(n_layers));
    pb.per_layer_v.reserve(static_cast<size_t>(n_layers));
    for (int il = 0; il < n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        ggml_tensor * Kcross = ggml_mul_mat(ctx, blk.cross_k_w, pb.encoder_out_in);
        ggml_tensor * Vcross = ggml_mul_mat(ctx, blk.cross_v_w, pb.encoder_out_in);

        char kname[64], vname[64];
        std::snprintf(kname, sizeof(kname), "xkv.proj.k.%d", il);
        std::snprintf(vname, sizeof(vname), "xkv.proj.v.%d", il);
        named(Kcross, kname);
        named(Vcross, vname);
        ggml_set_output(Kcross);
        ggml_set_output(Vcross);

        pb.per_layer_k.push_back(Kcross);
        pb.per_layer_v.push_back(Vcross);

        ggml_build_forward_expand(pb.graph, Kcross);
        ggml_build_forward_expand(pb.graph, Vcross);
    }

    return pb;
}

// Cross-KV commit (host buffers → persistent cache).
CrossKVCommitBuild build_cross_kv_commit_graph(ggml_context *                    ctx,
                                               const MoonshineStreamingHParams & hp,
                                               MoonshineStreamingKvCache &       kv_cache,
                                               int                               T_enc) {
    CrossKVCommitBuild cb{};

    if (ctx == nullptr || T_enc <= 0 || kv_cache.cross_k == nullptr || kv_cache.cross_v == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming cross_kv_commit: invalid arg "
                "(ctx=%p, T_enc=%d, cache=%s)",
                static_cast<void *>(ctx), T_enc,
                (kv_cache.cross_k == nullptr || kv_cache.cross_v == nullptr) ? "null" : "ok");
        return cb;
    }

    const int d_model = hp.dec_d_model;
    cb.graph          = ggml_new_graph_custom(ctx, 4096, false);
    if (cb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming cross_kv_commit: ggml_new_graph_custom failed");
        return cb;
    }

    const int n_layers = static_cast<int>(hp.dec_n_layers);
    cb.per_layer_k_in.reserve(static_cast<size_t>(n_layers));
    cb.per_layer_v_in.reserve(static_cast<size_t>(n_layers));

    const size_t k_elem = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem = ggml_element_size(kv_cache.cross_v);

    for (int il = 0; il < n_layers; ++il) {
        ggml_tensor * K_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_enc);
        ggml_tensor * V_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_enc);
        char          kname[64], vname[64];
        std::snprintf(kname, sizeof(kname), "xkv.commit.k_in.%d", il);
        std::snprintf(vname, sizeof(vname), "xkv.commit.v_in.%d", il);
        named(K_in, kname);
        named(V_in, vname);
        ggml_set_input(K_in);
        ggml_set_input(V_in);
        cb.per_layer_k_in.push_back(K_in);
        cb.per_layer_v_in.push_back(V_in);

        ggml_tensor * k_dst = ggml_view_1d(ctx, kv_cache.cross_k, static_cast<int64_t>(T_enc) * d_model,
                                           k_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));
        ggml_tensor * v_dst = ggml_view_1d(ctx, kv_cache.cross_v, static_cast<int64_t>(T_enc) * d_model,
                                           v_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * d_model));

        ggml_build_forward_expand(cb.graph, ggml_cpy(ctx, K_in, k_dst));
        ggml_build_forward_expand(cb.graph, ggml_cpy(ctx, V_in, v_dst));
    }

    return cb;
}

// KV-cached decoder graph (prompt + step).
DecoderBuild build_decoder_graph_kv(ggml_context *                    ctx,
                                    const MoonshineStreamingWeights & w,
                                    const MoonshineStreamingHParams & hp,
                                    MoonshineStreamingKvCache &       kv_cache,
                                    int                               n_tokens,
                                    int                               n_past,
                                    int                               T_enc,
                                    bool                              skip_log_softmax,
                                    bool                              use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || n_tokens <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming decoder_kv: invalid arg "
                "(ctx=%p, n_tokens=%d, T_enc=%d)",
                static_cast<void *>(ctx), n_tokens, T_enc);
        return db;
    }
    const int n_kv = n_past + n_tokens;
    if (n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming decoder_kv: n_kv=%d exceeds n_ctx=%d", n_kv,
                kv_cache.n_ctx);
        return db;
    }

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;

    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    db.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(db.pos_ids_in, "dec.pos_ids");
    ggml_set_input(db.pos_ids_in);

    ggml_tensor * causal_mask = nullptr;
    const bool    need_mask   = (n_tokens > 1);
    if (need_mask) {
        db.causal_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_kv, n_tokens);
        named(db.causal_mask_in, "dec.causal_mask");
        ggml_set_input(db.causal_mask_in);
        causal_mask = ggml_cast(ctx, db.causal_mask_in, GGML_TYPE_F16);
    }

    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, db.token_ids_in);
    named(tok_emb, "dec.token_emb");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(tok_emb);
        db.dumps.token_emb = tok_emb;
    }

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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming decoder_kv: ggml_new_graph_custom failed");
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

        if (n_past == 0) {
            // Track every block so model.cpp can index by layer; only mark
            // the auto_blocks subset for dump (see dump_block_index).
            db.dumps.block_outs.push_back(x);
            if (dump_block_index(i, n_blocks)) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "dec.block.%d.out", i);
                named(x, bname);
                transcribe::debug::mark_tensor_for_dump(x);
            }
        }
    }

    x = layer_norm(ctx, x, w.dec_top.final_norm_w, /*beta=*/nullptr);
    named(x, "dec.out_before_head");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(x);
        db.dumps.out_before_head = x;
    }

    // Untied lm_head: project against dec.lm_head.weight, NOT the token embed.
    ggml_tensor * logits_raw = ggml_mul_mat(ctx, w.dec_top.lm_head_w, x);
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

    // Backend argmax over the last position when skipping log_softmax —
    // every step pass downloads a single int32 instead of full vocab logits.
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

// Offline batched decode (B utterances).
namespace {

ggml_tensor * unpad_head_dim_4d(ggml_context * ctx, ggml_tensor * t, int head_dim, int pad) {
    if (pad <= 0) {
        return t;
    }
    return ggml_view_4d(ctx, t, head_dim, t->ne[1], t->ne[2], t->ne[3], t->nb[1], t->nb[2], t->nb[3], 0);
}

ggml_tensor * mha_self_step_batched(ggml_context *                    ctx,
                                    ggml_cgraph *                     gf,
                                    ggml_tensor *                     x,
                                    MoonshineStreamingKvCache &       kv_cache,
                                    ggml_tensor *                     pos_ids,
                                    ggml_tensor *                     kv_idx,
                                    ggml_tensor *                     mask,
                                    ggml_tensor *                     q_w,
                                    ggml_tensor *                     k_w,
                                    ggml_tensor *                     v_w,
                                    ggml_tensor *                     out_w,
                                    const MoonshineStreamingHParams & hp,
                                    int                               n_heads,
                                    int                               d_model,
                                    int                               il,
                                    int                               max_n_kv,
                                    int                               B) {
    const int     head_dim     = d_model / n_heads;
    const int     head_dim_pad = hp.dec_head_dim_padded();
    const int     head_dim_rot = hp.dec_head_dim_rot();
    const int     pad          = head_dim_pad - head_dim;
    const int64_t n_ctx        = kv_cache.n_ctx;
    const float   scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t  k_elem       = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem       = ggml_element_size(kv_cache.self_v);

    ggml_tensor * Q = ggml_mul_mat(ctx, q_w, x);
    ggml_tensor * K = ggml_mul_mat(ctx, k_w, x);
    ggml_tensor * V = ggml_mul_mat(ctx, v_w, x);
    Q               = ggml_reshape_3d(ctx, Q, head_dim, n_heads, B);
    K               = ggml_reshape_3d(ctx, K, head_dim, n_heads, B);
    V               = ggml_reshape_3d(ctx, V, head_dim, n_heads, B);
    Q               = apply_partial_rope(ctx, Q, pos_ids, hp, head_dim_rot);
    K               = apply_partial_rope(ctx, K, pos_ids, hp, head_dim_rot);

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

    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * Qp    = pad_head_dim(ctx, Q_att, pad);
    ggml_tensor * Kp    = pad_head_dim(ctx, ggml_cont(ctx, K_att), pad);
    ggml_tensor * Vp    = pad_head_dim(ctx, ggml_cont(ctx, V_att), pad);

    ggml_tensor * o = ggml_flash_attn_ext(ctx, Qp, Kp, Vp, mask, scale, 0.0f, 0.0f);
    o               = ggml_cont(ctx, unpad_head_dim_4d(ctx, o, head_dim, pad));
    o               = ggml_reshape_2d(ctx, o, d_model, B);

    o = ggml_mul_mat(ctx, out_w, o);
    return o;
}

ggml_tensor * mha_cross_step_batched(ggml_context *                    ctx,
                                     ggml_tensor *                     x,
                                     MoonshineStreamingKvCache &       kv_cache,
                                     ggml_tensor *                     cross_mask,
                                     ggml_tensor *                     q_w,
                                     ggml_tensor *                     out_w,
                                     const MoonshineStreamingHParams & hp,
                                     int                               n_heads,
                                     int                               d_model,
                                     int                               il,
                                     int                               T_enc_max,
                                     int                               B) {
    const int    head_dim     = d_model / n_heads;
    const int    head_dim_pad = hp.dec_head_dim_padded();
    const int    pad          = head_dim_pad - head_dim;
    const float  scale        = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t k_elem       = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem       = ggml_element_size(kv_cache.cross_v);

    ggml_tensor * Q     = ggml_mul_mat(ctx, q_w, x);
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

DecoderBuild build_cross_kv_graph_batched(ggml_context *                    ctx,
                                          const MoonshineStreamingWeights & w,
                                          const MoonshineStreamingHParams & hp,
                                          MoonshineStreamingKvCache &       kv_cache,
                                          int                               T_enc_max,
                                          int                               n_batch) {
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

StepBuildBatched build_step_graph_batched(ggml_context *                    ctx,
                                          const MoonshineStreamingWeights & w,
                                          const MoonshineStreamingHParams & hp,
                                          MoonshineStreamingKvCache &       kv_cache,
                                          int                               max_n_kv,
                                          int                               T_enc_max,
                                          int                               n_batch,
                                          bool                              use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || T_enc_max <= 0 || n_batch <= 0) {
        return sb;
    }
    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming step(batched): requires flash path");
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
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_top.lm_head_w, x);  // untied head

    sb.argmax_out = ggml_argmax(ctx, logits);                          // [B]
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);
    return sb;
}

}  // namespace transcribe::moonshine_streaming
