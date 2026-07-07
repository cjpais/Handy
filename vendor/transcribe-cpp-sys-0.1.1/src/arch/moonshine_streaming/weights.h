// arch/moonshine_streaming/weights.h - Moonshine-Streaming ASR tensor catalog.
//
// Differences from src/arch/moonshine/weights.h (the closest analog):
//
//   - Frontend.  No raw-PCM 3-conv stem. Instead a time-domain stack:
//       CMVN (parameter-free, eps=1e-6) → asinh(exp(log_k)·x)
//       → Linear(frame_len → hidden, no bias) + SiLU
//       → CausalConv1d(hidden → 2·hidden, k=5, s=2) + bias + SiLU
//       → CausalConv1d(2·hidden → hidden, k=5, s=2) + bias
//     Tensor slots: enc.embedder.{comp.log_k, linear.weight,
//                                  conv1.{weight,bias}, conv2.{weight,bias}}
//
//   - Encoder LayerNorms.  MoonshineStreamingLayerNorm uses unit_offset=True
//     (effective gain = γ + 1.0). The converter PRE-FOLDS the +1.0 into
//     the gamma tensor so C++ uses ordinary `LayerNorm(no affine) * scale`.
//
//   - Encoder attention.  No RoPE. Position information is encoded by
//     per-layer (left, right) sliding-window attention masks. The flat
//     [L0, R0, L1, R1, ...] array lives in
//     `stt.moonshine_streaming.encoder.sliding_windows`.
//
//   - Adapter.  A new namespace between encoder and decoder cross-attn:
//       adapter.pos_emb.weight  [max_pos, enc_hidden]   (always present)
//       adapter.proj.weight     [enc_hidden, dec_hidden] (only when
//                                                         enc_hidden != dec_hidden)
//
//   - Untied lm_head.  proj_out.weight is a SEPARATE tensor (mapped to
//     dec.lm_head.weight in the GGUF). Matrix multiplied against the
//     final decoder hidden to produce logits.
//
//   - Decoder LayerNorms.  Vanilla nn.LayerNorm(bias=False) — no
//     unit_offset trick.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::moonshine_streaming {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct MoonshineStreamingHParams {
    // Encoder.
    int32_t              enc_n_layers   = 0;
    int32_t              enc_d_model    = 0;
    int32_t              enc_n_heads    = 0;
    int32_t              enc_n_kv_heads = 0;  // streaming-tiny has no GQA: == n_heads
    int32_t              enc_head_dim   = 0;  // explicit head_dim in config
    int32_t              enc_ffn_dim    = 0;  // intermediate_size
    std::string          enc_activation;      // "gelu"
    float                enc_frame_ms  = 0.0f;
    int32_t              enc_frame_len = 0;   // = round(sample_rate * frame_ms / 1000)
    // Flattened per-layer (left, right) windows: 2 ints per layer.
    std::vector<int32_t> enc_sliding_windows;

    // Decoder.
    int32_t     dec_n_layers                = 0;
    int32_t     dec_d_model                 = 0;
    int32_t     dec_n_heads                 = 0;
    int32_t     dec_n_kv_heads              = 0;
    int32_t     dec_head_dim                = 0;
    int32_t     dec_ffn_dim                 = 0;
    int32_t     dec_max_position_embeddings = 0;
    int32_t     dec_vocab_size              = 0;
    std::string dec_activation;  // "silu"
    bool        dec_tie_word_embeddings = false;

    // Special tokens.
    int32_t bos_token_id           = -1;  // 1
    int32_t eos_token_id           = -1;  // 2
    int32_t pad_token_id           = -1;  // 0  (vs moonshine's 2)
    int32_t decoder_start_token_id = -1;  // 1

    // Attention / RoPE (decoder only — encoder has no RoPE).
    float   partial_rotary_factor = 0.8f;
    float   rope_theta            = 10000.0f;
    bool    attention_bias        = false;
    int32_t pad_head_dim_multiple = 0;

    // Frontend (raw waveform — converted to frames inside the encoder).
    std::string fe_type;             // "raw"
    int32_t     fe_sample_rate = 0;  // 16000

    // CMVN epsilon (fixed at 1e-6 in the reference).
    float cmvn_eps = 1e-6f;

    // Adapter.
    int32_t encoder_hidden_size = 0;  // mirrors enc_d_model; convenience read
    bool    adapter_has_proj    = false;

    // Capability flags.
    bool cap_lang_detect = false;
    bool cap_translate   = false;
    bool cap_timestamps  = false;
    bool cap_streaming   = true;

    // Derived helpers.
    //
    // Encoder attention "channel" dimension = n_heads * head_dim. Note this
    // is NOT necessarily equal to enc_d_model: small/medium have
    // enc_d_model=620/768 while enc_attn_dim=512/640. Q/K/V project from
    // enc_d_model to enc_attn_dim and the output projection takes
    // enc_attn_dim back to enc_d_model. Tiny coincidentally has both equal
    // (320 = 8 × 40).
    int32_t enc_attn_dim() const { return enc_n_heads * enc_head_dim; }

    int32_t enc_head_dim_padded() const {
        const int32_t hd = enc_head_dim;
        const int32_t m  = pad_head_dim_multiple;
        if (m <= 0 || hd <= 0) {
            return hd;
        }
        return ((hd + m - 1) / m) * m;
    }

    int32_t dec_head_dim_padded() const {
        const int32_t hd = dec_head_dim;
        const int32_t m  = pad_head_dim_multiple;
        if (m <= 0 || hd <= 0) {
            return hd;
        }
        return ((hd + m - 1) / m) * m;
    }

    // Decoder partial RoPE rotation width: 32 of 40 head_dim for tiny
    // (head_dim · 0.8 = 32). Mask off the odd bit so interleaved RoPE
    // halves match.
    int32_t dec_head_dim_rot() const {
        const int32_t hd = dec_head_dim;
        const int32_t r  = static_cast<int32_t>(static_cast<float>(hd) * partial_rotary_factor);
        return r & ~int32_t{ 1 };
    }
};

transcribe_status read_moonshine_streaming_hparams(const gguf_context * gguf, MoonshineStreamingHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots
// ---------------------------------------------------------------------------

// Encoder embedder: time-domain frontend living inside the encoder graph.
//
//   comp.log_k        [1]                  (1-D scalar; converter reshapes
//                                            from 0-D to [1])
//   linear.weight     [frame_len, hidden]  (no bias)
//   conv1.weight      [k=5, hidden, 2·hidden]
//   conv1.bias        [2·hidden]
//   conv2.weight      [k=5, 2·hidden, hidden]
//   conv2.bias        [hidden]
struct MoonshineStreamingEmbedder {
    ggml_tensor * comp_log_k = nullptr;  // [1] f32 — learned scalar
    ggml_tensor * linear_w   = nullptr;  // [frame_len, hidden] no bias
    ggml_tensor * conv1_w    = nullptr;  // [5, hidden, 2·hidden]
    ggml_tensor * conv1_b    = nullptr;  // [2·hidden]
    ggml_tensor * conv2_w    = nullptr;  // [5, 2·hidden, hidden]
    ggml_tensor * conv2_b    = nullptr;  // [hidden]
};

struct MoonshineStreamingEncTop {
    ggml_tensor * final_norm_w = nullptr;  // [hidden]  (pre-folded gain)
};

// One encoder transformer block. attention_bias=False on q/k/v/o.
// LayerNorm scales are pre-folded (gain = original γ + 1.0).
struct MoonshineStreamingEncBlock {
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;

    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * ffn_fc1_w  = nullptr;  // [hidden, ffn_dim]
    ggml_tensor * ffn_fc1_b  = nullptr;  // [ffn_dim]
    ggml_tensor * ffn_fc2_w  = nullptr;  // [ffn_dim, hidden]
    ggml_tensor * ffn_fc2_b  = nullptr;  // [hidden]
};

// Adapter: pos_emb add (+ proj when enc_hidden != dec_hidden).
struct MoonshineStreamingAdapter {
    ggml_tensor * pos_emb_w = nullptr;  // [max_pos, enc_hidden] always present
    ggml_tensor * proj_w    = nullptr;  // [enc_hidden, dec_hidden] optional
};

// Decoder top: untied embed + final LN + lm_head.
struct MoonshineStreamingDecTop {
    ggml_tensor * token_embd_w = nullptr;  // [hidden, vocab]
    ggml_tensor * final_norm_w = nullptr;  // [hidden] (no offset trick)
    ggml_tensor * lm_head_w    = nullptr;  // [hidden, vocab]  separate from token_embd
};

// One decoder block. q_proj/k_proj/v_proj from MoonshineStreamingAttention
// are bias-less (attention_bias=False). o_proj is bias-less by definition
// (line 603 of modeling). norm_self/norm_cross/norm_ffn are vanilla LN.
struct MoonshineStreamingDecBlock {
    // Self-attention.
    ggml_tensor * norm_self_w = nullptr;
    ggml_tensor * self_q_w    = nullptr;
    ggml_tensor * self_k_w    = nullptr;
    ggml_tensor * self_v_w    = nullptr;
    ggml_tensor * self_out_w  = nullptr;

    // Cross-attention (no RoPE).
    ggml_tensor * norm_cross_w = nullptr;
    ggml_tensor * cross_q_w    = nullptr;
    ggml_tensor * cross_k_w    = nullptr;
    ggml_tensor * cross_v_w    = nullptr;
    ggml_tensor * cross_out_w  = nullptr;

    // SwiGLU MLP (with biases).
    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * ffn_fc1_w  = nullptr;  // [hidden, 2·ffn_dim]
    ggml_tensor * ffn_fc1_b  = nullptr;  // [2·ffn_dim]
    ggml_tensor * ffn_fc2_w  = nullptr;  // [ffn_dim, hidden]
    ggml_tensor * ffn_fc2_b  = nullptr;  // [hidden]
};

struct MoonshineStreamingWeights {
    MoonshineStreamingEmbedder              embedder;
    MoonshineStreamingEncTop                enc_top;
    std::vector<MoonshineStreamingEncBlock> enc_blocks;
    MoonshineStreamingAdapter               adapter;
    MoonshineStreamingDecTop                dec_top;
    std::vector<MoonshineStreamingDecBlock> dec_blocks;
};

transcribe_status build_moonshine_streaming_weights(ggml_context *                    ctx_meta,
                                                    const MoonshineStreamingHParams & hp,
                                                    MoonshineStreamingWeights &       weights);

}  // namespace transcribe::moonshine_streaming
