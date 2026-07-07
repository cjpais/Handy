// arch/cohere/decoder.cpp - Cohere ASR Transformer decoder graph builder.
//
// Graph builders: build_decoder_graph (non-cached prompt pass, kept for
// reference), build_cross_kv_graph (cross-attn K/V once per utterance),
// build_decoder_graph_kv (KV-cached, both prompt and step passes), plus the
// static-topology step graphs.
//
// Decoder forward pass:
//   1. token + positional embedding lookups + LayerNorm
//   2. per layer: Pre-LN self-attn / cross-attn / FFN, each with residual add
//   3. final LayerNorm
//   4. tied LM head (token embedding^T) + bias + optional log-softmax

#include "decoder.h"

#include "cohere.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::cohere {

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

// Standard multi-head attention (no relative position encoding).
// x:         [hidden, seq_len]
// context:   [hidden, T_ctx] (for cross-attn) or nullptr (for self-attn)
// mask:      additive mask in f16, or nullptr
// use_flash: true  -> ggml_flash_attn_ext (fused GPU kernel)
//            false -> manual mul_mat + soft_max_ext + mul_mat
// Returns:   [hidden, seq_len]
ggml_tensor * mha(ggml_context * ctx,
                  ggml_tensor *  x,
                  ggml_tensor *  context,
                  ggml_tensor *  mask,
                  ggml_tensor *  q_w,
                  ggml_tensor *  q_b,
                  ggml_tensor *  k_w,
                  ggml_tensor *  k_b,
                  ggml_tensor *  v_w,
                  ggml_tensor *  v_b,
                  ggml_tensor *  out_w,
                  ggml_tensor *  out_b,
                  int            n_heads,
                  int            hidden,
                  bool           use_flash) {
    const int     head_dim = hidden / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T_q      = x->ne[1];

    ggml_tensor * source = (context != nullptr) ? context : x;
    const int64_t T_k    = source->ne[1];

    // Q, K, V projections with bias.
    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        q = ggml_add(ctx, q, q_b);
    }
    ggml_tensor * k = ggml_mul_mat(ctx, k_w, source);
    if (k_b != nullptr) {
        k = ggml_add(ctx, k, k_b);
    }
    ggml_tensor * v = ggml_mul_mat(ctx, v_w, source);
    if (v_b != nullptr) {
        v = ggml_add(ctx, v, v_b);
    }

    // Split into heads: [head_dim, n_heads, T, 1] -> permute to
    // [head_dim, T, n_heads, 1] for flash_attn / standard.
    q = ggml_reshape_4d(ctx, q, head_dim, n_heads, T_q, 1);
    q = ggml_permute(ctx, q, 0, 2, 1, 3);

    k = ggml_reshape_4d(ctx, k, head_dim, n_heads, T_k, 1);
    k = ggml_permute(ctx, k, 0, 2, 1, 3);

    v = ggml_reshape_4d(ctx, v, head_dim, n_heads, T_k, 1);
    v = ggml_permute(ctx, v, 0, 2, 1, 3);

    ggml_tensor * o;
    if (use_flash) {
        // Flash attention path: fused Q @ K^T + mask + softmax + @V.
        o = ggml_flash_attn_ext(ctx, q, k, v, mask, scale, 0.0f, 0.0f);
        // [head_dim, n_heads, T_q, 1] -> [hidden, T_q]
        o = ggml_reshape_2d(ctx, o, hidden, T_q);
    } else {
        // Manual attention path: explicit matmuls + soft_max_ext.
        //   K is [head_dim, T_k, n_heads, 1] after the permute above.
        //   mul_mat(K, Q) computes Q^T @ K per head -> [T_k, T_q, n_heads, 1]
        ggml_tensor * kq = ggml_mul_mat(ctx, k, q);

        // soft_max_ext fuses scale + optional mask into softmax. Mask
        // shape must be [T_k, T_q] (or broadcastable); the caller
        // already prepared it as f16 at that shape.
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);

        // V needs transposing for the value matmul.
        //   V is [head_dim, T_k, n_heads, 1]; we need
        //        [T_k, head_dim, n_heads, 1].
        ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));

        // attn_weights @ V -> [head_dim, T_q, n_heads, 1]
        o = ggml_mul_mat(ctx, v_t, kq_soft);

        // Merge heads: permute [head_dim, T_q, n_heads] ->
        //                      [head_dim, n_heads, T_q], then reshape
        // to [hidden, T_q]. The permute makes heads contiguous for
        // the reshape, as in the encoder's manual path.
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
        o = ggml_reshape_2d(ctx, o, hidden, T_q);
    }

    // Output projection with bias.
    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Multi-head self-attention with KV cache.
//
// x:         [hidden, n_tokens] -- input for new tokens only
// kv_cache:  the pre-allocated KV cache
// mask:      [n_kv, n_tokens] additive mask (f16), or nullptr
// il:        layer index (for cache offset calculation)
// n_past:    number of tokens already in cache
// n_tokens:  number of new tokens being processed
// n_ctx:     max sequence length (cache size per layer)
//
// Writes new K/V into cache, reads full [0..n_past+n_tokens) for attn.
// Returns: [hidden, n_tokens]
ggml_tensor * mha_self_cached(ggml_context *  ctx,
                              ggml_cgraph *   gf,
                              ggml_tensor *   x,
                              CohereKvCache & kv_cache,
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

    // Q, K, V projections from new tokens.
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

    // Build Q for attention: [head_dim, n_tokens, n_heads].
    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);
    // Q is now [head_dim, n_tokens, n_heads, 1]

    // Write new K/V into cache. Layout: per-layer [hidden, n_ctx] contiguous
    // (n_state per position); layer il at flat offset il*n_ctx*hidden.
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

    // Read K from cache: [head_dim, n_kv, n_heads].
    ggml_tensor * K =
        ggml_view_3d(ctx, kv_cache.self_k, head_dim, n_kv, n_heads,
                     ggml_element_size(kv_cache.self_k) * hidden,    // nb1: stride between positions
                     ggml_element_size(kv_cache.self_k) * head_dim,  // nb2: stride between heads
                     ggml_element_size(kv_cache.self_k) *
                         static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));  // offset to layer

    // Read V from cache: [head_dim, n_kv, n_heads].
    ggml_tensor * V = ggml_view_3d(
        ctx, kv_cache.self_v, head_dim, n_kv, n_heads, ggml_element_size(kv_cache.self_v) * hidden,
        ggml_element_size(kv_cache.self_v) * head_dim,
        ggml_element_size(kv_cache.self_v) * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * hidden));

    // Flash path is fused; manual path is the mul_mat + soft_max_ext +
    // mul_mat triple (see `mha` above for the shape walkthrough).
    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    } else {
        // Manual path — see `mha` for the shape walkthrough.
        // K: [head_dim, n_kv, n_heads, 1]; V transposed for the value matmul.
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    }

    // Output projection.
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
                            CohereKvCache & kv_cache,
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

// Multi-head cross-attention reading from pre-computed KV cache.
//
// x:        [hidden, n_tokens] -- query input
// kv_cache: the cross-attention cache (already populated)
// il:       layer index
// T_enc:    number of encoder frames
//
// Returns: [hidden, n_tokens]
ggml_tensor * mha_cross_cached(ggml_context *  ctx,
                               ggml_tensor *   x,
                               CohereKvCache & kv_cache,
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

    // Only Q is computed from current input. K/V come from cache.
    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    // Read K from cross cache: [head_dim, T_enc, n_heads].
    ggml_tensor * K = ggml_view_3d(
        ctx, kv_cache.cross_k, head_dim, T_enc, n_heads, ggml_element_size(kv_cache.cross_k) * hidden,
        ggml_element_size(kv_cache.cross_k) * head_dim,
        ggml_element_size(kv_cache.cross_k) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

    // Read V from cross cache: [head_dim, T_enc, n_heads].
    ggml_tensor * V = ggml_view_3d(
        ctx, kv_cache.cross_v, head_dim, T_enc, n_heads, ggml_element_size(kv_cache.cross_v) * hidden,
        ggml_element_size(kv_cache.cross_v) * head_dim,
        ggml_element_size(kv_cache.cross_v) * static_cast<size_t>(static_cast<int64_t>(il) * T_enc * hidden));

    // No mask for cross-attention.
    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, nullptr, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    } else {
        // Manual path — see `mha` for the shape walkthrough.
        // K: [head_dim, T_enc, n_heads, 1]; V transposed for the value matmul.
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, hidden, n_tokens);
    }

    // Output projection.
    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

}  // namespace

// Original non-cached prompt-pass graph (kept for reference/validation).
DecoderBuild build_decoder_graph(ggml_context *        ctx,
                                 const CohereWeights & w,
                                 const CohereHParams & hp,
                                 int                   seq_len,
                                 int                   T_enc,
                                 bool                  use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || seq_len <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "cohere decoder: invalid arg "
                "(ctx=%p, seq_len=%d, T_enc=%d)",
                static_cast<void *>(ctx), seq_len, T_enc);
        return db;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int     n_heads = hp.dec_n_heads;

    // Input tensors.
    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, seq_len);
    ggml_set_name(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    db.pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, seq_len);
    ggml_set_name(db.pos_ids_in, "dec.pos_ids");
    ggml_set_input(db.pos_ids_in);

    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T_enc);
    ggml_set_name(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    // Token embedding: get_rows over [hidden, vocab_size] -> [hidden, seq_len].
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, db.token_ids_in);
    tok_emb               = named(tok_emb, "dec.token_emb");
    ggml_set_output(tok_emb);
    db.dumps.token_emb = tok_emb;

    // Positional embedding: get_rows over [hidden, max_seq].
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, db.pos_ids_in);
    pos_emb               = named(pos_emb, "dec.pos_emb");
    ggml_set_output(pos_emb);
    db.dumps.pos_emb = pos_emb;

    // x = LayerNorm(tok_emb + pos_emb)
    ggml_tensor * x = ggml_add(ctx, tok_emb, pos_emb);
    x               = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);
    x               = named(x, "dec.embed_norm");
    ggml_set_output(x);
    db.dumps.embed_norm = x;

    // Causal self-attention mask [T_k=seq_len, T_q=seq_len]: 0 if j <= i else
    // -inf. Driver fills the F32 input; cast to F16 for flash_attn.
    ggml_tensor * causal_mask = nullptr;
    if (seq_len > 1) {
        causal_mask = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, seq_len, seq_len);
        ggml_set_name(causal_mask, "dec.causal_mask");
        ggml_set_input(causal_mask);
        causal_mask = ggml_cast(ctx, causal_mask, GGML_TYPE_F16);
    }

    // Decoder layers.
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];

        // Pre-LN self-attention with causal mask.
        {
            ggml_tensor * x_norm   = layer_norm(ctx, x, blk.norm_self_w, blk.norm_self_b);
            ggml_tensor * self_out = mha(ctx, x_norm, nullptr, causal_mask, blk.self_q_w, blk.self_q_b, blk.self_k_w,
                                         blk.self_k_b, blk.self_v_w, blk.self_v_b, blk.self_out_w, blk.self_out_b,
                                         n_heads, static_cast<int>(hidden), use_flash);
            x                      = ggml_add(ctx, x, self_out);
        }

        // Pre-LN cross-attention (encoder output as K/V, no mask).
        {
            ggml_tensor * x_norm    = layer_norm(ctx, x, blk.norm_cross_w, blk.norm_cross_b);
            ggml_tensor * cross_out = mha(ctx, x_norm, db.encoder_out_in, nullptr, blk.cross_q_w, blk.cross_q_b,
                                          blk.cross_k_w, blk.cross_k_b, blk.cross_v_w, blk.cross_v_b, blk.cross_out_w,
                                          blk.cross_out_b, n_heads, static_cast<int>(hidden), use_flash);
            x                       = ggml_add(ctx, x, cross_out);
        }

        // Pre-LN FFN with ReLU (or silu).
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_ff_w, blk.norm_ff_b);
            ggml_tensor * ff     = ggml_mul_mat(ctx, blk.ff_in_w, x_norm);
            if (blk.ff_in_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_in_b);
            }
            if (hp.dec_activation == "relu") {
                ff = ggml_relu(ctx, ff);
            } else {
                ff = ggml_silu(ctx, ff);
            }
            ff = ggml_mul_mat(ctx, blk.ff_out_w, ff);
            if (blk.ff_out_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_out_b);
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    // Final LayerNorm.
    x                        = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);
    x                        = named(x, "dec.out_before_head");
    db.dumps.out_before_head = x;

    // Head: tied weight (the [hidden, vocab_size] token embedding) + bias +
    // log-softmax. mul_mat(token_w, x[hidden,seq]) -> [vocab_size, seq_len].
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_embed.token_w, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    if (hp.head_log_softmax) {
        logits = ggml_log(ctx, ggml_soft_max(ctx, logits));
    }
    logits          = named(logits, "dec.logits");
    db.dumps.logits = logits;

    db.out = logits;
    ggml_set_output(db.out);

    // Build the forward graph.
    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere decoder: ggml_new_graph_custom failed");
        return db;
    }
    ggml_build_forward_expand(db.graph, db.out);

    return db;
}

// Cross-attention KV computation graph.
DecoderBuild build_cross_kv_graph(ggml_context *        ctx,
                                  const CohereWeights & w,
                                  const CohereHParams & hp,
                                  CohereKvCache &       kv_cache,
                                  int                   T_enc) {
    DecoderBuild db{};

    if (ctx == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere cross_kv: invalid arg (ctx=%p, T_enc=%d)", static_cast<void *>(ctx),
                T_enc);
        return db;
    }

    const int64_t hidden = hp.dec_hidden;

    // Input: encoder output [hidden, T_enc].
    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T_enc);
    ggml_set_name(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere cross_kv: ggml_new_graph_custom failed");
        return db;
    }

    // For each decoder layer, compute K and V from encoder output,
    // then write into the cross-attention cache.
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

        // Write into cross cache. Layout: flat [hidden * T_enc] per layer.
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

// KV-cached decoder graph (prompt + step passes).
DecoderBuild build_decoder_graph_kv(ggml_context *        ctx,
                                    const CohereWeights & w,
                                    const CohereHParams & hp,
                                    CohereKvCache &       kv_cache,
                                    int                   n_tokens,
                                    int                   n_past,
                                    int                   T_enc,
                                    bool                  skip_log_softmax,
                                    bool                  use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || n_tokens <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "cohere decoder_kv: invalid arg "
                "(ctx=%p, n_tokens=%d, T_enc=%d)",
                static_cast<void *>(ctx), n_tokens, T_enc);
        return db;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int     n_heads = hp.dec_n_heads;
    const int     n_kv    = n_past + n_tokens;

    // Input tensors (only for new tokens).
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

    // x = LayerNorm(tok_emb + pos_emb)
    ggml_tensor * x = ggml_add(ctx, tok_emb, pos_emb);
    x               = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);
    x               = named(x, "dec.embed_norm");
    ggml_set_output(x);
    db.dumps.embed_norm = x;

    // Causal mask [n_kv, n_tokens]: mask[k, q] = 0 if k <= q else -inf. Only
    // the prompt pass (n_tokens > 1) needs it; a single step sees all n_kv.
    ggml_tensor * causal_mask = nullptr;
    if (n_tokens > 1) {
        causal_mask = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_kv, n_tokens);
        ggml_set_name(causal_mask, "dec.causal_mask");
        ggml_set_input(causal_mask);
        causal_mask = ggml_cast(ctx, causal_mask, GGML_TYPE_F16);
    }

    // Pre-create the graph: the self-attention cache writes call
    // ggml_build_forward_expand on it.
    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere decoder_kv: ggml_new_graph_custom failed");
        return db;
    }

    // Decoder layers.
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];

        // Pre-LN self-attention with KV cache.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_self_w, blk.norm_self_b);
            ggml_tensor * self_out =
                mha_self_cached(ctx, db.graph, x_norm, kv_cache, causal_mask, blk.self_q_w, blk.self_q_b, blk.self_k_w,
                                blk.self_k_b, blk.self_v_w, blk.self_v_b, blk.self_out_w, blk.self_out_b, n_heads,
                                static_cast<int>(hidden), i, n_past, n_tokens, use_flash);
            x = ggml_add(ctx, x, self_out);
        }

        // Pre-LN cross-attention reading from cached K/V.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_cross_w, blk.norm_cross_b);
            ggml_tensor * cross_out =
                mha_cross_cached(ctx, x_norm, kv_cache, blk.cross_q_w, blk.cross_q_b, blk.cross_out_w, blk.cross_out_b,
                                 n_heads, static_cast<int>(hidden), i, T_enc, use_flash);
            x = ggml_add(ctx, x, cross_out);
        }

        // Pre-LN FFN.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_ff_w, blk.norm_ff_b);
            ggml_tensor * ff     = ggml_mul_mat(ctx, blk.ff_in_w, x_norm);
            if (blk.ff_in_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_in_b);
            }
            if (hp.dec_activation == "relu") {
                ff = ggml_relu(ctx, ff);
            } else {
                ff = ggml_silu(ctx, ff);
            }
            ff = ggml_mul_mat(ctx, blk.ff_out_w, ff);
            if (blk.ff_out_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_out_b);
            }
            x = ggml_add(ctx, x, ff);
        }

        // Per-layer output dump.
        {
            char bname[32];
            std::snprintf(bname, sizeof(bname), "dec.block.%d.out", i);
            x = named(x, bname);
            ggml_set_output(x);
            db.dumps.block_out[i] = x;
        }
    }

    // Final LayerNorm.
    x                        = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);
    x                        = named(x, "dec.out_before_head");
    db.dumps.out_before_head = x;

    // Head: tied weight + bias + optional log-softmax.
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_embed.token_w, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    // Pre-softmax logits for numerically stable comparison.
    logits = named(logits, "dec.logits_raw");
    ggml_set_output(logits);
    db.dumps.logits_raw = logits;

    if (hp.head_log_softmax && !skip_log_softmax) {
        logits = ggml_log(ctx, ggml_soft_max(ctx, logits));
    }
    logits          = named(logits, "dec.logits");
    db.dumps.logits = logits;

    db.out = logits;
    ggml_set_output(db.out);

    // When skipping log_softmax, argmax on GPU so we read back a single int32
    // instead of the full logits tensor. For n_tokens > 1 only the last
    // position matters, so view its row before argmax.
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

// Static-topology single-token step graph (GPU dispatch path).
StepBuild build_step_graph(ggml_context *        ctx,
                           const CohereWeights & w,
                           const CohereHParams & hp,
                           CohereKvCache &       kv_cache,
                           int                   max_n_kv,
                           int                   T_enc,
                           bool                  use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere step: invalid arg (max_n_kv=%d, T_enc=%d)", max_n_kv, T_enc);
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int64_t hidden  = hp.dec_hidden;
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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere step: ggml_new_graph_custom failed");
        return sb;
    }

    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, sb.token_id_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, sb.pos_id_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);
    x                     = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);

    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];

        // Pre-LN self-attention with set_rows-based KV write.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_self_w, blk.norm_self_b);
            ggml_tensor * self_out =
                mha_self_step(ctx, sb.graph, x_norm, kv_cache, sb.mask_in, sb.kv_idx_in, blk.self_q_w, blk.self_q_b,
                              blk.self_k_w, blk.self_k_b, blk.self_v_w, blk.self_v_b, blk.self_out_w, blk.self_out_b,
                              n_heads, static_cast<int>(hidden), i, max_n_kv, use_flash);
            x = ggml_add(ctx, x, self_out);
        }

        // Pre-LN cross-attention. Unchanged from prompt path: cross_k/v
        // are pre-populated full T_enc, no n_past dependency.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_cross_w, blk.norm_cross_b);
            ggml_tensor * cross_out =
                mha_cross_cached(ctx, x_norm, kv_cache, blk.cross_q_w, blk.cross_q_b, blk.cross_out_w, blk.cross_out_b,
                                 n_heads, static_cast<int>(hidden), i, T_enc, use_flash);
            x = ggml_add(ctx, x, cross_out);
        }

        // Pre-LN FFN.
        {
            ggml_tensor * x_norm = layer_norm(ctx, x, blk.norm_ff_w, blk.norm_ff_b);
            ggml_tensor * ff     = ggml_mul_mat(ctx, blk.ff_in_w, x_norm);
            if (blk.ff_in_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_in_b);
            }
            if (hp.dec_activation == "relu") {
                ff = ggml_relu(ctx, ff);
            } else {
                ff = ggml_silu(ctx, ff);
            }
            ff = ggml_mul_mat(ctx, blk.ff_out_w, ff);
            if (blk.ff_out_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_out_b);
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    x = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);

    // Tied LM head: token embedding transposed.
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_embed.token_w, x);
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    sb.argmax_out = ggml_argmax(ctx, logits);
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);

    return sb;
}

// Offline batched decode (B utterances).
namespace {

// Batched self-attention step. x: [hidden, B] (one token per utterance).
// Mirrors mha_self_step but threads an utterance batch on the trailing
// flash axis (ne[3]); KV slab per (layer) is [hidden, n_ctx, B].
ggml_tensor * mha_self_step_batched(ggml_context *  ctx,
                                    ggml_cgraph *   gf,
                                    ggml_tensor *   x,
                                    CohereKvCache & kv_cache,
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

    // KV write: per-layer slab [hidden, n_ctx, B]; one set_rows writes B
    // rows at B independent indices kv_idx[b].
    {
        const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
        const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, hidden, n_ctx, B, k_elem * hidden, k_elem * hidden * n_ctx, layer_off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, hidden, n_ctx, B, v_elem * hidden, v_elem * hidden * n_ctx, layer_off_v);
        ggml_tensor * K_row = ggml_reshape_3d(ctx, Kcur, hidden, 1, B);
        ggml_tensor * V_row = ggml_reshape_3d(ctx, Vcur, hidden, 1, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
    const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * n_ctx * hidden * B;
    ggml_tensor * K           = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, B, k_elem * hidden,
                                             k_elem * head_dim, k_elem * hidden * n_ctx, layer_off_k);
    ggml_tensor * V           = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, B, v_elem * hidden,
                                             v_elem * head_dim, v_elem * hidden * n_ctx, layer_off_v);

    // Q [head_dim, n_heads, B] -> [head_dim, 1, n_heads, B] for flash.
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * o     = ggml_flash_attn_ext(ctx, Q_att, K, V, mask, scale, 0.0f, 0.0f);
    o                   = ggml_reshape_2d(ctx, o, hidden, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Batched cross-attention step. x: [hidden, B]; reads cross slab
// [hidden, T_enc_max, B] per layer with a per-utterance cross-pad mask.
ggml_tensor * mha_cross_step_batched(ggml_context *  ctx,
                                     ggml_tensor *   x,
                                     CohereKvCache & kv_cache,
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

    const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
    const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * T_enc_max * hidden * B;
    ggml_tensor * K           = ggml_view_4d(ctx, kv_cache.cross_k, head_dim, T_enc_max, n_heads, B, k_elem * hidden,
                                             k_elem * head_dim, k_elem * hidden * T_enc_max, layer_off_k);
    ggml_tensor * V           = ggml_view_4d(ctx, kv_cache.cross_v, head_dim, T_enc_max, n_heads, B, v_elem * hidden,
                                             v_elem * head_dim, v_elem * hidden * T_enc_max, layer_off_v);

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
                                          const CohereWeights & w,
                                          const CohereHParams & hp,
                                          CohereKvCache &       kv_cache,
                                          int                   T_enc_max,
                                          int                   n_batch) {
    DecoderBuild db{};
    if (ctx == nullptr || T_enc_max <= 0 || n_batch <= 0) {
        return db;
    }

    const int64_t hidden = hp.dec_hidden;
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
        // Kc/Vc: [hidden, T_enc_max, B].

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
                                          const CohereWeights & w,
                                          const CohereHParams & hp,
                                          CohereKvCache &       kv_cache,
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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere step(batched): requires flash path");
        return sb;
    }

    const int64_t hidden  = hp.dec_hidden;
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

    // Embedding: get_rows over B token ids -> [hidden, B].
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_embed.token_w, sb.token_ids_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_embed.pos_enc, sb.pos_ids_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);
    x                     = layer_norm(ctx, x, w.dec_embed.norm_w, w.dec_embed.norm_b);

    for (int i = 0; i < hp.dec_n_layers; ++i) {
        const auto & blk = w.dec_blocks[i];
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm_self_w, blk.norm_self_b);
            ggml_tensor * so = mha_self_step_batched(ctx, sb.graph, xn, kv_cache, sb.self_mask_in, sb.kv_idx_in,
                                                     blk.self_q_w, blk.self_q_b, blk.self_k_w, blk.self_k_b,
                                                     blk.self_v_w, blk.self_v_b, blk.self_out_w, blk.self_out_b,
                                                     n_heads, static_cast<int>(hidden), i, max_n_kv, B);
            x                = ggml_add(ctx, x, so);
        }
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm_cross_w, blk.norm_cross_b);
            ggml_tensor * co = mha_cross_step_batched(ctx, xn, kv_cache, sb.cross_mask_in, blk.cross_q_w, blk.cross_q_b,
                                                      blk.cross_out_w, blk.cross_out_b, n_heads,
                                                      static_cast<int>(hidden), i, T_enc_max, B);
            x                = ggml_add(ctx, x, co);
        }
        {
            ggml_tensor * xn = layer_norm(ctx, x, blk.norm_ff_w, blk.norm_ff_b);
            ggml_tensor * ff = ggml_mul_mat(ctx, blk.ff_in_w, xn);
            if (blk.ff_in_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_in_b);
            }
            ff = (hp.dec_activation == "relu") ? ggml_relu(ctx, ff) : ggml_silu(ctx, ff);
            ff = ggml_mul_mat(ctx, blk.ff_out_w, ff);
            if (blk.ff_out_b != nullptr) {
                ff = ggml_add(ctx, ff, blk.ff_out_b);
            }
            x = ggml_add(ctx, x, ff);
        }
    }

    x                    = layer_norm(ctx, x, w.dec_final.norm_w, w.dec_final.norm_b);
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_embed.token_w, x);  // [vocab, B]
    if (w.head.bias != nullptr) {
        logits = ggml_add(ctx, logits, w.head.bias);
    }

    sb.argmax_out = ggml_argmax(ctx, logits);  // [B]
    ggml_set_name(sb.argmax_out, "step.argmax");
    ggml_set_output(sb.argmax_out);
    ggml_build_forward_expand(sb.graph, sb.argmax_out);
    return sb;
}

}  // namespace transcribe::cohere
