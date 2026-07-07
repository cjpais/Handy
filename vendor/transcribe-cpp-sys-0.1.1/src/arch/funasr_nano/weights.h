// arch/funasr_nano/weights.h - Fun-ASR-Nano tensor catalog and hparams.
//
// INTERNAL to src/arch/funasr_nano/. Defines:
//
//   - FunAsrNanoHParams: KV the loader reads from stt.funasr_nano.* and
//     stt.frontend.* before any tensors are allocated.
//
//   - FunAsrNanoWeights: borrowed ggml_tensor* slots for every logical
//     weight. Tensors live in the model's ctx_meta / backend buffer.
//
// Architecture pattern: audio-llm.
//
//   enc.*       SenseVoiceEncoderSmall  (50 SAN-M blocks + 20 tp blocks,
//                                        d_model=512, d_ff=2048; no CMVN
//                                        and no prefix-embedding prepend)
//   adaptor.*   2-layer transformer adaptor:
//                   linear1   (512 → 2048)
//                   ReLU
//                   linear2   (2048 → llm_dim=1024)
//                   blocks[2] (LayerNorm + plain MHA + bottleneck FFN
//                              1024 → 256 → 1024)
//   dec.*       Qwen3-0.6B LM (28 layers, hidden=1024, GQA 16/8,
//                              head_dim=128, NeoX RoPE @ freq_base=1e6,
//                              tied lm_head)
//
// Tensor naming mirrors scripts/convert-funasr_nano.py exactly.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::funasr_nano {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct FunAsrNanoHParams {
    // Encoder (SenseVoiceEncoderSmall, frozen).
    int32_t     enc_n_blocks   = 0;  // 50; includes the encoders0[0] projection block
    int32_t     enc_tp_blocks  = 0;  // 20
    int32_t     enc_d_model    = 0;  // 512
    int32_t     enc_d_input    = 0;  // 560 = n_mels (80) × lfr_m (7)
    int32_t     enc_n_heads    = 0;  // 4
    int32_t     enc_d_ff       = 0;  // 2048
    int32_t     enc_kernel     = 0;  // 11 (FSMN depthwise conv)
    int32_t     enc_sanm_shift = 0;  // 0 (left-pad shift for non-causal SAN-M)
    std::string enc_attn_type;       // "sanm"
    bool        enc_normalize_before = true;

    // Audio adaptor (2-layer transformer, frozen).
    int32_t     adaptor_n_blocks       = 0;      // 2
    int32_t     adaptor_encoder_dim    = 0;      // 512
    int32_t     adaptor_llm_dim        = 0;      // 1024
    int32_t     adaptor_pre_ffn_dim    = 0;      // 2048 (linear1 hidden, between encoder→llm projection)
    int32_t     adaptor_block_ffn_dim  = 0;      // 256  (per-block PositionwiseFeedForward hidden)
    int32_t     adaptor_n_heads        = 0;      // 8
    int32_t     adaptor_d_head         = 0;      // 128
    float       adaptor_layer_norm_eps = 1e-12f;
    std::string adaptor_activation;              // "relu"
    int32_t     adaptor_downsample_rate    = 0;  // 1
    bool        adaptor_use_low_frame_rate = true;

    // LLM decoder (Qwen3-0.6B, frozen).
    int32_t     dec_n_layers                = 0;     // 28
    int32_t     dec_hidden                  = 0;     // 1024
    int32_t     dec_intermediate            = 0;     // 3072
    int32_t     dec_n_heads                 = 0;     // 16
    int32_t     dec_n_kv_heads              = 0;     // 8
    int32_t     dec_head_dim                = 0;     // 128
    int32_t     dec_vocab_size              = 0;     // 151936
    int32_t     dec_max_position_embeddings = 0;     // 40960
    float       dec_rms_norm_eps            = 0.0f;  // ~1e-6
    float       dec_rope_theta              = 0.0f;  // 1e6
    bool        dec_tie_word_embeddings     = true;
    std::string dec_activation;                      // "silu"

    // Token IDs (resolved via tokenizer).
    int32_t bos_token_id = -1;
    int32_t eos_token_id = -1;
    int32_t pad_token_id = -1;
    int32_t vocab_size   = 0;

    // Frontend (WavFrontend; kaldi HTK fbank + LFR; no CMVN).
    std::string fe_type;             // "kaldi_fbank_lfr"
    int32_t     fe_num_mels    = 0;  // 80
    int32_t     fe_sample_rate = 0;  // 16000
    int32_t     fe_n_fft       = 0;  // 400
    int32_t     fe_win_length  = 0;  // 400
    int32_t     fe_hop_length  = 0;  // 160
    std::string fe_window;           // "hamming"
    std::string fe_normalize;        // "none"
    std::string fe_fbank_style;      // "kaldi_htk"
    float       fe_dither          = 0.0f;
    bool        fe_upscale_samples = true;
    bool        fe_snip_edges      = true;
    int32_t     fe_lfr_m           = 0;  // 7
    int32_t     fe_lfr_n           = 0;  // 6
    bool        fe_apply_cmvn      = false;
};

transcribe_status read_funasr_nano_hparams(const gguf_context * gguf, FunAsrNanoHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots — encoder (SAN-M)
// ---------------------------------------------------------------------------

// One SAN-M block. Same shape contract for every block (encoders0,
// encoders, tp_encoders) — only encoders0[0] differs in that its
// norm_attn and qkv input dim is d_input rather than d_model.
struct EncBlock {
    ggml_tensor * norm_attn_w = nullptr;  // [d_in]
    ggml_tensor * norm_attn_b = nullptr;  // [d_in]
    ggml_tensor * attn_qkv_w  = nullptr;  // [d_in, 3·d_model]   fused QKV
    ggml_tensor * attn_qkv_b  = nullptr;  // [3·d_model]
    ggml_tensor * attn_out_w  = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_out_b  = nullptr;  // [d_model]
    ggml_tensor * attn_fsmn_w = nullptr;  // [kernel, 1, d_model] depthwise conv1d
    ggml_tensor * norm_ffn_w  = nullptr;  // [d_model]
    ggml_tensor * norm_ffn_b  = nullptr;  // [d_model]
    ggml_tensor * ffn_fc1_w   = nullptr;  // [d_model, d_ff]
    ggml_tensor * ffn_fc1_b   = nullptr;  // [d_ff]
    ggml_tensor * ffn_fc2_w   = nullptr;  // [d_ff, d_model]
    ggml_tensor * ffn_fc2_b   = nullptr;  // [d_model]
};

// ---------------------------------------------------------------------------
// Weight slots — adaptor
// ---------------------------------------------------------------------------

// Adaptor block: pre-LN MHA (with bias) + pre-LN bottleneck FFN.
// LayerNorm eps = 1e-12 (matches adaptor.layer_norm_eps KV).
struct AdaptorBlock {
    // norm1
    ggml_tensor * norm_attn_w = nullptr;  // [llm_dim]
    ggml_tensor * norm_attn_b = nullptr;  // [llm_dim]
    // self-attention (separate q/k/v with biases — plain MultiHeadedAttention)
    ggml_tensor * attn_q_w    = nullptr;  // [llm_dim, llm_dim]
    ggml_tensor * attn_q_b    = nullptr;  // [llm_dim]
    ggml_tensor * attn_k_w    = nullptr;  // [llm_dim, llm_dim]
    ggml_tensor * attn_k_b    = nullptr;  // [llm_dim]
    ggml_tensor * attn_v_w    = nullptr;  // [llm_dim, llm_dim]
    ggml_tensor * attn_v_b    = nullptr;  // [llm_dim]
    ggml_tensor * attn_out_w  = nullptr;  // [llm_dim, llm_dim]
    ggml_tensor * attn_out_b  = nullptr;  // [llm_dim]
    // norm2
    ggml_tensor * norm_ffn_w  = nullptr;  // [llm_dim]
    ggml_tensor * norm_ffn_b  = nullptr;  // [llm_dim]
    // PositionwiseFeedForward: w_1 → ReLU → w_2 (with biases)
    ggml_tensor * ffn_fc1_w   = nullptr;  // [llm_dim, block_ffn_dim]
    ggml_tensor * ffn_fc1_b   = nullptr;  // [block_ffn_dim]
    ggml_tensor * ffn_fc2_w   = nullptr;  // [block_ffn_dim, llm_dim]
    ggml_tensor * ffn_fc2_b   = nullptr;  // [llm_dim]
};

struct AdaptorWeights {
    // First projection: encoder_dim*k → pre_ffn_dim → llm_dim. With
    // downsample_rate=1, k=1 so linear1 input is encoder_dim (=512).
    ggml_tensor * linear1_w = nullptr;  // [encoder_dim, pre_ffn_dim]
    ggml_tensor * linear1_b = nullptr;  // [pre_ffn_dim]
    ggml_tensor * linear2_w = nullptr;  // [pre_ffn_dim, llm_dim]
    ggml_tensor * linear2_b = nullptr;  // [llm_dim]

    std::vector<AdaptorBlock> blocks;
};

// ---------------------------------------------------------------------------
// Weight slots — decoder (Qwen3-0.6B)
// ---------------------------------------------------------------------------

struct DecEmbed {
    ggml_tensor * token_w = nullptr;  // [hidden, vocab] — tied to lm_head
};

struct DecBlock {
    ggml_tensor * norm_attn_w   = nullptr;  // input_layernorm (RMSNorm, no bias)
    ggml_tensor * norm_ffn_w    = nullptr;  // post_attention_layernorm
    ggml_tensor * attn_q_w      = nullptr;  // [hidden, n_heads * head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_v_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_o_w      = nullptr;  // [n_heads * head_dim, hidden]
    ggml_tensor * attn_q_norm   = nullptr;  // [head_dim]
    ggml_tensor * attn_k_norm   = nullptr;  // [head_dim]
    ggml_tensor * ffn_gate_w    = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_w      = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_down_w    = nullptr;  // [intermediate, hidden]
    // Packed gate+up [hidden, 2*intermediate], filled at load time.
    ggml_tensor * ffn_gate_up_w = nullptr;
};

struct DecFinal {
    ggml_tensor * norm_w = nullptr;
};

struct FunAsrNanoWeights {
    // Encoder.
    EncBlock              encoders0;  // single 560 → 512 projection block
    std::vector<EncBlock> encoders;   // n_blocks - 1 blocks at d_model
    ggml_tensor *         after_norm_w = nullptr;
    ggml_tensor *         after_norm_b = nullptr;
    std::vector<EncBlock> tp_encoders;
    ggml_tensor *         tp_norm_w = nullptr;
    ggml_tensor *         tp_norm_b = nullptr;

    // Adaptor.
    AdaptorWeights adaptor;

    // Decoder.
    DecEmbed              dec_embed;
    std::vector<DecBlock> dec_blocks;
    DecFinal              dec_final;
};

transcribe_status build_funasr_nano_weights(ggml_context *            ctx_meta,
                                            const FunAsrNanoHParams & hp,
                                            FunAsrNanoWeights &       weights);

}  // namespace transcribe::funasr_nano
