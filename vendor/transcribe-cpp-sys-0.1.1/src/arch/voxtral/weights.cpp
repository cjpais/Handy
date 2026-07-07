// arch/voxtral/weights.cpp - read_voxtral_hparams + build_voxtral_weights.
//
// Pattern mirrors arch/whisper/weights.cpp (encoder) and
// arch/qwen3_asr/weights.cpp (decoder). Every required hparam is read
// explicitly; a missing tensor or shape mismatch is fatal.
//
// Unlike qwen3_asr, the lm_head is UNTIED: dec.output.weight is a
// separate tensor (not dec.token_embd.weight). The decoder block carries
// no per-head Q/K norm (Llama, not Qwen3).

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::voxtral {

namespace {
constexpr const char * kFamilyTag = "voxtral";
}

transcribe_status read_voxtral_hparams(const gguf_context * gguf, VoxtralHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Audio encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.head_dim", kFamilyTag, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.ffn_dim", kFamilyTag, hp.enc_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.num_mel_bins", kFamilyTag, hp.enc_num_mel_bins);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.encoder.max_source_positions", kFamilyTag,
                                       hp.enc_max_source_positions);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral.encoder.activation", kFamilyTag, hp.enc_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Projector.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.projector.downsample_factor", kFamilyTag, hp.proj_downsample);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.projector.input_dim", kFamilyTag, hp.proj_in);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral.projector.hidden_act", kFamilyTag, hp.proj_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Text LM.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.hidden_size", kFamilyTag, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.intermediate_size", kFamilyTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.n_kv_heads", kFamilyTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral.decoder.hidden_act", kFamilyTag, hp.dec_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral.decoder.rms_norm_eps", kFamilyTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral.decoder.rope_theta", kFamilyTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.voxtral.decoder.tie_word_embeddings", kFamilyTag, false,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Audio-token injection id.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral.audio_token_id", kFamilyTag, hp.audio_token_id);
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
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.mel_norm", kFamilyTag, "slaney", hp.fe_mel_norm);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.center", kFamilyTag, true, hp.fe_center);
        st != TRANSCRIBE_OK) {
        return st;
    }
    {
        const auto read_optional = [&](const char * key, int32_t & dst) -> transcribe_status {
            uint32_t tmp = 0;
            switch (read_uint32_kv(gguf, key, tmp)) {
                case KvResult::Ok:
                    dst = static_cast<int32_t>(tmp);
                    return TRANSCRIBE_OK;
                case KvResult::Absent:
                    return TRANSCRIBE_OK;
                case KvResult::BadType:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: \"%s\" has wrong type", key);
                    return TRANSCRIBE_ERR_GGUF;
            }
            return TRANSCRIBE_ERR_GGUF;
        };
        if (auto st = read_optional("stt.frontend.chunk_length", hp.fe_chunk_length); st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_optional("stt.frontend.n_samples", hp.fe_n_samples); st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_optional("stt.frontend.nb_max_frames", hp.fe_nb_max_frames); st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // Cross-field invariants.
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_ffn_dim <= 0 ||
        hp.enc_num_mel_bins <= 0 || hp.enc_max_source_positions <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_head_dim != hp.enc_d_model / hp.enc_n_heads) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: encoder head_dim (%d) != d_model/n_heads (%d)", hp.enc_head_dim,
                hp.enc_d_model / hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.proj_downsample <= 0 || hp.proj_in <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: projector hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.proj_in != hp.enc_d_model * hp.proj_downsample) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: projector input_dim (%d) != enc_d_model*downsample (%d)",
                hp.proj_in, hp.enc_d_model * hp.proj_downsample);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_max_source_positions % hp.proj_downsample != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: max_source_positions (%d) not divisible by downsample (%d)",
                hp.enc_max_source_positions, hp.proj_downsample);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_hidden <= 0 || hp.dec_n_heads <= 0 || hp.dec_n_kv_heads <= 0 ||
        hp.dec_head_dim <= 0 || hp.dec_intermediate <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: n_heads (%d) not divisible by n_kv_heads (%d)", hp.dec_n_heads,
                hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_hidden_act != "silu" && hp.dec_hidden_act != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: unsupported decoder hidden_act \"%s\" (only silu/swish)",
                hp.dec_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_activation != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: unsupported encoder activation \"%s\" (only gelu)",
                hp.enc_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.proj_hidden_act != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: unsupported projector hidden_act \"%s\" (only gelu)",
                hp.proj_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: unsupported frontend type \"%s\"", hp.fe_type.c_str());
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

transcribe_status build_voxtral_weights(ggml_context * ctx_meta, const VoxtralHParams & hp, VoxtralWeights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_model = hp.enc_d_model;
    const int64_t n_mel   = hp.enc_num_mel_bins;
    const int64_t enc_ff  = hp.enc_ffn_dim;
    const int64_t src_pos = hp.enc_max_source_positions;

    // ----- encoder conv stem -----
    // Conv1d kernels: PyTorch [out, in, K] -> ggml ne=[K, in, out]. F16.
    GET_CONV(weights.enc_stem.conv0_w, "enc.conv.0.weight", 3, n_mel, d_model);
    GET_F32(weights.enc_stem.conv0_b, "enc.conv.0.bias", d_model);
    GET_CONV(weights.enc_stem.conv1_w, "enc.conv.1.weight", 3, d_model, d_model);
    GET_F32(weights.enc_stem.conv1_b, "enc.conv.1.bias", d_model);

    // ----- encoder top (sinusoidal pos emb + final LN) -----
    GET_F32(weights.enc_top.pos_emb_w, "enc.pos_emb.weight", d_model, src_pos);
    GET_F32(weights.enc_top.ln_post_w, "enc.ln_post.weight", d_model);
    GET_F32(weights.enc_top.ln_post_b, "enc.ln_post.bias", d_model);

    // ----- encoder blocks (q/v/out bias, k no bias) -----
    weights.enc_blocks.assign(hp.enc_n_layers, VoxtralEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];
        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_model);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), d_model);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), d_model, d_model);
        GET_F32(b.attn_q_b, lname("enc.blocks.%d.attn.q.bias", i), d_model);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.k.weight", i), d_model, d_model);
        // attn.k has NO bias.
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.v.weight", i), d_model, d_model);
        GET_F32(b.attn_v_b, lname("enc.blocks.%d.attn.v.bias", i), d_model);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), d_model, d_model);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.out.bias", i), d_model);
        GET_F32(b.norm_ffn_w, lname("enc.blocks.%d.norm_ffn.weight", i), d_model);
        GET_F32(b.norm_ffn_b, lname("enc.blocks.%d.norm_ffn.bias", i), d_model);
        GET_LIN(b.fc1_w, lname("enc.blocks.%d.ffn.fc1.weight", i), d_model, enc_ff);
        GET_F32(b.fc1_b, lname("enc.blocks.%d.ffn.fc1.bias", i), enc_ff);
        GET_LIN(b.fc2_w, lname("enc.blocks.%d.ffn.fc2.weight", i), enc_ff, d_model);
        GET_F32(b.fc2_b, lname("enc.blocks.%d.ffn.fc2.bias", i), d_model);
    }

    // ----- projector (no biases) -----
    const int64_t proj_in = hp.proj_in;
    const int64_t dec_h   = hp.dec_hidden;
    GET_LIN(weights.proj.linear_1_w, "proj.linear_1.weight", proj_in, dec_h);
    GET_LIN(weights.proj.linear_2_w, "proj.linear_2.weight", dec_h, dec_h);

    // ----- text LM: embedding + UNTIED output -----
    GET_LIN(weights.dec_embed.token_w, "dec.token_embd.weight", dec_h, hp.dec_vocab_size);
    GET_LIN(weights.dec_embed.output_w, "dec.output.weight", dec_h, hp.dec_vocab_size);

    // ----- text LM: blocks (Llama; no biases, no Q/K norm) -----
    const int64_t dec_nh  = hp.dec_n_heads;
    const int64_t dec_nkv = hp.dec_n_kv_heads;
    const int64_t dec_hd  = hp.dec_head_dim;
    const int64_t dec_im  = hp.dec_intermediate;
    const int64_t q_out   = dec_nh * dec_hd;
    const int64_t kv_out  = dec_nkv * dec_hd;

    weights.dec_blocks.assign(hp.dec_n_layers, VoxtralDecBlock{});
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
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: ffn gate/up dtype mismatch at layer %d (%d vs %d)", i,
                    static_cast<int>(b.ffn_gate_w->type), static_cast<int>(b.ffn_up_w->type));
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- text LM: final norm -----
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::voxtral
