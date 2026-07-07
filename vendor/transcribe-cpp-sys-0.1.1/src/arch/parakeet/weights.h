// arch/parakeet/weights.h - canonical Parakeet tensor catalog and
// per-instance weight slots. Internal to src/arch/parakeet/.
//
//   - ParakeetHParams: architecture KV read from stt.parakeet.* /
//     stt.frontend.* before allocating tensors. Every shape-driving dim
//     lives here.
//   - ParakeetWeights: named borrowed ggml_tensor* slots, one per
//     logical weight. Storage is owned by the model's ggml_context; the
//     slots must not outlive it.
//
// The source file walks the canonical name list and validates each
// tensor (presence + shape) against the hparams.
//
// Tensor layout conventions (matched by the converter):
//   - Linear weights: PyTorch [out, in] (ggml_mul_mat reads ne0 = in).
//   - Conv2d weights: PyTorch OIHW [out_channels, in_channels, kH, kW].
//   - Conv1d weights: PyTorch [out_channels, in_channels, kernel] for
//     pointwise; [out_channels, kernel, 1] for depthwise (groups =
//     out_channels = in_channels).
//   - LSTM gates (Wx, Wh): single concatenated [4*hidden, input] in
//     PyTorch (i, f, g, o) order; bias concatenated [4*hidden].
//   - LayerNorm/RMSNorm: separate weight + bias, shape [d_model].
//   - BatchNorm: weight + bias + running_mean + running_var; all four
//     kept (folded into the conv at compute time, eval mode → affine).

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::parakeet {

// Hyperparameters, read from stt.parakeet.* / stt.frontend.* via
// read_parakeet_hparams(). Every shape-driving field lives here so the
// loader validates weight shapes against KV, not against tensor sizes.

// Decoder head dispatch (stt.parakeet.head_kind string KV; absent
// defaults to TDT for legacy v2/v3). Drives conditional KV reads and the
// per-head decoder dispatch in run().
enum class HeadKind { TDT, RNNT, CTC };

struct ParakeetHParams {
    HeadKind head_kind = HeadKind::TDT;

    // Encoder (Conformer).
    int32_t enc_n_layers             = 0;
    int32_t enc_d_model              = 0;
    int32_t enc_n_heads              = 0;
    int32_t enc_d_ff                 = 0;  // d_model * ff_expansion_factor
    int32_t enc_conv_kernel          = 0;  // depthwise conv kernel inside the Conformer block
    int32_t enc_subsampling_factor   = 0;  // total downsampling on the time axis
    int32_t enc_subsampling_channels = 0;  // intermediate channel count in pre_encode
    int32_t enc_pos_emb_max_len      = 0;
    bool    enc_use_bias             = false;
    // NeMo's RelPositionalEncoding multiplies x by sqrt(d_model) when
    // xscaling=True. Default false so legacy GGUFs (v2/v3, xscaling=False)
    // keep working unchanged.
    bool    enc_xscaling             = false;
    // Local attention window. -1/-1 = full attention (most variants).
    // When non-negative, each query attends to keys within [left, right]
    // per enc_att_context_style below:
    //   Regular        — pos_emb shortened to (left+right+1) and the band
    //     mask in matrix_bd (NeMo LocalAttRelPositionalEncoding);
    //     parakeet-tdt_ctc-1.1b sets [128, 128].
    //   ChunkedLimited — full RelPositionalEncoding (pos_emb 2T-1) plus a
    //     host-side chunked mask; chunk_size = right+1, left_chunks =
    //     left/chunk_size. nemotron-speech-streaming-en-0.6b ([70, 13]).
    int32_t enc_att_context_left     = -1;
    int32_t enc_att_context_right    = -1;

    // Multi-lookahead training menu for cache-aware streaming models. A
    // flat GGUF int32 array [L0,R0,L1,R1,...]; index 0 is the default
    // (max-context) setting and always equals (enc_att_context_left,
    // enc_att_context_right). The caller picks via
    // transcribe_parakeet_stream_ext::att_context_right. Empty for offline
    // variants; the loader synthesizes a one-element list from the scalar
    // fields so call sites read uniformly. nemotron-speech-streaming-en
    // is trained on [[70,13],[70,6],[70,1],[70,0]].
    std::vector<std::pair<int32_t, int32_t>> enc_att_context_size_choices;

    // Chunked-limited-with-rc training menu (buffered streaming, i.e.
    // parakeet-unified-en-0.6b): three independent L / C / R lists whose
    // cartesian product is the set of (L, C, R) tuples the model trained
    // against; the runtime picks one entry from each at inference. Empty
    // unless enc_att_context_style == ChunkedLimitedWithRc. For
    // parakeet-unified-en-0.6b: L ∈ {70}, C ∈ {1,2,7,13},
    // R ∈ {0,1,2,3,4,7,13}; default (70, 13, 13).
    std::vector<int32_t> enc_att_chunk_left_choices;
    std::vector<int32_t> enc_att_chunk_chunk_choices;
    std::vector<int32_t> enc_att_chunk_right_choices;

    // Self-attention context style (stt.parakeet.encoder.att_context_style
    // string KV; optional, default "regular").
    //   Regular              — full attention (L=R=-1) or local window;
    //     every offline variant.
    //   ChunkedLimited       — cache-aware streaming, 2-tuple (L, R) mask;
    //     nemotron-speech-streaming-en-0.6b.
    //   ChunkedLimitedWithRc — buffered streaming, 3-tuple (L, C, R) mask;
    //     parakeet-unified-en-0.6b. The offline path stays on full
    //     attention (cfg att_context_size [-1, -1]); the mask is engaged
    //     only by the buffered driver.
    enum class AttContextStyle { Regular, ChunkedLimited, ChunkedLimitedWithRc };
    AttContextStyle enc_att_context_style = AttContextStyle::Regular;

    // Depthwise conv context window inside the Conformer block. -1/-1 =
    // symmetric (kernel-1)/2 (every offline variant). NeMo's causal
    // conv_context_size emits [kernel-1, 0] (left-context only).
    int32_t enc_conv_context_left  = -1;
    int32_t enc_conv_context_right = -1;

    // Conv-module normalisation. Offline default BatchNorm; streaming
    // variants use LayerNorm. With LayerNorm the GGUF carries the same
    // conv.bn.weight / conv.bn.bias names but omits running_mean/var, and
    // the conformer applies an unfused LayerNorm (per-channel mean/std
    // then affine with bn_w / bn_b).
    enum class ConvNormType { BatchNorm, LayerNorm };
    ConvNormType enc_conv_norm_type = ConvNormType::BatchNorm;

    // Cache-aware streaming pre-encode constants (optional KVs under
    // stt.parakeet.encoder.streaming.*; both 0 for offline variants, on
    // which the streaming paths gate).
    //   pre_encode_cache_size: mel-history frames prepended to each
    //     non-first chunk to fill the conv-subsample receptive field
    //     (9 on nemotron).
    //   drop_extra_pre_encoded: encoder frames discarded after the
    //     subsample stack per non-first chunk to align with the cache
    //     boundary (2 on nemotron).
    int32_t enc_stream_pre_encode_cache_size  = 0;
    int32_t enc_stream_drop_extra_pre_encoded = 0;
    // ConvSubsampling's first-chunk output frames; used for
    // chunk_size_first = sampling_frames_first + subsampling_factor * R.
    // FastConformer ships sampling_frames=[1, 8] (first entry 1). Defaults
    // to subsampling_factor when absent (no first-chunk special case).
    int32_t enc_stream_sampling_frames_first  = 0;

    // Predictor (RNN-T prediction network). TDT and RNNT only; zero for CTC.
    int32_t pred_hidden   = 0;
    int32_t pred_n_layers = 0;
    int32_t pred_vocab    = 0;  // includes the +1 "start" / blank-row entry of the embed matrix
                                // (raw token vocab size is pred_vocab - 1)

    // Joint network. TDT and RNNT only; zero for CTC.
    int32_t     joint_hidden            = 0;
    int32_t     joint_num_extra_outputs = 0;  // TDT durations count; 0 for RNNT
    // Joint output activation (stt.parakeet.joint.activation), validated
    // against the C++ joint forward's allow-list at load. v2/v3 ship
    // "relu". Read so the C++ side doesn't hard-code an activation and
    // silently produce wrong logits on a model that asked for another.
    std::string joint_activation;

    // TDT durations: the values the joint's "extra outputs" axis predicts
    // (e.g. [0,1,2,3,4]); argmax over duration logits indexes this list,
    // and the chosen integer is how many encoder frames the decoder
    // advances after a token. KV stt.parakeet.tdt.durations; length must
    // equal joint_num_extra_outputs (cross-validated at load).
    std::vector<int32_t> tdt_durations;

    // tdt_max_symbols: per-frame "stuck" cap (NeMo greedy_batch). Optional
    // KV, default 10. After N consecutive zero-duration tokens on the same
    // frame, forces a +1 advance. 0 disables.
    int32_t tdt_max_symbols = 10;

    // Frontend (mel feature extractor). The complete stt.frontend.* set
    // the converter emits and the C++ frontend reads; the loader gates on
    // it. CMVN/LFR fields are omitted (no published Parakeet variant uses
    // them).
    std::string fe_type;  // "mel" for parakeet
    int32_t     fe_num_mels    = 0;
    int32_t     fe_sample_rate = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;      // samples
    int32_t     fe_hop_length  = 0;      // samples
    std::string fe_window;               // "hann" for parakeet
    std::string fe_normalize;            // "per_feature" for parakeet
    float       fe_dither       = 0.0f;
    float       fe_pre_emphasis = 0.0f;  // 0.0 == disabled
    float       fe_f_min        = 0.0f;
    float       fe_f_max        = 0.0f;

    // CTC head. Populated by build_parakeet_weights when head_kind=CTC,
    // by reading the leading dim of head.ctc.weight (shape
    // [vocab+1, d_model, 1] in PyTorch -> ggml ne [1, d_model, vocab+1]).
    // Zero for TDT/RNNT.
    int32_t head_ctc_n_classes = 0;

    // Language-conditioning prompt MLP (NeMo
    // EncDecRNNTBPEModelWithPrompt). Multilingual variants mix a one-hot
    // prompt vector into the encoder output before the RNN-T joint:
    //   x = concat(enc[d_model], one_hot(prompt_id)[num_prompts])
    //   h = relu(W0 @ x + b0)        // W0: [prompt_hidden, d_model+P]
    //   y = W2 @ h + b2              // W2: [d_model, prompt_hidden]
    //   enc_out := y
    // Gated on has_prompt (stt.parakeet.prompt.num_prompts present).
    // Today: nemotron-3.5-asr-streaming-0.6b.
    bool                     has_prompt         = false;
    int32_t                  prompt_num_prompts = 0;
    int32_t                  prompt_hidden      = 0;
    std::string              prompt_field;
    std::string              prompt_activation;
    std::vector<std::string> prompt_dictionary_locales;
    std::vector<int32_t>     prompt_dictionary_indices;
    int32_t                  prompt_auto_id = -1;

    // Derived. Convenience accessors keyed off the above.
    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    int32_t joint_n_classes() const { return (pred_vocab - 1) + joint_num_extra_outputs + 1; }
};

// Read every required stt.parakeet.* / stt.frontend.* KV into hp.
// Returns INVALID_ARG if gguf is null, ERR_GGUF on a missing/wrong-type
// key or a cross-field invariant failure (e.g. d_model % n_heads != 0).
transcribe_status read_parakeet_hparams(const gguf_context * gguf, ParakeetHParams & hp);

// Weight slots. Every ggml_tensor* below is borrowed into the model's
// ggml_context and is invalidated when the model is freed.

struct ParakeetPreEncode {
    // dw_striding subsampling: 1 standard conv + 2 depthwise-separable
    // conv pairs = 8x time downsampling. Indices follow NeMo's
    // torch.nn.Sequential (1 and 4 are activation modules, skipped).
    ggml_tensor * conv0_w = nullptr;  // [out=channels,        in=1,        kh=3, kw=3]
    ggml_tensor * conv0_b = nullptr;  // [channels]
    ggml_tensor * conv2_w = nullptr;  // [out=channels,        in=1 (dw),   kh=3, kw=3]
    ggml_tensor * conv2_b = nullptr;  // [channels]
    ggml_tensor * conv3_w = nullptr;  // [out=channels,        in=channels, kh=1, kw=1]
    ggml_tensor * conv3_b = nullptr;  // [channels]
    ggml_tensor * conv5_w = nullptr;  // [out=channels,        in=1 (dw),   kh=3, kw=3]
    ggml_tensor * conv5_b = nullptr;  // [channels]
    ggml_tensor * conv6_w = nullptr;  // [out=channels,        in=channels, kh=1, kw=1]
    ggml_tensor * conv6_b = nullptr;  // [channels]
    // Final projection from flattened [channels * (num_mels / subsampling)] to d_model.
    ggml_tensor * out_w   = nullptr;
    ggml_tensor * out_b   = nullptr;
};

// One Conformer block (same shape contract for every block). The bias
// slots (*_b on linear/conv layers, NOT on norms / BN / pos_bias) are
// populated only when enc_use_bias=true; null for the v2/v3 baseline.
struct ParakeetBlock {
    // Macaron feed-forward 1.
    ggml_tensor * norm_ff1_w = nullptr;  // [d_model]
    ggml_tensor * norm_ff1_b = nullptr;  // [d_model]
    ggml_tensor * ff1_lin1_w = nullptr;  // [d_ff,    d_model]
    ggml_tensor * ff1_lin1_b = nullptr;  // [d_ff],    nullable (use_bias)
    ggml_tensor * ff1_lin2_w = nullptr;  // [d_model, d_ff]
    ggml_tensor * ff1_lin2_b = nullptr;  // [d_model], nullable (use_bias)

    // Self-attention with relative positional encoding.
    ggml_tensor * norm_attn_w = nullptr;  // [d_model]
    ggml_tensor * norm_attn_b = nullptr;  // [d_model]
    ggml_tensor * attn_q_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_q_b    = nullptr;  // [d_model], nullable (use_bias)
    ggml_tensor * attn_k_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_k_b    = nullptr;  // [d_model], nullable (use_bias)
    ggml_tensor * attn_v_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_v_b    = nullptr;  // [d_model], nullable (use_bias)
    ggml_tensor * attn_out_w  = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_out_b  = nullptr;  // [d_model], nullable (use_bias)
    ggml_tensor * attn_pos_w =
        nullptr;  // [d_model, d_model]   (linear_pos is bias-free in NeMo even when use_bias=true)
    ggml_tensor * attn_pos_u = nullptr;  // [n_heads, head_dim]   learned rel-pos bias u
    ggml_tensor * attn_pos_v = nullptr;  // [n_heads, head_dim]   learned rel-pos bias v

    // Convolution module: pointwise -> GLU -> depthwise -> BN -> activation -> pointwise.
    ggml_tensor * norm_conv_w = nullptr;  // [d_model]
    ggml_tensor * norm_conv_b = nullptr;  // [d_model]
    ggml_tensor * conv_pw1_w  = nullptr;  // [2*d_model, d_model, 1]   conv1d k=1; 2x for GLU
    ggml_tensor * conv_pw1_b  = nullptr;  // [2*d_model], nullable (use_bias)
    ggml_tensor * conv_dw_w   = nullptr;  // [d_model,   1,       k]   depthwise conv1d
    ggml_tensor * conv_dw_b   = nullptr;  // [d_model],   nullable (use_bias)
    ggml_tensor * conv_pw2_w  = nullptr;  // [d_model,   d_model, 1]   conv1d k=1
    ggml_tensor * conv_pw2_b  = nullptr;  // [d_model],   nullable (use_bias)
    ggml_tensor * conv_bn_w   = nullptr;  // [d_model]   BN scale
    ggml_tensor * conv_bn_b   = nullptr;  // [d_model]   BN shift
    ggml_tensor * conv_bn_rm  = nullptr;  // [d_model]   running_mean
    ggml_tensor * conv_bn_rv  = nullptr;  // [d_model]   running_var

    // Fused BN parameters (computed at load time, replaces batch_norm()
    // in the graph with a single mul + add):
    //   fused_scale = bn_w / sqrt(bn_rv + eps)
    //   fused_bias  = bn_b - bn_rm * fused_scale
    ggml_tensor * conv_bn_fused_scale = nullptr;  // [d_model]
    ggml_tensor * conv_bn_fused_bias  = nullptr;  // [d_model]

    // Macaron feed-forward 2.
    ggml_tensor * norm_ff2_w = nullptr;  // [d_model]
    ggml_tensor * norm_ff2_b = nullptr;  // [d_model]
    ggml_tensor * ff2_lin1_w = nullptr;  // [d_ff,    d_model]
    ggml_tensor * ff2_lin1_b = nullptr;  // [d_ff],    nullable (use_bias)
    ggml_tensor * ff2_lin2_w = nullptr;  // [d_model, d_ff]
    ggml_tensor * ff2_lin2_b = nullptr;  // [d_model], nullable (use_bias)

    // Final per-block layer norm.
    ggml_tensor * norm_out_w = nullptr;  // [d_model]
    ggml_tensor * norm_out_b = nullptr;  // [d_model]
};

struct ParakeetPredictor {
    // Token embedding; the +1 row is NeMo's "start of sequence" embedding.
    ggml_tensor * embed_w = nullptr;  // [pred_vocab, pred_hidden]

    // 2-layer LSTM (v2/v3 0.6b). Concatenated gate weights in PyTorch
    // (i, f, g, o) order, 4*pred_hidden rows.
    struct LstmLayer {
        ggml_tensor * Wx =
            nullptr;  // [4*pred_hidden, pred_hidden]   (input-to-hidden, layer 0 input dim is pred_hidden because of the embed)
        ggml_tensor * Wh = nullptr;  // [4*pred_hidden, pred_hidden]   (hidden-to-hidden)
        ggml_tensor * b  = nullptr;  // [4*pred_hidden]
    };

    std::vector<LstmLayer> lstm;  // pred_n_layers entries
};

struct ParakeetJoint {
    ggml_tensor * enc_w  = nullptr;  // [joint_hidden, enc_d_model]
    ggml_tensor * enc_b  = nullptr;  // [joint_hidden]
    ggml_tensor * pred_w = nullptr;  // [joint_hidden, pred_hidden]
    ggml_tensor * pred_b = nullptr;  // [joint_hidden]
    ggml_tensor * out_w  = nullptr;  // [joint_n_classes, joint_hidden]
    ggml_tensor * out_b  = nullptr;  // [joint_n_classes]
};

// CTC head. Single 1×1 Conv1d projecting d_model -> vocab+1. Loaded
// only when head_kind=CTC; predictor + joint stay empty in that case.
struct ParakeetCtcHead {
    ggml_tensor * weight = nullptr;  // [vocab+1, d_model, 1]   conv1d k=1
    ggml_tensor * bias   = nullptr;  // [vocab+1]
};

// Language-conditioning prompt MLP (multilingual variants). NeMo's
// nn.Sequential indexing is preserved: `.0` is the input linear,
// `.2` is the output linear, `.1` is the parameter-free activation.
// Loaded only when hp.has_prompt is true; otherwise the slots stay
// null and the encoder skips the prompt path.
struct ParakeetPromptMlp {
    ggml_tensor * mlp0_w = nullptr;  // [prompt_hidden, d_model + num_prompts]
    ggml_tensor * mlp0_b = nullptr;  // [prompt_hidden]
    ggml_tensor * mlp2_w = nullptr;  // [d_model, prompt_hidden]
    ggml_tensor * mlp2_b = nullptr;  // [d_model]
};

struct ParakeetWeights {
    ParakeetPreEncode          pre_encode;
    std::vector<ParakeetBlock> blocks;     // hp.enc_n_layers entries
    ParakeetPredictor          predictor;  // empty when head_kind=CTC
    ParakeetJoint              joint;      // empty when head_kind=CTC
    ParakeetCtcHead            ctc_head;   // populated when head_kind=CTC
    ParakeetPromptMlp          prompt;     // populated when hp.has_prompt
};

// Walk the canonical tensor list, look up each tensor by name, validate
// its shape against hp, and store the borrowed pointer in the matching
// slot. The ggml_tensors must already exist in ctx_meta. Every required
// tensor must be present with the exact shape; missing/mismatched returns
// TRANSCRIBE_ERR_GGUF naming the tensor. On failure `weights` is left
// indeterminate (the caller throws the model away).
transcribe_status build_parakeet_weights(ggml_context *          ctx_meta,
                                         const ParakeetHParams & hp,
                                         ParakeetWeights &       weights);

}  // namespace transcribe::parakeet
