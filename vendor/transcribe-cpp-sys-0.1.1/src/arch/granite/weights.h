// arch/granite/weights.h - Granite Speech tensor catalog and hparams.
//
// INTERNAL to src/arch/granite/. Defines:
//
//   - GraniteHParams: architecture KV that drives tensor shapes. Read
//     from stt.granite.* and stt.frontend.* at load time, before any
//     tensors are allocated.
//
//   - GraniteWeights: named ggml_tensor* slots, one per logical
//     weight. Borrowed pointers — the tensors themselves live in the
//     model's ctx_meta / backend buffer.
//
// Architecture pattern: audio-llm. Three-component weight layout
// (mirrors the HF state-dict prefix scheme that the converter emits):
//
//   enc.*   Conformer encoder (16 layers, hidden=1024, Shaw block-local
//           attn over context_size=200 blocks, GLU conv with BatchNorm,
//           self-conditioned CTC bypass via out_mid at layer N/2)
//   proj.*  BLIP-2 Q-Former projector (3 learned queries, 2 layers of
//           self+cross attention + FFN, final LN, linear lift to
//           LM hidden)
//   dec.*   Granite-4 causal LM (40 layers, hidden=2048, GQA 16/4,
//           RMSNorm, SwiGLU, 4 scalar multipliers baked into the graph)
//
// Tensor naming mirrors the converter's output. Linear weights use
// PyTorch [out, in] order (i.e. ggml ne[0]=in, ne[1]=out). Conv1d
// kernels use PyTorch (out, in/groups, k) order (ggml ne[0]=k,
// ne[1]=in/groups, ne[2]=out).

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::granite {

// Hyperparameters

struct GraniteHParams {
    // Identity. `stt.variant` distinguishes models that share the same
    // architecture and hparams but differ in their published evaluation
    // prompt (e.g. granite-4.0-1b-speech, granite-speech-4.1-2b,
    // granite-speech-4.1-2b-plus). Used in model.cpp to pick the
    // per-variant instruction + optional system prompt that matches the
    // model card. Empty string falls back to the historical default.
    std::string variant;

    // Encoder (Conformer, granite_speech_encoder).
    int32_t enc_n_layers         = 0;
    int32_t enc_hidden           = 0;  // 1024
    int32_t enc_n_heads          = 0;  // 8
    int32_t enc_head_dim         = 0;  // 128
    int32_t enc_input_dim        = 0;  // 160 (= 2 mel frames stacked)
    int32_t enc_output_dim       = 0;  // 348 (CTC vocabulary head)
    int32_t enc_feedforward_mult = 0;  // 4 (FFN inner = hidden*mult)
    int32_t enc_conv_kernel_size = 0;  // 15
    int32_t enc_conv_expansion   = 0;  // 2 (conv inner = hidden*expansion)
    int32_t enc_max_pos_emb      = 0;  // 512
    int32_t enc_context_size     = 0;  // 200 (block size for local attn)

    // cat_hidden_layers (uint32 array, possibly empty). For -plus this
    // is [3]: the encoder concatenates block[3].out with the final
    // block hidden along the channel axis, doubling the projector's
    // cross-attention K/V input from 1024 to 2048. Empty list means
    // no concat (projector_input == hidden).
    std::vector<int32_t> enc_cat_hidden_layers;

    // Projector (BLIP-2 Q-Former).
    int32_t     prj_n_layers            = 0;  // 2
    int32_t     prj_hidden              = 0;  // 1024
    int32_t     prj_intermediate        = 0;  // 4096
    int32_t     prj_n_heads             = 0;  // 16
    int32_t     prj_encoder_hidden_size = 0;  // 1024 or 2048 (varies on -plus)
    int32_t     prj_cross_attn_freq     = 0;  // 1 (every layer has cross-attn)
    std::string prj_hidden_act;               // "gelu"
    float       prj_layer_norm_eps = 0.0f;
    int32_t     prj_max_pos_emb    = 0;       // 2048 (Q-Former absolute pos emb)
    std::string prj_pos_embed_type;           // "absolute"

    // Text LM (Granite-4 causal LM).
    int32_t     dec_n_layers     = 0;  // 40
    int32_t     dec_hidden       = 0;  // 2048
    int32_t     dec_intermediate = 0;  // 4096
    int32_t     dec_n_heads      = 0;  // 16
    int32_t     dec_n_kv_heads   = 0;  // 4 (GQA 16/4)
    int32_t     dec_head_dim     = 0;  // 128
    std::string dec_hidden_act;        // "silu"
    float       dec_rms_norm_eps            = 0.0f;
    float       dec_rope_theta              = 0.0f;
    int32_t     dec_max_position_embeddings = 0;      // 4096
    bool        dec_tie_word_embeddings     = false;  // true on -plus only
    int32_t     dec_vocab_size              = 0;      // 100353

    // Granite-4 scalar multipliers. Each one is silently load-bearing —
    // missing any of them degrades accuracy without crashing.
    float dec_embedding_multiplier = 0.0f;  // 12.0
    float dec_logits_scaling       = 0.0f;  //  8.0  (logits /= this)
    float dec_attention_multiplier = 0.0f;  //  1/128 = 0.0078125 (softmax scale)
    float dec_residual_multiplier  = 0.0f;  //  0.22 (every residual add)

    // Audio fusion.
    int32_t audio_token_id  = 0;  // 100352
    int32_t downsample_rate = 0;  // 5
    int32_t window_size     = 0;  // 15 (encoder frames per Q-Former window)

    // Resolved token ids (filled from tokenizer KV at load).
    int32_t bos_token_id = -1;  // 100257
    int32_t eos_token_id = -1;  // 100257 (same as bos)
    int32_t pad_token_id = -1;  // 100256
    int32_t vocab_size   = 0;   // 100353

    // Frontend (torchaudio MelSpectrogram). htk-norm mel, no log, no
    // per-utterance norm, power=2 spectrogram.
    std::string fe_type;             // "mel"
    int32_t     fe_num_mels    = 0;  // 80
    int32_t     fe_sample_rate = 0;  // 16000
    int32_t     fe_n_fft       = 0;  // 512
    int32_t     fe_win_length  = 0;  // 400
    int32_t     fe_hop_length  = 0;  // 160
    std::string fe_window;           // "hann_periodic"
    std::string fe_normalize;        // "none"
    float       fe_dither       = 0.0f;
    float       fe_pre_emphasis = 0.0f;
    float       fe_f_min        = 0.0f;
    float       fe_f_max        = 0.0f;
    std::string fe_pad_mode;  // "reflect"
    bool        fe_center = true;
    std::string fe_mel_norm;  // "htk"
};

transcribe_status read_granite_hparams(const gguf_context * gguf, GraniteHParams & hp);

// Weight slots - encoder (Conformer)

// Top-level encoder tensors: input linear (160 -> 1024) and the
// self-conditioned CTC bypass pair (ctc_proj: 1024 -> 348 for the
// auxiliary CTC head; ctc_bypass: 348 -> 1024 to add the softmaxed CTC
// logits back into the residual stream at layer N/2).
struct GraniteEncTop {
    ggml_tensor * input_linear_w = nullptr;  // [input_dim,  hidden]
    ggml_tensor * input_linear_b = nullptr;  // [hidden]
    ggml_tensor * ctc_proj_w     = nullptr;  // [hidden,     output_dim]
    ggml_tensor * ctc_proj_b     = nullptr;  // [output_dim]
    ggml_tensor * ctc_bypass_w   = nullptr;  // [output_dim, hidden]
    ggml_tensor * ctc_bypass_b   = nullptr;  // [hidden]
};

// One Conformer block. Macaron FFN (ff1, ff2) flank Shaw block-local
// self-attention and a GLU conv module with BatchNorm. Final per-block
// post-LN. attn.q is separate; attn.kv is fused (2*inner_dim output);
// attn.rel_pos_emb is a learned table for Shaw-style positional bias.
struct GraniteEncBlock {
    // FF1 (first half macaron). LN gamma/beta + up/down projections,
    // both with biases. SiLU between.
    ggml_tensor * norm_ff1_w = nullptr;  // [hidden]
    ggml_tensor * norm_ff1_b = nullptr;
    ggml_tensor * ff1_up_w   = nullptr;  // [hidden, hidden*ffn_mult]
    ggml_tensor * ff1_up_b   = nullptr;  // [hidden*ffn_mult]
    ggml_tensor * ff1_down_w = nullptr;  // [hidden*ffn_mult, hidden]
    ggml_tensor * ff1_down_b = nullptr;  // [hidden]

    // Block-local Shaw self-attention.
    ggml_tensor * norm_attn_w      = nullptr;  // [hidden]
    ggml_tensor * norm_attn_b      = nullptr;
    ggml_tensor * attn_q_w         = nullptr;  // [hidden, inner_dim]
    ggml_tensor * attn_kv_w        = nullptr;  // [hidden, 2*inner_dim]  (fused K|V)
    ggml_tensor * attn_out_w       = nullptr;  // [inner_dim, hidden]
    ggml_tensor * attn_out_b       = nullptr;  // [hidden]
    // Shaw relative-position table: [head_dim, 2*max_pos_emb+1].
    // Looked up by clamped (k - q + max_pos_emb) and added to QK^T.
    ggml_tensor * attn_rel_pos_emb = nullptr;  // [head_dim, 2*max_pos_emb+1]

    // Conv module (see granite_conv_module in encoder.cpp).
    ggml_tensor * norm_conv_w         = nullptr;  // [hidden]
    ggml_tensor * norm_conv_b         = nullptr;
    ggml_tensor * conv_pointwise1_w   = nullptr;  // [1, hidden, 2*inner_dim]
    ggml_tensor * conv_pointwise1_b   = nullptr;  // [2*inner_dim]
    ggml_tensor * conv_depthwise_w    = nullptr;  // [k, 1, inner_dim]
    ggml_tensor * conv_bn_w           = nullptr;  // [inner_dim]  (gamma)
    ggml_tensor * conv_bn_b           = nullptr;  // [inner_dim]  (beta)
    ggml_tensor * conv_bn_mean        = nullptr;  // [inner_dim]  (running mean)
    ggml_tensor * conv_bn_var         = nullptr;  // [inner_dim]  (running var)
    ggml_tensor * conv_pointwise2_w   = nullptr;  // [1, inner_dim, hidden]
    ggml_tensor * conv_pointwise2_b   = nullptr;  // [hidden]
    // Fused BatchNorm scale/bias precomputed at load (model.cpp
    // fuse_batch_norm); the raw tensors above only feed the fusion step.
    ggml_tensor * conv_bn_fused_scale = nullptr;  // [inner_dim]
    ggml_tensor * conv_bn_fused_bias  = nullptr;  // [inner_dim]

    // FF2 (second half macaron).
    ggml_tensor * norm_ff2_w = nullptr;
    ggml_tensor * norm_ff2_b = nullptr;
    ggml_tensor * ff2_up_w   = nullptr;
    ggml_tensor * ff2_up_b   = nullptr;
    ggml_tensor * ff2_down_w = nullptr;
    ggml_tensor * ff2_down_b = nullptr;

    // Post-block LayerNorm.
    ggml_tensor * norm_post_w = nullptr;
    ggml_tensor * norm_post_b = nullptr;
};

// Weight slots - projector (BLIP-2 Q-Former)

// Top-level projector tensors.
//   query: [hidden, num_queries=3, 1] — learned queries broadcast over
//          per-window encoder slices.
//   linear: lifts Q-Former hidden (1024) to LM hidden (2048).
//   qformer.final_norm: despite the name (a converter-side misnomer),
//          this is Blip2QFormerModel.layernorm — the Q-Former INPUT LN,
//          applied to the queries BEFORE the layer stack, not after it.
//          See projector.cpp (qformer_layer_norm at graph entry).
struct GraniteProjTop {
    ggml_tensor * query                = nullptr;  // [hidden, num_queries, 1]
    ggml_tensor * linear_w             = nullptr;  // [hidden, lm_hidden]
    ggml_tensor * linear_b             = nullptr;  // [lm_hidden]
    ggml_tensor * qformer_final_norm_w = nullptr;  // [hidden]
    ggml_tensor * qformer_final_norm_b = nullptr;
};

// One Q-Former layer: self-attn -> LN -> cross-attn -> LN -> FFN -> LN.
// All linears carry bias. The cross-attention K/V input dim is
// `encoder_hidden_size`, which is 1024 for the 1b/2b variants and 2048
// for the -plus variant.
struct GraniteProjBlock {
    // Self-attention (Q/K/V all hidden -> hidden).
    ggml_tensor * self_attn_q_w    = nullptr;
    ggml_tensor * self_attn_q_b    = nullptr;
    ggml_tensor * self_attn_k_w    = nullptr;
    ggml_tensor * self_attn_k_b    = nullptr;
    ggml_tensor * self_attn_v_w    = nullptr;
    ggml_tensor * self_attn_v_b    = nullptr;
    ggml_tensor * self_attn_out_w  = nullptr;
    ggml_tensor * self_attn_out_b  = nullptr;
    ggml_tensor * norm_self_attn_w = nullptr;
    ggml_tensor * norm_self_attn_b = nullptr;

    // Cross-attention. Q from query stream; K/V from encoder slice.
    ggml_tensor * cross_attn_q_w    = nullptr;  // [prj_hidden,        prj_hidden]
    ggml_tensor * cross_attn_q_b    = nullptr;
    ggml_tensor * cross_attn_k_w    = nullptr;  // [encoder_hidden,    prj_hidden]
    ggml_tensor * cross_attn_k_b    = nullptr;
    ggml_tensor * cross_attn_v_w    = nullptr;  // [encoder_hidden,    prj_hidden]
    ggml_tensor * cross_attn_v_b    = nullptr;
    ggml_tensor * cross_attn_out_w  = nullptr;  // [prj_hidden,        prj_hidden]
    ggml_tensor * cross_attn_out_b  = nullptr;
    ggml_tensor * norm_cross_attn_w = nullptr;
    ggml_tensor * norm_cross_attn_b = nullptr;

    // FFN (intermediate_query / output_query collapse).
    ggml_tensor * ffn_up_w   = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_b   = nullptr;
    ggml_tensor * ffn_down_w = nullptr;  // [intermediate, hidden]
    ggml_tensor * ffn_down_b = nullptr;
    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * norm_ffn_b = nullptr;
};

// Weight slots - Granite-4 LM

struct GraniteDecEmbed {
    ggml_tensor * token_w = nullptr;  // [hidden, vocab]
};

struct GraniteDecBlock {
    ggml_tensor * norm_attn_w   = nullptr;  // input_layernorm (RMSNorm, no bias)
    ggml_tensor * norm_ffn_w    = nullptr;  // post_attention_layernorm
    // GQA projections; no biases on Granite-4.
    ggml_tensor * attn_q_w      = nullptr;  // [hidden, n_heads * head_dim]
    ggml_tensor * attn_k_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_v_w      = nullptr;  // [hidden, n_kv_heads * head_dim]
    ggml_tensor * attn_o_w      = nullptr;  // [n_heads * head_dim, hidden]
    // SwiGLU MLP.
    ggml_tensor * ffn_gate_w    = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_up_w      = nullptr;  // [hidden, intermediate]
    ggml_tensor * ffn_down_w    = nullptr;  // [intermediate, hidden]
    // Optional gate/up pack: [hidden, 2*intermediate]. Allocated by
    // causal_lm::pack_gate_up at load time.
    ggml_tensor * ffn_gate_up_w = nullptr;
};

struct GraniteDecFinal {
    ggml_tensor * norm_w   = nullptr;  // RMSNorm before lm_head
    // lm_head, only present when tie_word_embeddings=false (1b / 2b
    // variants). When tied (plus), the graph reuses dec_embed.token_w.
    ggml_tensor * output_w = nullptr;  // [hidden, vocab] or nullptr
};

// Aggregated weights

struct GraniteWeights {
    GraniteEncTop                enc_top;
    std::vector<GraniteEncBlock> enc_blocks;

    GraniteProjTop                proj_top;
    std::vector<GraniteProjBlock> proj_blocks;

    GraniteDecEmbed              dec_embed;
    std::vector<GraniteDecBlock> dec_blocks;
    GraniteDecFinal              dec_final;
};

transcribe_status build_granite_weights(ggml_context * ctx_meta, const GraniteHParams & hp, GraniteWeights & weights);

}  // namespace transcribe::granite
