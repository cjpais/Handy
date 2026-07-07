// arch/canary/encoder.h - Canary FastConformer encoder graph builder.
//
// Parakeet-shape FastConformer but with biases on every linear; 180m-flash
// adds an optional encoder->decoder projection.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::canary {

struct CanaryHParams;
struct CanaryWeights;

struct EncoderDumps {
    ggml_tensor * mel_in         = nullptr;
    ggml_tensor * pre_encode_out = nullptr;
    ggml_tensor * pos_emb        = nullptr;
    ggml_tensor * block0_out     = nullptr;
    ggml_tensor * block_mid_out  = nullptr;  // block n_layers/2
    ggml_tensor * block_last_out = nullptr;  // block n_layers-1
    ggml_tensor * native_out     = nullptr;  // pre-projection (dump as enc.native)
    ggml_tensor * final_out      = nullptr;  // post-projection (dump as enc.final)
};

struct EncoderBuild {
    ggml_tensor * mel_in     = nullptr;
    ggml_tensor * pos_emb_in = nullptr;
    ggml_tensor * out        = nullptr;  // post-projection if proj exists, else native
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

EncoderBuild build_encoder_graph(ggml_context *        compute_ctx,
                                 const CanaryWeights & weights,
                                 const CanaryHParams & hp,
                                 int                   n_mel_frames,
                                 ggml_type             kv_type      = GGML_TYPE_COUNT,
                                 bool                  use_flash    = true,
                                 const char *          backend_name = "");

}  // namespace transcribe::canary
