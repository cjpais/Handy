// arch/moonshine/encoder.h - Moonshine encoder graph builder.
//
// 3-conv stem on raw 16 kHz PCM (see encoder.cpp / weights.h) + n_layers
// pre-LN bidirectional partial-RoPE transformer blocks + final bias-less LN.
//
// Dump points:
//   enc.audio.in       raw PCM input
//   enc.conv1.out      after tanh(conv0)            (BEFORE GroupNorm)
//   enc.groupnorm.out  after GroupNorm
//   enc.conv2.out      after gelu(conv1)
//   enc.conv3.out      after gelu(conv2) + permute  (this is the block input)
//   enc.block.{i}.out  per-block residual stream
//   enc.final          after final LN

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::moonshine {

struct MoonshineHParams;
struct MoonshineWeights;

struct EncoderDumps {
    ggml_tensor *              audio_in      = nullptr;
    ggml_tensor *              conv1_out     = nullptr;  // tanh(conv0(audio))
    ggml_tensor *              groupnorm_out = nullptr;
    ggml_tensor *              conv2_out     = nullptr;  // gelu(conv1(.))
    ggml_tensor *              conv3_out     = nullptr;  // gelu(conv2(.)) (block input)
    ggml_tensor *              rope_cos      = nullptr;
    ggml_tensor *              rope_sin      = nullptr;
    std::vector<ggml_tensor *> block_outs;
    ggml_tensor *              final_out = nullptr;
};

struct EncoderBuild {
    // Input tensor: raw PCM [n_samples] f32. Caller uploads via
    // ggml_backend_tensor_set after alloc.
    ggml_tensor * audio_in = nullptr;

    // Position ids for partial-RoPE (encoder positions 0..T_enc-1).
    // The encoder graph builds T_enc from `n_samples` so it is the
    // builder's responsibility to compute and upload position ids
    // matching the actual T_enc.
    ggml_tensor * pos_ids_in = nullptr;

    // Output: final encoder hidden state [d_model, T_enc] f32.
    ggml_tensor * out = nullptr;

    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    // Number of encoder frames produced for the supplied n_samples.
    // Computed inside the builder using the conv-stem output formula
    // and exposed here so the caller can size pos_ids / cross-KV.
    int T_enc = 0;
};

// Compute the encoder output frame count for a given input sample count.
// Mirrors the shape arithmetic of the 3-conv stem (no padding):
//
//   T1 = (T_in     - 127) / 64 + 1
//   T2 = (T1       - 7  ) / 3  + 1
//   T3 = (T2       - 3  ) / 2  + 1
//
// Returns 0 if the input is too short to produce at least one output.
int encoder_t_enc(const MoonshineHParams & hp, int n_samples);

// Build the encoder forward graph in compute_ctx.
EncoderBuild build_encoder_graph(ggml_context *           compute_ctx,
                                 const MoonshineWeights & weights,
                                 const MoonshineHParams & hp,
                                 int                      n_samples,
                                 bool                     use_flash = true);

}  // namespace transcribe::moonshine
