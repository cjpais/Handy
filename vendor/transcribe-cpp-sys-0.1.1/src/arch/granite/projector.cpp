// arch/granite/projector.cpp - Granite Speech BLIP-2 Q-Former projector.
//
// Reference: GraniteSpeechEncoderProjector +
// Blip2QFormerLayer/Encoder/Model in transformers/models/blip_2/.
//
// Conventions:
//   * BLIP-2 layer uses "post-LN" (residual + LN AFTER each output
//     projection), unlike the granite encoder's pre-LN macaron / pre-LN
//     attention pattern.
//   * Q-Former head_dim = hidden / num_heads = 1024 / 16 = 64
//     (independent of the encoder's head_dim=128).
//   * No positional embeddings on the query stream.
//   * GELU is the PyTorch exact-erf form (granite config `hidden_act="gelu"`).

#include "projector.h"

#include "ggml.h"
#include "granite.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::granite {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

// LayerNorm with eps from the granite GGUF (layer_norm_eps=1e-12 per
// the BLIP-2 Q-Former config) and explicit gamma/beta.
ggml_tensor * qformer_layer_norm(ggml_context * ctx,
                                 ggml_tensor *  x,
                                 ggml_tensor *  gamma,
                                 ggml_tensor *  beta,
                                 float          eps) {
    ggml_tensor * y = ggml_norm(ctx, x, eps);
    y               = ggml_mul(ctx, y, gamma);
    if (beta != nullptr) {
        y = ggml_add(ctx, y, beta);
    }
    return y;
}

ggml_tensor * linear(ggml_context * ctx, ggml_tensor * x, ggml_tensor * w, ggml_tensor * b) {
    ggml_tensor * y = ggml_mul_mat(ctx, w, x);
    if (b != nullptr) {
        y = ggml_add(ctx, y, b);
    }
    return y;
}

// Scaled-dot-product attention with input projections already applied.
// q, k, v all have ne = [hidden, n_seq, batch]; attention runs per `batch`
// (= nblocks for cross-attn). head_dim = hidden / n_heads.
ggml_tensor * qformer_attn(ggml_context * ctx,
                           ggml_tensor *  q_in,  // [hidden, q_seq, batch]
                           ggml_tensor *  k_in,  // [hidden, k_seq, batch]
                           ggml_tensor *  v_in,  // [hidden, k_seq, batch]
                           int            n_heads,
                           int            head_dim) {
    const int64_t hidden = q_in->ne[0];
    const int64_t q_seq  = q_in->ne[1];
    const int64_t k_seq  = k_in->ne[1];
    const int64_t batch  = q_in->ne[2];
    const float   scale  = 1.0f / std::sqrt(static_cast<float>(head_dim));
    (void) hidden;

    // Reshape into (head_dim, n_heads, seq, batch).
    auto split_heads = [&](ggml_tensor * t, int64_t seq) {
        ggml_tensor * r = ggml_reshape_4d(ctx, t, head_dim, n_heads, seq, batch);
        // Permute to (head_dim, seq, n_heads, batch): args (0, 2, 1, 3).
        r               = ggml_cont(ctx, ggml_permute(ctx, r, 0, 2, 1, 3));
        return r;
    };
    ggml_tensor * q = split_heads(q_in, q_seq);
    ggml_tensor * k = split_heads(k_in, k_seq);
    ggml_tensor * v = split_heads(v_in, k_seq);

    // scores = QK^T / sqrt(head_dim).
    // ggml_mul_mat(k, q) on (head_dim, seq, n_heads, batch) yields
    // (k_seq, q_seq, n_heads, batch).
    ggml_tensor * kq = ggml_mul_mat(ctx, k, q);
    kq               = ggml_scale(ctx, kq, scale);

    // Softmax over ne[0] = k_seq.
    ggml_tensor * attn = ggml_soft_max(ctx, kq);
    // attn ne = [k_seq, q_seq, n_heads, batch]

    // V^T: permute V to (k_seq, head_dim, n_heads, batch) so the
    // contraction axis (k_seq) is at ne[0].
    ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
    ggml_tensor * out = ggml_mul_mat(ctx, v_t, attn);
    // out ne = [head_dim, q_seq, n_heads, batch]

    // Collapse back to (hidden, q_seq, batch). First permute to put
    // n_heads adjacent to head_dim so the reshape interleaves correctly:
    out = ggml_cont(ctx, ggml_permute(ctx, out, 0, 2, 1, 3));
    // out ne = [head_dim, n_heads, q_seq, batch]
    out = ggml_reshape_3d(ctx, out, static_cast<int64_t>(head_dim) * n_heads, q_seq, batch);
    return out;
}

// One BLIP-2 Q-Former layer.
//
// Granite uses cross_attention_frequency=1 (every layer has cross-attn).
ggml_tensor * qformer_layer(ggml_context *           ctx,
                            ggml_tensor *            query,       // [hidden, num_queries, nblocks]
                            ggml_tensor *            enc_window,  // [hidden_enc, window_size, nblocks]
                            const GraniteProjBlock & b,
                            int                      n_heads,
                            int                      head_dim,
                            float                    ln_eps) {
    // -------- Self-attention sublayer --------
    ggml_tensor * sa_in = query;
    ggml_tensor * sa_q  = linear(ctx, sa_in, b.self_attn_q_w, b.self_attn_q_b);
    ggml_tensor * sa_k  = linear(ctx, sa_in, b.self_attn_k_w, b.self_attn_k_b);
    ggml_tensor * sa_v  = linear(ctx, sa_in, b.self_attn_v_w, b.self_attn_v_b);
    ggml_tensor * sa    = qformer_attn(ctx, sa_q, sa_k, sa_v, n_heads, head_dim);
    sa                  = linear(ctx, sa, b.self_attn_out_w, b.self_attn_out_b);
    sa                  = ggml_add(ctx, sa, sa_in);
    sa                  = qformer_layer_norm(ctx, sa, b.norm_self_attn_w, b.norm_self_attn_b, ln_eps);

    // -------- Cross-attention sublayer --------
    ggml_tensor * ca_in = sa;
    ggml_tensor * ca_q  = linear(ctx, ca_in, b.cross_attn_q_w, b.cross_attn_q_b);
    ggml_tensor * ca_k  = linear(ctx, enc_window, b.cross_attn_k_w, b.cross_attn_k_b);
    ggml_tensor * ca_v  = linear(ctx, enc_window, b.cross_attn_v_w, b.cross_attn_v_b);
    ggml_tensor * ca    = qformer_attn(ctx, ca_q, ca_k, ca_v, n_heads, head_dim);
    ca                  = linear(ctx, ca, b.cross_attn_out_w, b.cross_attn_out_b);
    ca                  = ggml_add(ctx, ca, ca_in);
    ca                  = qformer_layer_norm(ctx, ca, b.norm_cross_attn_w, b.norm_cross_attn_b, ln_eps);

    // -------- FFN sublayer --------
    ggml_tensor * ffn_in  = ca;
    ggml_tensor * mid     = linear(ctx, ffn_in, b.ffn_up_w, b.ffn_up_b);
    // PyTorch's `gelu` (the default) is the exact erf form.
    mid                   = ggml_gelu_erf(ctx, mid);
    ggml_tensor * ffn_out = linear(ctx, mid, b.ffn_down_w, b.ffn_down_b);
    ffn_out               = ggml_add(ctx, ffn_out, ffn_in);
    ffn_out               = qformer_layer_norm(ctx, ffn_out, b.norm_ffn_w, b.norm_ffn_b, ln_eps);

    return ffn_out;
}

}  // namespace

ProjectorBuild build_projector_graph(ggml_context *         ctx,
                                     const GraniteWeights & weights,
                                     const GraniteHParams & hp,
                                     int                    T_enc) {
    ProjectorBuild pb{};

    const int window_size = hp.window_size;              // 15
    const int downsample  = hp.downsample_rate;          // 5
    const int num_queries = window_size / downsample;    // 3
    const int n_heads     = hp.prj_n_heads;              // 16
    const int prj_hidden  = hp.prj_hidden;               // 1024
    const int enc_hidden  = hp.prj_encoder_hidden_size;  // 1024 or 2048 (-plus)
    (void) hp.dec_hidden;                                // text_hidden = 2048; consumed via proj.linear shape.
    const float ln_eps = hp.prj_layer_norm_eps;          // 1e-12

    if (prj_hidden % n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite projector: prj_hidden (%d) not divisible by "
                "n_heads (%d)",
                prj_hidden, n_heads);
        return pb;
    }
    const int head_dim = prj_hidden / n_heads;  // 64

    pb.nblocks        = (T_enc + window_size - 1) / window_size;
    const int T_pad   = pb.nblocks * window_size;
    pb.t_enc_pad      = T_pad - T_enc;
    pb.n_audio_tokens = pb.nblocks * num_queries;

    // Graph inputs.
    pb.enc_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, enc_hidden, T_enc);
    named(pb.enc_in, "proj.enc_in");
    ggml_set_input(pb.enc_in);

    if (pb.t_enc_pad > 0) {
        pb.enc_pad = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, enc_hidden, pb.t_enc_pad);
        named(pb.enc_pad, "proj.enc_pad");
        ggml_set_input(pb.enc_pad);
    }

    // Build window tensor.
    // enc_full: [hidden_enc, T_pad] (concat of enc_in and enc_pad).
    ggml_tensor * enc_full = pb.enc_in;
    if (pb.enc_pad != nullptr) {
        enc_full = ggml_concat(ctx, pb.enc_in, pb.enc_pad, /*dim=*/1);
    }
    // Reshape to [hidden_enc, window_size, nblocks]. The reference's
    // .view(bsz*nblocks, window_size, dim) lays frames contiguously
    // within each window: position i in T_pad → (block = i / window_size,
    // pos_in_window = i % window_size). The ggml reshape with
    // (hidden_enc, window_size, nblocks) gives ne[1] = window_size as
    // the faster axis, exactly matching this layout.
    ggml_tensor * enc_window = ggml_reshape_3d(ctx, enc_full, enc_hidden, window_size, pb.nblocks);

    // Build query tensor.
    // weights.proj_top.query is stored at the GGUF's reference dtype
    // (BF16 for granite). The QFormer layer adds it into the F32
    // post-attention activation and feeds the result into LayerNorm;
    // ggml_norm's internal centring-subtract expects matching dtypes
    // on both inputs, so we cast the query to F32 up front.
    ggml_tensor * query = ggml_cast(ctx, weights.proj_top.query, GGML_TYPE_F32);

    // Apply the Q-Former INPUT layernorm. Despite the GGUF tensor name
    // "proj.qformer.final_norm" (a converter-side misnomer), this is
    // Blip2QFormerModel.layernorm and runs BEFORE the encoder stack:
    //   embedding_output = self.layernorm(query_embeds)
    // Applying it after the layers instead silently corrupts the output —
    // the layers would operate on unnormalised queries.
    query = qformer_layer_norm(ctx, query, weights.proj_top.qformer_final_norm_w, weights.proj_top.qformer_final_norm_b,
                               ln_eps);

    // Broadcast (hidden, num_queries, 1, 1) → (hidden, num_queries, nblocks, 1).
    query = ggml_repeat_4d(ctx, query, prj_hidden, num_queries, pb.nblocks, 1);
    // ggml_repeat_4d emits a fresh contiguous tensor; we can pass it
    // directly into subsequent ops.

    // Q-Former layers.
    for (int i = 0; i < hp.prj_n_layers; ++i) {
        query = qformer_layer(ctx, query, enc_window, weights.proj_blocks[i], n_heads, head_dim, ln_eps);
    }

    // proj.qformer.out = Blip2QFormerModel.last_hidden_state, the encoder
    // stack output — there is no layernorm after the layers (the input norm
    // above is the only wrapper layernorm).
    named(query, "proj.qformer.out");
    pb.dumps.qformer_out = query;
    transcribe::debug::mark_tensor_for_dump(query);

    // Reshape to [hidden, num_queries*nblocks] and linear lift.
    // The reference's .view(batch_size, nblocks*num_queries, hidden) is
    // a flat reshape over (block_idx, query_idx). In ggml ne layout
    // (hidden, num_queries, nblocks) the natural reshape to
    // (hidden, num_queries*nblocks) interleaves as
    //   flat_idx = block_idx * num_queries + query_idx
    // because num_queries is faster than nblocks. Exactly matches the
    // upstream order.
    ggml_tensor * tokens = ggml_reshape_2d(ctx, query, prj_hidden, static_cast<int64_t>(num_queries) * pb.nblocks);

    // Linear lift to text_hidden.
    ggml_tensor * out = ggml_mul_mat(ctx, weights.proj_top.linear_w, tokens);
    out               = ggml_add(ctx, out, weights.proj_top.linear_b);
    named(out, "proj.out");
    pb.out            = out;
    pb.dumps.proj_out = out;
    ggml_set_output(out);
    transcribe::debug::mark_tensor_for_dump(out);

    // The Q-Former graph has on the order of:
    //   2 layers × (3 sublayers × 5 op-cluster + Q/K/V/out/LN ≈ 25 ops)
    //   plus the final LN + reshape + linear lift ≈ 55 ops total.
    // 4096 nodes is plenty of headroom.
    pb.graph = ggml_new_graph_custom(ctx, /*size=*/4096, /*grads=*/false);
    if (pb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite projector: ggml_new_graph_custom failed");
        return pb;
    }
    ggml_build_forward_expand(pb.graph, pb.out);
    if (pb.dumps.qformer_out) {
        ggml_build_forward_expand(pb.graph, pb.dumps.qformer_out);
    }

    return pb;
}

}  // namespace transcribe::granite
