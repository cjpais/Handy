// arch/granite/projector.h - Granite Speech projector (BLIP-2 Q-Former).
//
// Reference: GraniteSpeechEncoderProjector +
// Blip2QFormerModel/Encoder/Layer in transformers/models/blip_2/.
// See projector.cpp for the forward conventions. INTERNAL to src/arch/granite/.

#pragma once

#include "ggml.h"
#include "weights.h"

#include <cstdint>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::granite {

struct ProjectorBuild {
    // Graph inputs.
    ggml_tensor * enc_in  = nullptr;  // [hidden_enc, T_enc]
    ggml_tensor * enc_pad = nullptr;  // [hidden_enc, pad_n], or nullptr
                                      // when T_enc is aligned to
                                      // window_size

    // Graph output.
    ggml_tensor * out = nullptr;  // [text_hidden, n_audio_tokens]

    // Dump points.
    struct {
        ggml_tensor * qformer_out = nullptr;  // [hidden, num_queries, nblocks]
        ggml_tensor * proj_out    = nullptr;  // == out, named "proj.out"
    } dumps;

    ggml_cgraph * graph = nullptr;

    int nblocks        = 0;
    int n_audio_tokens = 0;
    int t_enc_pad      = 0;  // pad_n = nblocks * window_size - T_enc
};

ProjectorBuild build_projector_graph(ggml_context *         ctx,
                                     const GraniteWeights & weights,
                                     const GraniteHParams & hp,
                                     int                    T_enc);

}  // namespace transcribe::granite
