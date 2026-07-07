// arch/voxtral_realtime/weights.cpp - read_hparams + build_weights.
//
// Every required hparam is read explicitly; a missing tensor or shape
// mismatch is fatal. The encoder is a causal RoPE sliding-window transformer
// (RMSNorm, NEOX RoPE), distinct from the 2507 Whisper encoder. The decoder
// is a Ministral LM with TIED lm_head (reuse dec.token_embd for logits) and a
// per-layer adaptive-norm (ada.linear_{1,2}).

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::voxtral_realtime {

namespace {
constexpr const char * kFamilyTag = "voxtral_realtime";
}

transcribe_status read_hparams(const gguf_context * gguf, HParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    constexpr const char * T = kFamilyTag;

    // Audio encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.n_layers", T, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.d_model", T, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.n_heads", T, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.n_kv_heads", T, hp.enc_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.head_dim", T, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.ffn_dim", T, hp.enc_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.num_mel_bins", T, hp.enc_num_mel_bins);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.max_position_embeddings", T, hp.enc_max_pos);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.encoder.sliding_window", T, hp.enc_sliding_window);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral_realtime.encoder.rope_theta", T, hp.enc_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral_realtime.encoder.rms_norm_eps", T, hp.enc_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral_realtime.encoder.hidden_act", T, hp.enc_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Projector.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.projector.downsample_factor", T, hp.proj_downsample);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.projector.input_dim", T, hp.proj_in);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral_realtime.projector.hidden_act", T, hp.proj_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.projector.audio_length_per_tok", T,
                                       hp.audio_length_per_tok);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Text LM.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.n_layers", T, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.hidden_size", T, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.intermediate_size", T, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.n_heads", T, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.n_kv_heads", T, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.head_dim", T, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.voxtral_realtime.decoder.hidden_act", T, hp.dec_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral_realtime.decoder.rms_norm_eps", T, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral_realtime.decoder.rope_theta", T, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.sliding_window", T, hp.dec_sliding_window);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.max_position_embeddings", T, hp.dec_max_position);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.voxtral_realtime.decoder.tie_word_embeddings", T, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.decoder.vocab_size", T, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Time conditioning.
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.time.default_num_delay_tokens", T,
                                       hp.default_num_delay_tokens);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.voxtral_realtime.time.embed_theta", T, hp.time_embed_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.time.embed_dim", T, hp.time_embed_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.voxtral_realtime.time.ada_hidden", T, hp.ada_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }

    {
        uint32_t spt = 32;
        switch (read_uint32_kv(gguf, "stt.voxtral_realtime.streaming_pad_token_id", spt)) {
            case KvResult::Ok:
                hp.streaming_pad_token_id = static_cast<int32_t>(spt);
                break;
            case KvResult::Absent:
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: streaming_pad_token_id wrong type");
                return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Frontend.
    if (auto st = read_required_string_kv(gguf, "stt.frontend.type", T, hp.fe_type); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.num_mels", T, hp.fe_num_mels); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.sample_rate", T, hp.fe_sample_rate); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.n_fft", T, hp.fe_n_fft); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.win_length", T, hp.fe_win_length); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.hop_length", T, hp.fe_hop_length); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.window", T, hp.fe_window); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.normalize", T, hp.fe_normalize); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.global_log_mel_max", T, hp.fe_global_log_mel_max);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.dither", T, hp.fe_dither); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.pre_emphasis", T, hp.fe_pre_emphasis); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_min", T, hp.fe_f_min); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_max", T, hp.fe_f_max); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.pad_mode", T, "reflect", hp.fe_pad_mode);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.mel_norm", T, "slaney", hp.fe_mel_norm);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.frontend.center", T, true, hp.fe_center); st != TRANSCRIBE_OK) {
        return st;
    }

    // Cross-field invariants.
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_head_dim <= 0 ||
        hp.enc_ffn_dim <= 0 || hp.enc_num_mel_bins <= 0 || hp.enc_sliding_window <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_n_heads != hp.enc_n_kv_heads) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: encoder expects full MHA (n_heads==n_kv_heads)");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.proj_in != hp.enc_d_model * hp.proj_downsample) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: projector input_dim (%d) != enc_d_model*downsample (%d)",
                hp.proj_in, hp.enc_d_model * hp.proj_downsample);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_hidden <= 0 || hp.dec_n_heads <= 0 || hp.dec_n_kv_heads <= 0 ||
        hp.dec_head_dim <= 0 || hp.dec_intermediate <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: n_heads (%d) not divisible by n_kv_heads (%d)",
                hp.dec_n_heads, hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (!hp.dec_tie_word_embeddings) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: decoder expects tied lm_head");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_hidden_act != "silu" && hp.dec_hidden_act != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: unsupported decoder hidden_act \"%s\"",
                hp.dec_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_hidden_act != "silu" && hp.enc_hidden_act != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: unsupported encoder hidden_act \"%s\"",
                hp.enc_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.proj_hidden_act != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: unsupported projector hidden_act \"%s\"",
                hp.proj_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.time_embed_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: time_embed_dim (%d) != dec_hidden (%d)",
                hp.time_embed_dim, hp.dec_hidden);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: unsupported frontend type \"%s\"", hp.fe_type.c_str());
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

transcribe_status build_weights(ggml_context * ctx_meta, const HParams & hp, Weights & weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_model = hp.enc_d_model;
    const int64_t n_mel   = hp.enc_num_mel_bins;
    const int64_t enc_ff  = hp.enc_ffn_dim;
    const int64_t enc_qkv = static_cast<int64_t>(hp.enc_n_heads) * hp.enc_head_dim;

    // ----- encoder conv stem (causal) -----
    GET_CONV(weights.enc_stem.conv0_w, "enc.conv.0.weight", 3, n_mel, d_model);
    GET_F32(weights.enc_stem.conv0_b, "enc.conv.0.bias", d_model);
    GET_CONV(weights.enc_stem.conv1_w, "enc.conv.1.weight", 3, d_model, d_model);
    GET_F32(weights.enc_stem.conv1_b, "enc.conv.1.bias", d_model);
    GET_F32(weights.enc_stem.final_norm_w, "enc.final_norm.weight", d_model);

    // ----- encoder blocks (q/v/out bias, k no bias; SwiGLU, down bias) -----
    weights.enc_blocks.assign(hp.enc_n_layers, EncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];
        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_model);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), d_model, enc_qkv);
        GET_F32(b.attn_q_b, lname("enc.blocks.%d.attn.q.bias", i), enc_qkv);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.k.weight", i), d_model, enc_qkv);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.v.weight", i), d_model, enc_qkv);
        GET_F32(b.attn_v_b, lname("enc.blocks.%d.attn.v.bias", i), enc_qkv);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), enc_qkv, d_model);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.out.bias", i), d_model);
        GET_F32(b.norm_ffn_w, lname("enc.blocks.%d.norm_ffn.weight", i), d_model);
        GET_LIN(b.ffn_gate_w, lname("enc.blocks.%d.ffn.gate.weight", i), d_model, enc_ff);
        GET_LIN(b.ffn_up_w, lname("enc.blocks.%d.ffn.up.weight", i), d_model, enc_ff);
        GET_LIN(b.ffn_down_w, lname("enc.blocks.%d.ffn.down.weight", i), enc_ff, d_model);
        GET_F32(b.ffn_down_b, lname("enc.blocks.%d.ffn.down.bias", i), d_model);
        if (b.ffn_gate_w->type != b.ffn_up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: enc ffn gate/up dtype mismatch at layer %d", i);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- projector (no biases) -----
    const int64_t proj_in = hp.proj_in;
    const int64_t dec_h   = hp.dec_hidden;
    GET_LIN(weights.proj.linear_1_w, "proj.linear_1.weight", proj_in, dec_h);
    GET_LIN(weights.proj.linear_2_w, "proj.linear_2.weight", dec_h, dec_h);

    // ----- text LM: tied embedding -----
    GET_LIN(weights.dec_embed.token_w, "dec.token_embd.weight", dec_h, hp.dec_vocab_size);

    // ----- text LM: blocks (no attn biases; ada-norm linears) -----
    const int64_t dec_nh  = hp.dec_n_heads;
    const int64_t dec_nkv = hp.dec_n_kv_heads;
    const int64_t dec_hd  = hp.dec_head_dim;
    const int64_t dec_im  = hp.dec_intermediate;
    const int64_t q_out   = dec_nh * dec_hd;
    const int64_t kv_out  = dec_nkv * dec_hd;
    const int64_t ada_h   = hp.ada_hidden;

    weights.dec_blocks.assign(hp.dec_n_layers, DecBlock{});
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
        GET_LIN(b.ada_linear_1_w, lname("dec.blocks.%d.ada.linear_1.weight", i), dec_h, ada_h);
        GET_LIN(b.ada_linear_2_w, lname("dec.blocks.%d.ada.linear_2.weight", i), ada_h, dec_h);
        if (b.ffn_gate_w->type != b.ffn_up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: dec ffn gate/up dtype mismatch at layer %d", i);
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- text LM: final norm -----
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);

    // ----- baked time-embedding inv_freq -----
    GET_F32(weights.time_inv_freq, "dec.time_embed.inv_freq", hp.time_embed_dim / 2);

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::voxtral_realtime
