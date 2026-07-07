// arch/funasr_nano/encoder.h - SAN-M encoder graph builder.
//
// Pruned fork of arch/sensevoice/encoder.h:
//   - drop the prefix-embedding prepend (no `enc.embed.weight` lookup,
//     no [lid, event_emo, textnorm] cat),
//   - drop the CTC head (the GGUF carries no `ctc.head.*`),
//   - encoder output is `enc.tp_norm.out` of shape [d_model, T_lfr],
//     consumed by the audio adaptor.
//
// Sinusoidal PE depth = current width = d_input (=560), 1-based positions
// (matches funasr.SinusoidalPositionEncoder.encode).

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::funasr_nano {

struct FunAsrNanoHParams;
struct FunAsrNanoWeights;

struct EncoderDumps {
    ggml_tensor * frontend_out      = nullptr;  // [d_input, T_lfr] (host-set)
    ggml_tensor * embed_out         = nullptr;  // post-PE add
    ggml_tensor * encoders0_0_out   = nullptr;
    ggml_tensor * encoders_first    = nullptr;
    ggml_tensor * encoders_mid      = nullptr;
    ggml_tensor * encoders_last     = nullptr;
    ggml_tensor * after_norm_out    = nullptr;
    ggml_tensor * tp_encoders_first = nullptr;
    ggml_tensor * tp_encoders_mid   = nullptr;
    ggml_tensor * tp_encoders_last  = nullptr;
    ggml_tensor * tp_norm_out       = nullptr;
};

struct EncoderBuild {
    ggml_tensor * frontend_in = nullptr;  // [d_input, T_lfr] f32 input
    ggml_tensor * pe_in       = nullptr;  // [d_input, T_lfr] f32 sinusoidal PE
    ggml_tensor * out         = nullptr;  // [d_model, T_lfr]
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

EncoderBuild build_encoder_graph(ggml_context *            compute_ctx,
                                 const FunAsrNanoWeights & weights,
                                 const FunAsrNanoHParams & hp,
                                 int                       n_lfr_frames);

}  // namespace transcribe::funasr_nano
