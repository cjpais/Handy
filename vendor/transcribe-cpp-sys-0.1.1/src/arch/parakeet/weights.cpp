// arch/parakeet/weights.cpp - read_parakeet_hparams and
// build_parakeet_weights.
//
// Read every required hparam from KV explicitly, then build the tensor
// catalog as get_tensor() calls with explicit expected shapes; a missing
// tensor or shape mismatch returns TRANSCRIBE_ERR_GGUF naming the tensor.
// The shape contract documented in weights.h is enforced here.

#include "weights.h"

#include "ggml.h"
#include "gguf.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "transcribe-weights-util.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>

namespace transcribe::parakeet {

// ---------------------------------------------------------------------------
// Hparams
// ---------------------------------------------------------------------------

namespace {

constexpr const char * kFamilyTag = "parakeet";

}  // namespace

transcribe_status read_parakeet_hparams(const gguf_context * gguf, ParakeetHParams & hp) {
    if (gguf == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Encoder.
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.n_layers", kFamilyTag, hp.enc_n_layers);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.d_model", kFamilyTag, hp.enc_d_model);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.n_heads", kFamilyTag, hp.enc_n_heads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.d_ff", kFamilyTag, hp.enc_d_ff);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.conv_kernel", kFamilyTag, hp.enc_conv_kernel);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.subsampling_factor", kFamilyTag,
                                       hp.enc_subsampling_factor);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.parakeet.encoder.subsampling_channels", kFamilyTag,
                                       hp.enc_subsampling_channels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st =
            read_required_u32_kv(gguf, "stt.parakeet.encoder.pos_emb_max_len", kFamilyTag, hp.enc_pos_emb_max_len);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // use_bias: false for v2/v3, true for 1.1B and rnnt/ctc (11 biases
    // per block). Default false for legacy GGUFs without the KV.
    if (auto st = read_optional_bool_kv(gguf, "stt.parakeet.encoder.use_bias", kFamilyTag, false, hp.enc_use_bias);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // xscaling (see weights.h). Default false for legacy GGUFs.
    if (auto st = read_optional_bool_kv(gguf, "stt.parakeet.encoder.xscaling", kFamilyTag, false, hp.enc_xscaling);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Local attention window. Default -1/-1 = full attention.
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.att_context_left", kFamilyTag, -1,
                                         hp.enc_att_context_left);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.att_context_right", kFamilyTag, -1,
                                         hp.enc_att_context_right);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Multi-lookahead training menu (cache-aware streaming). Flat int32
    // [L0,R0,L1,R1,...]; index 0 must equal (att_context_left,
    // att_context_right). Absent for offline GGUFs → synthesize a
    // one-element list from the scalar fields.
    {
        std::vector<int32_t> flat;
        switch (read_int32_array_kv(gguf, "stt.parakeet.encoder.att_context_size_choices", flat)) {
            case KvResult::Ok:
                if (flat.empty() || (flat.size() % 2) != 0) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: stt.parakeet.encoder.att_context_size_choices "
                            "has odd length %zu (expected pairs)",
                            flat.size());
                    return TRANSCRIBE_ERR_GGUF;
                }
                hp.enc_att_context_size_choices.clear();
                hp.enc_att_context_size_choices.reserve(flat.size() / 2);
                for (size_t i = 0; i + 1 < flat.size(); i += 2) {
                    hp.enc_att_context_size_choices.emplace_back(flat[i], flat[i + 1]);
                }
                if (hp.enc_att_context_size_choices.front().first != hp.enc_att_context_left ||
                    hp.enc_att_context_size_choices.front().second != hp.enc_att_context_right) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: att_context_size_choices[0] = (%d, %d) "
                            "but att_context_left/right = (%d, %d); "
                            "GGUF is inconsistent",
                            hp.enc_att_context_size_choices.front().first,
                            hp.enc_att_context_size_choices.front().second, hp.enc_att_context_left,
                            hp.enc_att_context_right);
                    return TRANSCRIBE_ERR_GGUF;
                }
                break;
            case KvResult::Absent:
                // Synthesize from the scalar fields. For full-attention
                // GGUFs (-1, -1) this still produces a one-element list;
                // streaming code paths gate on att_context_style anyway.
                hp.enc_att_context_size_choices.clear();
                hp.enc_att_context_size_choices.emplace_back(hp.enc_att_context_left, hp.enc_att_context_right);
                break;
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: stt.parakeet.encoder.att_context_size_choices "
                        "has wrong type (expected int32 array)");
                return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Chunked-limited-with-rc training menu (buffered streaming). Three
    // independent int32 L / C / R arrays; absent for non-buffered GGUFs.
    {
        auto read_menu = [&](const char * key, std::vector<int32_t> & out) -> transcribe_status {
            std::vector<int32_t> raw;
            switch (read_int32_array_kv(gguf, key, raw)) {
                case KvResult::Ok:
                    out = std::move(raw);
                    return TRANSCRIBE_OK;
                case KvResult::Absent:
                    out.clear();
                    return TRANSCRIBE_OK;
                case KvResult::BadType:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: %s has wrong type "
                            "(expected int32 array)",
                            key);
                    return TRANSCRIBE_ERR_GGUF;
            }
            return TRANSCRIBE_ERR_GGUF;
        };
        if (auto st = read_menu("stt.parakeet.encoder.att_chunk_left_choices", hp.enc_att_chunk_left_choices);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_menu("stt.parakeet.encoder.att_chunk_chunk_choices", hp.enc_att_chunk_chunk_choices);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_menu("stt.parakeet.encoder.att_chunk_right_choices", hp.enc_att_chunk_right_choices);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // Attention context style. Optional, default "regular" for legacy GGUFs.
    {
        std::string style;
        if (auto st =
                read_optional_string_kv(gguf, "stt.parakeet.encoder.att_context_style", kFamilyTag, "regular", style);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (style == "regular") {
            hp.enc_att_context_style = ParakeetHParams::AttContextStyle::Regular;
        } else if (style == "chunked_limited") {
            hp.enc_att_context_style = ParakeetHParams::AttContextStyle::ChunkedLimited;
        } else if (style == "chunked_limited_with_rc") {
            hp.enc_att_context_style = ParakeetHParams::AttContextStyle::ChunkedLimitedWithRc;
            if (hp.enc_att_chunk_left_choices.empty() || hp.enc_att_chunk_chunk_choices.empty() ||
                hp.enc_att_chunk_right_choices.empty()) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: att_context_style=chunked_limited_with_rc "
                        "requires non-empty att_chunk_{left,chunk,right}_choices "
                        "arrays in the GGUF; got %zu/%zu/%zu entries",
                        hp.enc_att_chunk_left_choices.size(), hp.enc_att_chunk_chunk_choices.size(),
                        hp.enc_att_chunk_right_choices.size());
                return TRANSCRIBE_ERR_GGUF;
            }
        } else {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: unsupported att_context_style \"%s\" "
                    "(allowed: regular, chunked_limited, chunked_limited_with_rc)",
                    style.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Conv module: per-side depthwise padding and norm type. Optional;
    // defaults -1 (centred (k-1)/2) and "batch_norm" (fused BN with
    // running_mean / running_var).
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.conv_context_left", kFamilyTag, -1,
                                         hp.enc_conv_context_left);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.conv_context_right", kFamilyTag, -1,
                                         hp.enc_conv_context_right);
        st != TRANSCRIBE_OK) {
        return st;
    }
    {
        std::string norm_type;
        if (auto st = read_optional_string_kv(gguf, "stt.parakeet.encoder.conv_norm_type", kFamilyTag, "batch_norm",
                                              norm_type);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (norm_type == "batch_norm") {
            hp.enc_conv_norm_type = ParakeetHParams::ConvNormType::BatchNorm;
        } else if (norm_type == "layer_norm") {
            hp.enc_conv_norm_type = ParakeetHParams::ConvNormType::LayerNorm;
        } else {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: unsupported conv_norm_type \"%s\" "
                    "(allowed: batch_norm, layer_norm)",
                    norm_type.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // Cache-aware streaming pre-encode constants. Optional, default 0
    // (non-streaming); the streaming paths gate on both > 0.
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.streaming.pre_encode_cache_size", kFamilyTag, 0,
                                         hp.enc_stream_pre_encode_cache_size);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.streaming.drop_extra_pre_encoded", kFamilyTag, 0,
                                         hp.enc_stream_drop_extra_pre_encoded);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_optional_int32_kv(gguf, "stt.parakeet.encoder.streaming.sampling_frames_first", kFamilyTag, 0,
                                         hp.enc_stream_sampling_frames_first);
        st != TRANSCRIBE_OK) {
        return st;
    }

    // Head kind dispatch. Optional KV, default "tdt" for legacy v2/v3.
    {
        std::string head_kind_str;
        if (auto st = read_optional_string_kv(gguf, "stt.parakeet.head_kind", kFamilyTag, "tdt", head_kind_str);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (head_kind_str == "tdt") {
            hp.head_kind = HeadKind::TDT;
        } else if (head_kind_str == "rnnt") {
            hp.head_kind = HeadKind::RNNT;
        } else if (head_kind_str == "ctc") {
            hp.head_kind = HeadKind::CTC;
        } else {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: unsupported head_kind \"%s\" "
                    "(allowed: tdt, rnnt, ctc)",
                    head_kind_str.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    if (hp.head_kind != HeadKind::CTC) {
        // Predictor (TDT and RNNT only; CTC has no predictor).
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.predictor.hidden", kFamilyTag, hp.pred_hidden);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.predictor.n_layers", kFamilyTag, hp.pred_n_layers);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.predictor.vocab", kFamilyTag, hp.pred_vocab);
            st != TRANSCRIBE_OK) {
            return st;
        }

        // Joint (TDT and RNNT only).
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.joint.hidden", kFamilyTag, hp.joint_hidden);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.joint.num_extra_outputs", kFamilyTag,
                                           hp.joint_num_extra_outputs);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (auto st = read_required_string_kv(gguf, "stt.parakeet.joint.activation", kFamilyTag, hp.joint_activation);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    if (hp.head_kind == HeadKind::TDT) {
        // TDT decoding parameters. durations required; max_symbols
        // optional (default 10). RNNT carries neither.
        switch (read_int32_array_kv(gguf, "stt.parakeet.tdt.durations", hp.tdt_durations)) {
            case KvResult::Ok:
                break;
            case KvResult::Absent:
            case KvResult::BadType:
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: required KV \"stt.parakeet.tdt.durations\" "
                        "missing or wrong type (head_kind=tdt)");
                return TRANSCRIBE_ERR_GGUF;
        }
        {
            uint32_t max_sym = 10;
            switch (read_uint32_kv(gguf, "stt.parakeet.tdt.max_symbols", max_sym)) {
                case KvResult::Ok:
                case KvResult::Absent:
                    hp.tdt_max_symbols = static_cast<int32_t>(max_sym);
                    break;
                case KvResult::BadType:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: optional KV \"stt.parakeet.tdt.max_symbols\" "
                            "has wrong type");
                    return TRANSCRIBE_ERR_GGUF;
            }
        }
    } else if (hp.head_kind == HeadKind::RNNT) {
        // RNNT: no durations, joint emits exactly vocab+1. tdt_max_symbols
        // is reused as the per-frame stuck cap.
        hp.tdt_durations.clear();
        hp.tdt_max_symbols = 10;
    } else {
        // CTC: no predictor, no joint, no durations.
        hp.tdt_durations.clear();
        hp.tdt_max_symbols = 0;
    }

    // Frontend. The complete stt.frontend.* contract between converter
    // and loader; reading every field makes the loader the gate that
    // catches a converter that drops one. CMVN/LFR fields are not read
    // (Parakeet doesn't use them).
    if (auto st = read_required_string_kv(gguf, "stt.frontend.type", kFamilyTag, hp.fe_type); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.num_mels", kFamilyTag, hp.fe_num_mels);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.sample_rate", kFamilyTag, hp.fe_sample_rate);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.n_fft", kFamilyTag, hp.fe_n_fft); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.win_length", kFamilyTag, hp.fe_win_length);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_u32_kv(gguf, "stt.frontend.hop_length", kFamilyTag, hp.fe_hop_length);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.window", kFamilyTag, hp.fe_window); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_string_kv(gguf, "stt.frontend.normalize", kFamilyTag, hp.fe_normalize);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.dither", kFamilyTag, hp.fe_dither); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.pre_emphasis", kFamilyTag, hp.fe_pre_emphasis);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_min", kFamilyTag, hp.fe_f_min); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_required_f32_kv(gguf, "stt.frontend.f_max", kFamilyTag, hp.fe_f_max); st != TRANSCRIBE_OK) {
        return st;
    }

    // Prompt MLP. Optional (multilingual variants only). Presence of
    // stt.parakeet.prompt.num_prompts is the gate; absent leaves every
    // prompt_* field zero/empty and the encoder skips the prompt path.
    if (gguf_find_key(gguf, "stt.parakeet.prompt.num_prompts") >= 0) {
        if (auto st = read_required_u32_kv(gguf, "stt.parakeet.prompt.num_prompts", kFamilyTag, hp.prompt_num_prompts);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (hp.prompt_num_prompts > 0) {
            hp.has_prompt = true;

            if (auto st = read_required_u32_kv(gguf, "stt.parakeet.prompt.hidden", kFamilyTag, hp.prompt_hidden);
                st != TRANSCRIBE_OK) {
                return st;
            }
            if (auto st = read_required_string_kv(gguf, "stt.parakeet.prompt.field", kFamilyTag, hp.prompt_field);
                st != TRANSCRIBE_OK) {
                return st;
            }
            if (auto st =
                    read_required_string_kv(gguf, "stt.parakeet.prompt.activation", kFamilyTag, hp.prompt_activation);
                st != TRANSCRIBE_OK) {
                return st;
            }

            switch (
                read_string_array_kv(gguf, "stt.parakeet.prompt.dictionary.locales", hp.prompt_dictionary_locales)) {
                case KvResult::Ok:
                    break;
                case KvResult::Absent:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: stt.parakeet.prompt.dictionary.locales "
                            "is required when prompt.num_prompts > 0");
                    return TRANSCRIBE_ERR_GGUF;
                case KvResult::BadType:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: stt.parakeet.prompt.dictionary.locales "
                            "has wrong type (expected string array)");
                    return TRANSCRIBE_ERR_GGUF;
            }
            switch (read_int32_array_kv(gguf, "stt.parakeet.prompt.dictionary.indices", hp.prompt_dictionary_indices)) {
                case KvResult::Ok:
                    break;
                case KvResult::Absent:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: stt.parakeet.prompt.dictionary.indices "
                            "is required when prompt.num_prompts > 0");
                    return TRANSCRIBE_ERR_GGUF;
                case KvResult::BadType:
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: stt.parakeet.prompt.dictionary.indices "
                            "has wrong type (expected int32 array)");
                    return TRANSCRIBE_ERR_GGUF;
            }
            if (hp.prompt_dictionary_locales.size() != hp.prompt_dictionary_indices.size()) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: prompt dictionary locales/indices length "
                        "mismatch (%zu vs %zu)",
                        hp.prompt_dictionary_locales.size(), hp.prompt_dictionary_indices.size());
                return TRANSCRIBE_ERR_GGUF;
            }
            if (gguf_find_key(gguf, "stt.parakeet.prompt.auto_id") >= 0) {
                if (auto st = read_required_u32_kv(gguf, "stt.parakeet.prompt.auto_id", kFamilyTag, hp.prompt_auto_id);
                    st != TRANSCRIBE_OK) {
                    return st;
                }
            } else {
                hp.prompt_auto_id = -1;
            }

            // The C++ prompt MLP implements ReLU only.
            if (hp.prompt_activation != "relu") {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: unsupported prompt activation \"%s\" "
                        "(only \"relu\" is implemented)",
                        hp.prompt_activation.c_str());
                return TRANSCRIBE_ERR_GGUF;
            }
            if (hp.prompt_hidden <= 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: prompt.hidden must be > 0 (got %d)", hp.prompt_hidden);
                return TRANSCRIBE_ERR_GGUF;
            }
            for (int32_t idx : hp.prompt_dictionary_indices) {
                if (idx < 0 || idx >= hp.prompt_num_prompts) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "parakeet: prompt dictionary index %d out of range "
                            "[0, %d)",
                            idx, hp.prompt_num_prompts);
                    return TRANSCRIBE_ERR_GGUF;
                }
            }
            if (hp.prompt_auto_id >= 0 && hp.prompt_auto_id >= hp.prompt_num_prompts) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: prompt.auto_id %d out of range [0, %d)",
                        hp.prompt_auto_id, hp.prompt_num_prompts);
                return TRANSCRIBE_ERR_GGUF;
            }
        }
    }

    // Cross-field invariants — caught here rather than surfacing as
    // confusing shape mismatches downstream.
    if (hp.enc_n_layers <= 0 || hp.enc_d_model <= 0 || hp.enc_n_heads <= 0 || hp.enc_d_ff <= 0 ||
        hp.enc_conv_kernel <= 0 || hp.enc_subsampling_factor <= 0 || hp.enc_subsampling_channels <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: encoder hparams must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.enc_d_model % hp.enc_n_heads != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: encoder d_model (%d) not divisible by n_heads (%d)",
                hp.enc_d_model, hp.enc_n_heads);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.head_kind != HeadKind::CTC) {
        if (hp.pred_hidden <= 0 || hp.pred_n_layers <= 0 || hp.pred_vocab <= 1) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: predictor hparams must be positive");
            return TRANSCRIBE_ERR_GGUF;
        }
        if (hp.joint_hidden <= 0 || hp.joint_num_extra_outputs < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: joint hparams invalid");
            return TRANSCRIBE_ERR_GGUF;
        }
        // Joint activation allow-list (the C++ joint forward implements
        // these three).
        if (hp.joint_activation != "relu" && hp.joint_activation != "sigmoid" && hp.joint_activation != "tanh") {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: unsupported joint activation \"%s\" "
                    "(only relu, sigmoid, tanh are implemented)",
                    hp.joint_activation.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    if (hp.head_kind == HeadKind::TDT) {
        // TDT durations: non-empty, length == num_extra_outputs, all
        // non-negative (negative jumps would walk backwards in time).
        if (hp.tdt_durations.empty()) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: stt.parakeet.tdt.durations must be non-empty (head_kind=tdt)");
            return TRANSCRIBE_ERR_GGUF;
        }
        if (static_cast<int32_t>(hp.tdt_durations.size()) != hp.joint_num_extra_outputs) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: stt.parakeet.tdt.durations length (%zu) "
                    "must equal joint.num_extra_outputs (%d)",
                    hp.tdt_durations.size(), hp.joint_num_extra_outputs);
            return TRANSCRIBE_ERR_GGUF;
        }
        for (int32_t d : hp.tdt_durations) {
            if (d < 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "parakeet: stt.parakeet.tdt.durations contains "
                        "negative value %d",
                        d);
                return TRANSCRIBE_ERR_GGUF;
            }
        }
        if (hp.tdt_max_symbols < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: stt.parakeet.tdt.max_symbols must be >= 0 "
                    "(got %d)",
                    hp.tdt_max_symbols);
            return TRANSCRIBE_ERR_GGUF;
        }
    } else if (hp.head_kind == HeadKind::RNNT) {
        // RNNT joint emits exactly vocab+1 — no duration extras.
        if (hp.joint_num_extra_outputs != 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "parakeet: head_kind=rnnt requires joint.num_extra_outputs=0 "
                    "(got %d)",
                    hp.joint_num_extra_outputs);
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    if (hp.fe_num_mels <= 0 || hp.fe_sample_rate <= 0 || hp.fe_n_fft <= 0 || hp.fe_win_length <= 0 ||
        hp.fe_hop_length <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: frontend dimensions must be positive");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_win_length > hp.fe_n_fft) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: frontend win_length (%d) > n_fft (%d)", hp.fe_win_length,
                hp.fe_n_fft);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_f_min < 0.0f || hp.fe_f_max <= hp.fe_f_min) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: frontend mel band invalid: f_min=%f f_max=%f", hp.fe_f_min,
                hp.fe_f_max);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_dither < 0.0f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: frontend dither must be >= 0 (got %f)", hp.fe_dither);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (hp.fe_type != "mel") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: unsupported frontend type \"%s\" (only \"mel\")",
                hp.fe_type.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    // Frontend value-domain checks: the loader reads fields the C++
    // MelFrontend doesn't parameterize on, so fail loud if a GGUF asks
    // for a value the implementation can't honor rather than silently
    // substituting the hard-coded behavior.

    // Window: only symmetric Hann is implemented.
    if (hp.fe_window != "hann") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet: unsupported frontend window \"%s\" "
                "(only \"hann\" is implemented)",
                hp.fe_window.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    // Normalize: "per_feature" (offline variants) or "none" (streaming,
    // normalisation baked into training). NeMo's all_features / CMVN are
    // not implemented.
    if (hp.fe_normalize != "per_feature" && hp.fe_normalize != "none") {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet: unsupported frontend normalize \"%s\" "
                "(only \"per_feature\" and \"none\" are implemented)",
                hp.fe_normalize.c_str());
        return TRANSCRIBE_ERR_GGUF;
    }

    // n_fft must be a power of two: the FFT is a radix-2 Cooley-Tukey
    // whose bit-reversal produces garbage for non-power-of-two sizes.
    if ((hp.fe_n_fft & (hp.fe_n_fft - 1)) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "parakeet: frontend n_fft (%d) must be a power of 2 "
                "(radix-2 FFT requirement)",
                hp.fe_n_fft);
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Weights
// ---------------------------------------------------------------------------

namespace {

using transcribe::weights::lname;

// find_tensor() + lname() live in src/transcribe-weights-util.{h,cpp};
// the GET_* macros below live here because their type allowlists encode
// a per-family quantization policy. Three buckets, each with a fixed
// type allowlist:
//   GET_F32  - norms, biases, BN running stats, attn pos_bias (tiny +
//              precision-sensitive; fp32 across every preset).
//   GET_CONV - conv kernels (TRANSCRIBE_QUANT_CONV_TYPES = F32/F16;
//              quantized kernels would need a quantized im2col path
//              ggml lacks).
//   GET_LIN  - mul_mat linear weights (TRANSCRIBE_QUANT_LINEAR_TYPES;
//              every family accepts the same set so a quantize preset
//              never produces a tensor the loader refuses).
constexpr const char * kTag = kFamilyTag;

#define GET_F32(slot, name, ...)                                                                          \
    do {                                                                                                  \
        ggml_tensor * _t =                                                                                \
            transcribe::weights::find_tensor(ctx_meta, (name), { GGML_TYPE_F32 }, { __VA_ARGS__ }, kTag); \
        if (_t == nullptr)                                                                                \
            return TRANSCRIBE_ERR_GGUF;                                                                   \
        (slot) = _t;                                                                                      \
    } while (0)

#define GET_CONV(slot, name, ...)                                                                              \
    do {                                                                                                       \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_CONV_TYPES }, \
                                                            { __VA_ARGS__ }, kTag);                            \
        if (_t == nullptr)                                                                                     \
            return TRANSCRIBE_ERR_GGUF;                                                                        \
        (slot) = _t;                                                                                           \
    } while (0)

#define GET_LIN(slot, name, ...)                                                                                 \
    do {                                                                                                         \
        ggml_tensor * _t = transcribe::weights::find_tensor(ctx_meta, (name), { TRANSCRIBE_QUANT_LINEAR_TYPES }, \
                                                            { __VA_ARGS__ }, kTag);                              \
        if (_t == nullptr)                                                                                       \
            return TRANSCRIBE_ERR_GGUF;                                                                          \
        (slot) = _t;                                                                                             \
    } while (0)

}  // namespace

transcribe_status build_parakeet_weights(ggml_context *          ctx_meta,
                                         const ParakeetHParams & hp,
                                         ParakeetWeights &       weights) {
    if (ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int64_t channels = hp.enc_subsampling_channels;
    const int64_t d_model  = hp.enc_d_model;
    const int64_t d_ff     = hp.enc_d_ff;
    const int64_t n_heads  = hp.enc_n_heads;
    const int64_t head_dim = hp.enc_head_dim();
    const int64_t k        = hp.enc_conv_kernel;
    const int64_t pred_h   = hp.pred_hidden;        // 0 for CTC
    const int64_t pred_v   = hp.pred_vocab;         // 0 for CTC
    const int64_t joint_h  = hp.joint_hidden;       // 0 for CTC
    const int64_t joint_n  = hp.joint_n_classes();  // meaningless for CTC; we read CTC head shape instead

    // The flattened "freq" axis into pre_encode.out is
    // (subsampling_channels * F'), where F' is the freq dim after three
    // k=3 s=2 convs. Padding tracks NeMo's causal_downsampling (inferred
    // from the attention style — only ChunkedLimited is causal), NOT the
    // conformer conv_context_size:
    //   non-causal: symmetric (k-1)/2 both sides. 128→64→32→16; 256*16=4096.
    //   causal:     (left=k-1, right=stride-1). 128→65→33→17; 256*17=4352.
    auto pre_encode_F_prime = [&]() {
        const bool causal = (hp.enc_att_context_style == ParakeetHParams::AttContextStyle::ChunkedLimited);
        const int  k = 3, s = 2;
        // Total per-axis pad: (k-1)/2 + (k-1)/2 = k-1 = 2 for non-causal;
        // (k-1) + (s-1) = 3 for causal.
        const int  total_pad = causal ? ((k - 1) + (s - 1)) : ((k - 1) / 2 + (k - 1) / 2);
        int        dim       = hp.fe_num_mels;
        for (int i = 0; i < 3; ++i) {
            dim = ((dim + total_pad - k) / s) + 1;
        }
        return static_cast<int64_t>(dim);
    };
    const int64_t pre_encode_freq = channels * pre_encode_F_prime();
    const int64_t pre_encode_in   = pre_encode_freq;

    // ----- pre_encode -----
    // Conv shapes documented in weights.h. ggml_tensor::ne is fast-to-
    // slow dim order; for an OIHW conv kernel that's [kw, kh, in, out].
    // 1×1 pointwise convs collapse to [1, 1, in, out].
    GET_CONV(weights.pre_encode.conv0_w, "enc.pre_encode.conv.0.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv0_b, "enc.pre_encode.conv.0.bias", channels);
    GET_CONV(weights.pre_encode.conv2_w, "enc.pre_encode.conv.2.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv2_b, "enc.pre_encode.conv.2.bias", channels);
    GET_CONV(weights.pre_encode.conv3_w, "enc.pre_encode.conv.3.weight", 1, 1, channels, channels);
    GET_F32(weights.pre_encode.conv3_b, "enc.pre_encode.conv.3.bias", channels);
    GET_CONV(weights.pre_encode.conv5_w, "enc.pre_encode.conv.5.weight", 3, 3, 1, channels);
    GET_F32(weights.pre_encode.conv5_b, "enc.pre_encode.conv.5.bias", channels);
    GET_CONV(weights.pre_encode.conv6_w, "enc.pre_encode.conv.6.weight", 1, 1, channels, channels);
    GET_F32(weights.pre_encode.conv6_b, "enc.pre_encode.conv.6.bias", channels);
    GET_LIN(weights.pre_encode.out_w, "enc.pre_encode.out.weight", pre_encode_in, d_model);
    GET_F32(weights.pre_encode.out_b, "enc.pre_encode.out.bias", d_model);

    // ----- encoder blocks -----
    weights.blocks.assign(hp.enc_n_layers, ParakeetBlock{});
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        auto & b = weights.blocks[i];

        // Macaron FF1.
        GET_F32(b.norm_ff1_w, lname("enc.blocks.%d.norm_ff1.weight", i), d_model);
        GET_F32(b.norm_ff1_b, lname("enc.blocks.%d.norm_ff1.bias", i), d_model);
        GET_LIN(b.ff1_lin1_w, lname("enc.blocks.%d.ff1.linear1.weight", i), d_model, d_ff);
        GET_LIN(b.ff1_lin2_w, lname("enc.blocks.%d.ff1.linear2.weight", i), d_ff, d_model);

        // Self-attention with relative position. Q/K/V/out biases only
        // when use_bias=true; linear_pos is bias-free even then.
        GET_F32(b.norm_attn_w, lname("enc.blocks.%d.norm_attn.weight", i), d_model);
        GET_F32(b.norm_attn_b, lname("enc.blocks.%d.norm_attn.bias", i), d_model);
        GET_LIN(b.attn_q_w, lname("enc.blocks.%d.attn.linear_q.weight", i), d_model, d_model);
        GET_LIN(b.attn_k_w, lname("enc.blocks.%d.attn.linear_k.weight", i), d_model, d_model);
        GET_LIN(b.attn_v_w, lname("enc.blocks.%d.attn.linear_v.weight", i), d_model, d_model);
        GET_LIN(b.attn_out_w, lname("enc.blocks.%d.attn.linear_out.weight", i), d_model, d_model);
        GET_LIN(b.attn_pos_w, lname("enc.blocks.%d.attn.linear_pos.weight", i), d_model, d_model);
        // pos_bias_u/v are added directly to a fp32 q tensor via
        // ggml_add — must stay fp32 to avoid a mixed-type broadcast.
        GET_F32(b.attn_pos_u, lname("enc.blocks.%d.attn.pos_bias_u", i), head_dim, n_heads);
        GET_F32(b.attn_pos_v, lname("enc.blocks.%d.attn.pos_bias_v", i), head_dim, n_heads);

        // Conv module. pointwise1 doubles the channel count for the
        // GLU split; depthwise has groups=d_model so its weight is
        // [kernel, 1, d_model] in ggml ne order. pointwise2 collapses
        // back to d_model.
        GET_F32(b.norm_conv_w, lname("enc.blocks.%d.norm_conv.weight", i), d_model);
        GET_F32(b.norm_conv_b, lname("enc.blocks.%d.norm_conv.bias", i), d_model);
        GET_CONV(b.conv_pw1_w, lname("enc.blocks.%d.conv.pointwise1.weight", i), 1, d_model, 2 * d_model);
        GET_CONV(b.conv_dw_w, lname("enc.blocks.%d.conv.depthwise.weight", i), k, 1, d_model);
        GET_CONV(b.conv_pw2_w, lname("enc.blocks.%d.conv.pointwise2.weight", i), 1, d_model, d_model);
        GET_F32(b.conv_bn_w, lname("enc.blocks.%d.conv.bn.weight", i), d_model);
        GET_F32(b.conv_bn_b, lname("enc.blocks.%d.conv.bn.bias", i), d_model);
        if (hp.enc_conv_norm_type == ParakeetHParams::ConvNormType::BatchNorm) {
            GET_F32(b.conv_bn_rm, lname("enc.blocks.%d.conv.bn.running_mean", i), d_model);
            GET_F32(b.conv_bn_rv, lname("enc.blocks.%d.conv.bn.running_var", i), d_model);
        }

        // Macaron FF2.
        GET_F32(b.norm_ff2_w, lname("enc.blocks.%d.norm_ff2.weight", i), d_model);
        GET_F32(b.norm_ff2_b, lname("enc.blocks.%d.norm_ff2.bias", i), d_model);
        GET_LIN(b.ff2_lin1_w, lname("enc.blocks.%d.ff2.linear1.weight", i), d_model, d_ff);
        GET_LIN(b.ff2_lin2_w, lname("enc.blocks.%d.ff2.linear2.weight", i), d_ff, d_model);

        // Final per-block layer norm.
        GET_F32(b.norm_out_w, lname("enc.blocks.%d.norm_out.weight", i), d_model);
        GET_F32(b.norm_out_b, lname("enc.blocks.%d.norm_out.bias", i), d_model);

        // Optional encoder linear/conv biases (use_bias=true only;
        // linear_pos has no bias slot).
        if (hp.enc_use_bias) {
            GET_F32(b.ff1_lin1_b, lname("enc.blocks.%d.ff1.linear1.bias", i), d_ff);
            GET_F32(b.ff1_lin2_b, lname("enc.blocks.%d.ff1.linear2.bias", i), d_model);
            GET_F32(b.attn_q_b, lname("enc.blocks.%d.attn.linear_q.bias", i), d_model);
            GET_F32(b.attn_k_b, lname("enc.blocks.%d.attn.linear_k.bias", i), d_model);
            GET_F32(b.attn_v_b, lname("enc.blocks.%d.attn.linear_v.bias", i), d_model);
            GET_F32(b.attn_out_b, lname("enc.blocks.%d.attn.linear_out.bias", i), d_model);
            GET_F32(b.conv_pw1_b, lname("enc.blocks.%d.conv.pointwise1.bias", i), 2 * d_model);
            GET_F32(b.conv_dw_b, lname("enc.blocks.%d.conv.depthwise.bias", i), d_model);
            GET_F32(b.conv_pw2_b, lname("enc.blocks.%d.conv.pointwise2.bias", i), d_model);
            GET_F32(b.ff2_lin1_b, lname("enc.blocks.%d.ff2.linear1.bias", i), d_ff);
            GET_F32(b.ff2_lin2_b, lname("enc.blocks.%d.ff2.linear2.bias", i), d_model);
        }
    }

    if (hp.head_kind == HeadKind::CTC) {
        // CTC head: single 1×1 Conv1d projecting d_model -> vocab+1.
        // PyTorch shape [vocab+1, d_model, 1] → ggml ne [1, d_model,
        // vocab+1]. vocab+1 isn't in the hparams (CTC GGUFs ship neither
        // predictor.vocab nor joint_n_classes), so peek at the raw tensor
        // for the trailing dim, then run find_tensor with the full shape.
        ggml_tensor * ctc_peek = ggml_get_tensor(ctx_meta, "head.ctc.weight");
        if (ctc_peek == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: missing tensor \"head.ctc.weight\"");
            return TRANSCRIBE_ERR_GGUF;
        }
        const int64_t n_classes = ctc_peek->ne[2];
        if (n_classes <= 1) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "parakeet: head.ctc.weight vocab+1 (=%lld) must be > 1",
                    (long long) n_classes);
            return TRANSCRIBE_ERR_GGUF;
        }
        GET_LIN(weights.ctc_head.weight, "head.ctc.weight", 1, d_model, n_classes);
        GET_F32(weights.ctc_head.bias, "head.ctc.bias", n_classes);
        // Stash the resolved vocab+1 into hp. const_cast is safe: the
        // loader is the single producer of hp, writable while loading.
        const_cast<ParakeetHParams &>(hp).head_ctc_n_classes = static_cast<int32_t>(n_classes);
    } else {
        // ----- predictor (TDT, RNNT) -----
        GET_LIN(weights.predictor.embed_w, "pred.embed.weight", pred_h, pred_v);

        weights.predictor.lstm.assign(hp.pred_n_layers, ParakeetPredictor::LstmLayer{});
        for (int i = 0; i < hp.pred_n_layers; ++i) {
            auto &        l     = weights.predictor.lstm[i];
            // Wx/Wh both project from pred_hidden; 4*hidden output =
            // concatenated (i, f, g, o) gates.
            const int64_t gates = 4 * pred_h;
            GET_LIN(l.Wx, lname("pred.lstm.%d.Wx", i), pred_h, gates);
            GET_LIN(l.Wh, lname("pred.lstm.%d.Wh", i), pred_h, gates);
            GET_F32(l.b, lname("pred.lstm.%d.bias", i), gates);
        }

        // ----- joint (TDT, RNNT) -----
        GET_LIN(weights.joint.enc_w, "joint.enc.weight", d_model, joint_h);
        GET_F32(weights.joint.enc_b, "joint.enc.bias", joint_h);
        GET_LIN(weights.joint.pred_w, "joint.pred.weight", pred_h, joint_h);
        GET_F32(weights.joint.pred_b, "joint.pred.bias", joint_h);
        GET_LIN(weights.joint.out_w, "joint.out.weight", joint_h, joint_n);
        GET_F32(weights.joint.out_b, "joint.out.bias", joint_n);
    }

    // ----- prompt MLP (multilingual variants, has_prompt only) -----
    // PyTorch nn.Sequential indexing preserved (.0 input, .2 output).
    if (hp.has_prompt) {
        const int64_t prompt_h = hp.prompt_hidden;
        const int64_t in_dim   = static_cast<int64_t>(d_model) + static_cast<int64_t>(hp.prompt_num_prompts);
        GET_LIN(weights.prompt.mlp0_w, "prompt.mlp.0.weight", in_dim, prompt_h);
        GET_F32(weights.prompt.mlp0_b, "prompt.mlp.0.bias", prompt_h);
        GET_LIN(weights.prompt.mlp2_w, "prompt.mlp.2.weight", prompt_h, d_model);
        GET_F32(weights.prompt.mlp2_b, "prompt.mlp.2.bias", d_model);
    }

    return TRANSCRIBE_OK;
}

#undef GET_F32
#undef GET_CONV
#undef GET_LIN

}  // namespace transcribe::parakeet
