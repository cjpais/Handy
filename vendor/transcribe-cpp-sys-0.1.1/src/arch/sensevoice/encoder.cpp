// arch/sensevoice/encoder.cpp - SenseVoice SAN-M encoder graph builder.
//
// Forward (single-utterance, channel-innermost ggml ne; T = 4 + T_lfr after
// the prefix prepend):
//   frontend.in [d_input=560, T_lfr] -> prefix prepend (lid + event_emo +
//   textnorm) -> scale by sqrt(d_model=512) + sinusoidal PE ->
//   encoders0[0] (SAN-M projection 560->512) -> encoders x49 ->
//   after_norm -> tp_encoders x20 -> tp_norm -> ctc.head [vocab, T].
//
// SAN-M block topology lives in src/sanm/sanm.h (shared with funasr_nano).
// This file owns the surrounding shape — prefix prepend, sinusoidal PE
// add, after_norm / tp_norm, CTC head, and dump-marker wiring.
//
// Mask is omitted: batch=1 inference always has every position valid.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "sanm/sanm.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::sensevoice {

namespace {

namespace conf = transcribe::conformer;
namespace sanm = transcribe::sanm;
using conf::named;

sanm::SanmBlockView to_sanm_view(const SenseVoiceBlock & b) {
    sanm::SanmBlockView v;
    v.norm_attn_w = b.norm_attn_w;
    v.norm_attn_b = b.norm_attn_b;
    v.attn_qkv_w  = b.attn_qkv_w;
    v.attn_qkv_b  = b.attn_qkv_b;
    v.attn_out_w  = b.attn_out_w;
    v.attn_out_b  = b.attn_out_b;
    v.attn_fsmn_w = b.attn_fsmn_w;
    v.norm_ffn_w  = b.norm_ffn_w;
    v.norm_ffn_b  = b.norm_ffn_b;
    v.ffn_fc1_w   = b.ffn_fc1_w;
    v.ffn_fc1_b   = b.ffn_fc1_b;
    v.ffn_fc2_w   = b.ffn_fc2_w;
    v.ffn_fc2_b   = b.ffn_fc2_b;
    return v;
}

// Name a tensor, mark it for the post-compute dump pass (no-op when
// TRANSCRIBE_DUMP_DIR is unset), and stash the pointer in the dumps slot.
void mark_dump(ggml_tensor *& slot, ggml_tensor * t, const char * name) {
    named(t, name);
    transcribe::debug::mark_tensor_for_dump(t);
    slot = t;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *            ctx,
                                 const SenseVoiceWeights & w,
                                 const SenseVoiceHParams & hp,
                                 int                       n_lfr_frames,
                                 int                       n_batch,
                                 bool                      batch_var_len) {
    EncoderBuild eb{};

    if (ctx == nullptr || n_lfr_frames <= 0 || n_batch <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice encoder: invalid arg "
                "(ctx=%p, n_lfr_frames=%d, n_batch=%d)",
                static_cast<void *>(ctx), n_lfr_frames, n_batch);
        return eb;
    }

    const int  d_input = hp.enc_d_input;
    const int  d_model = hp.enc_d_model;
    const int  T_in    = n_lfr_frames;
    const int  T       = T_in + 4;  // post-prefix length
    const int  B       = n_batch;
    const bool var_len = batch_var_len && B > 1;

    sanm::SanmBlockParams block_params{
        /*n_heads=*/hp.enc_n_heads,
        /*d_model=*/d_model,
        /*kernel=*/hp.enc_kernel,
    };

    // ----- inputs ----------------------------------------------------
    // Batch axis at ne[2]. B == 1 collapses to the pre-batch [d_input, T_in]
    // shape (ne[2] == 1), keeping the single-shot graph byte-identical.
    eb.frontend_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, d_input, T_in, B);
    named(eb.frontend_in, "frontend.in");
    ggml_set_input(eb.frontend_in);
    mark_dump(eb.dumps.frontend_out, eb.frontend_in, "frontend.fbank.lfr.cmvn.out");

    // Variable-length batch masks (filled host-side by the driver).
    if (var_len) {
        // Attention key-padding mask: [T, 1, 1, B], 0 on real keys / -INF on
        // padded. Forces the manual SDPA path in every block.
        eb.attn_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T, 1, 1, B);
        named(eb.attn_pad_mask_in, "batch.attn_pad_mask");
        ggml_set_input(eb.attn_pad_mask_in);

        // FSMN valid-frame mask: [1, T, B], 1 on real frames / 0 on padded.
        eb.conv_pad_mask_in = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, 1, T, B);
        named(eb.conv_pad_mask_in, "batch.conv_pad_mask");
        ggml_set_input(eb.conv_pad_mask_in);

        block_params.attn_pad_mask = eb.attn_pad_mask_in;
        block_params.conv_pad_mask = eb.conv_pad_mask_in;
        block_params.use_flash     = false;
    }

    eb.lid_idx = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    named(eb.lid_idx, "prefix.lid_idx");
    ggml_set_input(eb.lid_idx);

    eb.event_emo_idx = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 2);
    named(eb.event_emo_idx, "prefix.event_emo_idx");
    ggml_set_input(eb.event_emo_idx);

    eb.textnorm_idx = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    named(eb.textnorm_idx, "prefix.textnorm_idx");
    ggml_set_input(eb.textnorm_idx);

    // Sinusoidal PE input (host fills with 1-based positions encoded
    // at depth=d_input).
    ggml_tensor * pe_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_input, T);
    named(pe_in, "pe.in");
    ggml_set_input(pe_in);
    eb.pe_in = pe_in;

    // ----- prefix prepend --------------------------------------------
    ggml_tensor * lid_emb       = ggml_get_rows(ctx, w.embed, eb.lid_idx);        // [d_in, 1]
    ggml_tensor * event_emo_emb = ggml_get_rows(ctx, w.embed, eb.event_emo_idx);  // [d_in, 2]
    ggml_tensor * textnorm_emb  = ggml_get_rows(ctx, w.embed, eb.textnorm_idx);   // [d_in, 1]

    mark_dump(eb.dumps.prefix_lid, lid_emb, "enc.prefix.lid_emb");
    mark_dump(eb.dumps.prefix_event_emo, event_emo_emb, "enc.prefix.event_emo_emb");
    mark_dump(eb.dumps.prefix_textnorm, textnorm_emb, "enc.prefix.textnorm_emb");

    // Reference order:
    //   speech = cat(textnorm,     speech)
    //   speech = cat(lid, ev_emo,  speech)
    // -> [lid(1), ev_emo(2), textnorm(1), speech...] along ne[1] (time).
    ggml_tensor * x;
    if (B == 1) {
        // Single-shot: keep the pre-batch concat grouping verbatim.
        ggml_tensor * tn_then_speech = ggml_concat(ctx, textnorm_emb, eb.frontend_in, /*dim=*/1);
        ggml_tensor * lid_evev       = ggml_concat(ctx, lid_emb, event_emo_emb, /*dim=*/1);
        x                            = ggml_concat(ctx, lid_evev, tn_then_speech, /*dim=*/1);
    } else {
        // Batched: the 4-token prefix is identical across utterances (one
        // run_params per batch), so build it once at ne[2] == 1 and broadcast
        // it over the batch before prepending to each utterance's speech.
        ggml_tensor * lid_evev = ggml_concat(ctx, lid_emb, event_emo_emb, /*dim=*/1);  // [d_in,3,1]
        ggml_tensor * prefix   = ggml_concat(ctx, lid_evev, textnorm_emb, /*dim=*/1);  // [d_in,4,1]
        ggml_tensor * tmpl     = ggml_new_tensor_3d(ctx, GGML_TYPE_F32, d_input, 4, B);
        prefix                 = ggml_repeat(ctx, prefix, tmpl);                       // [d_in,4,B]
        x                      = ggml_concat(ctx, prefix, eb.frontend_in, /*dim=*/1);  // [d_in,T,B]
    }
    mark_dump(eb.dumps.input_with_prefix, x, "enc.input.with_prefix");

    // ----- embedding scale + PE add ----------------------------------
    // Scale by sqrt(d_model). NOTE: x is still 560-d here, but the
    // scale is sqrt(d_model=512), per the reference's encoder.forward().
    const float embed_scale = std::sqrt(static_cast<float>(d_model));
    x                       = ggml_scale(ctx, x, embed_scale);
    x                       = ggml_add(ctx, x, pe_in);
    mark_dump(eb.dumps.embed_out, x, "enc.embed.out");

    // ----- encoders0[0] (560 -> 512 projection) ----------------------
    x = sanm::sanm_block_projection(ctx, x, to_sanm_view(w.encoders0), block_params);
    mark_dump(eb.dumps.encoders0_0_out, x, "enc.encoders0.0.out");

    // ----- encoders[0..n-2] ------------------------------------------
    const int n_main = static_cast<int>(w.encoders.size());
    if (n_main > 0) {
        const int last_idx = n_main - 1;
        const int mid_idx  = n_main / 2;
        for (int i = 0; i < n_main; ++i) {
            x = sanm::sanm_block_residual(ctx, x, to_sanm_view(w.encoders[static_cast<size_t>(i)]), block_params);
            if (i == 0) {
                mark_dump(eb.dumps.encoders_first, x, "enc.encoders.0.out");
            } else if (i == mid_idx) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.encoders.%d.out", i);
                mark_dump(eb.dumps.encoders_mid, x, nm);
            } else if (i == last_idx) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.encoders.%d.out", i);
                mark_dump(eb.dumps.encoders_last, x, nm);
            }
        }
    }

    // ----- after_norm ------------------------------------------------
    x = sanm::sv_layer_norm(ctx, x, w.after_norm_w, w.after_norm_b);
    mark_dump(eb.dumps.after_norm_out, x, "enc.after_norm.out");

    // ----- tp_encoders[0..tp-1] --------------------------------------
    const int n_tp = static_cast<int>(w.tp_encoders.size());
    if (n_tp > 0) {
        const int last_idx = n_tp - 1;
        const int mid_idx  = n_tp / 2;
        for (int i = 0; i < n_tp; ++i) {
            x = sanm::sanm_block_residual(ctx, x, to_sanm_view(w.tp_encoders[static_cast<size_t>(i)]), block_params);
            if (i == 0) {
                mark_dump(eb.dumps.tp_encoders_first, x, "enc.tp_encoders.0.out");
            } else if (i == mid_idx) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.tp_encoders.%d.out", i);
                mark_dump(eb.dumps.tp_encoders_mid, x, nm);
            } else if (i == last_idx) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.tp_encoders.%d.out", i);
                mark_dump(eb.dumps.tp_encoders_last, x, nm);
            }
        }
    }

    // ----- tp_norm ---------------------------------------------------
    x = sanm::sv_layer_norm(ctx, x, w.tp_norm_w, w.tp_norm_b);
    mark_dump(eb.dumps.tp_norm_out, x, "enc.tp_norm.out");

    // ----- CTC head --------------------------------------------------
    ggml_tensor * logits = ggml_mul_mat(ctx, w.ctc_head_w, x);
    logits               = ggml_add(ctx, logits, w.ctc_head_b);
    mark_dump(eb.dumps.ctc_logits, logits, "ctc.logits.raw");

    // log_softmax over the vocab axis (ne[0]).
    ggml_tensor * log_probs = ggml_log(ctx, ggml_soft_max(ctx, logits));
    mark_dump(eb.dumps.ctc_log_probs, log_probs, "ctc.log_probs");

    eb.out = log_probs;
    ggml_set_output(eb.out);

    // 70 SAN-M blocks * ~30 ops/block + frontend/PE/CTC ≈ 2300 nodes; the
    // variable-length batch path's manual SDPA adds a handful more per block.
    // 8192 leaves ample headroom.
    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    return eb;
}

}  // namespace transcribe::sensevoice
