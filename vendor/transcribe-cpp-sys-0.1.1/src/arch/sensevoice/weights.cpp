// arch/sensevoice/weights.cpp - read_sensevoice_hparams +
// build_sensevoice_weights.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <initializer_list>

namespace transcribe::sensevoice {

namespace {

constexpr const char * kFamilyTag = "sensevoice";

}  // namespace

transcribe_status read_sensevoice_hparams(const gguf_context * gguf, SenseVoiceHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.n_blocks", kFamilyTag, hp.enc_n_blocks);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.tp_blocks", kFamilyTag, hp.enc_tp_blocks);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.d_input", kFamilyTag, hp.enc_d_input);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.d_ff", kFamilyTag, hp.enc_d_ff);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.kernel_size", kFamilyTag, hp.enc_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.encoder.sanm_shift", kFamilyTag, hp.enc_sanm_shift);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.sensevoice.encoder.attention_type", kFamilyTag, hp.enc_attn_type);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.sensevoice.encoder.normalize_before", kFamilyTag, true,
                                        hp.enc_normalize_before);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Pre-encoder prefix indices. Required so the runtime never has to
    // hard-code lid_dict / textnorm_dict layouts.
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_auto", kFamilyTag, hp.prefix_lang_auto);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_zh", kFamilyTag, hp.prefix_lang_zh);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_en", kFamilyTag, hp.prefix_lang_en);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_yue", kFamilyTag, hp.prefix_lang_yue);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_ja", kFamilyTag, hp.prefix_lang_ja);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.lang_ko", kFamilyTag, hp.prefix_lang_ko);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.sensevoice.special.lang_nospeech", kFamilyTag, hp.prefix_lang_nospeech);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.withitn", kFamilyTag, hp.prefix_withitn);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.sensevoice.special.woitn", kFamilyTag, hp.prefix_woitn);
        st != TRANSCRIBE_OK) {
        return st;
    }
    // event_speech / emotion_neutral are documentation-only KV; the
    // hard-coded literal [1, 2] in the upstream forward is what the
    // C++ encoder uses, so we don't read these keys into hparams.

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

    // Cross-field invariants.
    if (hp.enc_n_blocks <= 1 || hp.enc_tp_blocks < 0 || hp.enc_d_model <= 0 || hp.enc_d_input <= 0 ||
        hp.enc_n_heads <= 0 || hp.enc_d_ff <= 0 || hp.enc_kernel <= 0 || hp.enc_kernel % 2 == 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: encoder hparams invalid (n_blocks=%d "
                "tp_blocks=%d d_model=%d d_input=%d n_heads=%d d_ff=%d "
                "kernel=%d — kernel must be positive and odd)",
                hp.enc_n_blocks, hp.enc_tp_blocks, hp.enc_d_model, hp.enc_d_input, hp.enc_n_heads, hp.enc_d_ff,
                hp.enc_kernel);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_num_mels <= 0 || hp.fe_sample_rate <= 0 || hp.fe_n_fft <= 0 || hp.fe_win_length <= 0 ||
        hp.fe_hop_length <= 0 || hp.fe_lfr_m <= 0 || hp.fe_lfr_n <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice: frontend dimensions must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_input != hp.fe_num_mels * hp.fe_lfr_m) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: encoder d_input (%d) must equal num_mels (%d) "
                "× lfr_m (%d) = %d",
                hp.enc_d_input, hp.fe_num_mels, hp.fe_lfr_m, hp.fe_num_mels * hp.fe_lfr_m);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_attn_type != "sanm") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: unsupported attention type \"%s\" "
                "(only \"sanm\" is implemented)",
                hp.enc_attn_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "kaldi_fbank_lfr") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: unsupported frontend type \"%s\" "
                "(only \"kaldi_fbank_lfr\" is implemented)",
                hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_window != "hamming") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: unsupported frontend window \"%s\" "
                "(only \"hamming\" is implemented)",
                hp.fe_window.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_normalize != "per_feature") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: unsupported frontend normalize \"%s\" "
                "(only \"per_feature\" is implemented)",
                hp.fe_normalize.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_fbank_style != "kaldi_htk") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: unsupported fbank_style \"%s\" "
                "(only \"kaldi_htk\" is implemented)",
                hp.fe_fbank_style.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

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

// Bind one SAN-M block (13 tensors). `prefix` is e.g. "enc.encoders.7"
// or "enc.encoders0.0". `d_in_norm` and `d_in_qkv` are the input-side
// dims (= d_model except for encoders0[0] where they equal d_input).
transcribe_status load_block(ggml_context *    ctx_meta,
                             const char *      prefix,
                             int64_t           d_in,
                             int64_t           d_model,
                             int64_t           d_ff,
                             int64_t           kernel,
                             SenseVoiceBlock & b) {
    char buf[256];

    auto namef = [&](const char * suffix) -> const char * {
        std::snprintf(buf, sizeof(buf), "%s.%s", prefix, suffix);
        return buf;
    };
    // Names are built as local std::strings so two can be live in one
    // expression without the single `buf` aliasing.
    std::string name_norm_attn_w = std::string(prefix) + ".norm_attn.weight";
    std::string name_norm_attn_b = std::string(prefix) + ".norm_attn.bias";
    std::string name_qkv_w       = std::string(prefix) + ".attn.qkv.weight";
    std::string name_qkv_b       = std::string(prefix) + ".attn.qkv.bias";
    std::string name_out_w       = std::string(prefix) + ".attn.out.weight";
    std::string name_out_b       = std::string(prefix) + ".attn.out.bias";
    std::string name_fsmn_w      = std::string(prefix) + ".attn.fsmn.weight";
    std::string name_norm_ffn_w  = std::string(prefix) + ".norm_ffn.weight";
    std::string name_norm_ffn_b  = std::string(prefix) + ".norm_ffn.bias";
    std::string name_fc1_w       = std::string(prefix) + ".ffn.fc1.weight";
    std::string name_fc1_b       = std::string(prefix) + ".ffn.fc1.bias";
    std::string name_fc2_w       = std::string(prefix) + ".ffn.fc2.weight";
    std::string name_fc2_b       = std::string(prefix) + ".ffn.fc2.bias";

    GET_F32(b.norm_attn_w, name_norm_attn_w.c_str(), d_in);
    GET_F32(b.norm_attn_b, name_norm_attn_b.c_str(), d_in);
    // Fused QKV: PyTorch [out=3·d_model, in=d_in]; ggml ne[0]=in, ne[1]=out.
    GET_LIN(b.attn_qkv_w, name_qkv_w.c_str(), d_in, 3 * d_model);
    GET_F32(b.attn_qkv_b, name_qkv_b.c_str(), 3 * d_model);
    // attn_out: PyTorch [d_model, d_model] always operates in d_model space.
    GET_LIN(b.attn_out_w, name_out_w.c_str(), d_model, d_model);
    GET_F32(b.attn_out_b, name_out_b.c_str(), d_model);
    // FSMN depthwise conv. PyTorch shape (out=d_model, in_per_group=1, kernel).
    // ggml stores conv1d weights as [kernel, in_per_group, out] in ne order
    // — see parakeet's depthwise convention.
    GET_CONV(b.attn_fsmn_w, name_fsmn_w.c_str(), kernel, 1, d_model);

    GET_F32(b.norm_ffn_w, name_norm_ffn_w.c_str(), d_model);
    GET_F32(b.norm_ffn_b, name_norm_ffn_b.c_str(), d_model);
    GET_LIN(b.ffn_fc1_w, name_fc1_w.c_str(), d_model, d_ff);
    GET_F32(b.ffn_fc1_b, name_fc1_b.c_str(), d_ff);
    GET_LIN(b.ffn_fc2_w, name_fc2_w.c_str(), d_ff, d_model);
    GET_F32(b.ffn_fc2_b, name_fc2_b.c_str(), d_model);
    (void) namef;
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status build_sensevoice_weights(ggml_context *            ctx_meta,
                                           const SenseVoiceHParams & hp,
                                           SenseVoiceWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_input = hp.enc_d_input;
    const int64_t d_model = hp.enc_d_model;
    const int64_t d_ff    = hp.enc_d_ff;
    const int64_t kernel  = hp.enc_kernel;
    const int64_t vocab   = hp.vocab_size;

    if (vocab <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice: build_sensevoice_weights called before "
                "tokenizer load (vocab_size=%d)",
                hp.vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- frontend (per-feature CMVN) -----
    GET_F32(weights.cmvn_shift, "frontend.cmvn.shift", d_input);
    GET_F32(weights.cmvn_scale, "frontend.cmvn.scale", d_input);

    // ----- pre-encoder prefix embedding -----
    // 16 rows × d_input cols. Embed table rows are read with
    // ggml_get_rows; PyTorch shape [16, d_input] → ggml ne[0]=d_input,
    // ne[1]=16.
    GET_F32(weights.embed, "enc.embed.weight", d_input, 16);

    // ----- encoders0[0] (560 → 512 projection) -----
    if (auto st = load_block(ctx_meta, "enc.encoders0.0",
                             /*d_in=*/d_input, d_model, d_ff, kernel, weights.encoders0);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // ----- encoders[0..n_blocks-2] (49 blocks at d_model) -----
    weights.encoders.assign(hp.enc_n_blocks - 1, SenseVoiceBlock{});
    for (int i = 0; i < hp.enc_n_blocks - 1; ++i) {
        char prefix[64];
        std::snprintf(prefix, sizeof(prefix), "enc.encoders.%d", i);
        if (auto st =
                load_block(ctx_meta, prefix, d_model, d_model, d_ff, kernel, weights.encoders[static_cast<size_t>(i)]);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // ----- after_norm -----
    GET_F32(weights.after_norm_w, "enc.after_norm.weight", d_model);
    GET_F32(weights.after_norm_b, "enc.after_norm.bias", d_model);

    // ----- tp_encoders[0..tp_blocks-1] -----
    weights.tp_encoders.assign(hp.enc_tp_blocks, SenseVoiceBlock{});
    for (int i = 0; i < hp.enc_tp_blocks; ++i) {
        char prefix[64];
        std::snprintf(prefix, sizeof(prefix), "enc.tp_encoders.%d", i);
        if (auto st = load_block(ctx_meta, prefix, d_model, d_model, d_ff, kernel,
                                 weights.tp_encoders[static_cast<size_t>(i)]);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // ----- tp_norm -----
    GET_F32(weights.tp_norm_w, "enc.tp_norm.weight", d_model);
    GET_F32(weights.tp_norm_b, "enc.tp_norm.bias", d_model);

    // ----- CTC head -----
    // PyTorch [vocab, d_model] → ggml ne[0]=d_model, ne[1]=vocab.
    GET_LIN(weights.ctc_head_w, "ctc.head.weight", d_model, vocab);
    GET_F32(weights.ctc_head_b, "ctc.head.bias", vocab);

    return TRANSCRIBE_OK;
}

#undef GET_F32
#undef GET_CONV
#undef GET_LIN

}  // namespace transcribe::sensevoice
