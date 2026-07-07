// arch/whisper/weights.h - canonical Whisper ASR tensor catalog and
// per-instance weight slots. INTERNAL to src/arch/whisper/.
//
// WhisperHParams holds the architecture KV the loader reads (stt.whisper.* /
// stt.frontend.* / stt.capability.*); WhisperWeights holds the borrowed
// ggml_tensor* slots. Attention quirk: q/v/out carry bias, k does NOT (self
// and cross). Logits head is tied to dec.token_embd.weight, no separate bias.

#pragma once

#include "transcribe-mel.h"
#include "transcribe.h"

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

struct gguf_context;
struct ggml_context;
struct ggml_tensor;

namespace transcribe::whisper {

struct WhisperHParams {
    // Encoder.
    int32_t     enc_n_layers             = 0;
    int32_t     enc_d_model              = 0;
    int32_t     enc_n_heads              = 0;
    int32_t     enc_ffn_dim              = 0;
    int32_t     enc_num_mel_bins         = 0;
    int32_t     enc_max_source_positions = 0;  // 1500 across variants
    std::string enc_activation;                // always "gelu" in shipped checkpoints

    // Decoder.
    int32_t     dec_n_layers             = 0;
    int32_t     dec_d_model              = 0;  // equal to enc_d_model in upstream; carried separately for safety
    int32_t     dec_n_heads              = 0;
    int32_t     dec_ffn_dim              = 0;
    int32_t     dec_max_target_positions = 0;
    int32_t     dec_vocab_size           = 0;
    std::string dec_activation;
    bool        dec_tie_word_embeddings = true;
    bool        dec_scale_embedding     = false;  // HF config.scale_embedding; always false for upstream Whisper

    // Whisper generation contract.
    int32_t decoder_start_token_id = -1;  // <|startoftranscript|>, 50258 for multilingual
    int32_t no_timestamps_token_id = -1;  // <|notimestamps|>, 50363
    int32_t sot_token_id           = -1;  // alias of decoder_start_token_id; kept separate for clarity
    int32_t transcribe_token_id    = -1;  // <|transcribe|>
    int32_t translate_token_id     = -1;  // <|translate|>
    int32_t prev_sot_token_id      = -1;  // <|startofprev|>

    // Suppression lists (may be empty for .en variants).
    std::vector<int32_t> suppress_tokens;        // applied every step
    std::vector<int32_t> begin_suppress_tokens;  // applied on first generated step only

    // Frontend (WhisperFeatureExtractor).
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
    std::string fe_pad_mode;           // "reflect" for whisper
    bool        fe_center = true;
    std::string fe_mel_norm;           // "slaney"
    int32_t     fe_chunk_length  = 0;  // 30 (seconds)
    int32_t     fe_n_samples     = 0;  // 480000
    int32_t     fe_nb_max_frames = 0;  // 3000

    // Capability flags read from stt.capability.*.
    bool cap_lang_detect = false;
    bool cap_translate   = false;
    bool cap_timestamps  = false;

    // Derived helpers.
    int32_t enc_head_dim() const { return enc_n_heads > 0 ? enc_d_model / enc_n_heads : 0; }

    int32_t dec_head_dim() const { return dec_n_heads > 0 ? dec_d_model / dec_n_heads : 0; }
};

transcribe_status read_whisper_hparams(const gguf_context * gguf, WhisperHParams & hp);

// Shared mel frontend buffers. preprocessor_config.json ships the
// exact slaney filterbank and Hann window the model was trained with;
// the converter stores them verbatim so C++ does not need to
// recompute them from hparams.
struct WhisperFrontend {
    ggml_tensor * mel_filterbank = nullptr;  // [num_mels, n_fft/2+1]
    ggml_tensor * window         = nullptr;  // [n_fft]
};

// Encoder conv stem: two 1D convolutions with stride {1, 2}, kernel=3.
struct WhisperEncStem {
    ggml_tensor * conv0_w = nullptr;  // [d_model, num_mel_bins, 3]
    ggml_tensor * conv0_b = nullptr;  // [d_model]
    ggml_tensor * conv1_w = nullptr;  // [d_model, d_model, 3]
    ggml_tensor * conv1_b = nullptr;  // [d_model]
};

// Encoder learned positional embedding + final LayerNorm.
struct WhisperEncTop {
    ggml_tensor * pos_emb_w    = nullptr;  // [d_model, max_source_positions]
    ggml_tensor * final_norm_w = nullptr;  // [d_model]
    ggml_tensor * final_norm_b = nullptr;  // [d_model]
};

// One encoder transformer block.
struct WhisperEncBlock {
    ggml_tensor * norm_attn_w = nullptr;
    ggml_tensor * norm_attn_b = nullptr;
    ggml_tensor * attn_q_w    = nullptr;
    ggml_tensor * attn_q_b    = nullptr;
    ggml_tensor * attn_k_w    = nullptr;  // no bias
    ggml_tensor * attn_v_w    = nullptr;
    ggml_tensor * attn_v_b    = nullptr;
    ggml_tensor * attn_out_w  = nullptr;
    ggml_tensor * attn_out_b  = nullptr;
    ggml_tensor * norm_ffn_w  = nullptr;
    ggml_tensor * norm_ffn_b  = nullptr;
    ggml_tensor * ffn_fc1_w   = nullptr;
    ggml_tensor * ffn_fc1_b   = nullptr;
    ggml_tensor * ffn_fc2_w   = nullptr;
    ggml_tensor * ffn_fc2_b   = nullptr;
};

// Decoder token+position embedding and final LN.
struct WhisperDecTop {
    ggml_tensor * token_embd_w = nullptr;  // [d_model, vocab_size]  — also used as lm_head weight (tied)
    ggml_tensor * pos_emb_w    = nullptr;  // [d_model, max_target_positions]
    ggml_tensor * final_norm_w = nullptr;
    ggml_tensor * final_norm_b = nullptr;
};

// One decoder block: self-attn + cross-attn + FFN, all pre-LN.
struct WhisperDecBlock {
    ggml_tensor * norm_self_w = nullptr;
    ggml_tensor * norm_self_b = nullptr;
    ggml_tensor * self_q_w    = nullptr;
    ggml_tensor * self_q_b    = nullptr;
    ggml_tensor * self_k_w    = nullptr;  // no bias
    ggml_tensor * self_v_w    = nullptr;
    ggml_tensor * self_v_b    = nullptr;
    ggml_tensor * self_out_w  = nullptr;
    ggml_tensor * self_out_b  = nullptr;

    // Cross-attention queries decoder state against encoder output.
    ggml_tensor * norm_cross_w = nullptr;
    ggml_tensor * norm_cross_b = nullptr;
    ggml_tensor * cross_q_w    = nullptr;
    ggml_tensor * cross_q_b    = nullptr;
    ggml_tensor * cross_k_w    = nullptr;  // no bias
    ggml_tensor * cross_v_w    = nullptr;
    ggml_tensor * cross_v_b    = nullptr;
    ggml_tensor * cross_out_w  = nullptr;
    ggml_tensor * cross_out_b  = nullptr;

    ggml_tensor * norm_ffn_w = nullptr;
    ggml_tensor * norm_ffn_b = nullptr;
    ggml_tensor * ffn_fc1_w  = nullptr;
    ggml_tensor * ffn_fc1_b  = nullptr;
    ggml_tensor * ffn_fc2_w  = nullptr;
    ggml_tensor * ffn_fc2_b  = nullptr;
};

struct WhisperWeights {
    WhisperFrontend              frontend;
    WhisperEncStem               enc_stem;
    WhisperEncTop                enc_top;
    std::vector<WhisperEncBlock> enc_blocks;
    WhisperDecTop                dec_top;
    std::vector<WhisperDecBlock> dec_blocks;
};

transcribe_status build_whisper_weights(ggml_context * ctx_meta, const WhisperHParams & hp, WhisperWeights & weights);

// Build a MelConfig from `hp`, install the caller-provided
// `filterbank` + `window` buffers (either may be empty — MelFrontend
// reconstructs from cfg constants in that case), validate the
// fe_normalize tag, and emplace a MelFrontend into `out_mel`. Shared
// between the GGUF load path (buffers fetched via
// read_f32_tensor_checked) and the legacy whisper.cpp `.bin` adapter
// (mel filterbank from the parsed file, Hann window computed in C++).
transcribe_status install_mel_from_buffers(const WhisperHParams &                   hp,
                                           std::vector<float>                       filterbank,
                                           std::vector<float>                       window,
                                           std::optional<transcribe::MelFrontend> & out_mel);

}  // namespace transcribe::whisper
