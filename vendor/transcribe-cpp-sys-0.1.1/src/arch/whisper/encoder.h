// arch/whisper/encoder.h - Whisper encoder graph builder.
//
// 2-layer Conv1d stem -> pre-LN transformer blocks -> final LayerNorm; no
// relative position encoding, no in-block conv, no enc-dec projection
// (enc_d_model == dec_d_model upstream).
//
// Layout: all intermediate activations are kept in [d_model, T] ggml layout
// (d_model innermost, T second). The conv stem transposes into [T, d_model]
// for the ggml_conv_1d calls and back out afterwards.

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::whisper {

struct WhisperHParams;
struct WhisperWeights;

struct EncoderDumps {
    ggml_tensor * mel_in         = nullptr;
    ggml_tensor * conv1_out      = nullptr;
    ggml_tensor * conv2_out      = nullptr;
    ggml_tensor * pos_emb        = nullptr;
    ggml_tensor * embed_out      = nullptr;
    ggml_tensor * block0_out     = nullptr;
    ggml_tensor * block_last_out = nullptr;
    ggml_tensor * final_out      = nullptr;

    // Every post-block residual stream output, in block order (alongside
    // block0_out / block_last_out for callers that want all N layers).
    std::vector<ggml_tensor *> block_outs;
};

struct EncoderBuild {
    // Input tensor: [n_mel_bins, n_mel_frames] host-side layout. The
    // graph builder declares ggml_set_input on this tensor; the caller
    // uploads the mel spectrogram with ggml_backend_tensor_set before
    // the graph is computed.
    ggml_tensor * mel_in = nullptr;

    // Output tensor: [d_model, T_enc] where T_enc = n_mel_frames / 2
    // after the stride-2 second conv.
    ggml_tensor * out = nullptr;

    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build the encoder forward graph in compute_ctx.
//
// n_mel_frames must be positive and even (upstream pads/trims to 3000; smaller
// even values are accepted for test fixtures). use_flash selects
// ggml_flash_attn_ext vs. manual mul_mat + soft_max; the caller gates it
// against backend support. backend_name is informational only.
EncoderBuild build_encoder_graph(ggml_context *         compute_ctx,
                                 const WhisperWeights & weights,
                                 const WhisperHParams & hp,
                                 int                    n_mel_frames,
                                 bool                   use_flash    = true,
                                 const char *           backend_name = "");

}  // namespace transcribe::whisper
