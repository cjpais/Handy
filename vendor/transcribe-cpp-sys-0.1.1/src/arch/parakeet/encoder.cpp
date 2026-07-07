// arch/parakeet/encoder.cpp - Parakeet Conformer encoder graph builder.
//
// Thin glue over the shared Conformer helpers in transcribe::conformer
// (src/conformer/conformer.{h,cpp}). Parakeet-specific pieces kept here:
//   - detect_direct_dw (different policy than Cohere).
//   - to_view() helpers projecting ParakeetPreEncode / ParakeetBlock onto
//     the shared nullable-pointer views (block linear layers ship without
//     biases; pre_encode convs carry biases that ARE copied through).
//   - build_encoder_graph orchestration — every block goes through
//     conf::build_conformer_block; block 0 passes a dump observer.
//
// Layout cheat sheet:
//   MelFrontend output : row-major [n_mels, T_mel] f32
//   ggml mel_in        : ne=[T_mel, n_mels, 1, 1] f32 (byte-identical)
//   conv input         : [W=F, H=T, IC, N] per ggml_conv_2d; OIHW catalog
//                        kernels map to ggml ne [KW, KH, IC, OC] with KW
//                        on the FREQ axis and KH on the TIME axis (the 3x3
//                        kernel isn't symmetric, so W=F, H=T is required).
// After 3 stride-2 k=3 pad=1 convs, W and H shrink floor((d+2-3)/2)+1
// per layer (n_mels=128: F 128→64→32→16); post-conv ne =
// [F', T_enc, channels, 1].

#include "encoder.h"

#include "conformer/conformer.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-flash-policy.h"
#include "transcribe-log.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

namespace transcribe::parakeet {

namespace {

namespace conf = transcribe::conformer;

// ----- Per-family depthwise dispatch policy -----
//
// In-block depthwise conv uses the direct conv_2d_dw path on every
// backend (faster than the 31x im2col expansion). Env vars override
// both sites.
bool detect_direct_dw_in_block(const char * backend) {
    (void) backend;
    return conf::resolve_conv_direct("TRANSCRIBE_CONV_DIRECT_DW", "TRANSCRIBE_CONV_NO_DIRECT_DW",
                                     /*backend_default=*/true);
}

// Pre-encode depthwise dispatch: direct path EXCEPT on Metal, where
// CONV_2D_DW is unsupported in this vendored ggml (see conv_2d_dw_f32 in
// conformer.cpp) so the im2col helper is used. CPU/Vulkan have native
// CONV_2D_DW; the direct path avoids the ~31x im2col expansion and the
// degenerate per-channel [1 x K] matmul (~40-50ms here).
bool detect_direct_dw_in_pre_encode(const char * backend) {
    const bool is_metal =
        backend != nullptr && (std::strstr(backend, "Metal") != nullptr || std::strstr(backend, "metal") != nullptr);
    return conf::resolve_conv_direct("TRANSCRIBE_CONV_DIRECT_DW", "TRANSCRIBE_CONV_NO_DIRECT_DW",
                                     /*backend_default=*/!is_metal);
}

// ----- Views ------------------------------------------------------

conf::PreEncodeView to_view(const ParakeetPreEncode & pe) {
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

conf::BlockView to_view(const ParakeetBlock & b) {
    conf::BlockView v;

    // FF1.
    v.norm_ff1_w = b.norm_ff1_w;
    v.norm_ff1_b = b.norm_ff1_b;
    v.ff1_lin1_w = b.ff1_lin1_w;
    v.ff1_lin1_b = b.ff1_lin1_b;  // null when use_bias=false
    v.ff1_lin2_w = b.ff1_lin2_w;
    v.ff1_lin2_b = b.ff1_lin2_b;

    // Self-attention. Q/K/V/out biases only when use_bias=true; linear_pos
    // is bias-free in NeMo regardless.
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

    // Conv module.
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
    // LayerNorm path (streaming variants): the GGUF stores the affine
    // scale/bias under the same conv.bn.weight/bias names. The conformer
    // block reads these only when conv_norm_type == LayerNorm.
    v.conv_ln_w           = b.conv_bn_w;
    v.conv_ln_b           = b.conv_bn_b;

    // FF2.
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

// ----- Block 0 observer -----
//
// For each canonical point build_conformer_block fires on, names the
// tensor `enc.block.0.<tag>` and stashes it in the matching EncoderDumps
// slot for Parakeet::run's try_dump loop. The dump FILE names in
// model.cpp (enc.block.0.ff1, ...) are independent of these ggml names
// (enc.block.0.after_ff1, ...).
struct Block0DumpSink {
    EncoderDumps * dumps;
};

void parakeet_block0_observer_cb(void * user, const char * tag, ggml_tensor * t) {
    auto * sink = static_cast<Block0DumpSink *>(user);
    if (sink == nullptr || sink->dumps == nullptr || t == nullptr) {
        return;
    }

    char name_buf[64];
    std::snprintf(name_buf, sizeof(name_buf), "enc.block.0.%s", tag);
    ggml_set_name(t, name_buf);

    if (std::strcmp(tag, "after_ff1") == 0) {
        sink->dumps->block0_after_ff1 = t;
        transcribe::debug::mark_tensor_for_dump(t);
    } else if (std::strcmp(tag, "after_attn") == 0) {
        sink->dumps->block0_after_attn = t;
        transcribe::debug::mark_tensor_for_dump(t);
    } else if (std::strcmp(tag, "after_conv") == 0) {
        sink->dumps->block0_after_conv = t;
        transcribe::debug::mark_tensor_for_dump(t);
    } else if (std::strcmp(tag, "after_ff2") == 0) {
        sink->dumps->block0_after_ff2 = t;
        transcribe::debug::mark_tensor_for_dump(t);
    } else if (std::strcmp(tag, "out") == 0) {
        sink->dumps->block0_out = t;
        transcribe::debug::mark_tensor_for_dump(t);
    }
}

// Sub-block observer for blocks 1..N-1, gated by
// TRANSCRIBE_DUMP_SUB_BLOCKS. Names tensors
// `enc.block.<i>.{ff1,attn,conv,ff2}` (the helper's "out" tag is already
// covered by the mid/last/all-block paths).
struct SubBlockSink {
    int            block_idx;
    EncoderDumps * dumps;
};

void parakeet_subblock_observer_cb(void * user, const char * tag, ggml_tensor * t) {
    auto * sink = static_cast<SubBlockSink *>(user);
    if (sink == nullptr || sink->dumps == nullptr || t == nullptr) {
        return;
    }

    const char * short_tag = tag;
    if (std::strcmp(tag, "after_ff1") == 0) {
        short_tag = "ff1";
    } else if (std::strcmp(tag, "after_attn") == 0) {
        short_tag = "attn";
    } else if (std::strcmp(tag, "after_conv") == 0) {
        short_tag = "conv";
    } else if (std::strcmp(tag, "after_ff2") == 0) {
        short_tag = "ff2";
    } else if (std::strcmp(tag, "out") == 0) {
        return;  // block.<i>.out covered elsewhere
    }

    char name_buf[64];
    std::snprintf(name_buf, sizeof(name_buf), "enc.block.%d.%s", sink->block_idx, short_tag);
    ggml_set_name(t, name_buf);
    transcribe::debug::mark_tensor_for_dump(t);
    sink->dumps->sub_block_dumps.emplace_back(std::string(name_buf), t);
}

// Parse "0,12,22,23" into a sorted unique vector of block indices.
// Returns empty if the env var is unset or empty.
std::vector<int> parse_sub_block_env(int n_layers) {
    std::vector<int> out;
    const char *     raw = transcribe::debug::dump_sub_blocks_spec();
    if (raw == nullptr || *raw == '\0') {
        return out;
    }
    const char * p = raw;
    while (*p != '\0') {
        // Skip leading separators.
        while (*p == ',' || *p == ' ' || *p == '\t') {
            ++p;
        }
        if (*p == '\0') {
            break;
        }
        char * end = nullptr;
        long   v   = std::strtol(p, &end, 10);
        if (end == p) {
            break;  // no progress — malformed
        }
        if (v >= 0 && v < n_layers) {
            out.push_back(static_cast<int>(v));
        }
        p = end;
    }
    std::sort(out.begin(), out.end());
    out.erase(std::unique(out.begin(), out.end()), out.end());
    return out;
}

}  // namespace

EncoderBuild build_encoder_graph(ggml_context *                     ctx,
                                 const ParakeetWeights &            w,
                                 const ParakeetHParams &            hp,
                                 int                                n_mel_frames,
                                 ggml_type                          kv_type,
                                 const char *                       backend_name,
                                 const BufferedStreamMaskOverride * buf_mask,
                                 int                                n_batch,
                                 bool                               batch_var_len) {
    if (n_batch < 1) {
        n_batch = 1;
    }
    const bool       var_len_masks = batch_var_len && n_batch > 1;
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    policy.direct_dw_in_block      = detect_direct_dw_in_block(backend_name);
    policy.direct_dw_in_pre_encode = detect_direct_dw_in_pre_encode(backend_name);
    // Cache-aware streaming (NeMo causal_downsampling=true) uses
    // CausalConv2D for the pre-encode subsample (left=k-1, right=stride-1).
    // Inferred from the attention style — only ChunkedLimited is causal.
    // Independent of the conformer conv-module's conv_context.
    policy.causal_pre_encode       = (hp.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);

    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet encoder: invalid arg "
                "(ctx=%p, n_mel_frames=%d)",
                static_cast<void *>(ctx), n_mel_frames);
        return eb;
    }

    // Only subsampling_factor=8 (every published variant) is implemented;
    // a different factor would need a build_pre_encode rework.
    if (hp.enc_subsampling_factor != 8) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet encoder: unsupported subsampling_factor=%d "
                "(only 8 implemented)",
                hp.enc_subsampling_factor);
        return eb;
    }
    if (hp.fe_num_mels <= 0 || (hp.fe_num_mels % hp.enc_subsampling_factor) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet encoder: n_mels=%d not divisible by "
                "subsampling_factor=%d",
                hp.fe_num_mels, hp.enc_subsampling_factor);
        return eb;
    }

    // Mel input handle. ne=[T_mel, n_mels, 1, n_batch] matches the
    // MelFrontend's row-major [n_mels, n_frames] byte for byte; utterance
    // b is the contiguous slab at offset b * n_mel_frames * n_mels.
    eb.mel_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, n_mel_frames, hp.fe_num_mels, 1, n_batch);
    if (eb.mel_in == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet encoder: failed to allocate mel_in tensor");
        return eb;
    }
    ggml_set_name(eb.mel_in, "mel.in");
    ggml_set_input(eb.mel_in);
    // Pin mel_in so the scheduler doesn't reuse its slot before the dump
    // pass reads it (no-op in normal builds).
    transcribe::debug::mark_tensor_for_dump(eb.mel_in);

    // Build the pre_encode subgraph. Variable-length offline batching
    // enables masked subsampling (zero each conv intermediate's padded
    // time region). Only the non-causal pre-encode is masked.
    conf::PreEncodeValidMasks pe_masks;
    const bool                mask_pre_encode = var_len_masks && !policy.causal_pre_encode;
    ggml_tensor * x = conf::build_pre_encode(ctx, to_view(w.pre_encode), eb.mel_in, policy,
                                             /*name_prefix=*/"enc.pre_encode",
                                             /*error_tag=*/"parakeet", mask_pre_encode ? &pe_masks : nullptr);
    if (mask_pre_encode) {
        eb.pre_encode_mask_s1_in = pe_masks.mask_s1;
        eb.pre_encode_mask_s2_in = pe_masks.mask_s2;
        eb.pre_encode_mask_s3_in = pe_masks.mask_s3;
    }
    if (x == nullptr) {
        // build_pre_encode already logged the diagnostic.
        return eb;
    }
    eb.dumps.pre_encode_out = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ----- xscaling -----
    //
    // NeMo's RelPositionalEncoding applies x = x * sqrt(d_model) when
    // xscaling=True, on the running residual after pre_encode and before
    // block 0 (ctc/rnnt/unified-en are True; v2/v3/tdt are False).
    // Sub-block LayerNorms normalize the scale away, so it only matters
    // for the residual mix. Omitting it on a True model accumulates drift
    // past 24 blocks — empty transcripts on unified-en, ~0.04% WER drift
    // on ctc/rnnt.
    if (hp.enc_xscaling) {
        const float scale = std::sqrt(static_cast<float>(hp.enc_d_model));
        x                 = ggml_scale(ctx, x, scale);
        x                 = conf::named(x, "enc.pre_encode.xscaled");
    }

    // ----- Conformer blocks -----
    //
    // Every block goes through conf::build_conformer_block. Block 0 passes
    // a dump observer; mid/last blocks get their outputs named after
    // return (the loop is otherwise plain so the dump dir doesn't fill).
    if (!w.blocks.empty()) {
        // After pre_encode, x ne = [d_model, T_enc, 1, 1].
        const int64_t T_enc = x->ne[1];

        // pos_emb input, ne = [d_model, pos_len, 1, 1] (numpy
        // (pos_len, d_model)), filled by the driver. Local attention
        // (Regular, both contexts >= 0) shortens pos_len to (left+right+1)
        // (NeMo LocalAttRelPositionalEncoding); ChunkedLimited keeps the
        // full 2T-1 and uses a separate mask. ChunkedLimitedWithRc engages
        // the mask only when buf_mask is non-null (offline runs full attn).
        const bool is_chunked =
            (hp.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited) ||
            (hp.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc && buf_mask != nullptr);
        const bool    is_local_pe = (!is_chunked) && (hp.enc_att_context_left >= 0 && hp.enc_att_context_right >= 0);
        const int64_t pos_len     = is_local_pe ?
                                        static_cast<int64_t>(hp.enc_att_context_left + hp.enc_att_context_right + 1) :
                                        (2 * T_enc - 1);
        eb.pos_emb_in             = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, pos_len);
        ggml_set_name(eb.pos_emb_in, "pos_emb.in");
        ggml_set_input(eb.pos_emb_in);

        // ChunkedLimited mask. [T_enc, T_enc, 1, 1] F32; the driver
        // fills it host-side after the compute buffer is allocated.
        // Broadcasts across heads inside rel_pos_mhsa.
        ggml_tensor * chunked_mask_in = nullptr;
        if (is_chunked) {
            chunked_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, T_enc, 1, 1);
            ggml_set_name(chunked_mask_in, "attn.chunked_mask.in");
            ggml_set_input(chunked_mask_in);
            eb.chunked_mask_in = chunked_mask_in;
        }

        ggml_tensor * conv_pad_mask_in = nullptr;
        if (buf_mask != nullptr && buf_mask->valid_frames >= 0 && buf_mask->valid_frames < T_enc) {
            conv_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, 1, 1);
            ggml_set_name(conv_pad_mask_in, "conv.pad_mask.in");
            ggml_set_input(conv_pad_mask_in);
            eb.conv_pad_mask_in = conv_pad_mask_in;
        }

        // Variable-length batch masks. When batching utterances of differing
        // length we pad every mel to a common T_max; these two masks keep
        // each utterance's padded tail from corrupting its real frames:
        //   - attn_pad_mask_in [T_enc, 1, 1, n_batch]: additive (-INF on
        //     padded keys) so real queries never attend to padded keys.
        //   - conv_pad_mask_in [T_enc, 1, n_batch, 1]: 0/1 valid-frame mask
        //     that zeroes padded frames after pw1+GLU and before the
        //     depthwise conv (same role as the buffered-streaming mask, but
        //     per utterance). Reuses the conv_pad_mask_in field since the
        //     buffered path is mutually exclusive with offline batching.
        ggml_tensor * attn_pad_mask_in = nullptr;
        if (var_len_masks) {
            attn_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, 1, n_batch);
            ggml_set_name(attn_pad_mask_in, "attn.pad_mask.in");
            ggml_set_input(attn_pad_mask_in);
            eb.attn_pad_mask_in = attn_pad_mask_in;

            conv_pad_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_enc, 1, n_batch, 1);
            ggml_set_name(conv_pad_mask_in, "conv.pad_mask.batch.in");
            ggml_set_input(conv_pad_mask_in);
            eb.conv_pad_mask_in = conv_pad_mask_in;
        }

        conf::BlockParams bparams{};
        bparams.d_model     = hp.enc_d_model;
        bparams.n_head      = hp.enc_n_heads;
        bparams.conv_kernel = hp.enc_conv_kernel;
        bparams.kv_type     = kv_type;
        // Flash attention by default. Flash batches correctly (the batched
        // encoder output is bit-identical to single-shot flash), so the batch
        // axis does not change the path. TRANSCRIBE_NO_FLASH forces the manual
        // F32 mul_mat + soft_max path — used by the bit-exact CPU tensor gate
        // (flash casts the rel-pos mask to F16, manual stays F32);
        // TRANSCRIBE_FORCE_FLASH forces it back on. Parakeet has no attention
        // decoder, so the decoder flag is a throwaway.
        bool enc_use_flash = true, dec_use_flash = false;
        transcribe::flash::apply_env_overrides(enc_use_flash, dec_use_flash);
        bparams.use_flash          = enc_use_flash;
        bparams.policy             = policy;
        bparams.att_context_left   = hp.enc_att_context_left;
        bparams.att_context_right  = hp.enc_att_context_right;
        bparams.att_context_style  = is_chunked ? conf::BlockParams::AttContextStyle::ChunkedLimited :
                                                  conf::BlockParams::AttContextStyle::Regular;
        bparams.attn_chunked_mask  = chunked_mask_in;
        bparams.attn_pad_mask      = attn_pad_mask_in;
        bparams.conv_context_left  = hp.enc_conv_context_left;
        bparams.conv_context_right = hp.enc_conv_context_right;
        bparams.conv_pad_mask      = conv_pad_mask_in;
        bparams.conv_norm_type     = (hp.enc_conv_norm_type == ParakeetHParams::ConvNormType::LayerNorm) ?
                                         conf::BlockParams::ConvNormType::LayerNorm :
                                         conf::BlockParams::ConvNormType::BatchNorm;

        // Block 0: shared helper + the dump observer.
        {
            Block0DumpSink      sink{ &eb.dumps };
            conf::BlockObserver obs{};
            obs.on_point = parakeet_block0_observer_cb;
            obs.user     = &sink;
            x            = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[0]), bparams, &obs);
        }

        // Blocks 1..N-1. Mark the mid (n/2) and last (n-1) outputs for
        // dump (0/12/23 on 24-layer v2/v3); the file name is synthesized
        // in model.cpp from the saved index because conf::named's trailing
        // "enc.final" rename would otherwise clobber the last-block name.
        const int n_layers      = static_cast<int>(w.blocks.size());
        const int mid_layer     = n_layers / 2;
        const int last_layer    = n_layers - 1;
        eb.dumps.mid_block_idx  = mid_layer;
        eb.dumps.last_block_idx = last_layer;
        eb.dumps.all_block_outs.assign(n_layers, nullptr);
        eb.dumps.all_block_outs[0] = eb.dumps.block0_out;
        // Per-block dump opt-in for the divergence bisect
        // (TRANSCRIBE_DUMP_ALL_BLOCKS).
        const bool dump_all_blocks = transcribe::debug::dump_all_blocks_requested();
        if (dump_all_blocks && eb.dumps.block0_out != nullptr) {
            transcribe::debug::mark_tensor_for_dump(eb.dumps.block0_out);
        }
        // Sub-block observation gate (TRANSCRIBE_DUMP_SUB_BLOCKS). Sinks
        // are kept alive for the build (the observer captures their addr).
        const std::vector<int>    sub_blocks = parse_sub_block_env(n_layers);
        std::vector<SubBlockSink> sub_sinks;
        sub_sinks.reserve(sub_blocks.size());
        for (int idx : sub_blocks) {
            sub_sinks.push_back(SubBlockSink{ idx, &eb.dumps });
        }
        for (int i = 1; i < n_layers; ++i) {
            const auto it       = std::lower_bound(sub_blocks.begin(), sub_blocks.end(), i);
            const bool with_obs = (it != sub_blocks.end() && *it == i);
            if (with_obs) {
                const size_t        sink_idx = static_cast<size_t>(it - sub_blocks.begin());
                conf::BlockObserver obs{};
                obs.on_point = parakeet_subblock_observer_cb;
                obs.user     = &sub_sinks[sink_idx];
                x            = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[i]), bparams, &obs);
            } else {
                x = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[i]), bparams);
            }
            eb.dumps.all_block_outs[i] = x;
            if (dump_all_blocks) {
                transcribe::debug::mark_tensor_for_dump(x);
            }
            if (i == mid_layer) {
                eb.dumps.mid_block_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
            if (i == last_layer) {
                eb.dumps.last_block_out = x;
                transcribe::debug::mark_tensor_for_dump(x);
            }
        }
    }

    // Final encoder output (= last block's norm_out).
    eb.dumps.final_out = x;
    x                  = conf::named(x, "enc.final");
    transcribe::debug::mark_tensor_for_dump(x);
    // Keep final_out as an output so the host can read it back separately
    // from the prompted output (else the scheduler may free it once eb.out
    // is realised, racing the "dec.enc_out" dump).
    if (hp.has_prompt) {
        ggml_set_output(x);
    }

    // Prompt MLP (multilingual variants). Mirrors NeMo's
    // EncDecRNNTBPEModelWithPrompt.forward():
    //   x_cat = concat(enc[d_model], one_hot(prompt_id)[num_prompts])
    //   h     = relu(W0 @ x_cat + b0)   // W0: [prompt_hidden, d_model+P]
    //   y     = W2 @ h + b2             // W2: [d_model, prompt_hidden]
    // The one-hot is replicated across T_enc on the host so the in-graph
    // step is concat + two matmuls + ReLU; y replaces eb.out.
    if (hp.has_prompt) {
        const int T_enc_val = static_cast<int>(x->ne[1]);
        const int P         = hp.prompt_num_prompts;
        const int B         = n_batch;

        // Per-utterance one-hot, ne = [num_prompts, T_enc, n_batch, 1]
        // (batch on ne[2] to match the block output layout).
        ggml_tensor * one_hot = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, P, T_enc_val, B, 1);
        if (one_hot == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet encoder: failed to allocate "
                    "prompt.one_hot.in tensor");
            return eb;
        }
        ggml_set_name(one_hot, "prompt.one_hot.in");
        ggml_set_input(one_hot);
        eb.prompt_one_hot_in = one_hot;

        ggml_tensor * cat = ggml_concat(ctx, x, one_hot, /*dim=*/0);
        ggml_tensor * h   = ggml_mul_mat(ctx, w.prompt.mlp0_w, cat);
        h                 = ggml_add(ctx, h, w.prompt.mlp0_b);
        h                 = ggml_relu(ctx, h);
        ggml_tensor * y   = ggml_mul_mat(ctx, w.prompt.mlp2_w, h);
        y                 = ggml_add(ctx, y, w.prompt.mlp2_b);

        eb.dumps.prompted_out = y;
        y                     = conf::named(y, "enc.prompted");
        transcribe::debug::mark_tensor_for_dump(y);

        x = y;
    }

    eb.out = x;
    ggml_set_output(eb.out);

    // 8192-node cgraph: the default 2048 is too small (each conformer
    // block contributes ~90 ops, 24 blocks > 2200).
    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet encoder: ggml_new_graph_custom failed");
        return eb;
    }
    ggml_build_forward_expand(eb.graph, eb.out);
    // With the prompt MLP present, final_out is otherwise an unconnected
    // node; add it as a forward root so it stays materialised for the
    // "dec.enc_out" dump.
    if (hp.has_prompt && eb.dumps.final_out != nullptr) {
        ggml_build_forward_expand(eb.graph, eb.dumps.final_out);
    }

    return eb;
}

// ---------------------------------------------------------------------------
// Streaming encoder graph (cache-aware, per-chunk feed)
// ---------------------------------------------------------------------------
//
// Like build_encoder_graph but: operates on a mel chunk; slices off
// drop_extra_pre_encoded post-subsample frames; threads per-layer
// cache_last_channel/time through BlockParams and emits fresh cache
// outputs the driver reads back; sizes pos_emb and chunked_mask for
// T_virtual = T_cache + T_q_new.
//
// Shapes:
//   eb.mel_in            [n_mel_chunk_frames, n_mels, 1, 1]
//   eb.pos_emb_in        [d_model, 2*T_virtual - 1, 1, 1]
//   eb.chunked_mask_in   [T_virtual, T_virtual, 1, 1]
//   eb.out               [d_model, T_q_new, 1, 1]
//   cache_io.channel_out[i]  [d_model, T_cache, 1, 1]   (fresh)
//   cache_io.time_out[i]     [k_minus_1, d_model, 1, 1] (fresh)
//
// cache_io.channel_in / time_in must come from the persistent cache
// buffer with the shapes above (the scheduler treats them as inputs).
EncoderBuild build_encoder_graph_streaming(ggml_context *            ctx,
                                           const ParakeetWeights &   w,
                                           const ParakeetHParams &   hp,
                                           int                       n_mel_chunk_frames,
                                           int                       drop_extra_pre_encoded,
                                           StreamingEncoderCacheIO & cache_io,
                                           ggml_type                 kv_type,
                                           const char *              backend_name) {
    conf::ConvPolicy policy{};
    policy.direct_pw               = conf::detect_direct_pw(backend_name);
    policy.direct_dw_in_block      = detect_direct_dw_in_block(backend_name);
    policy.direct_dw_in_pre_encode = detect_direct_dw_in_pre_encode(backend_name);
    // Causal pre-encode is the cache-aware streaming convention only.
    policy.causal_pre_encode       = (hp.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);

    EncoderBuild eb{};

    if (ctx == nullptr || n_mel_chunk_frames <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet streaming encoder: invalid arg "
                "(ctx=%p, n_mel_chunk_frames=%d)",
                static_cast<void *>(ctx), n_mel_chunk_frames);
        return eb;
    }

    if (hp.enc_subsampling_factor != 8) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet streaming encoder: unsupported "
                "subsampling_factor=%d (only 8 implemented)",
                hp.enc_subsampling_factor);
        return eb;
    }
    if (hp.enc_att_context_style != ParakeetHParams::AttContextStyle::ChunkedLimited) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet streaming encoder: requires "
                "att_context_style=ChunkedLimited");
        return eb;
    }

    const int n_layers = static_cast<int>(w.blocks.size());
    if (static_cast<int>(cache_io.channel_in.size()) != n_layers ||
        static_cast<int>(cache_io.time_in.size()) != n_layers) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet streaming encoder: cache_io.in vectors "
                "must be sized to n_layers=%d (got channel=%zu, "
                "time=%zu)",
                n_layers, cache_io.channel_in.size(), cache_io.time_in.size());
        return eb;
    }

    // ----- Mel chunk input -----
    eb.mel_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, n_mel_chunk_frames, hp.fe_num_mels);
    ggml_set_name(eb.mel_in, "stream.mel.in");
    ggml_set_input(eb.mel_in);

    // ----- Pre-encode + (optional) xscaling -----
    ggml_tensor * x = conf::build_pre_encode(ctx, to_view(w.pre_encode), eb.mel_in, policy,
                                             /*name_prefix=*/"stream.enc.pre_encode",
                                             /*error_tag=*/"parakeet stream");
    if (x == nullptr) {
        return eb;
    }

    if (hp.enc_xscaling) {
        const float scale = std::sqrt(static_cast<float>(hp.enc_d_model));
        x                 = ggml_scale(ctx, x, scale);
    }

    // x ne = [d_model, T_pre_enc]. Slice off the first
    // drop_extra_pre_encoded frames (NeMo's mechanism to handle the
    // 8x-subsample overlap after the mel-history prepend).
    if (drop_extra_pre_encoded > 0) {
        const int64_t T_pre = x->ne[1];
        if (drop_extra_pre_encoded >= T_pre) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet streaming encoder: drop_extra=%d >= "
                    "T_pre_enc=%lld",
                    drop_extra_pre_encoded, (long long) T_pre);
            return eb;
        }
        x = ggml_view_2d(ctx, x, x->ne[0], T_pre - drop_extra_pre_encoded, x->nb[1], drop_extra_pre_encoded * x->nb[1]);
        x = ggml_cont(ctx, x);
    }

    const int64_t T_q_new = x->ne[1];
    const int64_t T_cache = cache_io.channel_in[0]->ne[1];
    const int64_t T_virt  = T_cache + T_q_new;

    // Query slicing: attention queries cover only the T_q_new new frames
    // while K/V span the full virtual window, so pos_emb and the mask are
    // sized for the rectangular [T_virt, T_q_new] geometry.

    // Streaming KV cache: consume per-layer pre-projected K/V instead of
    // recomputing from the channel cache. Disabled under the dump harness
    // (which compares last_channel against NeMo); the channel-recompute
    // path is bit-identical and produces the last_channel the gate needs.
    const bool kv_cache_mode = !transcribe::debug::enabled() && static_cast<int>(cache_io.k_in.size()) == n_layers &&
                               static_cast<int>(cache_io.v_in.size()) == n_layers;

    // ----- Pos_emb (rectangular: pos_len = T_q + T_kv - 1) -----
    const int64_t pos_len = T_virt + T_q_new - 1;
    // Rel-pos projection memoization: when the driver's cache matches
    // this chunk's pos_len, every block consumes its precomputed
    // projection and the graph needs no pos_emb input at all.
    const bool    pos_proj_hit =
        cache_io.pos_proj_len == pos_len && static_cast<int>(cache_io.pos_proj.size()) == n_layers;
    if (!pos_proj_hit) {
        eb.pos_emb_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, pos_len);
        ggml_set_name(eb.pos_emb_in, "stream.pos_emb.in");
        ggml_set_input(eb.pos_emb_in);
    }

    // Chunked mask: [T_virtual, T_q_new] (keys x queries). Built host-side.
    eb.chunked_mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, T_virt, T_q_new, 1, 1);
    ggml_set_name(eb.chunked_mask_in, "stream.attn.chunked_mask.in");
    ggml_set_input(eb.chunked_mask_in);

    // Create the graph BEFORE the cache_out tensors so the helpers can
    // forward-expand their cpy nodes onto it.
    eb.graph = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (eb.graph == nullptr) {
        return eb;
    }

    conf::BlockParams bparams{};
    bparams.d_model     = hp.enc_d_model;
    bparams.n_head      = hp.enc_n_heads;
    bparams.conv_kernel = hp.enc_conv_kernel;
    bparams.kv_type     = kv_type;
    // Flash-attention opt-out (TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH).
    // On CPU at streaming geometry the flash kernel is dramatically slower
    // (~33 ms/layer vs ~1 ms on a Cortex-A55). Parakeet has no attention
    // decoder, so the decoder flag is a throwaway.
    bool enc_use_flash = true, dec_use_flash = false;
    transcribe::flash::apply_env_overrides(enc_use_flash, dec_use_flash);
    bparams.use_flash          = enc_use_flash;
    bparams.policy             = policy;
    // No att_context_left/right: ChunkedLimited derives the band entirely
    // from attn_chunked_mask (build_conformer_block reads them only on the
    // Regular path, never taken here).
    bparams.att_context_style  = conf::BlockParams::AttContextStyle::ChunkedLimited;
    bparams.attn_chunked_mask  = eb.chunked_mask_in;
    bparams.conv_context_left  = hp.enc_conv_context_left;
    bparams.conv_context_right = hp.enc_conv_context_right;
    bparams.conv_norm_type     = (hp.enc_conv_norm_type == ParakeetHParams::ConvNormType::LayerNorm) ?
                                     conf::BlockParams::ConvNormType::LayerNorm :
                                     conf::BlockParams::ConvNormType::BatchNorm;
    bparams.streaming_graph    = eb.graph;
    bparams.streaming_T_q_new  = static_cast<int>(T_q_new);

    cache_io.channel_out.assign(n_layers, nullptr);
    cache_io.time_out.assign(n_layers, nullptr);
    const int k_minus_1 = static_cast<int>(cache_io.time_in[0]->ne[0]);

    if (kv_cache_mode) {
        cache_io.k_out.assign(n_layers, nullptr);
        cache_io.v_out.assign(n_layers, nullptr);
    }
    for (int i = 0; i < n_layers; ++i) {
        // Per-layer cache I/O: "in" from the persistent buffer, "out"
        // fresh in compute_ctx (filled via ggml_cpy, read after compute).
        // KV mode leaves channel_out null.
        ggml_tensor * cache_tm_out = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, k_minus_1, hp.enc_d_model);
        if (cache_tm_out == nullptr) {
            return eb;
        }
        char nm[64];
        std::snprintf(nm, sizeof(nm), "stream.cache_tm_out.%d", i);
        ggml_set_name(cache_tm_out, nm);
        ggml_set_output(cache_tm_out);

        ggml_tensor * cache_ch_out = nullptr;
        ggml_tensor * cache_k_out  = nullptr;
        ggml_tensor * cache_v_out  = nullptr;
        if (kv_cache_mode) {
            cache_k_out = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, T_cache);
            cache_v_out = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, T_cache);
            if (cache_k_out == nullptr || cache_v_out == nullptr) {
                return eb;
            }
            std::snprintf(nm, sizeof(nm), "stream.cache_k_out.%d", i);
            ggml_set_name(cache_k_out, nm);
            std::snprintf(nm, sizeof(nm), "stream.cache_v_out.%d", i);
            ggml_set_name(cache_v_out, nm);
            ggml_set_output(cache_k_out);
            ggml_set_output(cache_v_out);
        } else {
            cache_ch_out = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hp.enc_d_model, T_cache);
            if (cache_ch_out == nullptr) {
                return eb;
            }
            std::snprintf(nm, sizeof(nm), "stream.cache_ch_out.%d", i);
            ggml_set_name(cache_ch_out, nm);
            ggml_set_output(cache_ch_out);
        }

        bparams.streaming_channel_in  = cache_io.channel_in[i];
        bparams.streaming_channel_out = cache_ch_out;
        bparams.streaming_time_in     = cache_io.time_in[i];
        bparams.streaming_time_out    = cache_tm_out;
        bparams.streaming_pos_proj_in = pos_proj_hit ? cache_io.pos_proj[i] : nullptr;
        bparams.streaming_kv_k_in     = kv_cache_mode ? cache_io.k_in[i] : nullptr;
        bparams.streaming_kv_v_in     = kv_cache_mode ? cache_io.v_in[i] : nullptr;
        bparams.streaming_kv_k_out    = cache_k_out;
        bparams.streaming_kv_v_out    = cache_v_out;

        x = conf::build_conformer_block(ctx, x, eb.pos_emb_in, to_view(w.blocks[i]), bparams,
                                        /*obs=*/nullptr);

        cache_io.channel_out[i] = cache_ch_out;
        cache_io.time_out[i]    = cache_tm_out;
        if (kv_cache_mode) {
            cache_io.k_out[i] = cache_k_out;
            cache_io.v_out[i] = cache_v_out;
        }
    }

    // Tag the streaming per-chunk encoder output so the host-side dump
    // sees the same name as the offline path.
    eb.dumps.final_out = x;
    x                  = conf::named(x, "enc.final");
    transcribe::debug::mark_tensor_for_dump(x);
    if (hp.has_prompt) {
        ggml_set_output(x);
    }

    // Prompt MLP on the per-chunk output (multilingual streaming
    // variants). Same forward as the offline path; only the T axis
    // size differs (T_q_new instead of full T_enc).
    if (hp.has_prompt) {
        const int T_q = static_cast<int>(x->ne[1]);
        const int P   = hp.prompt_num_prompts;

        ggml_tensor * one_hot = ggml_new_tensor_4d(ctx, GGML_TYPE_F32, P, T_q, 1, 1);
        if (one_hot == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet streaming encoder: failed to allocate "
                    "prompt.one_hot.in tensor");
            return eb;
        }
        ggml_set_name(one_hot, "prompt.one_hot.in");
        ggml_set_input(one_hot);
        eb.prompt_one_hot_in = one_hot;

        ggml_tensor * cat = ggml_concat(ctx, x, one_hot, /*dim=*/0);
        ggml_tensor * h   = ggml_mul_mat(ctx, w.prompt.mlp0_w, cat);
        h                 = ggml_add(ctx, h, w.prompt.mlp0_b);
        h                 = ggml_relu(ctx, h);
        ggml_tensor * y   = ggml_mul_mat(ctx, w.prompt.mlp2_w, h);
        y                 = ggml_add(ctx, y, w.prompt.mlp2_b);

        eb.dumps.prompted_out = y;
        y                     = conf::named(y, "enc.prompted");
        transcribe::debug::mark_tensor_for_dump(y);
        x = y;
    }

    eb.out = x;
    ggml_set_output(eb.out);
    ggml_build_forward_expand(eb.graph, eb.out);
    if (hp.has_prompt && eb.dumps.final_out != nullptr) {
        ggml_build_forward_expand(eb.graph, eb.dumps.final_out);
    }
    return eb;
}

}  // namespace transcribe::parakeet
