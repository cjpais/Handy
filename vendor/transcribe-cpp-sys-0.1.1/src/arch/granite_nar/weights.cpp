// arch/granite_nar/weights.cpp - hparams + tensor catalog reader.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>

namespace transcribe::granite_nar {

namespace {
constexpr const char * kTag = "granite_nar";

using transcribe::weights::lname;

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

transcribe_status read_granite_nar_hparams(const gguf_context * gguf, GraniteNarHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.n_layers", kTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.hidden", kTag, hp.enc_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.n_heads", kTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.head_dim", kTag, hp.enc_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.input_dim", kTag, hp.enc_input_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.output_dim", kTag, hp.enc_output_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.bpe_output_dim", kTag, hp.enc_bpe_output_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.bpe_pool_window", kTag, hp.enc_bpe_pool_window);
        st != TRANSCRIBE_OK) {
        return st;
    }
    // bpe_blank_id is an optional key; older GGUFs lack it. Default to 0 to
    // preserve the legacy decode scheme.
    {
        int32_t tmp = 0;
        if (read_required_u32_kv(gguf, "stt.granite_nar.encoder.bpe_blank_id", kTag, tmp) == TRANSCRIBE_OK) {
            hp.enc_bpe_blank_id = tmp;
        } else {
            hp.enc_bpe_blank_id = 0;
        }
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.self_cond_layer", kTag, hp.enc_self_cond_layer);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.feedforward_mult", kTag, hp.enc_feedforward_mult);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.conv_kernel_size", kTag, hp.enc_conv_kernel_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.conv_expansion", kTag, hp.enc_conv_expansion);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.max_pos_emb", kTag, hp.enc_max_pos_emb);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.encoder.context_size", kTag, hp.enc_context_size);
        st != TRANSCRIBE_OK) {
        return st;
    }

    {
        std::vector<int32_t> tmp;
        switch (read_int32_array_kv(gguf, "stt.granite_nar.encoder.layer_indices", tmp)) {
            case KvResult::Ok:
                hp.enc_layer_indices = std::move(tmp);
                break;
            case KvResult::Absent:
                hp.enc_layer_indices = { -1 };
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: encoder.layer_indices wrong type", kTag);
                return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Projector
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.n_layers", kTag, hp.prj_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.hidden", kTag, hp.prj_hidden);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.mlp_ratio", kTag, hp.prj_mlp_ratio);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.n_heads", kTag, hp.prj_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.encoder_dim", kTag, hp.prj_encoder_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.granite_nar.projector.num_encoder_layers", kTag, hp.prj_num_encoder_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.block_size", kTag, hp.prj_block_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.downsample_rate", kTag, hp.prj_downsample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.projector.llm_dim", kTag, hp.prj_llm_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite_nar.projector.layernorm_eps", kTag, hp.prj_layernorm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.granite_nar.projector.attn_bias", kTag, true, hp.prj_attn_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.granite_nar.projector.mlp_bias", kTag, true, hp.prj_mlp_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.granite_nar.scale_projected_embeddings", kTag, true,
                                        hp.scale_projected_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Text LM
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.n_layers", kTag, hp.dec_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.hidden", kTag, hp.dec_hidden); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.intermediate", kTag, hp.dec_intermediate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.n_heads", kTag, hp.dec_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.n_kv_heads", kTag, hp.dec_n_kv_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.head_dim", kTag, hp.dec_head_dim);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.granite_nar.text.hidden_act", kTag, hp.dec_hidden_act);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite_nar.text.rms_norm_eps", kTag, hp.dec_rms_norm_eps);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite_nar.text.rope_theta", kTag, hp.dec_rope_theta);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.max_position_embeddings", kTag, hp.dec_max_pos_emb);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_bool_kv(gguf, "stt.granite_nar.text.tie_word_embeddings", kTag, true,
                                        hp.dec_tie_word_embeddings);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.vocab_size", kTag, hp.dec_vocab_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_f32_kv(gguf, "stt.granite_nar.text.embedding_multiplier", kTag, hp.dec_embedding_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.granite_nar.text.logits_scaling", kTag, hp.dec_logits_scaling);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_f32_kv(gguf, "stt.granite_nar.text.attention_multiplier", kTag, hp.dec_attention_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_f32_kv(gguf, "stt.granite_nar.text.residual_multiplier", kTag, hp.dec_residual_multiplier);
        st != TRANSCRIBE_OK) {
        return st;
    }
    {
        int32_t tmp;
        if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.bos_id", kTag, tmp); st != TRANSCRIBE_OK) {
            return st;
        }
        hp.dec_bos_id = tmp;
        if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.eos_id", kTag, tmp); st != TRANSCRIBE_OK) {
            return st;
        }
        hp.dec_eos_id = tmp;
        if (auto st = read_required_u32_kv(gguf, "stt.granite_nar.text.pad_id", kTag, tmp); st != TRANSCRIBE_OK) {
            return st;
        }
        hp.dec_pad_id = tmp;
    }

    // Frontend
    if (auto st = read_required_string_kv(gguf, "stt.frontend.type", kTag, hp.fe_type); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.sample_rate", kTag, hp.fe_sample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.num_mels", kTag, hp.fe_num_mels); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.n_fft", kTag, hp.fe_n_fft); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.win_length", kTag, hp.fe_win_length); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.hop_length", kTag, hp.fe_hop_length); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.window", kTag, hp.fe_window); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.normalize", kTag, hp.fe_normalize); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.pad_mode", kTag, "reflect", hp.fe_pad_mode);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_string_kv(gguf, "stt.frontend.mel_norm", kTag, "htk", hp.fe_mel_norm);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // CTC chars array.
    {
        std::vector<std::string> tmp;
        switch (read_string_array_kv(gguf, "stt.granite_nar.ctc_chars", tmp)) {
            case KvResult::Ok:
                hp.ctc_chars = std::move(tmp);
                break;
            case KvResult::Absent:
                hp.ctc_chars.clear();
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: ctc_chars wrong type", kTag);
                return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Cross-field invariants.
    if (hp.prj_num_encoder_layers != (int32_t) hp.enc_layer_indices.size()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "%s: prj_num_encoder_layers (%d) != "
                "len(enc_layer_indices) (%zu)",
                kTag, hp.prj_num_encoder_layers, hp.enc_layer_indices.size());
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.dec_hidden != hp.prj_llm_dim) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: dec_hidden (%d) != prj_llm_dim (%d)", kTag, hp.dec_hidden,
                hp.prj_llm_dim);
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

transcribe_status build_granite_nar_weights(ggml_context *            ctx_meta,
                                            const GraniteNarHParams & hp,
                                            GraniteNarWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t enc_h       = hp.enc_hidden;
    const int64_t enc_in      = hp.enc_input_dim;
    const int64_t enc_out     = hp.enc_output_dim;
    const int64_t enc_bpe_out = hp.enc_bpe_output_dim;
    const int64_t enc_inner   = (int64_t) hp.enc_n_heads * hp.enc_head_dim;
    const int64_t enc_ffn     = enc_h * hp.enc_feedforward_mult;
    const int64_t conv_inner  = enc_h * hp.enc_conv_expansion;
    const int64_t conv_up_out = conv_inner * 2;
    const int64_t conv_k      = hp.enc_conv_kernel_size;
    const int64_t rel_pos_len = 2 * hp.enc_max_pos_emb + 1;

    const int64_t prj_h        = hp.prj_hidden;
    const int64_t prj_im       = (int64_t) hp.prj_hidden * hp.prj_mlp_ratio;
    const int64_t prj_kv_in    = (int64_t) hp.prj_num_encoder_layers * hp.prj_encoder_dim;
    const int64_t prj_llm_dim  = hp.prj_llm_dim;
    const int64_t prj_n_layers = hp.prj_n_layers;
    const int64_t prj_n_query  = hp.prj_block_size / hp.prj_downsample_rate;
    const int64_t prj_block_sz = hp.prj_block_size;
    const int64_t prj_num_enc  = hp.prj_num_encoder_layers;

    const int64_t dec_h     = hp.dec_hidden;
    const int64_t dec_vocab = hp.dec_vocab_size;
    const int64_t dec_nh    = hp.dec_n_heads;
    const int64_t dec_nkv   = hp.dec_n_kv_heads;
    const int64_t dec_hd    = hp.dec_head_dim;
    const int64_t dec_im    = hp.dec_intermediate;
    const int64_t q_out     = dec_nh * dec_hd;
    const int64_t kv_out    = dec_nkv * dec_hd;

    // Encoder top.
    GET_LIN(weights.enc_top.input_linear_w, "enc.input_linear.weight", enc_in, enc_h);
    GET_F32(weights.enc_top.input_linear_b, "enc.input_linear.bias", enc_h);
    GET_LIN(weights.enc_top.ctc_proj_w, "enc.ctc_proj.weight", enc_h, enc_out);
    GET_F32(weights.enc_top.ctc_proj_b, "enc.ctc_proj.bias", enc_out);
    GET_LIN(weights.enc_top.ctc_bypass_w, "enc.ctc_bypass.weight", enc_out, enc_h);
    GET_F32(weights.enc_top.ctc_bypass_b, "enc.ctc_bypass.bias", enc_h);
    if (enc_bpe_out > 0) {
        GET_LIN(weights.enc_top.ctc_bpe_w, "enc.ctc_bpe.weight", enc_h, enc_bpe_out);
        GET_F32(weights.enc_top.ctc_bpe_b, "enc.ctc_bpe.bias", enc_bpe_out);
    }

    // Encoder blocks.
    weights.enc_blocks.assign(hp.enc_n_layers, GraniteNarEncBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.enc_blocks[i];
        GET_F32(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i), enc_h);
        GET_F32(b.norm_ff1_b, lname("enc.blocks.%d.norm_ff1.bias", i), enc_h);
        GET_LIN(b.ff1_up_w, lname("enc.blocks.%d.ff1_up.weight", i), enc_h, enc_ffn);
        GET_F32(b.ff1_up_b, lname("enc.blocks.%d.ff1_up.bias", i), enc_ffn);
        GET_LIN(b.ff1_down_w, lname("enc.blocks.%d.ff1_down.weight", i), enc_ffn, enc_h);
        GET_F32(b.ff1_down_b, lname("enc.blocks.%d.ff1_down.bias", i), enc_h);

        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), enc_h);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), enc_h);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn_q.weight", i), enc_h, enc_inner);
        GET_LIN(b.attn_kv_w, lname("enc.blocks.%d.attn_kv.weight", i), enc_h, 2 * enc_inner);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn_out.weight", i), enc_inner, enc_h);
        GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn_out.bias", i), enc_h);
        GET_LIN(b.attn_rel_pos_emb, lname("enc.blocks.%d.attn_rel_pos_emb.weight", i), hp.enc_head_dim, rel_pos_len);

        GET_F32(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i), enc_h);
        GET_F32(b.norm_conv_b, lname("enc.blocks.%d.norm_conv.bias", i), enc_h);
        GET_CONV(b.conv_pointwise1_w, lname("enc.blocks.%d.conv_pointwise1.weight", i), 1, enc_h, conv_up_out);
        GET_F32(b.conv_pointwise1_b, lname("enc.blocks.%d.conv_pointwise1.bias", i), conv_up_out);
        GET_CONV(b.conv_depthwise_w, lname("enc.blocks.%d.conv_depthwise.weight", i), conv_k, 1, conv_inner);
        GET_F32(b.conv_bn_w, lname("enc.blocks.%d.conv_bn.weight", i), conv_inner);
        GET_F32(b.conv_bn_b, lname("enc.blocks.%d.conv_bn.bias", i), conv_inner);
        GET_F32(b.conv_bn_mean, lname("enc.blocks.%d.conv_bn.running_mean", i), conv_inner);
        GET_F32(b.conv_bn_var, lname("enc.blocks.%d.conv_bn.running_var", i), conv_inner);
        GET_CONV(b.conv_pointwise2_w, lname("enc.blocks.%d.conv_pointwise2.weight", i), 1, conv_inner, enc_h);
        GET_F32(b.conv_pointwise2_b, lname("enc.blocks.%d.conv_pointwise2.bias", i), enc_h);

        GET_F32(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i), enc_h);
        GET_F32(b.norm_ff2_b, lname("enc.blocks.%d.norm_ff2.bias", i), enc_h);
        GET_LIN(b.ff2_up_w, lname("enc.blocks.%d.ff2_up.weight", i), enc_h, enc_ffn);
        GET_F32(b.ff2_up_b, lname("enc.blocks.%d.ff2_up.bias", i), enc_ffn);
        GET_LIN(b.ff2_down_w, lname("enc.blocks.%d.ff2_down.weight", i), enc_ffn, enc_h);
        GET_F32(b.ff2_down_b, lname("enc.blocks.%d.ff2_down.bias", i), enc_h);

        GET_F32(b.norm_post_w, lname("enc.blocks.%d.norm_post.weight", i), enc_h);
        GET_F32(b.norm_post_b, lname("enc.blocks.%d.norm_post.bias", i), enc_h);
    }

    // Projector top.
    GET_LIN(weights.proj_top.layer_projector_w, "prj.layer_projector.weight", prj_kv_in, prj_h);
    GET_F32(weights.proj_top.layer_projector_b, "prj.layer_projector.bias", prj_h);
    GET_F32(weights.proj_top.out_norm_w, "prj.out_norm.weight", prj_h);
    GET_F32(weights.proj_top.out_norm_b, "prj.out_norm.bias", prj_h);
    GET_LIN(weights.proj_top.out_linear_w, "prj.out_linear.weight", prj_h, prj_llm_dim);
    GET_F32(weights.proj_top.out_linear_b, "prj.out_linear.bias", prj_llm_dim);
    // query is [1, n_query=3, hidden=2048]. In HF tensor.shape this is
    // (1, 3, 2048). ggml stores reverse: ne[0]=2048, ne[1]=3, ne[2]=1.
    {
        ggml_tensor * _t = transcribe::weights::find_tensor(
            ctx_meta, "prj.query", { GGML_TYPE_F32, GGML_TYPE_BF16, GGML_TYPE_F16 }, { prj_h, prj_n_query, 1 }, kTag);
        if (_t == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        weights.proj_top.query = _t;
    }
    {
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, "prj.window_positions",
                                                            { GGML_TYPE_F32, GGML_TYPE_BF16, GGML_TYPE_F16 },
                                                            { prj_h, prj_block_sz, 1 }, kTag);
        if (_t == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        weights.proj_top.window_positions = _t;
    }

    weights.proj_top.layer_norms_w.assign(prj_num_enc, nullptr);
    weights.proj_top.layer_norms_b.assign(prj_num_enc, nullptr);
    for (int j = 0; j < prj_num_enc; ++j) {
        GET_F32(weights.proj_top.layer_norms_w[j], lname("prj.layer_norms.%d.weight", j), hp.prj_encoder_dim);
        GET_F32(weights.proj_top.layer_norms_b[j], lname("prj.layer_norms.%d.bias", j), hp.prj_encoder_dim);
    }

    weights.proj_blocks.assign(prj_n_layers, GraniteNarProjBlock{});
    for (int i = 0; i < prj_n_layers; ++i) {
        auto & b = weights.proj_blocks[i];
        GET_F32(b.norm_attn_w, lname("prj.blocks.%d.norm_attn.weight", i), prj_h);
        GET_F32(b.norm_attn_b, lname("prj.blocks.%d.norm_attn.bias", i), prj_h);

        GET_LIN(b.cross_attn_q_w, lname("prj.blocks.%d.cross_attn_q.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_q_b, lname("prj.blocks.%d.cross_attn_q.bias", i), prj_h);
        GET_LIN(b.cross_attn_k_w, lname("prj.blocks.%d.cross_attn_k.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_k_b, lname("prj.blocks.%d.cross_attn_k.bias", i), prj_h);
        GET_LIN(b.cross_attn_v_w, lname("prj.blocks.%d.cross_attn_v.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_v_b, lname("prj.blocks.%d.cross_attn_v.bias", i), prj_h);
        GET_LIN(b.cross_attn_o_w, lname("prj.blocks.%d.cross_attn_o.weight", i), prj_h, prj_h);
        GET_F32(b.cross_attn_o_b, lname("prj.blocks.%d.cross_attn_o.bias", i), prj_h);

        GET_F32(b.norm_ffn_w, lname("prj.blocks.%d.norm_ffn.weight", i), prj_h);
        GET_F32(b.norm_ffn_b, lname("prj.blocks.%d.norm_ffn.bias", i), prj_h);
        GET_LIN(b.ffn_fc1_w, lname("prj.blocks.%d.ffn_fc1.weight", i), prj_h, prj_im);
        GET_F32(b.ffn_fc1_b, lname("prj.blocks.%d.ffn_fc1.bias", i), prj_im);
        GET_LIN(b.ffn_fc2_w, lname("prj.blocks.%d.ffn_fc2.weight", i), prj_im, prj_h);
        GET_F32(b.ffn_fc2_b, lname("prj.blocks.%d.ffn_fc2.bias", i), prj_h);
    }

    // LLM (granite-4).
    GET_LIN(weights.dec_embed.token_w, "dec.token_embd.weight", dec_h, dec_vocab);

    weights.dec_blocks.assign(hp.dec_n_layers, GraniteNarDecBlock{});
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
    }

    GET_F32(weights.dec_final.norm_w, "dec.output_norm.weight", dec_h);

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::granite_nar
