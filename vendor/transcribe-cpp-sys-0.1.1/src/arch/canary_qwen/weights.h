// arch/canary_qwen/weights.h - Canary-Qwen tensor catalog and hparams.
//
// INTERNAL to src/arch/canary_qwen/. Defines:
//
//   - CanaryQwenHParams: KV read from stt.canary_qwen.* and stt.frontend.*
//     before any tensors are allocated.
//
//   - CanaryQwenWeights: borrowed ggml_tensor* slots for every logical
//     weight. Tensors live in the model's ctx_meta / backend buffer.
//
// Architecture: audio-LLM.
//
//   enc.*               FastConformer encoder (32 blocks, byte-for-byte
//                       identical to canary-1b-flash perception encoder
//                       per SALM cfg). Every Linear/Conv1d carries a bias
//                       (use_bias=true, untie_biases=true).
//   enc.proj.*          Perception projection nn.Linear(1024, 2048) + bias.
//   dec.token_embd.*    Tied embedding/lm_head (Qwen3-1.7B vocab=151936,
//                       hidden=2048).
//   dec.blocks[i].*     Qwen3-1.7B decoder layer (28 layers; no biases on
//                       Q/K/V/O or MLP; per-head q_norm/k_norm on head_dim).
//   dec.output_norm.*   Final RMSNorm.
//
// Tensor naming mirrors scripts/convert-canary-qwen.py exactly.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::canary_qwen {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct CanaryQwenHParams {
    // Encoder (FastConformer; matches canary-1b-flash).
    int32_t enc_n_layers           = 0;  // 32
    int32_t enc_d_model            = 0;  // 1024
    int32_t enc_n_heads            = 0;  // 8 (-> head_dim = 128)
    int32_t enc_d_ff               = 0;  // 4096
    int32_t enc_conv_kernel        = 0;  // 9
    int32_t enc_subsampling_factor = 0;  // 8
    int32_t enc_subsampling_chans  = 0;  // 256
    int32_t enc_pos_emb_max_len    = 0;  // 5000

    // Perception projection.
    int32_t perception_output_dim = 0;  // 2048 (must equal dec_hidden)

    // Audio locator placeholder id (single Qwen2 BPE special token).
    int32_t audio_locator_id = -1;  // 151669 = "<|audioplaceholder|>"

    // Decoder (Qwen3-1.7B).
    int32_t dec_n_layers            = 0;     // 28
    int32_t dec_hidden              = 0;     // 2048
    int32_t dec_intermediate        = 0;     // 6144
    int32_t dec_n_heads             = 0;     // 16
    int32_t dec_n_kv_heads          = 0;     // 8
    int32_t dec_head_dim            = 0;     // 128
    int32_t dec_max_position        = 0;     // 40960
    int32_t dec_vocab_size          = 0;     // 151936
    float   dec_rms_norm_eps        = 0.0f;  // 1e-6
    float   dec_rope_theta          = 0.0f;  // 1e6
    bool    dec_tie_word_embeddings = true;

    // Tokenizer-derived.
    int32_t vocab_size   = 0;   // matches dec_vocab_size
    int32_t bos_token_id = -1;
    int32_t eos_token_id = -1;  // 151645 = "<|im_end|>"

    // Frontend (NeMo AudioToMelSpectrogramPreprocessor cfg).
    int32_t     fe_sample_rate  = 0;     // 16000
    int32_t     fe_num_mels     = 0;     // 128
    int32_t     fe_n_fft        = 0;     // 512
    int32_t     fe_win_length   = 0;     // 400
    int32_t     fe_hop_length   = 0;     // 160
    float       fe_pre_emphasis = 0.0f;  // 0.97
    float       fe_f_min        = 0.0f;  // 0.0
    float       fe_f_max        = 0.0f;  // 8000.0
    std::string fe_pad_mode;             // "reflect"
    std::string fe_normalize;            // "per_feature"
};

// ---------------------------------------------------------------------------
// Weight slots (borrowed pointers; tensors live in model.ctx_meta /
// backend buffer).
// ---------------------------------------------------------------------------

struct PreEncodeSlots {
    // ConvSubsampling dw_striding (factor 8, channels 256):
    //   conv2d(1->256, 3x3 stride 2)   -> ReLU
    //   conv2d(1->256, 3x3 stride 2)   -> ReLU   (2nd 2D conv treated as 1ch -> 256ch)
    //   conv2d(256->256, 1x1)
    //   conv2d(1->256, 3x3 stride 2)   -> ReLU
    //   conv2d(256->256, 1x1)
    //   linear(reduced_feat_in -> d_model=1024) + bias
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

struct EncBlockSlots {
    // Macaron FF1
    ggml_tensor * norm_ff1_w = nullptr;
    ggml_tensor * norm_ff1_b = nullptr;
    ggml_tensor * ff1_lin1_w = nullptr;
    ggml_tensor * ff1_lin1_b = nullptr;
    ggml_tensor * ff1_lin2_w = nullptr;
    ggml_tensor * ff1_lin2_b = nullptr;

    // Self-attention with relative position
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
    ggml_tensor * attn_pos_w  = nullptr;  // linear_pos has no bias
    ggml_tensor * attn_pos_u  = nullptr;  // (n_heads, head_dim)
    ggml_tensor * attn_pos_v  = nullptr;  // (n_heads, head_dim)

    // Convolution module: pw1 -> GLU -> dw -> BN -> SiLU -> pw2
    ggml_tensor * norm_conv_w = nullptr;
    ggml_tensor * norm_conv_b = nullptr;
    ggml_tensor * conv_pw1_w  = nullptr;
    ggml_tensor * conv_pw1_b  = nullptr;
    ggml_tensor * conv_dw_w   = nullptr;
    ggml_tensor * conv_dw_b   = nullptr;
    ggml_tensor * conv_pw2_w  = nullptr;
    ggml_tensor * conv_pw2_b  = nullptr;

    // BatchNorm (raw weights from GGUF).
    ggml_tensor * conv_bn_w  = nullptr;
    ggml_tensor * conv_bn_b  = nullptr;
    ggml_tensor * conv_bn_rm = nullptr;
    ggml_tensor * conv_bn_rv = nullptr;

    // BN fused at load time into (scale, bias). Owned by m.bn_fused_ctx.
    ggml_tensor * conv_bn_fused_scale = nullptr;
    ggml_tensor * conv_bn_fused_bias  = nullptr;

    // Macaron FF2
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_lin1_w = nullptr;
    ggml_tensor * ff2_lin1_b = nullptr;
    ggml_tensor * ff2_lin2_w = nullptr;
    ggml_tensor * ff2_lin2_b = nullptr;

    // Final per-block LayerNorm
    ggml_tensor * norm_out_w = nullptr;
    ggml_tensor * norm_out_b = nullptr;
};

struct PerceptionProjSlots {
    ggml_tensor * weight = nullptr;  // (in=1024, out=2048) row-major as
                                     // stored by ggml: nb0=stride along
                                     // in-dim. ggml_mul_mat(W, x) where
                                     // x has shape (in, T) yields (out, T).
    ggml_tensor * bias   = nullptr;  // (2048,)
};

struct DecEmbedSlots {
    ggml_tensor * token_w = nullptr;  // dec.token_embd.weight (vocab, hidden)
                                      // Tied to lm_head; reused for both
                                      // ggml_get_rows and final mul_mat.
};

struct DecBlockSlots {
    // RMSNorms (no bias on Qwen3)
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;

    // Self-attention (no biases)
    ggml_tensor * attn_q_w = nullptr;
    ggml_tensor * attn_k_w = nullptr;
    ggml_tensor * attn_v_w = nullptr;
    ggml_tensor * attn_o_w = nullptr;

    // Per-head Q/K RMSNorm (acts on head_dim)
    ggml_tensor * attn_q_norm = nullptr;  // (head_dim,)
    ggml_tensor * attn_k_norm = nullptr;  // (head_dim,)

    // SwiGLU MLP — packed gate+up at load time
    ggml_tensor * ffn_gate_w    = nullptr;
    ggml_tensor * ffn_up_w      = nullptr;
    ggml_tensor * ffn_gate_up_w = nullptr;  // packed [hidden, 2*intermediate]
    ggml_tensor * ffn_down_w    = nullptr;
};

struct DecFinalSlots {
    ggml_tensor * norm_w = nullptr;  // dec.output_norm.weight
};

struct CanaryQwenWeights {
    PreEncodeSlots             pre_encode;
    std::vector<EncBlockSlots> blocks;  // size = enc_n_layers
    PerceptionProjSlots        perception_proj;

    DecEmbedSlots              dec_embed;
    std::vector<DecBlockSlots> dec_blocks;  // size = dec_n_layers
    DecFinalSlots              dec_final;
};

// ---------------------------------------------------------------------------
// Loader entry points
// ---------------------------------------------------------------------------

transcribe_status read_canary_qwen_hparams(gguf_context * gguf_ctx, CanaryQwenHParams & out);

transcribe_status build_canary_qwen_weights(ggml_context * ctx, const CanaryQwenHParams & hp, CanaryQwenWeights & out);

}  // namespace transcribe::canary_qwen
