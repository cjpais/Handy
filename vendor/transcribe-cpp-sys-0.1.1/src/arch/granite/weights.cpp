// arch/granite/weights.cpp - read_granite_hparams + build_granite_weights.
//
// Pattern mirrors arch/qwen3_asr/weights.cpp. Every required KV is
// read explicitly; BadType is always fatal. The tensor catalog is a
// sequence of find_tensor() calls with expected shapes; missing tensor
// or shape mismatch returns TRANSCRIBE_ERR_GGUF.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::granite {

namespace {
constexpr const char * kFamilyTag = "granite";
}

transcribe_status read_granite_hparams(const gguf_context * gguf, GraniteHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Identity.
    if (auto st = read_optional_string_kv(gguf, "stt.variant", kFamilyTag, "", hp.variant); st != TRANSCRIBE_OK) {
        return st;
    }

    // Encoder (Conformer).
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.hidden", kFamilyTag, hp.enc_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.head_dim", kFamilyTag, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.input_dim", kFamilyTag, hp.enc_input_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.output_dim", kFamilyTag, hp.enc_output_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.granite.encoder.feedforward_mult", kFamilyTag, hp.enc_feedforward_mult);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.granite.encoder.conv_kernel_size", kFamilyTag, hp.enc_conv_kernel_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.conv_expansion", kFamilyTag, hp.enc_conv_expansion);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.max_pos_emb", kFamilyTag, hp.enc_max_pos_emb);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.encoder.context_size", kFamilyTag, hp.enc_context_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // cat_hidden_layers is an optional array. Absent or empty array both
    // mean "no concat". Converter writes the uint32 array via gguf-py;
    // we read via read_int32_array_kv (uint32 ↔ int32 is safe for layer
    // indices in [0, num_layers)).
    {
        std::vector<int32_t> tmp;
        switch (read_int32_array_kv(gguf, "stt.granite.encoder.cat_hidden_layers", tmp)) {
            case KvResult::Ok:
                hp.enc_cat_hidden_layers = std::move(tmp);
                break;
            case KvResult::Absent:
                hp.enc_cat_hidden_layers.clear();
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: stt.granite.encoder.cat_hidden_layers has wrong type");
                return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Projector (BLIP-2 Q-Former).
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.n_layers", kFamilyTag, hp.prj_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.hidden", kFamilyTag, hp.prj_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.intermediate", kFamilyTag, hp.prj_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.n_heads", kFamilyTag, hp.prj_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.encoder_hidden_size", kFamilyTag,
                                       hp.prj_encoder_hidden_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.cross_attn_frequency", kFamilyTag,
                                       hp.prj_cross_attn_freq);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.granite.projector.hidden_act", kFamilyTag, hp.prj_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.projector.layer_norm_eps", kFamilyTag, hp.prj_layer_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.projector.max_pos_emb", kFamilyTag, hp.prj_max_pos_emb);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.granite.projector.position_embedding_type", kFamilyTag,
                                          hp.prj_pos_embed_type);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Text LM (Granite-4).
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.hidden_size", kFamilyTag, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.intermediate_size", kFamilyTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.n_kv_heads", kFamilyTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.granite.decoder.hidden_act", kFamilyTag, hp.dec_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.rms_norm_eps", kFamilyTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.rope_theta", kFamilyTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.granite.decoder.tie_word_embeddings", kFamilyTag, false,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Granite-4 scalar multipliers. All four are required — missing any
    // of them silently degrades accuracy without crashing.
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.embedding_multiplier", kFamilyTag,
                                       hp.dec_embedding_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.logits_scaling", kFamilyTag, hp.dec_logits_scaling);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.attention_multiplier", kFamilyTag,
                                       hp.dec_attention_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite.decoder.residual_multiplier", kFamilyTag,
                                       hp.dec_residual_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Audio fusion.
    if (auto st = read_required_u32_kv(gguf, "stt.granite.audio_token_id", kFamilyTag, hp.audio_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.downsample_rate", kFamilyTag, hp.downsample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite.window_size", kFamilyTag, hp.window_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Frontend (torchaudio MelSpectrogram).
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
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.mel_norm", kFamilyTag, "htk", hp.fe_mel_norm);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.center", kFamilyTag, true, hp.fe_center);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Cross-field invariants.
    if (hp.enc_n_layers <= 0 || hp.enc_hidden <= 0 || hp.enc_n_heads <= 0 || hp.enc_head_dim <= 0 ||
        hp.enc_input_dim <= 0 || hp.enc_output_dim <= 0 || hp.enc_feedforward_mult <= 0 ||
        hp.enc_conv_kernel_size <= 0 || hp.enc_conv_expansion <= 0 || hp.enc_max_pos_emb <= 0 ||
        hp.enc_context_size <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_n_heads * hp.enc_head_dim != hp.enc_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: encoder n_heads * head_dim (%d * %d) "
                "!= hidden (%d)",
                hp.enc_n_heads, hp.enc_head_dim, hp.enc_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_context_size > hp.enc_max_pos_emb) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: context_size (%d) > max_pos_emb (%d)", hp.enc_context_size,
                hp.enc_max_pos_emb);
        return TRANSCRIBE_ERR_GGUF;
    }
    // cat_hidden_layers entries must reference real layers and the
    // expanded projector input dim must match prj_encoder_hidden_size.
    {
        const int32_t expected_kv_dim = hp.enc_hidden * (static_cast<int32_t>(hp.enc_cat_hidden_layers.size()) + 1);
        if (expected_kv_dim != hp.prj_encoder_hidden_size) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "granite: prj.encoder_hidden_size (%d) does not match "
                    "hidden*(1+|cat_hidden_layers|) = %d * (1 + %d) = %d",
                    hp.prj_encoder_hidden_size, hp.enc_hidden, static_cast<int>(hp.enc_cat_hidden_layers.size()),
                    expected_kv_dim);
            return TRANSCRIBE_ERR_GGUF;
        }
        for (int32_t idx : hp.enc_cat_hidden_layers) {
            if (idx < 0 || idx >= hp.enc_n_layers) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "granite: cat_hidden_layers entry %d out of range "
                        "[0, %d)",
                        idx, hp.enc_n_layers);
                return TRANSCRIBE_ERR_GGUF;
            }
        }
    }
    if (hp.prj_n_layers <= 0 || hp.prj_hidden <= 0 || hp.prj_intermediate <= 0 || hp.prj_n_heads <= 0 ||
        hp.prj_encoder_hidden_size <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: projector hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.prj_hidden % hp.prj_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: projector hidden (%d) not divisible by n_heads (%d)",
                hp.prj_hidden, hp.prj_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.prj_hidden_act != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: unsupported projector hidden_act \"%s\" "
                "(only gelu)",
                hp.prj_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.prj_pos_embed_type != "absolute") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: unsupported projector position_embedding_type "
                "\"%s\" (only absolute)",
                hp.prj_pos_embed_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_hidden <= 0 || hp.dec_n_heads <= 0 || hp.dec_n_kv_heads <= 0 ||
        hp.dec_head_dim <= 0 || hp.dec_intermediate <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: dec n_heads (%d) not divisible by n_kv_heads (%d)",
                hp.dec_n_heads, hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads * hp.dec_head_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: dec n_heads * head_dim (%d * %d) != hidden (%d)", hp.dec_n_heads,
                hp.dec_head_dim, hp.dec_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_hidden_act != "silu" && hp.dec_hidden_act != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: unsupported decoder hidden_act \"%s\" "
                "(only silu/swish)",
                hp.dec_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_embedding_multiplier <= 0.0f || hp.dec_logits_scaling <= 0.0f || hp.dec_attention_multiplier <= 0.0f ||
        hp.dec_residual_multiplier <= 0.0f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: decoder multipliers must be > 0 "
                "(emb=%g, logits=%g, attn=%g, residual=%g)",
                hp.dec_embedding_multiplier, hp.dec_logits_scaling, hp.dec_attention_multiplier,
                hp.dec_residual_multiplier);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: unsupported frontend type \"%s\"", hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_normalize != "per_utterance") {
        // GraniteSpeechFeatureExtractor applies whisper-style per-utterance
        // normalization (log10 → max-8 floor → /4 + 1). The C++ MelFrontend
        // produces a bit-identical result under "per_utterance" mode.
        // Other modes would mean the converter changed; fail loudly.
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: unsupported frontend normalize \"%s\" "
                "(only \"per_utterance\")",
                hp.fe_normalize.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_mel_norm != "htk") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "granite: unsupported frontend mel_norm \"%s\" "
                "(only \"htk\")",
                hp.fe_mel_norm.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    // Audio injection precondition: projector lifts to LM hidden, so
    // the linear-lift output dim must equal dec.hidden.
    // (We check that on the actual tensor shape during build_weights.)

    return TRANSCRIBE_OK;
}

// Weights.

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

transcribe_status build_granite_weights(ggml_context * ctx_meta, const GraniteHParams & hp, GraniteWeights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Derived dims.
    const int64_t enc_h        = hp.enc_hidden;
    const int64_t enc_in       = hp.enc_input_dim;
    const int64_t enc_out      = hp.enc_output_dim;
    const int64_t enc_n_heads  = hp.enc_n_heads;
    const int64_t enc_head_dim = hp.enc_head_dim;
    const int64_t enc_inner    = enc_n_heads * enc_head_dim;  // typically == enc_h
    const int64_t enc_ffn      = enc_h * hp.enc_feedforward_mult;
    const int64_t conv_inner   = enc_h * hp.enc_conv_expansion;
    const int64_t conv_up_out  = conv_inner * 2;  // pre-GLU
    const int64_t conv_k       = hp.enc_conv_kernel_size;
    const int64_t rel_pos_len  = 2 * hp.enc_max_pos_emb + 1;

    const int64_t prj_h        = hp.prj_hidden;
    const int64_t prj_im       = hp.prj_intermediate;
    const int64_t prj_kv_in    = hp.prj_encoder_hidden_size;  // 1024 or 2048
    const int64_t prj_n_layers = hp.prj_n_layers;
    // num_queries (3 on granite) is not in KV — it's the ne[1] of the
    // query tensor. Read it implicitly via find_tensor's ne[] match.

    const int64_t dec_h     = hp.dec_hidden;
    const int64_t dec_vocab = hp.dec_vocab_size;
    const int64_t dec_nh    = hp.dec_n_heads;
    const int64_t dec_nkv   = hp.dec_n_kv_heads;
    const int64_t dec_hd    = hp.dec_head_dim;
    const int64_t dec_im    = hp.dec_intermediate;
    const int64_t q_out     = dec_nh * dec_hd;
    const int64_t kv_out    = dec_nkv * dec_hd;

    // Encoder: top-level.
    GET_LIN(weights.enc_top.input_linear_w, "enc.input_linear.weight", enc_in, enc_h);
    GET_F32(weights.enc_top.input_linear_b, "enc.input_linear.bias", enc_h);
    GET_LIN(weights.enc_top.ctc_proj_w, "enc.ctc_proj.weight", enc_h, enc_out);
    GET_F32(weights.enc_top.ctc_proj_b, "enc.ctc_proj.bias", enc_out);
    GET_LIN(weights.enc_top.ctc_bypass_w, "enc.ctc_bypass.weight", enc_out, enc_h);
    GET_F32(weights.enc_top.ctc_bypass_b, "enc.ctc_bypass.bias", enc_h);

    // Encoder: per-block.
    weights.enc_blocks.assign(hp.enc_n_layers, GraniteEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];

        // FF1 (macaron half).
        GET_F32(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i), enc_h);
        GET_F32(b.norm_ff1_b, lname("enc.blocks.%d.norm_ff1.bias", i), enc_h);
        GET_LIN(b.ff1_up_w, lname("enc.blocks.%d.ff1.up.weight", i), enc_h, enc_ffn);
        GET_F32(b.ff1_up_b, lname("enc.blocks.%d.ff1.up.bias", i), enc_ffn);
        GET_LIN(b.ff1_down_w, lname("enc.blocks.%d.ff1.down.weight", i), enc_ffn, enc_h);
        GET_F32(b.ff1_down_b, lname("enc.blocks.%d.ff1.down.bias", i), enc_h);

        // Block-local Shaw self-attention.
        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), enc_h);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), enc_h);
        // attn.q : [enc_h, inner_dim]. No upstream bias.
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), enc_h, enc_inner);
        // attn.kv fused output dim is 2*inner_dim. No bias.
        GET_LIN(b.attn_kv_w, lname("enc.blocks.%d.attn.kv.weight", i), enc_h, 2 * enc_inner);
        // attn.out has bias.
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), enc_inner, enc_h);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.out.bias", i), enc_h);
        // Shaw rel-pos embedding table. find_tensor accepts any linear
        // dtype here (BF16/F16/F32) — the loader will keep whatever the
        // converter emitted.
        GET_LIN(b.attn_rel_pos_emb, lname("enc.blocks.%d.attn.rel_pos_emb.weight", i), enc_head_dim, rel_pos_len);

        // Conv module (LN -> pointwise expand -> GLU -> depthwise -> BN -> SiLU -> pointwise contract).
        GET_F32(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i), enc_h);
        GET_F32(b.norm_conv_b, lname("enc.blocks.%d.norm_conv.bias", i), enc_h);
        // Conv1d kernel layout (k, in/groups, out_channels). pointwise k=1.
        GET_CONV(b.conv_pointwise1_w, lname("enc.blocks.%d.conv.pointwise1.weight", i), 1, enc_h, conv_up_out);
        GET_F32(b.conv_pointwise1_b, lname("enc.blocks.%d.conv.pointwise1.bias", i), conv_up_out);
        // Depthwise: groups = in_channels = conv_inner.
        GET_CONV(b.conv_depthwise_w, lname("enc.blocks.%d.conv.depthwise.weight", i), conv_k, 1, conv_inner);
        // BatchNorm1d fields are F32 (the converter routes anything
        // matching .bn. into the F32 bucket).
        GET_F32(b.conv_bn_w, lname("enc.blocks.%d.conv.bn.weight", i), conv_inner);
        GET_F32(b.conv_bn_b, lname("enc.blocks.%d.conv.bn.bias", i), conv_inner);
        GET_F32(b.conv_bn_mean, lname("enc.blocks.%d.conv.bn.running_mean", i), conv_inner);
        GET_F32(b.conv_bn_var, lname("enc.blocks.%d.conv.bn.running_var", i), conv_inner);
        GET_CONV(b.conv_pointwise2_w, lname("enc.blocks.%d.conv.pointwise2.weight", i), 1, conv_inner, enc_h);
        GET_F32(b.conv_pointwise2_b, lname("enc.blocks.%d.conv.pointwise2.bias", i), enc_h);

        // FF2 (macaron half).
        GET_F32(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i), enc_h);
        GET_F32(b.norm_ff2_b, lname("enc.blocks.%d.norm_ff2.bias", i), enc_h);
        GET_LIN(b.ff2_up_w, lname("enc.blocks.%d.ff2.up.weight", i), enc_h, enc_ffn);
        GET_F32(b.ff2_up_b, lname("enc.blocks.%d.ff2.up.bias", i), enc_ffn);
        GET_LIN(b.ff2_down_w, lname("enc.blocks.%d.ff2.down.weight", i), enc_ffn, enc_h);
        GET_F32(b.ff2_down_b, lname("enc.blocks.%d.ff2.down.bias", i), enc_h);

        // Post-block LN.
        GET_F32(b.norm_post_w, lname("enc.blocks.%d.norm_post.weight", i), enc_h);
        GET_F32(b.norm_post_b, lname("enc.blocks.%d.norm_post.bias", i), enc_h);
    }

    // Projector: top-level.
    //
    // proj.query has shape (1, num_queries, hidden) in PyTorch, which
    // is ne[hidden, num_queries, 1] in ggml. num_queries is not in KV;
    // we accept whatever the file declared and stash it for the graph
    // builder via the tensor's ne[1]. The converter ships 3 queries on
    // every public variant. Use find_tensor with a wildcard num_queries
    // by not specifying it — but find_tensor requires every dim, so we
    // hardcode 3 here and add an explicit check below if a future
    // variant changes the count. The query tensor is a learned linear
    // embedding; we accept LINEAR_TYPES dtypes.
    GET_LIN(weights.proj_top.query, "proj.query", prj_h, 3, 1);
    GET_LIN(weights.proj_top.linear_w, "proj.linear.weight", prj_h, dec_h);
    GET_F32(weights.proj_top.linear_b, "proj.linear.bias", dec_h);
    GET_F32(weights.proj_top.qformer_final_norm_w, "proj.qformer.final_norm.weight", prj_h);
    GET_F32(weights.proj_top.qformer_final_norm_b, "proj.qformer.final_norm.bias", prj_h);

    // Projector: per-Q-Former-layer.
    weights.proj_blocks.assign(prj_n_layers, GraniteProjBlock{});
    for (int i = 0; i < prj_n_layers; ++i) {
        auto & b = weights.proj_blocks[i];

        // Self-attention: Q/K/V/Out all map prj_h -> prj_h, every linear
        // has bias.
        GET_LIN(b.self_attn_q_w, lname("proj.qformer.blocks.%d.self_attn.q.weight", i), prj_h, prj_h);
        GET_F32(b.self_attn_q_b, lname("proj.qformer.blocks.%d.self_attn.q.bias", i), prj_h);
        GET_LIN(b.self_attn_k_w, lname("proj.qformer.blocks.%d.self_attn.k.weight", i), prj_h, prj_h);
        GET_F32(b.self_attn_k_b, lname("proj.qformer.blocks.%d.self_attn.k.bias", i), prj_h);
        GET_LIN(b.self_attn_v_w, lname("proj.qformer.blocks.%d.self_attn.v.weight", i), prj_h, prj_h);
        GET_F32(b.self_attn_v_b, lname("proj.qformer.blocks.%d.self_attn.v.bias", i), prj_h);
        GET_LIN(b.self_attn_out_w, lname("proj.qformer.blocks.%d.self_attn.out.weight", i), prj_h, prj_h);
        GET_F32(b.self_attn_out_b, lname("proj.qformer.blocks.%d.self_attn.out.bias", i), prj_h);
        GET_F32(b.norm_self_attn_w, lname("proj.qformer.blocks.%d.norm_self_attn.weight", i), prj_h);
        GET_F32(b.norm_self_attn_b, lname("proj.qformer.blocks.%d.norm_self_attn.bias", i), prj_h);

        // Cross-attention: Q is prj_h->prj_h; K/V are prj_kv_in->prj_h
        // (prj_kv_in == encoder hidden for 1b/2b, doubled for -plus).
        GET_LIN(b.cross_attn_q_w, lname("proj.qformer.blocks.%d.cross_attn.q.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_q_b, lname("proj.qformer.blocks.%d.cross_attn.q.bias", i), prj_h);
        GET_LIN(b.cross_attn_k_w, lname("proj.qformer.blocks.%d.cross_attn.k.weight", i), prj_kv_in, prj_h);
        GET_F32(b.cross_attn_k_b, lname("proj.qformer.blocks.%d.cross_attn.k.bias", i), prj_h);
        GET_LIN(b.cross_attn_v_w, lname("proj.qformer.blocks.%d.cross_attn.v.weight", i), prj_kv_in, prj_h);
        GET_F32(b.cross_attn_v_b, lname("proj.qformer.blocks.%d.cross_attn.v.bias", i), prj_h);
        GET_LIN(b.cross_attn_out_w, lname("proj.qformer.blocks.%d.cross_attn.out.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_out_b, lname("proj.qformer.blocks.%d.cross_attn.out.bias", i), prj_h);
        GET_F32(b.norm_cross_attn_w, lname("proj.qformer.blocks.%d.norm_cross_attn.weight", i), prj_h);
        GET_F32(b.norm_cross_attn_b, lname("proj.qformer.blocks.%d.norm_cross_attn.bias", i), prj_h);

        // FFN.
        GET_LIN(b.ffn_up_w, lname("proj.qformer.blocks.%d.ffn.up.weight", i), prj_h, prj_im);
        GET_F32(b.ffn_up_b, lname("proj.qformer.blocks.%d.ffn.up.bias", i), prj_im);
        GET_LIN(b.ffn_down_w, lname("proj.qformer.blocks.%d.ffn.down.weight", i), prj_im, prj_h);
        GET_F32(b.ffn_down_b, lname("proj.qformer.blocks.%d.ffn.down.bias", i), prj_h);
        GET_F32(b.norm_ffn_w, lname("proj.qformer.blocks.%d.norm_ffn.weight", i), prj_h);
        GET_F32(b.norm_ffn_b, lname("proj.qformer.blocks.%d.norm_ffn.bias", i), prj_h);
    }

    // Text LM: embed.
    GET_LIN(weights.dec_embed.token_w, "dec.token_embd.weight", dec_h, dec_vocab);

    // Text LM: blocks.
    weights.dec_blocks.assign(hp.dec_n_layers, GraniteDecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];
        GET_F32(b.norm_attn_w, lname("dec.blocks.%d.norm_attn.weight", i), dec_h);
        GET_F32(b.norm_ffn_w, lname("dec.blocks.%d.norm_ffn.weight", i), dec_h);
        GET_LIN(b.attn_q_w, lname("dec.blocks.%d.attn.q.weight", i), dec_h, q_out);
        GET_LIN(b.attn_k_w, lname("dec.blocks.%d.attn.k.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_v_w, lname("dec.blocks.%d.attn.v.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_o_w, lname("dec.blocks.%d.attn.o.weight", i), q_out, dec_h);
        GET_LIN(b.ffn_gate_w, lname("dec.blocks.%d.ffn.gate.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_up_w, lname("dec.blocks.%d.ffn.up.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_down_w, lname("dec.blocks.%d.ffn.down.weight", i), dec_im, dec_h);

        if (b.ffn_gate_w->type != b.ffn_up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite: ffn gate/up dtype mismatch at dec layer %d", i);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Text LM: final norm + (conditional) output.
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);
    if (hp.dec_tie_word_embeddings) {
        // Tied head (granite-speech-4.1-2b-plus): no dec.output.weight
        // in the GGUF. The graph reuses dec_embed.token_w for the head.
        weights.dec_final.output_w = nullptr;
    } else {
        GET_LIN(weights.dec_final.output_w, "dec.output.weight", dec_h, dec_vocab);
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::granite
