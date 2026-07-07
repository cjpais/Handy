// arch/medasr/encoder.h - MedASR (LASR-CTC) encoder graph builder.

#pragma once

#include "ggml.h"
#include "weights.h"

#include <vector>

namespace transcribe::medasr {

struct EncoderDumps {
    // Subsampling output (= reference enc.subsampling.out).
    ggml_tensor * subsampling_out = nullptr;

    // Subsampling sub-step probes (used by the CUDA-quant diagnostic).
    ggml_tensor * sub_after_dense0 = nullptr;
    ggml_tensor * sub_after_conv0  = nullptr;
    ggml_tensor * sub_after_conv1  = nullptr;

    // Block 0 sub-steps (= reference enc.block.0.post_{ff1,attn,conv,ff2}).
    ggml_tensor * block0_post_ff1  = nullptr;
    ggml_tensor * block0_post_attn = nullptr;
    ggml_tensor * block0_post_conv = nullptr;
    ggml_tensor * block0_post_ff2  = nullptr;

    // Per-block outputs (every block's norm_out result). The reference
    // dump catalog covers indices 0/7/8/16 for the 17-layer variant; we
    // keep handles for all of them so model.cpp can dispatch any subset.
    std::vector<ggml_tensor *> all_block_outs;

    // Top-level encoder out_norm output (= reference enc.out_norm.out).
    ggml_tensor * out_norm_out = nullptr;

    // CTC head logits in "decode orientation" (ne=[vocab, T_enc, 1, B]).
    // The decode loop reads this tensor's bytes directly: byte stream is
    // [T_enc rows, vocab inner] in numpy terms, so per-frame argmax over
    // `vocab` consecutive floats Just Works.
    ggml_tensor * ctc_logits = nullptr;

    // CTC head logits in "reference-dump orientation" (ne=[T_enc, vocab, 1, B]).
    // The reference dumper writes numpy shape [vocab, T_enc] (Conv1d output
    // pre-transpose); ggml ne=[T_enc, vocab] is byte-equivalent. Only used
    // by the dump pass — never read for decode.
    ggml_tensor * ctc_logits_for_dump = nullptr;
};

struct EncoderBuild {
    // Inputs.
    ggml_tensor * mel_in    = nullptr;  // [T_mel, n_mels, 1, B]
    ggml_tensor * positions = nullptr;  // [T_enc] i32

    // Per-layer fused-BN scale/bias inputs (allocated in the same ctx as
    // the graph; the driver uploads from MedAsrWeights::fused_bn_*_storage
    // after sched_alloc_graph). One pair per encoder layer.
    std::vector<ggml_tensor *> bn_scale_inputs;  // each [d_model]
    std::vector<ggml_tensor *> bn_bias_inputs;   // each [d_model]

    // Optional variable-length batch masks (one or both null in single-shot
    // or same-length batch mode). Sized to per-encoder-frame T_enc.
    ggml_tensor * attn_pad_mask_in = nullptr;  // [T_enc, 1, 1, B] f32 add (-INF/0)
    ggml_tensor * conv_pad_mask_in = nullptr;  // [T_enc, 1, B, 1] f32 (0/1)

    // Output.
    ggml_tensor * out = nullptr;  // == ctc_logits

    ggml_cgraph * graph = nullptr;

    EncoderDumps dumps;
};

EncoderBuild build_encoder_graph(ggml_context *        ctx,
                                 const MedAsrWeights & w,
                                 const MedAsrHParams & hp,
                                 int                   n_mel_frames,
                                 const char *          backend_name,
                                 int                   n_batch       = 1,
                                 bool                  batch_var_len = false);

}  // namespace transcribe::medasr
