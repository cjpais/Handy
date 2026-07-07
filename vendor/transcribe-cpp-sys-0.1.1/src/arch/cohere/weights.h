// arch/cohere/weights.h - canonical Cohere ASR tensor catalog and
// per-instance weight slots.
//
// This header is INTERNAL to src/arch/cohere/. It defines:
//
//   - CohereHParams: the architecture KV the loader reads from
//     stt.cohere.* / stt.frontend.* before allocating any tensors.
//     Every dim that drives a tensor shape lives here.
//
//   - CohereWeights: a struct of named borrowed ggml_tensor* slots,
//     one per logical weight in a Cohere ASR model.
//
// The encoder is structurally identical to Parakeet EXCEPT FFN layers
// have bias. The decoder is an autoregressive Transformer with
// self-attention, cross-attention, and FFN layers.
//
// Naming conventions: same as parakeet/weights.h (see that file for
// the full rationale). Linear weights stored in PyTorch [out, in]
// order; conv kernels in OIHW; LayerNorm with separate w + b.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::cohere {

struct CohereHParams {
    // Encoder (Conformer).
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
    int32_t     dec_n_layers = 0;
    int32_t     dec_hidden   = 0;
    int32_t     dec_n_heads  = 0;
    int32_t     dec_inner    = 0;
    int32_t     dec_max_seq  = 0;
    std::string dec_activation;  // "relu" or "silu"

    // Token IDs.
    int32_t vocab_size             = 0;
    int32_t decoder_start_token_id = 0;
    int32_t bos_token_id           = 0;
    int32_t eos_token_id           = 0;
    int32_t pad_token_id           = 0;

    // Head.
    bool head_log_softmax  = false;
    bool head_tied_weights = false;

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
    std::string fe_pad_mode;  // "reflect" or "constant"

    // Derived.
    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    int32_t dec_head_dim() const { return dec_n_heads > 0 ? dec_hidden / dec_n_heads : 0; }
};

transcribe_status read_cohere_hparams(const gguf_context * gguf, CohereHParams & hp);

struct CoherePreEncode {
    // Same structure as Parakeet: dw_striding subsampling.
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

// One Conformer block. Same as Parakeet but FFN layers have bias.
struct CohereBlock {
    // Macaron feed-forward 1 (with bias).
    ggml_tensor * norm_ff1_w = nullptr;
    ggml_tensor * norm_ff1_b = nullptr;
    ggml_tensor * ff1_lin1_w = nullptr;
    ggml_tensor * ff1_lin1_b = nullptr;
    ggml_tensor * ff1_lin2_w = nullptr;
    ggml_tensor * ff1_lin2_b = nullptr;

    // Self-attention with relative positional encoding.
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
    ggml_tensor * attn_pos_w  = nullptr;  // linear_pos (no bias)
    ggml_tensor * attn_pos_u  = nullptr;
    ggml_tensor * attn_pos_v  = nullptr;

    // Convolution module.
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
    // Fused BN (computed at load time).
    ggml_tensor * conv_bn_fused_scale = nullptr;
    ggml_tensor * conv_bn_fused_bias  = nullptr;

    // Macaron feed-forward 2 (with bias).
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_lin1_w = nullptr;
    ggml_tensor * ff2_lin1_b = nullptr;
    ggml_tensor * ff2_lin2_w = nullptr;
    ggml_tensor * ff2_lin2_b = nullptr;

    // Final per-block layer norm.
    ggml_tensor * norm_out_w = nullptr;
    ggml_tensor * norm_out_b = nullptr;
};

// Encoder-decoder projection.
struct CohereEncDecProj {
    ggml_tensor * weight = nullptr;  // [enc_d_model, dec_hidden]
    ggml_tensor * bias   = nullptr;  // [dec_hidden]
};

// Decoder embedding.
struct CohereDecEmbed {
    ggml_tensor * token_w = nullptr;  // [dec_hidden, vocab_size]
    ggml_tensor * pos_enc = nullptr;  // [dec_hidden, dec_max_seq]
    ggml_tensor * norm_w  = nullptr;  // [dec_hidden]
    ggml_tensor * norm_b  = nullptr;  // [dec_hidden]
};

// One decoder block.
struct CohereDecBlock {
    // Pre-LN self-attention.
    ggml_tensor * norm_self_w = nullptr;
    ggml_tensor * norm_self_b = nullptr;
    ggml_tensor * self_q_w    = nullptr;
    ggml_tensor * self_q_b    = nullptr;
    ggml_tensor * self_k_w    = nullptr;
    ggml_tensor * self_k_b    = nullptr;
    ggml_tensor * self_v_w    = nullptr;
    ggml_tensor * self_v_b    = nullptr;
    ggml_tensor * self_out_w  = nullptr;
    ggml_tensor * self_out_b  = nullptr;

    // Pre-LN cross-attention.
    ggml_tensor * norm_cross_w = nullptr;
    ggml_tensor * norm_cross_b = nullptr;
    ggml_tensor * cross_q_w    = nullptr;
    ggml_tensor * cross_q_b    = nullptr;
    ggml_tensor * cross_k_w    = nullptr;
    ggml_tensor * cross_k_b    = nullptr;
    ggml_tensor * cross_v_w    = nullptr;
    ggml_tensor * cross_v_b    = nullptr;
    ggml_tensor * cross_out_w  = nullptr;
    ggml_tensor * cross_out_b  = nullptr;

    // Pre-LN FFN.
    ggml_tensor * norm_ff_w = nullptr;
    ggml_tensor * norm_ff_b = nullptr;
    ggml_tensor * ff_in_w   = nullptr;
    ggml_tensor * ff_in_b   = nullptr;
    ggml_tensor * ff_out_w  = nullptr;
    ggml_tensor * ff_out_b  = nullptr;
};

// Decoder final norm.
struct CohereDecFinal {
    ggml_tensor * norm_w = nullptr;
    ggml_tensor * norm_b = nullptr;
};

// Head (bias only; weight is tied to dec_embed.token_w).
struct CohereHead {
    ggml_tensor * bias = nullptr;  // [vocab_size]
};

struct CohereWeights {
    CoherePreEncode             pre_encode;
    std::vector<CohereBlock>    blocks;
    CohereEncDecProj            enc_dec_proj;
    CohereDecEmbed              dec_embed;
    std::vector<CohereDecBlock> dec_blocks;
    CohereDecFinal              dec_final;
    CohereHead                  head;
};

transcribe_status build_cohere_weights(ggml_context * ctx_meta, const CohereHParams & hp, CohereWeights & weights);

}  // namespace transcribe::cohere
