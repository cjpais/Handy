// arch/gigaam/encoder.cpp - GigaAM Conformer encoder graph builder.
//
// Reuses transcribe::conformer helpers (macaron FF, conv module, LayerNorm)
// plus gigaam-specific pieces: a 2-conv1d pre_encode and rotary self-attention
// with PRE-projection rotation (rotation applied to x before Wq/Wk; Wv gets
// unrotated x). Activations stay [d_model, T_enc, B]; T_enc = floor((T_mel-1)/4).

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <vector>

namespace transcribe::gigaam {

namespace {

namespace conf = transcribe::conformer;

// Project a GigaamBlock onto the shared BlockView so we can reuse
// conf::conv_module. The attention-half fields stay nullptr because we
// run a custom rotary path; the conv module fields are the only ones
// `conf::conv_module` reads.
conf::BlockView to_conv_view(const GigaamBlock & b) {
    conf::BlockView v;
    v.conv_pw1_w = b.conv_pw1_w;
    v.conv_pw1_b = b.conv_pw1_b;
    v.conv_dw_w  = b.conv_dw_w;
    v.conv_dw_b  = b.conv_dw_b;
    v.conv_pw2_w = b.conv_pw2_w;
    v.conv_pw2_b = b.conv_pw2_b;
    v.conv_ln_w  = b.conv_ln_w;
    v.conv_ln_b  = b.conv_ln_b;
    return v;
}

// Per-stage masked-subsampling handles for variable-length offline batching.
// Each is a graph input ne=[T_stage, 1, B] (broadcast over the channel axis)
// holding 1.0 on real time frames and 0.0 on padded; the driver fills them
// from each utterance's downsampled length. Null disables the masked path
// (single-shot / same-length batches stay bit-identical to the pre-batch
// graph). See build_pre_encode for why padding must be zeroed after each relu.
struct PreEncodeMasks {
    ggml_tensor * mask_s1 = nullptr;  // after conv0 relu
    ggml_tensor * mask_s2 = nullptr;  // after conv2 relu
};

// Build the 2-conv1d pre-encode stack.
//   Input  mel_in:  ne=[T_mel, n_mels, B]  (B == 1 single-shot)
//   Output pre_enc: ne=[d_model, T_enc, B] with T_enc ≈ T_mel / 4
//
// masks (optional): variable-length batching zero-pads each utterance's mel to
// T_max along time. The convs carry bias + ReLU, so a padded zero becomes
// non-zero after the first conv and then leaks into each utterance's last VALID
// frame via the next stride-2 conv's receptive field. NeMo's masked-subsampling
// fix is to zero each conv intermediate's padded time region after every ReLU
// (mask_s1 after conv0, mask_s2 after conv2).
ggml_tensor * build_pre_encode(ggml_context *          ctx,
                               const GigaamPreEncode & pe,
                               ggml_tensor *           mel_in,
                               int                     subs_kernel_size,
                               const char *            name_prefix,
                               const PreEncodeMasks *  masks = nullptr) {
    const int padding = (subs_kernel_size - 1) / 2;  // sym pad, e.g. k=5 -> 2

    // Conv 0: in=feat_in, out=d_model. Output ne=[T_out, d_model, B].
    ggml_tensor * x = conf::conv_1d_f32(ctx, pe.conv0_w, mel_in,
                                        /*stride=*/2, padding, /*dilation=*/1);
    {
        // Bias on the OC axis (ne[1]); broadcasts over time + batch.
        ggml_tensor * bias_r = ggml_reshape_3d(ctx, pe.conv0_b, 1, x->ne[1], 1);
        x                    = ggml_add(ctx, x, bias_r);
    }
    x = ggml_relu(ctx, x);
    if (masks != nullptr && masks->mask_s1 != nullptr) {
        // mask ne=[T_out, 1, B] zeros padded time, broadcasting over OC.
        x = ggml_mul(ctx, x, masks->mask_s1);
    }

    // Conv 2: in=d_model, out=d_model. x already ne=[T_out, d_model, B].
    x = conf::conv_1d_f32(ctx, pe.conv2_w, x,
                          /*stride=*/2, padding, /*dilation=*/1);
    {
        ggml_tensor * bias_r = ggml_reshape_3d(ctx, pe.conv2_b, 1, x->ne[1], 1);
        x                    = ggml_add(ctx, x, bias_r);
    }
    x = ggml_relu(ctx, x);
    if (masks != nullptr && masks->mask_s2 != nullptr) {
        x = ggml_mul(ctx, x, masks->mask_s2);
    }

    // Transpose into the [d_model, T_enc, B] layout the conformer blocks
    // consume. The cont after permute is what does the actual byte transpose.
    x = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));
    (void) name_prefix;
    return x;
}

// Build the rotary attention sub-block.
//
// GigaAM applies rotary to the PRE-projection x: reshape x to
// [d_k, n_head, T, B], rotate the d_k axis with NEOX split-halves, then
// reshape back to [d_model, T, B] and apply Wq / Wk. Wv uses the
// UNROTATED x. Output projection at the end.
//
// Input  x:       [d_model, T, B]   (B == 1 single-shot)
// Output:        [d_model, T, B]
//
// attn_pad_mask (optional): variable-length key-padding mask
// ne=[T, 1, 1, B], 0 on real keys / -INF on padded. When non-null the manual
// SDPA path is forced (the mask is added onto the score matrix before
// softmax) so a real query never attends another utterance's padded tail.
ggml_tensor * build_rotary_attn(ggml_context *      ctx,
                                ggml_tensor *       x,
                                ggml_tensor *       positions,
                                const GigaamBlock & b,
                                int                 d_model,
                                int                 n_head,
                                bool                use_flash,
                                ggml_tensor *       attn_pad_mask = nullptr) {
    const int     head_dim = d_model / n_head;
    const float   scale    = 1.0f / std::sqrt(static_cast<float>(head_dim));
    const int64_t T        = x->ne[1];
    const int64_t Bb       = x->ne[2];

    // A per-utterance key-padding mask is content/length dependent, so route
    // it through the manual mul_mat + soft_max path (flash's additive mask is
    // not exercised here for a per-batch ne[3]). Same-length batches and
    // single-shot keep flash.
    const bool flash = use_flash && (attn_pad_mask == nullptr);

    // Apply rotary to x BEFORE Wq/Wk projection. The rotation operates
    // on ne[0] (which must be the head_dim axis), so reshape x to
    // [head_dim, n_head, T, B] first. NEOX mode = split-halves, matching
    // gigaam's `rtt_half(x) = [-x[..., d_k/2:], x[..., :d_k/2]]`. RoPE uses
    // ne[2] as the position axis (== T) and ne[3] as the batch (== B).
    ggml_tensor * x_rot = ggml_reshape_4d(ctx, x, head_dim, n_head, T, Bb);
    // GigaAM passes pos_emb_max_len (default 5000) as the rotary `base`
    // (theta), NOT the standard 10000: the dumped enc.pos_emb tensor matches
    // base=5000 to ~1e-6 and base=10000 is off by ~0.1.
    x_rot               = ggml_rope_ext(ctx, x_rot, positions, /*c=*/nullptr,
                                        /*n_dims=*/head_dim, GGML_ROPE_TYPE_NEOX,
                                        /*n_ctx_orig=*/0,
                                        /*theta=*/5000.0f,
                                        /*freq_scale=*/1.0f,
                                        /*ext_factor=*/0.0f,
                                        /*attn_factor=*/1.0f,
                                        /*beta_fast=*/32.0f,
                                        /*beta_slow=*/1.0f);
    // Fold heads back to [d_model, T, B] for the Wq/Wk projection.
    x_rot               = ggml_cont(ctx, ggml_reshape_3d(ctx, x_rot, d_model, T, Bb));

    // Wq, Wk on the rotated input; Wv on the original x. mul_mat broadcasts
    // the weight across the batch axis (ne[2]); outputs stay [d_model, T, B].
    ggml_tensor * q = ggml_mul_mat(ctx, b.attn_q_w, x_rot);
    if (b.attn_q_b != nullptr) {
        q = ggml_add(ctx, q, b.attn_q_b);
    }
    ggml_tensor * k = ggml_mul_mat(ctx, b.attn_k_w, x_rot);
    if (b.attn_k_b != nullptr) {
        k = ggml_add(ctx, k, b.attn_k_b);
    }
    ggml_tensor * v = ggml_mul_mat(ctx, b.attn_v_w, x);
    if (b.attn_v_b != nullptr) {
        v = ggml_add(ctx, v, b.attn_v_b);
    }

    // Reshape for SDPA. Target layout: [head_dim, T, n_head, B]. The head
    // split lands n_head at ne[1] and the utterance batch at ne[3], so the
    // attention matmuls batch over (head, utterance) cleanly.
    auto to_attn = [&](ggml_tensor * t) -> ggml_tensor * {
        // t: [d_model, T, B] -> [head_dim, n_head, T, B] -> [head_dim, T, n_head, B]
        t = ggml_reshape_4d(ctx, t, head_dim, n_head, T, Bb);
        t = ggml_permute(ctx, t, 0, 2, 1, 3);
        return ggml_cont(ctx, t);
    };
    q = to_attn(q);
    k = to_attn(k);
    v = to_attn(v);

    ggml_tensor * o;
    if (flash) {
        o = ggml_flash_attn_ext(ctx, q, k, v, /*mask=*/nullptr, scale, 0.0f, 0.0f);
        // ggml_flash_attn_ext returns [head_dim, n_head, T, B].
        o = ggml_permute(ctx, o, 0, 2, 1, 3);
        o = ggml_cont(ctx, o);
    } else {
        ggml_tensor * kq = ggml_mul_mat(ctx, k, q);  // [T_k, T_q, n_head, B]
        // Additive key-padding mask: ne=[T_k, 1, 1, B] broadcasts over
        // queries (ne[1]) and heads (ne[2]). -INF/0 are scale-invariant, so
        // adding before soft_max_ext's internal scale is correct.
        if (attn_pad_mask != nullptr) {
            kq = ggml_add(ctx, kq, attn_pad_mask);
        }
        ggml_tensor * kq_soft = ggml_soft_max_ext(ctx, kq, /*mask=*/nullptr, scale, 0.0f);
        ggml_tensor * v_t     = ggml_cont(ctx, ggml_permute(ctx, v, 1, 0, 2, 3));
        o                     = ggml_mul_mat(ctx, v_t, kq_soft);
    }
    // o: [head_dim, T, n_head, B] -> [head_dim, n_head, T, B] -> [d_model, T, B]
    o = ggml_permute(ctx, o, 0, 2, 1, 3);
    o = ggml_cont(ctx, o);
    o = ggml_reshape_3d(ctx, o, d_model, T, Bb);

    // Output projection.
    o = ggml_mul_mat(ctx, b.attn_out_w, o);
    if (b.attn_out_b != nullptr) {
        o = ggml_add(ctx, o, b.attn_out_b);
    }
    return o;
}

// Build one Conformer block with rotary attention. Returns the
// post-norm-out tensor (= the block output).
ggml_tensor * build_block(ggml_context *           ctx,
                          ggml_tensor *            x,
                          ggml_tensor *            positions,
                          const GigaamBlock &      b,
                          int                      d_model,
                          int                      n_head,
                          int                      conv_kernel,
                          bool                     use_flash,
                          const conf::ConvPolicy & policy,
                          ggml_tensor *            attn_pad_mask  = nullptr,
                          ggml_tensor *            conv_pad_mask  = nullptr,
                          ggml_tensor **           out_after_attn = nullptr,
                          ggml_tensor **           out_after_conv = nullptr) {
    // Macaron FF1: x + 0.5 * FF(LN(x)).
    x = conf::macaron_ff_residual(ctx, x, b.norm_ff1_w, b.norm_ff1_b, b.ff1_lin1_w, b.ff1_lin1_b, b.ff1_lin2_w,
                                  b.ff1_lin2_b);

    // Self-attention residual: x + attn(LN(x)).
    {
        ggml_tensor * y = conf::layer_norm(ctx, x, b.norm_attn_w, b.norm_attn_b);
        y               = build_rotary_attn(ctx, y, positions, b, d_model, n_head, use_flash, attn_pad_mask);
        x               = ggml_add(ctx, x, y);
    }
    if (out_after_attn != nullptr) {
        *out_after_attn = x;
    }

    // Conv module residual: x + conv(LN(x)).
    {
        ggml_tensor *     y  = conf::layer_norm(ctx, x, b.norm_conv_w, b.norm_conv_b);
        conf::BlockView   cv = to_conv_view(b);
        conf::BlockParams cp{};
        cp.d_model        = d_model;
        cp.n_head         = n_head;
        cp.conv_kernel    = conv_kernel;
        cp.kv_type        = GGML_TYPE_F32;
        cp.use_flash      = use_flash;
        cp.policy         = policy;
        cp.conv_norm_type = conf::BlockParams::ConvNormType::LayerNorm;
        cp.conv_pad_mask  = conv_pad_mask;
        y                 = conf::conv_module(ctx, y, cv, cp);
        x                 = ggml_add(ctx, x, y);
    }
    if (out_after_conv != nullptr) {
        *out_after_conv = x;
    }

    // Macaron FF2.
    x = conf::macaron_ff_residual(ctx, x, b.norm_ff2_w, b.norm_ff2_b, b.ff2_lin1_w, b.ff2_lin1_b, b.ff2_lin2_w,
                                  b.ff2_lin2_b);

    // Final per-block LayerNorm. NOTE: the reference returns norm_out(x)
    // (not the residual stream); the block output IS the post-LN value.
    x = conf::layer_norm(ctx, x, b.norm_out_w, b.norm_out_b);
    return x;
}

}  // namespace

// One stride-2 conv with symmetric (k-1)/2 padding: floor((in-1)/2)+1.
// GigaAM's k=5/pad=2 and parakeet's k=3/pad=1 both reduce to this.
static int pre_encode_t_out(int in) {
    return (in - 1) / 2 + 1;
}

EncoderBuild build_encoder_graph(ggml_context *        ctx,
                                 const GigaamWeights & w,
                                 const GigaamHParams & hp,
                                 int                   n_mel_frames,
                                 ggml_type /*kv_type*/,
                                 const char * backend_name,
                                 int          n_batch,
                                 bool         batch_var_len) {
    EncoderBuild eb{};
    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "gigaam encoder: invalid arg "
                "(ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }
    if (n_batch < 1) {
        n_batch = 1;
    }
    const bool var_len_masks = batch_var_len && n_batch > 1;

    // Mel input handle. ne=[T_mel, n_mels, n_batch] (n_batch == 1 keeps the
    // pre-batch 2-D layout — ggml treats a trailing ne==1 axis identically).
    eb.mel_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, n_mel_frames, hp.fe_num_mels, n_batch);
    if (eb.mel_in == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam encoder: failed to allocate mel_in tensor");
        return eb;
    }
    ggml_set_name(eb.mel_in, "mel.in");
    ggml_set_input(eb.mel_in);

    // Variable-length masked-subsampling inputs (one per conv-relu stage).
    PreEncodeMasks pe_masks{};
    if (var_len_masks) {
        const int T_s1           = pre_encode_t_out(n_mel_frames);
        const int T_s2           = pre_encode_t_out(T_s1);
        eb.pre_encode_mask_s1_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, T_s1, 1, n_batch);
        eb.pre_encode_mask_s2_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, T_s2, 1, n_batch);
        ggml_set_name(eb.pre_encode_mask_s1_in, "enc.pre_encode.mask_s1");
        ggml_set_name(eb.pre_encode_mask_s2_in, "enc.pre_encode.mask_s2");
        ggml_set_input(eb.pre_encode_mask_s1_in);
        ggml_set_input(eb.pre_encode_mask_s2_in);
        pe_masks.mask_s1 = eb.pre_encode_mask_s1_in;
        pe_masks.mask_s2 = eb.pre_encode_mask_s2_in;
    }

    // Pre-encode: 2 stride-2 conv1d.
    ggml_tensor * x = build_pre_encode(ctx, w.pre_encode, eb.mel_in, hp.enc_subs_kernel_size, "enc.pre_encode",
                                       var_len_masks ? &pe_masks : nullptr);
    if (x == nullptr) {
        return eb;
    }
    ggml_set_name(x, "enc.subsample.out");
    eb.dumps.pre_encode_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    if (w.blocks.empty()) {
        eb.out             = x;
        eb.dumps.final_out = x;
        eb.graph           = ggml_new_graph(ctx);
        ggml_build_forward_expand(eb.graph, x);
        return eb;
    }

    const int64_t T_enc = x->ne[1];

    // Variable-length per-utterance block masks (filled host-side by the
    // driver). attn_pad_mask zeros softmax over padded keys; conv_pad_mask
    // zeros padded frames before each depthwise conv.
    if (var_len_masks) {
        eb.attn_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, 1, n_batch);
        eb.conv_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, n_batch, 1);
        ggml_set_name(eb.attn_pad_mask_in, "enc.attn_pad_mask");
        ggml_set_name(eb.conv_pad_mask_in, "enc.conv_pad_mask");
        ggml_set_input(eb.attn_pad_mask_in);
        ggml_set_input(eb.conv_pad_mask_in);
    }

    // Positions tensor for rotary. Allocate here, fill via
    // ggml_backend_tensor_set after compute alloc.
    ggml_tensor * positions = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_enc);
    ggml_set_name(positions, "enc.positions");
    ggml_set_input(positions);
    eb.dumps.pos_emb = positions;

    // Conv policy. GigaAM's depthwise k=5 — Metal direct path works.
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    policy.direct_dw_in_block      = true;
    policy.direct_dw_in_pre_encode = false;
    policy.causal_pre_encode       = false;

    // Use flash attention on GPU backends; manual path on CPU.
    const bool use_flash = (backend_name != nullptr && std::string(backend_name).find("CPU") == std::string::npos);

    const int n_layers = hp.enc_n_layers;
    eb.dumps.all_block_outs.reserve(n_layers);
    eb.dumps.mid_block_idx  = n_layers / 2 - 1;
    eb.dumps.last_block_idx = n_layers - 1;

    for (int i = 0; i < n_layers; ++i) {
        x = build_block(ctx, x, positions, w.blocks[i], hp.enc_d_model, hp.enc_n_heads, hp.enc_conv_kernel, use_flash,
                        policy, eb.attn_pad_mask_in, eb.conv_pad_mask_in,
                        i == 0 ? &eb.dumps.block0_after_attn : nullptr, i == 0 ? &eb.dumps.block0_after_conv : nullptr);

        eb.dumps.all_block_outs.push_back(x);

        if (i == 0) {
            ggml_set_name(x, "enc.block.0.out");
            eb.dumps.block0_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
        if (i == eb.dumps.mid_block_idx) {
            char nm[32];
            std::snprintf(nm, sizeof(nm), "enc.block.%d.out", i);
            ggml_set_name(x, nm);
            eb.dumps.mid_block_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
        if (i == eb.dumps.last_block_idx) {
            char nm[32];
            std::snprintf(nm, sizeof(nm), "enc.block.%d.out", i);
            ggml_set_name(x, nm);
            eb.dumps.last_block_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
    }

    // x has ne=[d_model, T_enc, 1] = PyTorch [B, T, D]. The RNN-T / CTC
    // decoder consumes this layout directly (matches the reference's
    // `encoded.transpose(1, 2)` for the joint network). Expose it
    // before the final transpose.
    ggml_set_name(x, "rnnt.encoded");
    eb.dumps.rnnt_encoded = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // The reference ConformerEncoder.forward returns
    // `audio_signal.transpose(1, 2)` — [B, T, D] -> [B, D, T]. Apply
    // the final transpose to match the reference enc.out byte layout.
    // CRITICAL: do NOT wrap in a reshape view afterwards; ggml_set_output
    // on a view doesn't preserve the materialized cont buffer.
    ggml_tensor * enc_out_t = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));
    ggml_set_name(enc_out_t, "enc.out");
    eb.out             = enc_out_t;
    eb.dumps.final_out = enc_out_t;
    transcribe::debug::mark_tensor_for_dump(enc_out_t);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    ggml_build_forward_expand(eb.graph, eb.out);
    return eb;
}

}  // namespace transcribe::gigaam
