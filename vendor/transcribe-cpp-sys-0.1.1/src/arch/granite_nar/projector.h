// arch/granite_nar/projector.h - NLE EncoderProjectorQFormer.
//
// Reference: EncoderProjectorQFormer in the NLE HF repo's modeling_nle.py.
// Strictly simpler than the AR granite BLIP-2 Q-Former: only cross-
// attention + MLP per layer, no self-attention. Windowed mean-pool of
// the encoder K/V plus a learned `window_positions` bias acts in place
// of positional encodings.
//
// The upstream layer math is PRE-LN (not post-LN like the AR BLIP-2 layer):
// norm comes BEFORE each sublayer's linear cluster, and the residual closes
// after the second linear. See projector.cpp for the forward graph.
//
// INTERNAL to src/arch/granite_nar/.

#pragma once

#include "ggml.h"
#include "weights.h"

#include <cstdint>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::granite_nar {

struct ProjectorBuild {
    ggml_tensor * enc_in  = nullptr;  // [num_layers * enc_hidden, T_enc]
    ggml_tensor * enc_pad = nullptr;  // optional [num_layers * enc_hidden, pad_n]

    ggml_tensor * out = nullptr;      // [llm_dim, n_audio_tokens]

    struct {
        ggml_tensor * qformer_out = nullptr;  // [llm_dim, n_query, nblocks]
        ggml_tensor * proj_out    = nullptr;  // == out
    } dumps;

    ggml_cgraph * graph          = nullptr;
    int           nblocks        = 0;
    int           n_audio_tokens = 0;
    int           t_enc_pad      = 0;
};

ProjectorBuild build_projector_graph(ggml_context *            ctx,
                                     const GraniteNarWeights & weights,
                                     const GraniteNarHParams & hp,
                                     int                       T_enc);

}  // namespace transcribe::granite_nar
