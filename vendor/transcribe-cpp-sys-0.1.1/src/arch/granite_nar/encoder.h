// arch/granite_nar/encoder.h - NLE Conformer encoder + BPE CTC head.
//
// Reference: GraniteSpeechNarCTCEncoder in modeling_granite_speech_nar.py
// from the IBM Granite Speech NAR HF repo. Structurally identical to the
// AR granite encoder (block-local Shaw self-attention, conv_expansion=2
// GLU, mid-layer self-conditioned CTC bypass) plus two NAR-only additions:
//
//   1. A BPE CTC head (1024 → 100352) over a posterior-weighted
//      window=4 pool of valid frames. We expose the bypass-step char-CTC
//      mid_logits (1024 → 348) as `enc.ctc_logits` — the exact tensor
//      the reference model computes at self_conditioning_layer for the
//      self-conditioning residual. The pooled BPE head is computed
//      host-side at run time.
//   2. All-hidden-states capture: the projector consumes 4 encoder
//      hidden states (post-LN, pre-bypass at the chosen layer
//      boundaries; indices [4, 8, 12, -1] 1-indexed → layer outputs
//      after blocks 3, 7, 11, 15 in 0-indexed). The encoder graph
//      emits these concatenated along the channel axis so the
//      projector consumes one [4096, T_enc] tensor.
//
// This header is INTERNAL to src/arch/granite_nar/.

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

namespace transcribe::granite_nar {

// Host-side mel + 2-frame stack. Reuses the AR granite implementation.
transcribe_status compute_mel_encoder_input(const transcribe::MelFrontend & mel,
                                            const float *                   pcm,
                                            int                             n_samples,
                                            int                             n_threads,
                                            std::vector<float> &            out_mel,
                                            int &                           out_t_enc);

struct EncoderBuild {
    // Graph inputs (caller uploads at compute time).
    ggml_tensor * mel_in          = nullptr;  // [input_dim, T_enc]
    ggml_tensor * attention_dists = nullptr;  // [ctx*ctx] i32
    ggml_tensor * last_block_mask = nullptr;  // [ctx, ctx, n_blocks_local]
    ggml_tensor * zero_pad        = nullptr;  // optional [hidden, T_pad-T_enc]

    // Outputs.
    ggml_tensor * cat_out         = nullptr;  // [num_enc_layers * hidden, T_enc]
                                              //  the projector input
    ggml_tensor * ctc_logits      = nullptr;  // [enc_out_dim=348, T_enc]
    ggml_tensor * ctc_bpe_logits  = nullptr;  // [bpe_output_dim, T_enc]
                                              //  raw frame-level (no pool)
    ggml_tensor * mid_blank_probs = nullptr;  // [T_enc] — softmax(mid_ctc)[blank].
                                              //  Used host-side as the BPE pool's
                                              //  importance weight (importance =
                                              //  1 - blank_prob_mid).

    ggml_cgraph * graph = nullptr;

    struct Dumps {
        ggml_tensor * input_linear_out  = nullptr;
        ggml_tensor * block_0_out       = nullptr;
        ggml_tensor * block_mid_pre     = nullptr;  // block (N/2 - 1) post-LN
                                                    // (== `enc.block.7.out`)
        ggml_tensor * block_mid_post    = nullptr;  // block (N/2)   post-LN
                                                    // (== `enc.block.8.out`)
        ggml_tensor * block_last_out    = nullptr;  // block (N-1)
        ggml_tensor * ctc_logits        = nullptr;  // == ctc_logits, named
        // Block-0 sub-step taps.
        ggml_tensor * block_0_post_ff1  = nullptr;
        ggml_tensor * block_0_post_attn = nullptr;
        ggml_tensor * block_0_post_conv = nullptr;
        ggml_tensor * block_0_post_ff2  = nullptr;
    } dumps;

    int n_blocks_local = 0;
    int last_block_rem = 0;
};

EncoderBuild build_encoder_graph(ggml_context *            ctx,
                                 const GraniteNarWeights & weights,
                                 const GraniteNarHParams & hp,
                                 int                       T_enc,
                                 bool                      use_flash);

// Shaw bookkeeping helpers (identical to AR granite).
std::vector<int32_t> precompute_attention_dists(int context_size, int max_pos_emb);
std::vector<float>   precompute_last_block_mask(int context_size, int t_enc_remainder);

// Host-side BPE CTC pool + greedy decode.
//
// Input:
//   importance_non_blank  [T_enc] host row of (1 - blank_prob) values used
//                         as per-frame pool weights. Must come from the
//                         *middle* (self-conditioning) CTC head's softmax,
//                         not the final head — see modeling_ctc.py.
//   ctc_bpe      [T_enc, n_bpe_vocab] host-row-major (frame-level BPE
//                logits). n_bpe_vocab is bpe_output_dim: 100353
//                (vocab_size + 1) for the old scheme, 100352 (vocab_size)
//                for the new scheme.
//   pool_window  4 in this family
//   blank_id     selects the decode scheme (see weights.h enc_bpe_blank_id):
//                  - 0   (old): channel 0 is a synthetic blank, channels
//                        1..N hold LLM ids; emitted id = argmax - 1.
//                  - 100257 (new, BOS): channels ARE the LLM ids directly,
//                        blank is the BOS id; emitted id = argmax (no shift).
// Output:
//   token_ids    initial-hypothesis BPE token ids after greedy + collapse +
//                blank removal. Used as the text portion of the NLE LLM
//                forward (each token gets an eos slot inserted around it).
//
// Reference: NLE NARDecoder.compute_text_ctc_preds. Per-frame non-blank
// frames are bucketed into windows of pool_window, the BPE logits over each
// window are weighted by the (softmaxed) CTC non-blank posterior, then a
// greedy argmax + collapse-repeats + drop-blanks yields the hypothesis. See
// the implementation in encoder.cpp.
void compute_bpe_ctc_initial_hypothesis(const std::vector<float> & importance_non_blank,
                                        const std::vector<float> & ctc_bpe_logits,
                                        int                        n_bpe_vocab,
                                        int                        T_enc,
                                        int                        pool_window,
                                        int                        blank_id,
                                        std::vector<int32_t> &     out_token_ids);

}  // namespace transcribe::granite_nar
