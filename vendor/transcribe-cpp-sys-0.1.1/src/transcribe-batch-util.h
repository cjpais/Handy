// transcribe-batch-util.h - shared helpers for the offline batch path
// (transcribe_run_batch family run_batch() hooks).
//
// Per-utterance feature extraction (mel / kaldi-fbank / LFR) is pure host
// code with no cross-utterance state, so it is embarrassingly parallel
// across the batch — and often the dominant wall cost when the encoder
// runs on a fast accelerator. Families supply the per-index work as a
// callable; this header owns the parallel-dispatch boilerplate.

#pragma once

#include "transcribe.h"

#include <cstddef>
#include <cstdint>
#include <functional>
#include <vector>

struct ggml_tensor;
struct ggml_cgraph;
struct transcribe_session;
typedef struct ggml_backend_sched * ggml_backend_sched_t;

namespace transcribe {

// Invoke `work(i)` for every i in [0, n) across up to `n_threads` worker
// threads (clamped to [1, n]; n_threads <= 0 means hardware_concurrency()).
// `work` MUST be reentrant: each index runs on one thread with no shared
// mutable state (write per-index outputs into index-disjoint storage).
// Returns true iff every invocation returned true; a false return does not
// stop the other indices (the caller handles partial failure — typically by
// falling back to the per-utterance path).
bool parallel_for_all(int n, int n_threads, const std::function<bool(int)> & work);

// Default CPU thread count for the ggml backends and the host parallel-for
// when the caller passes n_threads <= 0. Counts the CPUs the process may
// actually run on (the affinity mask via sched_getaffinity on Linux /
// GetProcessAffinityMask on Windows), NOT the host's total core count, then
// clamps to [1, cap]. This matters under a constrained scheduler (taskset,
// cpuset, a CI runner pinned to N vCPUs): hardware_concurrency() reports the
// full host, so a `min(cap, hardware_concurrency())` default oversubscribes
// the usable cores and ggml's spin-wait barriers livelock.
// CAVEAT: the affinity mask reflects taskset/cpuset but NOT a CFS bandwidth
// quota (`docker --cpus=N` without --cpuset) — that case still over-counts.
// A cap <= 0 disables the clamp (use all usable CPUs).
int default_n_threads(int cap = 8);

// Resolve a CPU thread count and apply it to every CPU/BLAS backend in `sched`
// via ggml_backend_set_n_threads. `requested <= 0` means default_n_threads().
// GPU backends don't expose the setter and are skipped; a null sched is a
// no-op. Returns the resolved count for reuse (e.g. the host parallel-for).
// The ONE place archs configure scheduler threads — call it instead of hand-
// rolling the loop so the affinity-aware default is never forgotten.
int configure_sched_n_threads(ggml_backend_sched_t sched, int requested);

// ---------------------------------------------------------------------------
// Encoder-batch scaffolding (the run_batch() fast-path recipe)
//
// Every batched-encoder family runs the same host-side steps around its
// encoder graph: pack+pad each utterance's features into one batch slab, build
// per-utterance padding masks, then host-slice the shared encoder output and
// decode each utterance. These helpers own the parts that are identical across
// families. They no-op cleanly on a null tensor so a caller can pass a mask the
// graph did not allocate.
// ---------------------------------------------------------------------------

// Pack per-utterance channel-major features into one batched slab and zero-pad
// each along time. `src[b]` is channel-major [n_ch, lens[b]] (element (c,t) at
// src[c*lens[b] + t]); `dst` is sized n_ch * T_max * n with slab b laid out as
// [T_max, n_ch] (element (t,c) at (b*n_ch + c)*T_max + t), matching the
// conformer mel_in tensor ne = [T_max, n_ch, 1, B]. Padded tail left zero.
void pack_pad_channel_major(std::vector<float> &                    dst,
                            const std::vector<std::vector<float>> & src,
                            const std::vector<int> &                lens,
                            int                                     n_ch,
                            int                                     T_max);

// Fill an attention key-padding mask (f32) of logical shape [T, .., n] with the
// host ordering index b*T + k: 0.0 on real keys (k < real_lens[b]), -inf on
// padded keys. No-op when `mask` is null. The element order is identical for
// the conformer [T,1,1,n] and SAN-M [T,1,1,n] key-padding tensors.
void fill_keypad_mask(ggml_tensor * mask, const std::vector<int> & real_lens, int T, int n);

// Fill a conv valid-frame mask (f32) with host ordering index b*T + t: 1.0 on
// real frames (t < real_lens[b]), 0.0 on padded. No-op when `mask` is null.
// The b*T + t order matches both the conformer [T,1,n,1] and SAN-M [1,T,n]
// valid-frame tensors.
void fill_valid_frame_mask(ggml_tensor * mask, const std::vector<int> & real_lens, int T, int n);

// Per-utterance decode loop for the batched-encoder fast path. For each
// utterance b in [0, n): polls abort, resets the scratch result slot, then
// calls `decode_fn(b, host_buf + b*utt_elems)`. Each result is snapshotted into
// session->batch_results with mel/encode timing set to the per-utterance
// amortization of the shared encode + total mel cost (so per-utt timings sum to
// the real batch wall cost); decode_fn's return is the utterance's status.
// Returns TRANSCRIBE_ERR_ABORTED if abort fires between utterances, else OK.
transcribe_status decode_batch_slices(transcribe_session * session,
                                      int                  n,
                                      const float *        host_buf,
                                      std::size_t          utt_elems,
                                      int64_t              total_encode_us,
                                      int64_t              total_mel_us,
                                      const std::function<transcribe_status(int b, const float * slice)> & decode_fn);

// ---------------------------------------------------------------------------
// Batched encoder-decoder greedy step loop (cohere / canary / moonshine)
//
// Shared decode driver: a uniform prompt feed of `prompt_len` lockstep steps
// over `prompt_ids`, then greedy generation until every row hits eos_id /
// max_new / the KV window. Self-attention keys are masked per row as positions
// fill; the KV read-window grows on demand (rebuilding the step graph) so short
// batches don't pay for the full n_ctx. moonshine is the degenerate no-growth
// case (init_window == max_n_kv, single decoder_start token). The cross-
// attention mask is static and uploaded by the family's rebuild callback.
// ---------------------------------------------------------------------------

// The B-utterance batched step graph as built by a family's
// build_step_graph_batched(). Field names differ across families, so the
// rebuild callback fills this projection.
struct EncDecStepIO {
    ggml_tensor * token_ids = nullptr;  // [B] i32
    ggml_tensor * pos_ids   = nullptr;  // [B] i32
    ggml_tensor * kv_idx    = nullptr;  // [B] i64
    ggml_tensor * self_mask = nullptr;  // [win, 1, 1, B] f16
    ggml_tensor * argmax    = nullptr;  // [B] i32 — graph output (next token)
    ggml_cgraph * graph     = nullptr;
};

// Build (or rebuild) the step graph for self-attention window `win`: allocate a
// fresh compute graph, build it, reset+alloc the scheduler, upload the static
// cross-attention mask, and fill `io`. Returns false on any failure. Called once
// at the initial window and again whenever the window must grow.
using EncDecRebuildFn = std::function<bool(int win, EncDecStepIO & io)>;

// Run the shared greedy enc-dec step loop. Feeds `prompt_ids[0..prompt_len)` as
// uniform lockstep tokens, then generates until each row emits eos_id, the batch
// reaches `max_new` produced tokens, or the position fills `max_n_kv`. Manages
// the self-attention key mask and dynamic window growth (via `rebuild`), and
// appends generated tokens to generated[b] (invalid rows are skipped, finished
// rows keep stepping into their own KV slab). Polls session->poll_abort() each
// step. Returns TRANSCRIBE_ERR_ABORTED / TRANSCRIBE_ERR_GGUF / TRANSCRIBE_OK;
// *n_steps_out (if non-null) receives the number of compute steps run.
//
// truncated_out (if non-null) is sized to n_batch and set per row: 1 when that
// (valid) row hit the generation budget (max_new) or the context window
// (max_n_kv) BEFORE emitting eos_id (transcript truncated), else 0. Lets a
// family report per-utterance TRANSCRIBE_ERR_OUTPUT_TRUNCATED from run_batch.
transcribe_status run_batched_encdec_step_loop(transcribe_session *                session,
                                               ggml_backend_sched_t                sched,
                                               const EncDecRebuildFn &             rebuild,
                                               const std::vector<int32_t> &        prompt_ids,
                                               int                                 prompt_len,
                                               int                                 init_window,
                                               int                                 max_new,
                                               int                                 max_n_kv,
                                               int32_t                             eos_id,
                                               int                                 n_batch,
                                               const std::vector<char> &           valid,
                                               std::vector<std::vector<int32_t>> & generated,
                                               int *                               n_steps_out   = nullptr,
                                               std::vector<char> *                 truncated_out = nullptr);

}  // namespace transcribe
