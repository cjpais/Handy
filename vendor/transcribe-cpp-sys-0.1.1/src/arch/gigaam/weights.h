// arch/gigaam/weights.h - canonical GigaAM tensor catalog and per-instance
// weight slots.
//
// INTERNAL to src/arch/gigaam/. Mirrors arch/parakeet/weights.h in shape;
// the differences are:
//
//   - Encoder uses rotary positional embeddings, not relative-pos. The
//     ConformerBlock has no `linear_pos`, `pos_bias_u`, or `pos_bias_v`
//     tensors.
//   - Pre-encode is 2-conv1d (NOT parakeet's 4-conv2d). Conv1d weights
//     are [out, in, k].
//   - Conv module uses LayerNorm. No `running_mean`/`running_var`; raw
//     LN scale + bias on `conv.ln.{weight,bias}`.
//   - Encoder linear+conv biases are ALWAYS present (no use_bias flag).
//   - RNN-T predictor / joint: 1 LSTM layer, hidden=320, joint_net is
//     2-element Sequential(ReLU, Linear). Output linear renamed to
//     `joint.out`.
//   - Frontend: htk mel scale, no slaney norm. `frontend.mel_filterbank`
//     (shape [n_mels=64, n_freq_bins=161]) and `frontend.window`
//     (length 320, Hann periodic) are baked into the GGUF — the C++
//     loader holds borrowed pointers and the mel path reads them
//     directly.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::gigaam {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

enum class HeadKind { RNNT, CTC };

struct GigaamHParams {
    HeadKind head_kind = HeadKind::RNNT;

    // Encoder (Conformer).
    int32_t     enc_n_layers           = 0;
    int32_t     enc_d_model            = 0;
    int32_t     enc_n_heads            = 0;
    int32_t     enc_d_ff               = 0;
    int32_t     enc_conv_kernel        = 0;
    int32_t     enc_subsampling_factor = 0;
    int32_t     enc_subs_kernel_size   = 0;
    int32_t     enc_pos_emb_max_len    = 0;
    int32_t     enc_feat_in            = 0;
    // Recorded for documentation; today only "rotary" is in scope.
    std::string enc_self_attention_model;
    // Conv module normalisation. GigaAM always ships "layer_norm" but
    // we read the KV so a future variant change is loud, not silent.
    std::string enc_conv_norm_type;

    // RNN-T predictor + joint (HeadKind::RNNT only).
    int32_t     pred_hidden     = 0;
    int32_t     pred_n_layers   = 0;
    int32_t     pred_vocab      = 0;
    int32_t     joint_hidden    = 0;
    int32_t     joint_n_classes = 0;
    std::string joint_activation;

    // CTC head (HeadKind::CTC only). Stored separately so the same
    // hparam struct can describe both head kinds.
    int32_t head_feat_in   = 0;
    int32_t head_n_classes = 0;

    // Frontend.
    std::string fe_type;  // "mel"
    int32_t     fe_num_mels    = 0;
    int32_t     fe_sample_rate = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;
    int32_t     fe_hop_length  = 0;
    std::string fe_window;     // "hann_periodic"
    std::string fe_normalize;  // "none"
    float       fe_dither       = 0.0f;
    float       fe_pre_emphasis = 0.0f;
    float       fe_f_min        = 0.0f;
    float       fe_f_max        = 0.0f;
    bool        fe_center       = false;
    std::string fe_mel_norm;  // "htk"
    float       fe_log_clamp_min = 1e-9f;
    float       fe_log_clamp_max = 1e9f;

    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }
};

transcribe_status read_gigaam_hparams(const gguf_context * gguf, GigaamHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots
// ---------------------------------------------------------------------------

struct GigaamPreEncode {
    // 2 stride-2 conv1d layers (the in-between ReLU is stateless).
    ggml_tensor * conv0_w = nullptr;  // [k=5, in=64,  out=768]
    ggml_tensor * conv0_b = nullptr;  // [768]
    ggml_tensor * conv2_w = nullptr;  // [k=5, in=768, out=768]
    ggml_tensor * conv2_b = nullptr;  // [768]
};

// One Conformer block. All biases present (GigaAM is always-bias).
struct GigaamBlock {
    // Macaron FF1.
    ggml_tensor * norm_ff1_w = nullptr;
    ggml_tensor * norm_ff1_b = nullptr;
    ggml_tensor * ff1_lin1_w = nullptr;  // [d_ff,    d_model]
    ggml_tensor * ff1_lin1_b = nullptr;
    ggml_tensor * ff1_lin2_w = nullptr;  // [d_model, d_ff]
    ggml_tensor * ff1_lin2_b = nullptr;

    // Conv module (LayerNorm post-depthwise).
    ggml_tensor * norm_conv_w = nullptr;
    ggml_tensor * norm_conv_b = nullptr;
    ggml_tensor * conv_pw1_w  = nullptr;  // [k=1, in=d_model, out=2*d_model]
    ggml_tensor * conv_pw1_b  = nullptr;  // [2*d_model]
    ggml_tensor * conv_dw_w   = nullptr;  // [k=conv_kernel, 1, d_model]
    ggml_tensor * conv_dw_b   = nullptr;  // [d_model]
    ggml_tensor * conv_ln_w   = nullptr;  // [d_model]
    ggml_tensor * conv_ln_b   = nullptr;  // [d_model]
    ggml_tensor * conv_pw2_w  = nullptr;  // [k=1, in=d_model, out=d_model]
    ggml_tensor * conv_pw2_b  = nullptr;  // [d_model]

    // Self-attention (rotary). No linear_pos / pos_bias_*.
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_q_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_q_b    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_k_b    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_v_b    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;

    // Macaron FF2.
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_lin1_w = nullptr;
    ggml_tensor * ff2_lin1_b = nullptr;
    ggml_tensor * ff2_lin2_w = nullptr;
    ggml_tensor * ff2_lin2_b = nullptr;

    // Final per-block layer norm.
    ggml_tensor * norm_out_w = nullptr;
    ggml_tensor * norm_out_b = nullptr;
};

// RNN-T predictor (only when head_kind=RNNT).
struct GigaamPredictor {
    ggml_tensor * embed_w = nullptr;  // [pred_vocab, pred_hidden]

    struct LstmLayer {
        ggml_tensor * Wx = nullptr;  // [4*pred_hidden, pred_hidden]
        ggml_tensor * Wh = nullptr;  // [4*pred_hidden, pred_hidden]
        ggml_tensor * b  = nullptr;  // [4*pred_hidden] (collapsed bias_ih+bias_hh)
    };

    std::vector<LstmLayer> lstm;
};

// RNN-T joint network (only when head_kind=RNNT). 2-element
// Sequential(ReLU, Linear) — no inner Linear like parakeet TDT.
struct GigaamJoint {
    ggml_tensor * enc_w  = nullptr;  // [joint_hidden, enc_d_model]
    ggml_tensor * enc_b  = nullptr;
    ggml_tensor * pred_w = nullptr;  // [joint_hidden, pred_hidden]
    ggml_tensor * pred_b = nullptr;
    ggml_tensor * out_w  = nullptr;  // [n_classes, joint_hidden]
    ggml_tensor * out_b  = nullptr;
};

// CTC head (only when head_kind=CTC). Single 1×1 Conv1d.
struct GigaamCtcHead {
    ggml_tensor * weight = nullptr;  // [k=1, d_model, n_classes]
    ggml_tensor * bias   = nullptr;  // [n_classes]
};

struct GigaamWeights {
    // Baked frontend buffers (always present).
    ggml_tensor * frontend_mel_filterbank = nullptr;  // [n_freq_bins, n_mels]
    ggml_tensor * frontend_window         = nullptr;  // [win_length]

    GigaamPreEncode          pre_encode;
    std::vector<GigaamBlock> blocks;
    GigaamPredictor          predictor;
    GigaamJoint              joint;
    GigaamCtcHead            ctc_head;
};

transcribe_status build_gigaam_weights(ggml_context * ctx_meta, const GigaamHParams & hp, GigaamWeights & weights);

}  // namespace transcribe::gigaam
