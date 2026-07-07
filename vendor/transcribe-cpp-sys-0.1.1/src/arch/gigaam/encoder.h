// arch/gigaam/encoder.h - GigaAM Conformer encoder graph builder.

#pragma once

#include "ggml.h"

#include <string>
#include <utility>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::gigaam {

struct GigaamHParams;
struct GigaamWeights;

struct EncoderDumps {
    ggml_tensor *              pre_encode_out = nullptr;
    ggml_tensor *              pos_emb        = nullptr;
    ggml_tensor *              block0_out     = nullptr;
    ggml_tensor *              mid_block_out  = nullptr;
    ggml_tensor *              last_block_out = nullptr;
    ggml_tensor *              final_out      = nullptr;
    // Pre-final-transpose tensor, ne=[d_model, T_enc, 1]. PyTorch
    // equivalent shape (1, T_enc, d_model). This is what the RNN-T /
    // CTC head consumes for decode and what the reference dumps as
    // `rnnt.encoded`. final_out (post-transpose) is what gets compared
    // against the reference `enc.out` dump.
    ggml_tensor *              rnnt_encoded   = nullptr;
    int                        mid_block_idx  = -1;
    int                        last_block_idx = -1;
    std::vector<ggml_tensor *> all_block_outs;
    // Block-0 sub-step taps (batch-drift localization).
    ggml_tensor *              block0_after_attn = nullptr;
    ggml_tensor *              block0_after_conv = nullptr;
};

struct EncoderBuild {
    ggml_tensor * mel_in = nullptr;
    ggml_tensor * out    = nullptr;

    // Variable-length offline batch masks. All null for single-shot and
    // same-length batches; allocated only when build_encoder_graph is
    // called with batch_var_len && n_batch > 1, and filled host-side by
    // the driver after the compute buffer is allocated.
    //
    //   attn_pad_mask_in   ne=[T_enc, 1, 1, B] f32, 0 on real keys / -INF
    //                      on padded keys. Added onto the attention score
    //                      matrix (forces the manual SDPA path) so a real
    //                      query never attends another utterance's tail.
    //   conv_pad_mask_in   ne=[T_enc, 1, B, 1] f32, 1 on real frames / 0 on
    //                      padded. Zeros padded frames before each block's
    //                      depthwise conv (conf::conv_module's conv_pad_mask).
    //   pre_encode_mask_s{1,2}_in  ne=[T_stage, 1, B] f32 (masked
    //                      subsampling) — one per conv-relu stage, zeroing
    //                      the padded time region so the next stride-2 conv
    //                      cannot leak padding into a real boundary frame.
    ggml_tensor * attn_pad_mask_in      = nullptr;
    ggml_tensor * conv_pad_mask_in      = nullptr;
    ggml_tensor * pre_encode_mask_s1_in = nullptr;  // after conv0 relu
    ggml_tensor * pre_encode_mask_s2_in = nullptr;  // after conv2 relu

    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build a fresh encoder forward graph.
//
// n_batch: utterances packed along the encoder batch axis (B at the
// activation's ne[2]). 1 is the single-shot path and is byte-identical to
// the pre-batch graph. > 1 (offline transcribe_run_batch) makes mel_in
// ne=[n_mel_frames, n_mels, n_batch] and `out` ne=[d_model, T_enc, n_batch].
// batch_var_len: when true and n_batch > 1, allocate the variable-length
// masks above and wire them into the pre-encode + every block. The driver
// fills them from per-utterance lengths. Same-length batches leave it false
// and stay mask-free (bit-identical to single-shot per utterance).
EncoderBuild build_encoder_graph(ggml_context *        compute_ctx,
                                 const GigaamWeights & weights,
                                 const GigaamHParams & hp,
                                 int                   n_mel_frames,
                                 ggml_type             kv_type       = GGML_TYPE_COUNT,
                                 const char *          backend_name  = "",
                                 int                   n_batch       = 1,
                                 bool                  batch_var_len = false);

}  // namespace transcribe::gigaam
