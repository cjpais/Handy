// arch/medasr/weights.cpp - read_medasr_hparams + build_medasr_weights.
//
// Reads every required KV, validates the tensor catalog by name + shape,
// then fuse_batch_norm() (called by load() after data streaming) folds
// each block's conv-module BatchNorm on the host to a per-channel scale +
// bias pair, after which the raw BN tensors stop being consulted.

#include "weights.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::medasr {

namespace {

constexpr const char * kFamilyTag = "medasr";

}  // namespace

transcribe_status read_medasr_hparams(const gguf_context * gguf, MedAsrHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.hidden", kFamilyTag, hp.enc_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.n_kv_heads", kFamilyTag, hp.enc_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.head_dim", kFamilyTag, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.intermediate", kFamilyTag, hp.enc_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.medasr.encoder.hidden_act", kFamilyTag, hp.enc_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.conv_kernel", kFamilyTag, hp.enc_conv_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.conv_residual_w0", kFamilyTag, hp.enc_conv_resid_w0);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.conv_residual_w1", kFamilyTag, hp.enc_conv_resid_w1);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.ff_residual_w0", kFamilyTag, hp.enc_ff_resid_w0);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.ff_residual_w1", kFamilyTag, hp.enc_ff_resid_w1);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.layer_norm_eps", kFamilyTag, hp.enc_layer_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.max_pos_emb", kFamilyTag, hp.enc_max_pos_emb);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.batch_norm_momentum", kFamilyTag,
                                       hp.enc_batch_norm_momentum);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.medasr.encoder.rope_theta", kFamilyTag, hp.enc_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.medasr.encoder.rope_type", kFamilyTag, hp.enc_rope_type);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.num_mel_bins", kFamilyTag, hp.enc_num_mel_bins);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.sub_kernel", kFamilyTag, hp.enc_sub_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.sub_stride", kFamilyTag, hp.enc_sub_stride);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.sub_channels", kFamilyTag, hp.enc_sub_channels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.encoder.sub_n_layers", kFamilyTag, hp.enc_sub_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }

    if (hp.enc_n_heads <= 0 || hp.enc_hidden % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr: invariant hidden %% n_heads != 0 "
                "(hidden=%d, n_heads=%d)",
                hp.enc_hidden, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_sub_n_layers != 2 || hp.enc_sub_stride != 2) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr: unsupported subsampling shape (n_layers=%d, "
                "stride=%d) — only 2 stride-2 convs implemented",
                hp.enc_sub_n_layers, hp.enc_sub_stride);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_rope_type != "default") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr: unsupported rope_type=%s (expected default)",
                hp.enc_rope_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    // CTC head.
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.ctc.vocab_size", kFamilyTag, hp.ctc_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.medasr.ctc.blank_id", kFamilyTag, hp.ctc_blank_id);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Frontend.
    if (auto st = read_required_string_kv(gguf, "stt.frontend.type", kFamilyTag, hp.fe_type); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.sample_rate", kFamilyTag, hp.fe_sample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.num_mels", kFamilyTag, hp.fe_num_mels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.n_fft", kFamilyTag, hp.fe_n_fft); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.win_length", kFamilyTag, hp.fe_win_length);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.hop_length", kFamilyTag, hp.fe_hop_length);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.window", kFamilyTag, hp.fe_window); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.normalize", kFamilyTag, hp.fe_normalize);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.pad_mode", kFamilyTag, hp.fe_pad_mode);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.mel_norm", kFamilyTag, hp.fe_mel_norm);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.log_clamp_min", kFamilyTag, hp.fe_log_clamp_min);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.mel_lower_hz", kFamilyTag, hp.fe_mel_lower_hz);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.mel_upper_hz", kFamilyTag, hp.fe_mel_upper_hz);
        st != TRANSCRIBE_OK) {
        return st;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Weights catalog
// ---------------------------------------------------------------------------

#define GET_LN(slot, name)                                                                                        \
    do {                                                                                                          \
        ggml_tensor * _t =                                                                                        \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { hp.enc_hidden }, kFamilyTag); \
        if (_t == nullptr)                                                                                        \
            return TRANSCRIBE_ERR_GGUF;                                                                           \
        (slot) = _t;                                                                                              \
    } while (0)

#define GET_LIN_W(slot, name, in_dim, out_dim)                                                                   \
    do {                                                                                                         \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_LINEAR_TYPES }, \
                                                            { (in_dim), (out_dim) }, kFamilyTag);                \
        if (_t == nullptr)                                                                                       \
            return TRANSCRIBE_ERR_GGUF;                                                                          \
        (slot) = _t;                                                                                             \
    } while (0)

#define GET_LIN_B(slot, name, out_dim)                                                                        \
    do {                                                                                                      \
        ggml_tensor * _t =                                                                                    \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { (out_dim) }, kFamilyTag); \
        if (_t == nullptr)                                                                                    \
            return TRANSCRIBE_ERR_GGUF;                                                                       \
        (slot) = _t;                                                                                          \
    } while (0)

#define GET_CONV_W(slot, name, k, in_dim, out_dim)                                                             \
    do {                                                                                                       \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_CONV_TYPES }, \
                                                            { (k), (in_dim), (out_dim) }, kFamilyTag);         \
        if (_t == nullptr)                                                                                     \
            return TRANSCRIBE_ERR_GGUF;                                                                        \
        (slot) = _t;                                                                                           \
    } while (0)

#define GET_F32_1D(slot, name, n)                                                                                      \
    do {                                                                                                               \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { (n) }, kFamilyTag); \
        if (_t == nullptr)                                                                                             \
            return TRANSCRIBE_ERR_GGUF;                                                                                \
        (slot) = _t;                                                                                                   \
    } while (0)

#define GET_F32_2D(slot, name, n0, n1)                                                                         \
    do {                                                                                                       \
        ggml_tensor * _t =                                                                                     \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { (n0), (n1) }, kFamilyTag); \
        if (_t == nullptr)                                                                                     \
            return TRANSCRIBE_ERR_GGUF;                                                                        \
        (slot) = _t;                                                                                           \
    } while (0)

using transcribe::weights::lname;

namespace {

// Raw BN tensors are catalogued in MedAsrBlock so they participate in
// the standard build_medasr_weights tensor-existence check. After load
// they are read back by fuse_batch_norm() and never consulted again at
// graph time — encoder.cpp consumes MedAsrWeights::fused_bn_*_storage.
struct RawBnTensors {
    ggml_tensor * w    = nullptr;
    ggml_tensor * b    = nullptr;
    ggml_tensor * mean = nullptr;
    ggml_tensor * var  = nullptr;
};

// Per-block raw BN slots, kept out of MedAsrBlock so the public weight
// struct stays focused on graph-time inputs only. Sized to n_layers in
// build_medasr_weights.
std::vector<RawBnTensors> g_raw_bn;

transcribe_status build_block(ggml_context *        ctx_meta,
                              const MedAsrHParams & hp,
                              int                   i,
                              MedAsrBlock &         b,
                              RawBnTensors &        bn) {
    const int d    = hp.enc_hidden;
    const int dff  = hp.enc_intermediate;
    const int dw_k = hp.enc_conv_kernel;

    // Macaron FF1 (LN no bias; linears no bias).
    GET_LN(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i));
    GET_LIN_W(b.ff1_up_w, lname("enc.blocks.%d.ff1_up.weight", i), d, dff);
    GET_LIN_W(b.ff1_down_w, lname("enc.blocks.%d.ff1_down.weight", i), dff, d);

    // Self-attention (RoPE; linears no bias).
    GET_LN(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i));
    GET_LIN_W(b.attn_q_w, lname("enc.blocks.%d.attn_q.weight", i), d, d);
    GET_LIN_W(b.attn_k_w, lname("enc.blocks.%d.attn_k.weight", i), d, d);
    GET_LIN_W(b.attn_v_w, lname("enc.blocks.%d.attn_v.weight", i), d, d);
    GET_LIN_W(b.attn_o_w, lname("enc.blocks.%d.attn_o.weight", i), d, d);

    // Conv module. pw1 is 1x1 conv into 2*d for GLU; depthwise k=32; pw2 1x1.
    GET_LN(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i));
    GET_CONV_W(b.conv_pw1_w, lname("enc.blocks.%d.conv_pointwise1.weight", i), 1, d, 2 * d);
    GET_CONV_W(b.conv_dw_w, lname("enc.blocks.%d.conv_depthwise.weight", i), dw_k, 1, d);
    GET_CONV_W(b.conv_pw2_w, lname("enc.blocks.%d.conv_pointwise2.weight", i), 1, d, d);

    // Raw BN tensors (read at fuse time, never consulted after).
    GET_F32_1D(bn.w, lname("enc.blocks.%d.conv_bn.weight", i), d);
    GET_F32_1D(bn.b, lname("enc.blocks.%d.conv_bn.bias", i), d);
    GET_F32_1D(bn.mean, lname("enc.blocks.%d.conv_bn.running_mean", i), d);
    GET_F32_1D(bn.var, lname("enc.blocks.%d.conv_bn.running_var", i), d);

    // Macaron FF2.
    GET_LN(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i));
    GET_LIN_W(b.ff2_up_w, lname("enc.blocks.%d.ff2_up.weight", i), d, dff);
    GET_LIN_W(b.ff2_down_w, lname("enc.blocks.%d.ff2_down.weight", i), dff, d);

    // Final per-block LN (no bias).
    GET_LN(b.norm_post_w, lname("enc.blocks.%d.norm_post.weight", i));

    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status build_medasr_weights(ggml_context * ctx_meta, const MedAsrHParams & hp, MedAsrWeights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int d      = hp.enc_hidden;
    const int n_mels = hp.enc_num_mel_bins;
    const int sub_k  = hp.enc_sub_kernel;
    const int sub_c  = hp.enc_sub_channels;
    const int n_freq = hp.fe_n_fft / 2 + 1;

    // Frontend buffers (baked into the GGUF by the converter). The
    // converter emits the filterbank with numpy shape (n_freq, n_mels);
    // GGUF reverses to ggml ne=[n_mels, n_freq] (fast=n_mels).
    GET_F32_2D(weights.frontend_mel_filterbank, "frontend.mel_filterbank", hp.fe_num_mels, n_freq);
    GET_F32_1D(weights.frontend_window, "frontend.window", hp.fe_win_length);

    // Subsampling: Linear(128 -> 512) -> ReLU -> Conv1d(512, 512, k=5, s=2)
    // -> ReLU -> Conv1d(512, 256, k=5, s=2) -> ReLU -> Linear(256 -> 512).
    GET_LIN_W(weights.subsampling.dense0_w, "enc.subsampling.dense_0.weight", n_mels, d);
    GET_LIN_B(weights.subsampling.dense0_b, "enc.subsampling.dense_0.bias", d);
    GET_CONV_W(weights.subsampling.conv0_w, "enc.subsampling.conv_0.weight", sub_k, d, d);
    GET_LIN_B(weights.subsampling.conv0_b, "enc.subsampling.conv_0.bias", d);
    GET_CONV_W(weights.subsampling.conv1_w, "enc.subsampling.conv_1.weight", sub_k, d, sub_c);
    GET_LIN_B(weights.subsampling.conv1_b, "enc.subsampling.conv_1.bias", sub_c);
    GET_LIN_W(weights.subsampling.dense1_w, "enc.subsampling.dense_1.weight", sub_c, d);
    GET_LIN_B(weights.subsampling.dense1_b, "enc.subsampling.dense_1.bias", d);

    // Conformer blocks.
    weights.blocks.resize(hp.enc_n_layers);
    g_raw_bn.assign(hp.enc_n_layers, RawBnTensors{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        if (auto st = build_block(ctx_meta, hp, i, weights.blocks[i], g_raw_bn[i]); st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // Top-level out norm (no bias).
    GET_LN(weights.enc_out_norm_w, "enc.out_norm.weight");

    // CTC head: Conv1d(d, vocab, k=1) with bias.
    GET_CONV_W(weights.ctc_head.proj_w, "ctc.proj.weight", 1, d, hp.ctc_vocab_size);
    GET_LIN_B(weights.ctc_head.proj_b, "ctc.proj.bias", hp.ctc_vocab_size);

    return TRANSCRIBE_OK;
}

transcribe_status fuse_batch_norm(const gguf_context * /*gguf_data*/,
                                  ggml_context * /*ctx_meta*/,
                                  const MedAsrHParams & hp,
                                  MedAsrWeights &       weights) {
    if (hp.enc_n_layers <= 0 || hp.enc_hidden <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (static_cast<int>(g_raw_bn.size()) != hp.enc_n_layers) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr fuse_batch_norm: raw BN catalog size %zu != "
                "n_layers %d",
                g_raw_bn.size(), hp.enc_n_layers);
        return TRANSCRIBE_ERR_GGUF;
    }

    // BatchNorm fusion: scale = w / sqrt(var + eps),
    //                    bias  = b - mean * scale.
    // The reference uses LASR's batch_norm_momentum 0.01 only at TRAIN time
    // for running stats — at inference the stored running_mean / running_var
    // are used directly. Eps is the standard PyTorch default 1e-5 (NOT the
    // encoder's layer_norm_eps 1e-6; the two epsilons are independent).
    constexpr float kBnEps = 1e-5f;
    const int       d      = hp.enc_hidden;

    weights.fused_bn_scale_storage.assign(static_cast<size_t>(hp.enc_n_layers), std::vector<float>(d, 0.0f));
    weights.fused_bn_bias_storage.assign(static_cast<size_t>(hp.enc_n_layers), std::vector<float>(d, 0.0f));

    std::vector<float> bn_w(d), bn_b(d), bn_mean(d), bn_var(d);

    for (int i = 0; i < hp.enc_n_layers; ++i) {
        const RawBnTensors & raw = g_raw_bn[static_cast<size_t>(i)];
        if (raw.w == nullptr || raw.b == nullptr || raw.mean == nullptr || raw.var == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "medasr fuse_batch_norm: missing raw BN tensor at "
                    "layer %d",
                    i);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_get(raw.w, bn_w.data(), 0, d * sizeof(float));
        ggml_backend_tensor_get(raw.b, bn_b.data(), 0, d * sizeof(float));
        ggml_backend_tensor_get(raw.mean, bn_mean.data(), 0, d * sizeof(float));
        ggml_backend_tensor_get(raw.var, bn_var.data(), 0, d * sizeof(float));

        auto & scale = weights.fused_bn_scale_storage[static_cast<size_t>(i)];
        auto & bias  = weights.fused_bn_bias_storage[static_cast<size_t>(i)];
        for (int c = 0; c < d; ++c) {
            const float s = bn_w[c] / std::sqrt(bn_var[c] + kBnEps);
            scale[c]      = s;
            bias[c]       = bn_b[c] - bn_mean[c] * s;
        }
    }

    // Catalog is consumed; release the raw pointer holder.
    g_raw_bn.clear();
    return TRANSCRIBE_OK;
}

}  // namespace transcribe::medasr
