// arch/sensevoice/encoder.h - SenseVoice SAN-M encoder graph builder.
//
// The .cpp file builds a ggml_cgraph that mirrors
// SenseVoiceEncoderSmall.forward (SAN-M blocks with FSMN memory branch,
// two-tier depth, sinusoidal PE, pre-encoder prefix-embedding prepend).
//
// This header is INTERNAL to src/arch/sensevoice/. The C ABI sees
// only SenseVoice::run, which builds and runs this graph.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::sensevoice {

struct SenseVoiceHParams;
struct SenseVoiceWeights;

// Named intermediate dump points. Each field is a borrowed pointer into
// the compute_ctx; nullptr means the sub-stage is not wired.
struct EncoderDumps {
    // Frontend output (LFR + CMVN). ne=[d_input, T_lfr, 1, 1].
    ggml_tensor * frontend_out      = nullptr;
    // Prefix embedding lookups + concat result.
    // Each lookup: ne=[d_input, n, 1, 1] (n = 1 or 2).
    ggml_tensor * prefix_lid        = nullptr;
    ggml_tensor * prefix_event_emo  = nullptr;
    ggml_tensor * prefix_textnorm   = nullptr;
    // After cat([lid, event_emo, textnorm, frontend_out]).
    // ne=[d_input, 4 + T_lfr, 1, 1].
    ggml_tensor * input_with_prefix = nullptr;

    // Post-PE (post-scale + post-sin/cos add). ne=[d_input, T, 1, 1].
    ggml_tensor * embed_out = nullptr;

    // Block-output spot checks. Each ne=[d_model, T, 1, 1] (or d_input
    // for encoders0[0]'s norm_attn input — but the BLOCK output is
    // d_model after the 560→512 projection).
    ggml_tensor * encoders0_0_out   = nullptr;
    ggml_tensor * encoders_first    = nullptr;  // encoders[0]
    ggml_tensor * encoders_mid      = nullptr;  // encoders[len/2]
    ggml_tensor * encoders_last     = nullptr;  // encoders[len-1]
    ggml_tensor * after_norm_out    = nullptr;
    ggml_tensor * tp_encoders_first = nullptr;
    ggml_tensor * tp_encoders_mid   = nullptr;
    ggml_tensor * tp_encoders_last  = nullptr;
    ggml_tensor * tp_norm_out       = nullptr;

    // CTC head outputs. ne=[vocab, T, 1, 1].
    ggml_tensor * ctc_logits    = nullptr;
    ggml_tensor * ctc_log_probs = nullptr;
};

struct EncoderBuild {
    // Input handle for the LFR + CMVN frontend output.
    // ne=[d_input, T_lfr, n_batch, 1] f32. Caller fills via
    // ggml_backend_tensor_set with the row-major [n_batch, T_lfr, d_input]
    // buffer (per utterance the kaldi-fbank frontend's [T_lfr, d_input]),
    // zero-padded to the batch's T_lfr (= n_lfr_frames) for variable
    // lengths.
    ggml_tensor * frontend_in = nullptr;

    // Prefix-token indices to look up via ggml_get_rows.
    // ne=[1] each (i32). Filled at run() time from the requested
    // language / use_itn flags. Shared across the batch (v1: one
    // run_params per batch).
    ggml_tensor * lid_idx       = nullptr;
    ggml_tensor * event_emo_idx = nullptr;  // ne=[2], values [1, 2] literally
    ggml_tensor * textnorm_idx  = nullptr;

    // Sinusoidal PE input, ne=[d_input, T] f32 (T = n_lfr_frames + 4). The
    // driver fills it host-side; it broadcasts over the batch axis.
    ggml_tensor * pe_in = nullptr;

    // Variable-length batch masks. Null unless build_encoder_graph was
    // called with batch_var_len and n_batch > 1; the driver fills them
    // host-side after the compute buffer is allocated.
    //   attn_pad_mask_in : ne=[T, 1, 1, n_batch] f32 (0 real, -INF padded)
    //   conv_pad_mask_in : ne=[1, T, n_batch]     f32 (1 real, 0 padded)
    // where T = n_lfr_frames + 4 (post-prefix length).
    ggml_tensor * attn_pad_mask_in = nullptr;
    ggml_tensor * conv_pad_mask_in = nullptr;

    // CTC log-probabilities. ne=[vocab, T, n_batch, 1].
    ggml_tensor * out = nullptr;

    EncoderDumps dumps{};

    ggml_cgraph * graph = nullptr;
};

// n_batch: number of utterances packed along the encoder batch axis (B at
// the activation's ne[2]). 1 is the single-shot path and is byte-identical
// to the pre-batch graph. > 1 (offline transcribe_run_batch) requires every
// packed utterance to share n_lfr_frames (same-length batch) unless
// batch_var_len is set, in which case the caller pads each mel to a common
// n_lfr_frames and the two masks above gate the padded tail.
EncoderBuild build_encoder_graph(ggml_context *            compute_ctx,
                                 const SenseVoiceWeights & weights,
                                 const SenseVoiceHParams & hp,
                                 int                       n_lfr_frames,
                                 int                       n_batch       = 1,
                                 bool                      batch_var_len = false);

}  // namespace transcribe::sensevoice
