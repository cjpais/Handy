// arch/gigaam/weights.cpp - read_gigaam_hparams + build_gigaam_weights.
//
// Pattern follows arch/parakeet/weights.cpp: read every required KV
// from the GGUF, then validate the tensor catalog by name + shape using
// transcribe::weights::find_tensor.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <cstring>

namespace transcribe::gigaam {

namespace {

constexpr const char * kFamilyTag = "gigaam";

}  // namespace

transcribe_status read_gigaam_hparams(const gguf_context * gguf, GigaamHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Head discriminator (drives RNN-T vs CTC reads below).
    {
        std::string head_kind_str;
        if (auto st = read_required_string_kv(gguf, "stt.gigaam.head_kind", kFamilyTag, head_kind_str);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (head_kind_str == "rnnt") {
            hp.head_kind = HeadKind::RNNT;
        } else if (head_kind_str == "ctc") {
            hp.head_kind = HeadKind::CTC;
        } else {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: unsupported stt.gigaam.head_kind=%s", head_kind_str.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.d_ff", kFamilyTag, hp.enc_d_ff); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.conv_kernel", kFamilyTag, hp.enc_conv_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.gigaam.encoder.subsampling_factor", kFamilyTag, hp.enc_subsampling_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.gigaam.encoder.subs_kernel_size", kFamilyTag, hp.enc_subs_kernel_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.pos_emb_max_len", kFamilyTag, hp.enc_pos_emb_max_len);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.gigaam.encoder.feat_in", kFamilyTag, hp.enc_feat_in);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.gigaam.encoder.self_attention_model", kFamilyTag,
                                          hp.enc_self_attention_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.gigaam.encoder.conv_norm_type", kFamilyTag, hp.enc_conv_norm_type);
        st != TRANSCRIBE_OK) {
        return st;
    }

    if (hp.enc_n_heads <= 0 || hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "gigaam: invariant d_model %% n_heads != 0 "
                "(d_model=%d, n_heads=%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_self_attention_model != "rotary") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: unsupported self_attention_model=%s (expected rotary)",
                hp.enc_self_attention_model.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_conv_norm_type != "layer_norm") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: unsupported conv_norm_type=%s (expected layer_norm)",
                hp.enc_conv_norm_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    if (hp.head_kind == HeadKind::RNNT) {
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.predictor.hidden", kFamilyTag, hp.pred_hidden);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.predictor.n_layers", kFamilyTag, hp.pred_n_layers);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.predictor.vocab", kFamilyTag, hp.pred_vocab);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.joint.hidden", kFamilyTag, hp.joint_hidden);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.joint.num_classes", kFamilyTag, hp.joint_n_classes);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_string_kv(gguf, "stt.gigaam.joint.activation", kFamilyTag, hp.joint_activation);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (hp.pred_vocab != hp.joint_n_classes) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: pred_vocab (%d) != joint_n_classes (%d)", hp.pred_vocab,
                    hp.joint_n_classes);
            return TRANSCRIBE_ERR_GGUF;
        }
    } else {  // CTC
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.head.feat_in", kFamilyTag, hp.head_feat_in);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.gigaam.head.num_classes", kFamilyTag, hp.head_n_classes);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (hp.head_feat_in != hp.enc_d_model) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: ctc head.feat_in (%d) != encoder.d_model (%d)",
                    hp.head_feat_in, hp.enc_d_model);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Frontend.
    if (auto st = read_required_string_kv(gguf, "stt.frontend.type", kFamilyTag, hp.fe_type); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.num_mels", kFamilyTag, hp.fe_num_mels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.sample_rate", kFamilyTag, hp.fe_sample_rate);
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
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.dither", kFamilyTag, hp.fe_dither); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.pre_emphasis", kFamilyTag, hp.fe_pre_emphasis);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_min", kFamilyTag, hp.fe_f_min); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_max", kFamilyTag, hp.fe_f_max); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.center", kFamilyTag, false, hp.fe_center);
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
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.log_clamp_max", kFamilyTag, hp.fe_log_clamp_max);
        st != TRANSCRIBE_OK) {
        return st;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Weights catalog
// ---------------------------------------------------------------------------

#define GET_LN(slot, name)                                                                                         \
    do {                                                                                                           \
        ggml_tensor * _t =                                                                                         \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { hp.enc_d_model }, kFamilyTag); \
        if (_t == nullptr)                                                                                         \
            return TRANSCRIBE_ERR_GGUF;                                                                            \
        (slot) = _t;                                                                                               \
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

transcribe_status build_block(ggml_context * ctx_meta, const GigaamHParams & hp, int i, GigaamBlock & b) {
    const int d    = hp.enc_d_model;
    const int dff  = hp.enc_d_ff;
    const int dw_k = hp.enc_conv_kernel;

    GET_LN(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i));
    GET_LN(b.norm_ff1_b, lname("enc.blocks.%d.norm_ff1.bias", i));
    GET_LIN_W(b.ff1_lin1_w, lname("enc.blocks.%d.ff1.linear1.weight", i), d, dff);
    GET_LIN_B(b.ff1_lin1_b, lname("enc.blocks.%d.ff1.linear1.bias", i), dff);
    GET_LIN_W(b.ff1_lin2_w, lname("enc.blocks.%d.ff1.linear2.weight", i), dff, d);
    GET_LIN_B(b.ff1_lin2_b, lname("enc.blocks.%d.ff1.linear2.bias", i), d);

    GET_LN(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i));
    GET_LN(b.norm_conv_b, lname("enc.blocks.%d.norm_conv.bias", i));
    GET_CONV_W(b.conv_pw1_w, lname("enc.blocks.%d.conv.pointwise1.weight", i), 1, d, 2 * d);
    GET_LIN_B(b.conv_pw1_b, lname("enc.blocks.%d.conv.pointwise1.bias", i), 2 * d);
    GET_CONV_W(b.conv_dw_w, lname("enc.blocks.%d.conv.depthwise.weight", i), dw_k, 1, d);
    GET_LIN_B(b.conv_dw_b, lname("enc.blocks.%d.conv.depthwise.bias", i), d);
    GET_LN(b.conv_ln_w, lname("enc.blocks.%d.conv.ln.weight", i));
    GET_LN(b.conv_ln_b, lname("enc.blocks.%d.conv.ln.bias", i));
    GET_CONV_W(b.conv_pw2_w, lname("enc.blocks.%d.conv.pointwise2.weight", i), 1, d, d);
    GET_LIN_B(b.conv_pw2_b, lname("enc.blocks.%d.conv.pointwise2.bias", i), d);

    // Self-attention (rotary, no linear_pos / pos_bias_*).
    GET_LN(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i));
    GET_LN(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i));
    GET_LIN_W(b.attn_q_w, lname("enc.blocks.%d.attn.linear_q.weight", i), d, d);
    GET_LIN_B(b.attn_q_b, lname("enc.blocks.%d.attn.linear_q.bias", i), d);
    GET_LIN_W(b.attn_k_w, lname("enc.blocks.%d.attn.linear_k.weight", i), d, d);
    GET_LIN_B(b.attn_k_b, lname("enc.blocks.%d.attn.linear_k.bias", i), d);
    GET_LIN_W(b.attn_v_w, lname("enc.blocks.%d.attn.linear_v.weight", i), d, d);
    GET_LIN_B(b.attn_v_b, lname("enc.blocks.%d.attn.linear_v.bias", i), d);
    GET_LIN_W(b.attn_out_w, lname("enc.blocks.%d.attn.linear_out.weight", i), d, d);
    GET_LIN_B(b.attn_out_b, lname("enc.blocks.%d.attn.linear_out.bias", i), d);

    GET_LN(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i));
    GET_LN(b.norm_ff2_b, lname("enc.blocks.%d.norm_ff2.bias", i));
    GET_LIN_W(b.ff2_lin1_w, lname("enc.blocks.%d.ff2.linear1.weight", i), d, dff);
    GET_LIN_B(b.ff2_lin1_b, lname("enc.blocks.%d.ff2.linear1.bias", i), dff);
    GET_LIN_W(b.ff2_lin2_w, lname("enc.blocks.%d.ff2.linear2.weight", i), dff, d);
    GET_LIN_B(b.ff2_lin2_b, lname("enc.blocks.%d.ff2.linear2.bias", i), d);

    GET_LN(b.norm_out_w, lname("enc.blocks.%d.norm_out.weight", i));
    GET_LN(b.norm_out_b, lname("enc.blocks.%d.norm_out.bias", i));

    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status build_gigaam_weights(ggml_context * ctx_meta, const GigaamHParams & hp, GigaamWeights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int d       = hp.enc_d_model;
    const int feat_in = hp.enc_feat_in;
    const int sub_k   = hp.enc_subs_kernel_size;
    const int n_freq  = hp.fe_n_fft / 2 + 1;

    // Frontend buffers (baked into the GGUF by the converter).
    GET_F32_2D(weights.frontend_mel_filterbank, "frontend.mel_filterbank", n_freq, hp.fe_num_mels);
    GET_F32_1D(weights.frontend_window, "frontend.window", hp.fe_win_length);

    // Pre-encode (2 stride-2 conv1d).
    GET_CONV_W(weights.pre_encode.conv0_w, "enc.pre_encode.conv.0.weight", sub_k, feat_in, d);
    GET_LIN_B(weights.pre_encode.conv0_b, "enc.pre_encode.conv.0.bias", d);
    GET_CONV_W(weights.pre_encode.conv2_w, "enc.pre_encode.conv.2.weight", sub_k, d, d);
    GET_LIN_B(weights.pre_encode.conv2_b, "enc.pre_encode.conv.2.bias", d);

    // Conformer blocks.
    weights.blocks.resize(hp.enc_n_layers);
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        if (auto st = build_block(ctx_meta, hp, i, weights.blocks[i]); st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // Head.
    if (hp.head_kind == HeadKind::RNNT) {
        // pred.embed.weight is a token-id lookup table consumed by the
        // host RNN-T decoder. The quantizer treats it as a linear-weight
        // tile (TRANSCRIBE_QUANT_LINEAR_TYPES: F32/F16/Q*), so the loader
        // must too. decoder.cpp's readback_f32 dequantizes once at load
        // time via ggml type traits.
        GET_LIN_W(weights.predictor.embed_w, "pred.embed.weight", hp.pred_hidden, hp.pred_vocab);

        weights.predictor.lstm.resize(hp.pred_n_layers);
        for (int i = 0; i < hp.pred_n_layers; ++i) {
            auto & L = weights.predictor.lstm[i];
            // PyTorch nn.LSTM packs 4 gates along the row dim.
            GET_LIN_W(L.Wx, lname("pred.lstm.%d.Wx", i), hp.pred_hidden, 4 * hp.pred_hidden);
            GET_LIN_W(L.Wh, lname("pred.lstm.%d.Wh", i), hp.pred_hidden, 4 * hp.pred_hidden);
            GET_LIN_B(L.b, lname("pred.lstm.%d.bias", i), 4 * hp.pred_hidden);
        }
        GET_LIN_W(weights.joint.enc_w, "joint.enc.weight", d, hp.joint_hidden);
        GET_LIN_B(weights.joint.enc_b, "joint.enc.bias", hp.joint_hidden);
        GET_LIN_W(weights.joint.pred_w, "joint.pred.weight", hp.pred_hidden, hp.joint_hidden);
        GET_LIN_B(weights.joint.pred_b, "joint.pred.bias", hp.joint_hidden);
        GET_LIN_W(weights.joint.out_w, "joint.out.weight", hp.joint_hidden, hp.joint_n_classes);
        GET_LIN_B(weights.joint.out_b, "joint.out.bias", hp.joint_n_classes);
    } else {  // CTC
        GET_CONV_W(weights.ctc_head.weight, "head.ctc.weight", 1, d, hp.head_n_classes);
        GET_LIN_B(weights.ctc_head.bias, "head.ctc.bias", hp.head_n_classes);
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::gigaam
