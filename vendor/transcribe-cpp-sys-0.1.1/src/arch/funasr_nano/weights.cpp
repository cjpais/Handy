// arch/funasr_nano/weights.cpp - read_funasr_nano_hparams +
// build_funasr_nano_weights.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::funasr_nano {

namespace {
constexpr const char * kFamilyTag = "funasr_nano";
}

transcribe_status read_funasr_nano_hparams(const gguf_context * gguf, FunAsrNanoHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.n_blocks", kFamilyTag, hp.enc_n_blocks);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.tp_blocks", kFamilyTag, hp.enc_tp_blocks);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.d_input", kFamilyTag, hp.enc_d_input);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.d_ff", kFamilyTag, hp.enc_d_ff);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.kernel_size", kFamilyTag, hp.enc_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.encoder.sanm_shift", kFamilyTag, hp.enc_sanm_shift);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.funasr_nano.encoder.attention_type", kFamilyTag, hp.enc_attn_type);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.funasr_nano.encoder.normalize_before", kFamilyTag, true,
                                        hp.enc_normalize_before);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Adaptor.
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.n_blocks", kFamilyTag, hp.adaptor_n_blocks);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.encoder_dim", kFamilyTag, hp.adaptor_encoder_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.llm_dim", kFamilyTag, hp.adaptor_llm_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.pre_ffn_dim", kFamilyTag, hp.adaptor_pre_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.block_ffn_dim", kFamilyTag, hp.adaptor_block_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.n_heads", kFamilyTag, hp.adaptor_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.d_head", kFamilyTag, hp.adaptor_d_head);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_f32_kv(gguf, "stt.funasr_nano.adaptor.layer_norm_eps", kFamilyTag, hp.adaptor_layer_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_string_kv(gguf, "stt.funasr_nano.adaptor.activation", kFamilyTag, hp.adaptor_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.adaptor.downsample_rate", kFamilyTag,
                                       hp.adaptor_downsample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.funasr_nano.adaptor.use_low_frame_rate", kFamilyTag, true,
                                        hp.adaptor_use_low_frame_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Decoder (Qwen3-0.6B).
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.hidden_size", kFamilyTag, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.funasr_nano.decoder.intermediate_size", kFamilyTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.n_kv_heads", kFamilyTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.funasr_nano.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.funasr_nano.decoder.rms_norm_eps", kFamilyTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.funasr_nano.decoder.rope_theta", kFamilyTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.funasr_nano.decoder.tie_word_embeddings", kFamilyTag, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.funasr_nano.decoder.activation", kFamilyTag, hp.dec_activation);
        st != TRANSCRIBE_OK) {
        return st;
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
    if (auto st = read_required_string_kv(gguf, "stt.frontend.fbank_style", kFamilyTag, hp.fe_fbank_style);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.dither", kFamilyTag, hp.fe_dither); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.upscale_samples", kFamilyTag, true, hp.fe_upscale_samples);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.snip_edges", kFamilyTag, true, hp.fe_snip_edges);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.lfr_m", kFamilyTag, hp.fe_lfr_m); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.lfr_n", kFamilyTag, hp.fe_lfr_n); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.apply_cmvn", kFamilyTag, false, hp.fe_apply_cmvn);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Cross-field invariants.
    if (hp.enc_n_blocks <= 1 || hp.enc_tp_blocks < 0 || hp.enc_d_model <= 0 || hp.enc_d_input <= 0 ||
        hp.enc_n_heads <= 0 || hp.enc_d_ff <= 0 || hp.enc_kernel <= 0 || hp.enc_kernel % 2 == 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano: encoder hparams invalid (n_blocks=%d "
                "tp_blocks=%d d_model=%d d_input=%d n_heads=%d d_ff=%d "
                "kernel=%d — kernel must be positive and odd)",
                hp.enc_n_blocks, hp.enc_tp_blocks, hp.enc_d_model, hp.enc_d_input, hp.enc_n_heads, hp.enc_d_ff,
                hp.enc_kernel);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_input != hp.fe_num_mels * hp.fe_lfr_m) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano: encoder d_input (%d) must equal num_mels (%d) "
                "× lfr_m (%d) = %d",
                hp.enc_d_input, hp.fe_num_mels, hp.fe_lfr_m, hp.fe_num_mels * hp.fe_lfr_m);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_attn_type != "sanm") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: unsupported encoder attention type \"%s\"",
                hp.enc_attn_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.adaptor_n_heads <= 0 || hp.adaptor_d_head <= 0 ||
        hp.adaptor_llm_dim != hp.adaptor_n_heads * hp.adaptor_d_head) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: adaptor llm_dim (%d) != n_heads (%d) * d_head (%d)",
                hp.adaptor_llm_dim, hp.adaptor_n_heads, hp.adaptor_d_head);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.adaptor_activation != "relu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano: unsupported adaptor activation \"%s\" "
                "(only \"relu\" is implemented)",
                hp.adaptor_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_hidden <= 0 || hp.dec_n_heads <= 0 || hp.dec_n_kv_heads <= 0 ||
        hp.dec_head_dim <= 0 || hp.dec_intermediate <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: dec n_heads (%d) not divisible by n_kv_heads (%d)",
                hp.dec_n_heads, hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_activation != "silu" && hp.dec_activation != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: unsupported decoder activation \"%s\"",
                hp.dec_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (!hp.dec_tie_word_embeddings) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano: decoder.tie_word_embeddings=false is not supported "
                "(graph reuses dec.token_embd.weight as the lm_head)");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.adaptor_llm_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: adaptor.llm_dim (%d) != decoder.hidden_size (%d)",
                hp.adaptor_llm_dim, hp.dec_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "kaldi_fbank_lfr") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: unsupported frontend type \"%s\"", hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_window != "hamming") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: unsupported frontend window \"%s\"", hp.fe_window.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_normalize != "none") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "funasr_nano: unsupported frontend normalize \"%s\" "
                "(only \"none\" is implemented; CMVN was dropped at convert)",
                hp.fe_normalize.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_fbank_style != "kaldi_htk") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: unsupported fbank_style \"%s\"", hp.fe_fbank_style.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Tensor catalog
// ---------------------------------------------------------------------------

namespace {

using transcribe::weights::find_tensor;
using transcribe::weights::lname;

constexpr const char * kTag = kFamilyTag;

#define GET_F32(slot, name, ...)                                                                    \
    do {                                                                                            \
        ggml_tensor * _t = find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { __VA_ARGS__ }, kTag); \
        if (_t == nullptr)                                                                          \
            return TRANSCRIBE_ERR_GGUF;                                                             \
        (slot) = _t;                                                                                \
    } while (0)

#define GET_CONV(slot, name, ...)                                                                                 \
    do {                                                                                                          \
        ggml_tensor * _t = find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_CONV_TYPES }, { __VA_ARGS__ }, kTag); \
        if (_t == nullptr)                                                                                        \
            return TRANSCRIBE_ERR_GGUF;                                                                           \
        (slot) = _t;                                                                                              \
    } while (0)

#define GET_LIN(slot, name, ...)                                                                                    \
    do {                                                                                                            \
        ggml_tensor * _t = find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_LINEAR_TYPES }, { __VA_ARGS__ }, kTag); \
        if (_t == nullptr)                                                                                          \
            return TRANSCRIBE_ERR_GGUF;                                                                             \
        (slot) = _t;                                                                                                \
    } while (0)

transcribe_status load_enc_block(ggml_context * ctx_meta,
                                 const char *   prefix,
                                 int64_t        d_in,
                                 int64_t        d_model,
                                 int64_t        d_ff,
                                 int64_t        kernel,
                                 EncBlock &     b) {
    std::string p = prefix;

    GET_F32(b.norm_attn_w, (p + ".norm_attn.weight").c_str(), d_in);
    GET_F32(b.norm_attn_b, (p + ".norm_attn.bias").c_str(), d_in);
    GET_LIN(b.attn_qkv_w, (p + ".attn.qkv.weight").c_str(), d_in, 3 * d_model);
    GET_F32(b.attn_qkv_b, (p + ".attn.qkv.bias").c_str(), 3 * d_model);
    GET_LIN(b.attn_out_w, (p + ".attn.out.weight").c_str(), d_model, d_model);
    GET_F32(b.attn_out_b, (p + ".attn.out.bias").c_str(), d_model);
    GET_CONV(b.attn_fsmn_w, (p + ".attn.fsmn.weight").c_str(), kernel, 1, d_model);
    GET_F32(b.norm_ffn_w, (p + ".norm_ffn.weight").c_str(), d_model);
    GET_F32(b.norm_ffn_b, (p + ".norm_ffn.bias").c_str(), d_model);
    GET_LIN(b.ffn_fc1_w, (p + ".ffn.fc1.weight").c_str(), d_model, d_ff);
    GET_F32(b.ffn_fc1_b, (p + ".ffn.fc1.bias").c_str(), d_ff);
    GET_LIN(b.ffn_fc2_w, (p + ".ffn.fc2.weight").c_str(), d_ff, d_model);
    GET_F32(b.ffn_fc2_b, (p + ".ffn.fc2.bias").c_str(), d_model);
    return TRANSCRIBE_OK;
}

transcribe_status load_adaptor_block(ggml_context * ctx_meta,
                                     const char *   prefix,
                                     int64_t        llm_dim,
                                     int64_t        block_ffn_dim,
                                     AdaptorBlock & b) {
    std::string p = prefix;

    GET_F32(b.norm_attn_w, (p + ".norm_attn.weight").c_str(), llm_dim);
    GET_F32(b.norm_attn_b, (p + ".norm_attn.bias").c_str(), llm_dim);
    GET_LIN(b.attn_q_w, (p + ".attn.q.weight").c_str(), llm_dim, llm_dim);
    GET_F32(b.attn_q_b, (p + ".attn.q.bias").c_str(), llm_dim);
    GET_LIN(b.attn_k_w, (p + ".attn.k.weight").c_str(), llm_dim, llm_dim);
    GET_F32(b.attn_k_b, (p + ".attn.k.bias").c_str(), llm_dim);
    GET_LIN(b.attn_v_w, (p + ".attn.v.weight").c_str(), llm_dim, llm_dim);
    GET_F32(b.attn_v_b, (p + ".attn.v.bias").c_str(), llm_dim);
    GET_LIN(b.attn_out_w, (p + ".attn.out.weight").c_str(), llm_dim, llm_dim);
    GET_F32(b.attn_out_b, (p + ".attn.out.bias").c_str(), llm_dim);
    GET_F32(b.norm_ffn_w, (p + ".norm_ffn.weight").c_str(), llm_dim);
    GET_F32(b.norm_ffn_b, (p + ".norm_ffn.bias").c_str(), llm_dim);
    GET_LIN(b.ffn_fc1_w, (p + ".ffn.fc1.weight").c_str(), llm_dim, block_ffn_dim);
    GET_F32(b.ffn_fc1_b, (p + ".ffn.fc1.bias").c_str(), block_ffn_dim);
    GET_LIN(b.ffn_fc2_w, (p + ".ffn.fc2.weight").c_str(), block_ffn_dim, llm_dim);
    GET_F32(b.ffn_fc2_b, (p + ".ffn.fc2.bias").c_str(), llm_dim);
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status build_funasr_nano_weights(ggml_context *            ctx_meta,
                                            const FunAsrNanoHParams & hp,
                                            FunAsrNanoWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_input = hp.enc_d_input;
    const int64_t d_model = hp.enc_d_model;
    const int64_t d_ff    = hp.enc_d_ff;
    const int64_t kernel  = hp.enc_kernel;

    // ----- encoder: encoders0[0] (560 → 512 projection) -----
    if (auto st = load_enc_block(ctx_meta, "enc.encoders0.0",
                                 /*d_in=*/d_input, d_model, d_ff, kernel, weights.encoders0);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // ----- encoder: encoders[0..n_blocks-2] -----
    weights.encoders.assign(hp.enc_n_blocks - 1, EncBlock{});
    for (int i = 0; i < hp.enc_n_blocks - 1; ++i) {
        char prefix[64];
        std::snprintf(prefix, sizeof(prefix), "enc.encoders.%d", i);
        if (auto st = load_enc_block(ctx_meta, prefix, d_model, d_model, d_ff, kernel, weights.encoders[i]);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // ----- encoder: after_norm -----
    GET_F32(weights.after_norm_w, "enc.after_norm.weight", d_model);
    GET_F32(weights.after_norm_b, "enc.after_norm.bias", d_model);

    // ----- encoder: tp_encoders[0..tp_blocks-1] -----
    weights.tp_encoders.assign(hp.enc_tp_blocks, EncBlock{});
    for (int i = 0; i < hp.enc_tp_blocks; ++i) {
        char prefix[64];
        std::snprintf(prefix, sizeof(prefix), "enc.tp_encoders.%d", i);
        if (auto st = load_enc_block(ctx_meta, prefix, d_model, d_model, d_ff, kernel, weights.tp_encoders[i]);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // ----- encoder: tp_norm -----
    GET_F32(weights.tp_norm_w, "enc.tp_norm.weight", d_model);
    GET_F32(weights.tp_norm_b, "enc.tp_norm.bias", d_model);

    // ----- adaptor: linear1 / linear2 -----
    const int64_t encoder_dim   = hp.adaptor_encoder_dim;
    const int64_t llm_dim       = hp.adaptor_llm_dim;
    const int64_t pre_ffn_dim   = hp.adaptor_pre_ffn_dim;
    const int64_t block_ffn_dim = hp.adaptor_block_ffn_dim;
    GET_LIN(weights.adaptor.linear1_w, "adaptor.linear1.weight", encoder_dim, pre_ffn_dim);
    GET_F32(weights.adaptor.linear1_b, "adaptor.linear1.bias", pre_ffn_dim);
    GET_LIN(weights.adaptor.linear2_w, "adaptor.linear2.weight", pre_ffn_dim, llm_dim);
    GET_F32(weights.adaptor.linear2_b, "adaptor.linear2.bias", llm_dim);

    // ----- adaptor blocks -----
    weights.adaptor.blocks.assign(hp.adaptor_n_blocks, AdaptorBlock{});
    for (int i = 0; i < hp.adaptor_n_blocks; ++i) {
        char prefix[64];
        std::snprintf(prefix, sizeof(prefix), "adaptor.blocks.%d", i);
        if (auto st = load_adaptor_block(ctx_meta, prefix, llm_dim, block_ffn_dim, weights.adaptor.blocks[i]);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // ----- decoder: token embed (tied to lm_head) -----
    {
        ggml_tensor * tw = ggml_get_tensor(ctx_meta, "dec.token_embd.weight");
        if (tw == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: missing tensor \"dec.token_embd.weight\"");
            return TRANSCRIBE_ERR_GGUF;
        }
        if (tw->ne[0] != hp.dec_hidden) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: dec.token_embd.weight ne[0]=%lld, expected %lld",
                    static_cast<long long>(tw->ne[0]), static_cast<long long>(hp.dec_hidden));
            return TRANSCRIBE_ERR_GGUF;
        }
        if (tw->ne[1] != hp.dec_vocab_size) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: dec.token_embd.weight ne[1]=%lld, expected %lld",
                    static_cast<long long>(tw->ne[1]), static_cast<long long>(hp.dec_vocab_size));
            return TRANSCRIBE_ERR_GGUF;
        }
        weights.dec_embed.token_w = tw;
    }

    // ----- decoder: layers -----
    const int64_t dec_h   = hp.dec_hidden;
    const int64_t dec_nh  = hp.dec_n_heads;
    const int64_t dec_nkv = hp.dec_n_kv_heads;
    const int64_t dec_hd  = hp.dec_head_dim;
    const int64_t dec_im  = hp.dec_intermediate;
    const int64_t q_out   = dec_nh * dec_hd;
    const int64_t kv_out  = dec_nkv * dec_hd;

    weights.dec_blocks.assign(hp.dec_n_layers, DecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];
        GET_F32(b.norm_attn_w, lname("dec.layers.%d.norm_attn.weight", i), dec_h);
        GET_F32(b.norm_ffn_w, lname("dec.layers.%d.norm_ffn.weight", i), dec_h);
        GET_LIN(b.attn_q_w, lname("dec.layers.%d.attn.q.weight", i), dec_h, q_out);
        GET_LIN(b.attn_k_w, lname("dec.layers.%d.attn.k.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_v_w, lname("dec.layers.%d.attn.v.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_o_w, lname("dec.layers.%d.attn.o.weight", i), q_out, dec_h);
        GET_F32(b.attn_q_norm, lname("dec.layers.%d.attn.q_norm.weight", i), dec_hd);
        GET_F32(b.attn_k_norm, lname("dec.layers.%d.attn.k_norm.weight", i), dec_hd);
        GET_LIN(b.ffn_gate_w, lname("dec.layers.%d.ffn.gate.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_up_w, lname("dec.layers.%d.ffn.up.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_down_w, lname("dec.layers.%d.ffn.down.weight", i), dec_im, dec_h);

        if (b.ffn_gate_w->type != b.ffn_up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "funasr_nano: ffn gate/up dtype mismatch at layer %d", i);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- decoder: final norm -----
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);

    return TRANSCRIBE_OK;
}

#undef GET_F32
#undef GET_CONV
#undef GET_LIN

}  // namespace transcribe::funasr_nano
