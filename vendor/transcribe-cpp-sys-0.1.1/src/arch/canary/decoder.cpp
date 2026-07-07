// arch/canary/decoder.cpp - Canary autoregressive Transformer decoder.
//
// Untied LM head; per-sublayer dump points at layers {0, n_layers/2, n_layers-1}.

#include "decoder.h"

#include "canary.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::canary {

namespace {

constexpr float kLayerNormEps = 1e-5f;

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta) {
    ggml_tensor * y = ggml_norm(ctx, x, kLayerNormEps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

// Self-attention with KV cache. Mirrors cohere's mha_self_cached.
ggml_tensor * mha_self_cached(ggml_context *  ctx,
                              ggml_cgraph *   gf,
                              ggml_tensor *   x,
                              CanaryKvCache & kv_cache,
                              ggml_tensor *   mask,
                              ggml_tensor *   q_w,
                              ggml_tensor *   q_b,
                              ggml_tensor *   k_w,
                              ggml_tensor *   k_b,
                              ggml_tensor *   v_w,
                              ggml_tensor *   v_b,
                              ggml_tensor *   out_w,
                              ggml_tensor *   out_b,
                              int             n_heads,
                              int             hidden,
                              int             il,
                              int             n_past,
                              int             n_tokens,
                              bool            use_flash) {
    const int   head_dim = hidden / n_heads;
    const float scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int   n_ctx    = kv_cache.n_ctx;
    const int   n_kv     = n_past + n_tokens;

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    if (k_b != nullptr) {
        Kcur = ggml_add(ctx, Kcur, k_b);
    }

    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    {
        ggml_tensor * k_dst = ggml_view_1d(
            ctx, kv_cache.self_k, static_cast<int64_t>(n_tokens) * hidden,
            ggml_element_size(kv_cache.self_k) *
                static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden + static_cast<int64_t>(n_past) * hidden));

        ggml_tensor * v_dst = ggml_view_1d(
            ctx, kv_cache.self_v, static_cast<int64_t>(n_tokens) * hidden,
            ggml_element_size(kv_cache.self_v) *
                static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden + static_cast<int64_t>(n_past) * hidden));

        ggml_build_forward_expand(gf, ggml_cpy(ctx, Kcur, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, Vcur, v_dst));
    }

    ggml_tensor * K = ggml_view_3d(
        ctx, kv_cache.self_k, head_dim, n_kv, n_heads, ggml_element_size(kv_cache.self_k) * hidden,
        ggml_element_size(kv_cache.self_k) * head_dim,
        ggml_element_size(kv_cache.self_k) * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));

    ggml_tensor * V = ggml_view_3d(
        ctx, kv_cache.self_v, head_dim, n_kv, n_heads, ggml_element_size(kv_cache.self_v) * hidden,
        ggml_element_size(kv_cache.self_v) * head_dim,
        ggml_element_size(kv_cache.self_v) * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Self-attention for the static-topology step graph. KV cache writes
// go through ggml_set_rows at runtime row index `kv_idx`; reads span
// the full [0, max_n_kv) window with `mask` gating valid positions.
// n_tokens is implicitly 1.
ggml_tensor * mha_self_step(ggml_context *  ctx,
                            ggml_cgraph *   gf,
                            ggml_tensor *   x,
                            CanaryKvCache & kv_cache,
                            ggml_tensor *   mask,
                            ggml_tensor *   kv_idx,
                            ggml_tensor *   q_w,
                            ggml_tensor *   q_b,
                            ggml_tensor *   k_w,
                            ggml_tensor *   k_b,
                            ggml_tensor *   v_w,
                            ggml_tensor *   v_b,
                            ggml_tensor *   out_w,
                            ggml_tensor *   out_b,
                            int             n_heads,
                            int             hidden,
                            int             il,
                            int             max_n_kv,
                            bool            use_flash) {
    const int   head_dim = hidden / n_heads;
    const float scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int   n_ctx    = kv_cache.n_ctx;

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    if (k_b != nullptr) {
        Kcur = ggml_add(ctx, Kcur, k_b);
    }

    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, /*n_tokens=*/1);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    // KV cache write via ggml_set_rows. Static topology — only kv_idx's
    // value changes per step. Per-layer view spans [hidden, n_ctx], and
    // we write a single [hidden, 1] row at the kv_idx position.
    {
        const size_t k_elem      = ggml_element_size(kv_cache.self_k);
        const size_t v_elem      = ggml_element_size(kv_cache.self_v);
        const size_t layer_off_k = k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden);
        const size_t layer_off_v = v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden);

        ggml_tensor * k_layer = ggml_view_2d(ctx, kv_cache.self_k, hidden, n_ctx, k_elem * hidden, layer_off_k);
        ggml_tensor * v_layer = ggml_view_2d(ctx, kv_cache.self_v, hidden, n_ctx, v_elem * hidden, layer_off_v);

        ggml_tensor * K_row = ggml_reshape_2d(ctx, Kcur, hidden, 1);
        ggml_tensor * V_row = ggml_reshape_2d(ctx, Vcur, hidden, 1);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    // Static read across [0, max_n_kv); mask zeros valid positions and
    // -inf's the rest, so flash-attn ignores empty/future slots. Costs
    // some bandwidth at low n_past — caller picks max_n_kv to bound it.
    ggml_tensor * K = ggml_view_3d(
        ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, ggml_element_size(kv_cache.self_k) * hidden,
        ggml_element_size(kv_cache.self_k) * head_dim,
        ggml_element_size(kv_cache.self_k) * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));

    ggml_tensor * V = ggml_view_3d(
        ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, ggml_element_size(kv_cache.self_v) * hidden,
        ggml_element_size(kv_cache.self_v) * head_dim,
        ggml_element_size(kv_cache.self_v) * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, hidden, 1);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, hidden, 1);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Cross-attention reading from pre-populated KV cache.
ggml_tensor * mha_cross_cached(ggml_context *  ctx,
                               ggml_tensor *   x,
                               CanaryKvCache & kv_cache,
                               ggml_tensor *   q_w,
                               ggml_tensor *   q_b,
                               ggml_tensor *   out_w,
                               ggml_tensor *   out_b,
                               int             n_heads,
                               int             hidden,
                               int             il,
                               int             T_enc,
                               bool            use_flash) {
    const int     head_dim = hidden / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t n_tokens = x->ne[1];

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    ggml_tensor * K = ggml_view_3d(
        ctx, kv_cache.cross_k, head_dim, T_enc, n_heads, ggml_element_size(kv_cache.cross_k) * hidden,
        ggml_element_size(kv_cache.cross_k) * head_dim,
        ggml_element_size(kv_cache.cross_k) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

    ggml_tensor * V = ggml_view_3d(
        ctx, kv_cache.cross_v, head_dim, T_enc, n_heads, ggml_element_size(kv_cache.cross_v) * hidden,
        ggml_element_size(kv_cache.cross_v) * head_dim,
        ggml_element_size(kv_cache.cross_v) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, nullptr, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Layers we attach per-sublayer dump points to: {0, n_layers/2, n_layers-1}
// (deduped when the set collapses, n_layers <= 2).
bool is_dump_layer(int i, int n_layers) {
    if (n_layers <= 0) {
        return false;
    }
    if (i == 0 || i == n_layers - 1) {
        return true;
    }
    return i == n_layers / 2;
}

}  // namespace

DecoderBuild build_cross_kv_graph(ggml_context *        ctx,
                                  const CanaryWeights & w,
                                  const CanaryHParams & hp,
                                  CanaryKvCache &       kv_cache,
                                  int                   T_enc) {
    DecoderBuild db{};

    if (ctx == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary cross_kv: invalid arg (ctx=%p, T_enc=%d)", static_cast<void *>(ctx),
                T_enc);
        return db;
    }

    const int64_t hidden = hp.dec_d_model;

    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T_enc);
    ggml_set_name(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary cross_kv: ggml_new_graph_custom failed");
        return db;
    }

    for (int il = 0; il < hp.dec_n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        ggml_tensor * Kcross = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        if (blk.cross_k_b != nullptr) {
            Kcross = ggml_add(ctx, Kcross, blk.cross_k_b);
        }

        ggml_tensor * Vcross = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);
        if (blk.cross_v_b != nullptr) {
            Vcross = ggml_add(ctx, Vcross, blk.cross_v_b);
        }

        ggml_tensor * k_dst = ggml_view_1d(
            ctx, kv_cache.cross_k, static_cast<int64_t>(T_enc) * hidden,
            ggml_element_size(kv_cache.cross_k) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

        ggml_tensor * v_dst = ggml_view_1d(
            ctx, kv_cache.cross_v, static_cast<int64_t>(T_enc) * hidden,
            ggml_element_size(kv_cache.cross_v) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Kcross, k_dst));
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Vcross, v_dst));
    }

    return db;
}

DecoderBuild build_decoder_graph_kv(ggml_context *        ctx,
                                    const CanaryWeights & w,
                                    const CanaryHParams & hp,
                                    CanaryKvCache &       kv_cache,
                                    int                   n_tokens,
                                    int                   n_past,
                                    int                   T_enc,
                                    bool                  skip_log_softmax,
                                    bool                  use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || n_tokens <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary decoder_kv: invalid arg "
                "(ctx=%p, n_tokens=%d, T_enc=%d)",
                static_cast<void *>(ctx), n_tokens, T_enc);
        return db;
    }

    const int64_t hidden  = hp.dec_d_model;
    const int     n_heads = hp.dec_n_heads;
    const int     n_kv    = n_past + n_tokens;

    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    ggml_set_name(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    db.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    ggml_set_name(db.pos_ids_in, "dec.pos_ids");
    ggml_set_input(db.pos_ids_in);

    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, db.token_ids_in);
    tok_emb               = named(tok_emb, "dec.token_emb");
    ggml_set_output(tok_emb);
    db.dumps.token_emb = tok_emb;

    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, db.pos_ids_in);
    pos_emb               = named(pos_emb, "dec.pos_emb");
    ggml_set_output(pos_emb);
    db.dumps.pos_emb = pos_emb;

    // Embedding layer norm: NeMo applies it AFTER tok+pos sum.
    ggml_tensor * x = ggml_add(ctx, tok_emb, pos_emb);
    x               = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);
    x               = named(x, "dec.embed_norm");
    ggml_set_output(x);
    db.dumps.embed_norm = x;

    // Causal mask for self-attention. Shape: [n_kv, n_tokens].
    ggml_tensor * causal_mask = nullptr;
    if (n_tokens > 1) {
        causal_mask = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_kv, n_tokens);
        ggml_set_name(causal_mask, "dec.causal_mask");
        ggml_set_input(causal_mask);
        causal_mask = ggml_cast(ctx, causal_mask, GGML_TYPE_F16);
    }

    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary decoder_kv: ggml_new_graph_custom failed");
        return db;
    }

    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk       = w.dec_blocks[i];
        const bool   dump_this = is_dump_layer(i, hp.dec_n_layers) && i < 64;

        // norm1 + self-attention. NeMo's forward_hook captures the sublayer
        // output BEFORE the residual add, so we dump self_out, not x.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm1_w, blk.norm1_b);
            ggml_tensor * self_out =
                mha_self_cached(ctx, db.graph, x_norm, kv_cache, causal_mask, blk.self_q_w, blk.self_q_b, blk.self_k_w,
                                blk.self_k_b, blk.self_v_w, blk.self_v_b, blk.self_o_w, blk.self_o_b, n_heads,
                                static_cast<int>(hidden), i, n_past, n_tokens, use_flash);
            if (dump_this) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "dec.layer.%d.self_attn.out", i);
                self_out = named(self_out, bname);
                ggml_set_output(self_out);
                db.dumps.sub_self[i] = self_out;
            }
            x = ggml_add(ctx, x, self_out);
        }

        // norm2 + cross-attention (dump before residual, as above).
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm2_w, blk.norm2_b);
            ggml_tensor * cross_out =
                mha_cross_cached(ctx, x_norm, kv_cache, blk.cross_q_w, blk.cross_q_b, blk.cross_o_w, blk.cross_o_b,
                                 n_heads, static_cast<int>(hidden), i, T_enc, use_flash);
            if (dump_this) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "dec.layer.%d.cross_attn.out", i);
                cross_out = named(cross_out, bname);
                ggml_set_output(cross_out);
                db.dumps.sub_cross[i] = cross_out;
            }
            x = ggml_add(ctx, x, cross_out);
        }

        // norm3 + FFN (dump before residual, as above).
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm3_w, blk.norm3_b);
            ggml_tensor * ff     = ggml_mul_mat(ctx, blk.ffn_up_w, x_norm);
            if (blk.ffn_up_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_up_b);
            }
            if (hp.dec_activation == "relu") {
                ff = ggml_relu(ctx, ff);
            } else {
                ff = ggml_silu(ctx, ff);
            }
            ff = ggml_mul_mat(ctx, blk.ffn_down_w, ff);
            if (blk.ffn_down_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_down_b);
            }
            if (dump_this) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "dec.layer.%d.ffn.out", i);
                ff = named(ff, bname);
                ggml_set_output(ff);
                db.dumps.sub_ffn[i] = ff;
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    // Final LayerNorm (NeMo applies this after the last block).
    x                        = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);
    x                        = named(x, "dec.out_before_head");
    db.dumps.out_before_head = x;

    // Untied LM head.
    ggml_tensor * logits = ggml_mul_mat(ctx, w.head.weight, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    logits = named(logits, "dec.logits_raw");
    ggml_set_output(logits);
    db.dumps.logits_raw = logits;

    if (!skip_log_softmax) {
        logits = ggml_log(ctx, ggml_soft_max(ctx, logits));
    }
    logits          = named(logits, "dec.logits");
    db.dumps.logits = logits;

    db.out = logits;
    ggml_set_output(db.out);

    if (skip_log_softmax) {
        ggml_tensor * last_logits = logits;
        if (n_tokens > 1) {
            const int64_t vocab     = logits->ne[0];
            const size_t  row_bytes = ggml_element_size(logits) * vocab;
            last_logits             = ggml_view_2d(ctx, logits, vocab, /*n=*/1, row_bytes, row_bytes * (n_tokens - 1));
            last_logits             = ggml_cont(ctx, last_logits);
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

StepBuild build_step_graph(ggml_context *        ctx,
                           const CanaryWeights & w,
                           const CanaryHParams & hp,
                           CanaryKvCache &       kv_cache,
                           int                   max_n_kv,
                           int                   T_enc,
                           bool                  use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary step: invalid arg (max_n_kv=%d, T_enc=%d)", max_n_kv, T_enc);
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int64_t hidden  = hp.dec_d_model;
    const int     n_heads = hp.dec_n_heads;

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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary step: ggml_new_graph_custom failed");
        return sb;
    }

    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, sb.token_id_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, sb.pos_id_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);
    x                     = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);

    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];

        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm1_w, blk.norm1_b);
            ggml_tensor * self_out =
                mha_self_step(ctx, sb.graph, x_norm, kv_cache, sb.mask_in, sb.kv_idx_in, blk.self_q_w, blk.self_q_b,
                              blk.self_k_w, blk.self_k_b, blk.self_v_w, blk.self_v_b, blk.self_o_w, blk.self_o_b,
                              n_heads, static_cast<int>(hidden), i, max_n_kv, use_flash);
            x = ggml_add(ctx, x, self_out);
        }

        // norm2 + cross-attention. cross_k/v are pre-populated full T_enc.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm2_w, blk.norm2_b);
            ggml_tensor * cross_out =
                mha_cross_cached(ctx, x_norm, kv_cache, blk.cross_q_w, blk.cross_q_b, blk.cross_o_w, blk.cross_o_b,
                                 n_heads, static_cast<int>(hidden), i, T_enc, use_flash);
            x = ggml_add(ctx, x, cross_out);
        }

        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm3_w, blk.norm3_b);
            ggml_tensor * ff     = ggml_mul_mat(ctx, blk.ffn_up_w, x_norm);
            if (blk.ffn_up_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_up_b);
            }
            if (hp.dec_activation == "relu") {
                ff = ggml_relu(ctx, ff);
            } else {
                ff = ggml_silu(ctx, ff);
            }
            ff = ggml_mul_mat(ctx, blk.ffn_down_w, ff);
            if (blk.ffn_down_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_down_b);
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    x = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);

    ggml_tensor * logits = ggml_mul_mat(ctx, w.head.weight, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    sb.argmax_out = ggml_argmax(ctx, logits);
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);

    return sb;
}

// ===========================================================================
// Offline batched decode (B utterances)
// ===========================================================================

namespace {

// Batched self-attention step. x: [hidden, B] (one token per utterance).
ggml_tensor * mha_self_step_batched(ggml_context *  ctx,
                                    ggml_cgraph *   gf,
                                    ggml_tensor *   x,
                                    CanaryKvCache & kv_cache,
                                    ggml_tensor *   mask,
                                    ggml_tensor *   kv_idx,
                                    ggml_tensor *   q_w,
                                    ggml_tensor *   q_b,
                                    ggml_tensor *   k_w,
                                    ggml_tensor *   k_b,
                                    ggml_tensor *   v_w,
                                    ggml_tensor *   v_b,
                                    ggml_tensor *   out_w,
                                    ggml_tensor *   out_b,
                                    int             n_heads,
                                    int             hidden,
                                    int             il,
                                    int             max_n_kv,
                                    int             B) {
    const int     head_dim = hidden / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t n_ctx    = kv_cache.n_ctx;
    const size_t  k_elem   = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem   = ggml_element_size(kv_cache.self_v);

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }
    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);
    if (k_b != nullptr) {
        Kcur = ggml_add(ctx, Kcur, k_b);
    }
    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, B);

    {
        const size_t  off_k = k_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
        const size_t  off_v = v_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, hidden, n_ctx, B, k_elem * hidden, k_elem * hidden * n_ctx, off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, hidden, n_ctx, B, v_elem * hidden, v_elem * hidden * n_ctx, off_v);
        ggml_tensor * K_row = ggml_reshape_3d(ctx, Kcur, hidden, 1, B);
        ggml_tensor * V_row = ggml_reshape_3d(ctx, Vcur, hidden, 1, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    const size_t  off_k = k_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
    const size_t  off_v = v_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
    ggml_tensor * K     = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, B, k_elem * hidden,
                                       k_elem * head_dim, k_elem * hidden * n_ctx, off_k);
    ggml_tensor * V     = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, B, v_elem * hidden,
                                       v_elem * head_dim, v_elem * hidden * n_ctx, off_v);

    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * o     = ggml_flash_attn_ext(ctx, Q_att, K, V, mask, scale, 0.0f, 0.0f);
    o                   = ggml_reshape_2d(ctx, o, hidden, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Batched cross-attention step. x: [hidden, B]; cross slab [hidden, T_enc_max, B].
ggml_tensor * mha_cross_step_batched(ggml_context *  ctx,
                                     ggml_tensor *   x,
                                     CanaryKvCache & kv_cache,
                                     ggml_tensor *   cross_mask,
                                     ggml_tensor *   q_w,
                                     ggml_tensor *   q_b,
                                     ggml_tensor *   out_w,
                                     ggml_tensor *   out_b,
                                     int             n_heads,
                                     int             hidden,
                                     int             il,
                                     int             T_enc_max,
                                     int             B) {
    const int    head_dim = hidden / n_heads;
    const float  scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t k_elem   = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem   = ggml_element_size(kv_cache.cross_v);

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }
    ggml_tensor * Q     = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, B);
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));

    const size_t  off_k = k_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
    const size_t  off_v = v_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
    ggml_tensor * K     = ggml_view_4d(ctx, kv_cache.cross_k, head_dim, T_enc_max, n_heads, B, k_elem * hidden,
                                       k_elem * head_dim, k_elem * hidden * T_enc_max, off_k);
    ggml_tensor * V     = ggml_view_4d(ctx, kv_cache.cross_v, head_dim, T_enc_max, n_heads, B, v_elem * hidden,
                                       v_elem * head_dim, v_elem * hidden * T_enc_max, off_v);

    ggml_tensor * o = ggml_flash_attn_ext(ctx, Q_att, K, V, cross_mask, scale, 0.0f, 0.0f);
    o               = ggml_reshape_2d(ctx, o, hidden, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

}  // namespace

DecoderBuild build_cross_kv_graph_batched(ggml_context *        ctx,
                                          const CanaryWeights & w,
                                          const CanaryHParams & hp,
                                          CanaryKvCache &       kv_cache,
                                          int                   T_enc_max,
                                          int                   n_batch) {
    DecoderBuild db{};
    if (ctx == nullptr || T_enc_max <= 0 || n_batch <= 0) {
        return db;
    }

    const int64_t hidden = hp.dec_d_model;
    const int     B      = n_batch;

    db.encoder_out_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, hidden, T_enc_max, B);
    ggml_set_name(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        return db;
    }

    const size_t k_elem = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem = ggml_element_size(kv_cache.cross_v);

    for (int il = 0; il < hp.dec_n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        ggml_tensor * Kc = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        if (blk.cross_k_b != nullptr) {
            Kc = ggml_add(ctx, Kc, blk.cross_k_b);
        }
        ggml_tensor * Vc = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);
        if (blk.cross_v_b != nullptr) {
            Vc = ggml_add(ctx, Vc, blk.cross_v_b);
        }

        const size_t  off_k = k_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
        const size_t  off_v = v_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
        ggml_tensor * k_dst = ggml_view_3d(ctx, kv_cache.cross_k, hidden, T_enc_max, B, k_elem * hidden,
                                           k_elem * hidden * T_enc_max, off_k);
        ggml_tensor * v_dst = ggml_view_3d(ctx, kv_cache.cross_v, hidden, T_enc_max, B, v_elem * hidden,
                                           v_elem * hidden * T_enc_max, off_v);
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Kc, k_dst));
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Vc, v_dst));
    }
    return db;
}

StepBuildBatched build_step_graph_batched(ggml_context *        ctx,
                                          const CanaryWeights & w,
                                          const CanaryHParams & hp,
                                          CanaryKvCache &       kv_cache,
                                          int                   max_n_kv,
                                          int                   T_enc_max,
                                          int                   n_batch,
                                          bool                  use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || T_enc_max <= 0 || n_batch <= 0) {
        return sb;
    }
    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary step(batched): requires flash path");
        return sb;
    }

    const int64_t hidden  = hp.dec_d_model;
    const int     n_heads = hp.dec_n_heads;
    const int     B       = n_batch;

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

    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, sb.token_ids_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, sb.pos_ids_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);
    x                     = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);

    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm1_w, blk.norm1_b);
            ggml_tensor * so =
                mha_self_step_batched(ctx, sb.graph, xn, kv_cache, sb.self_mask_in, sb.kv_idx_in, blk.self_q_w,
                                      blk.self_q_b, blk.self_k_w, blk.self_k_b, blk.self_v_w, blk.self_v_b,
                                      blk.self_o_w, blk.self_o_b, n_heads, static_cast<int>(hidden), i, max_n_kv, B);
            x = ggml_add(ctx, x, so);
        }
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm2_w, blk.norm2_b);
            ggml_tensor * co =
                mha_cross_step_batched(ctx, xn, kv_cache, sb.cross_mask_in, blk.cross_q_w, blk.cross_q_b, blk.cross_o_w,
                                       blk.cross_o_b, n_heads, static_cast<int>(hidden), i, T_enc_max, B);
            x = ggml_add(ctx, x, co);
        }
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm3_w, blk.norm3_b);
            ggml_tensor * ff = ggml_mul_mat(ctx, blk.ffn_up_w, xn);
            if (blk.ffn_up_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_up_b);
            }
            ff = (hp.dec_activation == "relu") ? ggml_relu(ctx, ff) : ggml_silu(ctx, ff);
            ff = ggml_mul_mat(ctx, blk.ffn_down_w, ff);
            if (blk.ffn_down_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ffn_down_b);
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    x                    = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);
    ggml_tensor * logits = ggml_mul_mat(ctx, w.head.weight, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    sb.argmax_out = ggml_argmax(ctx, logits);  // [B]
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);
    return sb;
}

}  // namespace transcribe::canary
