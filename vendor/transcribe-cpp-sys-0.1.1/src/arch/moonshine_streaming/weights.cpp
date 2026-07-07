// arch/moonshine_streaming/weights.cpp - read_moonshine_streaming_hparams
// + build_moonshine_streaming_weights.
//
// Mirrors arch/moonshine/weights.cpp: read every required KV explicitly,
// validate cross-field invariants, then resolve each tensor slot.
//
// Streaming-specific notes:
//
//   - Encoder LayerNorm scales are PRE-FOLDED by the converter: the GGUF
//     tensor at `enc.*.norm_*.weight` contains (γ + 1.0). C++ uses the
//     ordinary "ggml_norm * scale" pattern.
//
//   - Untied lm_head: dec.lm_head.weight is a SEPARATE tensor (not tied
//     to dec.token_embd.weight).
//
//   - Adapter: adapter.pos_emb.weight always present; adapter.proj.weight
//     present iff stt.moonshine_streaming.adapter_has_proj=true.
//
//   - Frontend tensors live under enc.embedder.* (CMVN is parameter-free
//     so no tensor; comp.log_k is a [1] f32 scalar).

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

namespace transcribe::moonshine_streaming {

namespace {

constexpr const char * kFamilyTag = "moonshine_streaming";

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

transcribe_status read_moonshine_streaming_hparams(const gguf_context * gguf, MoonshineStreamingHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    using transcribe::read_optional_bool_kv;
    using transcribe::read_required_string_kv;
    using transcribe::read_required_u32_kv;

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine_streaming.encoder.n_kv_heads", kFamilyTag, hp.enc_n_heads,
                                       hp.enc_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.head_dim", kFamilyTag, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.ffn_dim", kFamilyTag, hp.enc_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_string_kv(gguf, "stt.moonshine_streaming.encoder.activation", kFamilyTag, hp.enc_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }
    {
        float fr = 0.0f;
        if (auto st =
                transcribe::read_required_f32_kv(gguf, "stt.moonshine_streaming.encoder.frame_ms", kFamilyTag, fr);
            st != TRANSCRIBE_OK) {
            return st;
        }
        hp.enc_frame_ms = fr;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.encoder.frame_len", kFamilyTag, hp.enc_frame_len);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_array_kv(gguf, "stt.moonshine_streaming.encoder.sliding_windows", kFamilyTag,
                                             hp.enc_sliding_windows);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Decoder.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.d_model", kFamilyTag, hp.dec_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine_streaming.decoder.n_kv_heads", kFamilyTag, hp.dec_n_heads,
                                       hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.ffn_dim", kFamilyTag, hp.dec_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_string_kv(gguf, "stt.moonshine_streaming.decoder.activation", kFamilyTag, hp.dec_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.moonshine_streaming.decoder.tie_word_embeddings", kFamilyTag, false,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Special tokens.
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.bos_token_id", kFamilyTag, hp.bos_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.eos_token_id", kFamilyTag, hp.eos_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.pad_token_id", kFamilyTag, hp.pad_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.moonshine_streaming.decoder_start_token_id", kFamilyTag,
                                       hp.decoder_start_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Attention / RoPE.
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine_streaming.partial_rotary_factor", kFamilyTag, 0.8f,
                                       hp.partial_rotary_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine_streaming.rope_theta", kFamilyTag, 10000.0f, hp.rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_optional_bool_kv(gguf, "stt.moonshine_streaming.attention_bias", kFamilyTag, false, hp.attention_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine_streaming.pad_head_dim_to_multiple_of", kFamilyTag, 0,
                                       hp.pad_head_dim_multiple);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // CMVN epsilon (informational; the value is fixed in modeling code at 1e-6).
    if (auto st = read_optional_f32_kv(gguf, "stt.moonshine_streaming.cmvn_eps", kFamilyTag, 1e-6f, hp.cmvn_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Adapter.
    if (auto st = read_optional_u32_kv(gguf, "stt.moonshine_streaming.encoder_hidden_size", kFamilyTag, hp.enc_d_model,
                                       hp.encoder_hidden_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.moonshine_streaming.adapter_has_proj", kFamilyTag, false,
                                        hp.adapter_has_proj);
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

    // Capability flags. Re-read here for in-decoder convenience.
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
    if (auto st = read_optional_bool_kv(gguf, "stt.capability.streaming", kFamilyTag, true, hp.cap_streaming);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // ----- Cross-field invariants -----
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_head_dim <= 0 ||
        hp.enc_ffn_dim <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    // Encoder allows enc_d_model > n_heads * head_dim (small: 620 vs 512;
    // medium: 768 vs 640). Q/K/V project residual_dim → attn_dim and O
    // projects attn_dim → residual_dim. We require enc_d_model >=
    // n_heads * head_dim because the attention internal dim cannot exceed
    // the residual stream dim in any in-the-wild moonshine_streaming
    // variant; flag the inverted case to catch a malformed config early.
    if (hp.enc_d_model < hp.enc_n_heads * hp.enc_head_dim) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: encoder d_model (%d) < n_heads (%d) * head_dim (%d)",
                hp.enc_d_model, hp.enc_n_heads, hp.enc_head_dim);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_d_model <= 0 || hp.dec_n_heads <= 0 || hp.dec_head_dim <= 0 ||
        hp.dec_ffn_dim <= 0 || hp.dec_max_position_embeddings <= 0 || hp.dec_vocab_size <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_d_model != hp.dec_n_heads * hp.dec_head_dim) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: decoder d_model (%d) != n_heads (%d) * head_dim (%d)",
                hp.dec_d_model, hp.dec_n_heads, hp.dec_head_dim);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_activation != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming: only \"gelu\" encoder activation supported (got \"%s\")",
                hp.enc_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_activation != "silu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming: only \"silu\" decoder activation supported (got \"%s\")",
                hp.dec_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_tie_word_embeddings) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming: tie_word_embeddings=true is not supported "
                "(reference always carries a separate proj_out.weight)");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.attention_bias) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: attention_bias=true is not supported");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.partial_rotary_factor <= 0.0f || hp.partial_rotary_factor > 1.0f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: invalid partial_rotary_factor=%g (expected (0, 1])",
                hp.partial_rotary_factor);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "raw") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: unsupported frontend type \"%s\" (only \"raw\")",
                hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_sample_rate != 16000) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: unsupported sample_rate=%d (only 16000 Hz)",
                hp.fe_sample_rate);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_frame_len <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: invalid encoder frame_len=%d", hp.enc_frame_len);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_sliding_windows.size() != static_cast<size_t>(hp.enc_n_layers) * 2) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming: encoder.sliding_windows length %zu != 2 * n_layers (%d)",
                hp.enc_sliding_windows.size(), hp.enc_n_layers);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.encoder_hidden_size != hp.enc_d_model) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine_streaming: encoder_hidden_size (%d) disagrees with encoder.d_model (%d)",
                hp.encoder_hidden_size, hp.enc_d_model);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.bos_token_id < 0 || hp.eos_token_id < 0 || hp.pad_token_id < 0 || hp.decoder_start_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine_streaming: special token IDs must be set");
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

transcribe_status build_moonshine_streaming_weights(ggml_context *                    ctx_meta,
                                                    const MoonshineStreamingHParams & hp,
                                                    MoonshineStreamingWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t enc_h     = hp.enc_d_model;
    const int64_t enc_ff    = hp.enc_ffn_dim;
    const int64_t dec_h     = hp.dec_d_model;
    const int64_t dec_ff    = hp.dec_ffn_dim;
    const int64_t vocab     = hp.dec_vocab_size;
    const int64_t frame_len = hp.enc_frame_len;
    const int64_t max_pos   = hp.dec_max_position_embeddings;

    // ----- encoder embedder -----
    // PyTorch Conv1d weight: [out_channels, in_channels, K]; ggml ne is [K, in, out].
    // Linear weight: [out, in] in PyTorch → ggml ne [in, out].
    GET_F32(weights.embedder.comp_log_k, "enc.embedder.comp.log_k", /*ne0=*/1);
    GET_LIN(weights.embedder.linear_w, "enc.embedder.linear.weight", frame_len, enc_h);
    GET_CONV(weights.embedder.conv1_w, "enc.embedder.conv1.weight", /*K=*/5, /*in=*/enc_h, /*out=*/2 * enc_h);
    GET_F32(weights.embedder.conv1_b, "enc.embedder.conv1.bias", /*ne0=*/2 * enc_h);
    GET_CONV(weights.embedder.conv2_w, "enc.embedder.conv2.weight", /*K=*/5, /*in=*/2 * enc_h, /*out=*/enc_h);
    GET_F32(weights.embedder.conv2_b, "enc.embedder.conv2.bias", /*ne0=*/enc_h);

    // ----- encoder top -----
    GET_F32(weights.enc_top.final_norm_w, "enc.final_norm.weight", enc_h);

    // ----- encoder blocks -----
    // Q/K/V project residual_dim → attn_dim (PyTorch [attn, in_h]
    // → ggml ne [in_h, attn]); O projects attn_dim → residual_dim
    // ([in_h, attn_dim] → ggml ne [attn, in_h]). For tiny attn_h ==
    // enc_h; for small/medium attn_h < enc_h.
    const int64_t enc_attn_h = hp.enc_attn_dim();
    weights.enc_blocks.assign(hp.enc_n_layers, MoonshineStreamingEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];

        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), enc_h);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), enc_h, enc_attn_h);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.k.weight", i), enc_h, enc_attn_h);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.v.weight", i), enc_h, enc_attn_h);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), enc_attn_h, enc_h);

        GET_F32(b.norm_ffn_w, lname("enc.blocks.%d.norm_ffn.weight", i), enc_h);
        GET_LIN(b.ffn_fc1_w, lname("enc.blocks.%d.ffn.fc1.weight", i), enc_h, enc_ff);
        GET_F32(b.ffn_fc1_b, lname("enc.blocks.%d.ffn.fc1.bias", i), enc_ff);
        GET_LIN(b.ffn_fc2_w, lname("enc.blocks.%d.ffn.fc2.weight", i), enc_ff, enc_h);
        GET_F32(b.ffn_fc2_b, lname("enc.blocks.%d.ffn.fc2.bias", i), enc_h);
    }

    // ----- adapter -----
    // pos_emb is an embedding table: [hidden, num_embeddings] in ggml ne.
    GET_LIN(weights.adapter.pos_emb_w, "adapter.pos_emb.weight", enc_h, max_pos);
    if (hp.adapter_has_proj) {
        GET_LIN(weights.adapter.proj_w, "adapter.proj.weight", enc_h, dec_h);
    }

    // ----- decoder top -----
    GET_LIN(weights.dec_top.token_embd_w, "dec.token_embd.weight", dec_h, vocab);
    GET_F32(weights.dec_top.final_norm_w, "dec.final_norm.weight", dec_h);
    GET_LIN(weights.dec_top.lm_head_w, "dec.lm_head.weight", dec_h, vocab);

    // ----- decoder blocks -----
    weights.dec_blocks.assign(hp.dec_n_layers, MoonshineStreamingDecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];

        // Self-attn (bias-less).
        GET_F32(b.norm_self_w, lname("dec.blocks.%d.norm_self.weight", i), dec_h);
        GET_LIN(b.self_q_w, lname("dec.blocks.%d.self_attn.q.weight", i), dec_h, dec_h);
        GET_LIN(b.self_k_w, lname("dec.blocks.%d.self_attn.k.weight", i), dec_h, dec_h);
        GET_LIN(b.self_v_w, lname("dec.blocks.%d.self_attn.v.weight", i), dec_h, dec_h);
        GET_LIN(b.self_out_w, lname("dec.blocks.%d.self_attn.out.weight", i), dec_h, dec_h);

        // Cross-attn (bias-less). Decoder hidden_size is used for both
        // sides (cross-attn projects `dec_h → dec_h` per HF — encoder
        // output is already in `dec_h` after the adapter).
        GET_F32(b.norm_cross_w, lname("dec.blocks.%d.norm_cross.weight", i), dec_h);
        GET_LIN(b.cross_q_w, lname("dec.blocks.%d.cross_attn.q.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_k_w, lname("dec.blocks.%d.cross_attn.k.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_v_w, lname("dec.blocks.%d.cross_attn.v.weight", i), dec_h, dec_h);
        GET_LIN(b.cross_out_w, lname("dec.blocks.%d.cross_attn.out.weight", i), dec_h, dec_h);

        // SwiGLU MLP (with biases). fc1 emits 2·ffn_dim.
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

}  // namespace transcribe::moonshine_streaming
