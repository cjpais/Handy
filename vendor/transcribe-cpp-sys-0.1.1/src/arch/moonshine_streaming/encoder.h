// arch/moonshine_streaming/encoder.h - Moonshine-Streaming encoder graph.
//
// The encoder bundles a time-domain frontend (CMVN → asinh → linear +
// SiLU → 2× causal Conv1d) with N transformer blocks that use
// per-layer sliding-window attention masks (no RoPE on the encoder).
//
// Dump points:
//   enc.audio.in              raw PCM input
//   enc.embedder.cmvn.out     CMVN per-frame (eps=1e-6)
//   enc.embedder.comp.out     asinh(exp(log_k) * cmvn_out)
//   enc.embedder.linear.out   silu(linear(comp_out))
//   enc.embedder.conv1.out    silu(causal_conv1d(linear_out))
//   enc.embedder.conv2.out    causal_conv1d(conv1_out)  (NO silu)
//   enc.block.{i}.out         per-block residual stream
//   enc.final                 after final LN

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::moonshine_streaming {

struct MoonshineStreamingHParams;
struct MoonshineStreamingWeights;

struct EncoderDumps {
    ggml_tensor *              audio_in   = nullptr;
    ggml_tensor *              cmvn_out   = nullptr;
    ggml_tensor *              comp_out   = nullptr;
    ggml_tensor *              linear_out = nullptr;
    ggml_tensor *              conv1_out  = nullptr;
    ggml_tensor *              conv2_out  = nullptr;
    std::vector<ggml_tensor *> block_outs;
    ggml_tensor *              final_out = nullptr;
};

struct EncoderBuild {
    // Input tensor: raw PCM [n_samples] f32. Caller uploads via
    // ggml_backend_tensor_set after alloc.
    ggml_tensor * audio_in = nullptr;

    // Per-layer sliding-window attention masks. Each tensor is f32
    // shape [T_enc, T_enc] (n_kv, n_q). Caller uploads from host-built
    // mask buffers before computing the graph.
    std::vector<ggml_tensor *> per_layer_masks;

    // Output: final encoder hidden state [d_model, T_enc] f32.
    ggml_tensor * out = nullptr;

    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    // Number of encoder frames produced for the supplied n_samples.
    int T_enc = 0;
};

// Compute the encoder output frame count for a given input sample count.
// Streaming-tiny path:
//   T_frames = T_samples / frame_len
//   T1       = ceil(T_frames / 2)        (causal conv stride 2 keeps every
//                                          frame thanks to left-pad k-1)
//   T_enc    = ceil(T1 / 2)
//
// Returns 0 if the input is too short.
int encoder_t_enc(const MoonshineStreamingHParams & hp, int n_samples);

// Which per-layer blocks the encoder/decoder builders emit as debug dumps.
// Mirrors dump_reference_moonshine_streaming_transformers.py::auto_blocks
// so both sides dump the same subset.
bool dump_block_index(int i, int n_layers);

// Build the encoder forward graph in compute_ctx.
EncoderBuild build_encoder_graph(ggml_context *                    compute_ctx,
                                 const MoonshineStreamingWeights & weights,
                                 const MoonshineStreamingHParams & hp,
                                 int                               n_samples,
                                 bool                              use_flash = false);

// Build a host-side sliding-window mask of shape [T_enc, T_enc] for the
// (L, R) window. Allowed positions are 0.0; blocked are -INF.
//
// For row q (0..T_enc), col k is allowed iff:
//     (q-k >= 0 && q-k < L)   // up to L-1 positions back, including self
//   || (k-q >= 1 && k-q < R)  // up to R-1 positions ahead
//
// Caller-provided buffer must be at least T_enc*T_enc floats.
void build_sliding_window_mask(int T_enc, int left_window, int right_window, float * out_mask);

}  // namespace transcribe::moonshine_streaming
