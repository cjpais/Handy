// arch/granite_nar/decoder.cpp - bidirectional Granite-4 LM forward.
//
// Reference: NLENARDecoder (modeling_nle.py). Same block math as
// AR Granite-4 (pre-RMSNorm GQA, NeoX RoPE, attention_multiplier scaled
// softmax, residual_multiplier scaled residuals, SwiGLU MLP) but
// is_causal=False on every layer (BIDIRECTIONAL) and no KV cache.

#include "decoder.h"

#include "ggml.h"
#include "granite_nar.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

namespace transcribe::granite_nar {

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

struct BlockParams {
    int   n_heads;
    int   n_kv_heads;
    int   head_dim;
    int   max_position;
    float rms_eps;
    float rope_theta;
    float attn_scale;
    float residual_mul;
};

BlockParams to_params(const GraniteNarHParams & hp) {
    BlockParams p{};
    p.n_heads      = hp.dec_n_heads;
    p.n_kv_heads   = hp.dec_n_kv_heads;
    p.head_dim     = hp.dec_head_dim;
    p.max_position = hp.dec_max_pos_emb;
    p.rms_eps      = hp.dec_rms_norm_eps;
    p.rope_theta   = hp.dec_rope_theta;
    p.attn_scale   = hp.dec_attention_multiplier;
    p.residual_mul = hp.dec_residual_multiplier;
    return p;
}

// One bidirectional decoder block.
// x: [hidden, T_total]. positions: [T_total] i32.
ggml_tensor * block_bidi(ggml_context *             ctx,
                         ggml_tensor *              x,
                         const GraniteNarDecBlock & view,
                         const BlockParams &        params,
                         int                        T_total,
                         ggml_tensor *              positions) {
    const int64_t n_heads    = params.n_heads;
    const int64_t n_kv_heads = params.n_kv_heads;
    const int64_t n_groups   = n_heads / n_kv_heads;
    const int64_t head_dim   = params.head_dim;
    const int64_t q_dim      = n_heads * head_dim;
    (void) q_dim;
    const float rms_eps    = params.rms_eps;
    const float rope_theta = params.rope_theta;
    const float scale_attn = params.attn_scale;

    ggml_tensor * x_norm = rms_norm(ctx, x, view.norm_attn_w, rms_eps);
    ggml_tensor * Q      = ggml_mul_mat(ctx, view.attn_q_w, x_norm);
    ggml_tensor * K      = ggml_mul_mat(ctx, view.attn_k_w, x_norm);
    ggml_tensor * V      = ggml_mul_mat(ctx, view.attn_v_w, x_norm);

    Q = ggml_reshape_4d(ctx, Q, head_dim, n_heads, T_total, 1);
    K = ggml_reshape_4d(ctx, K, head_dim, n_kv_heads, T_total, 1);
    V = ggml_reshape_4d(ctx, V, head_dim, n_kv_heads, T_total, 1);

    Q = ggml_rope_ext(ctx, Q, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);
    K = ggml_rope_ext(ctx, K, positions, nullptr, static_cast<int>(head_dim), GGML_ROPE_TYPE_NEOX, params.max_position,
                      rope_theta, 1.0f, 0.0f, 1.0f, 32.0f, 1.0f);

    // Permute to [head_dim, T_total, n_kv_heads] (effective layout for
    // mul_mat with q permuted to [head_dim, T_total, n_heads]).
    ggml_tensor * Q_att = ggml_cont(ctx, ggml_permute(ctx, Q, 0, 2, 1, 3));
    ggml_tensor * K_att = ggml_cont(ctx, ggml_permute(ctx, K, 0, 2, 1, 3));
    ggml_tensor * V_att = ggml_cont(ctx, ggml_permute(ctx, V, 0, 2, 1, 3));

    // GQA: repeat each KV head across the n_groups Q heads.
    ggml_tensor * K_full;
    ggml_tensor * V_full;
    if (n_groups == 1) {
        K_full = K_att;
        V_full = V_att;
    } else {
        ggml_tensor * K_4d  = ggml_reshape_4d(ctx, K_att, head_dim, T_total, 1, n_kv_heads);
        ggml_tensor * V_4d  = ggml_reshape_4d(ctx, V_att, head_dim, T_total, 1, n_kv_heads);
        ggml_tensor * K_tpl = ggml_new_tensor_4d(ctx, K_att->type, head_dim, T_total, n_groups, n_kv_heads);
        ggml_tensor * V_tpl = ggml_new_tensor_4d(ctx, V_att->type, head_dim, T_total, n_groups, n_kv_heads);
        ggml_tensor * K_rep = ggml_repeat(ctx, K_4d, K_tpl);
        ggml_tensor * V_rep = ggml_repeat(ctx, V_4d, V_tpl);
        K_full              = ggml_reshape_3d(ctx, K_rep, head_dim, T_total, n_heads);
        V_full              = ggml_reshape_3d(ctx, V_rep, head_dim, T_total, n_heads);
    }

    // scores = (K^T Q) * scale_attn. ggml_mul_mat(K, Q) yields
    // [T_total_k, T_total_q, n_heads].
    ggml_tensor * kq      = ggml_mul_mat(ctx, K_full, Q_att);
    // No mask (bidirectional). Use soft_max_ext with mask=nullptr for the
    // scaling, then softmax.
    ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale_attn, /*max_bias=*/0.0f);

    ggml_tensor * V_t = ggml_cont(ctx, ggml_permute(ctx, V_full, 1, 0, 2, 3));
    ggml_tensor * o   = ggml_mul_mat(ctx, V_t, kq_soft);
    o                 = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
    o                 = ggml_reshape_2d(ctx, o, static_cast<int64_t>(n_heads) * head_dim, T_total);

    o = ggml_mul_mat(ctx, view.attn_o_w, o);
    o = ggml_scale(ctx, o, params.residual_mul);
    x = ggml_add(ctx, x, o);

    // MLP sub-layer (SwiGLU).
    ggml_tensor * f_norm = rms_norm(ctx, x, view.norm_ffn_w, rms_eps);
    ggml_tensor * gate   = ggml_mul_mat(ctx, view.ffn_gate_w, f_norm);
    ggml_tensor * up     = ggml_mul_mat(ctx, view.ffn_up_w, f_norm);
    ggml_tensor * ff     = ggml_mul(ctx, ggml_silu(ctx, gate), up);
    ff                   = ggml_mul_mat(ctx, view.ffn_down_w, ff);
    ff                   = ggml_scale(ctx, ff, params.residual_mul);
    x                    = ggml_add(ctx, x, ff);
    return x;
}

}  // namespace

ForwardBuild build_forward_graph(ggml_context *            ctx,
                                 const GraniteNarWeights & weights,
                                 const GraniteNarHParams & hp,
                                 int                       n_audio_tokens,
                                 int                       n_text) {
    ForwardBuild fb{};
    fb.n_audio_tokens = n_audio_tokens;
    fb.n_text         = n_text;
    fb.T_total        = n_audio_tokens + n_text;

    if (ctx == nullptr || n_audio_tokens <= 0 || n_text <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar decoder: invalid (n_audio=%d, n_text=%d)", n_audio_tokens,
                n_text);
        return fb;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;
    const float   emb_mul = hp.dec_embedding_multiplier;
    const auto    params  = to_params(hp);

    // Inputs.
    fb.audio_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, n_audio_tokens);
    named(fb.audio_in, "dec.audio_in");
    ggml_set_input(fb.audio_in);

    fb.text_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, n_text);
    named(fb.text_ids_in, "dec.text_ids");
    ggml_set_input(fb.text_ids_in);

    fb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, fb.T_total);
    named(fb.positions_in, "dec.positions");
    ggml_set_input(fb.positions_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar decoder: ggml_new_graph_custom failed");
        return fb;
    }
    fb.graph = gf;

    // Flat input embeds.
    // text embeds via lookup. ggml_get_rows returns the embed table dtype
    // (BF16 here); cast to F32 so ggml_concat with the F32 audio_in is
    // legal (concat requires matching dtypes).
    ggml_tensor * text_emb = ggml_get_rows(ctx, weights.dec_embed.token_w, fb.text_ids_in);
    if (text_emb->type != GGML_TYPE_F32) {
        text_emb = ggml_cast(ctx, text_emb, GGML_TYPE_F32);
    }
    // text_emb ne = [hidden, n_text]

    // Concat audio_in (already / embedding_multiplier) with text_emb
    // along the sequence axis.
    ggml_tensor * flat = ggml_concat(ctx, fb.audio_in, text_emb, /*dim=*/1);
    named(flat, "dec.flat_embeds");
    fb.dumps.flat_embeds = flat;
    transcribe::debug::mark_tensor_for_dump(flat);

    // Apply embedding_multiplier on the whole flat sequence (the
    // reference's GraniteSpeechForConditionalGeneration does this; the
    // audio_in is pre-divided so the post-multiplier audio rows
    // round-trip through ×(1/emb_mul)×emb_mul = identity).
    ggml_tensor * x = ggml_scale(ctx, flat, emb_mul);

    // Block stack.
    for (int il = 0; il < n_layer; ++il) {
        x = block_bidi(ctx, x, weights.dec_blocks[il], params, fb.T_total, fb.positions_in);
    }

    // Final RMSNorm.
    x = rms_norm(ctx, x, weights.dec_final.norm_w, rms_eps);

    // Slice text portion.
    // text_x = x[:, n_audio_tokens:]
    ggml_tensor * text_x = ggml_view_2d(ctx, x, hidden, n_text, ggml_element_size(x) * hidden,
                                        ggml_element_size(x) * hidden * static_cast<size_t>(n_audio_tokens));
    text_x               = ggml_cont(ctx, text_x);

    // LM head (tied to token_embd). The Granite-4 head divides by
    // logits_scaling; we mirror it so dec.text_logits aligns numerically with
    // the reference dump. Argmax is scale-invariant, so the transcript is
    // unchanged by this division.
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, text_x);
    if (hp.dec_logits_scaling > 0.0f && hp.dec_logits_scaling != 1.0f) {
        logits = ggml_scale(ctx, logits, 1.0f / hp.dec_logits_scaling);
    }
    // ne = [vocab, n_text]
    named(logits, "dec.text_logits");
    fb.out               = logits;
    fb.dumps.text_logits = logits;
    ggml_set_output(logits);
    transcribe::debug::mark_tensor_for_dump(logits);

    ggml_build_forward_expand(gf, fb.out);
    if (fb.dumps.flat_embeds) {
        ggml_build_forward_expand(gf, fb.dumps.flat_embeds);
    }

    return fb;
}

void add_insertion_slots(const std::vector<int32_t> & hyp_ids, int32_t eos_id, std::vector<int32_t> & out) {
    // Reference: total_len = max(2 * n + 1, 8). EOS is inserted between
    // and around every CTC token (positions 0, 2, 4, ...); CTC tokens
    // occupy odd positions. Length floored at 8 to give the bidirectional
    // editor a minimum context to operate over.
    const int n         = static_cast<int>(hyp_ids.size());
    const int total_len = std::max(2 * n + 1, 8);
    out.assign(total_len, eos_id);
    for (int i = 0; i < n; ++i) {
        out[2 * i + 1] = hyp_ids[i];
    }
}

void argmax_collapse_drop_eos(const std::vector<float> & text_logits,
                              int                        vocab,
                              int                        n_text,
                              int32_t                    eos_id,
                              std::vector<int32_t> &     out_ids) {
    out_ids.clear();
    int32_t prev = -1;
    for (int i = 0; i < n_text; ++i) {
        const float * row    = text_logits.data() + static_cast<size_t>(i) * vocab;
        int32_t       best   = 0;
        float         best_v = row[0];
        for (int v = 1; v < vocab; ++v) {
            if (row[v] > best_v) {
                best_v = row[v];
                best   = v;
            }
        }
        if (best == eos_id) {
            prev = best;
            continue;
        }
        if (best == prev) {
            continue;
        }
        out_ids.push_back(best);
        prev = best;
    }
}

}  // namespace transcribe::granite_nar
