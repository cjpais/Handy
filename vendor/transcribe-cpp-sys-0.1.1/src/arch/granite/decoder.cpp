// arch/granite/decoder.cpp - Granite-4 LM prefill + step graph builders.
//
// Reference: GraniteForCausalLM + GraniteDecoderLayer in
// transformers/models/granite/modeling_granite.py.
//
// Differences from src/causal_lm/block_prefill (which doesn't fit here):
//   - No per-head Q/K-RMSNorm.
//   - Attention scale = attention_multiplier (1/128), replacing
//     1/sqrt(head_dim).
//   - Residual adds are weighted: x + residual_multiplier * sub_out.
//   - Embedding * embedding_multiplier (12) applied AFTER audio scatter.
//   - Logits / logits_scaling (8) on the final mul_mat output.
//
// We reuse `transcribe::causal_lm::KvCache` because its memory layout
// (layer, position, head, dim) matches what granite needs — only the
// block math differs.

#include "decoder.h"

#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"

#include <cmath>
#include <cstdio>

namespace transcribe::granite {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

ggml_tensor * rms_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * weight, float eps) {
    return ggml_mul(ctx, ggml_rms_norm(ctx, x, eps), weight);
}

struct GraniteBlockParams {
    int   n_heads      = 0;
    int   n_kv_heads   = 0;
    int   head_dim     = 0;
    int   max_position = 0;
    float rms_eps      = 0.0f;
    float rope_theta   = 0.0f;
    float attn_scale   = 0.0f;  // attention_multiplier (e.g. 1/128)
    float residual_mul = 0.0f;  // residual_multiplier (e.g. 0.22)
};

GraniteBlockParams to_params(const GraniteHParams & hp) {
    GraniteBlockParams p{};
    p.n_heads      = hp.dec_n_heads;
    p.n_kv_heads   = hp.dec_n_kv_heads;
    p.head_dim     = hp.dec_head_dim;
    p.max_position = hp.dec_max_position_embeddings;
    p.rms_eps      = hp.dec_rms_norm_eps;
    p.rope_theta   = hp.dec_rope_theta;
    p.attn_scale   = hp.dec_attention_multiplier;
    p.residual_mul = hp.dec_residual_multiplier;
    return p;
}

// Granite block (prefill). x: [hidden, T_seq].
ggml_tensor * block_prefill(ggml_context *                   ctx,
                            ggml_cgraph *                    gf,
                            ggml_tensor *                    x,
                            const GraniteDecBlock &          view,
                            const GraniteBlockParams &       params,
                            transcribe::causal_lm::KvCache & kv_cache,
                            int                              layer_idx,
                            int                              T_seq,
                            ggml_tensor *                    mask,
                            ggml_tensor *                    positions,
                            bool                             use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int     n_ctx      = kv_cache.n_ctx;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = params.attn_scale;

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    // ---- Attention sub-layer ----
    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = ggml_mul_mat(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K = ggml_mul_mat(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V = ggml_mul_mat(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T_seq, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T_seq, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T_seq, 1);

    // RoPE (NeoX) on Q and K. No q_norm/k_norm on Granite.
    Q = ggml_rope_ext(ctx, Q, positions, /*c=*/nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX,
                      params.max_position, rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // KV write (1D cpy; same memory order as causal_lm).
    {
        const size_t  layer_off = static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
        const size_t  n_elem    = static_cast<size_t>(T_seq) * kv_dim;
        ggml_tensor * k_dst     = ggml_view_1d(ctx, kv_cache.self_k, n_elem, k_elem * layer_off);
        ggml_tensor * v_dst     = ggml_view_1d(ctx, kv_cache.self_v, n_elem, v_elem * layer_off);
        ggml_build_forward_expand(gf, ggml_cpy(ctx, K, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, V, v_dst));
    }

    // Read K/V back as strided 3D views.
    const size_t  layer_off_bytes_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    const size_t  layer_off_bytes_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    ggml_tensor * K_att             = ggml_view_3d(ctx, kv_cache.self_k, head_dim, T_seq, n_kv_heads, k_elem * kv_dim,
                                                   k_elem * head_dim, layer_off_bytes_k);
    ggml_tensor * V_att             = ggml_view_3d(ctx, kv_cache.self_v, head_dim, T_seq, n_kv_heads, v_elem * kv_dim,
                                                   v_elem * head_dim, layer_off_bytes_v);

    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, T_seq);
    } else {
        ggml_tensor * K_att_c        = ggml_cont(ctx, K_att);
        ggml_tensor * V_att_c        = ggml_cont(ctx, V_att);
        ggml_tensor * K_4d           = ggml_reshape_4d(ctx, K_att_c, head_dim, T_seq, 1, n_kv_heads);
        ggml_tensor * V_4d           = ggml_reshape_4d(ctx, V_att_c, head_dim, T_seq, 1, n_kv_heads);
        ggml_tensor * K_rep_template = ggml_new_tensor_4d(ctx, K_att->type, head_dim, T_seq, n_groups, n_kv_heads);
        ggml_tensor * V_rep_template = ggml_new_tensor_4d(ctx, V_att->type, head_dim, T_seq, n_groups, n_kv_heads);
        ggml_tensor * K_rep          = ggml_repeat(ctx, K_4d, K_rep_template);
        ggml_tensor * V_rep          = ggml_repeat(ctx, V_4d, V_rep_template);
        ggml_tensor * K_full         = ggml_reshape_3d(ctx, K_rep, head_dim, T_seq, n_heads);
        ggml_tensor * V_full         = ggml_reshape_3d(ctx, V_rep, head_dim, T_seq, n_heads);

        ggml_tensor * kq      = ggml_mul_mat(ctx, K_full, Q_att);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale_attn, /*max_bias=*/0.0f);
        ggml_tensor * V_t     = ggml_cont(ctx, ggml_permute(ctx, V_full, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, V_t, kq_soft);
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
        o                     = ggml_reshape_2d(ctx, o, q_dim, T_seq);
    }

    o = ggml_mul_mat(ctx, view.attn_o_w, o);
    // Residual: x + residual_multiplier * o.
    o = ggml_scale(ctx, o, params.residual_mul);
    x = ggml_add(ctx, x, o);

    // ---- MLP sub-layer (SwiGLU, separate gate + up) ----
    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    ggml_tensor * gate    = ggml_mul_mat(ctx, view.ffn_gate_w, ff_norm);
    ggml_tensor * up      = ggml_mul_mat(ctx, view.ffn_up_w, ff_norm);
    ggml_tensor * ff      = ggml_mul(ctx, ggml_silu(ctx, gate), up);
    ff                    = ggml_mul_mat(ctx, view.ffn_down_w, ff);
    ff                    = ggml_scale(ctx, ff, params.residual_mul);
    x                     = ggml_add(ctx, x, ff);
    return x;
}

// Granite block (step). x: [hidden, 1]. KV index from `kv_idx` tensor.
ggml_tensor * block_step(ggml_context *                   ctx,
                         ggml_cgraph *                    gf,
                         ggml_tensor *                    x,
                         const GraniteDecBlock &          view,
                         const GraniteBlockParams &       params,
                         transcribe::causal_lm::KvCache & kv_cache,
                         int                              layer_idx,
                         int                              max_n_kv,
                         ggml_tensor *                    mask,
                         ggml_tensor *                    position,
                         ggml_tensor *                    kv_idx,
                         bool                             use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int     n_ctx      = kv_cache.n_ctx;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = params.attn_scale;

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = ggml_mul_mat(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K = ggml_mul_mat(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V = ggml_mul_mat(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, 1, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, 1, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, 1, 1);

    Q = ggml_rope_ext(ctx, Q, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    {
        const size_t layer_off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
        const size_t layer_off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;

        ggml_tensor * k_layer = ggml_view_2d(ctx, kv_cache.self_k, kv_dim, n_ctx, k_elem * kv_dim, layer_off_k);
        ggml_tensor * v_layer = ggml_view_2d(ctx, kv_cache.self_v, kv_dim, n_ctx, v_elem * kv_dim, layer_off_v);

        ggml_tensor * K_row = ggml_reshape_2d(ctx, K, kv_dim, 1);
        ggml_tensor * V_row = ggml_reshape_2d(ctx, V, kv_dim, 1);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    const size_t  layer_off_bytes_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    const size_t  layer_off_bytes_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    ggml_tensor * K_att = ggml_view_3d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_kv_heads, k_elem * kv_dim,
                                       k_elem * head_dim, layer_off_bytes_k);
    ggml_tensor * V_att = ggml_view_3d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_kv_heads, v_elem * kv_dim,
                                       v_elem * head_dim, layer_off_bytes_v);

    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, 1);
    } else {
        ggml_tensor * K_att_c        = ggml_cont(ctx, K_att);
        ggml_tensor * V_att_c        = ggml_cont(ctx, V_att);
        ggml_tensor * K_4d           = ggml_reshape_4d(ctx, K_att_c, head_dim, max_n_kv, 1, n_kv_heads);
        ggml_tensor * V_4d           = ggml_reshape_4d(ctx, V_att_c, head_dim, max_n_kv, 1, n_kv_heads);
        ggml_tensor * K_rep_template = ggml_new_tensor_4d(ctx, K_att->type, head_dim, max_n_kv, n_groups, n_kv_heads);
        ggml_tensor * V_rep_template = ggml_new_tensor_4d(ctx, V_att->type, head_dim, max_n_kv, n_groups, n_kv_heads);
        ggml_tensor * K_rep          = ggml_repeat(ctx, K_4d, K_rep_template);
        ggml_tensor * V_rep          = ggml_repeat(ctx, V_4d, V_rep_template);
        ggml_tensor * K_full         = ggml_reshape_3d(ctx, K_rep, head_dim, max_n_kv, n_heads);
        ggml_tensor * V_full         = ggml_reshape_3d(ctx, V_rep, head_dim, max_n_kv, n_heads);

        ggml_tensor * kq      = ggml_mul_mat(ctx, K_full, Q_att);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale_attn, /*max_bias=*/0.0f);
        ggml_tensor * V_t     = ggml_cont(ctx, ggml_permute(ctx, V_full, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, V_t, kq_soft);
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
        o                     = ggml_reshape_2d(ctx, o, q_dim, 1);
    }

    o = ggml_mul_mat(ctx, view.attn_o_w, o);
    o = ggml_scale(ctx, o, params.residual_mul);
    x = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    ggml_tensor * ff;
    if (view.ffn_gate_up_w != nullptr) {
        ggml_tensor * gate_up = ggml_mul_mat(ctx, view.ffn_gate_up_w, ff_norm);
        ff                    = ggml_swiglu(ctx, gate_up);
    } else {
        ggml_tensor * gate = ggml_mul_mat(ctx, view.ffn_gate_w, ff_norm);
        ggml_tensor * up   = ggml_mul_mat(ctx, view.ffn_up_w, ff_norm);
        ff                 = ggml_mul(ctx, ggml_silu(ctx, gate), up);
    }
    ff = ggml_mul_mat(ctx, view.ffn_down_w, ff);
    ff = ggml_scale(ctx, ff, params.residual_mul);
    x  = ggml_add(ctx, x, ff);
    return x;
}

// Granite batched block (prefill). x: [hidden, T, B], batch on ne[2].
// Requires flash. Mirrors causal_lm::block_prefill_batched with Granite math.
ggml_tensor * block_prefill_batched(ggml_context *                   ctx,
                                    ggml_cgraph *                    gf,
                                    ggml_tensor *                    x,
                                    const GraniteDecBlock &          view,
                                    const GraniteBlockParams &       params,
                                    transcribe::causal_lm::KvCache & kv_cache,
                                    int                              layer_idx,
                                    int                              T_seq,
                                    int                              n_batch,
                                    ggml_tensor *                    mask,
                                    ggml_tensor *                    positions,
                                    ggml_tensor *                    kv_idx,
                                    bool                             use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int64_t n_ctx      = kv_cache.n_ctx;
    const int64_t T          = T_seq;
    const int64_t B          = n_batch;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = params.attn_scale;
    const size_t  k_elem     = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem     = ggml_element_size(kv_cache.self_v);

    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite block_prefill_batched: requires flash");
        return nullptr;
    }

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);
    ggml_tensor * Q      = ggml_mul_mat(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K      = ggml_mul_mat(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V      = ggml_mul_mat(ctx, view.attn_v_w, x_norm);
    Q                    = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T, B);
    K                    = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T, B);
    V                    = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T, B);
    Q = ggml_rope_ext(ctx, Q, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    {
        const size_t  off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        const size_t  off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, kv_dim, n_ctx, B, k_elem * kv_dim, k_elem * kv_dim * n_ctx, off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, kv_dim, n_ctx, B, v_elem * kv_dim, v_elem * kv_dim * n_ctx, off_v);
        ggml_tensor * K_rows = ggml_reshape_3d(ctx, K, kv_dim, T, B);
        ggml_tensor * V_rows = ggml_reshape_3d(ctx, V, kv_dim, T, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_rows, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_rows, kv_idx));
    }
    const size_t  off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    const size_t  off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    ggml_tensor * K_att = ggml_view_4d(ctx, kv_cache.self_k, head_dim, T, n_kv_heads, B, k_elem * kv_dim,
                                       k_elem * head_dim, k_elem * kv_dim * n_ctx, off_k);
    ggml_tensor * V_att = ggml_view_4d(ctx, kv_cache.self_v, head_dim, T, n_kv_heads, B, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * kv_dim * n_ctx, off_v);
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));
    ggml_tensor * o     = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, 0.0f, 0.0f);
    o                   = ggml_reshape_3d(ctx, o, q_dim, T, B);
    o                   = ggml_mul_mat(ctx, view.attn_o_w, o);
    o                   = ggml_scale(ctx, o, params.residual_mul);
    x                   = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    ggml_tensor * ff;
    if (view.ffn_gate_up_w != nullptr) {
        ff = ggml_swiglu(ctx, ggml_mul_mat(ctx, view.ffn_gate_up_w, ff_norm));
    } else {
        ggml_tensor * gate = ggml_mul_mat(ctx, view.ffn_gate_w, ff_norm);
        ggml_tensor * up   = ggml_mul_mat(ctx, view.ffn_up_w, ff_norm);
        ff                 = ggml_mul(ctx, ggml_silu(ctx, gate), up);
    }
    ff = ggml_mul_mat(ctx, view.ffn_down_w, ff);
    ff = ggml_scale(ctx, ff, params.residual_mul);
    x  = ggml_add(ctx, x, ff);
    return x;
}

// Granite batched block (step). x: [hidden, B], batch on ne[2] for RoPE,
// permute to ne[3] for flash. Mirrors causal_lm::block_step_batched.
ggml_tensor * block_step_batched(ggml_context *                   ctx,
                                 ggml_cgraph *                    gf,
                                 ggml_tensor *                    x,
                                 const GraniteDecBlock &          view,
                                 const GraniteBlockParams &       params,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              layer_idx,
                                 int                              max_n_kv,
                                 int                              n_batch,
                                 ggml_tensor *                    mask,
                                 ggml_tensor *                    position,
                                 ggml_tensor *                    kv_idx,
                                 bool                             use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int64_t n_ctx      = kv_cache.n_ctx;
    const int64_t B          = n_batch;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = params.attn_scale;
    const size_t  k_elem     = ggml_element_size(kv_cache.self_k);
    const size_t  v_elem     = ggml_element_size(kv_cache.self_v);

    if (!use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite block_step_batched: requires flash");
        return nullptr;
    }

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);
    ggml_tensor * Q      = ggml_mul_mat(ctx, view.attn_q_w, x_norm);  // [q_dim, B]
    ggml_tensor * K      = ggml_mul_mat(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V      = ggml_mul_mat(ctx, view.attn_v_w, x_norm);
    Q                    = ggml_reshape_3d(ctx, Q, head_dim, n_heads, B);
    K                    = ggml_reshape_3d(ctx, K, head_dim, n_kv_heads, B);
    V                    = ggml_reshape_3d(ctx, V, head_dim, n_kv_heads, B);
    Q = ggml_rope_ext(ctx, Q, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    {
        const size_t  off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        const size_t  off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, kv_dim, n_ctx, B, k_elem * kv_dim, k_elem * kv_dim * n_ctx, off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, kv_dim, n_ctx, B, v_elem * kv_dim, v_elem * kv_dim * n_ctx, off_v);
        ggml_tensor * K_row = ggml_reshape_3d(ctx, K, kv_dim, 1, B);
        ggml_tensor * V_row = ggml_reshape_3d(ctx, V, kv_dim, 1, B);
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }
    const size_t  off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    const size_t  off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    ggml_tensor * K_att = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_kv_heads, B, k_elem * kv_dim,
                                       k_elem * head_dim, k_elem * kv_dim * n_ctx, off_k);
    ggml_tensor * V_att = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_kv_heads, B, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * kv_dim * n_ctx, off_v);
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));
    ggml_tensor * o     = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, 0.0f, 0.0f);
    o                   = ggml_reshape_2d(ctx, o, q_dim, B);
    o                   = ggml_mul_mat(ctx, view.attn_o_w, o);
    o                   = ggml_scale(ctx, o, params.residual_mul);
    x                   = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    ggml_tensor * ff;
    if (view.ffn_gate_up_w != nullptr) {
        ff = ggml_swiglu(ctx, ggml_mul_mat(ctx, view.ffn_gate_up_w, ff_norm));
    } else {
        ggml_tensor * gate = ggml_mul_mat(ctx, view.ffn_gate_w, ff_norm);
        ggml_tensor * up   = ggml_mul_mat(ctx, view.ffn_up_w, ff_norm);
        ff                 = ggml_mul(ctx, ggml_silu(ctx, gate), up);
    }
    ff = ggml_mul_mat(ctx, view.ffn_down_w, ff);
    ff = ggml_scale(ctx, ff, params.residual_mul);
    x  = ggml_add(ctx, x, ff);
    return x;
}

}  // namespace

// Prefill graph.

PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const GraniteWeights &           weights,
                                 const GraniteHParams &           hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              T_prompt,
                                 int                              n_audio_tokens,
                                 int                              prefix_len,
                                 int                              suffix_len,
                                 bool                             use_flash,
                                 bool                             slice_last) {
    PrefillBuild pb{};
    pb.T_prompt       = T_prompt;
    pb.n_audio_tokens = n_audio_tokens;
    pb.prefix_len     = prefix_len;
    pb.suffix_len     = suffix_len;

    if (ctx == nullptr || T_prompt <= 0 || n_audio_tokens <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite decoder: invalid arg (T_prompt=%d, n_audio=%d)", T_prompt,
                n_audio_tokens);
        return pb;
    }
    if (prefix_len < 0 || suffix_len < 0 || prefix_len + n_audio_tokens + suffix_len != T_prompt) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite decoder: prefix(%d)+n_audio(%d)+suffix(%d) != T_prompt(%d)",
                prefix_len, n_audio_tokens, suffix_len, T_prompt);
        return pb;
    }
    if (kv_cache.self_k == nullptr || kv_cache.self_v == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite decoder: kv_cache not initialized");
        return pb;
    }
    if (T_prompt > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite decoder: T_prompt=%d exceeds kv_cache.n_ctx=%d", T_prompt,
                kv_cache.n_ctx);
        return pb;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;
    const float   emb_mul = hp.dec_embedding_multiplier;
    const float   inv_log = (hp.dec_logits_scaling > 0.0f) ? (1.0f / hp.dec_logits_scaling) : 1.0f;

    const auto params = to_params(hp);

    // Graph inputs.
    pb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt);
    named(pb.input_ids_in, "dec.input_ids");
    ggml_set_input(pb.input_ids_in);

    pb.audio_feats_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, n_audio_tokens);
    named(pb.audio_feats_in, "dec.audio_features");
    ggml_set_input(pb.audio_feats_in);

    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt);
    named(pb.positions_in, "dec.positions");
    ggml_set_input(pb.positions_in);

    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_prompt, T_prompt);
    named(pb.mask_in, "dec.attn_mask");
    ggml_set_input(pb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite decoder: ggml_new_graph_custom failed");
        return pb;
    }
    pb.graph = gf;

    // Token embedding (for full prompt, for dump parity).
    ggml_tensor * token_emb_all = ggml_get_rows(ctx, weights.dec_embed.token_w, pb.input_ids_in);
    named(token_emb_all, "dec.token_emb");
    pb.dumps.token_emb = token_emb_all;
    transcribe::debug::mark_tensor_for_dump(token_emb_all);

    // Audio injection via 3-way concat.
    const size_t  emb_elem = ggml_element_size(token_emb_all);
    ggml_tensor * x_prefix = nullptr;
    if (prefix_len > 0) {
        x_prefix = ggml_view_2d(ctx, token_emb_all, hidden, prefix_len, emb_elem * hidden,
                                /*off=*/0);
        x_prefix = ggml_cont(ctx, x_prefix);
    }
    ggml_tensor * x_audio  = pb.audio_feats_in;
    ggml_tensor * x_suffix = nullptr;
    if (suffix_len > 0) {
        x_suffix = ggml_view_2d(ctx, token_emb_all, hidden, suffix_len, emb_elem * hidden,
                                emb_elem * hidden * static_cast<size_t>(prefix_len + n_audio_tokens));
        x_suffix = ggml_cont(ctx, x_suffix);
    }

    ggml_tensor * x = x_audio;
    if (x_prefix != nullptr) {
        x = ggml_concat(ctx, x_prefix, x, /*dim=*/1);
    }
    if (x_suffix != nullptr) {
        x = ggml_concat(ctx, x, x_suffix, /*dim=*/1);
    }
    named(x, "dec.audio_injected");
    pb.dumps.audio_injected = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // Embedding multiplier (after the dump point).
    x = ggml_scale(ctx, x, emb_mul);

    // Block stack.
    const int mid_idx = n_layer / 2;
    for (int il = 0; il < n_layer; ++il) {
        x = block_prefill(ctx, gf, x, weights.dec_blocks[il], params, kv_cache, il, T_prompt, pb.mask_in,
                          pb.positions_in, use_flash);

        if (il == 0) {
            named(x, "dec.block.0.out");
            pb.dumps.block_0_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        } else if (il == mid_idx) {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", il);
            named(x, nm);
            pb.dumps.block_mid_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        } else if (il == n_layer - 1) {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", il);
            named(x, nm);
            pb.dumps.block_last_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
    }

    // Final RMSNorm.
    x = rms_norm(ctx, x, weights.dec_final.norm_w, rms_eps);
    named(x, "dec.out_before_head");
    pb.dumps.out_before_head = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // LM head (slice last position).
    ggml_tensor * last_x = ggml_view_2d(ctx, x, hidden, 1, ggml_element_size(x) * hidden,
                                        ggml_element_size(x) * hidden * static_cast<size_t>(T_prompt - 1));
    last_x               = ggml_cont(ctx, last_x);
    (void) slice_last;  // single-position slice is unconditional in granite

    // -plus has tie_word_embeddings=true; use token_w for the head.
    ggml_tensor * head_w =
        (weights.dec_final.output_w != nullptr) ? weights.dec_final.output_w : weights.dec_embed.token_w;
    ggml_tensor * logits = ggml_mul_mat(ctx, head_w, last_x);
    logits               = ggml_reshape_1d(ctx, logits, vocab);
    // logits_scaling divides logits (Granite scales the head down).
    logits               = ggml_scale(ctx, logits, inv_log);
    named(logits, "dec.logits_raw");
    pb.dumps.logits_raw = logits;
    transcribe::debug::mark_tensor_for_dump(logits);

    pb.out = logits;
    ggml_set_output(pb.out);

    ggml_build_forward_expand(gf, pb.out);
    if (pb.dumps.token_emb) {
        ggml_build_forward_expand(gf, pb.dumps.token_emb);
    }
    if (pb.dumps.audio_injected) {
        ggml_build_forward_expand(gf, pb.dumps.audio_injected);
    }
    if (pb.dumps.block_0_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_0_out);
    }
    if (pb.dumps.block_mid_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_mid_out);
    }
    if (pb.dumps.block_last_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_last_out);
    }
    if (pb.dumps.out_before_head) {
        ggml_build_forward_expand(gf, pb.dumps.out_before_head);
    }

    return pb;
}

// Step graph.

StepBuild build_step_graph(ggml_context *                   ctx,
                           const GraniteWeights &           weights,
                           const GraniteHParams &           hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite step: invalid arg (max_n_kv=%d)", max_n_kv);
        return sb;
    }
    if (kv_cache.self_k == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite step: kv_cache not initialized");
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;
    const float   emb_mul = hp.dec_embedding_multiplier;
    const float   inv_log = (hp.dec_logits_scaling > 0.0f) ? (1.0f / hp.dec_logits_scaling) : 1.0f;

    const auto params = to_params(hp);

    sb.input_id_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.input_id_in, "step.input_id");
    ggml_set_input(sb.input_id_in);

    sb.position_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.position_in, "step.position");
    ggml_set_input(sb.position_in);

    sb.kv_idx_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I64, 1);
    ggml_set_name(sb.kv_idx_in, "step.kv_idx");
    ggml_set_input(sb.kv_idx_in);

    sb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, max_n_kv, 1);
    ggml_set_name(sb.mask_in, "step.mask");
    ggml_set_input(sb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite step: ggml_new_graph_custom failed");
        return sb;
    }
    sb.graph = gf;

    // Token embedding for the single new token + embedding_multiplier.
    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w,
                                    sb.input_id_in);  // [hidden, 1]
    x               = ggml_scale(ctx, x, emb_mul);

    for (int il = 0; il < n_layer; ++il) {
        x = block_step(ctx, gf, x, weights.dec_blocks[il], params, kv_cache, il, max_n_kv, sb.mask_in, sb.position_in,
                       sb.kv_idx_in, use_flash);
    }

    x = rms_norm(ctx, x, weights.dec_final.norm_w, rms_eps);
    ggml_tensor * head_w =
        (weights.dec_final.output_w != nullptr) ? weights.dec_final.output_w : weights.dec_embed.token_w;
    ggml_tensor * logits = ggml_mul_mat(ctx, head_w, x);
    logits               = ggml_reshape_1d(ctx, logits, vocab);
    logits               = ggml_scale(ctx, logits, inv_log);
    ggml_set_name(logits, "step.logits");

    ggml_tensor * amax = ggml_argmax(ctx, logits);
    ggml_set_name(amax, "step.argmax");

    sb.out = amax;
    ggml_set_output(sb.out);

    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);

    return sb;
}

// Batched prefill / step (offline transcribe_run_batch).

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const GraniteWeights &           weights,
                                                const GraniteHParams &           hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                int                              T_prompt_max,
                                                int                              n_audio_max,
                                                int                              n_batch,
                                                bool                             use_flash) {
    PrefillBuildBatched pb{};
    pb.T_prompt_max = T_prompt_max;
    pb.n_audio_max  = n_audio_max;
    pb.n_batch      = n_batch;

    if (ctx == nullptr || T_prompt_max <= 0 || n_audio_max <= 0 || n_batch <= 0 || !use_flash ||
        kv_cache.self_k == nullptr || kv_cache.n_batch != n_batch || T_prompt_max > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite prefill(batched): invalid arg");
        return pb;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;
    const float   emb_mul = hp.dec_embedding_multiplier;
    const float   inv_log = (hp.dec_logits_scaling > 0.0f) ? (1.0f / hp.dec_logits_scaling) : 1.0f;
    const int     B       = n_batch;
    const auto    params  = to_params(hp);

    pb.input_ids_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, T_prompt_max, B);
    ggml_set_input(pb.input_ids_in);
    // Audio injection is an elementwise blend over the flat token axis (no
    // set_rows): a k-quant token_embd get_rows is unsupported on CUDA and runs
    // on CPU; a set_rows consuming that CPU tensor straddles the CPU/CUDA split
    // and faults. x*keep_mask + audio_dense crosses the split cleanly.
    pb.audio_dense_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, static_cast<int64_t>(T_prompt_max) * B);
    ggml_set_input(pb.audio_dense_in);
    pb.keep_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, 1, static_cast<int64_t>(T_prompt_max) * B);
    ggml_set_input(pb.keep_mask_in);
    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt_max);
    ggml_set_input(pb.positions_in);
    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_prompt_max, T_prompt_max);
    ggml_set_input(pb.mask_in);
    pb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, T_prompt_max, B);
    ggml_set_input(pb.kv_idx_in);
    pb.last_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, 1, B);
    ggml_set_input(pb.last_idx_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        return pb;
    }
    pb.graph = gf;

    ggml_tensor * ids_flat = ggml_reshape_1d(ctx, pb.input_ids_in, static_cast<int64_t>(T_prompt_max) * B);
    // x is 2D [hidden, T_prompt_max*B] (contiguous). Blend the audio embeds in
    // elementwise: x = x*keep_mask + audio_dense. keep_mask broadcasts over
    // hidden (ne[0]). Then reshape to 3D for the batched block stack.
    ggml_tensor * x        = ggml_get_rows(ctx, weights.dec_embed.token_w, ids_flat);
    x                      = ggml_add(ctx, ggml_mul(ctx, x, pb.keep_mask_in), pb.audio_dense_in);
    x                      = ggml_reshape_3d(ctx, x, hidden, T_prompt_max, B);
    x                      = ggml_scale(ctx, x, emb_mul);  // embedding_multiplier (after injection)

    for (int il = 0; il < n_layer; ++il) {
        x = block_prefill_batched(ctx, gf, x, weights.dec_blocks[il], params, kv_cache, il, T_prompt_max, B, pb.mask_in,
                                  pb.positions_in, pb.kv_idx_in, use_flash);
        if (x == nullptr) {
            pb.graph = nullptr;
            return pb;
        }
    }

    ggml_tensor * x_last = ggml_get_rows(ctx, x, pb.last_idx_in);
    x_last               = ggml_reshape_2d(ctx, x_last, hidden, B);
    x_last               = rms_norm(ctx, x_last, weights.dec_final.norm_w, rms_eps);
    ggml_tensor * head_w =
        (weights.dec_final.output_w != nullptr) ? weights.dec_final.output_w : weights.dec_embed.token_w;
    ggml_tensor * logits = ggml_mul_mat(ctx, head_w, x_last);
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    logits               = ggml_scale(ctx, logits, inv_log);
    pb.logits            = logits;
    pb.out               = ggml_argmax(ctx, logits);
    ggml_set_output(pb.out);
    ggml_build_forward_expand(gf, pb.out);
    ggml_build_forward_expand(gf, logits);
    return pb;
}

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const GraniteWeights &           weights,
                                          const GraniteHParams &           hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || n_batch <= 0 || !use_flash || kv_cache.self_k == nullptr ||
        kv_cache.n_batch != n_batch || max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite step(batched): invalid arg");
        return sb;
    }

    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;
    const float   emb_mul = hp.dec_embedding_multiplier;
    const float   inv_log = (hp.dec_logits_scaling > 0.0f) ? (1.0f / hp.dec_logits_scaling) : 1.0f;
    const int     B       = n_batch;
    const auto    params  = to_params(hp);

    sb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_input(sb.input_ids_in);
    sb.position_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_input(sb.position_in);
    sb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, 1, B);
    ggml_set_input(sb.kv_idx_in);
    sb.mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F16, max_n_kv, 1, 1, B);
    ggml_set_input(sb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        return sb;
    }
    sb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w, sb.input_ids_in);
    x               = ggml_scale(ctx, x, emb_mul);
    for (int il = 0; il < n_layer; ++il) {
        x = block_step_batched(ctx, gf, x, weights.dec_blocks[il], params, kv_cache, il, max_n_kv, B, sb.mask_in,
                               sb.position_in, sb.kv_idx_in, use_flash);
        if (x == nullptr) {
            sb.graph = nullptr;
            return sb;
        }
    }

    x = rms_norm(ctx, x, weights.dec_final.norm_w, rms_eps);
    ggml_tensor * head_w =
        (weights.dec_final.output_w != nullptr) ? weights.dec_final.output_w : weights.dec_embed.token_w;
    ggml_tensor * logits = ggml_mul_mat(ctx, head_w, x);
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    logits               = ggml_scale(ctx, logits, inv_log);
    sb.logits            = logits;
    sb.out               = ggml_argmax(ctx, logits);
    ggml_set_output(sb.out);
    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);
    return sb;
}

}  // namespace transcribe::granite
