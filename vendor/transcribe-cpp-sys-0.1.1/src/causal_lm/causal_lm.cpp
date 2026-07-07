// src/causal_lm/causal_lm.cpp - shared causal-decoder LM block math
// (Llama / Qwen3 lineage). See causal_lm.h for the API contract.

#include "causal_lm.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-backend.h"
#include "transcribe-log.h"
#include "transcribe-session.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <vector>

namespace transcribe::causal_lm {

namespace {

ggml_tensor * rms_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * weight, float eps) {
    return ggml_mul(ctx, ggml_rms_norm(ctx, x, eps), weight);
}

// Weight matmul forcing F32 accumulation for F16 weights (CANONICAL F16-accum
// note for this module). On CUDA, F16 weights take the cuBLAS path that
// accumulates in F16 (CUBLAS_COMPUTE_16F); a deep LLM residual stream has
// large-magnitude outlier channels that overflow F16's ~65504 range -> NaNs.
// GGML_PREC_F32 makes cuBLAS run the GEMM in F32, matching the CPU reference.
// Gated on F16 so BF16/quantized/F32 weights are untouched (BF16 already
// COMPUTE_32F; quantized uses MMQ) and CPU/Metal (always F32-accumulate) are
// unaffected.
ggml_tensor * mul_mat_f32acc(ggml_context * ctx, ggml_tensor * w, ggml_tensor * x) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (w->type == GGML_TYPE_F16) {
        ggml_mul_mat_set_prec(y, GGML_PREC_F32);
    }
    return y;
}

}  // namespace

void KvCache::free() {
    if (buffer != nullptr) {
        safe_buffer_free(buffer);
        buffer = nullptr;
    }
    if (ctx != nullptr) {
        ggml_free(ctx);
        ctx = nullptr;
    }
    self_k = nullptr;
    self_v = nullptr;
    n      = 0;
    head   = 0;
}

bool kv_init(KvCache &      cache,
             ggml_backend_t backend,
             int            n_ctx,
             int            n_kv_heads,
             int            head_dim,
             int            n_layer,
             ggml_type      kv_type) {
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init: unsupported kv_type=%d", static_cast<int>(kv_type));
        return false;
    }

    const size_t     ctx_size = 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init: ggml_init failed");
        return false;
    }

    const int64_t elems = static_cast<int64_t>(n_kv_heads) * head_dim * n_ctx * n_layer;
    cache.self_k        = ggml_new_tensor_1d(cache.ctx, kv_type, elems);
    cache.self_v        = ggml_new_tensor_1d(cache.ctx, kv_type, elems);
    ggml_set_name(cache.self_k, "kv_self_k");
    ggml_set_name(cache.self_v, "kv_self_v");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init: buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx   = n_ctx;
    cache.n       = 0;
    cache.head    = 0;
    cache.n_batch = 1;
    return true;
}

bool kv_init_batched(KvCache &      cache,
                     ggml_backend_t backend,
                     int            n_ctx,
                     int            n_kv_heads,
                     int            head_dim,
                     int            n_layer,
                     int            n_batch,
                     ggml_type      kv_type) {
    if (n_batch <= 1) {
        if (!kv_init(cache, backend, n_ctx, n_kv_heads, head_dim, n_layer, kv_type)) {
            return false;
        }
        cache.n_batch = 1;
        return true;
    }
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init_batched: unsupported kv_type=%d",
                static_cast<int>(kv_type));
        return false;
    }

    const size_t     ctx_size = 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init_batched: ggml_init failed");
        return false;
    }

    // Layout (slowest → fastest): layer, batch, position, head, dim. A
    // per-(layer,slot) slab is [kv_dim, n_ctx] contiguous, so prefill's 1D
    // cpy and the step's batched set_rows both address it with a simple
    // (slot + n_batch*layer)*n_ctx*kv_dim offset.
    const int64_t elems = static_cast<int64_t>(n_kv_heads) * head_dim * n_ctx * n_batch * n_layer;
    cache.self_k        = ggml_new_tensor_1d(cache.ctx, kv_type, elems);
    cache.self_v        = ggml_new_tensor_1d(cache.ctx, kv_type, elems);
    ggml_set_name(cache.self_k, "kv_self_k_batched");
    ggml_set_name(cache.self_v, "kv_self_v_batched");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm kv_init_batched: buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx   = n_ctx;
    cache.n       = 0;
    cache.head    = 0;
    cache.n_batch = n_batch;
    return true;
}

// Block forward — prefill.
ggml_tensor * block_prefill(ggml_context *      ctx,
                            ggml_cgraph *       gf,
                            ggml_tensor *       x,
                            const BlockView &   view,
                            const BlockParams & params,
                            KvCache &           kv_cache,
                            int                 layer_idx,
                            int                 T_seq,
                            ggml_tensor *       mask,
                            ggml_tensor *       positions,
                            BlockOpts           opts) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int     n_ctx      = kv_cache.n_ctx;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = 1.0f / std::sqrt(static_cast<float>(head_dim));

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    // Q/K/V projections (bias-free on Qwen3). Packing into one mul_mat
    // consistently regresses on Metal; left separate.
    ggml_tensor * Q = mul_mat_f32acc(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K = mul_mat_f32acc(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V = mul_mat_f32acc(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T_seq, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T_seq, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T_seq, 1);

    // Per-head Q/K RMSNorm is a Qwen3 feature; Llama-style decoders ship no
    // q_norm/k_norm tensors, so the call site leaves these slots null and we
    // skip the norm. (Same for block_step / step_n / batched below.)
    if (view.attn_q_norm != nullptr) {
        Q = ggml_mul(ctx, ggml_rms_norm(ctx, Q, rms_eps), view.attn_q_norm);
    }
    if (view.attn_k_norm != nullptr) {
        K = ggml_mul(ctx, ggml_rms_norm(ctx, K, rms_eps), view.attn_k_norm);
    }

    // RoPE (NeoX rotate_half) on Q and K. MRoPE collapses to NeoX for
    // text-only ASR (every position has the same (T,H,W) coordinate).
    Q = ggml_rope_ext(ctx, Q, positions, /*c=*/nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX,
                      params.max_position, rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // KV write: K, V have ne=[D, Hkv, T_seq, 1], same layout as the cache
    // (position-major within each layer), so a 1D cpy handles it. Slab
    // (layer, batch-slot) base offset in elements; defaults (kv_batch_slot=0,
    // kv_n_batch=1) collapse to layer_idx*n_ctx*kv_dim.
    const size_t slab =
        static_cast<size_t>(opts.kv_batch_slot) + static_cast<size_t>(opts.kv_n_batch) * static_cast<size_t>(layer_idx);
    const size_t slab_off = slab * n_ctx * kv_dim;
    {
        const size_t n_elem = static_cast<size_t>(T_seq) * kv_dim;

        ggml_tensor * k_dst = ggml_view_1d(ctx, kv_cache.self_k, n_elem, k_elem * slab_off);
        ggml_tensor * v_dst = ggml_view_1d(ctx, kv_cache.self_v, n_elem, v_elem * slab_off);

        ggml_build_forward_expand(gf, ggml_cpy(ctx, K, k_dst));
        ggml_build_forward_expand(gf, ggml_cpy(ctx, V, v_dst));
    }

    // Read K, V back from the cache for attention as strided
    // [D, T_seq, Hkv] views (no permute + cont needed). mul_mat and
    // flash_attn_ext both accept strided inputs.
    ggml_tensor * K_att = ggml_view_3d(ctx, kv_cache.self_k, head_dim, T_seq, n_kv_heads,
                                       /*nb1=*/k_elem * kv_dim,
                                       /*nb2=*/k_elem * head_dim, k_elem * slab_off);
    ggml_tensor * V_att = ggml_view_3d(ctx, kv_cache.self_v, head_dim, T_seq, n_kv_heads, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * slab_off);

    // Permute Q for attention: [D, H, T_seq, 1] → [D, T_seq, H, 1].
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));

    ggml_tensor * o;
    if (opts.use_flash) {
        // ggml_flash_attn_ext handles GQA natively when K/V have
        // n_kv_heads along axis 2 and Q has n_heads — no
        // repeat_interleave needed. Mask is F16 (host upload avoids
        // per-layer ggml_cast).
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, T_seq);
    } else {
        // Manual GQA path: emulate repeat_interleave via reshape
        // -into-(1,Hkv) → repeat → collapse. Numerics match the
        // reference's explicit repeat_kv.
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

    o = mul_mat_f32acc(ctx, view.attn_o_w, o);
    x = ggml_add(ctx, x, o);

    if (opts.slice_last_before_ffn) {
        const int64_t hidden = x->ne[0];
        const size_t  elem   = ggml_element_size(x);
        x = ggml_view_2d(ctx, x, hidden, 1, elem * hidden, elem * hidden * static_cast<size_t>(T_seq - 1));
        x = ggml_cont(ctx, x);
    }

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    if (view.ffn_scale != nullptr) {
        ff_norm = ggml_mul(ctx, ff_norm, view.ffn_scale);
    }
    ggml_tensor * gate_up = mul_mat_f32acc(ctx, view.ffn_gate_up_w, ff_norm);
    ggml_tensor * ff      = ggml_swiglu(ctx, gate_up);
    ff                    = mul_mat_f32acc(ctx, view.ffn_down_w, ff);

    x = ggml_add(ctx, x, ff);
    return x;
}

// Block forward — step.
ggml_tensor * block_step(ggml_context *      ctx,
                         ggml_cgraph *       gf,
                         ggml_tensor *       x,
                         const BlockView &   view,
                         const BlockParams & params,
                         KvCache &           kv_cache,
                         int                 layer_idx,
                         int                 max_n_kv,
                         ggml_tensor *       mask,
                         ggml_tensor *       position,
                         ggml_tensor *       kv_idx,
                         bool                use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int     n_ctx      = kv_cache.n_ctx;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = 1.0f / std::sqrt(static_cast<float>(head_dim));

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = mul_mat_f32acc(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K = mul_mat_f32acc(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V = mul_mat_f32acc(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, 1, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, 1, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, 1, 1);

    if (view.attn_q_norm != nullptr) {
        Q = ggml_mul(ctx, ggml_rms_norm(ctx, Q, rms_eps), view.attn_q_norm);
    }
    if (view.attn_k_norm != nullptr) {
        K = ggml_mul(ctx, ggml_rms_norm(ctx, K, rms_eps), view.attn_k_norm);
    }

    Q = ggml_rope_ext(ctx, Q, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // KV write via ggml_set_rows with a dynamic index. Static topology;
    // only `kv_idx`'s value changes per step.
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

    // Attention reads the FULL [0, max_n_kv) window. Mask zeros valid
    // positions and -inf's the rest, so softmax ignores empty slots; the
    // graph stays static (zero per-step rebuild).
    const size_t  layer_off_bytes = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    ggml_tensor * K_att           = ggml_view_3d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_kv_heads, k_elem * kv_dim,
                                                 k_elem * head_dim, layer_off_bytes);
    ggml_tensor * V_att = ggml_view_3d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_kv_heads, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim);

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

    o = mul_mat_f32acc(ctx, view.attn_o_w, o);
    x = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    if (view.ffn_scale != nullptr) {
        ff_norm = ggml_mul(ctx, ff_norm, view.ffn_scale);
    }
    ggml_tensor * gate_up = mul_mat_f32acc(ctx, view.ffn_gate_up_w, ff_norm);
    ggml_tensor * ff      = ggml_swiglu(ctx, gate_up);
    ff                    = mul_mat_f32acc(ctx, view.ffn_down_w, ff);

    x = ggml_add(ctx, x, ff);
    return x;
}

// Block forward — multi-position step (T_seq positions, single utterance).
ggml_tensor * block_step_n(ggml_context *      ctx,
                           ggml_cgraph *       gf,
                           ggml_tensor *       x,
                           const BlockView &   view,
                           const BlockParams & params,
                           KvCache &           kv_cache,
                           int                 layer_idx,
                           int                 T_seq,
                           int                 max_n_kv,
                           ggml_tensor *       mask,
                           ggml_tensor *       positions,
                           ggml_tensor *       kv_idx,
                           bool                use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int     n_ctx      = kv_cache.n_ctx;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = 1.0f / std::sqrt(static_cast<float>(head_dim));

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = mul_mat_f32acc(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K = mul_mat_f32acc(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V = mul_mat_f32acc(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T_seq, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T_seq, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T_seq, 1);

    if (view.attn_q_norm != nullptr) {
        Q = ggml_mul(ctx, ggml_rms_norm(ctx, Q, rms_eps), view.attn_q_norm);
    }
    if (view.attn_k_norm != nullptr) {
        K = ggml_mul(ctx, ggml_rms_norm(ctx, K, rms_eps), view.attn_k_norm);
    }

    Q = ggml_rope_ext(ctx, Q, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // KV write: T_seq rows at indices kv_idx[T_seq].
    {
        const size_t layer_off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
        const size_t layer_off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;

        ggml_tensor * k_layer = ggml_view_2d(ctx, kv_cache.self_k, kv_dim, n_ctx, k_elem * kv_dim, layer_off_k);
        ggml_tensor * v_layer = ggml_view_2d(ctx, kv_cache.self_v, kv_dim, n_ctx, v_elem * kv_dim, layer_off_v);

        ggml_tensor * K_rows = ggml_reshape_2d(ctx, K, kv_dim, T_seq);
        ggml_tensor * V_rows = ggml_reshape_2d(ctx, V, kv_dim, T_seq);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_rows, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_rows, kv_idx));
    }

    const size_t  layer_off_bytes = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim;
    ggml_tensor * K_att           = ggml_view_3d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_kv_heads, k_elem * kv_dim,
                                                 k_elem * head_dim, layer_off_bytes);
    ggml_tensor * V_att = ggml_view_3d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_kv_heads, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim);

    // Q permute: [head_dim, n_heads, T_seq, 1] → [head_dim, T_seq, n_heads, 1]
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        o = ggml_reshape_2d(ctx, o, q_dim, T_seq);
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
        o                     = ggml_reshape_2d(ctx, o, q_dim, T_seq);
    }

    o = mul_mat_f32acc(ctx, view.attn_o_w, o);
    x = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    if (view.ffn_scale != nullptr) {
        ff_norm = ggml_mul(ctx, ff_norm, view.ffn_scale);
    }
    ggml_tensor * gate_up = mul_mat_f32acc(ctx, view.ffn_gate_up_w, ff_norm);
    ggml_tensor * ff      = ggml_swiglu(ctx, gate_up);
    ff                    = mul_mat_f32acc(ctx, view.ffn_down_w, ff);

    x = ggml_add(ctx, x, ff);
    return x;
}

// Block forward — batched step (B utterances).
ggml_tensor * block_step_batched(ggml_context *      ctx,
                                 ggml_cgraph *       gf,
                                 ggml_tensor *       x,  // [hidden, B]
                                 const BlockView &   view,
                                 const BlockParams & params,
                                 KvCache &           kv_cache,
                                 int                 layer_idx,
                                 int                 max_n_kv,
                                 int                 n_batch,
                                 ggml_tensor *       mask,      // [max_n_kv, 1, 1, B] f16
                                 ggml_tensor *       position,  // [B] i32
                                 ggml_tensor *       kv_idx,    // [1, B] i64
                                 bool                use_flash) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    const int64_t kv_dim     = n_kv_heads * head_dim;
    const int64_t n_ctx      = kv_cache.n_ctx;
    const int64_t B          = n_batch;
    const float   rms_eps    = params.rms_eps;
    const float   rope_theta = params.rope_theta;
    const float   scale_attn = 1.0f / std::sqrt(static_cast<float>(head_dim));

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = mul_mat_f32acc(ctx, view.attn_q_w, x_norm);  // [q_dim, B]
    ggml_tensor * K = mul_mat_f32acc(ctx, view.attn_k_w, x_norm);  // [kv_dim, B]
    ggml_tensor * V = mul_mat_f32acc(ctx, view.attn_v_w, x_norm);  // [kv_dim, B]

    // Batch on ne[2] (the per-token/position axis) so RoPE applies each
    // utterance's own position. T == 1 per utterance, so ne[2] == B.
    Q = ggml_reshape_3d(ctx, Q, head_dim, n_heads, B);
    K = ggml_reshape_3d(ctx, K, head_dim, n_kv_heads, B);
    V = ggml_reshape_3d(ctx, V, head_dim, n_kv_heads, B);

    if (view.attn_q_norm != nullptr) {
        Q = ggml_mul(ctx, ggml_rms_norm(ctx, Q, rms_eps), view.attn_q_norm);
    }
    if (view.attn_k_norm != nullptr) {
        K = ggml_mul(ctx, ggml_rms_norm(ctx, K, rms_eps), view.attn_k_norm);
    }

    // position [B] indexes ne[2]: row b is rotated by position[b].
    Q = ggml_rope_ext(ctx, Q, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, position, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // Batched KV write: per layer the cache slab is [kv_dim, n_ctx, B]; one
    // ggml_set_rows writes B rows at B independent indices kv_idx[b].
    {
        const size_t layer_off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        const size_t layer_off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;

        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, kv_dim, n_ctx, B, k_elem * kv_dim, k_elem * kv_dim * n_ctx, layer_off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, kv_dim, n_ctx, B, v_elem * kv_dim, v_elem * kv_dim * n_ctx, layer_off_v);

        ggml_tensor * K_row = ggml_reshape_3d(ctx, K, kv_dim, 1, B);
        ggml_tensor * V_row = ggml_reshape_3d(ctx, V, kv_dim, 1, B);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_row, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_row, kv_idx));
    }

    // Attention read: [head_dim, max_n_kv, n_kv_heads, B] per layer.
    const size_t  layer_off_bytes_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    const size_t  layer_off_bytes_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    ggml_tensor * K_att             = ggml_view_4d(ctx, kv_cache.self_k, head_dim, max_n_kv, n_kv_heads, B,
                                                   /*nb1=*/k_elem * kv_dim,
                                                   /*nb2=*/k_elem * head_dim,
                                                   /*nb3=*/k_elem * kv_dim * n_ctx, layer_off_bytes_k);
    ggml_tensor * V_att = ggml_view_4d(ctx, kv_cache.self_v, head_dim, max_n_kv, n_kv_heads, B, v_elem * kv_dim,
                                       v_elem * head_dim, v_elem * kv_dim * n_ctx, layer_off_bytes_v);

    // Q for flash: [head_dim, n_heads, B, 1] → [head_dim, 1, n_heads, B].
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 3, 1));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        // o = [head_dim, n_heads, 1, B] → [q_dim, B].
        o = ggml_reshape_2d(ctx, o, q_dim, B);
    } else {
        // The manual GQA path is single-shot only; batched decode requires
        // flash. Callers gate on use_flash and fall back to serial run().
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm block_step_batched: non-flash path unsupported");
        return nullptr;
    }

    o = mul_mat_f32acc(ctx, view.attn_o_w, o);  // [hidden, B]
    x = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    if (view.ffn_scale != nullptr) {
        ff_norm = ggml_mul(ctx, ff_norm, view.ffn_scale);
    }
    ggml_tensor * gate_up = mul_mat_f32acc(ctx, view.ffn_gate_up_w, ff_norm);
    ggml_tensor * ff      = ggml_swiglu(ctx, gate_up);
    ff                    = mul_mat_f32acc(ctx, view.ffn_down_w, ff);

    x = ggml_add(ctx, x, ff);
    return x;
}

// Block forward — batched prefill (B utterances, T tokens each).
ggml_tensor * block_prefill_batched(ggml_context *      ctx,
                                    ggml_cgraph *       gf,
                                    ggml_tensor *       x,  // [hidden, T, B]
                                    const BlockView &   view,
                                    const BlockParams & params,
                                    KvCache &           kv_cache,
                                    int                 layer_idx,
                                    int                 T_seq,
                                    int                 n_batch,
                                    ggml_tensor *       mask,       // [T_seq, T_seq] f16 causal
                                    ggml_tensor *       positions,  // [T_seq] i32
                                    ggml_tensor *       kv_idx,     // [T_seq, B] i64
                                    bool                use_flash) {
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
    const float   scale_attn = 1.0f / std::sqrt(static_cast<float>(head_dim));

    const size_t k_elem = ggml_element_size(kv_cache.self_k);
    const size_t v_elem = ggml_element_size(kv_cache.self_v);

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);

    ggml_tensor * Q = mul_mat_f32acc(ctx, view.attn_q_w, x_norm);  // [q_dim, T, B]
    ggml_tensor * K = mul_mat_f32acc(ctx, view.attn_k_w, x_norm);  // [kv_dim, T, B]
    ggml_tensor * V = mul_mat_f32acc(ctx, view.attn_v_w, x_norm);  // [kv_dim, T, B]

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T, B);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T, B);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T, B);

    if (view.attn_q_norm != nullptr) {
        Q = ggml_mul(ctx, ggml_rms_norm(ctx, Q, rms_eps), view.attn_q_norm);
    }
    if (view.attn_k_norm != nullptr) {
        K = ggml_mul(ctx, ggml_rms_norm(ctx, K, rms_eps), view.attn_k_norm);
    }

    // RoPE position indexes ne[2] (the time axis); shared across heads/batch.
    Q = ggml_rope_ext(ctx, Q, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // Batched KV write: each utterance's T rows -> [0, T) of its slab.
    {
        const size_t  layer_off_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        const size_t  layer_off_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
        ggml_tensor * k_layer =
            ggml_view_3d(ctx, kv_cache.self_k, kv_dim, n_ctx, B, k_elem * kv_dim, k_elem * kv_dim * n_ctx, layer_off_k);
        ggml_tensor * v_layer =
            ggml_view_3d(ctx, kv_cache.self_v, kv_dim, n_ctx, B, v_elem * kv_dim, v_elem * kv_dim * n_ctx, layer_off_v);

        // K/V are [head_dim, n_kv_heads, T, B]; collapse heads -> [kv_dim, T, B].
        ggml_tensor * K_rows = ggml_reshape_3d(ctx, K, kv_dim, T, B);
        ggml_tensor * V_rows = ggml_reshape_3d(ctx, V, kv_dim, T, B);

        ggml_build_forward_expand(gf, ggml_set_rows(ctx, k_layer, K_rows, kv_idx));
        ggml_build_forward_expand(gf, ggml_set_rows(ctx, v_layer, V_rows, kv_idx));
    }

    // Attention read: [head_dim, T, n_kv_heads, B] (positions [0, T)).
    const size_t  layer_off_bytes_k = k_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    const size_t  layer_off_bytes_v = v_elem * static_cast<size_t>(layer_idx) * n_ctx * kv_dim * B;
    ggml_tensor * K_att             = ggml_view_4d(ctx, kv_cache.self_k, head_dim, T, n_kv_heads, B, k_elem * kv_dim,
                                                   k_elem * head_dim, k_elem * kv_dim * n_ctx, layer_off_bytes_k);
    ggml_tensor * V_att             = ggml_view_4d(ctx, kv_cache.self_v, head_dim, T, n_kv_heads, B, v_elem * kv_dim,
                                                   v_elem * head_dim, v_elem * kv_dim * n_ctx, layer_off_bytes_v);

    // Q -> [head_dim, T, n_heads, B].
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, Q_att, K_att, V_att, mask, scale_attn, 0.0f, 0.0f);
        o = ggml_reshape_3d(ctx, o, q_dim, T, B);
    } else {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "causal_lm block_prefill_batched: non-flash unsupported");
        return nullptr;
    }

    o = mul_mat_f32acc(ctx, view.attn_o_w, o);  // [hidden, T, B]
    x = ggml_add(ctx, x, o);

    ggml_tensor * ff_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    if (view.ffn_scale != nullptr) {
        ff_norm = ggml_mul(ctx, ff_norm, view.ffn_scale);
    }
    ggml_tensor * gate_up = mul_mat_f32acc(ctx, view.ffn_gate_up_w, ff_norm);
    ggml_tensor * ff      = ggml_swiglu(ctx, gate_up);
    ff                    = mul_mat_f32acc(ctx, view.ffn_down_w, ff);

    x = ggml_add(ctx, x, ff);
    return x;
}

void PackedGateUpHandles::free() {
    if (buffer != nullptr) {
        safe_buffer_free(buffer);
        buffer = nullptr;
    }
    if (ctx != nullptr) {
        ggml_free(ctx);
        ctx = nullptr;
    }
}

bool pack_gate_up(ggml_backend_t                   backend,
                  int                              hidden,
                  int                              intermediate,
                  const std::vector<GateUpEntry> & entries,
                  PackedGateUpHandles &            out_handles,
                  const char *                     error_tag) {
    if (backend == nullptr || hidden <= 0 || intermediate <= 0 || entries.empty()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up invalid args (hidden=%d intermediate=%d n=%zu)",
                error_tag, hidden, intermediate, entries.size());
        return false;
    }

    const size_t     ctx_size = entries.size() * ggml_tensor_overhead() + 1024;
    ggml_init_params packed_params{};
    packed_params.mem_size   = ctx_size;
    packed_params.mem_buffer = nullptr;
    packed_params.no_alloc   = true;

    out_handles.ctx = ggml_init(packed_params);
    if (out_handles.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up ggml_init failed", error_tag);
        return false;
    }

    // One packed tensor per block, same dtype as gate_w (gate and up share
    // a row-wise quant block size).
    for (size_t i = 0; i < entries.size(); ++i) {
        const auto & e = entries[i];
        if (e.gate_w == nullptr || e.up_w == nullptr || e.gate_up_w_out == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up entry %zu has null member", error_tag, i);
            return false;
        }
        if (e.gate_w->type != e.up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up entry %zu gate/up type mismatch (%d vs %d)",
                    error_tag, i, static_cast<int>(e.gate_w->type), static_cast<int>(e.up_w->type));
            return false;
        }
        ggml_tensor * t = ggml_new_tensor_2d(out_handles.ctx, e.gate_w->type, hidden, 2 * intermediate);
        if (t == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up new_tensor_2d failed at %zu", error_tag, i);
            return false;
        }
        *e.gate_up_w_out = t;
    }

    out_handles.buffer = ggml_backend_alloc_ctx_tensors(out_handles.ctx, backend);
    if (out_handles.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up backend buffer alloc failed", error_tag);
        return false;
    }
    ggml_backend_buffer_set_usage(out_handles.buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    // Fill: concat-along-dim-1 is gate's bytes followed by up's bytes
    // (one CPU round-trip per block at load time).
    std::vector<uint8_t> buf;
    for (const auto & e : entries) {
        ggml_tensor * gate_up    = *e.gate_up_w_out;
        const size_t  gate_bytes = ggml_nbytes(e.gate_w);
        const size_t  up_bytes   = ggml_nbytes(e.up_w);
        if (ggml_nbytes(gate_up) != gate_bytes + up_bytes) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: pack_gate_up size mismatch (%zu vs %zu + %zu)", error_tag,
                    ggml_nbytes(gate_up), gate_bytes, up_bytes);
            return false;
        }
        buf.resize(std::max(gate_bytes, up_bytes));
        ggml_backend_tensor_get(e.gate_w, buf.data(), 0, gate_bytes);
        ggml_backend_tensor_set(gate_up, buf.data(), 0, gate_bytes);
        ggml_backend_tensor_get(e.up_w, buf.data(), 0, up_bytes);
        ggml_backend_tensor_set(gate_up, buf.data(), gate_bytes, up_bytes);
    }
    return true;
}

transcribe_status run_batched_step_loop(transcribe_session *                session,
                                        ggml_backend_sched_t                sched,
                                        const StepBatchedIO &               io,
                                        int                                 n_batch,
                                        int                                 max_n_kv,
                                        int32_t                             eos_id,
                                        int                                 max_new,
                                        const StepBatchedState &            state,
                                        std::vector<std::vector<int32_t>> & generated,
                                        StepLoopStats *                     stats,
                                        std::vector<char> *                 truncated_out) {
    const int n = n_batch;

    // Per-row working state.
    std::vector<int32_t>      next_tok = state.next_tok;
    std::vector<int>          n_past   = state.n_past;
    const std::vector<char> & valid    = state.valid;
    std::vector<char>         finished(n, 1);
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            finished[b] = (next_tok[b] == eos_id);
        }
    }

    // Host-side step inputs.
    std::vector<int32_t>     ids_buf(n, 0), pos_buf(n, 0), out_buf(n, 0);
    std::vector<int64_t>     kvidx_buf(n, 0);
    const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> mask_buf(static_cast<size_t>(max_n_kv) * n, mn);
    // Initialise each row's mask: attendable [0, n_past[b]] (prompt + the first
    // generated token, which step 0 writes at row n_past[b]).
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        const size_t base = static_cast<size_t>(b) * max_n_kv;
        for (int c = 0; c <= n_past[b] && c < max_n_kv; ++c) {
            mask_buf[base + c] = mz;
        }
    }

    const int64_t t_step0  = ggml_time_us();
    int           n_steps  = 0;
    bool          all_done = false;
    while (!all_done) {
        if (session->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        for (int b = 0; b < n; ++b) {
            // Finished/invalid rows keep stepping with frozen, valid inputs;
            // their independent KV slab means the overwrite is a no-op for live
            // rows and their output is ignored.
            ids_buf[b]   = next_tok[b];
            pos_buf[b]   = n_past[b];
            kvidx_buf[b] = n_past[b];
        }
        ggml_backend_tensor_set(io.input_ids, ids_buf.data(), 0, ids_buf.size() * sizeof(int32_t));
        ggml_backend_tensor_set(io.positions, pos_buf.data(), 0, pos_buf.size() * sizeof(int32_t));
        ggml_backend_tensor_set(io.kv_idx, kvidx_buf.data(), 0, kvidx_buf.size() * sizeof(int64_t));
        ggml_backend_tensor_set(io.mask, mask_buf.data(), 0, mask_buf.size() * sizeof(ggml_fp16_t));

        if (ggml_backend_sched_graph_compute(sched, io.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_get(io.argmax, out_buf.data(), 0, out_buf.size() * sizeof(int32_t));
        ++n_steps;

        all_done = true;
        for (int b = 0; b < n; ++b) {
            if (finished[b] || !valid[b]) {
                continue;
            }
            const int32_t tok = out_buf[b];
            next_tok[b]       = tok;
            generated[b].push_back(tok);
            n_past[b] += 1;
            // Open the new KV position for the NEXT step's attention.
            const size_t base = static_cast<size_t>(b) * max_n_kv;
            if (n_past[b] < max_n_kv) {
                mask_buf[base + n_past[b]] = mz;
            }
            if (tok == eos_id || static_cast<int>(generated[b].size()) >= max_new || n_past[b] + 1 > max_n_kv) {
                finished[b] = 1;
            } else {
                all_done = false;
            }
        }
    }

    if (stats != nullptr) {
        stats->n_steps = n_steps;
        stats->step_us = ggml_time_us() - t_step0;
    }

    // A valid row was truncated if it stopped for a reason OTHER than eos
    // (generation budget or KV window). `finished` is set on every stop
    // reason, so it can't discriminate; the signal is the last sampled token:
    // `next_tok[b] != eos_id` means the row was cut off mid-transcript (it is
    // frozen once the row finishes). See docs/input-limits.md.
    if (truncated_out != nullptr) {
        truncated_out->assign(n, 0);
        for (int b = 0; b < n; ++b) {
            (*truncated_out)[b] = (valid[b] && next_tok[b] != eos_id) ? 1 : 0;
        }
    }
    return TRANSCRIBE_OK;
}

}  // namespace transcribe::causal_lm
