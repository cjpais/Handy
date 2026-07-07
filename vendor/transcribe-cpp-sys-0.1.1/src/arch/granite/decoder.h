// arch/granite/decoder.h - Granite-4 causal LM graph builders.
//
// Reference: GraniteForCausalLM + GraniteDecoderLayer in
// transformers/models/granite/modeling_granite.py.
//
// Granite-4 block math:
//   - pre-LN RMSNorm (eps from KV)
//   - GQA (n_heads / n_kv_heads from KV; head_dim 128 on 1b)
//   - NO per-head Q/K-RMSNorm (unlike Qwen3)
//   - NeoX-style RoPE @ θ=10000
//   - Attention softmax scale = attention_multiplier (1/128), NOT
//     1/sqrt(head_dim).
//   - Residual adds use residual_multiplier (0.22): x + 0.22 * sub_out
//   - SwiGLU MLP. When ffn_gate_up_w is present (packed at load via
//     causal_lm::pack_gate_up), the step, batched-prefill, and
//     batched-step paths take the fused single-mul_mat + ggml_swiglu
//     route, falling back to separate gate/up mul_mats otherwise. The
//     single (non-batched) prefill path always uses separate gate + up.
//     See decoder.cpp (ffn_gate_up_w branches in block_step /
//     block_prefill_batched / block_step_batched; block_prefill is
//     unconditionally separate).
//
// Outer flow (matches HF GraniteSpeechForConditionalGeneration +
// GraniteModel.forward):
//   1. inputs_embeds = embed_tokens(input_ids)   # dec.token_emb dump
//   2. inputs_embeds[audio_mask] = audio_features  # dec.audio_injected dump
//   3. inputs_embeds = inputs_embeds * embedding_multiplier  (×12)
//   4. block stack ...
//   5. x = RMSNorm(x)                           # dec.out_before_head
//   6. logits = lm_head(x) / logits_scaling     # dec.logits_raw (last pos)
//
// The public path handles a single contiguous audio block at positions
// [prefix_len, prefix_len + n_audio_tokens). Granite's chat template
// has exactly one <|audio|> placeholder per turn so multi-audio prompts
// aren't reachable from the public API today.

#pragma once

#include "causal_lm/causal_lm.h"  // reuse KvCache only
#include "ggml.h"
#include "granite.h"
#include "weights.h"

struct ggml_context;
struct ggml_cgraph;
struct ggml_tensor;

namespace transcribe::granite {

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;  // [hidden, T_prompt] embed lookup
    ggml_tensor * audio_injected  = nullptr;  // [hidden, T_prompt] post-scatter, pre-multiplier
    ggml_tensor * block_0_out     = nullptr;
    ggml_tensor * block_mid_out   = nullptr;  // mid-stack (e.g. layer 20 on 40-layer)
    ggml_tensor * block_last_out  = nullptr;
    ggml_tensor * out_before_head = nullptr;  // final RMSNorm output
    ggml_tensor * logits_raw      = nullptr;  // [vocab] for the last position
};

struct PrefillBuild {
    ggml_tensor * input_ids_in   = nullptr;  // [T_prompt] i32
    ggml_tensor * audio_feats_in = nullptr;  // [hidden, n_audio_tokens] f32
    ggml_tensor * positions_in   = nullptr;  // [T_prompt] i32 for RoPE
    ggml_tensor * mask_in        = nullptr;  // [T_prompt, T_prompt] f16 (causal)
    ggml_tensor * out            = nullptr;  // [vocab] — last-position logits
    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    int T_prompt       = 0;
    int n_audio_tokens = 0;
    int prefix_len     = 0;
    int suffix_len     = 0;
};

// Build a prefill graph that consumes the full prompt token sequence
// (with `n_audio_tokens` contiguous audio_token_id placeholders at
// positions [prefix_len, prefix_len + n_audio_tokens)) plus the
// projector's audio features, and produces logits for the last
// position only.
//
// The kv_cache is WRITTEN by the graph. Callers should set
// kv_cache.n = T_prompt and kv_cache.head = T_prompt after compute.
PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const GraniteWeights &           weights,
                                 const GraniteHParams &           hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              T_prompt,
                                 int                              n_audio_tokens,
                                 int                              prefix_len,
                                 int                              suffix_len,
                                 bool                             use_flash,
                                 bool                             slice_last);

// Step graph (one token).

struct StepBuild {
    ggml_tensor * input_id_in = nullptr;  // [1] i32
    ggml_tensor * position_in = nullptr;  // [1] i32, value = n_past
    ggml_tensor * kv_idx_in   = nullptr;  // [1] i64, KV write position
    ggml_tensor * mask_in     = nullptr;  // [max_n_kv, 1] f16
    ggml_tensor * out         = nullptr;  // [1] i32 — argmax token id
    ggml_cgraph * graph       = nullptr;

    int max_n_kv = 0;
};

// Build a static-shape single-token step graph reusable across every
// autoregressive step. Topology depends only on max_n_kv.
StepBuild build_step_graph(ggml_context *                   ctx,
                           const GraniteWeights &           weights,
                           const GraniteHParams &           hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash);

// Batched prefill / step (offline transcribe_run_batch).
// Same recipe as the causal_lm families but with Granite block math (no Q/K
// norm, attention_multiplier scale, residual_multiplier, embedding_multiplier,
// logits_scaling). Batch rides ne[2]; requires use_flash.

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
    int           n_audio_max    = 0;
    int           n_batch        = 0;
};

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const GraniteWeights &           weights,
                                                const GraniteHParams &           hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                int                              T_prompt_max,
                                                int                              n_audio_max,
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
                                          const GraniteWeights &           weights,
                                          const GraniteHParams &           hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash);

}  // namespace transcribe::granite
