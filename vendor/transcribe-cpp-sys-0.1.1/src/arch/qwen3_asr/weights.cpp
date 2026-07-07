// arch/qwen3_asr/weights.cpp - read_qwen3_asr_hparams + build_qwen3_asr_weights.
//
// Pattern mirrors arch/cohere/weights.cpp:
//   - every required hparam is read explicitly; BadType is always fatal
//   - tensor catalog is a sequence of get_tensor() calls with expected
//     shapes; missing tensor or shape mismatch -> TRANSCRIBE_ERR_GGUF
//
// The lm_head slot intentionally does not exist: tied_word_embeddings is
// always true for Qwen3-ASR and the graph builder will reuse
// dec.token_embd.weight for the output projection (llama.cpp
// TENSOR_DUPLICATED convention).

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::qwen3_asr {

namespace {
constexpr const char * kFamilyTag = "qwen3_asr";
}

transcribe_status read_qwen3_asr_hparams(const gguf_context * gguf, QwenAsrHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Audio encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.ffn_dim", kFamilyTag, hp.enc_ffn_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.num_mel_bins", kFamilyTag, hp.enc_num_mel_bins);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.downsample_hidden", kFamilyTag, hp.enc_downsample_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.output_dim", kFamilyTag, hp.enc_output_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.max_source_positions", kFamilyTag,
                                       hp.enc_max_source_positions);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.n_window", kFamilyTag, hp.enc_n_window);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.n_window_infer", kFamilyTag, hp.enc_n_window_infer);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.encoder.conv_chunksize", kFamilyTag, hp.enc_conv_chunksize);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.qwen3_asr.encoder.activation", kFamilyTag, hp.enc_activation);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Text LM.
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.n_layers", kFamilyTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.hidden_size", kFamilyTag, hp.dec_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.intermediate_size", kFamilyTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.n_heads", kFamilyTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.n_kv_heads", kFamilyTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.head_dim", kFamilyTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.qwen3_asr.decoder.hidden_act", kFamilyTag, hp.dec_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.qwen3_asr.decoder.rms_norm_eps", kFamilyTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.qwen3_asr.decoder.rope_theta", kFamilyTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.rope_mrope_section_t", kFamilyTag,
                                       hp.dec_rope_mrope_section_t);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.rope_mrope_section_h", kFamilyTag,
                                       hp.dec_rope_mrope_section_h);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.rope_mrope_section_w", kFamilyTag,
                                       hp.dec_rope_mrope_section_w);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.qwen3_asr.decoder.rope_mrope_interleaved", kFamilyTag, true,
                                        hp.dec_rope_mrope_interleaved);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.max_position_embeddings", kFamilyTag,
                                       hp.dec_max_position_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.qwen3_asr.decoder.tie_word_embeddings", kFamilyTag, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.decoder.vocab_size", kFamilyTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Audio-token injection ids.
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.audio_token_id", kFamilyTag, hp.audio_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.audio_start_token_id", kFamilyTag, hp.audio_start_token_id);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.qwen3_asr.audio_end_token_id", kFamilyTag, hp.audio_end_token_id);
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
    // Whisper-style windowing: chunk_length / n_samples / nb_max_frames
    // describe the reference preprocessor's fixed 30s chunking. Treated
    // as optional because not every future Qwen3-ASR variant will carry
    // them; the runtime may compute from sample_rate + hop_length. Use
    // the low-level KvResult path so a wrong-type KV (a converter bug)
    // still fails loudly — Absent is fine, BadType is not.
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
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: \"%s\" has wrong type", key);
                    return TRANSCRIBE_ERR_GGUF;
            }
            return TRANSCRIBE_ERR_GGUF;  // unreachable
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
        hp.enc_downsample_hidden <= 0 || hp.enc_output_dim <= 0 || hp.enc_num_mel_bins <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_layers <= 0 || hp.dec_hidden <= 0 || hp.dec_n_heads <= 0 || hp.dec_n_kv_heads <= 0 ||
        hp.dec_head_dim <= 0 || hp.dec_intermediate <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: decoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_n_heads % hp.dec_n_kv_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: n_heads (%d) not divisible by n_kv_heads (%d)", hp.dec_n_heads,
                hp.dec_n_kv_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_hidden_act != "silu" && hp.dec_hidden_act != "swish") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: unsupported decoder hidden_act \"%s\" "
                "(only silu/swish)",
                hp.dec_hidden_act.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_activation != "gelu") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: unsupported encoder activation \"%s\" "
                "(only gelu)",
                hp.enc_activation.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: unsupported frontend type \"%s\"", hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }
    // The graph reuses dec.token_embd.weight as the output projection.
    // A GGUF that declares tie_word_embeddings=false would need an
    // untied lm_head tensor that this port does not ship or consume;
    // fail at load rather than silently run an undefined graph.
    if (!hp.dec_tie_word_embeddings) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: decoder.tie_word_embeddings=false is not "
                "supported in this port (graph assumes tied lm_head)");
        return TRANSCRIBE_ERR_GGUF;
    }
    // Single-modality audio-LLM: the LM at inference processes one
    // position-id stream (temporal), so the 3D MRoPE reduces to plain
    // RoPE. The section sizes must still add up to head_dim/2 because
    // the converter writes them as the canonical MRoPE split. We
    // validate the reduction shape explicitly so a future checkpoint
    // that breaks the single-modality assumption fails at load rather
    // than silently mis-rotating Q/K.
    {
        const int32_t section_sum =
            hp.dec_rope_mrope_section_t + hp.dec_rope_mrope_section_h + hp.dec_rope_mrope_section_w;
        const int32_t half = hp.dec_head_dim / 2;
        if (section_sum != half) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "qwen3_asr: mrope_section t+h+w (%d) != head_dim/2 (%d); "
                    "single-modality RoPE reduction no longer valid",
                    section_sum, half);
            return TRANSCRIBE_ERR_GGUF;
        }
        // The decoder graph assumes the interleaved MRoPE layout (pairs
        // alternate t/h/w within each head), which is how Qwen3-ASR's
        // published checkpoints ship. The graph would mis-rotate Q/K
        // if a future checkpoint used the concatenated layout, so fail
        // at load rather than silently produce wrong logits.
        if (!hp.dec_rope_mrope_interleaved) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "qwen3_asr: dec_rope_mrope_interleaved=false is not "
                    "supported; the decoder graph assumes the interleaved "
                    "MRoPE layout");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    // Audio injection precondition: the encoder's output_dim must
    // equal the LM's hidden_size, since audio rows are scattered
    // directly into the LM input embedding. A mismatch here would
    // surface at graph-build time anyway; catching it at load gives
    // a clearer error and avoids paying encoder cost on every run().
    if (hp.enc_output_dim != hp.dec_hidden) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "qwen3_asr: enc_output_dim (%d) != dec_hidden (%d); "
                "audio injection requires matching dims",
                hp.enc_output_dim, hp.dec_hidden);
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

transcribe_status build_qwen3_asr_weights(ggml_context *         ctx_meta,
                                          const QwenAsrHParams & hp,
                                          QwenAsrWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t d_model = hp.enc_d_model;
    const int64_t ffn_dim = hp.enc_ffn_dim;
    const int64_t ds_h    = hp.enc_downsample_hidden;
    const int64_t n_mels  = hp.enc_num_mel_bins;
    const int64_t out_dim = hp.enc_output_dim;

    // Downsampled mel axis: (((n_mels + 1) / 2 + 1) / 2 + 1) / 2.
    const int64_t mel_ds1     = (n_mels + 1) / 2;
    const int64_t mel_ds2     = (mel_ds1 + 1) / 2;
    const int64_t mel_ds3     = (mel_ds2 + 1) / 2;
    const int64_t conv_out_in = ds_h * mel_ds3;

    // ----- audio encoder: subsample -----
    //
    // Conv2d(in=1, out=480, k=3, stride=2, pad=1). ggml conv_2d expects
    // the kernel in (kW, kH, in_ch, out_ch) order — same as PyTorch
    // (out, in, kH, kW) after permute/view. find_tensor validates the
    // four dims explicitly.
    GET_CONV(weights.enc_subsample.conv0_w, "enc.conv.0.weight", 3, 3, 1, ds_h);
    GET_F32(weights.enc_subsample.conv0_b, "enc.conv.0.bias", ds_h);
    GET_CONV(weights.enc_subsample.conv1_w, "enc.conv.1.weight", 3, 3, ds_h, ds_h);
    GET_F32(weights.enc_subsample.conv1_b, "enc.conv.1.bias", ds_h);
    GET_CONV(weights.enc_subsample.conv2_w, "enc.conv.2.weight", 3, 3, ds_h, ds_h);
    GET_F32(weights.enc_subsample.conv2_b, "enc.conv.2.bias", ds_h);
    // conv_out is a Linear (bias=False) mapping (ds_h * mel_ds3) -> d_model.
    GET_LIN(weights.enc_subsample.conv_out, "enc.conv_out.weight", conv_out_in, d_model);

    // ----- audio encoder: blocks -----
    weights.enc_blocks.assign(hp.enc_n_layers, QwenAsrEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];
        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_model);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), d_model);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.q.weight", i), d_model, d_model);
        GET_F32(b.attn_q_b, lname("enc.blocks.%d.attn.q.bias", i), d_model);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.k.weight", i), d_model, d_model);
        GET_F32(b.attn_k_b, lname("enc.blocks.%d.attn.k.bias", i), d_model);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.v.weight", i), d_model, d_model);
        GET_F32(b.attn_v_b, lname("enc.blocks.%d.attn.v.bias", i), d_model);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.out.weight", i), d_model, d_model);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.out.bias", i), d_model);
        GET_F32(b.norm_ffn_w, lname("enc.blocks.%d.norm_ffn.weight", i), d_model);
        GET_F32(b.norm_ffn_b, lname("enc.blocks.%d.norm_ffn.bias", i), d_model);
        GET_LIN(b.fc1_w, lname("enc.blocks.%d.ffn.fc1.weight", i), d_model, ffn_dim);
        GET_F32(b.fc1_b, lname("enc.blocks.%d.ffn.fc1.bias", i), ffn_dim);
        GET_LIN(b.fc2_w, lname("enc.blocks.%d.ffn.fc2.weight", i), ffn_dim, d_model);
        GET_F32(b.fc2_b, lname("enc.blocks.%d.ffn.fc2.bias", i), d_model);
    }

    // ----- audio encoder: head (LN + proj1 + GELU + proj2) -----
    GET_F32(weights.enc_head.ln_post_w, "enc.ln_post.weight", d_model);
    GET_F32(weights.enc_head.ln_post_b, "enc.ln_post.bias", d_model);
    GET_LIN(weights.enc_head.proj1_w, "enc.proj1.weight", d_model, d_model);
    GET_F32(weights.enc_head.proj1_b, "enc.proj1.bias", d_model);
    GET_LIN(weights.enc_head.proj2_w, "enc.proj2.weight", d_model, out_dim);
    GET_F32(weights.enc_head.proj2_b, "enc.proj2.bias", out_dim);

    // ----- text LM: embedding (tied output) -----
    {
        ggml_tensor * tw = ggml_get_tensor(ctx_meta, "dec.token_embd.weight");
        if (tw == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: missing tensor \"dec.token_embd.weight\"");
            return TRANSCRIBE_ERR_GGUF;
        }
        if (tw->ne[0] != hp.dec_hidden) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: dec.token_embd.weight ne[0]=%lld, expected %lld",
                    static_cast<long long>(tw->ne[0]), static_cast<long long>(hp.dec_hidden));
            return TRANSCRIBE_ERR_GGUF;
        }
        if (tw->ne[1] != hp.dec_vocab_size) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "qwen3_asr: dec.token_embd.weight ne[1]=%lld, expected %lld",
                    static_cast<long long>(tw->ne[1]), static_cast<long long>(hp.dec_vocab_size));
            return TRANSCRIBE_ERR_GGUF;
        }
        weights.dec_embed.token_w = tw;
    }

    // ----- text LM: blocks -----
    const int64_t dec_h   = hp.dec_hidden;
    const int64_t dec_nh  = hp.dec_n_heads;
    const int64_t dec_nkv = hp.dec_n_kv_heads;
    const int64_t dec_hd  = hp.dec_head_dim;
    const int64_t dec_im  = hp.dec_intermediate;
    const int64_t q_out   = dec_nh * dec_hd;
    const int64_t kv_out  = dec_nkv * dec_hd;

    weights.dec_blocks.assign(hp.dec_n_layers, QwenAsrDecBlock{});
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        auto & b = weights.dec_blocks[i];
        GET_F32(b.norm_attn_w, lname("dec.blocks.%d.norm_attn.weight", i), dec_h);
        GET_F32(b.norm_ffn_w, lname("dec.blocks.%d.norm_ffn.weight", i), dec_h);
        GET_LIN(b.attn_q_w, lname("dec.blocks.%d.attn.q.weight", i), dec_h, q_out);
        GET_LIN(b.attn_k_w, lname("dec.blocks.%d.attn.k.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_v_w, lname("dec.blocks.%d.attn.v.weight", i), dec_h, kv_out);
        GET_LIN(b.attn_o_w, lname("dec.blocks.%d.attn.o.weight", i), q_out, dec_h);
        GET_F32(b.attn_q_norm, lname("dec.blocks.%d.attn.q_norm.weight", i), dec_hd);
        GET_F32(b.attn_k_norm, lname("dec.blocks.%d.attn.k_norm.weight", i), dec_hd);
        GET_LIN(b.ffn_gate_w, lname("dec.blocks.%d.ffn.gate.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_up_w, lname("dec.blocks.%d.ffn.up.weight", i), dec_h, dec_im);
        GET_LIN(b.ffn_down_w, lname("dec.blocks.%d.ffn.down.weight", i), dec_im, dec_h);

        if (b.ffn_gate_w->type != b.ffn_up_w->type) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "qwen3_asr: ffn gate/up dtype mismatch at layer %d "
                    "(%d vs %d)",
                    i, static_cast<int>(b.ffn_gate_w->type), static_cast<int>(b.ffn_up_w->type));
            return TRANSCRIBE_ERR_GGUF;
        }
        // ffn_gate_up_w is allocated by causal_lm::pack_gate_up (see
        // model.cpp) into a separate ctx, because gguf_init sizes
        // ctx_meta exactly for the file tensors with no headroom.
    }

    // ----- text LM: final norm -----
    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::qwen3_asr
