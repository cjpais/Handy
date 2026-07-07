// arch/canary/weights.cpp - read_canary_hparams + build_canary_weights.
//
// FastConformer encoder (biases on every linear) + autoregressive decoder
// (untied dec.head.{weight,bias}); 180m-flash adds enc.proj.{weight,bias}.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <cstring>
#include <initializer_list>

namespace transcribe::canary {

namespace {

constexpr const char * kFamilyTag = "canary";

KvResult read_token_id_required(const gguf_context * gguf, const char * key, int32_t & out) {
    int  v = -1;
    auto r = read_token_id_kv(gguf, key, v);
    if (r == KvResult::Ok) {
        out = v;
    }
    return r;
}

}  // namespace

transcribe_status read_canary_hparams(const gguf_context * gguf, CanaryHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.d_ff", kFamilyTag, hp.enc_d_ff); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.conv_kernel", kFamilyTag, hp.enc_conv_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.canary.encoder.subsampling_factor", kFamilyTag, hp.enc_subsampling_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.subsampling_channels", kFamilyTag,
                                       hp.enc_subsampling_channels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.encoder.pos_emb_max_len", kFamilyTag, hp.enc_pos_emb_max_len);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.canary.encoder.use_bias", kFamilyTag, false, hp.enc_use_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }

    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.d_model", kFamilyTag, hp.dec_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.d_ff", kFamilyTag, hp.dec_d_ff); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.max_position", kFamilyTag, hp.dec_max_position);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.canary.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.canary.decoder.activation", kFamilyTag, hp.dec_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.canary.decoder.pre_ln", kFamilyTag, true, hp.dec_pre_ln);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.canary.decoder.learn_positional_encodings", kFamilyTag, false,
                                        hp.dec_learn_positional_encodings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.canary.decoder.encoder_decoder_proj", kFamilyTag, false,
                                        hp.dec_has_encoder_decoder_proj);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Multitask prompt + special tokens.
    if (auto st = read_required_string_kv(gguf, "stt.canary.tokenizer.prompt_format", kFamilyTag, hp.prompt_format);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_optional_bool_kv(gguf, "stt.canary.tokenizer.single_sp", kFamilyTag, false, hp.tokenizer_single_sp);
        st != TRANSCRIBE_OK) {
        return st;
    }

    auto require_special = [&](const char * key, int32_t & out) -> transcribe_status {
        const auto r = read_token_id_required(gguf, key, out);
        if (r == KvResult::Ok) {
            return TRANSCRIBE_OK;
        }
        if (r == KvResult::Absent) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: required special-token KV missing: %s", kFamilyTag, key);
            return TRANSCRIBE_ERR_GGUF;
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: special-token KV %s has wrong type", kFamilyTag, key);
        return TRANSCRIBE_ERR_GGUF;
    };

    if (auto st = require_special("stt.canary.special.startoftranscript_id", hp.startoftranscript_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = require_special("stt.canary.special.endoftext_id", hp.endoftext_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = require_special("stt.canary.special.pad_id", hp.pad_special_id); st != TRANSCRIBE_OK) {
        return st;
    }

    // Optional task tokens — present on canary2 variants, may be missing on canary-1b.
    auto opt_special = [&](const char * key, int32_t & out) -> transcribe_status {
        const auto r = read_token_id_required(gguf, key, out);
        if (r == KvResult::Ok || r == KvResult::Absent) {
            return TRANSCRIBE_OK;
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: optional special-token KV %s has wrong type", kFamilyTag, key);
        return TRANSCRIBE_ERR_GGUF;
    };

    if (auto st = opt_special("stt.canary.special.startofcontext_id", hp.startofcontext_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.nospeech_id", hp.nospeech_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.pnc_id", hp.pnc_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.nopnc_id", hp.nopnc_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.itn_id", hp.itn_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.noitn_id", hp.noitn_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.timestamp_id", hp.timestamp_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.notimestamp_id", hp.notimestamp_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.diarize_id", hp.diarize_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.nodiarize_id", hp.nodiarize_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.spkchange_id", hp.spkchange_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.audioseparator_id", hp.audioseparator_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.transcribe_id", hp.transcribe_id); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = opt_special("stt.canary.special.translate_id", hp.translate_id); st != TRANSCRIBE_OK) {
        return st;
    }

    // Language list -> language token id table. Two converter shapes for the
    // routing keys: aggregate tokenizers put per-language codes in
    // stt.canary.tokenizer.lang_codes (+ a "spl_tokens" entry with no id);
    // single-SP (canary-1b-v2) puts ["all"] there and the real codes in
    // general.languages. Either way the ids live under
    // stt.canary.special.lang.<code>_id, so walk both sources, dedupe, and
    // keep only codes whose _id KV resolves.
    {
        std::vector<std::string> all_codes;
        std::vector<std::string> tmp;
        if (read_string_array_kv(gguf, "stt.canary.tokenizer.lang_codes", tmp) == KvResult::Ok) {
            all_codes.insert(all_codes.end(), tmp.begin(), tmp.end());
        }
        tmp.clear();
        if (read_string_array_kv(gguf, "general.languages", tmp) == KvResult::Ok) {
            all_codes.insert(all_codes.end(), tmp.begin(), tmp.end());
        }
        std::vector<std::string> seen_codes;
        for (const auto & code : all_codes) {
            bool dup = false;
            for (const auto & s : seen_codes) {
                if (s == code) {
                    dup = true;
                    break;
                }
            }
            if (dup) {
                continue;
            }
            seen_codes.push_back(code);
            std::string key = std::string("stt.canary.special.lang.") + code + "_id";
            int         v   = -1;
            const auto  kr  = read_token_id_kv(gguf, key.c_str(), v);
            if (kr == KvResult::Ok) {
                hp.languages.push_back(code);
                hp.language_ids.push_back(v);
            } else if (kr == KvResult::BadType) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: %s wrong type", kFamilyTag, key.c_str());
                return TRANSCRIBE_ERR_GGUF;
            }
        }
    }

    {
        std::vector<std::string> pairs;
        switch (read_string_array_kv(gguf, "stt.translation.pairs", pairs)) {
            case KvResult::Absent:
                break;
            case KvResult::Ok:
                hp.translation_pairs = pairs;
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: stt.translation.pairs wrong type", kFamilyTag);
                return TRANSCRIBE_ERR_GGUF;
        }
    }

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
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.pad_mode", kFamilyTag, "reflect", hp.fe_pad_mode);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Cross-field invariants.
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_d_ff <= 0 ||
        hp.enc_conv_kernel <= 0 || hp.enc_subsampling_factor <= 0 || hp.enc_subsampling_channels <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_d_model <= 0 || hp.dec_n_heads <= 0 || hp.dec_d_ff <= 0 ||
        hp.dec_max_position <= 0 || hp.dec_vocab_size <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_d_model % hp.dec_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: decoder d_model (%d) not divisible by n_heads (%d)",
                hp.dec_d_model, hp.dec_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_activation != "relu" && hp.dec_activation != "silu" && hp.dec_activation != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary: unsupported decoder activation \"%s\" "
                "(only relu, silu, swish are implemented)",
                hp.dec_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: unsupported frontend type \"%s\" (only \"mel\")",
                hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_window != "hann") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: unsupported frontend window \"%s\" (only \"hann\")",
                hp.fe_window.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_normalize != "per_feature") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: unsupported frontend normalize \"%s\"", hp.fe_normalize.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if ((hp.fe_n_fft & (hp.fe_n_fft - 1)) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: frontend n_fft (%d) must be a power of 2", hp.fe_n_fft);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_pad_mode != "reflect" && hp.fe_pad_mode != "constant") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: unsupported frontend pad_mode \"%s\"", hp.fe_pad_mode.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.prompt_format != "canary" && hp.prompt_format != "canary2") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary: unsupported prompt_format \"%s\" "
                "(only \"canary\" and \"canary2\")",
                hp.prompt_format.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Weights
// ---------------------------------------------------------------------------

namespace {

using transcribe::weights::lname;

constexpr const char * kTag = kFamilyTag;

#define GET_F32(slot, name, ...)                                                                          \
    do {                                                                                                  \
        ggml_tensor * _t =                                                                                \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { __VA_ARGS__ }, kTag); \
        if (_t == nullptr)                                                                                \
            return TRANSCRIBE_ERR_GGUF;                                                                   \
        (slot) = _t;                                                                                      \
    } while (0)

#define GET_CONV(slot, name, ...)                                                                              \
    do {                                                                                                       \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_CONV_TYPES }, \
                                                            { __VA_ARGS__ }, kTag);                            \
        if (_t == nullptr)                                                                                     \
            return TRANSCRIBE_ERR_GGUF;                                                                        \
        (slot) = _t;                                                                                           \
    } while (0)

#define GET_LIN(slot, name, ...)                                                                                 \
    do {                                                                                                         \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_LINEAR_TYPES }, \
                                                            { __VA_ARGS__ }, kTag);                              \
        if (_t == nullptr)                                                                                       \
            return TRANSCRIBE_ERR_GGUF;                                                                          \
        (slot) = _t;                                                                                             \
    } while (0)

}  // namespace

transcribe_status build_canary_weights(ggml_context * ctx_meta, const CanaryHParams & hp, CanaryWeights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t channels = hp.enc_subsampling_channels;
    const int64_t d_enc    = hp.enc_d_model;
    const int64_t d_ff_e   = hp.enc_d_ff;
    const int64_t n_heads  = hp.enc_n_heads;
    const int64_t head_dim = hp.enc_head_dim();
    const int64_t k        = hp.enc_conv_kernel;
    const int64_t d_dec    = hp.dec_d_model;
    const int64_t d_ff_d   = hp.dec_d_ff;
    const int64_t vocab    = hp.dec_vocab_size;

    const int64_t pre_encode_freq = channels * (hp.fe_num_mels / hp.enc_subsampling_factor);
    const int64_t pre_encode_in   = pre_encode_freq;

    // pre_encode
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

    // Encoder blocks. Every linear carries a bias term — both macaron FFs,
    // Q/K/V/out, attention-pos projection, and the conv pointwise pair
    // (unlike parakeet, which is bias-free).
    weights.blocks.assign(hp.enc_n_layers, CanaryBlock{});
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

    // Optional encoder->decoder projection (180m-flash only).
    if (hp.dec_has_encoder_decoder_proj) {
        GET_LIN(weights.enc_proj.weight, "enc.proj.weight", d_enc, d_dec);
        GET_F32(weights.enc_proj.bias, "enc.proj.bias", d_dec);
    }

    // Decoder embedding.
    {
        ggml_tensor * tw = ggml_get_tensor(ctx_meta, "dec.embed.token.weight");
        if (tw == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: missing tensor \"dec.embed.token.weight\"");
            return TRANSCRIBE_ERR_GGUF;
        }
        if (tw->ne[0] != d_dec || tw->ne[1] != vocab) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary: dec.embed.token.weight shape [%lld,%lld] expected [%lld,%lld]",
                    (long long) tw->ne[0], (long long) tw->ne[1], (long long) d_dec, (long long) vocab);
            return TRANSCRIBE_ERR_GGUF;
        }
        weights.dec_embed.token_w = tw;
    }
    GET_F32(weights.dec_embed.pos_enc, "dec.embed.pos_enc", d_dec, hp.dec_max_position);
    GET_F32(weights.dec_embed.norm_w, "dec.embed.norm.weight", d_dec);
    GET_F32(weights.dec_embed.norm_b, "dec.embed.norm.bias", d_dec);

    // Decoder blocks (canary tensor naming).
    weights.dec_blocks.assign(hp.dec_n_layers, CanaryDecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & db = weights.dec_blocks[i];

        GET_F32(db.norm1_w, lname("dec.layer.%d.norm1.weight", i), d_dec);
        GET_F32(db.norm1_b, lname("dec.layer.%d.norm1.bias", i), d_dec);
        GET_LIN(db.self_q_w, lname("dec.layer.%d.self_attn.q.weight", i), d_dec, d_dec);
        GET_F32(db.self_q_b, lname("dec.layer.%d.self_attn.q.bias", i), d_dec);
        GET_LIN(db.self_k_w, lname("dec.layer.%d.self_attn.k.weight", i), d_dec, d_dec);
        GET_F32(db.self_k_b, lname("dec.layer.%d.self_attn.k.bias", i), d_dec);
        GET_LIN(db.self_v_w, lname("dec.layer.%d.self_attn.v.weight", i), d_dec, d_dec);
        GET_F32(db.self_v_b, lname("dec.layer.%d.self_attn.v.bias", i), d_dec);
        GET_LIN(db.self_o_w, lname("dec.layer.%d.self_attn.o.weight", i), d_dec, d_dec);
        GET_F32(db.self_o_b, lname("dec.layer.%d.self_attn.o.bias", i), d_dec);

        GET_F32(db.norm2_w, lname("dec.layer.%d.norm2.weight", i), d_dec);
        GET_F32(db.norm2_b, lname("dec.layer.%d.norm2.bias", i), d_dec);
        GET_LIN(db.cross_q_w, lname("dec.layer.%d.cross_attn.q.weight", i), d_dec, d_dec);
        GET_F32(db.cross_q_b, lname("dec.layer.%d.cross_attn.q.bias", i), d_dec);
        GET_LIN(db.cross_k_w, lname("dec.layer.%d.cross_attn.k.weight", i), d_dec, d_dec);
        GET_F32(db.cross_k_b, lname("dec.layer.%d.cross_attn.k.bias", i), d_dec);
        GET_LIN(db.cross_v_w, lname("dec.layer.%d.cross_attn.v.weight", i), d_dec, d_dec);
        GET_F32(db.cross_v_b, lname("dec.layer.%d.cross_attn.v.bias", i), d_dec);
        GET_LIN(db.cross_o_w, lname("dec.layer.%d.cross_attn.o.weight", i), d_dec, d_dec);
        GET_F32(db.cross_o_b, lname("dec.layer.%d.cross_attn.o.bias", i), d_dec);

        GET_F32(db.norm3_w, lname("dec.layer.%d.norm3.weight", i), d_dec);
        GET_F32(db.norm3_b, lname("dec.layer.%d.norm3.bias", i), d_dec);
        GET_LIN(db.ffn_up_w, lname("dec.layer.%d.ffn.up.weight", i), d_dec, d_ff_d);
        GET_F32(db.ffn_up_b, lname("dec.layer.%d.ffn.up.bias", i), d_ff_d);
        GET_LIN(db.ffn_down_w, lname("dec.layer.%d.ffn.down.weight", i), d_ff_d, d_dec);
        GET_F32(db.ffn_down_b, lname("dec.layer.%d.ffn.down.bias", i), d_dec);
    }

    // Decoder final norm.
    GET_F32(weights.dec_final.norm_w, "dec.norm.weight", d_dec);
    GET_F32(weights.dec_final.norm_b, "dec.norm.bias", d_dec);

    // LM head (untied).
    GET_LIN(weights.head.weight, "dec.head.weight", d_dec, vocab);
    GET_F32(weights.head.bias, "dec.head.bias", vocab);

    return TRANSCRIBE_OK;
}

#undef GET_F32
#undef GET_CONV
#undef GET_LIN

}  // namespace transcribe::canary
