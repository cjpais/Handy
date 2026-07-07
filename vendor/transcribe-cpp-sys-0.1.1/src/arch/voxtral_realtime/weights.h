// arch/voxtral_realtime/weights.h - Voxtral Realtime (2602) catalog + hparams.
//
// INTERNAL to src/arch/voxtral_realtime/. Streaming audio-LLM, distinct from the
// 2507 `voxtral` family. Three-sided weight layout:
//
//   enc.*   causal RoPE audio encoder (2x left-pad causal Conv1d stem +
//           32 pre-norm RMSNorm transformer layers; NEOX RoPE theta 1e6
//           head_dim 64; causal + sliding-window(750) attention with
//           q/v/out bias but NO k bias; SwiGLU/silu MLP with bias only on
//           down_proj; final RMSNorm; d_model=1280)
//   proj.*  projector (Linear 5120->H, GELU, Linear H->H; no bias)
//   dec.*   Ministral text LM (26 layers, GQA 32/8, head_dim 128, SwiGLU,
//           RMSNorm, NEOX RoPE theta 1e6, TIED lm_head, per-layer
//           delay-conditioned adaptive-norm FFN scale)
//
// Tensor naming mirrors scripts/convert-voxtral_realtime.py. Linear weights
// use PyTorch [out, in] order (ggml ne=[in, out]); Conv1d kernels use PyTorch
// [out, in, k] (ggml ne=[k, in, out]).

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::voxtral_realtime {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct HParams {
    // Audio encoder (causal RoPE, sliding-window).
    int32_t     enc_n_layers       = 0;
    int32_t     enc_d_model        = 0;
    int32_t     enc_n_heads        = 0;
    int32_t     enc_n_kv_heads     = 0;  // == n_heads (full MHA)
    int32_t     enc_head_dim       = 0;  // 64
    int32_t     enc_ffn_dim        = 0;  // 5120
    int32_t     enc_num_mel_bins   = 0;  // 128
    int32_t     enc_max_pos        = 0;  // 1500
    int32_t     enc_sliding_window = 0;  // 750
    float       enc_rope_theta     = 0.0f;
    float       enc_rms_norm_eps   = 0.0f;
    std::string enc_hidden_act;  // "silu" (MLP)

    // Projector (multi-modal).
    int32_t     proj_downsample      = 0;  // 4
    int32_t     proj_in              = 0;  // enc_d_model * downsample (5120)
    int32_t     audio_length_per_tok = 0;  // 8 mel frames per audio token
    std::string proj_hidden_act;           // "gelu"

    // Text LM (Ministral).
    int32_t     dec_n_layers     = 0;  // 26
    int32_t     dec_hidden       = 0;  // 3072
    int32_t     dec_intermediate = 0;  // 9216
    int32_t     dec_n_heads      = 0;  // 32
    int32_t     dec_n_kv_heads   = 0;  // 8
    int32_t     dec_head_dim     = 0;  // 128
    std::string dec_hidden_act;        // "silu"
    float       dec_rms_norm_eps        = 0.0f;
    float       dec_rope_theta          = 0.0f;
    int32_t     dec_sliding_window      = 0;  // 8192
    int32_t     dec_max_position        = 0;  // 131072
    bool        dec_tie_word_embeddings = true;
    int32_t     dec_vocab_size          = 0;  // 131072

    // Delay-token time conditioning.
    int32_t default_num_delay_tokens = 0;     // 6 (480 ms)
    float   time_embed_theta         = 0.0f;  // 10000
    int32_t time_embed_dim           = 0;     // 3072 (== dec_hidden)
    int32_t ada_hidden               = 0;     // 32

    // Streaming control token (additive audio fusion placeholder).
    int32_t streaming_pad_token_id = 32;

    // Token IDs (resolved from tokenizer KV at load time).
    int32_t bos_token_id = -1;
    int32_t eos_token_id = -1;
    int32_t pad_token_id = -1;
    int32_t vocab_size   = 0;

    // Frontend (streaming log-mel; FIXED global_log_mel_max).
    std::string fe_type;
    int32_t     fe_num_mels    = 0;
    int32_t     fe_sample_rate = 0;
    int32_t     fe_n_fft       = 0;
    int32_t     fe_win_length  = 0;
    int32_t     fe_hop_length  = 0;
    std::string fe_window;
    std::string fe_normalize;  // "global"
    float       fe_global_log_mel_max = 1.5f;
    float       fe_dither             = 0.0f;
    float       fe_pre_emphasis       = 0.0f;
    float       fe_f_min              = 0.0f;
    float       fe_f_max              = 8000.0f;
    std::string fe_pad_mode;
    bool        fe_center = true;
    std::string fe_mel_norm;

    // Audio tokens produced for `mel_frames` mel frames: ceil(mel/8).
    int32_t audio_tokens_for(int32_t mel_frames) const {
        return (audio_length_per_tok > 0) ? (mel_frames + audio_length_per_tok - 1) / audio_length_per_tok : 0;
    }
};

transcribe_status read_hparams(const gguf_context * gguf, HParams & hp);

// ---------------------------------------------------------------------------
// Weight slots - audio encoder
// ---------------------------------------------------------------------------

// 2x causal Conv1d stem + final RMSNorm.
struct EncStem {
    ggml_tensor * conv0_w      = nullptr;  // [3, num_mel_bins, d_model] (F16)
    ggml_tensor * conv0_b      = nullptr;  // [d_model] (F32)
    ggml_tensor * conv1_w      = nullptr;  // [3, d_model, d_model] (F16)
    ggml_tensor * conv1_b      = nullptr;  // [d_model] (F32)
    ggml_tensor * final_norm_w = nullptr;  // RMSNorm [d_model] (F32)
};

// One encoder transformer block. q/v/out carry bias; k does NOT. SwiGLU/silu
// MLP with bias only on down_proj. Pre-norm RMSNorm (no bias).
struct EncBlock {
    ggml_tensor * norm_attn_w   = nullptr;  // RMSNorm [d_model]
    ggml_tensor * attn_q_w      = nullptr;  // [d_model, n_heads*head_dim]
    ggml_tensor * attn_q_b      = nullptr;  // [n_heads*head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // no bias
    ggml_tensor * attn_v_w      = nullptr;
    ggml_tensor * attn_v_b      = nullptr;
    ggml_tensor * attn_out_w    = nullptr;
    ggml_tensor * attn_out_b    = nullptr;
    ggml_tensor * norm_ffn_w    = nullptr;  // RMSNorm [d_model]
    ggml_tensor * ffn_gate_w    = nullptr;  // [d_model, ffn_dim]
    ggml_tensor * ffn_up_w      = nullptr;  // [d_model, ffn_dim]
    ggml_tensor * ffn_down_w    = nullptr;  // [ffn_dim, d_model]
    ggml_tensor * ffn_down_b    = nullptr;  // [d_model]
    // Packed gate+up [d_model, 2*ffn_dim], filled at load time.
    ggml_tensor * ffn_gate_up_w = nullptr;
};

// Multi-modal projector (no biases).
struct Projector {
    ggml_tensor * linear_1_w = nullptr;  // [proj_in, dec_hidden]
    ggml_tensor * linear_2_w = nullptr;  // [dec_hidden, dec_hidden]
};

// ---------------------------------------------------------------------------
// Weight slots - text LM (Ministral, additive fusion, ada-norm)
// ---------------------------------------------------------------------------

struct DecEmbed {
    ggml_tensor * token_w = nullptr;  // [hidden, vocab] embed_tokens (TIED lm_head)
};

struct DecBlock {
    ggml_tensor * norm_attn_w    = nullptr;  // input_layernorm (RMSNorm)
    ggml_tensor * norm_ffn_w     = nullptr;  // post_attention_layernorm
    ggml_tensor * attn_q_w       = nullptr;  // [hidden, n_heads*head_dim]
    ggml_tensor * attn_k_w       = nullptr;  // [hidden, n_kv_heads*head_dim]
    ggml_tensor * attn_v_w       = nullptr;
    ggml_tensor * attn_o_w       = nullptr;  // [n_heads*head_dim, hidden]
    ggml_tensor * ffn_gate_w     = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_w       = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_down_w     = nullptr;  // [intermediate, hidden]
    ggml_tensor * ffn_gate_up_w  = nullptr;  // packed [hidden, 2*intermediate]
    // Adaptive-norm: ada(t) = linear2(gelu(linear1(t))). NO RMSNorm inside.
    ggml_tensor * ada_linear_1_w = nullptr;  // [hidden, ada_hidden]
    ggml_tensor * ada_linear_2_w = nullptr;  // [ada_hidden, hidden]
};

struct DecFinal {
    ggml_tensor * norm_w = nullptr;  // dec.output_norm.weight (RMSNorm)
};

struct Weights {
    EncStem               enc_stem;
    std::vector<EncBlock> enc_blocks;

    Projector proj;

    DecEmbed              dec_embed;
    std::vector<DecBlock> dec_blocks;
    DecFinal              dec_final;

    // Baked sinusoidal time-embedding inv_freq [time_embed_dim/2] (F32).
    ggml_tensor * time_inv_freq = nullptr;
};

transcribe_status build_weights(ggml_context * ctx_meta, const HParams & hp, Weights & weights);

}  // namespace transcribe::voxtral_realtime
