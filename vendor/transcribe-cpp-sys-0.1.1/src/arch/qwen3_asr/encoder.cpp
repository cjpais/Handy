// arch/qwen3_asr/encoder.cpp - audio encoder graph + host-side helpers.
//
// Reference: Qwen3ASRAudioEncoder in modeling_qwen3_asr.py (qwen_asr
// package, v0.0.6).

#include "encoder.h"

#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <vector>

namespace transcribe::qwen3_asr {

// Timing / cu_seqlens

int32_t aftercnn_len(int32_t mel_len) {
    // Three rounds of Conv2d(stride=2, pad=1, kernel=3):
    //   out = floor((L + 2*1 - 3) / 2) + 1 = floor((L-1)/2) + 1
    // which equals (L + 1) / 2 for L >= 1 under C integer division.
    auto step = [](int32_t L) {
        return (L + 1) / 2;
    };
    return step(step(step(mel_len)));
}

EncoderTiming compute_encoder_timing(int32_t n_mel_frames, const QwenAsrHParams & hp) {
    EncoderTiming t{};
    t.n_mel_frames  = n_mel_frames;
    t.mel_per_chunk = hp.enc_n_window * 2;
    if (n_mel_frames <= 0 || t.mel_per_chunk <= 0) {
        return t;
    }
    t.n_chunks   = (n_mel_frames + t.mel_per_chunk - 1) / t.mel_per_chunk;
    int32_t last = n_mel_frames - (t.n_chunks - 1) * t.mel_per_chunk;
    if (last == 0) {
        last = t.mel_per_chunk;
    }
    t.last_chunk_real_mel = last;
    t.per_chunk_aftercnn  = aftercnn_len(t.mel_per_chunk);
    t.last_chunk_aftercnn = aftercnn_len(last);
    t.T_enc_padded        = t.n_chunks * t.per_chunk_aftercnn;
    t.T_enc               = (t.n_chunks - 1) * t.per_chunk_aftercnn + t.last_chunk_aftercnn;
    t.aftercnn_lens_total = aftercnn_len(n_mel_frames);
    return t;
}

std::vector<float> build_sinusoid_pe(int32_t d_model, int32_t length, double max_timescale) {
    std::vector<float> pe(static_cast<size_t>(d_model) * length, 0.0f);
    if (d_model <= 0 || length <= 0 || (d_model % 2) != 0) {
        return pe;
    }

    const int32_t half   = d_model / 2;
    const double  log_ts = std::log(max_timescale) / (half - 1);

    for (int32_t p = 0; p < length; ++p) {
        for (int32_t k = 0; k < half; ++k) {
            const double inv_ts                             = std::exp(-log_ts * k);
            const double scaled                             = static_cast<double>(p) * inv_ts;
            pe[static_cast<size_t>(p) * d_model + k]        = static_cast<float>(std::sin(scaled));
            pe[static_cast<size_t>(p) * d_model + k + half] = static_cast<float>(std::cos(scaled));
        }
    }
    return pe;
}

std::vector<float> build_cu_seqlens_mask(const EncoderTiming & t, const QwenAsrHParams & hp) {
    (void) hp;
    const int32_t T = t.T_enc;
    // Full attention over the valid aftercnn rows. The measured
    // upstream reference (qwen_asr 0.0.6 + transformers eager / sdpa)
    // ignores cu_seqlens and runs unmasked bidirectional attention over
    // the post-pad-select tensor. vLLM's flash-attn-2 path honors
    // cu_seqlens and chunks at window_aftercnn, but its LibriSpeech WER
    // is worse than the eager path in our measurements; we follow the
    // eager reference so that transcribe.cpp's greedy decode matches
    // the per-tensor dumps byte-for-byte after the pad-row trim in
    // build_enc_graph.
    //
    // Zero-filled mask = softmax(scale * QK) with no bias, which is
    // identical to soft_max_ext(nullptr) and matches eager semantics.
    return std::vector<float>(static_cast<size_t>(T) * T, 0.0f);
}

// Graph construction

namespace {

constexpr float kLayerNormEps = 1e-5f;

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

ggml_tensor * add_conv_bias(ggml_context * ctx, ggml_tensor * x, ggml_tensor * bias) {
    // bias is [channels]; reshape to [1, 1, channels, 1] to broadcast
    // across the spatial and batch axes of conv output [W, H, C, N].
    ggml_tensor * b4 = ggml_reshape_4d(ctx, bias, 1, 1, bias->ne[0], 1);
    return ggml_add(ctx, x, b4);
}

ggml_tensor * layer_norm(ggml_context * ctx, ggml_tensor * x, ggml_tensor * gamma, ggml_tensor * beta) {
    ggml_tensor * y = ggml_norm(ctx, x, kLayerNormEps);
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

// One encoder block: pre-LN self-attention (bidirectional, full-
// sequence mask supplied by the caller) + pre-LN GELU FFN. Residuals
// are full 1.0.
ggml_tensor * build_enc_block(ggml_context *          ctx,
                              ggml_tensor *           x,
                              ggml_tensor *           mask,
                              const QwenAsrEncBlock & w,
                              int                     d_model,
                              int                     n_heads,
                              bool                    use_flash) {
    const int     head_dim = d_model / n_heads;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T        = x->ne[1];
    // Offline batch: utterances ride ne[2]. B == 1 is the single-shot path
    // and every reshape below collapses to the pre-batch topology exactly.
    const int64_t B        = x->ne[2];

    // ----- Self-attention sub-layer -----
    ggml_tensor * x_norm = layer_norm(ctx, x, w.norm_attn_w, w.norm_attn_b);

    ggml_tensor * q = linear(ctx, x_norm, w.attn_q_w, w.attn_q_b);
    ggml_tensor * k = linear(ctx, x_norm, w.attn_k_w, w.attn_k_b);
    ggml_tensor * v = linear(ctx, x_norm, w.attn_v_w, w.attn_v_b);

    // Split heads: [d_model, T, B] -> [head_dim, n_heads, T, B].
    q = ggml_reshape_4d(ctx, q, head_dim, n_heads, T, B);
    k = ggml_reshape_4d(ctx, k, head_dim, n_heads, T, B);
    v = ggml_reshape_4d(ctx, v, head_dim, n_heads, T, B);

    // Permute to [head_dim, T, n_heads, B] for attention (heads ne[2],
    // batch ne[3] — matches flash_attn_ext / batched soft_max_ext).
    q = ggml_cont(ctx, ggml_permute(ctx, q, 0, 2, 1, 3));
    k = ggml_cont(ctx, ggml_permute(ctx, k, 0, 2, 1, 3));
    v = ggml_cont(ctx, ggml_permute(ctx, v, 0, 2, 1, 3));

    ggml_tensor * o;
    if (use_flash) {
        o = ggml_flash_attn_ext(ctx, q, k, v, mask, scale, /*max_bias=*/0.0f,
                                /*logit_softcap=*/0.0f);
        o = (B == 1) ? ggml_reshape_2d(ctx, o, d_model, T) : ggml_reshape_3d(ctx, o, d_model, T, B);
    } else {
        ggml_tensor * kq      = ggml_mul_mat(ctx, k, q);
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, mask, scale, /*max_bias=*/0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
        o                     = ggml_cont(ctx, ggml_permute(ctx, o, 0, 2, 1, 3));
        o                     = (B == 1) ? ggml_reshape_2d(ctx, o, d_model, T) : ggml_reshape_3d(ctx, o, d_model, T, B);
    }

    o = linear(ctx, o, w.attn_out_w, w.attn_out_b);

    x = ggml_add(ctx, x, o);

    // ----- FFN sub-layer -----
    ggml_tensor * y = layer_norm(ctx, x, w.norm_ffn_w, w.norm_ffn_b);
    y               = linear(ctx, y, w.fc1_w, w.fc1_b);
    // PyTorch F.gelu default is the exact erf GELU.
    y               = ggml_gelu_erf(ctx, y);
    y               = linear(ctx, y, w.fc2_w, w.fc2_b);

    return ggml_add(ctx, x, y);
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *         ctx,
                                 const QwenAsrWeights & weights,
                                 const QwenAsrHParams & hp,
                                 const EncoderTiming &  timing,
                                 bool                   use_flash) {
    EncoderBuild eb{};
    eb.timing = timing;

    if (ctx == nullptr || timing.n_chunks <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr encoder: invalid arg "
                "(ctx=%p, n_chunks=%d)",
                static_cast<void *>(ctx), timing.n_chunks);
        return eb;
    }

    // Ragged-tail note: when the last chunk has fewer real mel frames
    // than mel_per_chunk, the batched conv still emits per_chunk_aftercnn
    // frames for it (the tail is zero-padded in the mel-input packer).
    // The reference ragged-selects only the real aftercnn frames via
    //   `hidden_states = padded_embed[padded_mask_after_cnn]`
    // before the 18 encoder blocks. We mirror that: reshape the per-
    // chunk conv output to [d_model, T_enc_padded], then VIEW the first
    // T_enc rows — dropping the trailing padded rows of the last chunk
    // — so the encoder blocks, the attention mask, and every downstream
    // shape see T_enc (not T_enc_padded). This matches the reference
    // byte-for-byte at the pos_add.out boundary and upward.

    const int64_t d_model       = hp.enc_d_model;
    const int64_t n_heads       = hp.enc_n_heads;
    const int64_t ds_h          = hp.enc_downsample_hidden;
    const int64_t n_mels        = hp.enc_num_mel_bins;
    const int64_t mel_per_chunk = timing.mel_per_chunk;
    const int64_t n_chunks      = timing.n_chunks;
    const int64_t T_per_chunk   = timing.per_chunk_aftercnn;
    const int64_t T_enc_padded  = timing.T_enc_padded;
    const int64_t T_enc         = timing.T_enc;

    // ----- Graph inputs -----
    // mel layout: ggml fast-to-slow ne=[mel_per_chunk, n_mels, 1, n_chunks]
    // ≡ [W=time, H=n_mels, C=1, N=n_chunks]. ggml_conv_2d with a
    // symmetric 3×3 kernel and symmetric stride/pad is invariant under
    // a W/H swap, so this is equivalent to Cohere's time-along-H
    // convention.
    eb.mel_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, mel_per_chunk, n_mels, 1, n_chunks);
    named(eb.mel_in, "enc.mel_in");
    ggml_set_input(eb.mel_in);
    eb.dumps.mel_in = eb.mel_in;

    eb.pos_emb_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_per_chunk);
    named(eb.pos_emb_in, "enc.pos_emb.in");
    ggml_set_input(eb.pos_emb_in);

    eb.mask_in = ggml_new_tensor_2d(ctx, use_flash ? GGML_TYPE_F16 : GGML_TYPE_F32, T_enc, T_enc);
    named(eb.mask_in, "enc.attn_mask.in");
    ggml_set_input(eb.mask_in);

    // ----- Subsample: 3x Conv2d + GELU + conv_out linear -----
    // Reference layout: PyTorch Conv2d on [B, 1, H=n_mels, W=mel_per_chunk].
    // For ggml_conv_2d we use [W=mel_per_chunk, H=n_mels, C=1, N=B] —
    // ggml swaps (W, H) relative to PyTorch, but the 3x3 kernel is
    // symmetric and stride/pad are (2,2)/(1,1), so the arithmetic is
    // invariant. Kernels are stored as ne=[KW=3, KH=3, IC, OC].
    ggml_tensor * x = eb.mel_in;

    x = ggml_conv_2d(ctx, weights.enc_subsample.conv0_w, x,
                     /*s0=*/2, /*s1=*/2, /*p0=*/1, /*p1=*/1, /*d0=*/1, /*d1=*/1);
    x = add_conv_bias(ctx, x, weights.enc_subsample.conv0_b);
    x = ggml_gelu_erf(ctx, x);

    x = ggml_conv_2d(ctx, weights.enc_subsample.conv1_w, x, 2, 2, 1, 1, 1, 1);
    x = add_conv_bias(ctx, x, weights.enc_subsample.conv1_b);
    x = ggml_gelu_erf(ctx, x);

    x = ggml_conv_2d(ctx, weights.enc_subsample.conv2_w, x, 2, 2, 1, 1, 1, 1);
    x = add_conv_bias(ctx, x, weights.enc_subsample.conv2_b);
    x = ggml_gelu_erf(ctx, x);
    // Now ne = [W=mel_ds, H=n_mels_ds, C=ds_h, N=n_chunks].

    const int64_t W_ds = x->ne[0];
    const int64_t H_ds = x->ne[1];
    if (W_ds != T_per_chunk) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr encoder: post-conv W=%lld does not match "
                "per_chunk_aftercnn=%lld",
                static_cast<long long>(W_ds), static_cast<long long>(T_per_chunk));
        return eb;
    }

    // Reshape for the linear projection. Reference does:
    //   permute(0, 3, 1, 2).contiguous().view(b, t, c*f)
    // i.e. [B, C, F_ds, T_ds] -> [B, T_ds, C, F_ds] -> [B, T_ds, C*F_ds].
    // The flat inner axis is (c * F_ds + f).
    //
    // In ggml ne layout we have [W=T_ds, H=F_ds, C=ds_h, N=B]. To get a
    // flat axis of size ds_h * F_ds where c is the slower sub-axis and
    // f the faster — matching the reference — we want axes in order
    // [f, c, T, B]: ne = [F_ds, ds_h, T_ds, B]. That's a permute(1, 2, 0, 3).
    // ggml_permute uses INVERSE semantics vs PyTorch: the i-th argument
    // says which NEW axis old axis i goes to (new[a_i] = old[i]). To
    // mirror PyTorch's `permute(0,3,1,2)` on [B, C, F, T] (equivalently,
    // map [W=13, H=16, C=480, N=11] → [F=16, C=480, T=13, B=11]), we
    // need new[0]=old[1], new[1]=old[2], new[2]=old[0], new[3]=old[3],
    // which is ggml_permute args (2, 0, 1, 3).
    x = ggml_permute(ctx, x, /*a0=*/2, /*a1=*/0, /*a2=*/1, /*a3=*/3);
    x = ggml_cont(ctx, x);
    x = ggml_reshape_3d(ctx, x, H_ds * ds_h, T_per_chunk, n_chunks);

    // Linear conv_out: [ds_h*F_ds, d_model] weight; maps to d_model.
    x = ggml_mul_mat(ctx, weights.enc_subsample.conv_out, x);
    // ne = [d_model, T_per_chunk, n_chunks]
    named(x, "enc.subsample.out");
    eb.dumps.subsample_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // Add sinusoidal PE (broadcast across n_chunks). pos_emb_in has
    // ne=[d_model, T_per_chunk]; ggml_add broadcasts missing axes.
    x = ggml_add(ctx, x, eb.pos_emb_in);

    // Flatten batch: [d_model, T_per_chunk, n_chunks] -> [d_model, T_enc_padded].
    // Row-major semantics: position r in the flattened sequence is
    // chunk = r / T_per_chunk, pos_in_chunk = r % T_per_chunk, which
    // matches pad_sequence(..., batch_first=True)[mask].reshape() when
    // every chunk has exactly T_per_chunk real frames.
    //
    // ggml_reshape_2d requires the input to be contiguous. The ggml_add
    // result is a freshly-allocated tensor so it IS contiguous in the
    // usual sense, BUT at graph-build time ggml's contiguity check
    // cares about strides, not allocation — pending ggml_cont ensures
    // strides match ne without relying on scheduler behaviour.
    x = ggml_cont(ctx, x);
    // Mark the contiguous (pre-reshape) tensor as an output before
    // reshape. The reshape is a view on this buffer; without pinning
    // the source, the scheduler is free to recycle its memory after
    // the downstream blocks consume it, and later dumps read stale
    // data.
    transcribe::debug::mark_tensor_for_dump(x);
    x = ggml_reshape_2d(ctx, x, d_model, T_enc_padded);

    // Drop padded aftercnn rows before encoder blocks. Matches
    // reference's `padded_embed[padded_mask_after_cnn]` step — without
    // this, the 18 blocks see the full T_enc_padded and the attention
    // pattern diverges from ref on any utterance whose mel length is
    // not a multiple of `n_window*2` (nearly all LibriSpeech).
    if (T_enc < T_enc_padded) {
        x = ggml_view_2d(ctx, x, d_model, T_enc, x->nb[1], 0);
        x = ggml_cont(ctx, x);
    }

    named(x, "enc.pos_add.out");
    eb.dumps.pos_add_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ----- 18 encoder blocks -----
    const int n_layers = static_cast<int>(weights.enc_blocks.size());
    for (int i = 0; i < n_layers; ++i) {
        x = build_enc_block(ctx, x, eb.mask_in, weights.enc_blocks[i], static_cast<int>(d_model),
                            static_cast<int>(n_heads), use_flash);
        if (i == 0) {
            named(x, "enc.block.0.out");
            eb.dumps.block_0_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        } else if (i == n_layers - 1) {
            char bname[64];
            std::snprintf(bname, sizeof(bname), "enc.block.%d.out", i);
            named(x, bname);
            eb.dumps.block_last_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
    }

    // ----- Post head: LN -> proj1 -> GELU -> proj2 -----
    x = layer_norm(ctx, x, weights.enc_head.ln_post_w, weights.enc_head.ln_post_b);
    named(x, "enc.ln_post.out");
    eb.dumps.ln_post_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    x = linear(ctx, x, weights.enc_head.proj1_w, weights.enc_head.proj1_b);
    x = ggml_gelu_erf(ctx, x);
    x = linear(ctx, x, weights.enc_head.proj2_w, weights.enc_head.proj2_b);
    named(x, "enc.proj.out");
    eb.dumps.proj_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    eb.out = x;
    ggml_set_output(eb.out);

    // 18 blocks * ~20 ops + subsample + head => ~450 graph nodes. 8192
    // leaves headroom for debug-name nodes.
    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    // Force evaluation of non-output debug tensors so they survive the
    // scheduler's live-range packing and are readable after compute.
    if (eb.dumps.subsample_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.subsample_out);
    }
    if (eb.dumps.pos_add_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.pos_add_out);
    }
    if (eb.dumps.block_0_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.block_0_out);
    }
    if (eb.dumps.block_last_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.block_last_out);
    }
    if (eb.dumps.ln_post_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.ln_post_out);
    }
    if (eb.dumps.proj_out) {
        ggml_build_forward_expand(eb.graph, eb.dumps.proj_out);
    }

    return eb;
}

EncoderBuildBatched build_encoder_graph_batched(ggml_context *         ctx,
                                                const QwenAsrWeights & weights,
                                                const QwenAsrHParams & hp,
                                                int                    n_chunks_max,
                                                int                    n_batch,
                                                bool                   use_flash) {
    EncoderBuildBatched eb{};
    if (ctx == nullptr || n_chunks_max <= 0 || n_batch <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr encoder(batched): invalid arg "
                "(n_chunks_max=%d, n_batch=%d)",
                n_chunks_max, n_batch);
        return eb;
    }

    const int64_t d_model       = hp.enc_d_model;
    const int64_t n_heads       = hp.enc_n_heads;
    const int64_t ds_h          = hp.enc_downsample_hidden;
    const int64_t n_mels        = hp.enc_num_mel_bins;
    const int64_t mel_per_chunk = hp.enc_n_window * 2;
    const int64_t T_per_chunk   = aftercnn_len(static_cast<int32_t>(mel_per_chunk));
    const int64_t B             = n_batch;
    const int64_t N             = B * n_chunks_max;  // packed conv batch
    const int64_t T_pad_max     = n_chunks_max * T_per_chunk;

    eb.n_batch      = n_batch;
    eb.n_chunks_max = n_chunks_max;
    eb.T_per_chunk  = static_cast<int>(T_per_chunk);
    eb.T_pad_max    = static_cast<int>(T_pad_max);

    // ----- Graph inputs -----
    eb.mel_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, mel_per_chunk, n_mels, 1, N);
    named(eb.mel_in, "enc.mel_in");
    ggml_set_input(eb.mel_in);

    eb.pos_emb_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_model, T_per_chunk);
    named(eb.pos_emb_in, "enc.pos_emb.in");
    ggml_set_input(eb.pos_emb_in);

    // Key-pad mask: per-utterance [T_pad_max, T_pad_max, 1, B]. Built host-side.
    eb.mask_in = ggml_new_tensor_4d(ctx, use_flash ? GGML_TYPE_F16 : GGML_TYPE_F32, T_pad_max, T_pad_max, 1, B);
    named(eb.mask_in, "enc.attn_mask.in");
    ggml_set_input(eb.mask_in);

    // ----- Subsample: 3x Conv2d + GELU (per-chunk over N = B*n_chunks_max) -----
    ggml_tensor * x = eb.mel_in;
    x               = ggml_conv_2d(ctx, weights.enc_subsample.conv0_w, x, 2, 2, 1, 1, 1, 1);
    x               = add_conv_bias(ctx, x, weights.enc_subsample.conv0_b);
    x               = ggml_gelu_erf(ctx, x);
    x               = ggml_conv_2d(ctx, weights.enc_subsample.conv1_w, x, 2, 2, 1, 1, 1, 1);
    x               = add_conv_bias(ctx, x, weights.enc_subsample.conv1_b);
    x               = ggml_gelu_erf(ctx, x);
    x               = ggml_conv_2d(ctx, weights.enc_subsample.conv2_w, x, 2, 2, 1, 1, 1, 1);
    x               = add_conv_bias(ctx, x, weights.enc_subsample.conv2_b);
    x               = ggml_gelu_erf(ctx, x);
    // ne = [W=T_per_chunk, H=n_mels_ds, C=ds_h, N].

    const int64_t W_ds = x->ne[0];
    const int64_t H_ds = x->ne[1];
    if (W_ds != T_per_chunk) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr encoder(batched): post-conv W=%lld != "
                "per_chunk_aftercnn=%lld",
                static_cast<long long>(W_ds), static_cast<long long>(T_per_chunk));
        return eb;
    }

    // [W=T_ds, H=F_ds, C=ds_h, N] -> [F_ds, ds_h, T_ds, N] (see single-shot).
    x = ggml_permute(ctx, x, 2, 0, 1, 3);
    x = ggml_cont(ctx, x);
    x = ggml_reshape_3d(ctx, x, H_ds * ds_h, T_per_chunk, N);
    x = ggml_mul_mat(ctx, weights.enc_subsample.conv_out, x);  // [d_model, T_per_chunk, N]

    // Add sinusoidal PE (broadcast across the N = B*n_chunks_max axis).
    x = ggml_add(ctx, x, eb.pos_emb_in);

    // Reorganise N=(b*n_chunks_max + c) -> [d_model, T_per_chunk, n_chunks_max, B],
    // then flatten (T_per_chunk, n_chunks_max) chunk-major into the sequence:
    // row r = chunk*T_per_chunk + pos. Result [d_model, T_pad_max, B].
    x = ggml_cont(ctx, x);
    x = ggml_reshape_4d(ctx, x, d_model, T_per_chunk, n_chunks_max, B);
    x = ggml_reshape_3d(ctx, x, d_model, T_pad_max, B);

    // ----- 18 encoder blocks (batch on ne[2], per-utterance key-pad mask) -----
    const int n_layers = static_cast<int>(weights.enc_blocks.size());
    for (int i = 0; i < n_layers; ++i) {
        x = build_enc_block(ctx, x, eb.mask_in, weights.enc_blocks[i], static_cast<int>(d_model),
                            static_cast<int>(n_heads), use_flash);
    }

    // ----- Post head: LN -> proj1 -> GELU -> proj2 -----
    x = layer_norm(ctx, x, weights.enc_head.ln_post_w, weights.enc_head.ln_post_b);
    x = linear(ctx, x, weights.enc_head.proj1_w, weights.enc_head.proj1_b);
    x = ggml_gelu_erf(ctx, x);
    x = linear(ctx, x, weights.enc_head.proj2_w, weights.enc_head.proj2_b);
    named(x, "enc.proj.out");

    eb.out = x;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr encoder(batched): ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    return eb;
}

}  // namespace transcribe::qwen3_asr
