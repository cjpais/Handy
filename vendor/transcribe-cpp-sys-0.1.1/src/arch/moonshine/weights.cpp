// arch/moonshine/weights.cpp - read_moonshine_hparams + build_moonshine_weights.
//
// Pattern mirrors arch/whisper/weights.cpp: read every required KV
// explicitly, validate cross-field invariants, then resolve each tensor
// slot against the GGUF with expected shape.
//
// Moonshine-specific notes:
//
//   - All attention projections (q/k/v/o) are bias-less on both encoder
//     and decoder; no `_b` slots in the catalog.
//
//   - All LayerNorms are bias-less (`nn.LayerNorm(..., bias=False)`).
//     Only conv groupnorm and MLP fc1/fc2 carry bias.
//
//   - Decoder MLP is SwiGLU: fc1 emits 2·intermediate (split into
//     [x_proj, gate]), fc2 takes intermediate. Encoder MLP is plain GELU.
//
//   - Logits head: tied to dec.token_embd.weight (the converter does not
//     emit a separate lm_head tensor).

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <cstring>
#include <initializer_list>
#include <vector>

namespace transcribe::moonshine {

namespace {

constexpr const char * kFamilyTag = "moonshine";

transcribe_status read_optional_u32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       int32_t              default_value,
                                       int32_t &            out) {
    uint32_t   value = 0;
    const auto st    = transcribe::read_uint32_kv(gguf, key, value);
    if (st == transcribe::KvResult::Ok) {
        out = static_cast<int32_t>(value);
        return TRANSCRIBE_OK;
    }
    if (st == transcribe::KvResult::Absent) {
        out = default_value;
        return TRANSCRIBE_OK;
    }
    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: KV %s has wrong type", error_tag, key);
    return TRANSCRIBE_ERR_GGUF;
}

transcribe_status read_optional_f32_kv(const gguf_context * gguf,
                                       const char *         key,
                                       const char *         error_tag,
                                       float                default_value,
                                       float &              out) {
    float      value = default_value;
    const auto st    = transcribe::read_float32_kv(gguf, key, value);
    if (st == transcribe::KvResult::Ok) {
        out = value;
        return TRANSCRIBE_OK;
    }
    if (st == transcribe::KvResult::Absent) {
        out = default_value;
        return TRANSCRIBE_OK;
    }
    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: KV %s has wrong type", error_tag, key);
    return TRANSCRIBE_ERR_GGUF;
}

// uint32 array → vector<int32_t>. Used for the conv-stem shape lists.
transcribe_status read_required_u32_array_kv(const gguf_context *   gguf,
                                             const char *           key,
                                             const char *           error_tag,
                                             std::vector<int32_t> & out) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int64_t kid = gguf_find_key(gguf, key);
    if (kid < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: missing required KV %s", error_tag, key);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (gguf_get_kv_type(gguf, kid) != GGUF_TYPE_ARRAY) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: KV %s is not an array", error_tag, key);
        return TRANSCRIBE_ERR_GGUF;
    }
    const gguf_type elem_type = gguf_get_arr_type(gguf, kid);
    if (elem_type != GGUF_TYPE_UINT32 && elem_type != GGUF_TYPE_INT32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: KV %s array element type is not u32/i32 (got %d)", error_tag, key,
                static_cast<int>(elem_type));
        return TRANSCRIBE_ERR_GGUF;
    }
    const size_t n    = gguf_get_arr_n(gguf, kid);
    const void * data = gguf_get_arr_data(gguf, kid);
    out.resize(n);
    if (elem_type == GGUF_TYPE_UINT32) {
        const uint32_t * src = static_cast<const uint32_t *>(data);
        for (size_t i = 0; i < n; ++i) {
            out[i] = static_cast<int32_t>(src[i]);
        }
    } else {
        const int32_t * src = static_cast<const int32_t *>(data);
        for (size_t i = 0; i < n; ++i) {
            out[i] = src[i];
        }
    }
    return TRANSCRIBE_OK;
}

}  // namespace

// ---------------------------------------------------------------------------
// Hparams
// ---------------------------------------------------------------------------

transcribe_status read_moonshine_hparams(const gguf_context * gguf, MoonshineHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine.encoder.n_kv_heads", kFamilyTag, hp.enc_n_heads,
                                       hp.enc_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.encoder.ffn_dim", kFamilyTag, hp.enc_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.moonshine.encoder.activation", kFamilyTag, hp.enc_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Decoder.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.d_model", kFamilyTag, hp.dec_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine.decoder.n_kv_heads", kFamilyTag, hp.dec_n_heads,
                                       hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.ffn_dim", kFamilyTag, hp.dec_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.moonshine.decoder.activation", kFamilyTag, hp.dec_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.moonshine.decoder.tie_word_embeddings", kFamilyTag, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Special tokens.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.bos_token_id", kFamilyTag, hp.bos_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.eos_token_id", kFamilyTag, hp.eos_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine.pad_token_id", kFamilyTag, hp.pad_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.moonshine.decoder_start_token_id", kFamilyTag, hp.decoder_start_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Attention / RoPE.
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine.partial_rotary_factor", kFamilyTag, 0.9f,
                                       hp.partial_rotary_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine.rope_theta", kFamilyTag, 10000.0f, hp.rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.moonshine.attention_bias", kFamilyTag, false, hp.attention_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine.pad_head_dim_to_multiple_of", kFamilyTag, 0,
                                       hp.pad_head_dim_multiple);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Conv stem.
    if (auto st = read_required_u32_array_kv(gguf, "stt.moonshine.conv_stem.channels", kFamilyTag, hp.conv_channels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_array_kv(gguf, "stt.moonshine.conv_stem.kernel_sizes", kFamilyTag, hp.conv_kernel_sizes);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_array_kv(gguf, "stt.moonshine.conv_stem.strides", kFamilyTag, hp.conv_strides);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine.conv_stem.groupnorm_num_groups", kFamilyTag, 1,
                                       hp.conv_groupnorm_num_groups);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine.conv_stem.groupnorm_eps", kFamilyTag, 1e-5f,
                                       hp.conv_groupnorm_eps);
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

    // Capability flags. Re-read here for in-decoder convenience; the
    // shared read_capability_kv() (called later in the load path) also
    // updates the public transcribe_capabilities struct.
    if (auto st = read_optional_bool_kv(gguf, "stt.capability.lang_detect", kFamilyTag, false, hp.cap_lang_detect);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.capability.translate", kFamilyTag, false, hp.cap_translate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.capability.timestamps", kFamilyTag, false, hp.cap_timestamps);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // ----- Cross-field invariants -----
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_ffn_dim <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: enc d_model (%d) not divisible by n_heads (%d)", hp.enc_d_model,
                hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_d_model <= 0 || hp.dec_n_heads <= 0 || hp.dec_ffn_dim <= 0 ||
        hp.dec_max_position_embeddings <= 0 || hp.dec_vocab_size <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_d_model % hp.dec_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: dec d_model (%d) not divisible by n_heads (%d)", hp.dec_d_model,
                hp.dec_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model != hp.dec_d_model) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine: encoder d_model (%d) != decoder d_model (%d); "
                "cross-attn would mismatch",
                hp.enc_d_model, hp.dec_d_model);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_activation != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: only \"gelu\" encoder activation is supported (got \"%s\")",
                hp.enc_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_activation != "silu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: only \"silu\" decoder activation is supported (got \"%s\")",
                hp.dec_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (!hp.dec_tie_word_embeddings) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine: stt.moonshine.decoder.tie_word_embeddings=false "
                "is not supported (no separate lm_head tensor)");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.attention_bias) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine: stt.moonshine.attention_bias=true is not supported "
                "(catalog has no attn bias slots)");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.partial_rotary_factor <= 0.0f || hp.partial_rotary_factor > 1.0f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: invalid partial_rotary_factor=%g (expected (0, 1])",
                hp.partial_rotary_factor);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "raw") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: unsupported frontend type \"%s\" (only \"raw\")",
                hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_sample_rate != 16000) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: unsupported sample_rate=%d (only 16000 Hz)", hp.fe_sample_rate);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.conv_channels.size() != 3 || hp.conv_kernel_sizes.size() != 3 || hp.conv_strides.size() != 3) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine: conv stem expected 3 entries each "
                "(channels=%zu, kernel_sizes=%zu, strides=%zu)",
                hp.conv_channels.size(), hp.conv_kernel_sizes.size(), hp.conv_strides.size());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.conv_channels[0] != hp.enc_d_model || hp.conv_channels[2] != hp.enc_d_model) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: conv stem channels [%d, %d, %d] disagree with enc_d_model=%d",
                hp.conv_channels[0], hp.conv_channels[1], hp.conv_channels[2], hp.enc_d_model);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.conv_groupnorm_num_groups != 1) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: only num_groups=1 GroupNorm is supported (got %d)",
                hp.conv_groupnorm_num_groups);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.bos_token_id < 0 || hp.eos_token_id < 0 || hp.pad_token_id < 0 || hp.decoder_start_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: special token IDs must be set");
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

transcribe_status build_moonshine_weights(ggml_context *           ctx_meta,
                                          const MoonshineHParams & hp,
                                          MoonshineWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_model    = hp.enc_d_model;
    const int64_t enc_ff     = hp.enc_ffn_dim;
    const int64_t dec_h      = hp.dec_d_model;
    const int64_t dec_ff     = hp.dec_ffn_dim;
    const int64_t vocab_size = hp.dec_vocab_size;
    const int64_t k0         = hp.conv_kernel_sizes[0];
    const int64_t k1         = hp.conv_kernel_sizes[1];
    const int64_t k2         = hp.conv_kernel_sizes[2];
    const int64_t c0         = hp.conv_channels[0];  // enc_d_model
    const int64_t c1         = hp.conv_channels[1];  // 2·enc_d_model on tiny
    const int64_t c2         = hp.conv_channels[2];  // enc_d_model

    // ----- encoder conv stem -----
    // PyTorch Conv1d weight is [out, in, K]; ggml ne is [K, in, out].
    // conv0 takes 1 audio channel and emits enc_d_model. No bias (HF
    // Conv1d(bias=False)).
    GET_CONV(weights.enc_stem.conv0_w, "enc.conv.0.weight", k0, /*in=*/1, /*out=*/c0);
    GET_CONV(weights.enc_stem.conv1_w, "enc.conv.1.weight", k1, c0, c1);
    GET_F32(weights.enc_stem.conv1_b, "enc.conv.1.bias", c1);
    GET_CONV(weights.enc_stem.conv2_w, "enc.conv.2.weight", k2, c1, c2);
    GET_F32(weights.enc_stem.conv2_b, "enc.conv.2.bias", c2);
    GET_F32(weights.enc_stem.gn_w, "enc.conv.norm.weight", d_model);
    GET_F32(weights.enc_stem.gn_b, "enc.conv.norm.bias", d_model);

    // ----- encoder top -----
    GET_F32(weights.enc_top.final_norm_w, "enc.final_norm.weight", d_model);

    // ----- encoder blocks -----
    weights.enc_blocks.assign(hp.enc_n_layers, MoonshineEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];

        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_model);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), d_model, d_model);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.k.weight", i), d_model, d_model);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.v.weight", i), d_model, d_model);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), d_model, d_model);

        GET_F32(b.norm_ffn_w, lname("enc.blocks.%d.norm_ffn.weight", i), d_model);
        GET_LIN(b.ffn_fc1_w, lname("enc.blocks.%d.ffn.fc1.weight", i), d_model, enc_ff);
        GET_F32(b.ffn_fc1_b, lname("enc.blocks.%d.ffn.fc1.bias", i), enc_ff);
        GET_LIN(b.ffn_fc2_w, lname("enc.blocks.%d.ffn.fc2.weight", i), enc_ff, d_model);
        GET_F32(b.ffn_fc2_b, lname("enc.blocks.%d.ffn.fc2.bias", i), d_model);
    }

    // ----- decoder top (tied embed + final LN) -----
    GET_LIN(weights.dec_top.token_embd_w, "dec.token_embd.weight", dec_h, vocab_size);
    GET_F32(weights.dec_top.final_norm_w, "dec.final_norm.weight", dec_h);

    // ----- decoder blocks -----
    weights.dec_blocks.assign(hp.dec_n_layers, MoonshineDecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];

        // Self-attn (bias-less).
        GET_F32(b.norm_self_w, lname("dec.blocks.%d.norm_self.weight", i), dec_h);
        GET_LIN(b.self_q_w, lname("dec.blocks.%d.self_attn.q.weight", i), dec_h, dec_h);
        GET_LIN(b.self_k_w, lname("dec.blocks.%d.self_attn.k.weight", i), dec_h, dec_h);
        GET_LIN(b.self_v_w, lname("dec.blocks.%d.self_attn.v.weight", i), dec_h, dec_h);
        GET_LIN(b.self_out_w, lname("dec.blocks.%d.self_attn.out.weight", i), dec_h, dec_h);

        // Cross-attn (bias-less).
        GET_F32(b.norm_cross_w, lname("dec.blocks.%d.norm_cross.weight", i), dec_h);
        GET_LIN(b.cross_q_w, lname("dec.blocks.%d.cross_attn.q.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_k_w, lname("dec.blocks.%d.cross_attn.k.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_v_w, lname("dec.blocks.%d.cross_attn.v.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_out_w, lname("dec.blocks.%d.cross_attn.out.weight", i), dec_h, dec_h);

        // SwiGLU MLP. fc1 packs [x_proj, gate]: hidden → 2·intermediate.
        GET_F32(b.norm_ffn_w, lname("dec.blocks.%d.norm_ffn.weight", i), dec_h);
        GET_LIN(b.ffn_fc1_w, lname("dec.blocks.%d.ffn.fc1.weight", i), dec_h, 2 * dec_ff);
        GET_F32(b.ffn_fc1_b, lname("dec.blocks.%d.ffn.fc1.bias", i), 2 * dec_ff);
        GET_LIN(b.ffn_fc2_w, lname("dec.blocks.%d.ffn.fc2.weight", i), dec_ff, dec_h);
        GET_F32(b.ffn_fc2_b, lname("dec.blocks.%d.ffn.fc2.bias", i), dec_h);
    }

    return TRANSCRIBE_OK;
}

#undef GET_F32
#undef GET_CONV
#undef GET_LIN

}  // namespace transcribe::moonshine
