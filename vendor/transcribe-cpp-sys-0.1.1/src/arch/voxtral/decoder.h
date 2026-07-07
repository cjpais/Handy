// arch/voxtral/decoder.h - Voxtral LM graph builders (prefill + step).
//
// The LM is a Llama / Ministral causal decoder:
//   - pre-LN RMSNorm (eps 1e-5)
//   - GQA (32 Q heads, 8 KV heads, head_dim 128), NO per-head Q/K norm
//   - NEOX RoPE (rotate_half) at theta 1e8
//   - SwiGLU MLP: down(silu(gate(x)) * up(x)) via packed gate+up
//   - UNTIED lm_head (dec.output.weight, separate from token_embd)
//
// The per-block math is the shared causal_lm module; this file owns graph
// allocation, audio injection, tensor naming / dump parity, the untied head, and
// the driver-facing build structs.
//
// Audio injection: the prompt has `audio_token_id` placeholders at a single
// contiguous run [prefix_len, prefix_len + T_enc); the projector output (T_enc
// audio embeddings) is spliced there via a 3-way concat (prefix | proj.out |
// suffix).

#pragma once

#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "voxtral.h"
#include "weights.h"

struct ggml_context;
struct ggml_cgraph;
struct ggml_tensor;

namespace transcribe::voxtral {

struct DecoderDumps {
    ggml_tensor * token_emb       = nullptr;  // embed_tokens, pre-injection
    ggml_tensor * audio_injected  = nullptr;  // after audio splice
    ggml_tensor * block_0_out     = nullptr;
    ggml_tensor * block_mid_out   = nullptr;
    ggml_tensor * block_last_out  = nullptr;
    ggml_tensor * out_before_head = nullptr;  // final RMSNorm output
    ggml_tensor * logits_raw      = nullptr;  // [vocab] last position
};

struct PrefillBuild {
    ggml_tensor * input_ids_in = nullptr;  // [T_prompt] i32
    ggml_tensor * enc_out_in   = nullptr;  // [dec_hidden, T_enc] f32 (proj.out)
    ggml_tensor * positions_in = nullptr;  // [T_prompt] i32 for RoPE
    ggml_tensor * mask_in      = nullptr;  // [T_prompt, T_prompt] f16 causal
    ggml_tensor * out          = nullptr;  // [vocab] last-position logits
    DecoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    int T_prompt   = 0;
    int T_enc      = 0;
    int prefix_len = 0;
    int suffix_len = 0;
};

// Prefill graph: token-embed the prompt, splice proj.out over the audio
// placeholder run, run the LM blocks (writing KV [0,T_prompt)), final
// RMSNorm + UNTIED head, output last-position logits.
PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const VoxtralWeights &           weights,
                                 const VoxtralHParams &           hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              T_prompt,
                                 int                              T_enc,
                                 int                              prefix_len,
                                 int                              suffix_len,
                                 bool                             use_flash,
                                 bool                             slice_last);

struct StepBuild {
    ggml_tensor * input_id_in = nullptr;  // [1] i32
    ggml_tensor * position_in = nullptr;  // [1] i32 (= n_past)
    ggml_tensor * kv_idx_in   = nullptr;  // [1] i64 (KV write position)
    ggml_tensor * mask_in     = nullptr;  // [max_n_kv, 1] f16
    ggml_tensor * out         = nullptr;  // [1] i32 argmax token id
    ggml_tensor * logits      = nullptr;  // [vocab] f32 step logits (for gen<N> dump)
    ggml_cgraph * graph       = nullptr;

    int max_n_kv = 0;
};

// Static-shape single-token step graph, reused across every step.
StepBuild build_step_graph(ggml_context *                   ctx,
                           const VoxtralWeights &           weights,
                           const VoxtralHParams &           hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash);

// ---------------------------------------------------------------------------
// Batched prefill / step (offline transcribe_run_batch fast path)
//
// B utterances decoded in lockstep against a batched KV cache (n_batch=B).
// Ragged prompts are right-padded to T_prompt_max; the shared causal mask works
// because pads land after each utterance's real tokens, and per-row last_idx
// selects each real final position. Audio embeddings are blended in at the
// audio_token_id positions. Both require use_flash (the batched blocks have no
// manual GQA path). UNTIED lm_head (dec.output.weight).
// ---------------------------------------------------------------------------

struct PrefillBuildBatched {
    ggml_tensor * input_ids_in   = nullptr;  // [T_prompt_max, B] i32 (right-padded)
    // Audio injection via elementwise blend (no set_rows): audio_dense holds
    // the audio embeds scattered (host-side) into their prompt positions, zero
    // elsewhere; keep_mask is 0 at audio positions and 1 elsewhere. The block
    // input is x*keep_mask + audio_dense. Elementwise ops cross the CPU/CUDA
    // split (forced by k-quant token_embd get_rows) cleanly, unlike a set_rows.
    ggml_tensor * audio_dense_in = nullptr;  // [dec_hidden, T_prompt_max*B] f32
    ggml_tensor * keep_mask_in   = nullptr;  // [1, T_prompt_max*B] f32 (0=audio,1=keep)
    ggml_tensor * positions_in   = nullptr;  // [T_prompt_max] i32 (shared)
    ggml_tensor * mask_in        = nullptr;  // [T_prompt_max, T_prompt_max] f16 causal
    ggml_tensor * kv_idx_in      = nullptr;  // [T_prompt_max, B] i64 (idx[t,b]=t)
    ggml_tensor * last_idx_in    = nullptr;  // [1, B] i32 (real last position per utt)
    ggml_tensor * out            = nullptr;  // [B] i32 argmax of last-position logits
    ggml_tensor * logits         = nullptr;  // [vocab, B] f32
    ggml_cgraph * graph          = nullptr;

    int T_prompt_max = 0;
    int T_audio_max  = 0;
    int n_batch      = 0;
};

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const VoxtralWeights &           weights,
                                                const VoxtralHParams &           hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                int                              T_prompt_max,
                                                int                              T_audio_max,
                                                int                              n_batch,
                                                bool                             use_flash);

struct StepBuildBatched {
    ggml_tensor * input_ids_in = nullptr;  // [B] i32
    ggml_tensor * position_in  = nullptr;  // [B] i32 (RoPE position per utt)
    ggml_tensor * kv_idx_in    = nullptr;  // [1, B] i64 (KV write row per utt)
    ggml_tensor * mask_in      = nullptr;  // [max_n_kv, 1, 1, B] f16 per-row window
    ggml_tensor * out          = nullptr;  // [B] i32 argmax token id
    ggml_tensor * logits       = nullptr;  // [vocab, B] f32
    ggml_cgraph * graph        = nullptr;

    int max_n_kv = 0;
    int n_batch  = 0;
};

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const VoxtralWeights &           weights,
                                          const VoxtralHParams &           hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash);

}  // namespace transcribe::voxtral
