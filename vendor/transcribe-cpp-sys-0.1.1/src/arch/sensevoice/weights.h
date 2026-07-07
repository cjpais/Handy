// arch/sensevoice/weights.h - canonical SenseVoice tensor catalog and
// per-instance weight slots.
//
// Internal to src/arch/sensevoice/. Defines:
//
//   - SenseVoiceHParams: every architecture KV the loader reads from
//     stt.sensevoice.* / stt.frontend.* before allocating tensors.
//
//   - SenseVoiceWeights: borrowed ggml_tensor* slots for every logical
//     weight in a SenseVoiceSmall checkpoint. Storage is owned by the
//     model's ggml_context (gguf_init_from_file with no_alloc=false).
//
// Tensor naming and shapes match scripts/convert-sensevoice.py exactly:
//
//   frontend.cmvn.{shift,scale}        [d_input]                 F32
//   enc.embed.weight                   [16, d_input]             F32
//   enc.encoders0.0.attn.qkv.weight    [3·d_model, d_input]      F32
//   enc.encoders0.0.attn.qkv.bias      [3·d_model]               F32
//   enc.encoders0.0.attn.out.weight    [d_model, d_model]        F32
//   enc.encoders0.0.attn.out.bias      [d_model]                 F32
//   enc.encoders0.0.attn.fsmn.weight   [kernel, 1, d_model]      F32  (depthwise conv)
//   enc.encoders0.0.ffn.fc1.weight     [d_ff, d_model]           F32
//   enc.encoders0.0.ffn.fc1.bias       [d_ff]                    F32
//   enc.encoders0.0.ffn.fc2.weight     [d_model, d_ff]           F32
//   enc.encoders0.0.ffn.fc2.bias       [d_model]                 F32
//   enc.encoders0.0.norm_attn.weight   [d_input]                 F32  (norm in input space)
//   enc.encoders0.0.norm_attn.bias     [d_input]                 F32
//   enc.encoders0.0.norm_ffn.weight    [d_model]                 F32
//   enc.encoders0.0.norm_ffn.bias      [d_model]                 F32
//
//   enc.encoders.{i}.* (i = 0..n_blocks-2)      same per-block shape
//                                                but every dim is d_model
//   enc.tp_encoders.{i}.* (i = 0..tp_blocks-1)  same shape as encoders
//
//   enc.after_norm.{weight,bias}       [d_model]                 F32
//   enc.tp_norm.{weight,bias}          [d_model]                 F32
//
//   ctc.head.weight                    [d_model, vocab]          quantizable
//   ctc.head.bias                      [vocab]                   F32

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::sensevoice {

// Hyperparameter contract. Read from stt.sensevoice.* / stt.frontend.*.
struct SenseVoiceHParams {
    // Encoder dims.
    int32_t     enc_n_blocks   = 0;  // includes the encoders0[0] projection block
    int32_t     enc_tp_blocks  = 0;
    int32_t     enc_d_model    = 0;  // 512
    int32_t     enc_d_input    = 0;  // 560 = 80 mels × lfr_m=7
    int32_t     enc_n_heads    = 0;  // 4
    int32_t     enc_d_ff       = 0;  // 2048
    int32_t     enc_kernel     = 0;  // 11 (FSMN depthwise conv)
    int32_t     enc_sanm_shift = 0;  // 0 — left-pad shift for non-causal SAN-M
    std::string enc_attn_type;       // "sanm"
    bool        enc_normalize_before = true;

    // Vocab is implicit in the SentencePiece tokenizer table the loader
    // ingests; the CTC head's row count must match it. Stored here as
    // a derived value once the tokenizer is loaded.
    int32_t vocab_size = 0;

    // Frontend (kaldi HTK fbank + LFR + per-feature CMVN).
    std::string fe_type;             // "kaldi_fbank_lfr"
    int32_t     fe_num_mels    = 0;  // 80
    int32_t     fe_sample_rate = 0;  // 16000
    int32_t     fe_n_fft       = 0;  // 400 (kaldi pads to next pow2 internally)
    int32_t     fe_win_length  = 0;  // 400
    int32_t     fe_hop_length  = 0;  // 160
    std::string fe_window;           // "hamming"
    std::string fe_normalize;        // "per_feature"
    std::string fe_fbank_style;      // "kaldi_htk"
    float       fe_dither          = 0.0f;
    bool        fe_upscale_samples = true;
    bool        fe_snip_edges      = true;
    int32_t     fe_lfr_m           = 0;  // 7
    int32_t     fe_lfr_n           = 0;  // 6

    // Pre-encoder prefix-embedding indices baked into the GGUF KV (so
    // the runtime does not have to memorize lid_dict / textnorm_dict).
    // All indices are rows in enc.embed.weight (vocabulary IS the
    // 16-row prefix-embedding table, NOT the SentencePiece output).
    int32_t prefix_lang_auto       = 0;
    int32_t prefix_lang_zh         = 0;
    int32_t prefix_lang_en         = 0;
    int32_t prefix_lang_yue        = 0;
    int32_t prefix_lang_ja         = 0;
    int32_t prefix_lang_ko         = 0;
    int32_t prefix_lang_nospeech   = 0;
    int32_t prefix_event_speech    = 0;
    int32_t prefix_emotion_neutral = 0;
    int32_t prefix_withitn         = 0;
    int32_t prefix_woitn           = 0;

    // Derived.
    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }
};

transcribe_status read_sensevoice_hparams(const gguf_context * gguf, SenseVoiceHParams & hp);

// One SAN-M block. Same shape contract for every block (encoders0,
// encoders, tp_encoders) — only encoders0[0] differs in that its
// norm_attn and qkv input dim is d_input rather than d_model.
struct SenseVoiceBlock {
    ggml_tensor * norm_attn_w = nullptr;  // [d_in]
    ggml_tensor * norm_attn_b = nullptr;  // [d_in]
    ggml_tensor * attn_qkv_w  = nullptr;  // [3·d_model, d_in]   fused QKV
    ggml_tensor * attn_qkv_b  = nullptr;  // [3·d_model]
    ggml_tensor * attn_out_w  = nullptr;  // [d_model, d_model]
    ggml_tensor * attn_out_b  = nullptr;  // [d_model]
    ggml_tensor * attn_fsmn_w = nullptr;  // [kernel, 1, d_model] depthwise conv1d
    ggml_tensor * norm_ffn_w  = nullptr;  // [d_model]
    ggml_tensor * norm_ffn_b  = nullptr;  // [d_model]
    ggml_tensor * ffn_fc1_w   = nullptr;  // [d_ff, d_model]
    ggml_tensor * ffn_fc1_b   = nullptr;  // [d_ff]
    ggml_tensor * ffn_fc2_w   = nullptr;  // [d_model, d_ff]
    ggml_tensor * ffn_fc2_b   = nullptr;  // [d_model]
};

struct SenseVoiceWeights {
    // Frontend per-feature CMVN (baked from am.mvn). forward op:
    // (x + shift) * scale, mirrors funasr.frontends.wav_frontend.apply_cmvn.
    ggml_tensor * cmvn_shift = nullptr;  // [d_input]
    ggml_tensor * cmvn_scale = nullptr;  // [d_input]

    // Pre-encoder prefix-embedding table (16 rows × d_input cols).
    // Indices come from the upstream lid_dict / textnorm_dict.
    ggml_tensor * embed = nullptr;  // [16, d_input]

    // The first block lives separately because its norm_attn / attn_qkv
    // input dim is d_input (not d_model) and the attention sub-layer
    // skips the residual add.
    SenseVoiceBlock encoders0;

    // Main tier (49 blocks at d_model).
    std::vector<SenseVoiceBlock> encoders;  // size = enc_n_blocks - 1

    // Inter-tier LayerNorm.
    ggml_tensor * after_norm_w = nullptr;  // [d_model]
    ggml_tensor * after_norm_b = nullptr;  // [d_model]

    // TP tier (20 blocks at d_model).
    std::vector<SenseVoiceBlock> tp_encoders;  // size = enc_tp_blocks

    ggml_tensor * tp_norm_w = nullptr;         // [d_model]
    ggml_tensor * tp_norm_b = nullptr;         // [d_model]

    // CTC head.
    ggml_tensor * ctc_head_w = nullptr;  // [d_model, vocab]
    ggml_tensor * ctc_head_b = nullptr;  // [vocab]
};

// Walk the canonical tensor list and bind each slot in `weights` to a
// borrowed pointer in ctx_meta. Required by default; missing or
// shape-mismatched tensors return TRANSCRIBE_ERR_GGUF naming the
// offending tensor.
//
// On failure the partially-built `weights` is in an indeterminate
// state — the caller must throw the model away.
transcribe_status build_sensevoice_weights(ggml_context *            ctx_meta,
                                           const SenseVoiceHParams & hp,
                                           SenseVoiceWeights &       weights);

}  // namespace transcribe::sensevoice
