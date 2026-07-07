// arch/cohere/encoder.cpp - Cohere ASR Conformer encoder graph builder.
//
// Thin glue over the shared Conformer helpers in transcribe::conformer.
// Cohere-specific bits: detect_direct_dw (depthwise dispatch policy),
// to_view() projections of CoherePreEncode / CohereBlock onto the shared
// views, and build_encoder_graph (including the enc_dec_proj projection).

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace transcribe::cohere {

namespace {

namespace conf = transcribe::conformer;

// Depthwise dispatch policy: direct ggml_conv_2d_dw_direct on Vulkan/CUDA
// (native kernels), im2col + mul_mat on Metal/CPU. See the ConvPolicy
// comment in conformer.h for why the two dw sites share one detect result.
bool detect_direct_dw(const char * backend) {
    const bool backend_default =
        backend != nullptr && (std::strstr(backend, "Vulkan") != nullptr || std::strstr(backend, "CUDA") != nullptr);
    return conf::resolve_conv_direct("TRANSCRIBE_CONV_DIRECT_DW", "TRANSCRIBE_CONV_NO_DIRECT_DW", backend_default);
}

// ----- Views ------------------------------------------------------

conf::PreEncodeView to_view(const CoherePreEncode & pe) {
    conf::PreEncodeView v;
    v.conv0_w = pe.conv0_w;
    v.conv0_b = pe.conv0_b;
    v.conv2_w = pe.conv2_w;
    v.conv2_b = pe.conv2_b;
    v.conv3_w = pe.conv3_w;
    v.conv3_b = pe.conv3_b;
    v.conv5_w = pe.conv5_w;
    v.conv5_b = pe.conv5_b;
    v.conv6_w = pe.conv6_w;
    v.conv6_b = pe.conv6_b;
    v.out_w   = pe.out_w;
    v.out_b   = pe.out_b;
    return v;
}

conf::BlockView to_view(const CohereBlock & b) {
    conf::BlockView v;

    // FF1 (biases present).
    v.norm_ff1_w = b.norm_ff1_w;
    v.norm_ff1_b = b.norm_ff1_b;
    v.ff1_lin1_w = b.ff1_lin1_w;
    v.ff1_lin1_b = b.ff1_lin1_b;
    v.ff1_lin2_w = b.ff1_lin2_w;
    v.ff1_lin2_b = b.ff1_lin2_b;

    // Self-attention (biases on Q/K/V/out; pos projection is bias-free).
    v.norm_attn_w = b.norm_attn_w;
    v.norm_attn_b = b.norm_attn_b;
    v.attn_q_w    = b.attn_q_w;
    v.attn_q_b    = b.attn_q_b;
    v.attn_k_w    = b.attn_k_w;
    v.attn_k_b    = b.attn_k_b;
    v.attn_v_w    = b.attn_v_w;
    v.attn_v_b    = b.attn_v_b;
    v.attn_out_w  = b.attn_out_w;
    v.attn_out_b  = b.attn_out_b;
    v.attn_pos_w  = b.attn_pos_w;
    v.attn_pos_u  = b.attn_pos_u;
    v.attn_pos_v  = b.attn_pos_v;

    // Conv module (biases on pw1/dw/pw2).
    v.norm_conv_w         = b.norm_conv_w;
    v.norm_conv_b         = b.norm_conv_b;
    v.conv_pw1_w          = b.conv_pw1_w;
    v.conv_pw1_b          = b.conv_pw1_b;
    v.conv_dw_w           = b.conv_dw_w;
    v.conv_dw_b           = b.conv_dw_b;
    v.conv_pw2_w          = b.conv_pw2_w;
    v.conv_pw2_b          = b.conv_pw2_b;
    v.conv_bn_fused_scale = b.conv_bn_fused_scale;
    v.conv_bn_fused_bias  = b.conv_bn_fused_bias;

    // FF2 (biases present).
    v.norm_ff2_w = b.norm_ff2_w;
    v.norm_ff2_b = b.norm_ff2_b;
    v.ff2_lin1_w = b.ff2_lin1_w;
    v.ff2_lin1_b = b.ff2_lin1_b;
    v.ff2_lin2_w = b.ff2_lin2_w;
    v.ff2_lin2_b = b.ff2_lin2_b;

    v.norm_out_w = b.norm_out_w;
    v.norm_out_b = b.norm_out_b;
    return v;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *        ctx,
                                 const CohereWeights & w,
                                 const CohereHParams & hp,
                                 int                   n_mel_frames,
                                 ggml_type             kv_type,
                                 bool                  use_flash,
                                 const char *          backend_name) {
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    const bool direct_dw           = detect_direct_dw(backend_name);
    policy.direct_dw_in_block      = direct_dw;
    policy.direct_dw_in_pre_encode = direct_dw;

    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "cohere encoder: invalid arg "
                "(ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }

    if (hp.enc_subsampling_factor != 8 || hp.fe_num_mels != 128) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "cohere encoder: unsupported geometry "
                "subsampling_factor=%d num_mels=%d "
                "(only 8/128 implemented)",
                hp.enc_subsampling_factor, hp.fe_num_mels);
        return eb;
    }

    // Mel input handle.
    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mel_frames, hp.fe_num_mels);
    if (eb.mel_in == nullptr) {
        return eb;
    }
    ggml_set_name(eb.mel_in, "mel.in");
    ggml_set_input(eb.mel_in);
    eb.dumps.mel_in = eb.mel_in;

    // Pre-encode.
    ggml_tensor * x = conf::build_pre_encode(ctx, to_view(w.pre_encode), eb.mel_in, policy,
                                             /*name_prefix=*/"enc.pre_encode",
                                             /*error_tag=*/"cohere");
    if (x == nullptr) {
        return eb;
    }
    eb.dumps.pre_encode_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // Conformer blocks.
    if (!w.blocks.empty()) {
        const int64_t T_enc = x->ne[1];

        // Positional embedding input.
        const int64_t pos_len = 2 * T_enc - 1;
        eb.pos_emb_in         = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, pos_len);
        ggml_set_name(eb.pos_emb_in, "pos_emb.in");
        ggml_set_input(eb.pos_emb_in);
        eb.dumps.pos_emb = eb.pos_emb_in;

        conf::BlockParams bparams{};
        bparams.d_model     = hp.enc_d_model;
        bparams.n_head      = hp.enc_n_heads;
        bparams.conv_kernel = hp.enc_conv_kernel;
        bparams.kv_type     = kv_type;
        bparams.use_flash   = use_flash;
        bparams.policy      = policy;

        // Block 0 (named for the dump stream).
        {
            x                   = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[0]), bparams);
            x                   = conf::named(x, "enc.block.0.out");
            eb.dumps.block0_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }

        // Blocks 1..N-1.
        for (size_t i = 1; i < w.blocks.size(); ++i) {
            x = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[i]), bparams);
            // Spot-check at the middle block (n_layers/2 - 1).
            if (i == static_cast<size_t>(hp.enc_n_layers / 2 - 1)) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
                x                      = conf::named(x, bname);
                eb.dumps.block_mid_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
            // Spot-check last block.
            if (i == w.blocks.size() - 1) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
                x                       = conf::named(x, bname);
                eb.dumps.block_last_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
        }
    }

    eb.dumps.final_out = x;
    x                  = conf::named(x, "enc.final");
    transcribe::debug::mark_tensor_for_dump(x);

    // Encoder-decoder projection (Cohere-specific, not in the shared block).
    {
        ggml_tensor * proj        = ggml_mul_mat(ctx, w.enc_dec_proj.weight, x);
        proj                      = ggml_add(ctx, proj, w.enc_dec_proj.bias);
        proj                      = conf::named(proj, "enc_dec_proj.out");
        eb.dumps.enc_dec_proj_out = proj;
        transcribe::debug::mark_tensor_for_dump(proj);
        x = proj;
    }

    eb.out = x;
    ggml_set_output(eb.out);

    // Build the forward cgraph. 48 blocks + enc_dec_proj means a
    // large graph; 16384 leaves headroom.
    eb.graph = ggml_new_graph_custom(ctx, 16384, false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "cohere encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);

    return eb;
}

}  // namespace transcribe::cohere
