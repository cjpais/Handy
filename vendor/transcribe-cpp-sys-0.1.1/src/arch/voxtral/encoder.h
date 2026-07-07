// arch/voxtral/encoder.h - Voxtral audio encoder + projector graph.
//
// The encoder is the Whisper-large-v3 encoder: a 2-layer Conv1d stem
// followed by a fixed sinusoidal positional embedding, 32 pre-LN
// transformer blocks (LayerNorm with bias; q/v/out biases but no k
// bias; GELU FFN) and a final LayerNorm. The projector then groups 4
// consecutive encoder frames (1500 -> 375) and runs Linear -> GELU ->
// Linear (no bias) into the decoder hidden dim.
//
// One graph processes ONE 30 s chunk: mel [n_mels, 3000] in,
// proj.out [dec_hidden, 375] out (the audio embeddings injected into
// the LM). enc.out [d_model, 1500] is exposed for dump parity.

#pragma once

#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::voxtral {

struct VoxtralHParams;
struct VoxtralWeights;

struct EncoderDumps {
    ggml_tensor *              enc_out  = nullptr;  // final encoder LayerNorm output
    ggml_tensor *              proj_out = nullptr;  // projector output (audio embeds)
    std::vector<ggml_tensor *> block_outs;          // per-block residual outputs
};

struct EncoderBuild {
    ggml_tensor * mel_in = nullptr;  // [n_mels, n_mel_frames] input
    ggml_tensor * out    = nullptr;  // proj.out [dec_hidden, n_audio_tokens]
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

// Build the encoder+projector forward graph for ONE 30 s chunk.
// n_mel_frames must be 2 * max_source_positions (3000) so the stride-2
// conv2 yields exactly max_source_positions (1500) frames and the
// fixed positional embedding lines up.
EncoderBuild build_encoder_graph(ggml_context *         ctx,
                                 const VoxtralWeights & weights,
                                 const VoxtralHParams & hp,
                                 int                    n_mel_frames,
                                 bool                   use_flash);

// Batched encoder+projector: process `n_chunks` equal-length 30 s chunks at once
// (batch on ne[2]). mel_in is [n_mels, n_mel_frames, n_chunks]; out (proj.out) is
// [dec_hidden, n_audio_tokens, n_chunks]. Each chunk is an independent forward
// pass sharing weights, so chunk b is numerically identical to the single-chunk
// build_encoder_graph. Requires use_flash (no batched manual-GQA path); the
// caller falls back to per-chunk when flash is off. No dump hooks.
EncoderBuild build_encoder_graph_batched(ggml_context *         ctx,
                                         const VoxtralWeights & weights,
                                         const VoxtralHParams & hp,
                                         int                    n_mel_frames,
                                         int                    n_chunks,
                                         bool                   use_flash);

}  // namespace transcribe::voxtral
