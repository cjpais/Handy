// arch/medasr/weights.h - MedASR (Google LASR-CTC) tensor catalog and
// per-instance weight slots.
//
// INTERNAL to src/arch/medasr/. Encoder-CTC Conformer + rotary, with a
// 4x subsampling stem (LINEAR -> CONV1D(s=2) -> CONV1D(s=2) -> LINEAR),
// bias=false LayerNorms / linears, BatchNorm conv module (fused at load),
// macaron FF residual scalars [1.5, 0.5] and conv [2.0, 1.0], and a
// Conv1d(k=1) CTC head with bias (blank id = 0).

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::medasr {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct MedAsrHParams {
    // Encoder.
    int32_t     enc_n_layers     = 0;
    int32_t     enc_hidden       = 0;            // d_model
    int32_t     enc_n_heads      = 0;
    int32_t     enc_n_kv_heads   = 0;            // = n_heads (no GQA)
    int32_t     enc_head_dim     = 0;
    int32_t     enc_intermediate = 0;            // d_ff
    std::string enc_hidden_act;                  // "silu"
    int32_t     enc_conv_kernel         = 0;     // 32
    float       enc_conv_resid_w0       = 0.0f;  // 2.0
    float       enc_conv_resid_w1       = 0.0f;  // 1.0
    float       enc_ff_resid_w0         = 0.0f;  // 1.5
    float       enc_ff_resid_w1         = 0.0f;  // 0.5
    float       enc_layer_norm_eps      = 1e-6f;
    int32_t     enc_max_pos_emb         = 0;
    float       enc_batch_norm_momentum = 0.01f;
    float       enc_rope_theta          = 10000.0f;
    std::string enc_rope_type;  // "default"

    // Subsampling (encoder stem).
    int32_t enc_num_mel_bins = 0;  // 128
    int32_t enc_sub_kernel   = 0;  // 5
    int32_t enc_sub_stride   = 0;  // 2
    int32_t enc_sub_channels = 0;  // 256
    int32_t enc_sub_n_layers = 0;  // 2

    // CTC head.
    int32_t ctc_vocab_size = 0;
    int32_t ctc_blank_id   = 0;  // 0 (<epsilon>)

    // Frontend.
    std::string fe_type;  // "mel"
    int32_t     fe_sample_rate = 0;
    int32_t     fe_num_mels    = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;
    int32_t     fe_hop_length  = 0;
    std::string fe_window;     // "hann_symmetric"
    std::string fe_normalize;  // "none"
    std::string fe_pad_mode;   // "zero"
    std::string fe_mel_norm;   // "htk"
    float       fe_log_clamp_min = 1e-5f;
    float       fe_mel_lower_hz  = 125.0f;
    float       fe_mel_upper_hz  = 7500.0f;
};

transcribe_status read_medasr_hparams(const gguf_context * gguf, MedAsrHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots
// ---------------------------------------------------------------------------

// Subsampling: Linear(128 -> 512) -> ReLU -> Conv1d(512, 512, k=5, s=2)
// -> ReLU -> Conv1d(512, 256, k=5, s=2) -> ReLU -> Linear(256 -> 512).
struct MedAsrSubsampling {
    ggml_tensor * dense0_w = nullptr;  // [n_mels=128, d_model=512]
    ggml_tensor * dense0_b = nullptr;  // [d_model]
    ggml_tensor * conv0_w  = nullptr;  // [k=5, in=d_model, out=d_model]
    ggml_tensor * conv0_b  = nullptr;  // [d_model]
    ggml_tensor * conv1_w  = nullptr;  // [k=5, in=d_model, out=sub_channels=256]
    ggml_tensor * conv1_b  = nullptr;  // [sub_channels]
    ggml_tensor * dense1_w = nullptr;  // [sub_channels, d_model]
    ggml_tensor * dense1_b = nullptr;  // [d_model]
};

// One Conformer block. All encoder LNs are bias=false, all linears and
// convs (except conv biases) are bias=false. The conv module uses fused
// BatchNorm scale/bias precomputed at load time.
struct MedAsrBlock {
    // Macaron FF1.
    ggml_tensor * norm_ff1_w = nullptr;
    ggml_tensor * ff1_up_w   = nullptr;  // [d_model, d_ff]
    ggml_tensor * ff1_down_w = nullptr;  // [d_ff, d_model]

    // Self-attention (RoPE, no positional bias).
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * attn_q_w    = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_o_w    = nullptr;

    // Conv module (BatchNorm post-depthwise).
    ggml_tensor * norm_conv_w   = nullptr;
    ggml_tensor * conv_pw1_w    = nullptr;  // [k=1, in=d_model, out=2*d_model]
    ggml_tensor * conv_dw_w     = nullptr;  // [k=conv_kernel, 1, d_model]
    // Fused BatchNorm parameters: scale = w / sqrt(var + eps),
    //                            bias  = b - mean * scale.
    // Fused once at load time from conv_bn.{weight,bias,running_mean,running_var}.
    ggml_tensor * conv_bn_scale = nullptr;  // [d_model]
    ggml_tensor * conv_bn_bias  = nullptr;  // [d_model]
    ggml_tensor * conv_pw2_w    = nullptr;  // [k=1, d_model, d_model]

    // Macaron FF2.
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * ff2_up_w   = nullptr;
    ggml_tensor * ff2_down_w = nullptr;

    // Final per-block layer norm (output of the block).
    ggml_tensor * norm_post_w = nullptr;
};

// CTC head: Conv1d(d_model -> vocab, k=1) with bias.
struct MedAsrCtcHead {
    ggml_tensor * proj_w = nullptr;  // [k=1, in=d_model, out=vocab]
                                     //   or [d_model, vocab] post-load
    ggml_tensor * proj_b = nullptr;  // [vocab]
};

struct MedAsrWeights {
    // Baked frontend buffers (always present).
    ggml_tensor * frontend_mel_filterbank = nullptr;  // [n_mels, n_freq] f32
    ggml_tensor * frontend_window         = nullptr;  // [win_length] f32

    MedAsrSubsampling        subsampling;
    std::vector<MedAsrBlock> blocks;
    ggml_tensor *            enc_out_norm_w = nullptr;  // [d_model]
    MedAsrCtcHead            ctc_head;

    // Host-side BatchNorm fusion outputs (per-layer scale/bias arrays,
    // sized [n_layers][d_model]). The fused conv_bn_{scale,bias} pointers
    // above point into these vectors after fuse_batch_norm() runs.
    std::vector<std::vector<float>> fused_bn_scale_storage;
    std::vector<std::vector<float>> fused_bn_bias_storage;
};

transcribe_status build_medasr_weights(ggml_context * ctx_meta, const MedAsrHParams & hp, MedAsrWeights & weights);

// Fuse BatchNorm at load time. Reads raw bn.{weight,bias,running_mean,
// running_var} from the GGUF, computes fused scale = w / sqrt(var + eps)
// and bias = b - mean * scale per-channel, and creates new f32 tensors
// in `ctx_meta` for each block. Must run AFTER ctx tensors are allocated
// and weight bytes streamed into the backend buffer.
transcribe_status fuse_batch_norm(const gguf_context *  gguf_data,
                                  ggml_context *        ctx_meta,
                                  const MedAsrHParams & hp,
                                  MedAsrWeights &       weights);

}  // namespace transcribe::medasr
