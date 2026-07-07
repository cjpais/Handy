// src/causal_lm/causal_lm.h - shared causal-decoder LM block math,
// KV cache, and packed gate/up MLP packing.
//
// Dense decoder-only transformer block of the Llama / Qwen3 lineage:
// pre-LN RMSNorm, GQA, NeoX rotate_half RoPE, SwiGLU MLP via packed
// gate+up, with OPTIONAL per-head Q/K RMSNorm (Qwen3) and an optional
// per-layer FFN scale (voxtral_realtime). Not Qwen3-specific: the
// optional slots cover Qwen3, Llama/Ministral, and infra-only reuse
// by Granite (KvCache / pack_gate_up / run_batched_step_loop only).
// Primitives follow the view + free-function pattern of src/sanm/ and
// src/conformer/. Helpers return raw ggml_tensor* so each family owns
// embedding/audio-injection, final-norm/lm_head, dump naming, graph
// allocation, the decode driver, and chat-template handling.

#pragma once

#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe.h"

#include <cstdint>
#include <vector>

struct transcribe_session;

namespace transcribe::causal_lm {

// Nullable-pointer projection over one causal-decoder block's weights.
// Field names match arch/qwen3_asr and arch/funasr_nano so the call-site
// builder is a struct initializer. All slots required except three
// optional ones (null = skip): attn_q_norm / attn_k_norm and ffn_scale.
struct BlockView {
    ggml_tensor * norm_attn_w   = nullptr;  // input_layernorm (RMSNorm, no bias)
    ggml_tensor * norm_ffn_w    = nullptr;  // post_attention_layernorm
    ggml_tensor * attn_q_w      = nullptr;  // [hidden, n_heads * head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_v_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_o_w      = nullptr;  // [n_heads * head_dim, hidden]
    // Per-head Q/K RMSNorm (Qwen3). Null on Llama-style decoders (e.g.
    // Voxtral's Ministral backbone); helpers skip the norm when null.
    ggml_tensor * attn_q_norm   = nullptr;  // [head_dim] per-head Q-norm, or null
    ggml_tensor * attn_k_norm   = nullptr;  // [head_dim] per-head K-norm, or null
    // Packed gate+up filled by pack_gate_up at load time; the graph runs
    // one mul_mat + ggml_swiglu instead of two mul_mats + manual silu·mul.
    ggml_tensor * ffn_gate_up_w = nullptr;  // [hidden, 2·intermediate]
    ggml_tensor * ffn_down_w    = nullptr;  // [intermediate, hidden]
    // Optional per-layer FFN-branch scale (ff_norm *= ffn_scale, broadcast
    // over the token axis). Null for standard callers; voxtral_realtime's
    // delay-conditioned adaptive-norm uses it.
    ggml_tensor * ffn_scale     = nullptr;  // [hidden] or [hidden,1], or null
};

// Per-call scalar block params.
struct BlockParams {
    int   n_heads      = 0;
    int   n_kv_heads   = 0;
    int   head_dim     = 0;
    int   max_position = 0;  // RoPE max position (passed to ggml_rope_ext)
    float rms_eps      = 0.0f;
    float rope_theta   = 0.0f;
};

// KV cache (self-attention only). Flat 1D K and V tensors sized
// [n_kv_heads · head_dim · n_ctx · n_layer]. Layout (slowest → fastest):
// layer, position, head, dim.
struct KvCache {
    ggml_tensor * self_k = nullptr;
    ggml_tensor * self_v = nullptr;

    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    int n_ctx   = 0;
    int n       = 0;  // current fill (high-water mark)
    int head    = 0;  // next write position
    int n_batch = 1;  // utterances packed along the batch axis
                      // (offline batched decode); 1 == single-shot

    void free();
};

// Allocate K/V tensors on `backend` and bind them to a fresh context.
// Returns false on failure (only F16 / F32 KV types supported).
bool kv_init(KvCache &      cache,
             ggml_backend_t backend,
             int            n_ctx,
             int            n_kv_heads,
             int            head_dim,
             int            n_layer,
             ggml_type      kv_type);

// Batched variant: allocates [n_kv_heads · head_dim · n_ctx · n_batch · n_layer]
// flat per K and V, laid out (slowest → fastest) layer, batch, position, head,
// dim. With n_batch == 1 the size and layout match kv_init. Sets cache.n_batch.
bool kv_init_batched(KvCache &      cache,
                     ggml_backend_t backend,
                     int            n_ctx,
                     int            n_kv_heads,
                     int            head_dim,
                     int            n_layer,
                     int            n_batch,
                     ggml_type      kv_type);

struct BlockOpts {
    bool use_flash = true;

    // When true, after the post-attention residual add but before the FFN,
    // slice x to the last position only (output [hidden, 1]). Used on the
    // LAST block when only the final logits are consumed (llama.cpp's
    // inp_out_ids optimization).
    bool slice_last_before_ffn = false;

    // Offline batched decode: write/read this utterance's KV in slab
    // `kv_batch_slot` of a batched cache holding `kv_n_batch` slabs. The
    // per-(layer,slot) byte offset becomes
    //   (kv_batch_slot + kv_n_batch * layer_idx) * n_ctx * kv_dim.
    // Defaults (0, 1) reproduce the single-shot offset.
    int kv_batch_slot = 0;
    int kv_n_batch    = 1;
};

// Block forward (prefill, T_seq > 1). Runs one block on `x`
// (ne = [hidden, T_seq]), writing K/V for positions [0, T_seq) at layer
// `layer_idx`. Mask is [T_seq, T_seq] f16 causal; positions are [T_seq]
// i32. Returns the post-block hidden state ([hidden, T_seq], or
// [hidden, 1] under opts.slice_last_before_ffn). Names no tensors; the
// family's prefill builder owns naming and dump-point selection.
ggml_tensor * block_prefill(ggml_context *      ctx,
                            ggml_cgraph *       gf,
                            ggml_tensor *       x,
                            const BlockView &   view,
                            const BlockParams & params,
                            KvCache &           kv_cache,
                            int                 layer_idx,
                            int                 T_seq,
                            ggml_tensor *       mask,       // [T_seq, T_seq] f16
                            ggml_tensor *       positions,  // [T_seq] i32
                            BlockOpts           opts = {});

// Block forward (step, T_seq == 1). Runs one block on a single new token,
// writing K/V for the row indexed by `kv_idx` (i64 [1]) into layer
// `layer_idx`. Reads the full [0, max_n_kv) KV window; `mask` is
// [max_n_kv, 1] f16 (zeros ≤ n_past, -inf beyond). `position` ([1] i32)
// and `kv_idx` ([1] i64) are equal at runtime but kept distinct (RoPE
// wants i32, set_rows wants i64).
ggml_tensor * block_step(ggml_context *      ctx,
                         ggml_cgraph *       gf,
                         ggml_tensor *       x,  // [hidden, 1]
                         const BlockView &   view,
                         const BlockParams & params,
                         KvCache &           kv_cache,
                         int                 layer_idx,
                         int                 max_n_kv,
                         ggml_tensor *       mask,      // [max_n_kv, 1] f16
                         ggml_tensor *       position,  // [1] i32
                         ggml_tensor *       kv_idx,    // [1] i64
                         bool                use_flash);

// Block forward (multi-position step). Like block_step but processes
// T_seq positions in one forward: writes T_seq rows at the indices in
// `kv_idx` (i64 [T_seq]) and reads the full [0, max_n_kv) window. `mask`
// is [max_n_kv, T_seq] f16 — column `c` masks the c-th query position.
// Used by the spec-decode verify pass.
ggml_tensor * block_step_n(ggml_context *      ctx,
                           ggml_cgraph *       gf,
                           ggml_tensor *       x,  // [hidden, T_seq]
                           const BlockView &   view,
                           const BlockParams & params,
                           KvCache &           kv_cache,
                           int                 layer_idx,
                           int                 T_seq,
                           int                 max_n_kv,
                           ggml_tensor *       mask,       // [max_n_kv, T_seq] f16
                           ggml_tensor *       positions,  // [T_seq] i32
                           ggml_tensor *       kv_idx,     // [T_seq] i64
                           bool                use_flash);

// Block forward (batched step). Runs one block on B new tokens
// (x = [hidden, B]), one per utterance stepping in lockstep against a
// batched KV cache (kv_init_batched, n_batch=B). The batch axis rides
// ne[2] through projection + RoPE so each utterance gets its own RoPE
// position from `position` [B]; Q is then permuted to [head_dim, 1,
// n_heads, B] for flash_attn_ext (batches on ne[3]). One batched
// ggml_set_rows writes B KV rows.
//
// Inputs:
//   x         [hidden, B]
//   max_n_kv  static KV window (full window read under per-row mask)
//   n_batch   B
//   mask      [max_n_kv, 1, 1, B] f16 — per-utterance causal+pad mask
//   position  [B] i32 — RoPE position per utterance (= that row's n_past)
//   kv_idx    [1, B] i64 — KV write row per utterance (= that row's n_past)
//
// Requires use_flash (the manual GQA path is single-shot only). Returns
// [hidden, B].
ggml_tensor * block_step_batched(ggml_context *      ctx,
                                 ggml_cgraph *       gf,
                                 ggml_tensor *       x,  // [hidden, B]
                                 const BlockView &   view,
                                 const BlockParams & params,
                                 KvCache &           kv_cache,
                                 int                 layer_idx,
                                 int                 max_n_kv,
                                 int                 n_batch,
                                 ggml_tensor *       mask,      // [max_n_kv, 1, 1, B] f16
                                 ggml_tensor *       position,  // [B] i32
                                 ggml_tensor *       kv_idx,    // [1, B] i64
                                 bool                use_flash);

// Block forward (batched prefill). Runs one block over B prompts of T
// tokens each (x = [hidden, T, B]), writing each utterance's K/V to
// positions [0, T) of its own slab (kv_init_batched, n_batch=B). Batch
// rides ne[2]; RoPE positions [T] and the causal mask [T, T] are shared.
// One batched ggml_set_rows writes B*T KV rows (idx[t,b]=t). Pad tokens
// (t >= T_prompt[b]) compute harmlessly: their KV lands in pad positions
// the step loop overwrites before attending, and the caller gathers only
// each utterance's real last-position output. Requires use_flash.
// Returns [hidden, T, B].
ggml_tensor * block_prefill_batched(ggml_context *      ctx,
                                    ggml_cgraph *       gf,
                                    ggml_tensor *       x,  // [hidden, T, B]
                                    const BlockView &   view,
                                    const BlockParams & params,
                                    KvCache &           kv_cache,
                                    int                 layer_idx,
                                    int                 T_seq,
                                    int                 n_batch,
                                    ggml_tensor *       mask,       // [T_seq, T_seq] f16 causal (shared)
                                    ggml_tensor *       positions,  // [T_seq] i32 (shared)
                                    ggml_tensor *       kv_idx,     // [T_seq, B] i64 (idx[t,b] = t)
                                    bool                use_flash);

// Load-time gate/up packing.

// One block's input pointers (gate_w, up_w) and output slot
// (gate_up_w_out, written by pack_gate_up). gate_w / up_w must already be
// allocated and bound to a backend buffer.
struct GateUpEntry {
    ggml_tensor *  gate_w;
    ggml_tensor *  up_w;
    ggml_tensor ** gate_up_w_out;
};

// Owns the ctx + backend buffer that the packed tensors live in.
// Caller (the model struct) keeps the handles alive for the lifetime
// of the model and calls `free()` on shutdown — including from the
// partial-failure path of `pack_gate_up`, which leaves the handles in
// a state safe to free.
struct PackedGateUpHandles {
    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    void free();
};

// Allocate a packed context + a [hidden, 2·intermediate] tensor per block
// on `backend`, then copy gate's bytes followed by up's bytes into each.
// Compatible with row-wise quants (Q4/Q5/Q6/Q8) because concat-along-dim-1
// is byte-concat for those types. Writes `*entries[i].gate_up_w_out` to the
// new tensor and marks the buffer GGML_BACKEND_BUFFER_USAGE_WEIGHTS.
// Returns false on alloc / size-mismatch failure; `out_handles` is left in
// a state safe to free.
bool pack_gate_up(ggml_backend_t                   backend,
                  int                              hidden,
                  int                              intermediate,
                  const std::vector<GateUpEntry> & entries,
                  PackedGateUpHandles &            out_handles,
                  const char *                     error_tag = "causal_lm");

// Batched greedy step loop (offline transcribe_run_batch decode).

// The B utterances' batched step graph, as built by a family's
// build_step_graph_batched(). Field names differ across families, so the
// caller fills this projection at the call site.
struct StepBatchedIO {
    ggml_tensor * input_ids = nullptr;  // [B] i32   — token fed each step
    ggml_tensor * positions = nullptr;  // [B] i32   — RoPE position per row
    ggml_tensor * kv_idx    = nullptr;  // [1, B] i64 — KV write row per utterance
    ggml_tensor * mask      = nullptr;  // [max_n_kv, 1, 1, B] f16 — per-row window
    ggml_tensor * argmax    = nullptr;  // [B] i32   — graph output (next token)
    ggml_cgraph * graph     = nullptr;
};

// Per-row state on entry to the step loop, i.e. just after serial prefill.
//   valid[b]    : the utterance produced a usable prefill
//   next_tok[b] : the prefill's first generated token (fed into step 0)
//   n_past[b]   : == T_prompt[b] (the prompt KV length already written)
// The caller must also have seeded generated[b] with next_tok[b].
struct StepBatchedState {
    std::vector<char>    valid;
    std::vector<int32_t> next_tok;
    std::vector<int>     n_past;
};

struct StepLoopStats {
    int     n_steps = 0;
    int64_t step_us = 0;
};

// Run the lockstep batched greedy decode. Each row steps until it emits
// `eos_id`, accumulates `max_new` generated tokens, or fills the KV window;
// each emitted token is appended to generated[b]. Finished / invalid rows keep
// stepping into their own KV slab (a no-op for live rows). Polls
// session->poll_abort() once per step. The step graph must already be built
// and allocated on `sched`. Returns TRANSCRIBE_ERR_ABORTED on abort,
// TRANSCRIBE_ERR_GGUF on a compute failure, else TRANSCRIBE_OK.
transcribe_status run_batched_step_loop(transcribe_session *                session,
                                        ggml_backend_sched_t                sched,
                                        const StepBatchedIO &               io,
                                        int                                 n_batch,
                                        int                                 max_n_kv,
                                        int32_t                             eos_id,
                                        int                                 max_new,
                                        const StepBatchedState &            state,
                                        std::vector<std::vector<int32_t>> & generated,
                                        StepLoopStats *                     stats         = nullptr,
                                        std::vector<char> *                 truncated_out = nullptr);

}  // namespace transcribe::causal_lm
