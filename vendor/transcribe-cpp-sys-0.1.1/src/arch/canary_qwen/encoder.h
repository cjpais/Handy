// arch/canary_qwen/encoder.h - perception encoder graph builder.
//
// FastConformer encoder (32 blocks) followed by perception projection
// (Linear(1024, 2048) + bias). The conformer body is byte-for-byte
// identical to canary-1b-flash; the projection is canary_qwen-specific.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::canary_qwen {

struct CanaryQwenHParams;
struct CanaryQwenWeights;

struct EncoderDumps {
    ggml_tensor * mel_in         = nullptr;
    ggml_tensor * pre_encode_out = nullptr;
    ggml_tensor * pos_emb        = nullptr;
    ggml_tensor * block0_out     = nullptr;
    ggml_tensor * block_mid_out  = nullptr;  // block n_layers/2
    ggml_tensor * block_last_out = nullptr;  // block n_layers-1
    ggml_tensor * final_out      = nullptr;  // post-encoder, pre-projection
    ggml_tensor * perception_out = nullptr;  // post Linear(1024, 2048)
};

struct EncoderBuild {
    ggml_tensor * mel_in     = nullptr;
    ggml_tensor * pos_emb_in = nullptr;
    ggml_tensor * out        = nullptr;  // perception_out
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

EncoderBuild build_encoder_graph(ggml_context *            ctx,
                                 const CanaryQwenWeights & weights,
                                 const CanaryQwenHParams & hp,
                                 int                       n_mel_frames,
                                 ggml_type                 kv_type      = GGML_TYPE_COUNT,
                                 bool                      use_flash    = false,
                                 const char *              backend_name = "");

}  // namespace transcribe::canary_qwen
