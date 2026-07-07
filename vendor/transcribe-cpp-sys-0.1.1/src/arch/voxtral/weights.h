// arch/voxtral/weights.h - Voxtral (2507) tensor catalog and hparams.
//
// INTERNAL to src/arch/voxtral/. VoxtralHParams: architecture KV (from
// stt.voxtral.* / stt.frontend.*, read before any tensor is allocated).
// VoxtralWeights: named borrowed ggml_tensor* slots (the tensors live in the
// model's ctx_meta / backend buffer). Three-sided weight layout:
//
//   enc.*   audio encoder  (2x Conv1d stem + 32 bidirectional pre-LN
//                           transformer layers, LayerNorm-with-bias,
//                           q/v/out biases but NO k bias, GELU FFN,
//                           final layer_norm; d_model=1280)
//   proj.*  projector       (Linear 5120->H, GELU, Linear H->H; no bias)
//   dec.*   text LM          (30 Llama layers, GQA 32/8, head_dim 128,
//                           SwiGLU, RMSNorm, NEOX RoPE theta 1e8,
//                           UNTIED lm_head)
//
// Tensor naming mirrors scripts/convert-voxtral.py. Linear weights use
// PyTorch [out, in] order (ggml ne=[in, out]); Conv1d kernels use
// PyTorch [out, in, k] (ggml ne=[k, in, out]).

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::voxtral {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct VoxtralHParams {
    // Audio encoder (Whisper-large-v3 encoder).
    int32_t     enc_n_layers             = 0;
    int32_t     enc_d_model              = 0;
    int32_t     enc_n_heads              = 0;
    int32_t     enc_head_dim             = 0;
    int32_t     enc_ffn_dim              = 0;
    int32_t     enc_num_mel_bins         = 0;
    int32_t     enc_max_source_positions = 0;  // 1500
    std::string enc_activation;                // "gelu"

    // Projector (multi-modal).
    int32_t     proj_downsample = 0;  // 4x frame grouping
    int32_t     proj_in         = 0;  // enc_d_model * downsample (5120)
    std::string proj_hidden_act;      // "gelu"

    // Text LM (Llama / Ministral).
    int32_t     dec_n_layers     = 0;
    int32_t     dec_hidden       = 0;
    int32_t     dec_intermediate = 0;
    int32_t     dec_n_heads      = 0;
    int32_t     dec_n_kv_heads   = 0;
    int32_t     dec_head_dim     = 0;
    std::string dec_hidden_act;  // "silu"
    float       dec_rms_norm_eps            = 0.0f;
    float       dec_rope_theta              = 0.0f;
    int32_t     dec_max_position_embeddings = 0;
    bool        dec_tie_word_embeddings     = false;  // UNTIED for Voxtral
    int32_t     dec_vocab_size              = 0;

    // Audio-token injection id.
    int32_t audio_token_id = 0;  // [AUDIO] placeholder (24)

    // Token IDs (resolved from tokenizer KV at load time).
    int32_t bos_token_id = -1;
    int32_t eos_token_id = -1;
    int32_t pad_token_id = -1;
    int32_t vocab_size   = 0;

    // Frontend (Whisper feature extractor).
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
    bool        fe_center = true;
    std::string fe_mel_norm;
    int32_t     fe_chunk_length  = 0;  // 30 (seconds)
    int32_t     fe_n_samples     = 0;  // 480000
    int32_t     fe_nb_max_frames = 0;  // 3000

    // Derived.
    int32_t enc_head_dim_calc() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    // Audio tokens produced per 30 s chunk: enc frames (1500) / downsample.
    int32_t audio_tokens_per_chunk() const {
        return (enc_max_source_positions > 0 && proj_downsample > 0) ? enc_max_source_positions / proj_downsample : 0;
    }
};

transcribe_status read_voxtral_hparams(const gguf_context * gguf, VoxtralHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots - audio encoder (Whisper-large-v3)
// ---------------------------------------------------------------------------

// 2x Conv1d stem + fixed sinusoidal positional embedding + final LN.
struct VoxtralEncStem {
    ggml_tensor * conv0_w = nullptr;  // [3, num_mel_bins, d_model] (F16)
    ggml_tensor * conv0_b = nullptr;  // [d_model] (F32)
    ggml_tensor * conv1_w = nullptr;  // [3, d_model, d_model] (F16)
    ggml_tensor * conv1_b = nullptr;  // [d_model] (F32)
};

struct VoxtralEncTop {
    ggml_tensor * pos_emb_w = nullptr;  // [d_model, max_source_positions] (F32)
    ggml_tensor * ln_post_w = nullptr;  // [d_model]
    ggml_tensor * ln_post_b = nullptr;  // [d_model]
};

// One encoder transformer block. q/v/out carry bias; k does NOT.
struct VoxtralEncBlock {
    ggml_tensor * norm_attn_w = nullptr;  // self_attn_layer_norm (w+b)
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_q_b    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;  // no bias
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_v_b    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;  // final_layer_norm (w+b)
    ggml_tensor * norm_ffn_b  = nullptr;
    ggml_tensor * fc1_w       = nullptr;  // [d_model, ffn_dim]
    ggml_tensor * fc1_b       = nullptr;
    ggml_tensor * fc2_w       = nullptr;  // [ffn_dim, d_model]
    ggml_tensor * fc2_b       = nullptr;
};

// Multi-modal projector (no biases).
struct VoxtralProjector {
    ggml_tensor * linear_1_w = nullptr;  // [proj_in, dec_hidden]
    ggml_tensor * linear_2_w = nullptr;  // [dec_hidden, dec_hidden]
};

// ---------------------------------------------------------------------------
// Weight slots - text LM (Llama / Ministral)
// ---------------------------------------------------------------------------

struct VoxtralDecEmbed {
    ggml_tensor * token_w  = nullptr;  // [hidden, vocab] embed_tokens
    ggml_tensor * output_w = nullptr;  // [hidden, vocab] UNTIED lm_head
};

struct VoxtralDecBlock {
    ggml_tensor * norm_attn_w   = nullptr;  // input_layernorm (RMSNorm)
    ggml_tensor * norm_ffn_w    = nullptr;  // post_attention_layernorm
    // GQA projections; no biases, NO per-head Q/K norm (Llama, not Qwen3).
    ggml_tensor * attn_q_w      = nullptr;  // [hidden, n_heads * head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_v_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_o_w      = nullptr;  // [n_heads * head_dim, hidden]
    // SwiGLU MLP.
    ggml_tensor * ffn_gate_w    = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_w      = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_down_w    = nullptr;  // [intermediate, hidden]
    // Packed gate+up: [hidden, 2*intermediate]. Filled at load time by
    // causal_lm::pack_gate_up(); the graph uses one mul_mat for both.
    ggml_tensor * ffn_gate_up_w = nullptr;
};

struct VoxtralDecFinal {
    ggml_tensor * norm_w = nullptr;  // dec.output_norm.weight (RMSNorm)
};

struct VoxtralWeights {
    VoxtralEncStem               enc_stem;
    VoxtralEncTop                enc_top;
    std::vector<VoxtralEncBlock> enc_blocks;

    VoxtralProjector proj;

    VoxtralDecEmbed              dec_embed;
    std::vector<VoxtralDecBlock> dec_blocks;
    VoxtralDecFinal              dec_final;
};

transcribe_status build_voxtral_weights(ggml_context * ctx_meta, const VoxtralHParams & hp, VoxtralWeights & weights);

}  // namespace transcribe::voxtral
