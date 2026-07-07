// arch/parakeet/parakeet.h - Parakeet-family internal model and context
// types: the concrete classes deriving from transcribe_model /
// transcribe_session. Internal to the Parakeet sources (also visible to
// tests/transcribe_tokenizer_smoke via the test target's PRIVATE include
// path). The central dispatcher talks to the family only through the Arch
// trait and the base classes.

#pragma once

#include "decoder.h"
#include "transcribe-backend.h"
#include "transcribe-mel.h"
#include "transcribe-model.h"
#include "transcribe-session.h"
#include "transcribe-tokenizer.h"
#include "weights.h"

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_backend;
struct ggml_backend_buffer;
struct ggml_backend_sched;
typedef struct ggml_backend *        ggml_backend_t;
typedef struct ggml_backend_buffer * ggml_backend_buffer_t;
typedef struct ggml_backend_sched *  ggml_backend_sched_t;

namespace transcribe::parakeet {

// Host-side attention-mask helpers for the streaming paths. Declared
// here so the per-mask unit tests can call them without a ggml backend
// or a GGUF on disk.
//
// chunked_limited_with_rc mask (buffered streaming). Mirrors NeMo
// ConformerEncoder._create_masks:
//   c_q = q / C
//   window_start = max(0, c_q * C - L)
//   window_end   = min(T - 1, c_q * C + C - 1 + R)
//   mask[q, k]   = 0 if window_start <= k <= window_end else -INF
// Output is row-major [T_q, T_k] with T_q == T_k == T (>= T*T floats);
// broadcasts over heads in rel_pos_mhsa.
//
// `pad_length` (optional) folds in NeMo's conv-overhang pad_mask: cells
// where q >= pad_length OR k >= pad_length are masked regardless of the
// window. pad_length = T disables it (offline). For buffered streaming
// pass effective_T = ctx.total / encoder_frame2audio_samples — the conv
// frontend may produce T_enc > effective_T, and those trailing frames
// must be masked or they contaminate valid frames' attention scores.
void compute_chunked_limited_with_rc_mask(float * out_buf,
                                          int     T,
                                          int     left_context_frames,
                                          int     chunk_size_frames,
                                          int     right_context_frames,
                                          int     pad_length);

// Family defaults — applied before transcribe::read_capability_kv runs
// (KV present overrides, KV absent leaves the default). Defined in
// capabilities.cpp. There is no per-variant resolver: variant identity
// is descriptive metadata; v2/v3 differences are expressed as
// stt.capability.* / general.languages KV.
void apply_family_invariants(transcribe_model & model);

// Concrete model. Owns ctx_meta, which holds every weight tensor's
// data buffer; the destructor frees it, invalidating every borrowed
// ggml_tensor* in `weights`.
struct ParakeetModel final : public transcribe_model {
    Tokenizer       tok;
    ParakeetHParams hparams;
    ParakeetWeights weights;
    ggml_context *  ctx_meta = nullptr;

    // Runtime backend plan, resolved at load() via
    // load_common::init_backends (see transcribe-backend.h). plan's
    // scheduler_list owns the backends; the destructor frees them in
    // reverse order.
    transcribe::BackendPlan plan;
    ggml_backend_buffer_t   backend_buffer = nullptr;

    // Fused BN parameters live in a separate ggml context + buffer,
    // computed at load time from the raw BN tensors. Freed in dtor.
    ggml_context *        bn_fused_ctx    = nullptr;
    ggml_backend_buffer_t bn_fused_buffer = nullptr;

    // On a CPU primary backend, the conformer 1×1 pointwise conv weights
    // are dequantized from F16 to F32 at load: CPUs without native F16
    // arithmetic (Zen 2 and earlier) pay a per-matmul F16→F32 upconvert
    // that erases the bandwidth win, so promoting once at load is cheaper.
    // No-op on GPU backends and on non-CPU primary backends. The F16
    // source tensors stay resident (ggml can't free individual slots);
    // the weight slot is repointed at the F32 copy.
    ggml_context *        conv_pw_f32_ctx    = nullptr;
    ggml_backend_buffer_t conv_pw_f32_buffer = nullptr;

    // Mel front-end, constructed once at load() from the hparams
    // (precomputes Hann window + Slaney mel filterbank).
    // const-after-ctor and thread-safe, so shared by every context.
    // std::optional because MelFrontend has no default constructor.
    std::optional<transcribe::MelFrontend> mel;

    // Host mirror of the predictor + joint weights, populated at load()
    // via build_host_decoder_weights. The decoder runs on host (see
    // decoder.h); the const-after-load mirror lets every context share it
    // without a per-run backend readback.
    HostDecoderWeights host_decoder;

    ParakeetModel() = default;
    ~ParakeetModel() override;

    const transcribe::Tokenizer * tokenizer() const override { return &tok; }
};

// Per-context streaming encoder state, allocated lazily on the first
// stream_begin for ChunkedLimited variants. Mirrors NeMo's
// get_initial_cache_state tuple. Layout cheat sheet:
//   cache_last_channel  [d_model, T_cache, n_layer] f32 — per-layer
//     post-attn-LN cache, concat-prepended to x_norm before Q/K/V.
//   cache_last_time     [k_minus_1, d_model, n_layer] f32 — per-layer
//     post-pw1+GLU cache, replaces the depthwise conv's left zero-pad.
//   cache_last_channel_len  scalar — valid frames (0..T_cache); drives
//     the streaming-mask offset until the cache fills.
// Sizes for nemotron-speech-streaming-en-0.6b at [70,13]: T_cache=70,
// k_minus_1=8, n_layer=24, d_model=1024 (~7.5 MB total).
struct ParakeetStreamingCaches {
    ggml_context *        ctx    = nullptr;
    ggml_backend_buffer_t buffer = nullptr;

    // Per-layer cache tensors. Shapes:
    //   last_channel[i] = ne[d_model, T_cache, 1, 1]
    //   last_time[i]    = ne[k_minus_1, d_model, 1, 1]
    std::vector<ggml_tensor *> last_channel;
    std::vector<ggml_tensor *> last_time;

    // KV-cache variant of last_channel: per-layer pre-projected K/V
    // (bias included) over the T_cache window, ne[d_model, T_cache, 1, 1]
    // each. Equivalent to recomputing K/V from last_channel every chunk
    // but the per-chunk projections run on T_q_new columns instead of
    // T_cache + T_q_new. The default graph consumes these; an active dump
    // harness falls back to the channel recompute path. Cleared at
    // stream_begin.
    std::vector<ggml_tensor *> last_k;
    std::vector<ggml_tensor *> last_v;

    // Rel-pos projection memoization: per-layer
    // [head_dim, pos_proj_len, n_head, 1] f32 tensors holding
    // attn_pos_w @ pos_emb. Geometry-keyed (pos_proj_len), so it survives
    // across chunks and streams; rebuilt by ensure_pos_proj_cache on a
    // pos_len change. Own ctx + buffer (size varies with geometry).
    ggml_context *             pos_proj_ctx = nullptr;
    ggml_backend_buffer_t      pos_proj_buf = nullptr;
    std::vector<ggml_tensor *> pos_proj;
    int                        pos_proj_len = -1;

    int32_t channel_len = 0;

    // Absolute mel-frame cursor across the whole stream, anchoring
    // token/segment timestamps to original-audio time even when chunks
    // vary in size. Counts mel frames consumed (minus history prepend).
    int64_t mel_frames_consumed = 0;

    // Absolute sample index of stream_pcm_buffer[0]. The buffer is a
    // sliding window: prefix samples no longer needed by any unemitted
    // frame's STFT window are trimmed (hop-aligned), bounding per-feed mel
    // cost to O(new audio). Always a hop_length multiple so mel-frame
    // indexing translates cleanly between absolute and buffer-relative.
    int64_t pcm_start_sample = 0;

    // Streaming geometry, resolved at stream_begin from ParakeetHParams +
    // the caller-selected att_context_right. Constant across chunks within
    // a stream. First- vs subsequent-chunk geometry differ (NeMo's
    // setup_streaming_params); the driver picks the pair via
    // is_first_chunk. For nemotron-speech-streaming-en at R=13:
    //   chunk_size_first=105, chunk_size_subsequent=112,
    //   mel_fed_first=105, mel_fed_subsequent=121 (9 cache + 112 new),
    //   drop_extra_first=0, drop_extra_subsequent=2.
    int  att_context_right     = 0;
    int  att_context_left      = 0;
    int  chunk_size_first      = 0;
    int  chunk_size_subsequent = 0;
    int  mel_fed_first         = 0;
    int  mel_fed_subsequent    = 0;
    int  drop_extra_first      = 0;
    int  drop_extra_subsequent = 0;
    // Whether the next emit_streaming_chunk is the FIRST in this stream.
    bool is_first_chunk        = true;

    // Per-stream chunk counter (NeMo's step_num). Resets at stream_begin,
    // increments per emit; indexes dump filenames.
    int chunk_step = 0;

    bool initialized = false;
};

// Per-context streaming RNN-T decoder state, owned on the context so it
// carries across stream_feed calls. Reset at each stream_begin to a fresh
// start-of-sequence state.
struct ParakeetStreamingDecoderState {
    // LSTM (h, c) per layer; sized at stream_begin.
    LstmState lstm_state;

    // Last emitted non-blank token id (predictor input); -1 = SOS.
    int32_t prev_token_id = -1;

    // Absolute encoder-frame offset for converting per-chunk step_at_emit
    // into stream-wide frame indices.
    int64_t frame_offset = 0;

    bool initialized = false;
};

// Concrete context. Owns a per-call compute context and a persistent
// multi-backend scheduler that dispatches encoder graph ops to the best
// available backend.
struct ParakeetSession final : public transcribe_session {
    // Compute context: cgraph + intermediate tensor metadata. no_alloc;
    // data lives in sched-managed buffers. Reset each run().
    ggml_context * compute_ctx = nullptr;

    // Multi-backend scheduler. Persists across calls; manages compute
    // buffer allocation and reuses buffers when topology is unchanged.
    ggml_backend_sched_t sched = nullptr;

    // Encoder forward output, borrowed into compute_ctx; invalidated when
    // compute_ctx is reset next run().
    ggml_tensor * encoder_out = nullptr;

    // Per-context scratch reused across runs.
    std::vector<float>    mel_buf;
    std::vector<float>    pos_buf;
    std::vector<float>    pos_div_term;
    std::vector<float>    enc_host;
    std::vector<TdtToken> raw_tokens;

    // Per-call timings and the TDT decode result (tokens / words /
    // segments / full_text / result_kind / has_result) live on the base
    // transcribe_session; no per-family copies here.

    // ---- streaming-of-whole state ----
    //
    // stream_begin captures run_params; stream_feed appends PCM;
    // stream_finalize drains the buffer through the inference helper;
    // stream_reset clears (keeping capacity). NOT touched by
    // clear_result (the family owns its per-utterance audio scratch).
    std::vector<float>    stream_pcm_buffer;
    transcribe_run_params stream_run_params{};

    // ---- incremental streaming state (cache-aware) ----
    //
    // Allocated on the first stream_begin for ChunkedLimited variants,
    // zeroed at each stream_begin, persists across feed/finalize within
    // an utterance, freed in the dtor.
    ParakeetStreamingCaches       stream_caches;
    ParakeetStreamingDecoderState stream_dec_state;

    // ---- Buffered streaming state (parakeet-unified-en-0.6b) ----
    //
    // Mirrors NeMo's StreamingBatchedAudioBuffer + reference inference
    // loop. Variable-stride: step 0 consumes samples_chunk + samples_right
    // of new audio; subsequent feeds consume samples_chunk; finalize's
    // last step consumes the rest (chunk slot absorbs the trailing right
    // context, no zero-pad). The buf_* geometry reflects the active
    // (L, C, R) tuple; the ctx_* fields track the buffer's internal
    // ContextSize, updated per-chunk via buf_ctx_add_frames (NeMo's
    // add_frames_get_removed_). The RNN-T state rides on stream_dec_state
    // (same predictor/joint path, no per-layer encoder cache).
    int32_t buf_left_frames     = 0;  // L (expected)
    int32_t buf_chunk_frames    = 0;  // C
    int32_t buf_right_frames    = 0;  // R
    int32_t buf_samples_left    = 0;  // L * encoder_frame2audio_samples
    int32_t buf_samples_chunk   = 0;
    int32_t buf_samples_right   = 0;
    // Next sample index in stream_pcm_buffer to feed (= NeMo's
    // `left_sample` at the start of the next step). 0 at stream_begin;
    // advances by num_new_samples after each emit.
    int64_t buf_next_audio_read = 0;
    // Buffer's internal ContextSize. (0,0,0) at stream_begin; ramps up
    // as audio arrives until it saturates at (samples_left, chunk, right).
    int64_t buf_ctx_left        = 0;
    int64_t buf_ctx_chunk       = 0;
    int64_t buf_ctx_right       = 0;
    // Whether step 0 (the initial fill of samples_chunk+samples_right)
    // has run. False at stream_begin; first emit flips it.
    bool    buf_initialized     = false;
    // Per-stream chunk step counter (== NeMo's `step_num`). Used for
    // per-chunk dump file naming.
    int32_t buf_chunk_step      = 0;
    bool    buf_active          = false;

    ParakeetSession() = default;
    ~ParakeetSession() override;
};

}  // namespace transcribe::parakeet
