// arch/granite_nar/weights.h - tensor catalog + hparams for the IBM
// Granite Speech NLE (non-autoregressive editor) family.
//
// Architecture: encoder-CTC + Q-Former projector + bidirectional
// Granite-4 LM as a single-pass editor. See modeling_nle.py upstream.
//
// Layout summary:
//
//   enc.*  Conformer encoder (16 layers, hidden=1024) — same block as
//          the AR granite encoder, plus an extra `enc.ctc_bpe.*` head
//          used for the initial BPE CTC hypothesis.
//   prj.*  EncoderProjectorQFormer:
//             - 4 per-encoder-layer LayerNorms (one per layer index in
//               encoder_layer_indices, e.g. [4, 8, 12, -1])
//             - linear projector (4096 -> 2048) + GELU
//             - 2 Q-Former layers (cross-attn + MLP, NO self-attn)
//             - learned `query` (3 queries) and `window_positions`
//               (15-frame window pos emb)
//             - out_norm + out_linear (2048 -> 2048)
//   dec.*  Granite-4 1b base LM (40 layers, hidden=2048, GQA 16/4):
//             - tied lm_head (no separate output weight)
//             - 4 scalar multipliers baked into the graph (same as AR)
//             - is_causal = False on every layer (BIDIRECTIONAL forward)

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::granite_nar {

// Hyperparameters

struct GraniteNarHParams {
    // Encoder
    int32_t enc_n_layers         = 0;  // 16
    int32_t enc_hidden           = 0;  // 1024
    int32_t enc_n_heads          = 0;  // 8
    int32_t enc_head_dim         = 0;  // 128
    int32_t enc_input_dim        = 0;  // 160 (2 stacked mel frames)
    int32_t enc_output_dim       = 0;  // 348 (CTC char vocab)
    int32_t enc_bpe_output_dim   = 0;  // 100352 (new snapshot) or 100353 (old)
    int32_t enc_bpe_pool_window  = 0;  // 4
    // BPE-CTC blank id. New snapshot: 100257 (BOS, channel exists inside
    // the bpe_output_dim=vocab_size head, decode uses argmax directly).
    // Old snapshot: 0 (separate blank channel at index 0, decode subtracts 1
    // from non-blank argmax to recover the LLM token id).
    int32_t enc_bpe_blank_id     = 0;
    int32_t enc_self_cond_layer  = 0;  // 8 (1-indexed boundary)
    int32_t enc_feedforward_mult = 0;  // 4
    int32_t enc_conv_kernel_size = 0;  // 15
    int32_t enc_conv_expansion   = 0;  // 2
    int32_t enc_max_pos_emb      = 0;  // 512
    int32_t enc_context_size     = 0;  // 200

    // 1-indexed layer indices to capture into the projector's multi-layer
    // concat. e.g. [4, 8, 12, -1]. Negative values are interpreted as
    // "after layer N" with -1 == last; the C++ resolver normalizes to
    // the 0-indexed all_hidden_states array (where index 0 = input_linear
    // output, indices 1..N = output of each layer).
    std::vector<int32_t> enc_layer_indices;

    // Projector (EncoderProjectorQFormer)
    int32_t prj_n_layers           = 0;  // 2
    int32_t prj_hidden             = 0;  // 2048
    int32_t prj_mlp_ratio          = 0;  // 2
    int32_t prj_n_heads            = 0;  // 32
    int32_t prj_encoder_dim        = 0;  // 1024
    int32_t prj_num_encoder_layers = 0;  // 4 (matches len(enc_layer_indices))
    int32_t prj_block_size         = 0;  // 15
    int32_t prj_downsample_rate    = 0;  // 5 (block_size / downsample_rate = 3 queries)
    int32_t prj_llm_dim            = 0;  // 2048
    float   prj_layernorm_eps      = 0.0f;
    bool    prj_attn_bias          = true;
    bool    prj_mlp_bias           = true;

    // Whether the projector output is divided by embedding_multiplier on
    // its way into the LLM (true on this variant).
    bool scale_projected_embeddings = true;

    // Text LM (Granite-4)
    int32_t     dec_n_layers     = 0;  // 40
    int32_t     dec_hidden       = 0;  // 2048
    int32_t     dec_intermediate = 0;  // 4096
    int32_t     dec_n_heads      = 0;  // 16
    int32_t     dec_n_kv_heads   = 0;  // 4
    int32_t     dec_head_dim     = 0;  // 128
    std::string dec_hidden_act;
    float       dec_rms_norm_eps        = 0.0f;
    float       dec_rope_theta          = 0.0f;
    int32_t     dec_max_pos_emb         = 0;
    bool        dec_tie_word_embeddings = true;
    int32_t     dec_vocab_size          = 0;  // 100352

    float dec_embedding_multiplier = 0.0f;    // 12
    float dec_logits_scaling       = 0.0f;    // 8
    float dec_attention_multiplier = 0.0f;    // 1/128 = 0.0078125
    float dec_residual_multiplier  = 0.0f;    // 0.22

    int32_t dec_bos_id = 0;
    int32_t dec_eos_id = 0;
    int32_t dec_pad_id = 0;

    // Frontend
    std::string fe_type;  // "mel"
    int32_t     fe_sample_rate = 0;
    int32_t     fe_num_mels    = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;
    int32_t     fe_hop_length  = 0;
    std::string fe_window;
    std::string fe_normalize;
    std::string fe_pad_mode;
    std::string fe_mel_norm;

    // CTC char table: index -> single-character UTF-8 string. Index 0
    // is the blank token; gaps (no char maps there) are empty strings.
    std::vector<std::string> ctc_chars;
};

transcribe_status read_granite_nar_hparams(const gguf_context * gguf, GraniteNarHParams & hp);

// Weight slots

struct GraniteNarEncTop {
    ggml_tensor * input_linear_w = nullptr;
    ggml_tensor * input_linear_b = nullptr;
    ggml_tensor * ctc_proj_w     = nullptr;  // 1024 -> 348
    ggml_tensor * ctc_proj_b     = nullptr;
    ggml_tensor * ctc_bypass_w   = nullptr;  // 348 -> 1024 (out_mid)
    ggml_tensor * ctc_bypass_b   = nullptr;
    ggml_tensor * ctc_bpe_w      = nullptr;  // 1024 -> bpe_output_dim
    ggml_tensor * ctc_bpe_b      = nullptr;
};

struct GraniteNarEncBlock {
    // FF1 macaron
    ggml_tensor * norm_ff1_w          = nullptr;
    ggml_tensor * norm_ff1_b          = nullptr;
    ggml_tensor * ff1_up_w            = nullptr;
    ggml_tensor * ff1_up_b            = nullptr;
    ggml_tensor * ff1_down_w          = nullptr;
    ggml_tensor * ff1_down_b          = nullptr;
    // Shaw block-local attention
    ggml_tensor * norm_attn_w         = nullptr;
    ggml_tensor * norm_attn_b         = nullptr;
    ggml_tensor * attn_q_w            = nullptr;
    ggml_tensor * attn_kv_w           = nullptr;
    ggml_tensor * attn_out_w          = nullptr;
    ggml_tensor * attn_out_b          = nullptr;
    ggml_tensor * attn_rel_pos_emb    = nullptr;
    // Conv module
    ggml_tensor * norm_conv_w         = nullptr;
    ggml_tensor * norm_conv_b         = nullptr;
    ggml_tensor * conv_pointwise1_w   = nullptr;
    ggml_tensor * conv_pointwise1_b   = nullptr;
    ggml_tensor * conv_depthwise_w    = nullptr;
    ggml_tensor * conv_bn_w           = nullptr;
    ggml_tensor * conv_bn_b           = nullptr;
    ggml_tensor * conv_bn_mean        = nullptr;
    ggml_tensor * conv_bn_var         = nullptr;
    ggml_tensor * conv_pointwise2_w   = nullptr;
    ggml_tensor * conv_pointwise2_b   = nullptr;
    // Pre-fused BN (computed at load time).
    ggml_tensor * conv_bn_fused_scale = nullptr;
    ggml_tensor * conv_bn_fused_bias  = nullptr;
    // FF2 macaron
    ggml_tensor * norm_ff2_w          = nullptr;
    ggml_tensor * norm_ff2_b          = nullptr;
    ggml_tensor * ff2_up_w            = nullptr;
    ggml_tensor * ff2_up_b            = nullptr;
    ggml_tensor * ff2_down_w          = nullptr;
    ggml_tensor * ff2_down_b          = nullptr;
    // Post-block LayerNorm
    ggml_tensor * norm_post_w         = nullptr;
    ggml_tensor * norm_post_b         = nullptr;
};

struct GraniteNarProjTop {
    // Per-encoder-layer LayerNorms (one per encoder_layer_indices entry).
    // Indexed [0 .. num_encoder_layers).
    std::vector<ggml_tensor *> layer_norms_w;
    std::vector<ggml_tensor *> layer_norms_b;

    ggml_tensor * layer_projector_w = nullptr;  // 4096 -> 2048
    ggml_tensor * layer_projector_b = nullptr;
    ggml_tensor * out_norm_w        = nullptr;  // LN 2048
    ggml_tensor * out_norm_b        = nullptr;
    ggml_tensor * out_linear_w      = nullptr;  // 2048 -> 2048
    ggml_tensor * out_linear_b      = nullptr;
    ggml_tensor * query             = nullptr;  // [hidden, n_queries, 1]
    ggml_tensor * window_positions  = nullptr;  // [hidden, block_size, 1]
};

struct GraniteNarProjBlock {
    // pre-norm before cross-attention
    ggml_tensor * norm_attn_w    = nullptr;
    ggml_tensor * norm_attn_b    = nullptr;
    // cross-attention (q from query stream, k/v from encoder slice)
    ggml_tensor * cross_attn_q_w = nullptr;
    ggml_tensor * cross_attn_q_b = nullptr;
    ggml_tensor * cross_attn_k_w = nullptr;
    ggml_tensor * cross_attn_k_b = nullptr;
    ggml_tensor * cross_attn_v_w = nullptr;
    ggml_tensor * cross_attn_v_b = nullptr;
    ggml_tensor * cross_attn_o_w = nullptr;
    ggml_tensor * cross_attn_o_b = nullptr;
    // pre-norm before FFN
    ggml_tensor * norm_ffn_w     = nullptr;
    ggml_tensor * norm_ffn_b     = nullptr;
    // FFN (fc1 -> SiLU -> fc2)
    ggml_tensor * ffn_fc1_w      = nullptr;
    ggml_tensor * ffn_fc1_b      = nullptr;
    ggml_tensor * ffn_fc2_w      = nullptr;
    ggml_tensor * ffn_fc2_b      = nullptr;
};

struct GraniteNarDecEmbed {
    ggml_tensor * token_w = nullptr;  // tied: also serves as lm_head
};

struct GraniteNarDecBlock {
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_o_w    = nullptr;
    ggml_tensor * ffn_gate_w  = nullptr;
    ggml_tensor * ffn_up_w    = nullptr;
    ggml_tensor * ffn_down_w  = nullptr;
};

struct GraniteNarDecFinal {
    ggml_tensor * norm_w = nullptr;
};

struct GraniteNarWeights {
    GraniteNarEncTop                enc_top;
    std::vector<GraniteNarEncBlock> enc_blocks;

    GraniteNarProjTop                proj_top;
    std::vector<GraniteNarProjBlock> proj_blocks;

    GraniteNarDecEmbed              dec_embed;
    std::vector<GraniteNarDecBlock> dec_blocks;
    GraniteNarDecFinal              dec_final;
};

transcribe_status build_granite_nar_weights(ggml_context *            ctx_meta,
                                            const GraniteNarHParams & hp,
                                            GraniteNarWeights &       weights);

}  // namespace transcribe::granite_nar
