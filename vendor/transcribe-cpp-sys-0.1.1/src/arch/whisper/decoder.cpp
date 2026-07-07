// arch/whisper/decoder.cpp - Whisper decoder graph builder.
//
// Mirrors the transformers reference (WhisperDecoderLayer):
//
//   x = embed_tokens(ids) + embed_positions(weight[0:seq_len])
//   for layer in decoder.layers:
//       x = x + self_attn(LN_self(x), causal_mask)
//       x = x + cross_attn(LN_cross(x), encoder_hidden)
//       x = x + ffn(LN_ffn(x))
//   x = layer_norm(x)
//   logits_raw = proj_out(x)              # tied: W = embed_tokens.weight
//   logits     = log_softmax(logits_raw)
//
// NO post-embed LayerNorm (unlike cohere). q/v/out carry bias, k does NOT
// (self and cross); the mha_* helpers take nullable bias slots.

#include "decoder.h"

#include "conformer/conformer.h"
#include "encoder.h"  // for transcribe::conformer namespace alias context
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"
#include "whisper.h"

#include <algorithm>
#include <cmath>
#include <cstdio>

namespace transcribe::whisper {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

// Standard multi-head attention used by both self- and cross-attention
// in the decoder. context==nullptr selects self-attention (Q/K/V from x);
// non-null context selects cross-attention (Q from x, K/V from context).
//
// x:        [d_model, T_q]
// context:  [d_model, T_k]   or nullptr for self-attention (T_k = T_q)
// mask:     additive mask (f16) shape [T_k, T_q] or nullptr
// Returns:  [d_model, T_q]
ggml_tensor * mha_decoder(ggml_context * ctx,
                          ggml_tensor *  x,
                          ggml_tensor *  context,
                          ggml_tensor *  mask,
                          ggml_tensor *  q_w,
                          ggml_tensor *  q_b,
                          ggml_tensor *  k_w,
                          ggml_tensor *  v_w,
                          ggml_tensor *  v_b,
                          ggml_tensor *  out_w,
                          ggml_tensor *  out_b,
                          int            n_heads,
                          int            d_model,
                          bool           use_flash) {
    const int     head_dim = d_model / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T_q      = x->ne[1];

    ggml_tensor * source = (context != nullptr) ? context : x;
    const int64_t T_k    = source->ne[1];

    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        q = ggml_add(ctx, q, q_b);
    }

    ggml_tensor * k = ggml_mul_mat(ctx, k_w, source);  // k has no bias

    ggml_tensor * v = ggml_mul_mat(ctx, v_w, source);
    if (v_b != nullptr) {
        v = ggml_add(ctx, v, v_b);
    }

    // [d_model, T] -> [head_dim, n_heads, T] -> permute to
    // [head_dim, T, n_heads] for both flash and manual paths.
    q = ggml_reshape_3d(ctx, q, head_dim, n_heads, T_q);
    q = ggml_permute(ctx, q, 0, 2, 1, 3);

    k = ggml_reshape_3d(ctx, k, head_dim, n_heads, T_k);
    k = ggml_permute(ctx, k, 0, 2, 1, 3);

    v = ggml_reshape_3d(ctx, v, head_dim, n_heads, T_k);
    v = ggml_permute(ctx, v, 0, 2, 1, 3);

    ggml_tensor * o;
    if (use_flash) {
        ggml_tensor * q_c = ggml_cont(ctx, q);
        ggml_tensor * k_c = ggml_cont(ctx, k);
        ggml_tensor * v_c = ggml_cont(ctx, v);
        o                 = ggml_flash_attn_ext(ctx, q_c, k_c, v_c, mask, scale, 0.0f, 0.0f);
        o                 = ggml_reshape_2d(ctx, o, d_model, T_q);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, ggml_cont(ctx, k), ggml_cont(ctx, q));
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);

        ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                 = ggml_mul_mat(ctx, v_t, kq_soft);

        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
        o = ggml_reshape_2d(ctx, o, d_model, T_q);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// FFN: fc1 -> GELU -> fc2. Both projections carry bias.
ggml_tensor * ffn(ggml_context * ctx,
                  ggml_tensor *  x,
                  ggml_tensor *  fc1_w,
                  ggml_tensor *  fc1_b,
                  ggml_tensor *  fc2_w,
                  ggml_tensor *  fc2_b) {
    ggml_tensor * h = ggml_mul_mat(ctx, fc1_w, x);
    if (fc1_b != nullptr) {
        h = ggml_add(ctx, h, fc1_b);
    }
    // HF Whisper uses nn.functional.gelu (exact erf form); ggml_gelu
    // is the tanh approximation and drifts ~1e-4 per element.
    h               = ggml_gelu_erf(ctx, h);
    ggml_tensor * o = ggml_mul_mat(ctx, fc2_w, h);
    if (fc2_b != nullptr) {
        o = ggml_add(ctx, o, fc2_b);
    }
    return o;
}

ggml_tensor * build_block(ggml_context *          ctx,
                          ggml_tensor *           x,
                          ggml_tensor *           encoder_hidden,
                          ggml_tensor *           causal_mask,
                          const WhisperDecBlock & b,
                          int                     n_heads,
                          int                     d_model,
                          bool                    use_flash) {
    // Self-attention (causal).
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, b.norm_self_b);
        y = mha_decoder(ctx, y, /*context=*/nullptr, causal_mask, b.self_q_w, b.self_q_b, b.self_k_w, b.self_v_w,
                        b.self_v_b, b.self_out_w, b.self_out_b, n_heads, d_model, use_flash);
        x = ggml_add(ctx, x, y);
    }
    // Cross-attention (no mask; queries decoder state against encoder).
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, b.norm_cross_b);
        y = mha_decoder(ctx, y, encoder_hidden, /*mask=*/nullptr, b.cross_q_w, b.cross_q_b, b.cross_k_w, b.cross_v_w,
                        b.cross_v_b, b.cross_out_w, b.cross_out_b, n_heads, d_model, use_flash);
        x = ggml_add(ctx, x, y);
    }
    // FFN.
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
        y               = ffn(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
        x               = ggml_add(ctx, x, y);
    }
    return x;
}

// KV-cached helpers.
//
// Cache layout (flat 1D tensors, one slot per layer, per role):
//   self_k / self_v   : [hidden * n_layer * n_ctx]
//       layer il offset:  il * n_ctx * hidden
//       position n slot:  il * n_ctx * hidden + n * hidden
//   cross_k / cross_v : [hidden * n_layer * T_enc]
//       layer il offset:  il * T_enc * hidden

// Self-attention reading/writing the self KV cache.
//
// x:         [d_model, n_tokens] input for new tokens only
// mask:      [n_kv, n_tokens] f16 additive mask, or nullptr
// n_past:    tokens already in cache; n_tokens: new tokens this call
//
// Writes new K/V into the cache at [il, n_past..n_past+n_tokens), then reads
// the full [il, 0..n_past+n_tokens) window. Returns [d_model, n_tokens].
ggml_tensor * mha_self_cached(ggml_context *   ctx,
                              ggml_cgraph *    gf,
                              ggml_tensor *    x,
                              WhisperKvCache & kv_cache,
                              ggml_tensor *    mask,
                              ggml_tensor *    q_w,
                              ggml_tensor *    q_b,
                              ggml_tensor *    k_w,
                              ggml_tensor *    v_w,
                              ggml_tensor *    v_b,
                              ggml_tensor *    out_w,
                              ggml_tensor *    out_b,
                              int              n_heads,
                              int              d_model,
                              int              il,
                              int              n_past,
                              int              n_tokens,
                              int              n_kv,
                              bool             use_flash) {
    const int   head_dim = d_model / n_heads;
    const float scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int   n_ctx    = kv_cache.n_ctx;
    // n_kv may be padded beyond n_past + n_tokens (FA-friendly multiple);
    // trailing slots are masked to -inf. The cache write still touches only
    // [n_past, n_past+n_tokens).

    // Q / K / V projections for the new tokens only.
    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);

    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    // Q in [head_dim, n_tokens, n_heads] for flash / manual paths.
    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    // Write new K/V into the cache. Layout is [hidden, n_ctx] per layer,
    // stored as a flat 1D buffer. Layer il occupies
    //   offset = il * n_ctx * hidden  (in elements).
    {
        const size_t k_elem = ggml_element_size(kv_cache.self_k);
        const size_t v_elem = ggml_element_size(kv_cache.self_v);

        ggml_tensor * k_dst = ggml_view_1d(ctx, kv_cache.self_k, static_cast<int64_t>(n_tokens) * d_model,
                                           k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model +
                                                                        static_cast<int64_t>(n_past) * d_model));

        ggml_tensor * v_dst = ggml_view_1d(ctx, kv_cache.self_v, static_cast<int64_t>(n_tokens) * d_model,
                                           v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model +
                                                                        static_cast<int64_t>(n_past) * d_model));

        ggml_build_forward_expand(gf, ggml_cpy(ctx, Kcur, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, Vcur, v_dst));
    }

    // Read K / V as [head_dim, n_kv, n_heads] for attention.
    const size_t  k_elem = ggml_element_size(kv_cache.self_k);
    ggml_tensor * K      = ggml_view_3d(ctx, kv_cache.self_k, head_dim, n_kv, n_heads,
                                        k_elem * d_model,   // nb1: stride between positions (one full d_model row)
                                        k_elem * head_dim,  // nb2: stride between heads inside a position
                                        k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.self_v);
    ggml_tensor * V = ggml_view_3d(ctx, kv_cache.self_v, head_dim, n_kv, n_heads, v_elem * d_model, v_elem * head_dim,
                                   v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, d_model, n_tokens);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, d_model, n_tokens);
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
ggml_tensor * mha_self_step(ggml_context *   ctx,
                            ggml_cgraph *    gf,
                            ggml_tensor *    x,
                            WhisperKvCache & kv_cache,
                            ggml_tensor *    mask,
                            ggml_tensor *    kv_idx,
                            ggml_tensor *    q_w,
                            ggml_tensor *    q_b,
                            ggml_tensor *    k_w,
                            ggml_tensor *    v_w,
                            ggml_tensor *    v_b,
                            ggml_tensor *    out_w,
                            ggml_tensor *    out_b,
                            int              n_heads,
                            int              d_model,
                            int              il,
                            int              max_n_kv,
                            bool             use_flash) {
    const int   head_dim = d_model / n_heads;
    const float scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int   n_ctx    = kv_cache.n_ctx;

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);

    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, /*n_tokens=*/1);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    // KV cache write via ggml_set_rows. Per-layer view spans [d_model, n_ctx];
    // we write one [d_model, 1] row at the kv_idx position.
    {
        const size_t k_elem      = ggml_element_size(kv_cache.self_k);
        const size_t v_elem      = ggml_element_size(kv_cache.self_v);
        const size_t layer_off_k = k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model);
        const size_t layer_off_v = v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model);

        ggml_tensor * k_layer = ggml_view_2d(ctx, kv_cache.self_k, d_model, n_ctx, k_elem * d_model, layer_off_k);
        ggml_tensor * v_layer = ggml_view_2d(ctx, kv_cache.self_v, d_model, n_ctx, v_elem * d_model, layer_off_v);

        ggml_tensor * K_row = ggml_reshape_2d(ctx, Kcur, d_model, 1);
        ggml_tensor * V_row = ggml_reshape_2d(ctx, Vcur, d_model, 1);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    // Static read across [0, max_n_kv).
    const size_t  k_elem = ggml_element_size(kv_cache.self_k);
    ggml_tensor * K =
        ggml_view_3d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, k_elem * d_model, k_elem * head_dim,
                     k_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.self_v);
    ggml_tensor * V =
        ggml_view_3d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, v_elem * d_model, v_elem * head_dim,
                     v_elem * static_cast<size_t>(static_cast<int64_t>(il) * n_ctx * d_model));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, d_model, 1);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, d_model, 1);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Cross-attention reading the pre-populated cross KV cache.
// x: [d_model, n_tokens] query input -> returns [d_model, n_tokens].
ggml_tensor * mha_cross_cached(ggml_context *   ctx,
                               ggml_tensor *    x,
                               WhisperKvCache & kv_cache,
                               ggml_tensor *    mask,
                               ggml_tensor *    q_w,
                               ggml_tensor *    q_b,
                               ggml_tensor *    out_w,
                               ggml_tensor *    out_b,
                               int              n_heads,
                               int              d_model,
                               int              il,
                               int              T_enc,
                               bool             use_flash) {
    const int     head_dim = d_model / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t n_tokens = x->ne[1];

    // Cross cache is T_enc_pad rows per layer; trailing rows hold zeros (never
    // written). FA reads the padded shape — kernel-friendly seq dim at the cost
    // of small dilution unless the caller masks the padded slots.
    const int T_enc_pad = kv_cache.T_enc_pad > 0 ? kv_cache.T_enc_pad : T_enc;
    (void) T_enc;

    // Q from current input; K / V read from the cross cache.
    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, n_tokens);
    Q               = ggml_permute(ctx, Q, 0, 2, 1, 3);

    const size_t  k_elem = ggml_element_size(kv_cache.cross_k);
    ggml_tensor * K =
        ggml_view_3d(ctx, kv_cache.cross_k, head_dim, T_enc_pad, n_heads, k_elem * d_model, k_elem * head_dim,
                     k_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc_pad * d_model));

    const size_t  v_elem = ggml_element_size(kv_cache.cross_v);
    ggml_tensor * V =
        ggml_view_3d(ctx, kv_cache.cross_v, head_dim, T_enc_pad, n_heads, v_elem * d_model, v_elem * head_dim,
                     v_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc_pad * d_model));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q, K, V, mask, scale, 0.0f, 0.0f);
        o = ggml_reshape_2d(ctx, o, d_model, n_tokens);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, K, Q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, V, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_permute(ctx, o, 0, 2, 1, 3);
        o                     = ggml_cont(ctx, o);
        o                     = ggml_reshape_2d(ctx, o, d_model, n_tokens);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

}  // namespace

DecoderBuild build_decoder_prefill_graph(ggml_context *         ctx,
                                         const WhisperWeights & w,
                                         const WhisperHParams & hp,
                                         int                    seq_len,
                                         int                    T_enc,
                                         bool                   use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || seq_len <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper decoder: invalid arg "
                "(ctx=%p, seq_len=%d, T_enc=%d)",
                static_cast<void *>(ctx), seq_len, T_enc);
        return db;
    }
    if (seq_len > hp.dec_max_target_positions) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper decoder: seq_len=%d exceeds "
                "max_target_positions=%d",
                seq_len, hp.dec_max_target_positions);
        return db;
    }

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;
    const int vocab   = hp.dec_vocab_size;

    // ---- Inputs ------------------------------------------------------
    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, seq_len);
    named(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    db.encoder_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_enc);
    named(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    // Causal mask declared F32, cast to F16 inside (flash_attn_ext /
    // soft_max_ext want f16).
    ggml_tensor * causal_mask = nullptr;
    if (seq_len > 1) {
        db.causal_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, seq_len, seq_len);
        named(db.causal_mask_in, "dec.causal_mask");
        ggml_set_input(db.causal_mask_in);
        causal_mask = ggml_cast(ctx, db.causal_mask_in, GGML_TYPE_F16);
    }

    // ---- Token embedding ----
    // get_rows(token_embd_w [d_model, vocab], ids [seq_len]) -> [d_model, seq_len].
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, db.token_ids_in);
    named(tok_emb, "dec.token_emb");
    transcribe::debug::mark_tensor_for_dump(tok_emb);
    db.dumps.token_emb = tok_emb;

    // ---- Positional embedding ----
    // Reference takes pos_emb_w[past:past+seq_len]; prefill is past=0, so a 2D
    // view of the leading seq_len rows is the same data (no pos-id input).
    ggml_tensor * pos_emb = w.dec_top.pos_emb_w;
    if (seq_len != hp.dec_max_target_positions) {
        pos_emb = ggml_view_2d(ctx, w.dec_top.pos_emb_w, d_model, seq_len, w.dec_top.pos_emb_w->nb[1], 0);
    }
    named(pos_emb, "dec.pos_emb");
    transcribe::debug::mark_tensor_for_dump(pos_emb);
    db.dumps.pos_emb = pos_emb;

    // ---- Embedding sum (no LayerNorm) ----
    ggml_tensor * x = ggml_add(ctx, tok_emb, pos_emb);
    named(x, "dec.embed_sum");
    transcribe::debug::mark_tensor_for_dump(x);
    db.dumps.embed_sum = x;

    // ---- Decoder blocks ----
    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    db.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        x = build_block(ctx, x, db.encoder_out_in, causal_mask, w.dec_blocks[i], n_heads, d_model, use_flash);

        char bname[64];
        std::snprintf(bname, sizeof(bname), "dec.block.%d.out", i);
        named(x, bname);
        transcribe::debug::mark_tensor_for_dump(x);
        db.dumps.block_outs.push_back(x);
    }

    // ---- Final LayerNorm ----
    x = layer_norm(ctx, x, w.dec_top.final_norm_w, w.dec_top.final_norm_b);
    named(x, "dec.out_before_head");
    transcribe::debug::mark_tensor_for_dump(x);
    db.dumps.out_before_head = x;

    // ---- Logits head (tied weight, no bias) ----
    // mul_mat(W [d_model, vocab], x [d_model, seq_len]) -> [vocab, seq_len].
    ggml_tensor * logits_raw = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);
    named(logits_raw, "dec.logits_raw");
    transcribe::debug::mark_tensor_for_dump(logits_raw);
    db.dumps.logits_raw = logits_raw;

    ggml_tensor * logits = ggml_log(ctx, ggml_soft_max(ctx, logits_raw));
    named(logits, "dec.logits");
    transcribe::debug::mark_tensor_for_dump(logits);
    db.dumps.logits = logits;

    db.out = logits;
    ggml_set_output(db.out);

    // 8192 leaves headroom up to large-v3 (32 blocks, ~20 ops each).
    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper decoder: ggml_new_graph_custom failed");
        return db;
    }
    ggml_build_forward_expand(db.graph, db.out);

    (void) vocab;

    return db;
}

// KV-cached path: cross-KV precompute + prompt/step decoder graph.

DecoderBuild build_cross_kv_graph(ggml_context *         ctx,
                                  const WhisperWeights & w,
                                  const WhisperHParams & hp,
                                  WhisperKvCache &       kv_cache,
                                  ggml_tensor *          encoder_out,
                                  int                    T_enc) {
    DecoderBuild db{};

    if (ctx == nullptr || encoder_out == nullptr || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper cross_kv: invalid arg "
                "(ctx=%p, encoder_out=%p, T_enc=%d)",
                static_cast<void *>(ctx), static_cast<void *>(encoder_out), T_enc);
        return db;
    }

    const int d_model = hp.dec_d_model;

    // View the persistent encoder_out tensor into this compute_ctx (no copy;
    // shares storage with the encoder-populated source on the same backend).
    db.encoder_out_in = ggml_view_2d(ctx, encoder_out, d_model, T_enc, encoder_out->nb[1], 0);
    named(db.encoder_out_in, "dec.encoder_out");

    db.graph = ggml_new_graph_custom(ctx, 4096, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper cross_kv: ggml_new_graph_custom failed");
        return db;
    }

    // Layer slots are spaced by T_enc_pad (allocator size) but each writes
    // only T_enc rows; trailing T_enc_pad - T_enc rows stay zero.
    const int T_enc_pad = kv_cache.T_enc_pad > 0 ? kv_cache.T_enc_pad : T_enc;

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    for (int il = 0; il < n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        // Cross k has no bias; cross v does.
        ggml_tensor * Kcross = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        ggml_tensor * Vcross = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);
        if (blk.cross_v_b != nullptr) {
            Vcross = ggml_add(ctx, Vcross, blk.cross_v_b);
        }

        // Write [d_model, T_enc] into this layer's slot at element offset
        // il * T_enc_pad * d_model (slots don't overlap).
        const size_t k_elem = ggml_element_size(kv_cache.cross_k);
        const size_t v_elem = ggml_element_size(kv_cache.cross_v);

        ggml_tensor * k_dst =
            ggml_view_1d(ctx, kv_cache.cross_k, static_cast<int64_t>(T_enc) * d_model,
                         k_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc_pad * d_model));

        ggml_tensor * v_dst =
            ggml_view_1d(ctx, kv_cache.cross_v, static_cast<int64_t>(T_enc) * d_model,
                         v_elem * static_cast<size_t>(static_cast<int64_t>(il) * T_enc_pad * d_model));

        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Kcross, k_dst));
        ggml_build_forward_expand(db.graph, ggml_cpy(ctx, Vcross, v_dst));
    }

    return db;
}

DecoderBuild build_decoder_graph_kv(ggml_context *         ctx,
                                    const WhisperWeights & w,
                                    const WhisperHParams & hp,
                                    WhisperKvCache &       kv_cache,
                                    int                    n_tokens,
                                    int                    n_past,
                                    int                    T_enc,
                                    int                    kv_pad,
                                    bool                   skip_log_softmax,
                                    bool                   use_flash) {
    DecoderBuild db{};

    if (ctx == nullptr || n_tokens <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper decoder_kv: invalid arg "
                "(ctx=%p, n_tokens=%d, T_enc=%d)",
                static_cast<void *>(ctx), n_tokens, T_enc);
        return db;
    }
    const int n_kv_active = n_past + n_tokens;
    if (n_kv_active > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper decoder_kv: n_kv=%d exceeds n_ctx=%d", n_kv_active,
                kv_cache.n_ctx);
        return db;
    }

    // Pad the K/V view length to a FA-friendly multiple. Trailing slots in
    // [n_kv_active, n_kv) hold zeros or stale K/V; the mask zeroes them.
    // Clamp to n_ctx so we don't read past the allocation.
    const int kv_pad_eff = kv_pad > 0 ? kv_pad : 1;
    int       n_kv       = n_kv_active;
    if (kv_pad_eff > 1) {
        n_kv = std::max(kv_pad_eff, static_cast<int>(GGML_PAD(n_kv_active, kv_pad_eff)));
        if (n_kv > kv_cache.n_ctx) {
            n_kv = kv_cache.n_ctx;
        }
    }
    const bool has_padding = n_kv > n_kv_active;

    const int d_model = hp.dec_d_model;
    const int n_heads = hp.dec_n_heads;

    // ---- Inputs -----------------------------------------------------
    db.token_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(db.token_ids_in, "dec.token_ids");
    ggml_set_input(db.token_ids_in);

    // Position ids as an input so the step loop uploads absolute positions
    // without rebuilding the graph (prompt pass uploads [0..n_tokens)).
    ggml_tensor * pos_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_tokens);
    named(pos_ids_in, "dec.pos_ids");
    ggml_set_input(pos_ids_in);

    // Self-attention mask [n_kv, n_tokens], covering both causality (prompt
    // pass, n_tokens > 1) and padding (kv_pad > 1): -inf for k >= n_kv_active
    // and for k > q + n_past. Omitted on the unpadded single-token step (it
    // attends the full real cache window); model.cpp fills the values.
    ggml_tensor * causal_mask = nullptr;
    const bool    need_mask   = (n_tokens > 1) || has_padding;
    if (need_mask) {
        db.causal_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_kv, n_tokens);
        named(db.causal_mask_in, "dec.causal_mask");
        ggml_set_input(db.causal_mask_in);
        causal_mask = ggml_cast(ctx, db.causal_mask_in, GGML_TYPE_F16);
    } else {
        GGML_ASSERT(n_tokens == 1 && "decoder_kv mask-null branch requires single-token step");
    }

    // Cross-attention mask, only when the cross cache is padded (T_enc_pad >
    // T_enc): the trailing zero-K/V slots would dilute the FA softmax, so -inf
    // there restores equivalence to an unpadded view.
    ggml_tensor * cross_mask = nullptr;
    const int     T_enc_pad  = kv_cache.T_enc_pad > 0 ? kv_cache.T_enc_pad : T_enc;
    if (T_enc_pad > T_enc) {
        db.cross_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, T_enc_pad, n_tokens);
        named(db.cross_mask_in, "dec.cross_mask");
        ggml_set_input(db.cross_mask_in);
        cross_mask = ggml_cast(ctx, db.cross_mask_in, GGML_TYPE_F16);
    }

    // ---- Token embedding ----
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, db.token_ids_in);
    named(tok_emb, "dec.token_emb");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(tok_emb);
        db.dumps.token_emb = tok_emb;
    }

    // ---- Positional embedding ----
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_top.pos_emb_w, pos_ids_in);
    named(pos_emb, "dec.pos_emb");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(pos_emb);
        db.dumps.pos_emb = pos_emb;
    }

    // ---- Embed sum (no LayerNorm) ----
    ggml_tensor * x = ggml_add(ctx, tok_emb, pos_emb);
    named(x, "dec.embed_sum");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(x);
        db.dumps.embed_sum = x;
    }

    // Pre-create the graph so mha_self_cached can wire cache writes into it.
    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper decoder_kv: ggml_new_graph_custom failed");
        return db;
    }

    // ---- Decoder blocks ----
    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    db.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];

        // Self-attention (pre-LN, causal via mask when prompt pass).
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, b.norm_self_b);
            y = mha_self_cached(ctx, db.graph, y, kv_cache, causal_mask, b.self_q_w, b.self_q_b, b.self_k_w, b.self_v_w,
                                b.self_v_b, b.self_out_w, b.self_out_b, n_heads, d_model, i, n_past, n_tokens, n_kv,
                                use_flash);
            x = ggml_add(ctx, x, y);
        }
        // Cross-attention (pre-LN, reads cross cache).
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, b.norm_cross_b);
            y = mha_cross_cached(ctx, y, kv_cache, cross_mask, b.cross_q_w, b.cross_q_b, b.cross_out_w, b.cross_out_b,
                                 n_heads, d_model, i, T_enc, use_flash);
            x = ggml_add(ctx, x, y);
        }
        // FFN.
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
            y               = ffn(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
            x               = ggml_add(ctx, x, y);
        }

        char bname[64];
        std::snprintf(bname, sizeof(bname), "dec.block.%d.out", i);
        named(x, bname);
        if (n_past == 0) {
            transcribe::debug::mark_tensor_for_dump(x);
            db.dumps.block_outs.push_back(x);
        }
    }

    // ---- Final LayerNorm ----
    x = layer_norm(ctx, x, w.dec_top.final_norm_w, w.dec_top.final_norm_b);
    named(x, "dec.out_before_head");
    if (n_past == 0) {
        transcribe::debug::mark_tensor_for_dump(x);
        db.dumps.out_before_head = x;
    }

    // ---- Logits head (tied weight, no bias) ----
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
    ggml_build_forward_expand(db.graph, db.out);

    return db;
}

// Static-topology single-token step graph (GPU dispatch path).
StepBuild build_step_graph(ggml_context *         ctx,
                           const WhisperWeights & w,
                           const WhisperHParams & hp,
                           WhisperKvCache &       kv_cache,
                           int                    max_n_kv,
                           int                    T_enc,
                           bool                   use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper step: invalid arg (max_n_kv=%d, T_enc=%d)", max_n_kv, T_enc);
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int  d_model         = hp.dec_d_model;
    const int  n_heads         = hp.dec_n_heads;
    const int  T_enc_pad       = kv_cache.T_enc_pad > 0 ? kv_cache.T_enc_pad : T_enc;
    const bool need_cross_mask = (T_enc_pad > T_enc);

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

    if (need_cross_mask) {
        sb.cross_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, T_enc_pad, 1);
        ggml_set_name(sb.cross_mask_in, "step.cross_mask");
        ggml_set_input(sb.cross_mask_in);
    }
    ggml_tensor * cross_mask_f16 = need_cross_mask ? ggml_cast(ctx, sb.cross_mask_in, GGML_TYPE_F16) : nullptr;

    sb.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (sb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper step: ggml_new_graph_custom failed");
        return sb;
    }

    // Token + positional embedding (no post-embed LayerNorm).
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, sb.token_id_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_top.pos_emb_w, sb.pos_id_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);

    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];

        // Self-attention (pre-LN, set_rows-based KV write).
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_self_w, b.norm_self_b);
            y = mha_self_step(ctx, sb.graph, y, kv_cache, sb.mask_in, sb.kv_idx_in, b.self_q_w, b.self_q_b, b.self_k_w,
                              b.self_v_w, b.self_v_b, b.self_out_w, b.self_out_b, n_heads, d_model, i, max_n_kv,
                              use_flash);
            x = ggml_add(ctx, x, y);
        }
        // Cross-attention (pre-LN, reads pre-populated cross cache).
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_cross_w, b.norm_cross_b);
            y = mha_cross_cached(ctx, y, kv_cache, cross_mask_f16, b.cross_q_w, b.cross_q_b, b.cross_out_w,
                                 b.cross_out_b, n_heads, d_model, i, T_enc, use_flash);
            x = ggml_add(ctx, x, y);
        }
        // FFN (pre-LN, GELU).
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
            y               = ffn(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
            x               = ggml_add(ctx, x, y);
        }
    }

    x = layer_norm(ctx, x, w.dec_top.final_norm_w, w.dec_top.final_norm_b);

    // Tied lm_head, no bias. Output: raw pre-softmax logits.
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);
    ggml_set_name(logits, "step.logits_raw");
    ggml_set_output(logits);
    sb.logits_out = logits;
    ggml_build_forward_expand(sb.graph, sb.logits_out);

    return sb;
}

// Offline batched decode (B utterances).
namespace {

// Batched self-attention step. x: [d_model, B] (one token per utterance).
// Mirrors mha_self_step but threads the utterance batch on the trailing
// flash axis (ne[3]); KV slab per layer is [d_model, n_ctx, B]. Flash-only.
ggml_tensor * mha_self_step_batched(ggml_context *   ctx,
                                    ggml_cgraph *    gf,
                                    ggml_tensor *    x,
                                    WhisperKvCache & kv_cache,
                                    ggml_tensor *    mask,
                                    ggml_tensor *    kv_idx,
                                    ggml_tensor *    q_w,
                                    ggml_tensor *    q_b,
                                    ggml_tensor *    k_w,
                                    ggml_tensor *    v_w,
                                    ggml_tensor *    v_b,
                                    ggml_tensor *    out_w,
                                    ggml_tensor *    out_b,
                                    int              n_heads,
                                    int              d_model,
                                    int              il,
                                    int              max_n_kv,
                                    int              B) {
    const int     head_dim = d_model / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t n_ctx    = kv_cache.n_ctx;
    const size_t  k_elem   = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem   = ggml_element_size(kv_cache.self_v);

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }
    ggml_tensor * Kcur = ggml_mul_mat(ctx, k_w, x);  // k has no bias
    ggml_tensor * Vcur = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        Vcur = ggml_add(ctx, Vcur, v_b);
    }

    ggml_tensor * Q = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, B);

    // KV write: per-layer slab [d_model, n_ctx, B]; one set_rows writes B
    // rows at B independent indices kv_idx[b].
    {
        const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
        const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
        ggml_tensor * k_layer     = ggml_view_3d(ctx, kv_cache.self_k, d_model, n_ctx, B, k_elem * d_model,
                                                 k_elem * d_model * n_ctx, layer_off_k);
        ggml_tensor * v_layer     = ggml_view_3d(ctx, kv_cache.self_v, d_model, n_ctx, B, v_elem * d_model,
                                                 v_elem * d_model * n_ctx, layer_off_v);
        ggml_tensor * K_row       = ggml_reshape_3d(ctx, Kcur, d_model, 1, B);
        ggml_tensor * V_row       = ggml_reshape_3d(ctx, Vcur, d_model, 1, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
    const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * n_ctx * d_model * B;
    ggml_tensor * K           = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_heads, B, k_elem * d_model,
                                             k_elem * head_dim, k_elem * d_model * n_ctx, layer_off_k);
    ggml_tensor * V           = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_heads, B, v_elem * d_model,
                                             v_elem * head_dim, v_elem * d_model * n_ctx, layer_off_v);

    // Q [head_dim, n_heads, B] -> [head_dim, 1, n_heads, B] for flash.
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * o     = ggml_flash_attn_ext(ctx, Q_att, K, V, mask, scale, 0.0f, 0.0f);
    o                   = ggml_reshape_2d(ctx, o, d_model, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// Batched cross-attention step. x: [d_model, B]; reads cross slab
// [d_model, T_enc_max, B] per layer with a per-utterance cross-pad mask.
// Whisper cross: q/out carry bias; k/v already live in the cache.
ggml_tensor * mha_cross_step_batched(ggml_context *   ctx,
                                     ggml_tensor *    x,
                                     WhisperKvCache & kv_cache,
                                     ggml_tensor *    cross_mask,
                                     ggml_tensor *    q_w,
                                     ggml_tensor *    q_b,
                                     ggml_tensor *    out_w,
                                     ggml_tensor *    out_b,
                                     int              n_heads,
                                     int              d_model,
                                     int              il,
                                     int              T_enc_max,
                                     int              B) {
    const int    head_dim = d_model / n_heads;
    const float  scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const size_t k_elem   = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem   = ggml_element_size(kv_cache.cross_v);

    ggml_tensor * Qcur = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        Qcur = ggml_add(ctx, Qcur, q_b);
    }
    ggml_tensor * Q     = ggml_reshape_3d(ctx, Qcur, head_dim, n_heads, B);
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));

    const size_t  layer_off_k = k_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
    const size_t  layer_off_v = v_elem * static_cast<size_t>(il) * T_enc_max * d_model * B;
    ggml_tensor * K           = ggml_view_4d(ctx, kv_cache.cross_k, head_dim, T_enc_max, n_heads, B, k_elem * d_model,
                                             k_elem * head_dim, k_elem * d_model * T_enc_max, layer_off_k);
    ggml_tensor * V           = ggml_view_4d(ctx, kv_cache.cross_v, head_dim, T_enc_max, n_heads, B, v_elem * d_model,
                                             v_elem * head_dim, v_elem * d_model * T_enc_max, layer_off_v);

    ggml_tensor * o = ggml_flash_attn_ext(ctx, Q_att, K, V, cross_mask, scale, 0.0f, 0.0f);
    o               = ggml_reshape_2d(ctx, o, d_model, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

}  // namespace

DecoderBuild build_cross_kv_graph_batched(ggml_context *         ctx,
                                          const WhisperWeights & w,
                                          const WhisperHParams & hp,
                                          WhisperKvCache &       kv_cache,
                                          int                    T_enc_max,
                                          int                    n_batch) {
    DecoderBuild db{};
    if (ctx == nullptr || T_enc_max <= 0 || n_batch <= 0) {
        return db;
    }

    const int64_t d_model = hp.dec_d_model;
    const int     B       = n_batch;

    db.encoder_out_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, d_model, T_enc_max, B);
    ggml_set_name(db.encoder_out_in, "dec.encoder_out");
    ggml_set_input(db.encoder_out_in);

    db.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (db.graph == nullptr) {
        return db;
    }

    const size_t k_elem = ggml_element_size(kv_cache.cross_k);
    const size_t v_elem = ggml_element_size(kv_cache.cross_v);

    const int n_layers = static_cast<int>(w.dec_blocks.size());
    for (int il = 0; il < n_layers; ++il) {
        const auto & blk = w.dec_blocks[il];

        // Cross k has no bias; cross v does.
        ggml_tensor * Kc = ggml_mul_mat(ctx, blk.cross_k_w, db.encoder_out_in);
        ggml_tensor * Vc = ggml_mul_mat(ctx, blk.cross_v_w, db.encoder_out_in);
        if (blk.cross_v_b != nullptr) {
            Vc = ggml_add(ctx, Vc, blk.cross_v_b);
        }
        // Kc/Vc: [d_model, T_enc_max, B].

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

StepBuildBatched build_step_graph_batched(ggml_context *         ctx,
                                          const WhisperWeights & w,
                                          const WhisperHParams & hp,
                                          WhisperKvCache &       kv_cache,
                                          int                    max_n_kv,
                                          int                    T_enc_max,
                                          int                    n_batch,
                                          bool                   use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || T_enc_max <= 0 || n_batch <= 0) {
        return sb;
    }
    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper step(batched): requires flash path");
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

    // Token + positional embedding (no post-embed LayerNorm).
    ggml_tensor * tok_emb = ggml_get_rows(ctx, w.dec_top.token_embd_w, sb.token_ids_in);
    ggml_tensor * pos_emb = ggml_get_rows(ctx, w.dec_top.pos_emb_w, sb.pos_ids_in);
    ggml_tensor * x       = ggml_add(ctx, tok_emb, pos_emb);

    const int n_blocks = static_cast<int>(w.dec_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        const auto & b = w.dec_blocks[i];
        {
            ggml_tensor * y  = layer_norm(ctx, x, b.norm_self_w, b.norm_self_b);
            ggml_tensor * so = mha_self_step_batched(ctx, sb.graph, y, kv_cache, sb.self_mask_in, sb.kv_idx_in,
                                                     b.self_q_w, b.self_q_b, b.self_k_w, b.self_v_w, b.self_v_b,
                                                     b.self_out_w, b.self_out_b, n_heads, d_model, i, max_n_kv, B);
            x                = ggml_add(ctx, x, so);
        }
        {
            ggml_tensor * y  = layer_norm(ctx, x, b.norm_cross_w, b.norm_cross_b);
            ggml_tensor * co = mha_cross_step_batched(ctx, y, kv_cache, sb.cross_mask_in, b.cross_q_w, b.cross_q_b,
                                                      b.cross_out_w, b.cross_out_b, n_heads, d_model, i, T_enc_max, B);
            x                = ggml_add(ctx, x, co);
        }
        {
            ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
            y               = ffn(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
            x               = ggml_add(ctx, x, y);
        }
    }

    x = layer_norm(ctx, x, w.dec_top.final_norm_w, w.dec_top.final_norm_b);

    // Tied lm_head, no bias. Output: raw pre-softmax logits [vocab, B].
    ggml_tensor * logits = ggml_mul_mat(ctx, w.dec_top.token_embd_w, x);
    ggml_set_name(logits, "step.logits_raw_b");
    ggml_set_output(logits);
    sb.logits_out = logits;
    ggml_build_forward_expand(sb.graph, sb.logits_out);
    return sb;
}

}  // namespace transcribe::whisper
