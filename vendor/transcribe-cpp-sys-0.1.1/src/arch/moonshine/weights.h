// arch/moonshine/weights.h - canonical Moonshine ASR tensor catalog and
// per-instance weight slots.
//
// Architectural highlights (see `reports/porting/moonshine/forward-map.md`
// for the full forward pass):
//
//   - Encoder: 3-conv stem on raw 16 kHz PCM (Conv1: k=127 s=64 no-bias
//     → tanh → GroupNorm(num_groups=1); Conv2: k=7 s=3 → GELU; Conv3:
//     k=3 s=2 → GELU), then n_layers pre-LN transformer blocks (MHSA
//     with partial RoPE 0.9 on Q/K + GELU MLP), then a final LN.
//
//   - Decoder: token embedding (tied to lm_head), n_layers pre-LN blocks
//     (self-attn with partial RoPE 0.9 on Q/K, cross-attn with NO RoPE,
//     SwiGLU MLP fc1 hidden→2·intermediate split-gate-silu), final LN,
//     tied logits head.
//
//   - Attention biases: ALL projections (q/k/v/o on both self- and
//     cross-attn) are bias-less (`config.attention_bias=false`). Only
//     MLP fc1/fc2 carry bias.
//
//   - LayerNorm biases: every `nn.LayerNorm` in modeling_moonshine.py
//     is constructed with `bias=False`. The catalog has no `_b` slot
//     for any norm.
//
//   - Logits head: tied to `dec.token_embd.weight` (no separate head
//     tensor in the safetensors).
//
//   - Frontend buffers: NONE — moonshine has no mel/STFT. The conv
//     stem operates directly on raw PCM.
//
// Tensor naming follows `scripts/convert-moonshine.py`:
//
//   Encoder top:   enc.conv.{0,1,2}.{weight,bias}, enc.conv.norm.{w,b},
//                  enc.final_norm.weight  (no bias)
//   Encoder block: enc.blocks.{i}.norm_attn.weight  (no bias)
//                  enc.blocks.{i}.attn.{q,k,v,out}.weight  (no biases)
//                  enc.blocks.{i}.norm_ffn.weight   (no bias)
//                  enc.blocks.{i}.ffn.fc{1,2}.{weight,bias}
//   Decoder top:   dec.token_embd.weight  (tied to lm_head),
//                  dec.final_norm.weight  (no bias)
//   Decoder block: dec.blocks.{i}.norm_self.weight   (no bias)
//                  dec.blocks.{i}.self_attn.{q,k,v,out}.weight  (no biases)
//                  dec.blocks.{i}.norm_cross.weight  (no bias)
//                  dec.blocks.{i}.cross_attn.{q,k,v,out}.weight  (no biases)
//                  dec.blocks.{i}.norm_ffn.weight    (no bias)
//                  dec.blocks.{i}.ffn.fc1.{weight,bias}  (fc1 emits 2·inter)
//                  dec.blocks.{i}.ffn.fc2.{weight,bias}

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::moonshine {

// ---------------------------------------------------------------------------
// Hyperparameters
// ---------------------------------------------------------------------------

struct MoonshineHParams {
    // Encoder.
    int32_t     enc_n_layers   = 0;
    int32_t     enc_d_model    = 0;
    int32_t     enc_n_heads    = 0;
    int32_t     enc_n_kv_heads = 0;  // moonshine has no GQA on tiny: == n_heads
    int32_t     enc_ffn_dim    = 0;  // intermediate_size
    std::string enc_activation;      // "gelu"

    // Decoder.
    int32_t     dec_n_layers                = 0;
    int32_t     dec_d_model                 = 0;  // == enc_d_model on every shipped variant
    int32_t     dec_n_heads                 = 0;
    int32_t     dec_n_kv_heads              = 0;
    int32_t     dec_ffn_dim                 = 0;  // intermediate_size (fc1 emits 2·this)
    int32_t     dec_max_position_embeddings = 0;  // 194 for tiny; both target
                                                  // length cap and rope max-seq
    int32_t     dec_vocab_size              = 0;
    std::string dec_activation;                   // "silu"
    bool        dec_tie_word_embeddings = true;

    // Special tokens.
    int32_t bos_token_id           = -1;  // 1, decoder_start
    int32_t eos_token_id           = -1;  // 2
    int32_t pad_token_id           = -1;  // 2 (== eos)
    int32_t decoder_start_token_id = -1;  // 1

    // Attention / RoPE.
    float   partial_rotary_factor = 0.9f;
    float   rope_theta            = 10000.0f;
    bool    attention_bias        = false;
    int32_t pad_head_dim_multiple = 0;  // 8 for tiny; 0 means "do not pad"

    // Conv stem (3-layer raw-PCM frontend).
    std::vector<int32_t> conv_channels;      // [enc_d_model, 2·enc_d_model, enc_d_model]
    std::vector<int32_t> conv_kernel_sizes;  // [127, 7, 3]
    std::vector<int32_t> conv_strides;       // [64, 3, 2]
    int32_t              conv_groupnorm_num_groups = 1;
    float                conv_groupnorm_eps        = 1e-5f;

    // Frontend (raw waveform — no mel).
    std::string fe_type;             // "raw"
    int32_t     fe_sample_rate = 0;  // 16000

    // Capability flags read from stt.capability.*.
    bool cap_lang_detect = false;
    bool cap_translate   = false;
    bool cap_timestamps  = false;

    // Derived helpers.
    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    int32_t dec_head_dim() const { return dec_n_heads > 0 ? dec_d_model / dec_n_heads : 0; }

    // Padded head dim used inside attention. HF Moonshine pads each
    // head's q/k/v to a multiple of `pad_head_dim_multiple`, runs the
    // matmul / FA, then slices the padding off after `o_proj`. For
    // tiny: head_dim=36 → padded=40 (pad=4). For variants where
    // head_dim is already a multiple of `pad_head_dim_multiple` the
    // padding is zero and the path is a no-op.
    int32_t enc_head_dim_padded() const {
        const int32_t hd = enc_head_dim();
        const int32_t m  = pad_head_dim_multiple;
        if (m <= 0 || hd <= 0) {
            return hd;
        }
        return ((hd + m - 1) / m) * m;
    }

    int32_t dec_head_dim_padded() const {
        const int32_t hd = dec_head_dim();
        const int32_t m  = pad_head_dim_multiple;
        if (m <= 0 || hd <= 0) {
            return hd;
        }
        return ((hd + m - 1) / m) * m;
    }

    // Number of leading head-dim positions rotated by partial RoPE.
    // HF rounds `int(head_dim · factor)` down to an even number so
    // the NEOX rotate_half halves match. For tiny: 36 · 0.9 = 32.4 →
    // floor 32, already even. The C++ port uses the floor-and-mask-odd
    // formula (`(int(hd*f)) & ~1`) for forward compatibility with
    // base / future variants.
    int32_t enc_head_dim_rot() const {
        const int32_t hd = enc_head_dim();
        const int32_t r  = static_cast<int32_t>(static_cast<float>(hd) * partial_rotary_factor);
        return r & ~int32_t{ 1 };
    }

    int32_t dec_head_dim_rot() const {
        const int32_t hd = dec_head_dim();
        const int32_t r  = static_cast<int32_t>(static_cast<float>(hd) * partial_rotary_factor);
        return r & ~int32_t{ 1 };
    }
};

transcribe_status read_moonshine_hparams(const gguf_context * gguf, MoonshineHParams & hp);

// ---------------------------------------------------------------------------
// Weight slots
// ---------------------------------------------------------------------------

// Encoder conv stem: 3 Conv1d layers + GroupNorm right after conv0+tanh.
struct MoonshineEncStem {
    ggml_tensor * conv0_w = nullptr;  // [127, 1,         d_model],   no bias
    ggml_tensor * conv1_w = nullptr;  // [7,   d_model,   2·d_model]
    ggml_tensor * conv1_b = nullptr;  // [2·d_model]
    ggml_tensor * conv2_w = nullptr;  // [3,   2·d_model, d_model]
    ggml_tensor * conv2_b = nullptr;  // [d_model]
    ggml_tensor * gn_w    = nullptr;  // [d_model]   (num_groups=1, ε=1e-5)
    ggml_tensor * gn_b    = nullptr;  // [d_model]
};

struct MoonshineEncTop {
    ggml_tensor * final_norm_w = nullptr;  // [d_model]   (no bias)
};

// One encoder transformer block. attention_bias=false: every q/k/v/o is
// weight-only. norm_* have no bias.
struct MoonshineEncBlock {
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;

    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * ffn_fc1_w  = nullptr;  // [d_model, ffn_dim]
    ggml_tensor * ffn_fc1_b  = nullptr;  // [ffn_dim]
    ggml_tensor * ffn_fc2_w  = nullptr;  // [ffn_dim, d_model]
    ggml_tensor * ffn_fc2_b  = nullptr;  // [d_model]
};

// Decoder top: tied token embed + final LN.
struct MoonshineDecTop {
    ggml_tensor * token_embd_w = nullptr;  // [d_model, vocab_size]
    ggml_tensor * final_norm_w = nullptr;  // [d_model]   (no bias)
};

// One decoder transformer block. attention_bias=false on both self- and
// cross-attn. Decoder MLP is SwiGLU: fc1 emits 2·ffn_dim, fc2 takes ffn_dim.
struct MoonshineDecBlock {
    // Self-attention (causal, partial RoPE on q/k).
    ggml_tensor * norm_self_w = nullptr;
    ggml_tensor * self_q_w    = nullptr;
    ggml_tensor * self_k_w    = nullptr;
    ggml_tensor * self_v_w    = nullptr;
    ggml_tensor * self_out_w  = nullptr;

    // Cross-attention (queries decoder state against encoder; no RoPE).
    ggml_tensor * norm_cross_w = nullptr;
    ggml_tensor * cross_q_w    = nullptr;
    ggml_tensor * cross_k_w    = nullptr;
    ggml_tensor * cross_v_w    = nullptr;
    ggml_tensor * cross_out_w  = nullptr;

    // SwiGLU MLP.
    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * ffn_fc1_w  = nullptr;  // [d_model, 2·ffn_dim]
    ggml_tensor * ffn_fc1_b  = nullptr;  // [2·ffn_dim]
    ggml_tensor * ffn_fc2_w  = nullptr;  // [ffn_dim, d_model]
    ggml_tensor * ffn_fc2_b  = nullptr;  // [d_model]
};

struct MoonshineWeights {
    MoonshineEncStem               enc_stem;
    MoonshineEncTop                enc_top;
    std::vector<MoonshineEncBlock> enc_blocks;
    MoonshineDecTop                dec_top;
    std::vector<MoonshineDecBlock> dec_blocks;
};

transcribe_status build_moonshine_weights(ggml_context *           ctx_meta,
                                          const MoonshineHParams & hp,
                                          MoonshineWeights &       weights);

}  // namespace transcribe::moonshine
