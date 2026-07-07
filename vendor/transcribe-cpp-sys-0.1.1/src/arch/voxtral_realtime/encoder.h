// arch/voxtral_realtime/encoder.h - causal RoPE audio encoder + projector.
//
// VoxtralRealtimeEncoder: a 2-layer LEFT-PAD causal Conv1d stem (GELU) then
// 32 pre-norm RMSNorm transformer blocks with NEOX RoPE (theta 1e6, head_dim
// 64), causal + sliding-window(750) attention (q/v/out bias, k none) and a
// SwiGLU/silu MLP (bias only on down). A final RMSNorm yields enc.out; the
// projector groups 4 consecutive frames and runs Linear -> GELU -> Linear.
//
// One graph processes the WHOLE clip offline: mel [n_mels, n_mel_frames] in,
// proj.out [dec_hidden, n_audio] out. positions + a host-prepared
// sliding-window-causal mask are graph inputs.

#pragma once

#include "causal_lm/causal_lm.h"
#include "ggml.h"

#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::voxtral_realtime {

struct HParams;
struct Weights;

struct EncoderDumps {
    ggml_tensor *              embedder_out = nullptr;  // conv stem output [d_model, T_enc]
    ggml_tensor *              enc_out      = nullptr;  // final RMSNorm output
    ggml_tensor *              proj_out     = nullptr;  // projector output (audio embeds)
    std::vector<ggml_tensor *> block_outs;              // per-block residual outputs
};

struct EncoderBuild {
    ggml_tensor * mel_in       = nullptr;  // [n_mels, n_mel_frames] input
    ggml_tensor * positions_in = nullptr;  // [T_enc] i32 RoPE positions
    ggml_tensor * mask_in      = nullptr;  // [T_enc, T_enc] f16 sw-causal mask
    ggml_tensor * out          = nullptr;  // proj.out [dec_hidden, n_audio]
    EncoderDumps  dumps{};
    ggml_cgraph * graph = nullptr;

    int T_enc   = 0;
    int n_audio = 0;
};

// Build the encoder+projector forward graph for `n_mel_frames` mel frames.
// The stride-2 causal conv2 yields T_enc = conv-out frames; n_audio = T_enc/4.
EncoderBuild build_encoder_graph(ggml_context *  ctx,
                                 const Weights & weights,
                                 const HParams & hp,
                                 int             n_mel_frames,
                                 bool            use_flash);

// ---------------------------------------------------------------------------
// Batched offline encoder (transcribe_run_batch fast path)
// ---------------------------------------------------------------------------
//
// Runs the audio tower batched over [B, T, C] with NO audio attention_mask:
// every clip's mel is RIGHT-padded to the batch-max `n_mel_frames` and stacked on
// ne[2]=B. The conv stem is causal (left-pad) and attention is causal +
// sliding-window, so a real frame at t only attends to <= t — the right-padding
// of shorter clips never contaminates their real [0, T_enc_b) outputs. A SINGLE
// `positions [T_enc]` and sw-causal `mask [T_enc, T_enc]` serve every row; the
// caller slices each row's real n_audio_b out of out[:, :, b]. `use_flash`
// honors the session's encoder_use_flash (so the non-flash CPU path batches too).
struct EncoderBuildBatched {
    ggml_tensor * mel_in       = nullptr;  // [n_mels, n_mel_frames, B] input
    ggml_tensor * positions_in = nullptr;  // [T_enc] i32 RoPE positions (shared)
    ggml_tensor * mask_in      = nullptr;  // [T_enc, T_enc] f16 sw-causal (shared)
    ggml_tensor * out          = nullptr;  // proj.out [dec_hidden, n_audio, B]
    ggml_cgraph * graph        = nullptr;

    int T_enc   = 0;
    int n_audio = 0;
    int n_batch = 0;
};

EncoderBuildBatched build_encoder_graph_batched(ggml_context *  ctx,
                                                const Weights & weights,
                                                const HParams & hp,
                                                int             n_mel_frames,
                                                int             n_batch,
                                                bool            use_flash);

// ---------------------------------------------------------------------------
// Incremental streaming encoder (matches the reference StaticCache mechanism)
// ---------------------------------------------------------------------------
//
// The encoder transformer runs INCREMENTALLY against an encoder StaticCache
// (sliding_window=750). Split into two graphs:
//
//   build_embedder_graph    — conv stem only: mel -> embedder frames
//                             [d_model, T_enc]. Causal convs (left-pad), so the
//                             frames are append-only/stable.
//   build_encoder_chunk_graph — INCREMENTAL: `n_new` embedder frames as queries
//                             attend to the encoder KV cache under a sliding-
//                             window-causal mask; their K/V are appended at the
//                             cache offset. Final RMSNorm + projector group-of-4.

struct EmbedderBuild {
    ggml_tensor * mel_in = nullptr;  // [n_mels, n_mel_frames] input
    ggml_tensor * out    = nullptr;  // [d_model, T_enc] embedder frames
    ggml_cgraph * graph  = nullptr;
    int           T_enc  = 0;
};

// Conv stem only (no transformer). T_enc = (n_mel_frames - 2)/2 + 1.
EmbedderBuild build_embedder_graph(ggml_context * ctx, const Weights & weights, const HParams & hp, int n_mel_frames);

// Conv stem with the streaming PADDING CACHE. Feeds only `n_new_mel` NEW mel
// frames; the conv left-context comes from host-carried caches instead of zero
// left-pad: conv1 (k3 s1, left_pad 2) ← last 2 mel frames; conv2 (k3 s2, left_pad
// 1) ← last 1 conv1-output frame. Both zero on the first chunk == whole-buffer
// left-pad. Emits M_emb = n_new_mel/2 new embedder frames (n_new_mel must be even)
// plus `cache2_out` = the chunk's last conv1-output frame for the next call.
struct EmbedderChunkBuild {
    ggml_tensor * mel_in     = nullptr;  // [n_mels, n_new_mel] new mel frames
    ggml_tensor * cache1_in  = nullptr;  // [n_mels, 2] prev 2 mel frames (conv1 left ctx)
    ggml_tensor * cache2_in  = nullptr;  // [d_model, 1] prev last conv1-out frame (conv2 left ctx)
    ggml_tensor * out        = nullptr;  // [d_model, M_emb] new embedder frames
    ggml_tensor * cache2_out = nullptr;  // [d_model, 1] new last conv1-out frame (next cache2)
    ggml_cgraph * graph      = nullptr;
    int           M_emb      = 0;
};

EmbedderChunkBuild build_embedder_chunk_graph(ggml_context *  ctx,
                                              const Weights & weights,
                                              const HParams & hp,
                                              int             n_new_mel);

struct EncoderChunkBuild {
    ggml_tensor * embed_in     = nullptr;  // [d_model, n_new] new embedder frames
    ggml_tensor * positions_in = nullptr;  // [n_new] i32 ABSOLUTE enc-frame pos
    ggml_tensor * mask_in      = nullptr;  // [read_len, n_new] f16 sw-causal
    ggml_tensor * out          = nullptr;  // proj.out audio embeds [dec_hidden, n_audio_new]
    ggml_tensor * enc_out      = nullptr;  // [d_model, n_new] final-normed enc frames
    ggml_cgraph * graph        = nullptr;
    int           n_new        = 0;
    int           read_len     = 0;
    int           n_audio_new  = 0;
};

// Build one incremental encoder chunk against the encoder KV cache ring. Writes
// the `n_new` frames' K/V into cache rows [write_slot, write_slot + n_new), then
// reads the `read_len`-row window starting at `read_start` for attention (the
// host mask enforces the 750-frame sliding window using ABSOLUTE positions). The
// caller owns the ring (compaction + slot bookkeeping). `n_new` must be a
// multiple of proj_downsample (4) so projector groups never straddle chunks.
EncoderChunkBuild build_encoder_chunk_graph(ggml_context *                   ctx,
                                            const Weights &                  weights,
                                            const HParams &                  hp,
                                            transcribe::causal_lm::KvCache & enc_kv,
                                            int                              n_new,
                                            int                              write_slot,
                                            int                              read_start,
                                            int                              read_len,
                                            bool                             use_flash);

}  // namespace transcribe::voxtral_realtime
