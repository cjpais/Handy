// arch/canary_qwen/decoder.h - Qwen3-1.7B LM prefill + step graphs.
//
// Same Qwen3 block as funasr_nano (and qwen3_asr): pre-LN RMSNorm, GQA
// 16/8, per-head Q/K-RMSNorm on head_dim=128, NeoX RoPE @ θ=1e6,
// SwiGLU MLP on packed gate_up, tied lm_head.
//
// Audio injection: 3-way concat [prefix_tok_emb | perception_out | suffix_tok_emb].
// For B=1 with one <|audioplaceholder|> in the prompt, this is exactly
// equivalent to NeMo SALM's `replace_placeholders_and_build_targets`.

#pragma once

#include "canary_qwen.h"
#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "weights.h"

struct ggml_context;
struct ggml_cgraph;
struct ggml_tensor;

namespace transcribe::canary_qwen {

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;
    ggml_tensor * audio_injected  = nullptr;
    ggml_tensor * block_0_out     = nullptr;
    ggml_tensor * block_mid_out   = nullptr;  // dec.block.{n_layers/2}
    ggml_tensor * block_last_out  = nullptr;  // dec.block.{n_layers-1}
    ggml_tensor * out_before_head = nullptr;
    ggml_tensor * logits_raw      = nullptr;  // dec.logits_raw.gen0
};

struct PrefillBuild {
    ggml_tensor * input_ids_in = nullptr;  // [T_prompt] i32
    ggml_tensor * audio_in     = nullptr;  // [hidden, T_audio] f32 (perception output)
    ggml_tensor * positions_in = nullptr;  // [T_prompt] i32
    ggml_tensor * mask_in      = nullptr;  // [T_prompt, T_prompt] f16
    ggml_tensor * out          = nullptr;  // [vocab] f32
    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    int T_prompt   = 0;
    int T_audio    = 0;
    int prefix_len = 0;
    int suffix_len = 0;
};

PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const CanaryQwenWeights &        weights,
                                 const CanaryQwenHParams &        hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              T_prompt,
                                 int                              T_audio,
                                 int                              prefix_len,
                                 int                              suffix_len,
                                 bool                             use_flash,
                                 bool                             slice_last);

struct StepBuild {
    ggml_tensor * input_id_in = nullptr;  // [1] i32
    ggml_tensor * position_in = nullptr;  // [1] i32
    ggml_tensor * kv_idx_in   = nullptr;  // [1] i64
    ggml_tensor * mask_in     = nullptr;  // [max_n_kv, 1] f16
    ggml_tensor * out         = nullptr;  // [1] i32 — argmax
    ggml_tensor * logits      = nullptr;  // [vocab] f32 — exposed for mid-gen dump
    ggml_cgraph * graph       = nullptr;

    int max_n_kv = 0;
};

StepBuild build_step_graph(ggml_context *                   ctx,
                           const CanaryQwenWeights &        weights,
                           const CanaryQwenHParams &        hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash);

// ---------- Batched prefill / step (offline transcribe_run_batch) ----------
// Mirror arch/qwen3_asr + arch/funasr_nano; audio block is the perception
// output. See causal_lm::block_prefill_batched / block_step_batched.

struct PrefillBuildBatched {
    ggml_tensor * input_ids_in   = nullptr;  // [T_prompt_max, B] i32
    // Audio injection via elementwise blend (no set_rows): audio_dense holds the
    // audio embeds scattered host-side into their prompt positions (zero
    // elsewhere), keep_mask is 0 there and 1 elsewhere, block input is
    // x*keep_mask + audio_dense. Elementwise ops cross the CPU/CUDA split
    // (forced by k-quant token_embd get_rows) cleanly, unlike a set_rows.
    ggml_tensor * audio_dense_in = nullptr;  // [hidden, T_prompt_max*B] f32
    ggml_tensor * keep_mask_in   = nullptr;  // [1, T_prompt_max*B] f32 (0=audio,1=keep)
    ggml_tensor * positions_in   = nullptr;  // [T_prompt_max] i32
    ggml_tensor * mask_in        = nullptr;  // [T_prompt_max, T_prompt_max] f16
    ggml_tensor * kv_idx_in      = nullptr;  // [T_prompt_max, B] i64
    ggml_tensor * last_idx_in    = nullptr;  // [1, B] i32
    ggml_tensor * logits         = nullptr;  // [vocab, B]
    ggml_tensor * out            = nullptr;  // [B] i32 argmax
    ggml_cgraph * graph          = nullptr;
    int           T_prompt_max   = 0;
    int           T_audio_max    = 0;
    int           n_batch        = 0;
};

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const CanaryQwenWeights &        weights,
                                                const CanaryQwenHParams &        hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                int                              T_prompt_max,
                                                int                              T_audio_max,
                                                int                              n_batch,
                                                bool                             use_flash);

struct StepBuildBatched {
    ggml_tensor * input_ids_in = nullptr;  // [B] i32
    ggml_tensor * position_in  = nullptr;  // [B] i32
    ggml_tensor * kv_idx_in    = nullptr;  // [1, B] i64
    ggml_tensor * mask_in      = nullptr;  // [max_n_kv, 1, 1, B] f16
    ggml_tensor * logits       = nullptr;  // [vocab, B]
    ggml_tensor * out          = nullptr;  // [B] i32 argmax
    ggml_cgraph * graph        = nullptr;
    int           max_n_kv     = 0;
    int           n_batch      = 0;
};

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const CanaryQwenWeights &        weights,
                                          const CanaryQwenHParams &        hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash);

}  // namespace transcribe::canary_qwen
