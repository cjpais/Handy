// arch/cohere/encoder.h - Cohere ASR Conformer encoder graph builder.
//
// Forked from parakeet/encoder.h. Key difference: FFN layers and
// attention projections have bias. The conformer block structure is
// otherwise identical.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::cohere {

struct CohereHParams;
struct CohereWeights;

struct EncoderDumps {
    ggml_tensor * mel_in           = nullptr;
    ggml_tensor * pre_encode_out   = nullptr;
    ggml_tensor * pos_emb          = nullptr;
    ggml_tensor * block0_out       = nullptr;
    ggml_tensor * block_mid_out    = nullptr;  // block 23 (middle of 48)
    ggml_tensor * block_last_out   = nullptr;  // block 47 (last)
    ggml_tensor * final_out        = nullptr;
    ggml_tensor * enc_dec_proj_out = nullptr;
};

struct EncoderBuild {
    ggml_tensor * mel_in     = nullptr;
    ggml_tensor * pos_emb_in = nullptr;
    ggml_tensor * out        = nullptr;  // after enc-dec projection
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

EncoderBuild build_encoder_graph(ggml_context *        compute_ctx,
                                 const CohereWeights & weights,
                                 const CohereHParams & hp,
                                 int                   n_mel_frames,
                                 ggml_type             kv_type      = GGML_TYPE_COUNT,
                                 bool                  use_flash    = true,
                                 const char *          backend_name = "");

}  // namespace transcribe::cohere
