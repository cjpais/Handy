// arch/parakeet/encoder.h - Parakeet Conformer encoder graph builder.
//
// The .cpp builds a ggml_cgraph mirroring NeMo's Conformer encoder
// forward, reading dims from ParakeetHParams and tensor pointers from
// ParakeetWeights (both populated by the loader). Internal to
// src/arch/parakeet/; the public C ABI only sees Parakeet::run.

#pragma once

#include "ggml.h"  // ggml_type

#include <string>
#include <utility>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe::parakeet {

struct ParakeetHParams;
struct ParakeetWeights;

// Result of building one pass of the encoder graph. The caller
// allocates a backend buffer for compute_ctx, uploads mel into `mel_in`,
// runs the cgraph, then reads/dumps `out`.
//
// Layout convention:
//   - The reference uses [B, T, C] (channels-last); ggml ne is
//     fast-to-slow, so the natural encoder activation is
//     ne=[d_model, T, B=1, 1].
//   - For pre_encode (Conv2d), T_mel is W (ne[0]) and n_mels is H
//     (ne[1]); this matches the C++ MelFrontend's row-major layout, so
//     the input tensor receives the mel buffer with no transpose.
//   - After three stride-2 convs, the activation enters the conformer
//     blocks at ne=[d_model, T_enc, 1, 1] where T_enc = floor(T_mel / 8)
//     (per-conv padding/kernel formula applied three times).
// Named intermediate dump points the encoder graph builder publishes.
// Parakeet::run iterates this after graph_compute and feeds each tensor
// to transcribe::debug::dump_tensor (gated on TRANSCRIBE_DUMP_DIR). Any
// tensor stored here must also be passed to mark_tensor_for_dump() while
// building, or the scheduler may reuse its buffer before the dump pass.
struct EncoderDumps {
    // Pre-encode. ne=[d_model, T_enc, 1, 1].
    ggml_tensor * pre_encode_out    = nullptr;
    // Block 0 sub-step outputs. Each ne=[d_model, T_enc, 1, 1].
    ggml_tensor * block0_after_ff1  = nullptr;
    ggml_tensor * block0_after_attn = nullptr;
    ggml_tensor * block0_after_conv = nullptr;
    ggml_tensor * block0_after_ff2  = nullptr;
    ggml_tensor * block0_out        = nullptr;
    // Mid- and last-layer spot-check outputs. last_block_out aliases
    // final_out. Layer indices synthesize file names like
    // "enc.block.21.out" because conf::named renames in place and the
    // trailing "enc.final" rename overwrites the spot-check name.
    ggml_tensor * mid_block_out     = nullptr;
    ggml_tensor * last_block_out    = nullptr;
    int           mid_block_idx     = -1;
    int           last_block_idx    = -1;
    // Final encoder output. Aliases last_block_out (same pointer).
    ggml_tensor * final_out         = nullptr;

    // All-block output handles for the layer-by-layer divergence bisect.
    // Populated unconditionally; mark_tensor_for_dump is only called on
    // each entry when TRANSCRIBE_DUMP_ALL_BLOCKS is set, so this costs
    // only a vector allocation in the normal case.
    std::vector<ggml_tensor *> all_block_outs;

    // Post-prompt-MLP encoder output (multilingual variants only).
    // Populated when hp.has_prompt is true; mirrors NeMo's
    // EncDecRNNTBPEModelWithPrompt: prompt_kernel(concat(enc, one_hot))
    // -> [T_enc, d_model] that the RNN-T joint consumes in place of
    // the raw encoder output. Null on variants without the prompt path.
    ggml_tensor * prompted_out = nullptr;

    // Sub-block intermediates for blocks listed in
    // TRANSCRIBE_DUMP_SUB_BLOCKS. Each entry is (dump_name, tensor)
    // where dump_name is the on-disk file stem
    // (e.g. "enc.block.12.ff1"). Populated only when the env var is
    // set; the dump pass in model.cpp iterates this and feeds each
    // entry to transcribe::debug::dump_tensor.
    std::vector<std::pair<std::string, ggml_tensor *>> sub_block_dumps;
};

struct EncoderBuild {
    // Input handle, ne=[T_mel, n_mels, 1, 1] f32. Caller fills via
    // ggml_backend_tensor_set with the row-major [n_mels, n_frames]
    // mel buffer from MelFrontend::compute (the memory layout is
    // identical because MelFrontend's row-major matches ggml's
    // ne[0]=fastest convention for this 2-D tensor).
    ggml_tensor * mel_in = nullptr;

    // Sinusoidal positional embedding handle, ne=[d_model, 2*T_enc-1, 1, 1].
    // The driver computes the buffer host-side from T_enc (read from
    // out->ne[1] after build) and uploads via ggml_backend_tensor_set.
    ggml_tensor * pos_emb_in = nullptr;

    // ChunkedLimited attention mask, ne=[T_enc, T_enc, 1, 1] f32. Null
    // for every variant on the regular / local-attention path; populated
    // only when the hparams declare att_context_style="chunked_limited"
    // (today: nemotron-speech-streaming-en-0.6b). Driver fills with 0
    // on allowed (q, k) pairs and -INF outside, then ggml broadcasts
    // across heads inside rel_pos_mhsa.
    ggml_tensor * chunked_mask_in = nullptr;

    // Buffered-streaming conv valid-frame mask, ne=[T_enc, 1, 1, 1] f32.
    // Null unless the buffered path has pre_encode overhang frames that
    // NeMo would expose via pad_mask to every Conformer conv module. Also
    // reused for variable-length batching, where it is sized
    // ne=[T_enc, 1, n_batch, 1] (per-utterance valid-frame mask).
    ggml_tensor * conv_pad_mask_in = nullptr;

    // Variable-length batch attention key-padding mask, ne=[T_enc, 1, 1,
    // n_batch] f32 (0 on real keys, -INF on padded keys). Null unless
    // build_encoder_graph was called with batch_var_len and n_batch > 1.
    // The driver fills it host-side after the compute buffer is allocated.
    ggml_tensor * attn_pad_mask_in = nullptr;

    // Per-utterance prompt one-hot vector, ne=[num_prompts, n_batch] f32.
    // Null unless hp.has_prompt is true. The driver fills this with a
    // one-hot (1.0 at prompt_id, 0.0 elsewhere) per utterance before
    // graph_compute; the prompt MLP at the encoder tail consumes it via
    // a host-precomputed broadcast across time frames (no in-graph
    // one_hot op needed).
    ggml_tensor * prompt_one_hot_in = nullptr;

    // Variable-length batch pre-encode valid-frame masks (NeMo masked
    // subsampling), one per ReLU stage, ne=[1, H_stage, 1, n_batch] f32
    // (1 on valid time frames, 0 on padded). Null unless variable-length
    // batching on a non-causal pre-encode. The driver fills them from the
    // per-utterance valid length downsampled to each stage.
    ggml_tensor * pre_encode_mask_s1_in = nullptr;  // after relu0
    ggml_tensor * pre_encode_mask_s2_in = nullptr;  // after relu3
    ggml_tensor * pre_encode_mask_s3_in = nullptr;  // after relu6

    // Encoder forward output, ne=[d_model, T_enc, 1, 1] f32. Equal
    // to dumps.final_out; provided as a separate field so callers
    // that don't care about intermediates can ignore the dumps
    // struct entirely.
    ggml_tensor * out = nullptr;

    // Named intermediate handles for the dump harness. Any field
    // that's nullptr is "not yet wired in this sub-stage" and the
    // driver simply skips it.
    EncoderDumps dumps{};

    // The forward graph. ggml_build_forward_expand has been called
    // with `out`.
    ggml_cgraph * graph = nullptr;
};

// Build a fresh encoder forward graph in `compute_ctx`. The context
// must have been created with `no_alloc=true` so the tensors carry
// metadata only; the caller allocates a backend buffer for them
// after this returns. `n_mel_frames` is the time dimension of the
// mel input (the second dim of the row-major [n_mels, n_frames]
// MelFrontend output) and shapes the input handle accordingly.
//
// Returns an EncoderBuild with all tensor handles populated. On failure
// (insufficient mem_size, weights/hparams mismatch, degenerate frame
// count) the struct has nullptr fields and a diagnostic is logged.
// kv_type: GGML type for K/V activations in flash attention;
// GGML_TYPE_COUNT means "auto" (f16 for quantized weights, f32 for f32).
// backend_name: primary backend name ("MTL0", "Vulkan0", "CPU") for
// auto-detecting conv strategy; env vars override.
//
// Runtime override for chunked_limited_with_rc streaming: pass a
// non-null BufferedStreamMaskOverride with (L, C, R) in encoder frames
// (buffered streaming on parakeet-unified-en-0.6b) to allocate a
// chunked_mask_in tensor and route blocks through the ChunkedLimited
// path; the caller fills the mask host-side. Null keeps offline full
// attention (att_context_size [-1, -1]).
struct BufferedStreamMaskOverride {
    int left_frames;
    int chunk_frames;
    int right_frames;
    int valid_frames;
};

// n_batch: number of utterances packed along the encoder batch axis (B at
// the activation's ne[2]). 1 is the single-shot path and is byte-identical
// to the pre-batch graph. > 1 (offline transcribe_run_batch) requires every
// packed utterance to share n_mel_frames (same-length batch); the mel_in
// handle becomes ne=[n_mel_frames, n_mels, 1, n_batch] and `out` becomes
// ne=[d_model, T_enc, n_batch]. Variable-length batching pads to a common
// n_mel_frames and masks the overhang — that masking is the caller's job.
EncoderBuild build_encoder_graph(ggml_context *                     compute_ctx,
                                 const ParakeetWeights &            weights,
                                 const ParakeetHParams &            hp,
                                 int                                n_mel_frames,
                                 ggml_type                          kv_type       = GGML_TYPE_COUNT,
                                 const char *                       backend_name  = "",
                                 const BufferedStreamMaskOverride * buf_mask      = nullptr,
                                 int                                n_batch       = 1,
                                 // When true and n_batch > 1, allocate the
                                 // variable-length batch masks (attn_pad_mask_in
                                 // + conv_pad_mask_in sized for the batch) and
                                 // wire them into every conformer block. The
                                 // driver fills them from per-utterance lengths.
                                 bool                               batch_var_len = false);

// Per-layer streaming cache I/O for the streaming encoder graph.
// The inputs are persistent backend tensors (allocated outside the
// per-call compute_ctx) holding the previous chunk's caches; the
// outputs are fresh tensors created inside compute_ctx that the graph
// fills via ggml_cpy and the driver reads back into the persistent
// caches after graph_compute. Both vectors are sized to n_layers.
struct StreamingEncoderCacheIO {
    // Inputs (from persistent backend buffer; one tensor per layer).
    std::vector<ggml_tensor *> channel_in;
    std::vector<ggml_tensor *> time_in;
    // Outputs (fresh in compute_ctx; per-layer). Filled in by
    // build_encoder_graph_streaming so the caller can copy back.
    std::vector<ggml_tensor *> channel_out;
    std::vector<ggml_tensor *> time_out;

    // Optional KV cache (driver-owned persistent tensors, one per layer,
    // [d_model, T_cache] each): pre-projected attention keys/values
    // replacing the last_channel recompute. When all four vectors are
    // sized to n_layers and the builder enables the KV path, blocks
    // consume k_in/v_in, emit k_out/v_out, and leave channel_out null.
    std::vector<ggml_tensor *> k_in;
    std::vector<ggml_tensor *> v_in;
    std::vector<ggml_tensor *> k_out;
    std::vector<ggml_tensor *> v_out;

    // Optional rel-pos projection memoization (driver-owned persistent
    // tensors, one per layer, [head_dim, pos_len, n_head, 1]). pos_emb is
    // a pure function of chunk geometry, so attn_pos_w @ pos_emb is
    // identical for every chunk with the same pos_len. When pos_proj_len
    // matches this build's pos_len, blocks consume pos_proj[i] directly
    // and the graph references no pos_emb input (eb.pos_emb_in null); on
    // mismatch blocks compute the projection inline and the driver
    // refills the cache (see ensure_pos_proj_cache in model.cpp).
    std::vector<ggml_tensor *> pos_proj;
    int                        pos_proj_len = -1;
};

// Build a cache-aware streaming encoder graph. Same overall topology
// as build_encoder_graph but:
//
//   - The mel input is a chunk (chunk_size mel frames), typically
//     112 for nemotron-streaming subsequent chunks (= 9 history
//     prepend + 103 new).
//   - After pre_encode, the first `drop_extra_pre_encoded` encoder
//     frames are sliced off (NeMo's mechanism to align the overlap
//     after the 8× subsample).
//   - The per-block self-attention runs on T_virtual =
//     T_cache + T_q_new positions (via prepend of cache_last_channel)
//     and the output is sliced back to the last T_q_new rows.
//   - The per-block conv module replaces its zero left-pad with the
//     previous chunk's cache_last_time.
//   - Each block emits two cache writes (channel + time) via
//     ggml_cpy into fresh tensors that the driver reads back.
//
// pos_emb and chunked_mask are sized for T_virtual (the caller is
// responsible for filling them; this builder only allocates the
// input handles). The encoder output `eb.out` has ne = [d_model,
// T_q_new, 1, 1] (the new-chunk frames only).
//
// `cache_io.channel_in` and `cache_io.time_in` MUST be pre-populated
// with per-layer tensor handles from the persistent cache buffer.
// `cache_io.channel_out` and `cache_io.time_out` are populated by
// this function.
EncoderBuild build_encoder_graph_streaming(ggml_context *            compute_ctx,
                                           const ParakeetWeights &   weights,
                                           const ParakeetHParams &   hp,
                                           int                       n_mel_chunk_frames,
                                           int                       drop_extra_pre_encoded,
                                           StreamingEncoderCacheIO & cache_io,
                                           ggml_type                 kv_type      = GGML_TYPE_COUNT,
                                           const char *              backend_name = "");

}  // namespace transcribe::parakeet
