// arch/canary_qwen/weights.cpp - SALM (FastConformer + Qwen3-1.7B) loader.
//
// KV reader and tensor catalog. Tensor names mirror
// scripts/convert-canary-qwen.py exactly.
//
// The encoder block layout is byte-for-byte the same as canary-1b-flash
// (use_bias=true, untie_biases=true). The decoder block layout is
// standard Qwen3 (no Q/K/V/O biases, per-head q_norm/k_norm).

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <cstring>

namespace transcribe::canary_qwen {

namespace {

constexpr const char * kFamilyTag = "canary_qwen";

}  // namespace

transcribe_status read_canary_qwen_hparams(gguf_context * gguf, CanaryQwenHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.d_ff", kFamilyTag, hp.enc_d_ff);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.conv_kernel", kFamilyTag, hp.enc_conv_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.subsampling_factor", kFamilyTag,
                                       hp.enc_subsampling_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.encoder.subsampling_channels", kFamilyTag,
                                       hp.enc_subsampling_chans);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.canary_qwen.encoder.pos_emb_max_len", kFamilyTag, hp.enc_pos_emb_max_len);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Perception (audio adapter -> LM input).
    if (auto st =
            read_required_u32_kv(gguf, "stt.canary_qwen.perception.output_dim", kFamilyTag, hp.perception_output_dim);
        st != TRANSCRIBE_OK) {
        // Older converters may not have written output_dim; fall back to LM hidden.
        hp.perception_output_dim = 0;
    }

    // Audio locator id (single Qwen2 BPE special token id).
    {
        int        v = -1;
        const auto r = read_token_id_kv(gguf, "stt.canary_qwen.perception.audio_locator_id", v);
        if (r == KvResult::Ok) {
            hp.audio_locator_id = v;
        } else {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: missing or invalid stt.canary_qwen.perception.audio_locator_id",
                    kFamilyTag);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Decoder (Qwen3-1.7B).
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.hidden_size", kFamilyTag, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.canary_qwen.decoder.intermediate_size", kFamilyTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.n_kv_heads", kFamilyTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary_qwen.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.canary_qwen.decoder.rms_norm_eps", kFamilyTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.canary_qwen.decoder.rope_theta", kFamilyTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.canary_qwen.decoder.tie_word_embeddings", kFamilyTag, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }

    if (hp.perception_output_dim == 0) {
        hp.perception_output_dim = hp.dec_hidden;
    }

    // Frontend.
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
    if (auto st = read_required_string_kv(gguf, "stt.frontend.pad_mode", kFamilyTag, hp.fe_pad_mode);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.normalize", kFamilyTag, hp.fe_normalize);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Cross-field validation.
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: dec.n_heads (%d) must be divisible by dec.n_kv_heads (%d)", kFamilyTag,
                hp.dec_n_heads, hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads * hp.dec_head_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: dec.n_heads(%d) * dec.head_dim(%d) != dec.hidden(%d)", kFamilyTag,
                hp.dec_n_heads, hp.dec_head_dim, hp.dec_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.perception_output_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: perception.output_dim(%d) must equal dec.hidden(%d) "
                "for direct concat into LM embedding stream",
                kFamilyTag, hp.perception_output_dim, hp.dec_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.audio_locator_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: audio_locator_id must be >= 0", kFamilyTag);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_subsampling_factor != 8 || hp.fe_num_mels != 128) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: only subsampling_factor=8 num_mels=128 supported (got %d, %d)",
                kFamilyTag, hp.enc_subsampling_factor, hp.fe_num_mels);
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Weights
// ---------------------------------------------------------------------------

namespace {

using transcribe::weights::lname;

#define GET_F32(slot, name, ...)                                                                                \
    do {                                                                                                        \
        ggml_tensor * _t =                                                                                      \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { __VA_ARGS__ }, kFamilyTag); \
        if (_t == nullptr)                                                                                      \
            return TRANSCRIBE_ERR_GGUF;                                                                         \
        (slot) = _t;                                                                                            \
    } while (0)

#define GET_CONV(slot, name, ...)                                                                              \
    do {                                                                                                       \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_CONV_TYPES }, \
                                                            { __VA_ARGS__ }, kFamilyTag);                      \
        if (_t == nullptr)                                                                                     \
            return TRANSCRIBE_ERR_GGUF;                                                                        \
        (slot) = _t;                                                                                           \
    } while (0)

#define GET_LIN(slot, name, ...)                                                                                 \
    do {                                                                                                         \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_LINEAR_TYPES }, \
                                                            { __VA_ARGS__ }, kFamilyTag);                        \
        if (_t == nullptr)                                                                                       \
            return TRANSCRIBE_ERR_GGUF;                                                                          \
        (slot) = _t;                                                                                             \
    } while (0)

}  // namespace

transcribe_status build_canary_qwen_weights(ggml_context *            ctx_meta,
                                            const CanaryQwenHParams & hp,
                                            CanaryQwenWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t channels = hp.enc_subsampling_chans;
    const int64_t d_enc    = hp.enc_d_model;
    const int64_t d_ff_e   = hp.enc_d_ff;
    const int64_t n_heads  = hp.enc_n_heads;
    const int64_t head_dim = hp.enc_d_model / hp.enc_n_heads;
    const int64_t k        = hp.enc_conv_kernel;
    const int64_t out_dim  = hp.perception_output_dim;
    const int64_t vocab    = hp.dec_vocab_size;
    const int64_t hidden   = hp.dec_hidden;
    const int64_t inter    = hp.dec_intermediate;
    const int64_t kv_dim   = hp.dec_n_kv_heads * hp.dec_head_dim;

    // pre_encode.out is Linear(channels * (n_mels / factor) -> d_enc)
    // = Linear(256 * (128/8) -> 1024) = Linear(4096 -> 1024).
    const int64_t pre_encode_in = channels * (hp.fe_num_mels / hp.enc_subsampling_factor);

    // ---------- pre_encode ----------
    GET_CONV(weights.pre_encode.conv0_w, "enc.pre_encode.conv.0.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv0_b, "enc.pre_encode.conv.0.bias", channels);
    GET_CONV(weights.pre_encode.conv2_w, "enc.pre_encode.conv.2.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv2_b, "enc.pre_encode.conv.2.bias", channels);
    GET_CONV(weights.pre_encode.conv3_w, "enc.pre_encode.conv.3.weight", 1, 1, channels, channels);
    GET_F32(weights.pre_encode.conv3_b, "enc.pre_encode.conv.3.bias", channels);
    GET_CONV(weights.pre_encode.conv5_w, "enc.pre_encode.conv.5.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv5_b, "enc.pre_encode.conv.5.bias", channels);
    GET_CONV(weights.pre_encode.conv6_w, "enc.pre_encode.conv.6.weight", 1, 1, channels, channels);
    GET_F32(weights.pre_encode.conv6_b, "enc.pre_encode.conv.6.bias", channels);
    GET_LIN(weights.pre_encode.out_w, "enc.pre_encode.out.weight", pre_encode_in, d_enc);
    GET_F32(weights.pre_encode.out_b, "enc.pre_encode.out.bias", d_enc);

    // ---------- encoder blocks (every linear has a bias) ----------
    weights.blocks.assign(hp.enc_n_layers, EncBlockSlots{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.blocks[i];

        GET_F32(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i), d_enc);
        GET_F32(b.norm_ff1_b, lname("enc.blocks.%d.norm_ff1.bias", i), d_enc);
        GET_LIN(b.ff1_lin1_w, lname("enc.blocks.%d.ff1.linear1.weight", i), d_enc, d_ff_e);
        GET_F32(b.ff1_lin1_b, lname("enc.blocks.%d.ff1.linear1.bias", i), d_ff_e);
        GET_LIN(b.ff1_lin2_w, lname("enc.blocks.%d.ff1.linear2.weight", i), d_ff_e, d_enc);
        GET_F32(b.ff1_lin2_b, lname("enc.blocks.%d.ff1.linear2.bias", i), d_enc);

        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_enc);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), d_enc);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.linear_q.weight", i), d_enc, d_enc);
        GET_F32(b.attn_q_b, lname("enc.blocks.%d.attn.linear_q.bias", i), d_enc);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.linear_k.weight", i), d_enc, d_enc);
        GET_F32(b.attn_k_b, lname("enc.blocks.%d.attn.linear_k.bias", i), d_enc);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.linear_v.weight", i), d_enc, d_enc);
        GET_F32(b.attn_v_b, lname("enc.blocks.%d.attn.linear_v.bias", i), d_enc);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.linear_out.weight", i), d_enc, d_enc);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.linear_out.bias", i), d_enc);
        GET_LIN(b.attn_pos_w, lname("enc.blocks.%d.attn.linear_pos.weight", i), d_enc, d_enc);
        GET_F32(b.attn_pos_u, lname("enc.blocks.%d.attn.pos_bias_u", i), head_dim, n_heads);
        GET_F32(b.attn_pos_v, lname("enc.blocks.%d.attn.pos_bias_v", i), head_dim, n_heads);

        GET_F32(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i), d_enc);
        GET_F32(b.norm_conv_b, lname("enc.blocks.%d.norm_conv.bias", i), d_enc);
        GET_CONV(b.conv_pw1_w, lname("enc.blocks.%d.conv.pointwise1.weight", i), 1, d_enc, 2 * d_enc);
        GET_F32(b.conv_pw1_b, lname("enc.blocks.%d.conv.pointwise1.bias", i), 2 * d_enc);
        GET_CONV(b.conv_dw_w, lname("enc.blocks.%d.conv.depthwise.weight", i), k, 1, d_enc);
        GET_F32(b.conv_dw_b, lname("enc.blocks.%d.conv.depthwise.bias", i), d_enc);
        GET_CONV(b.conv_pw2_w, lname("enc.blocks.%d.conv.pointwise2.weight", i), 1, d_enc, d_enc);
        GET_F32(b.conv_pw2_b, lname("enc.blocks.%d.conv.pointwise2.bias", i), d_enc);
        GET_F32(b.conv_bn_w, lname("enc.blocks.%d.conv.bn.weight", i), d_enc);
        GET_F32(b.conv_bn_b, lname("enc.blocks.%d.conv.bn.bias", i), d_enc);
        GET_F32(b.conv_bn_rm, lname("enc.blocks.%d.conv.bn.running_mean", i), d_enc);
        GET_F32(b.conv_bn_rv, lname("enc.blocks.%d.conv.bn.running_var", i), d_enc);

        GET_F32(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i), d_enc);
        GET_F32(b.norm_ff2_b, lname("enc.blocks.%d.norm_ff2.bias", i), d_enc);
        GET_LIN(b.ff2_lin1_w, lname("enc.blocks.%d.ff2.linear1.weight", i), d_enc, d_ff_e);
        GET_F32(b.ff2_lin1_b, lname("enc.blocks.%d.ff2.linear1.bias", i), d_ff_e);
        GET_LIN(b.ff2_lin2_w, lname("enc.blocks.%d.ff2.linear2.weight", i), d_ff_e, d_enc);
        GET_F32(b.ff2_lin2_b, lname("enc.blocks.%d.ff2.linear2.bias", i), d_enc);

        GET_F32(b.norm_out_w, lname("enc.blocks.%d.norm_out.weight", i), d_enc);
        GET_F32(b.norm_out_b, lname("enc.blocks.%d.norm_out.bias", i), d_enc);
    }

    // ---------- perception projection ----------
    GET_LIN(weights.perception_proj.weight, "enc.proj.weight", d_enc, out_dim);
    GET_F32(weights.perception_proj.bias, "enc.proj.bias", out_dim);

    // ---------- decoder ----------
    GET_LIN(weights.dec_embed.token_w, "dec.token_embd.weight", hidden, vocab);
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", hidden);

    weights.dec_blocks.assign(hp.dec_n_layers, DecBlockSlots{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];

        GET_F32(b.norm_attn_w, lname("dec.blocks.%d.norm_attn.weight", i), hidden);
        GET_F32(b.norm_ffn_w, lname("dec.blocks.%d.norm_ffn.weight", i), hidden);

        GET_LIN(b.attn_q_w, lname("dec.blocks.%d.attn.q.weight", i), hidden, hidden);
        GET_LIN(b.attn_k_w, lname("dec.blocks.%d.attn.k.weight", i), hidden, kv_dim);
        GET_LIN(b.attn_v_w, lname("dec.blocks.%d.attn.v.weight", i), hidden, kv_dim);
        GET_LIN(b.attn_o_w, lname("dec.blocks.%d.attn.o.weight", i), hidden, hidden);

        GET_F32(b.attn_q_norm, lname("dec.blocks.%d.attn.q_norm.weight", i), hp.dec_head_dim);
        GET_F32(b.attn_k_norm, lname("dec.blocks.%d.attn.k_norm.weight", i), hp.dec_head_dim);

        GET_LIN(b.ffn_gate_w, lname("dec.blocks.%d.ffn.gate.weight", i), hidden, inter);
        GET_LIN(b.ffn_up_w, lname("dec.blocks.%d.ffn.up.weight", i), hidden, inter);
        GET_LIN(b.ffn_down_w, lname("dec.blocks.%d.ffn.down.weight", i), inter, hidden);
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::canary_qwen
