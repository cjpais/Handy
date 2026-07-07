// arch/funasr_nano/adaptor.cpp - audio adaptor graph.

#include "adaptor.h"

#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::funasr_nano {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

// Weight matmul forcing F32 accumulation for F16 weights, so CUDA's cuBLAS
// path does not accumulate the adaptor's multi-token GEMMs in F16 (which
// overflows F16's ~65504 range -> NaNs). Gated on F16 so BF16/quantized/F32
// weights and CPU/Metal are bit-identical. See sanm.cpp / causal_lm.cpp.
ggml_tensor * mul_mat_f32acc(ggml_context * ctx, ggml_tensor * w, ggml_tensor * x) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (w->type == GGML_TYPE_F16) {
        ggml_mul_mat_set_prec(y, GGML_PREC_F32);
    }
    return y;
}

ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta, float eps) {
    ggml_tensor * y = ggml_norm(ctx, x, eps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

ggml_tensor * adaptor_attention(ggml_context *       ctx,
                                ggml_tensor *        x,
                                const AdaptorBlock & b,
                                int                  n_heads,
                                int                  head_dim,
                                int                  llm_dim) {
    const int64_t T     = x->ne[1];
    const float   scale = 1.0f / std::sqrt(static_cast<float>(head_dim));

    // q, k, v: separate linears with biases (plain MultiHeadedAttention).
    ggml_tensor * q = mul_mat_f32acc(ctx, b.attn_q_w, x);
    q               = ggml_add(ctx, q, b.attn_q_b);
    ggml_tensor * k = mul_mat_f32acc(ctx, b.attn_k_w, x);
    k               = ggml_add(ctx, k, b.attn_k_b);
    ggml_tensor * v = mul_mat_f32acc(ctx, b.attn_v_w, x);
    v               = ggml_add(ctx, v, b.attn_v_b);

    auto split_heads = [&](ggml_tensor * t) {
        return ggml_reshape_4d(ctx, t, head_dim, n_heads, T, 1);
    };
    q = split_heads(q);
    k = split_heads(k);
    v = split_heads(v);

    auto to_attn_layout = [&](ggml_tensor * t) {
        t = ggml_permute(ctx, t, 0, 2, 1, 3);  // [head_dim, T, n_heads, 1]
        return ggml_cont(ctx, t);
    };
    q = to_attn_layout(q);
    k = to_attn_layout(k);
    v = to_attn_layout(v);

    ggml_tensor * o =
        ggml_flash_attn_ext(ctx, q, k, v, /*mask=*/nullptr, scale, /*max_bias=*/0.0f, /*logit_softcap=*/0.0f);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_2d(ctx, o, llm_dim, T);

    o = mul_mat_f32acc(ctx, b.attn_out_w, o);
    o = ggml_add(ctx, o, b.attn_out_b);
    return o;
}

ggml_tensor * adaptor_ffn(ggml_context * ctx, ggml_tensor * x, const AdaptorBlock & b) {
    ggml_tensor * h = mul_mat_f32acc(ctx, b.ffn_fc1_w, x);
    h               = ggml_add(ctx, h, b.ffn_fc1_b);
    h               = ggml_relu(ctx, h);
    h               = mul_mat_f32acc(ctx, b.ffn_fc2_w, h);
    h               = ggml_add(ctx, h, b.ffn_fc2_b);
    return h;
}

ggml_tensor * adaptor_block(ggml_context *       ctx,
                            ggml_tensor *        x,
                            const AdaptorBlock & b,
                            int                  n_heads,
                            int                  head_dim,
                            int                  llm_dim,
                            float                eps) {
    // Pre-LN MHA + residual.
    ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b, eps);
    y               = adaptor_attention(ctx, y, b, n_heads, head_dim, llm_dim);
    x               = ggml_add(ctx, x, y);

    // Pre-LN FFN + residual.
    ggml_tensor * z = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b, eps);
    z               = adaptor_ffn(ctx, z, b);
    return ggml_add(ctx, x, z);
}

void mark_dump(ggml_tensor *& slot, ggml_tensor * t, const char * name) {
    named(t, name);
    transcribe::debug::mark_tensor_for_dump(t);
    slot = t;
}

}  // namespace

int compute_fake_token_len(int T_lfr, bool use_low_frame_rate) {
    if (!use_low_frame_rate) {
        return T_lfr;
    }
    // Floor division over PyTorch ints; T_lfr ≥ 0.
    int o1 = 1 + (T_lfr - 3 + 2) / 2;
    int o2 = 1 + (o1 - 3 + 2) / 2;
    int o3 = (o2 - 1) / 2 + 1;
    return o3;
}

AdaptorBuild build_adaptor_graph(ggml_context *            ctx,
                                 const FunAsrNanoWeights & w,
                                 const FunAsrNanoHParams & hp,
                                 int                       T_in) {
    AdaptorBuild ab{};
    if (ctx == nullptr || T_in <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano adaptor: invalid arg (T_in=%d)", T_in);
        return ab;
    }

    const int   encoder_dim = hp.adaptor_encoder_dim;
    const int   llm_dim     = hp.adaptor_llm_dim;
    const int   n_heads     = hp.adaptor_n_heads;
    const int   head_dim    = hp.adaptor_d_head;
    const float eps         = hp.adaptor_layer_norm_eps;

    ab.enc_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, encoder_dim, T_in);
    named(ab.enc_in, "adaptor.enc_in");
    ggml_set_input(ab.enc_in);

    // linear1 (512 → 2048) + bias.
    ggml_tensor * x = mul_mat_f32acc(ctx, w.adaptor.linear1_w, ab.enc_in);
    x               = ggml_add(ctx, x, w.adaptor.linear1_b);
    mark_dump(ab.dumps.linear1_out, x, "adaptor.linear1.out");

    x = ggml_relu(ctx, x);

    // linear2 (2048 → 1024) + bias.
    x = mul_mat_f32acc(ctx, w.adaptor.linear2_w, x);
    x = ggml_add(ctx, x, w.adaptor.linear2_b);
    mark_dump(ab.dumps.linear2_out, x, "adaptor.linear2.out");

    // Transformer blocks.
    const int n_b = static_cast<int>(w.adaptor.blocks.size());
    for (int i = 0; i < n_b; ++i) {
        x = adaptor_block(ctx, x, w.adaptor.blocks[i], n_heads, head_dim, llm_dim, eps);
        if (i == 0) {
            mark_dump(ab.dumps.block0_out, x, "adaptor.blocks.0.out");
        }
    }
    mark_dump(ab.dumps.adaptor_out, x, "adaptor.out");

    ab.out = x;
    ggml_set_output(ab.out);

    ab.graph = ggml_new_graph_custom(ctx, /*size=*/2048, /*grads=*/false);
    if (ab.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano adaptor: ggml_new_graph_custom failed");
        return ab;
    }
    ggml_build_forward_expand(ab.graph, ab.out);
    return ab;
}

}  // namespace transcribe::funasr_nano
