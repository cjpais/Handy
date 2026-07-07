// arch/qwen3_asr/weights.h - Qwen3-ASR tensor catalog and hparams.
//
// INTERNAL to src/arch/qwen3_asr/. Defines:
//
//   - QwenAsrHParams: architecture KV that drives tensor shapes. Read
//     from stt.qwen3_asr.* and stt.frontend.* at load time, before any
//     tensors are allocated.
//
//   - QwenAsrWeights: named ggml_tensor* slots, one per logical
//     weight. Borrowed pointers — the tensors themselves live in the
//     model's ctx_meta / backend buffer.
//
// Architecture pattern: audio-llm (audio encoder + causal Qwen3 LM
// with audio-token injection). Two-sided weight layout:
//
//   enc.*   audio encoder  (3x Conv2d subsampler + 18 bidirectional
//                           transformer layers, post-LayerNorm +
//                           proj1/proj2 head, 0.6B: d_model=896)
//   dec.*   text LM         (28 Qwen3 layers, GQA 16/8 heads,
//                           per-head Q/K-RMSNorm, SwiGLU, MRoPE,
//                           tied lm_head↔token_embd)
//
// Tensor naming mirrors the converter's output. Linear weights use
// PyTorch [out, in] order; Conv2d kernels use OIHW.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::qwen3_asr {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct QwenAsrHParams {
    // Audio encoder (thinker.audio_tower).
    int32_t     enc_n_layers             = 0;
    int32_t     enc_d_model              = 0;
    int32_t     enc_n_heads              = 0;
    int32_t     enc_ffn_dim              = 0;
    int32_t     enc_num_mel_bins         = 0;
    int32_t     enc_downsample_hidden    = 0;
    int32_t     enc_output_dim           = 0;
    int32_t     enc_max_source_positions = 0;
    int32_t     enc_n_window             = 0;
    int32_t     enc_n_window_infer       = 0;
    int32_t     enc_conv_chunksize       = 0;
    std::string enc_activation;  // "gelu"

    // Text LM (thinker.model — Qwen3 causal LM).
    int32_t     dec_n_layers     = 0;
    int32_t     dec_hidden       = 0;
    int32_t     dec_intermediate = 0;
    int32_t     dec_n_heads      = 0;
    int32_t     dec_n_kv_heads   = 0;
    int32_t     dec_head_dim     = 0;
    std::string dec_hidden_act;  // "silu"
    float       dec_rms_norm_eps = 0.0f;
    float       dec_rope_theta   = 0.0f;

    // Interleaved multimodal RoPE. The Qwen3 text-only ASR path uses
    // a single-modality position stream, so the [t,h,w] section split
    // collapses to flat rotation, but the cos/sin table shape is still
    // derived from these numbers. Kept explicit for fidelity to the
    // reference.
    int32_t dec_rope_mrope_section_t   = 0;
    int32_t dec_rope_mrope_section_h   = 0;
    int32_t dec_rope_mrope_section_w   = 0;
    bool    dec_rope_mrope_interleaved = true;

    int32_t dec_max_position_embeddings = 0;
    bool    dec_tie_word_embeddings     = true;
    int32_t dec_vocab_size              = 0;

    // Audio-token injection ids (from Qwen3ASRConfig.thinker_config).
    int32_t audio_token_id       = 0;  // <|audio_pad|>
    int32_t audio_start_token_id = 0;  // <|audio_start|>
    int32_t audio_end_token_id   = 0;  // <|audio_end|>

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
    int32_t     fe_chunk_length  = 0;
    int32_t     fe_n_samples     = 0;
    int32_t     fe_nb_max_frames = 0;
};

transcribe_status read_qwen3_asr_hparams(const gguf_context * gguf, QwenAsrHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots - audio encoder
// ---------------------------------------------------------------------------

// 3x Conv2d subsampler + final linear projection to d_model.
struct QwenAsrEncSubsample {
    ggml_tensor * conv0_w  = nullptr;  // [480, 1,   3, 3]
    ggml_tensor * conv0_b  = nullptr;  // [480]
    ggml_tensor * conv1_w  = nullptr;  // [480, 480, 3, 3]
    ggml_tensor * conv1_b  = nullptr;  // [480]
    ggml_tensor * conv2_w  = nullptr;  // [480, 480, 3, 3]
    ggml_tensor * conv2_b  = nullptr;  // [480]
    // conv_out flattens (downsample_hidden * mel_ds) -> d_model; no bias.
    ggml_tensor * conv_out = nullptr;  // [d_model, downsample_hidden * mel_ds]
};

// One audio-encoder transformer block (pre-LN, bidirectional,
// bias-carrying linear projections, per-chunk cu_seqlens mask).
struct QwenAsrEncBlock {
    ggml_tensor * norm_attn_w = nullptr;  // self_attn_layer_norm
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_q_b    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_k_b    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_v_b    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;  // final_layer_norm
    ggml_tensor * norm_ffn_b  = nullptr;
    ggml_tensor * fc1_w       = nullptr;  // [ffn_dim, d_model]
    ggml_tensor * fc1_b       = nullptr;
    ggml_tensor * fc2_w       = nullptr;  // [d_model, ffn_dim]
    ggml_tensor * fc2_b       = nullptr;
};

// Post-encoder head: LayerNorm + proj1 (GELU) + proj2 → output_dim.
struct QwenAsrEncHead {
    ggml_tensor * ln_post_w = nullptr;
    ggml_tensor * ln_post_b = nullptr;
    ggml_tensor * proj1_w   = nullptr;  // [d_model, d_model]
    ggml_tensor * proj1_b   = nullptr;
    ggml_tensor * proj2_w   = nullptr;  // [output_dim, d_model]
    ggml_tensor * proj2_b   = nullptr;
};

// ---------------------------------------------------------------------------
// Weight slots - text LM (Qwen3 causal LM)
// ---------------------------------------------------------------------------

struct QwenAsrDecEmbed {
    ggml_tensor * token_w = nullptr;  // [hidden, vocab_size] — tied to lm_head
};

struct QwenAsrDecBlock {
    ggml_tensor * norm_attn_w   = nullptr;  // input_layernorm (RMSNorm, no bias)
    ggml_tensor * norm_ffn_w    = nullptr;  // post_attention_layernorm
    // GQA projections; no biases on Qwen3. We experimented with packing
    // Q/K/V into one mul_mat but it consistently regressed on Metal —
    // the 3 small matvecs already run concurrently there and a combined
    // output's strided views trip downstream kernels. Left separate.
    ggml_tensor * attn_q_w      = nullptr;  // [hidden, n_heads * head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_v_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_o_w      = nullptr;  // [n_heads * head_dim, hidden]
    // Per-head Q/K RMSNorm applied on head_dim (Qwen3 innovation).
    ggml_tensor * attn_q_norm   = nullptr;  // [head_dim]
    ggml_tensor * attn_k_norm   = nullptr;  // [head_dim]
    // SwiGLU MLP.
    ggml_tensor * ffn_gate_w    = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_w      = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_down_w    = nullptr;  // [intermediate, hidden]
    // Packed gate+up projection: [hidden, 2*intermediate]. Filled at
    // load time from ffn_gate_w + ffn_up_w by causal_lm::pack_gate_up();
    // the graph uses this for one mul_mat instead of two.
    ggml_tensor * ffn_gate_up_w = nullptr;  // [hidden, 2*intermediate]
};

struct QwenAsrDecFinal {
    ggml_tensor * norm_w = nullptr;  // RMSNorm before lm_head
};

struct QwenAsrWeights {
    QwenAsrEncSubsample          enc_subsample;
    std::vector<QwenAsrEncBlock> enc_blocks;
    QwenAsrEncHead               enc_head;

    QwenAsrDecEmbed              dec_embed;
    std::vector<QwenAsrDecBlock> dec_blocks;
    QwenAsrDecFinal              dec_final;
    // No lm_head weight slot: tied to dec_embed.token_w. The graph
    // builder reuses token_w for the output projection.
};

transcribe_status build_qwen3_asr_weights(ggml_context * ctx_meta, const QwenAsrHParams & hp, QwenAsrWeights & weights);

}  // namespace transcribe::qwen3_asr
