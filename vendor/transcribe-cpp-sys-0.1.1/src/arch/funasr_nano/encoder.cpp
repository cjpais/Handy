// arch/funasr_nano/encoder.cpp - SAN-M encoder graph (no prefix prepend,
// no CTC head).
//
// frontend.in [d_input=560, T_lfr] -> scale by sqrt(d_model=512) + sinusoidal
// PE -> encoders0[0] (SAN-M projection 560->512) -> encoders x49 (residual
// SAN-M) -> after_norm -> tp_encoders x20 -> tp_norm [d_model, T_lfr].
//
// SAN-M block topology lives in src/sanm/sanm.h (shared with sensevoice).
// This file owns the surrounding shape — embedding scale + PE add,
// after_norm / tp_norm, and dump-marker wiring.

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

namespace transcribe::funasr_nano {

namespace {

namespace conf = transcribe::conformer;
namespace sanm = transcribe::sanm;
using conf::named;

sanm::SanmBlockView to_sanm_view(const EncBlock & b) {
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

void mark_dump(ggml_tensor *& slot, ggml_tensor * t, const char * name) {
    named(t, name);
    transcribe::debug::mark_tensor_for_dump(t);
    slot = t;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *            ctx,
                                 const FunAsrNanoWeights & w,
                                 const FunAsrNanoHParams & hp,
                                 int                       n_lfr_frames) {
    EncoderBuild eb{};
    if (ctx == nullptr || n_lfr_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano encoder: invalid arg "
                "(ctx=%p, n_lfr_frames=%d)",
                static_cast<void *>(ctx), n_lfr_frames);
        return eb;
    }

    const int d_input = hp.enc_d_input;
    const int d_model = hp.enc_d_model;
    const int T       = n_lfr_frames;

    const sanm::SanmBlockParams block_params{
        /*n_heads=*/hp.enc_n_heads,
        /*d_model=*/d_model,
        /*kernel=*/hp.enc_kernel,
    };

    eb.frontend_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_input, T);
    named(eb.frontend_in, "frontend.in");
    ggml_set_input(eb.frontend_in);
    mark_dump(eb.dumps.frontend_out, eb.frontend_in, "frontend.fbank.lfr.cmvn.out");

    eb.pe_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, d_input, T);
    named(eb.pe_in, "pe.in");
    ggml_set_input(eb.pe_in);

    // Embedding scale + PE add.
    const float   embed_scale = std::sqrt(static_cast<float>(d_model));
    ggml_tensor * x           = ggml_scale(ctx, eb.frontend_in, embed_scale);
    x                         = ggml_add(ctx, x, eb.pe_in);
    mark_dump(eb.dumps.embed_out, x, "enc.embed.out");

    // encoders0[0] (560 → 512 projection).
    x = sanm::sanm_block_projection(ctx, x, to_sanm_view(w.encoders0), block_params);
    mark_dump(eb.dumps.encoders0_0_out, x, "enc.encoders0.0.out");

    const int n_main = static_cast<int>(w.encoders.size());
    if (n_main > 0) {
        const int last_idx = n_main - 1;
        const int mid_idx  = n_main / 2;
        for (int i = 0; i < n_main; ++i) {
            x = sanm::sanm_block_residual(ctx, x, to_sanm_view(w.encoders[i]), block_params);
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

    x = sanm::sv_layer_norm(ctx, x, w.after_norm_w, w.after_norm_b);
    mark_dump(eb.dumps.after_norm_out, x, "enc.after_norm.out");

    const int n_tp = static_cast<int>(w.tp_encoders.size());
    if (n_tp > 0) {
        const int last_idx = n_tp - 1;
        const int mid_idx  = n_tp / 2;
        for (int i = 0; i < n_tp; ++i) {
            x = sanm::sanm_block_residual(ctx, x, to_sanm_view(w.tp_encoders[i]), block_params);
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

    x = sanm::sv_layer_norm(ctx, x, w.tp_norm_w, w.tp_norm_b);
    mark_dump(eb.dumps.tp_norm_out, x, "enc.tp_norm.out");

    eb.out = x;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    return eb;
}

}  // namespace transcribe::funasr_nano
