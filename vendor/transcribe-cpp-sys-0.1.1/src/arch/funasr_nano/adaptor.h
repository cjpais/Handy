// arch/funasr_nano/adaptor.h - audio adaptor graph builder.
//
// 2-layer transformer adaptor (FunASR's "Transformer" adaptor class):
// x_in [encoder_dim=512, T_lfr] -> linear1 (512->2048) -> ReLU ->
// linear2 (2048->llm_dim=1024) -> blocks[0..1] (pre-LN MHA + bottleneck FFN
// 1024->256->1024) -> [llm_dim, T_lfr].
//
// downsample_rate=1 means linear1 input is encoder_dim (no fold). Only
// retained as a configuration knob for sibling variants that may set k>1.

#pragma once

#include "ggml.h"

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::funasr_nano {

struct FunAsrNanoHParams;
struct FunAsrNanoWeights;

struct AdaptorDumps {
    ggml_tensor * linear1_out = nullptr;  // post-linear, pre-ReLU
    ggml_tensor * linear2_out = nullptr;
    ggml_tensor * block0_out  = nullptr;
    ggml_tensor * adaptor_out = nullptr;
};

struct AdaptorBuild {
    ggml_tensor * enc_in = nullptr;  // [encoder_dim, T_lfr] f32
    ggml_tensor * out    = nullptr;  // [llm_dim, T_lfr]
    AdaptorDumps  dumps{};
    ggml_cgraph * graph = nullptr;
};

AdaptorBuild build_adaptor_graph(ggml_context *            compute_ctx,
                                 const FunAsrNanoWeights & weights,
                                 const FunAsrNanoHParams & hp,
                                 int                       T_in);

// Compute the LLM-injected slice length for use_low_frame_rate=true.
// Mirrors FunASR's data_load_speech formula:
//
//   o1 = 1 + (T - 3 + 2) / 2     // = (T - 1) / 2 + 1
//   o2 = 1 + (o1 - 3 + 2) / 2
//   fake_token_len = (o2 - 1) / 2 + 1
//
// With T=183 → 23. With use_low_frame_rate=false the spliced length
// equals the input frame count.
int compute_fake_token_len(int T_lfr, bool use_low_frame_rate);

}  // namespace transcribe::funasr_nano
