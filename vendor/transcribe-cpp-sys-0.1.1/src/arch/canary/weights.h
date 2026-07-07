// arch/canary/weights.h - Canary tensor catalog and per-instance weight slots.
//
// FastConformer encoder (parakeet shape, biases on every linear) +
// autoregressive Transformer decoder (untied LM head). 180m-flash adds an
// enc->dec projection; other variants share enc_d_model=dec_d_model.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::canary {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct CanaryHParams {
    // Encoder (FastConformer).
    int32_t enc_n_layers             = 0;
    int32_t enc_d_model              = 0;
    int32_t enc_n_heads              = 0;
    int32_t enc_d_ff                 = 0;
    int32_t enc_conv_kernel          = 0;
    int32_t enc_subsampling_factor   = 0;
    int32_t enc_subsampling_channels = 0;
    int32_t enc_pos_emb_max_len      = 0;
    bool    enc_use_bias             = false;

    // Decoder (autoregressive Transformer).
    int32_t     dec_n_layers     = 0;
    int32_t     dec_d_model      = 0;  // hidden size; d_dec
    int32_t     dec_n_heads      = 0;
    int32_t     dec_d_ff         = 0;  // FFN inner size
    int32_t     dec_max_position = 0;
    int32_t     dec_vocab_size   = 0;  // declared in KV; cross-checked against dec.embed.token.weight
    std::string dec_activation;        // "relu" / "silu" / "swish"
    bool        dec_pre_ln                     = true;
    bool        dec_learn_positional_encodings = false;
    bool        dec_has_encoder_decoder_proj   = false;  // 180m-flash: true; others: false

    // True when the tokenizer is a single SentencePiece (canary-1b-v2)
    // rather than a CanaryTokenizer aggregate. Single-SP tokenizers
    // render an empty decoder-context slot as a leading whitespace
    // marker (`▁`) in the canary2 prompt — adds one token to the prompt
    // length. Aggregate tokenizers skip the empty slot entirely.
    bool tokenizer_single_sp = false;

    // Token IDs (filled from tokenizer at load time).
    int32_t bos_token_id = -1;
    int32_t eos_token_id = -1;
    int32_t pad_token_id = -1;

    // Multitask special-token catalog. Read from stt.canary.special.* KV.
    std::string prompt_format;  // "canary" or "canary2"
    int32_t     startoftranscript_id = -1;
    int32_t     startofcontext_id    = -1;
    int32_t     endoftext_id         = -1;
    int32_t     pad_special_id       = -1;
    int32_t     nospeech_id          = -1;
    int32_t     pnc_id               = -1;
    int32_t     nopnc_id             = -1;
    int32_t     itn_id               = -1;
    int32_t     noitn_id             = -1;
    int32_t     timestamp_id         = -1;
    int32_t     notimestamp_id       = -1;
    int32_t     diarize_id           = -1;
    int32_t     nodiarize_id         = -1;
    int32_t     spkchange_id         = -1;
    int32_t     audioseparator_id    = -1;
    // canary-1 explicit task tokens (canary2 omits these — task is
    // inferred from src_lang vs tgt_lang).
    int32_t     transcribe_id        = -1;
    int32_t     translate_id         = -1;

    // Language code -> language token id, in the order published via
    // stt.canary.tokenizer.lang_codes / stt.canary.special.lang.<code>_id.
    std::vector<std::string> languages;     // e.g. {"en","de","es","fr"}
    std::vector<int32_t>     language_ids;  // parallel: id of "<|en|>" etc.

    // Optional GGUF contract: allowed translation pairs as "src>target".
    // Old GGUFs do not carry this KV; an empty list preserves legacy behavior.
    std::vector<std::string> translation_pairs;

    // Frontend (mel feature extractor).
    std::string fe_type;
    int32_t     fe_num_mels    = 0;
    int32_t     fe_sample_rate = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;
    int32_t     fe_hop_length  = 0;
    std::string fe_window;
    std::string fe_normalize;
    float       fe_dither       = 0.0f;
    float       fe_pre_emphasis = 0.0f;
    float       fe_f_min        = 0.0f;
    float       fe_f_max        = 0.0f;
    std::string fe_pad_mode;

    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    int32_t dec_head_dim() const { return dec_n_heads > 0 ? dec_d_model / dec_n_heads : 0; }
};

transcribe_status read_canary_hparams(const gguf_context * gguf, CanaryHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots
// ---------------------------------------------------------------------------

struct CanaryPreEncode {
    // dw_striding subsampling — same shape as parakeet/cohere.
    ggml_tensor * conv0_w = nullptr;
    ggml_tensor * conv0_b = nullptr;
    ggml_tensor * conv2_w = nullptr;
    ggml_tensor * conv2_b = nullptr;
    ggml_tensor * conv3_w = nullptr;
    ggml_tensor * conv3_b = nullptr;
    ggml_tensor * conv5_w = nullptr;
    ggml_tensor * conv5_b = nullptr;
    ggml_tensor * conv6_w = nullptr;
    ggml_tensor * conv6_b = nullptr;
    ggml_tensor * out_w   = nullptr;
    ggml_tensor * out_b   = nullptr;
};

// One FastConformer encoder block. Every canary variant ships biases on the
// FFN and attention linears, regardless of the stt.canary.encoder.use_bias KV.
struct CanaryBlock {
    ggml_tensor * norm_ff1_w = nullptr;
    ggml_tensor * norm_ff1_b = nullptr;
    ggml_tensor * ff1_lin1_w = nullptr;
    ggml_tensor * ff1_lin1_b = nullptr;
    ggml_tensor * ff1_lin2_w = nullptr;
    ggml_tensor * ff1_lin2_b = nullptr;

    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_q_b    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_k_b    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_v_b    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;
    ggml_tensor * attn_pos_w  = nullptr;
    ggml_tensor * attn_pos_u  = nullptr;
    ggml_tensor * attn_pos_v  = nullptr;

    // Conv module — BN fused at load into _scale/_bias.
    ggml_tensor * norm_conv_w         = nullptr;
    ggml_tensor * norm_conv_b         = nullptr;
    ggml_tensor * conv_pw1_w          = nullptr;
    ggml_tensor * conv_pw1_b          = nullptr;
    ggml_tensor * conv_dw_w           = nullptr;
    ggml_tensor * conv_dw_b           = nullptr;
    ggml_tensor * conv_pw2_w          = nullptr;
    ggml_tensor * conv_pw2_b          = nullptr;
    ggml_tensor * conv_bn_w           = nullptr;
    ggml_tensor * conv_bn_b           = nullptr;
    ggml_tensor * conv_bn_rm          = nullptr;
    ggml_tensor * conv_bn_rv          = nullptr;
    ggml_tensor * conv_bn_fused_scale = nullptr;
    ggml_tensor * conv_bn_fused_bias  = nullptr;

    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_lin1_w = nullptr;
    ggml_tensor * ff2_lin1_b = nullptr;
    ggml_tensor * ff2_lin2_w = nullptr;
    ggml_tensor * ff2_lin2_b = nullptr;

    ggml_tensor * norm_out_w = nullptr;
    ggml_tensor * norm_out_b = nullptr;
};

// Optional encoder->decoder projection (180m-flash only).
struct CanaryEncProj {
    ggml_tensor * weight = nullptr;  // [enc_d_model, dec_d_model]
    ggml_tensor * bias   = nullptr;  // [dec_d_model]
};

struct CanaryDecEmbed {
    ggml_tensor * token_w = nullptr;  // [dec_d_model, vocab_size]
    ggml_tensor * pos_enc = nullptr;  // [dec_d_model, max_position]
    ggml_tensor * norm_w  = nullptr;  // [dec_d_model]
    ggml_tensor * norm_b  = nullptr;  // [dec_d_model]
};

// One decoder block. Sublayer naming follows NeMo's pre-LN ordering:
//   norm1 -> self_attn -> add residual
//   norm2 -> cross_attn -> add residual
//   norm3 -> ffn -> add residual
struct CanaryDecBlock {
    ggml_tensor * norm1_w  = nullptr;
    ggml_tensor * norm1_b  = nullptr;
    ggml_tensor * self_q_w = nullptr;
    ggml_tensor * self_q_b = nullptr;
    ggml_tensor * self_k_w = nullptr;
    ggml_tensor * self_k_b = nullptr;
    ggml_tensor * self_v_w = nullptr;
    ggml_tensor * self_v_b = nullptr;
    ggml_tensor * self_o_w = nullptr;
    ggml_tensor * self_o_b = nullptr;

    ggml_tensor * norm2_w   = nullptr;
    ggml_tensor * norm2_b   = nullptr;
    ggml_tensor * cross_q_w = nullptr;
    ggml_tensor * cross_q_b = nullptr;
    ggml_tensor * cross_k_w = nullptr;
    ggml_tensor * cross_k_b = nullptr;
    ggml_tensor * cross_v_w = nullptr;
    ggml_tensor * cross_v_b = nullptr;
    ggml_tensor * cross_o_w = nullptr;
    ggml_tensor * cross_o_b = nullptr;

    ggml_tensor * norm3_w    = nullptr;
    ggml_tensor * norm3_b    = nullptr;
    ggml_tensor * ffn_up_w   = nullptr;
    ggml_tensor * ffn_up_b   = nullptr;
    ggml_tensor * ffn_down_w = nullptr;
    ggml_tensor * ffn_down_b = nullptr;
};

struct CanaryDecFinal {
    ggml_tensor * norm_w = nullptr;
    ggml_tensor * norm_b = nullptr;
};

// Untied LM head (different from cohere — canary ships explicit weight + bias).
struct CanaryHead {
    ggml_tensor * weight = nullptr;  // [dec_d_model, vocab_size]
    ggml_tensor * bias   = nullptr;  // [vocab_size]
};

struct CanaryWeights {
    CanaryPreEncode             pre_encode;
    std::vector<CanaryBlock>    blocks;
    CanaryEncProj               enc_proj;  // unused when has_encoder_decoder_proj=false
    CanaryDecEmbed              dec_embed;
    std::vector<CanaryDecBlock> dec_blocks;
    CanaryDecFinal              dec_final;
    CanaryHead                  head;
};

transcribe_status build_canary_weights(ggml_context * ctx_meta, const CanaryHParams & hp, CanaryWeights & weights);

}  // namespace transcribe::canary
