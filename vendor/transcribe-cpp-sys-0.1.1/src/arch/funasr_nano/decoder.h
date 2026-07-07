// arch/funasr_nano/decoder.h - Qwen3 LM prefill + step graphs.
//
// Pruned fork of arch/qwen3_asr/decoder.h. Same Qwen3 block: pre-LN
// RMSNorm, GQA, per-head Q/K-RMSNorm, NeoX RoPE @ freq_base=1e6, SwiGLU
// MLP, tied lm_head. Audio injection via three-way concat
// [prefix | adaptor_out[:fake_token_len] | suffix].

#pragma once

#include "causal_lm/causal_lm.h"
#include "funasr_nano.h"
#include "ggml.h"
#include "weights.h"

struct ggml_context;
struct ggml_cgraph;
struct ggml_tensor;

namespace transcribe::funasr_nano {

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;
    ggml_tensor * audio_injected  = nullptr;
    ggml_tensor * block_0_out     = nullptr;
    ggml_tensor * block_last_out  = nullptr;
    ggml_tensor * out_before_head = nullptr;
    ggml_tensor * logits_raw      = nullptr;
};

struct PrefillBuild {
    ggml_tensor * input_ids_in = nullptr;  // [T_prompt] i32
    ggml_tensor * audio_in     = nullptr;  // [hidden, T_audio] f32 (slice of adaptor_out)
    ggml_tensor * positions_in = nullptr;  // [T_prompt] i32
    ggml_tensor * mask_in      = nullptr;  // [T_prompt, T_prompt] f16
    ggml_tensor * out          = nullptr;  // [vocab]
    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    int T_prompt   = 0;
    int T_audio    = 0;
    int prefix_len = 0;
    int suffix_len = 0;
};

PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const FunAsrNanoWeights &        weights,
                                 const FunAsrNanoHParams &        hp,
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
    ggml_tensor * logits      = nullptr;  // [vocab] f32 — pre-argmax, for dumps
    ggml_cgraph * graph       = nullptr;

    int max_n_kv = 0;
};

StepBuild build_step_graph(ggml_context *                   ctx,
                           const FunAsrNanoWeights &        weights,
                           const FunAsrNanoHParams &        hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash);

// ---------- Batched prefill / step (offline transcribe_run_batch) ----------
// Mirror arch/qwen3_asr's batched builders; the only family difference is the
// audio block is the adaptor output (already in llm_dim space). See
// causal_lm::block_prefill_batched / block_step_batched.

struct PrefillBuildBatched {
    ggml_tensor * input_ids_in   = nullptr;  // [T_prompt_max, B] i32
    // Audio injection via elementwise blend (no set_rows): audio_dense holds
    // the audio embeds scattered (host-side) into their prompt positions, zero
    // elsewhere; keep_mask is 0 at audio positions and 1 elsewhere. The block
    // input is x*keep_mask + audio_dense. Elementwise ops cross the CPU/CUDA
    // split (forced by k-quant token_embd get_rows) cleanly, unlike a set_rows.
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
                                                const FunAsrNanoWeights &        weights,
                                                const FunAsrNanoHParams &        hp,
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
                                          const FunAsrNanoWeights &        weights,
                                          const FunAsrNanoHParams &        hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash);

}  // namespace transcribe::funasr_nano
