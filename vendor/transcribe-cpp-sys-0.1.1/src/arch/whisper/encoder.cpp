// arch/whisper/encoder.cpp - Whisper encoder graph builder.
//
// Shape is a straightforward pre-LN transformer stack on top of a
// 2-layer Conv1d stem:
//
//   mel [n_mels=80, T=3000]
//     -> transpose + cont to [T, n_mels] for ggml_conv_1d
//     -> conv1 (k=3, s=1, p=1) + GELU                 [T=3000, d_model]
//     -> conv2 (k=3, s=2, p=1) + GELU                 [T=1500, d_model]
//     -> transpose + cont to [d_model, T=1500]        (channel-innermost)
//     -> + learned positional embedding [d_model, T=1500]
//     -> Nx pre-LN transformer block
//     -> final LayerNorm
//     -> enc.final [d_model, T=1500]
//
// ggml ne (fast innermost): [n_mels, T] for the mel input, [d_model, T]
// for everything after the stem transpose.
//
// Whisper attention quirk: q/v/out carry bias; k has NO bias. The mha helper
// takes nullable bias slots and skips the add when null.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>

namespace transcribe::whisper {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

// Reshape a 1D conv bias [Cout] into a 4D tensor [1, Cout, 1, 1] so it
// broadcasts across T when added to a ggml_conv_1d output
// [T_out, Cout, 1, 1]. ggml's `add_conv_bias` in conformer.cpp is for
// 2D conv (channels at ne[2]) and does not fit here.
ggml_tensor * add_conv1d_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d) {
    if (bias_1d == nullptr) {
        return conv_out;
    }
    const int64_t channels = bias_1d->ne[0];
    ggml_tensor * bias_4d  = ggml_reshape_4d(ctx, bias_1d, 1, channels, 1, 1);
    return ggml_add(ctx, conv_out, bias_4d);
}

// Standard multi-head self-attention without relative position.
// x: [d_model, T] -> returns [d_model, T]. Encoder is bidirectional
// (no causal mask).
ggml_tensor * mha_encoder(ggml_context * ctx,
                          ggml_tensor *  x,
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
    const int64_t T        = x->ne[1];

    // Q, K, V projections (k has no bias).
    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        q = ggml_add(ctx, q, q_b);
    }

    ggml_tensor * k = ggml_mul_mat(ctx, k_w, x);

    ggml_tensor * v = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        v = ggml_add(ctx, v, v_b);
    }

    // Split into heads: [d_model, T] -> [head_dim, n_heads, T] ->
    // permute to [head_dim, T, n_heads] so flash_attn / manual path
    // both see the canonical layout.
    q = ggml_reshape_3d(ctx, q, head_dim, n_heads, T);
    q = ggml_permute(ctx, q, 0, 2, 1, 3);

    k = ggml_reshape_3d(ctx, k, head_dim, n_heads, T);
    k = ggml_permute(ctx, k, 0, 2, 1, 3);

    v = ggml_reshape_3d(ctx, v, head_dim, n_heads, T);
    v = ggml_permute(ctx, v, 0, 2, 1, 3);

    ggml_tensor * o;
    if (use_flash) {
        // flash_attn_ext wants q/k/v contiguous.
        ggml_tensor * q_c = ggml_cont(ctx, q);
        ggml_tensor * k_c = ggml_cont(ctx, k);
        ggml_tensor * v_c = ggml_cont(ctx, v);
        o                 = ggml_flash_attn_ext(ctx, q_c, k_c, v_c, nullptr, scale, 0.0f, 0.0f);
        o                 = ggml_reshape_2d(ctx, o, d_model, T);
    } else {
        // Manual attention path: mul_mat(K, Q) gives [T_k, T_q, n_heads]
        // per the standard cohere pattern.
        ggml_tensor * kq      = ggml_mul_mat(ctx, ggml_cont(ctx, k), ggml_cont(ctx, q));
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, nullptr, scale, 0.0f);

        // v^T so the final mul_mat yields [head_dim, T_q, n_heads].
        ggml_tensor * v_t = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                 = ggml_mul_mat(ctx, v_t, kq_soft);

        // Merge heads: [head_dim, T_q, n_heads] -> [head_dim, n_heads, T_q]
        // -> cont -> reshape to [d_model, T_q].
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
        o = ggml_reshape_2d(ctx, o, d_model, T);
    }

    // Output projection with bias.
    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

// FFN: fc2(GELU(fc1(x))); pre-LN wrapped outside. fc1/fc2 carry bias.
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
    h               = ggml_gelu_erf(ctx, h);
    ggml_tensor * o = ggml_mul_mat(ctx, fc2_w, h);
    if (fc2_b != nullptr) {
        o = ggml_add(ctx, o, fc2_b);
    }
    return o;
}

// Build one pre-LN encoder block:
//   y = x + MHSA(LN_attn(x))
//   y = y + FFN(LN_ffn(y))
ggml_tensor * build_block(ggml_context *          ctx,
                          ggml_tensor *           x,
                          const WhisperEncBlock & b,
                          int                     n_heads,
                          int                     d_model,
                          bool                    use_flash) {
    // Self-attention sublayer.
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
        y = mha_encoder(ctx, y, b.attn_q_w, b.attn_q_b, b.attn_k_w, b.attn_v_w, b.attn_v_b, b.attn_out_w, b.attn_out_b,
                        n_heads, d_model, use_flash);
        x = ggml_add(ctx, x, y);
    }
    // FFN sublayer.
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
        y               = ffn(ctx, y, b.ffn_fc1_w, b.ffn_fc1_b, b.ffn_fc2_w, b.ffn_fc2_b);
        x               = ggml_add(ctx, x, y);
    }
    return x;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *         ctx,
                                 const WhisperWeights & w,
                                 const WhisperHParams & hp,
                                 int                    n_mel_frames,
                                 bool                   use_flash,
                                 const char *           backend_name) {
    (void) backend_name;

    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper encoder: invalid arg (ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }
    if (n_mel_frames % 2 != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper encoder: n_mel_frames=%d must be even "
                "(stride-2 conv2 would produce fractional T_enc)",
                n_mel_frames);
        return eb;
    }

    const int d_model = hp.enc_d_model;
    const int n_mels  = hp.enc_num_mel_bins;
    const int n_heads = hp.enc_n_heads;
    const int T_enc   = n_mel_frames / 2;

    if (T_enc > hp.enc_max_source_positions) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper encoder: T_enc=%d exceeds max_source_positions=%d", T_enc,
                hp.enc_max_source_positions);
        return eb;
    }

    // mel input handle. ggml ne=[n_mels, T]; caller uploads the mel
    // spectrogram row-major in this layout.
    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames);
    if (eb.mel_in == nullptr) {
        return eb;
    }
    named(eb.mel_in, "enc.mel.in");
    ggml_set_input(eb.mel_in);
    eb.dumps.mel_in = eb.mel_in;
    transcribe::debug::mark_tensor_for_dump(eb.mel_in);

    // conv stem. ggml_conv_1d wants data ne=[T, Cin]; transpose mel from
    // [n_mels, T] to [T, n_mels], cont, feed to conv1.
    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));

    // conv1: k=3, stride=1, padding=1 -> [T, d_model]. Use conf::conv_1d_f32,
    // not ggml_conv_1d: whisper conv kernels are F32, but ggml_conv_1d's
    // im2col hard-codes F16 dst and the CPU kernel asserts src0==F16 (crash).
    x = conf::conv_1d_f32(ctx, w.enc_stem.conv0_w, x, 1, 1, 1);
    x = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    // HF Whisper uses exact-erf GELU for conv stems and FFN; ggml_gelu is tanh.
    x = ggml_gelu_erf(ctx, x);
    x = ggml_cont(ctx, ggml_transpose(ctx, x));  // -> [d_model, T]
    named(x, "enc.conv1.out");
    eb.dumps.conv1_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // conv2: k=3, stride=2, padding=1 -> [T_enc, d_model] (needs [T, Cin]).
    x = ggml_cont(ctx, ggml_transpose(ctx, x));
    x = conf::conv_1d_f32(ctx, w.enc_stem.conv1_w, x, 2, 1, 1);
    x = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x = ggml_gelu_erf(ctx, x);
    x = ggml_cont(ctx, ggml_transpose(ctx, x));  // -> [d_model, T_enc]
    named(x, "enc.conv2.out");
    eb.dumps.conv2_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // positional embedding + add. pos_emb ne=[d_model, max_source_positions];
    // for T_enc < max take a prefix view (no-op when T_enc == max).
    ggml_tensor * pos_emb = w.enc_top.pos_emb_w;
    if (T_enc != hp.enc_max_source_positions) {
        pos_emb = ggml_view_2d(ctx, w.enc_top.pos_emb_w, d_model, T_enc, w.enc_top.pos_emb_w->nb[1], 0);
    }
    named(pos_emb, "enc.pos_emb");
    eb.dumps.pos_emb = pos_emb;
    transcribe::debug::mark_tensor_for_dump(pos_emb);

    x = ggml_add(ctx, x, pos_emb);
    named(x, "enc.embed.out");
    eb.dumps.embed_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // transformer blocks
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    eb.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        x = build_block(ctx, x, w.enc_blocks[i], n_heads, d_model, use_flash);

        // Name every block for layer-by-layer comparison. block0 / block_last
        // are stashed separately for callers that only want the spot-check pair.
        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
        named(x, bname);
        transcribe::debug::mark_tensor_for_dump(x);
        eb.dumps.block_outs.push_back(x);
        if (i == 0) {
            eb.dumps.block0_out = x;
        }
        if (i == n_blocks - 1) {
            eb.dumps.block_last_out = x;
        }
    }

    // final layer norm
    x = layer_norm(ctx, x, w.enc_top.final_norm_w, w.enc_top.final_norm_b);
    named(x, "enc.final");
    eb.dumps.final_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    eb.out = x;
    ggml_set_output(eb.out);

    // Build the forward cgraph. 8192 leaves headroom up to large (32 blocks).
    eb.graph = ggml_new_graph_custom(ctx, 8192, false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    return eb;
}

}  // namespace transcribe::whisper
