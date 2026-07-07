// arch/voxtral/encoder.cpp - Voxtral audio encoder + projector graph.
//
// Reference: VoxtralEncoder (= Whisper-large-v3 encoder) +
// VoxtralMultiModalProjector.
//
//   mel [n_mels=128, T=3000]
//     -> transpose to [T, n_mels] for ggml_conv_1d
//     -> conv1 (k=3, s=1, p=1) + GELU                  [T=3000, d_model]
//     -> conv2 (k=3, s=2, p=1) + GELU                  [T=1500, d_model]
//     -> transpose to [d_model, T=1500]
//     -> + fixed sinusoidal embed_positions [d_model, 1500]
//     -> 32x pre-LN block (LN; q/v/out bias, k no bias; GELU FFN)
//     -> final LayerNorm                                -> enc.out
//   projector:
//     reshape [d_model,1500] -> [5120, 375] (group 4 frames, C-order)
//     -> Linear(5120->H) -> GELU -> Linear(H->H)        -> proj.out
//
// Whisper attention quirk: q/v/out carry bias, k does NOT. q is
// pre-scaled by head_dim**-0.5 in the reference; we fold that scale into
// flash_attn/soft_max (mathematically identical, one application).

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>

namespace transcribe::voxtral {

namespace {

namespace conf = transcribe::conformer;
using conf::layer_norm;
using conf::named;

// Reshape a 1D conv bias [Cout] into [1, Cout, 1, 1] so it broadcasts
// across T when added to a ggml_conv_1d output [T_out, Cout, 1, 1].
ggml_tensor * add_conv1d_bias(ggml_context * ctx, ggml_tensor * conv_out, ggml_tensor * bias_1d) {
    if (bias_1d == nullptr) {
        return conv_out;
    }
    const int64_t channels = bias_1d->ne[0];
    ggml_tensor * bias_4d  = ggml_reshape_4d(ctx, bias_1d, 1, channels, 1, 1);
    return ggml_add(ctx, conv_out, bias_4d);
}

// Whisper-style MHSA. x:[d_model, T]; q/v/out bias, k no bias.
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

    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        q = ggml_add(ctx, q, q_b);
    }
    ggml_tensor * k = ggml_mul_mat(ctx, k_w, x);  // no bias
    ggml_tensor * v = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        v = ggml_add(ctx, v, v_b);
    }

    q = ggml_permute(ctx, ggml_reshape_3d(ctx, q, head_dim, n_heads, T), 0, 2, 1, 3);
    k = ggml_permute(ctx, ggml_reshape_3d(ctx, k, head_dim, n_heads, T), 0, 2, 1, 3);
    v = ggml_permute(ctx, ggml_reshape_3d(ctx, v, head_dim, n_heads, T), 0, 2, 1, 3);

    ggml_tensor * o;
    if (use_flash) {
        ggml_tensor * q_c = ggml_cont(ctx, q);
        ggml_tensor * k_c = ggml_cont(ctx, k);
        ggml_tensor * v_c = ggml_cont(ctx, v);
        o                 = ggml_flash_attn_ext(ctx, q_c, k_c, v_c, nullptr, scale, 0.0f, 0.0f);
        o                 = ggml_reshape_2d(ctx, o, d_model, T);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, ggml_cont(ctx, k), ggml_cont(ctx, q));
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
        o                     = ggml_reshape_2d(ctx, o, d_model, T);
    }

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

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

ggml_tensor * build_block(ggml_context *          ctx,
                          ggml_tensor *           x,
                          const VoxtralEncBlock & b,
                          int                     n_heads,
                          int                     d_model,
                          bool                    use_flash) {
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
        y = mha_encoder(ctx, y, b.attn_q_w, b.attn_q_b, b.attn_k_w, b.attn_v_w, b.attn_v_b, b.attn_out_w, b.attn_out_b,
                        n_heads, d_model, use_flash);
        x = ggml_add(ctx, x, y);
    }
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
        y               = ffn(ctx, y, b.fc1_w, b.fc1_b, b.fc2_w, b.fc2_b);
        x               = ggml_add(ctx, x, y);
    }
    return x;
}

// Batched (n_chunks on ne[2]) Whisper-style MHSA. x:[d_model, T, B];
// q/v/out bias, k no bias. Flash-only (the batch fast path requires it).
ggml_tensor * mha_encoder_batched(ggml_context * ctx,
                                  ggml_tensor *  x,
                                  ggml_tensor *  q_w,
                                  ggml_tensor *  q_b,
                                  ggml_tensor *  k_w,
                                  ggml_tensor *  v_w,
                                  ggml_tensor *  v_b,
                                  ggml_tensor *  out_w,
                                  ggml_tensor *  out_b,
                                  int            n_heads,
                                  int            d_model) {
    const int     head_dim = d_model / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T        = x->ne[1];
    const int64_t B        = x->ne[2];

    ggml_tensor * q = ggml_mul_mat(ctx, q_w, x);
    if (q_b != nullptr) {
        q = ggml_add(ctx, q, q_b);
    }
    ggml_tensor * k = ggml_mul_mat(ctx, k_w, x);  // no bias
    ggml_tensor * v = ggml_mul_mat(ctx, v_w, x);
    if (v_b != nullptr) {
        v = ggml_add(ctx, v, v_b);
    }

    // [d_model, T, B] -> [head_dim, n_heads, T, B] -> [head_dim, T, n_heads, B]
    q = ggml_permute(ctx, ggml_reshape_4d(ctx, q, head_dim, n_heads, T, B), 0, 2, 1, 3);
    k = ggml_permute(ctx, ggml_reshape_4d(ctx, k, head_dim, n_heads, T, B), 0, 2, 1, 3);
    v = ggml_permute(ctx, ggml_reshape_4d(ctx, v, head_dim, n_heads, T, B), 0, 2, 1, 3);

    ggml_tensor * q_c = ggml_cont(ctx, q);
    ggml_tensor * k_c = ggml_cont(ctx, k);
    ggml_tensor * v_c = ggml_cont(ctx, v);
    // flash batches on ne[3]; output [head_dim, n_heads, T, B] -> [d_model, T, B].
    ggml_tensor * o   = ggml_flash_attn_ext(ctx, q_c, k_c, v_c, nullptr, scale, 0.0f, 0.0f);
    o                 = ggml_reshape_3d(ctx, o, d_model, T, B);

    o = ggml_mul_mat(ctx, out_w, o);
    if (out_b != nullptr) {
        o = ggml_add(ctx, o, out_b);
    }
    return o;
}

ggml_tensor * ffn_batched(ggml_context * ctx,
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

ggml_tensor * build_block_batched(ggml_context *          ctx,
                                  ggml_tensor *           x,
                                  const VoxtralEncBlock & b,
                                  int                     n_heads,
                                  int                     d_model) {
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
        y = mha_encoder_batched(ctx, y, b.attn_q_w, b.attn_q_b, b.attn_k_w, b.attn_v_w, b.attn_v_b, b.attn_out_w,
                                b.attn_out_b, n_heads, d_model);
        x = ggml_add(ctx, x, y);
    }
    {
        ggml_tensor * y = layer_norm(ctx, x, b.norm_ffn_w, b.norm_ffn_b);
        y               = ffn_batched(ctx, y, b.fc1_w, b.fc1_b, b.fc2_w, b.fc2_b);
        x               = ggml_add(ctx, x, y);
    }
    return x;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *         ctx,
                                 const VoxtralWeights & w,
                                 const VoxtralHParams & hp,
                                 int                    n_mel_frames,
                                 bool                   use_flash) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0 || n_mel_frames % 2 != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral encoder: invalid n_mel_frames=%d (must be positive even)",
                n_mel_frames);
        return eb;
    }

    const int d_model = hp.enc_d_model;
    const int n_mels  = hp.enc_num_mel_bins;
    const int n_heads = hp.enc_n_heads;
    const int T_enc   = n_mel_frames / 2;

    if (T_enc != hp.enc_max_source_positions) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "voxtral encoder: T_enc=%d != max_source_positions=%d "
                "(mel must be padded to %d frames)",
                T_enc, hp.enc_max_source_positions, 2 * hp.enc_max_source_positions);
        return eb;
    }

    // ---- mel input ----
    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames);
    if (eb.mel_in == nullptr) {
        return eb;
    }
    named(eb.mel_in, "enc.mel.in");
    ggml_set_input(eb.mel_in);

    // ---- conv stem ----
    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));  // [T, n_mels]
    x               = conf::conv_1d_f32(ctx, w.enc_stem.conv0_w, x, /*s=*/1, /*p=*/1, /*d=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x               = ggml_gelu_erf(ctx, x);  // [T, d_model] — already the layout conv2 wants
    x               = conf::conv_1d_f32(ctx, w.enc_stem.conv1_w, x, /*s=*/2, /*p=*/1, /*d=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x               = ggml_gelu_erf(ctx, x);
    x               = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, T_enc]

    // ---- + fixed sinusoidal positional embedding ----
    x = ggml_add(ctx, x, w.enc_top.pos_emb_w);

    // ---- transformer blocks ----
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    eb.dumps.block_outs.reserve(static_cast<size_t>(n_blocks));
    for (int i = 0; i < n_blocks; ++i) {
        x = build_block(ctx, x, w.enc_blocks[i], n_heads, d_model, use_flash);
        char bname[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
        named(x, bname);
        transcribe::debug::mark_tensor_for_dump(x);
        eb.dumps.block_outs.push_back(x);
    }

    // ---- final LayerNorm -> enc.out ----
    x = layer_norm(ctx, x, w.enc_top.ln_post_w, w.enc_top.ln_post_b);
    named(x, "enc.out");
    eb.dumps.enc_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---- projector ----
    // Group 4 consecutive encoder frames: [d_model, 1500] -> [5120, 375].
    // ggml contiguous memory order is (d innermost, t next), which is the
    // C-order [1500, 1280] of the reference last_hidden_state, so a plain
    // reshape yields token i = concat(frames 4i..4i+3).
    const int     n_audio = hp.audio_tokens_per_chunk();
    ggml_tensor * xc      = ggml_cont(ctx, x);
    ggml_tensor * grouped = ggml_reshape_2d(ctx, xc, hp.proj_in, n_audio);
    ggml_tensor * p       = ggml_mul_mat(ctx, w.proj.linear_1_w, grouped);  // [H, 375]
    p                     = ggml_gelu_erf(ctx, p);
    p                     = ggml_mul_mat(ctx, w.proj.linear_2_w, p);        // [H, 375]
    named(p, "proj.out");
    eb.dumps.proj_out = p;
    transcribe::debug::mark_tensor_for_dump(p);

    eb.out = p;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    // Keep dumped intermediates alive (they are side outputs).
    if (eb.dumps.enc_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.enc_out);
    }
    for (ggml_tensor * t : eb.dumps.block_outs) {
        ggml_build_forward_expand(eb.graph, t);
    }

    return eb;
}

EncoderBuild build_encoder_graph_batched(ggml_context *         ctx,
                                         const VoxtralWeights & w,
                                         const VoxtralHParams & hp,
                                         int                    n_mel_frames,
                                         int                    n_chunks,
                                         bool                   use_flash) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0 || n_mel_frames % 2 != 0 || n_chunks <= 0 || !use_flash) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "voxtral encoder(batched): invalid arg "
                "(n_mel_frames=%d, n_chunks=%d, use_flash=%d)",
                n_mel_frames, n_chunks, static_cast<int>(use_flash));
        return eb;
    }

    const int d_model = hp.enc_d_model;
    const int n_mels  = hp.enc_num_mel_bins;
    const int n_heads = hp.enc_n_heads;
    const int T_enc   = n_mel_frames / 2;
    const int B       = n_chunks;

    if (T_enc != hp.enc_max_source_positions) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral encoder(batched): T_enc=%d != max_source_positions=%d", T_enc,
                hp.enc_max_source_positions);
        return eb;
    }

    // ---- mel input [n_mels, n_mel_frames, n_chunks] ----
    eb.mel_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, n_mels, n_mel_frames, B);
    if (eb.mel_in == nullptr) {
        return eb;
    }
    named(eb.mel_in, "enc.mel.in.batched");
    ggml_set_input(eb.mel_in);

    // ---- conv stem (batch rides ne[2]; conv_1d_f32 N>1 path) ----
    ggml_tensor * x = ggml_cont(ctx, ggml_transpose(ctx, eb.mel_in));  // [T, n_mels, B]
    x               = conf::conv_1d_f32(ctx, w.enc_stem.conv0_w, x, /*s=*/1, /*p=*/1, /*d=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv0_b);
    x               = ggml_gelu_erf(ctx, x);  // [T, d_model, B]
    x               = conf::conv_1d_f32(ctx, w.enc_stem.conv1_w, x, /*s=*/2, /*p=*/1, /*d=*/1);
    x               = add_conv1d_bias(ctx, x, w.enc_stem.conv1_b);
    x               = ggml_gelu_erf(ctx, x);                   // [T_enc, d_model, B]
    x               = ggml_cont(ctx, ggml_transpose(ctx, x));  // [d_model, T_enc, B]

    // ---- + fixed sinusoidal positional embedding (broadcast over batch) ----
    x = ggml_add(ctx, x, w.enc_top.pos_emb_w);

    // ---- transformer blocks ----
    const int n_blocks = static_cast<int>(w.enc_blocks.size());
    for (int i = 0; i < n_blocks; ++i) {
        x = build_block_batched(ctx, x, w.enc_blocks[i], n_heads, d_model);
    }

    // ---- final LayerNorm ----
    x = layer_norm(ctx, x, w.enc_top.ln_post_w, w.enc_top.ln_post_b);  // [d_model, T_enc, B]

    // ---- projector: group 4 frames per chunk -> [proj_in, n_audio, B] ----
    const int     n_audio = hp.audio_tokens_per_chunk();
    ggml_tensor * xc      = ggml_cont(ctx, x);
    ggml_tensor * grouped = ggml_reshape_3d(ctx, xc, hp.proj_in, n_audio, B);
    ggml_tensor * p       = ggml_mul_mat(ctx, w.proj.linear_1_w, grouped);  // [H, n_audio, B]
    p                     = ggml_gelu_erf(ctx, p);
    p                     = ggml_mul_mat(ctx, w.proj.linear_2_w, p);        // [dec_h, n_audio, B]
    named(p, "proj.out.batched");

    eb.out = p;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral encoder(batched): ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    return eb;
}

}  // namespace transcribe::voxtral
