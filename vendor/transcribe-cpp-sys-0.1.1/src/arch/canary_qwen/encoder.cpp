// arch/canary_qwen/encoder.cpp - SALM perception encoder graph builder.
//
// Glue over transcribe::conformer with a perception-projection tail.
// The conformer body is byte-for-byte identical to canary-1b-flash:
// every linear (Q/K/V/out, both macaron FFs, attention-pos projection,
// conv pointwise pair) carries a bias.
//
// What's canary_qwen-specific: a TRAILING Linear(d_model=1024,
// output_dim=2048) + bias that lifts the FastConformer output up to
// the LM hidden dim. The reference dumper emits `enc.final` for the
// pre-projection tensor and `perception.proj.out` for the post-
// projection tensor; we mark both for dump.

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace transcribe::canary_qwen {

namespace {

namespace conf = transcribe::conformer;

bool detect_direct_dw_in_block(const char * /*backend*/) {
    return conf::resolve_conv_direct("TRANSCRIBE_CONV_DIRECT_DW", "TRANSCRIBE_CONV_NO_DIRECT_DW",
                                     /*backend_default=*/true);
}

conf::PreEncodeView to_view(const PreEncodeSlots & pe) {
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

conf::BlockView to_view(const EncBlockSlots & b) {
    conf::BlockView v;

    v.norm_ff1_w = b.norm_ff1_w;
    v.norm_ff1_b = b.norm_ff1_b;
    v.ff1_lin1_w = b.ff1_lin1_w;
    v.ff1_lin1_b = b.ff1_lin1_b;
    v.ff1_lin2_w = b.ff1_lin2_w;
    v.ff1_lin2_b = b.ff1_lin2_b;

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

EncoderBuild build_encoder_graph(ggml_context *            ctx,
                                 const CanaryQwenWeights & w,
                                 const CanaryQwenHParams & hp,
                                 int                       n_mel_frames,
                                 ggml_type                 kv_type,
                                 bool                      use_flash,
                                 const char *              backend_name) {
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    policy.direct_dw_in_block      = detect_direct_dw_in_block(backend_name);
    policy.direct_dw_in_pre_encode = false;

    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen encoder: invalid arg (ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }

    if (hp.enc_subsampling_factor != 8 || hp.fe_num_mels != 128) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary_qwen encoder: unsupported geometry "
                "subsampling_factor=%d num_mels=%d",
                hp.enc_subsampling_factor, hp.fe_num_mels);
        return eb;
    }

    // Mel input.
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
                                             /*error_tag=*/"canary_qwen");
    if (x == nullptr) {
        return eb;
    }
    eb.dumps.pre_encode_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    if (!w.blocks.empty()) {
        const int64_t T_enc   = x->ne[1];
        const int64_t pos_len = 2 * T_enc - 1;

        eb.pos_emb_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, pos_len);
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

        const int mid_idx = hp.enc_n_layers / 2;

        // Block 0
        x                   = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[0]), bparams);
        x                   = conf::named(x, "enc.block.0.out");
        eb.dumps.block0_out = x;
        transcribe::debug::mark_tensor_for_dump(x);

        // Blocks 1..N-1
        for (size_t i = 1; i < w.blocks.size(); ++i) {
            x = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[i]), bparams);
            if (i == static_cast<size_t>(mid_idx)) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
                x                      = conf::named(x, bname);
                eb.dumps.block_mid_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
            if (i == w.blocks.size() - 1) {
                char bname[64];
                std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
                x                       = conf::named(x, bname);
                eb.dumps.block_last_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
        }
    }

    // Pre-projection encoder output. The reference dumper hooks the
    // FastConformer encoder forward, whose return is (B, C, T) per
    // NeMo's external convention — squeezed to (1024, 138). Our pipeline
    // carries x as ggml ne=[d_model, T_enc] which dumps as numpy
    // (T_enc, d_model). Permute + cont so the dumped shape matches
    // reference exactly (and so compare_tensors stops flagging SHAPE).
    {
        ggml_tensor * final_ct    = ggml_cont(ctx, ggml_permute(ctx, x, 1, 0, 2, 3));
        ggml_tensor * named_final = conf::named(final_ct, "enc.final");
        eb.dumps.final_out        = named_final;
        transcribe::debug::mark_tensor_for_dump(named_final);
    }

    // Perception projection: Linear(d_enc=1024, output_dim=2048) + bias.
    // ggml_mul_mat(W, x) where:
    //   W has ggml shape [d_enc, output_dim] (stored "transposed" by
    //                    ggml convention; W[i,j] = nn.Linear.weight[j,i])
    //   x has ggml shape [d_enc, T_enc]
    // -> output has shape [output_dim, T_enc]
    {
        ggml_tensor * proj = ggml_mul_mat(ctx, w.perception_proj.weight, x);
        if (w.perception_proj.bias != nullptr) {
            proj = ggml_add(ctx, proj, w.perception_proj.bias);
        }
        x                       = conf::named(proj, "perception.proj.out");
        eb.dumps.perception_out = x;
        transcribe::debug::mark_tensor_for_dump(x);
    }

    eb.out = x;
    ggml_set_output(eb.out);

    eb.graph = ggml_new_graph_custom(ctx, 16384, false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    // Side-branch dump tensors (those NOT on the path from inputs to
    // eb.out) must be expanded explicitly so the scheduler allocates a
    // backend buffer for them. enc.final is a permute+cont OFF of the
    // pre-projection encoder tensor — its data is needed for the dump
    // but is not consumed by perception_proj. Without this expand call
    // ggml_backend_tensor_get aborts ("tensor buffer not set").
    if (eb.dumps.final_out != nullptr) {
        ggml_build_forward_expand(eb.graph, eb.dumps.final_out);
    }

    return eb;
}

}  // namespace transcribe::canary_qwen
