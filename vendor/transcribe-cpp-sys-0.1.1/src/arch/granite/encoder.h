// arch/granite/encoder.h - Granite Speech audio encoder (Conformer).
//
// Reference: GraniteSpeechCTCEncoder + GraniteSpeechConformerBlock in
// transformers/models/granite_speech/modeling_granite_speech.py.
//
// Forward shape per block:
//   x = 0.5 * ff1(x) + x                 (macaron half FFN, SiLU)
//   x = shaw_block_attn(x, dists) + x    (block-local attention with
//                                         context_size=200 blocks and a
//                                         Shaw learned-position bias)
//   x = conv_module(x) + x               (LN → pw expand → GLU → dw → BN
//                                         → SiLU → pw contract; reuses
//                                         transcribe::conformer::conv_module)
//   x = 0.5 * ff2(x) + x                 (macaron half FFN, SiLU)
//   x = post_norm(x)                     (LayerNorm with bias)
//
// Self-conditioned CTC bypass at layer N/2 - 1 (0-indexed): after the
// `num_layers // 2`th block (idx==8 1-indexed for n=16) runs, project
// to ctc_proj (1024→348), softmax over the channel axis, project back
// via ctc_bypass (348→1024), add to the residual.
//
// This header is INTERNAL to src/arch/granite/.

#pragma once

#include "ggml.h"
#include "weights.h"

#include <cstdint>
#include <vector>

struct ggml_context;
struct ggml_tensor;
struct ggml_cgraph;

namespace transcribe {
class MelFrontend;
}

namespace transcribe::granite {

// Host-side mel preprocessing.

// Run the reference MelSpectrogram (via transcribe::MelFrontend in
// whisper-mode + htk filterbank + reflect-pad + hann_periodic window),
// drop the trailing odd frame, and stack pairs of consecutive mel
// frames into 160-dim rows. Mirrors
// GraniteSpeechFeatureExtractor._extract_mel_spectrograms exactly.
//
// Output: out_mel laid out as `[T_enc, 160]` row-major (T_enc =
// (n_frames_after_whisper_norm // 2)).
transcribe_status compute_mel_encoder_input(const transcribe::MelFrontend & mel,
                                            const float *                   pcm,
                                            int                             n_samples,
                                            int                             n_threads,
                                            std::vector<float> &            out_mel,
                                            int &                           out_t_enc);

// Graph construction.

struct EncoderBuild {
    // Graph inputs (caller uploads at compute time).
    ggml_tensor * mel_in          = nullptr;  // [input_dim, T_enc]
    ggml_tensor * attention_dists = nullptr;  // [context_size, context_size]
                                              //  int32, Shaw bias indices
    ggml_tensor * last_block_mask = nullptr;  // [context_size, context_size]
                                              //  f32 additive mask, all
                                              //  zeros except final-block
                                              //  pad positions = -INF
    // Zero-pad tile for attention's internal T_enc → T_pad expansion.
    // Allocated only when T_enc is not a multiple of context_size;
    // shape [hidden, T_pad - T_enc] f32. Caller uploads explicit zeros.
    ggml_tensor * zero_pad        = nullptr;
    // Graph output (encoder's [hidden, T_enc] tensor).
    ggml_tensor * out             = nullptr;

    ggml_cgraph * graph = nullptr;

    // Dump points (exposed so model.cpp can wire them up).
    struct Dumps {
        ggml_tensor * input_linear_out = nullptr;  // [hidden, T_enc]
        ggml_tensor * block_0_out      = nullptr;
        ggml_tensor * block_mid_out    = nullptr;  // block (n_layers/2 - 1) post-bypass
        ggml_tensor * block_last_out   = nullptr;  // block (n_layers - 1)
        ggml_tensor * out_named        = nullptr;  // == block_last_out, named "enc.out"
    } dumps;

    // Padding bookkeeping computed at build (caller uploads matching
    // attention_dists / last_block_mask shapes).
    int n_blocks_local = 0;  // ceil(T_enc / context_size)
    int last_block_rem = 0;  // T_enc % context_size (== 0 means no pad)
};

// Build the encoder graph. `T_enc` is the number of post-stack frames
// (== n_mel_frames / 2 after the whisper-mode trim). `use_flash` is
// reserved for future use — the implementation uses manual mul_mat + soft_max
// because the Shaw bias requires a per-(head, block) additive term and
// the flash_attn_ext path doesn't yet broadcast that cleanly.
EncoderBuild build_encoder_graph(ggml_context *         ctx,
                                 const GraniteWeights & weights,
                                 const GraniteHParams & hp,
                                 int                    T_enc,
                                 bool                   use_flash);

// Host-side precomputation of the Shaw attention_dists matrix.
// attention_dists[c, r] = clamp(c - r, -context_size, context_size) + max_pos_emb,
// where c is the key/column index and r is the query/row index (matches the
// reference `seq.view(-1,1) - seq.view(1,-1)`; see precompute_attention_dists
// in encoder.cpp). Flattened to row-major int32 [context_size * context_size].
// Caller uploads this once per encode into EncoderBuild::attention_dists.
//
// The same matrix is reused across every encoder block and every batch
// element (and across decode calls if the encoder shape is unchanged).
std::vector<int32_t> precompute_attention_dists(int context_size, int max_pos_emb);

// Host-side build of the last-block additive mask. For T_enc that is
// not a multiple of context_size, the last block is right-padded with
// zeros and the resulting (q_pad, *) and (*, k_pad) attention scores
// must be -INF before softmax. For other (q_valid, k_valid) pairs the
// mask is 0. The mask is uploaded only for the LAST block; non-last
// blocks use an all-zero mask (implicit via the per-block branch in the
// graph builder).
std::vector<float> precompute_last_block_mask(int context_size, int t_enc_remainder);

}  // namespace transcribe::granite
