// arch/whisper/bin_load.cpp - legacy whisper.cpp .bin adapter (see bin_load.h
// for the contract). Synthesizes hparams, capabilities, language list, special
// token ids, tokenizer, and frontend buffers from a parsed WhisperBinModel,
// then populates a canonical-named ctx_meta the byte-streaming step fills.

#include "bin_load.h"

#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "transcribe-arch.h"
#include "transcribe-bin-loader.h"
#include "transcribe-load-common.h"
#include "transcribe-log.h"
#include "transcribe-tokenizer.h"
#include "weights.h"
#include "whisper.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

namespace transcribe::whisper {

extern const Arch arch;  // defined in arch/whisper/model.cpp

namespace {

constexpr const char * kTag = "whisper-bin";

// Cosmetic variant string from .bin geometry, for diagnostic logging only
// (parity with the stt.variant a GGUF would carry). Doesn't gate runtime.
std::string detect_variant(const transcribe::bin_loader::WhisperBinModel & bm) {
    const auto & h = bm.hp;
    std::string  base;
    switch (h.n_audio_layer) {
        case 4:
            base = "whisper-tiny";
            break;
        case 6:
            base = "whisper-base";
            break;
        case 12:
            base = "whisper-small";
            break;
        case 24:
            base = "whisper-medium";
            break;
        case 32:
            if (h.n_text_layer == 4) {
                base = "whisper-large-v3-turbo";
            } else if (h.n_mels == 128) {
                base = "whisper-large-v3";
            } else {
                base = "whisper-large";
            }
            break;
        default:
            base = "whisper";
            break;
    }
    if (!bm.is_multilingual) {
        base += ".en";
    }
    return base;
}

// Canonical Whisper language table (verbatim from whisper.cpp). Index =
// whisper language id; multilingual variants install the first num_languages.
const char * const k_whisper_lang_codes[] = {
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt",  "tr", "pl", "ca", "nl", "ar", "sv",  "it", "id",
    "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro",  "da", "hu", "ta", "no", "th", "ur",  "hr", "bg",
    "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv",  "bn", "sr", "az", "sl", "kn", "et",  "mk", "br",
    "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw",  "gl", "mr", "pa", "si", "km", "sn",  "yo", "so",
    "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi",  "lo", "uz", "fo", "ht", "ps", "tk",  "nn", "mt",
    "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha", "ba", "jw", "su", "yue",
};
constexpr int k_whisper_n_lang_codes = static_cast<int>(sizeof(k_whisper_lang_codes) / sizeof(k_whisper_lang_codes[0]));

// Special token computation (mirrors whisper.cpp). Defaults are the .en /
// multilingual-base layout; for is_multilingual eot/sot shift +1 and the
// downstream tokens shift by num_languages - 98.
struct WhisperSpecials {
    int eot          = 50256;
    int sot          = 50257;
    int translate    = 50357;
    int transcribe   = 50358;
    int solm         = 50359;
    int prev         = 50360;
    int nosp         = 50361;
    int notimestamps = 50362;  // <|notimestamps|>
    int beg          = 50363;  // <|0.00|>, first timestamp token
};

WhisperSpecials compute_specials(int n_vocab) {
    WhisperSpecials s;
    const bool      is_multilingual = n_vocab >= 51865;
    if (is_multilingual) {
        s.eot += 1;
        s.sot += 1;
        const int num_languages = n_vocab - 51765 - 1;
        const int dt            = num_languages - 98;
        s.translate += dt;
        s.transcribe += dt;
        s.solm += dt;
        s.prev += dt;
        s.nosp += dt;
        s.notimestamps += dt;
        s.beg += dt;
    }
    return s;
}

// HF generation_config.suppress_tokens for whisper (~88 ids never emitted in
// transcripts). Hardcoded here because the .bin doesn't store it; source is HF
// generation_config.json (identical across variants; suppressing ids absent in
// the smaller .en vocab is a no-op).
const int32_t k_whisper_suppress_tokens[] = {
    1,     2,     7,     8,     9,     10,    14,    25,    26,    27,    28,    29,    31,    58,    59,
    60,    61,    62,    63,    90,    91,    92,    93,    359,   503,   522,   542,   873,   893,   902,
    918,   922,   931,   1350,  1853,  1982,  2460,  2627,  3246,  3253,  3268,  3536,  3846,  3961,  4183,
    4667,  6585,  6647,  7273,  9061,  9383,  10428, 10929, 11938, 12033, 12331, 12562, 13793, 14157, 14635,
    15265, 15618, 16553, 16604, 18362, 18956, 20075, 21675, 22520, 26130, 26161, 26435, 28279, 29464, 31650,
    32302, 32470, 36865, 42863, 47425, 49870, 50254, 50258, 50358, 50359, 50360, 50361, 50362,
};
constexpr int k_whisper_n_suppress =
    static_cast<int>(sizeof(k_whisper_suppress_tokens) / sizeof(k_whisper_suppress_tokens[0]));

// Tensor rename rules: legacy whisper.cpp name -> canonical transcribe.cpp
// name. collapse_to_1d drops the size-1 dim whisper.cpp uses for conv biases.
// Layered block tensors are expanded against n_audio_layer / n_text_layer.
struct StaticRename {
    const char * legacy;
    const char * canonical;
    bool         collapse_to_1d;  // whisper.cpp conv biases: 2D [1,d_model] → 1D [d_model]
};

const StaticRename k_static_renames[] = {
    { "encoder.positional_embedding",   "enc.pos_emb.weight",    false },
    { "encoder.conv1.weight",           "enc.conv.0.weight",     false },
    { "encoder.conv1.bias",             "enc.conv.0.bias",       true  },
    { "encoder.conv2.weight",           "enc.conv.1.weight",     false },
    { "encoder.conv2.bias",             "enc.conv.1.bias",       true  },
    { "encoder.ln_post.weight",         "enc.final_norm.weight", false },
    { "encoder.ln_post.bias",           "enc.final_norm.bias",   false },

    { "decoder.positional_embedding",   "dec.pos_emb.weight",    false },
    { "decoder.token_embedding.weight", "dec.token_embd.weight", false },
    { "decoder.ln.weight",              "dec.final_norm.weight", false },
    { "decoder.ln.bias",                "dec.final_norm.bias",   false },
};

struct LayeredRename {
    const char * legacy_fmt;
    const char * canonical_fmt;
};

const LayeredRename k_enc_block_renames[] = {
    { "encoder.blocks.%d.attn_ln.weight",    "enc.blocks.%d.norm_attn.weight" },
    { "encoder.blocks.%d.attn_ln.bias",      "enc.blocks.%d.norm_attn.bias"   },
    { "encoder.blocks.%d.attn.query.weight", "enc.blocks.%d.attn.q.weight"    },
    { "encoder.blocks.%d.attn.query.bias",   "enc.blocks.%d.attn.q.bias"      },
    { "encoder.blocks.%d.attn.key.weight",   "enc.blocks.%d.attn.k.weight"    }, // no bias
    { "encoder.blocks.%d.attn.value.weight", "enc.blocks.%d.attn.v.weight"    },
    { "encoder.blocks.%d.attn.value.bias",   "enc.blocks.%d.attn.v.bias"      },
    { "encoder.blocks.%d.attn.out.weight",   "enc.blocks.%d.attn.out.weight"  },
    { "encoder.blocks.%d.attn.out.bias",     "enc.blocks.%d.attn.out.bias"    },
    { "encoder.blocks.%d.mlp_ln.weight",     "enc.blocks.%d.norm_ffn.weight"  },
    { "encoder.blocks.%d.mlp_ln.bias",       "enc.blocks.%d.norm_ffn.bias"    },
    { "encoder.blocks.%d.mlp.0.weight",      "enc.blocks.%d.ffn.fc1.weight"   },
    { "encoder.blocks.%d.mlp.0.bias",        "enc.blocks.%d.ffn.fc1.bias"     },
    { "encoder.blocks.%d.mlp.2.weight",      "enc.blocks.%d.ffn.fc2.weight"   },
    { "encoder.blocks.%d.mlp.2.bias",        "enc.blocks.%d.ffn.fc2.bias"     },
};

const LayeredRename k_dec_block_renames[] = {
    { "decoder.blocks.%d.attn_ln.weight",          "dec.blocks.%d.norm_self.weight"      },
    { "decoder.blocks.%d.attn_ln.bias",            "dec.blocks.%d.norm_self.bias"        },
    { "decoder.blocks.%d.attn.query.weight",       "dec.blocks.%d.self_attn.q.weight"    },
    { "decoder.blocks.%d.attn.query.bias",         "dec.blocks.%d.self_attn.q.bias"      },
    { "decoder.blocks.%d.attn.key.weight",         "dec.blocks.%d.self_attn.k.weight"    },
    { "decoder.blocks.%d.attn.value.weight",       "dec.blocks.%d.self_attn.v.weight"    },
    { "decoder.blocks.%d.attn.value.bias",         "dec.blocks.%d.self_attn.v.bias"      },
    { "decoder.blocks.%d.attn.out.weight",         "dec.blocks.%d.self_attn.out.weight"  },
    { "decoder.blocks.%d.attn.out.bias",           "dec.blocks.%d.self_attn.out.bias"    },
    { "decoder.blocks.%d.cross_attn_ln.weight",    "dec.blocks.%d.norm_cross.weight"     },
    { "decoder.blocks.%d.cross_attn_ln.bias",      "dec.blocks.%d.norm_cross.bias"       },
    { "decoder.blocks.%d.cross_attn.query.weight", "dec.blocks.%d.cross_attn.q.weight"   },
    { "decoder.blocks.%d.cross_attn.query.bias",   "dec.blocks.%d.cross_attn.q.bias"     },
    { "decoder.blocks.%d.cross_attn.key.weight",   "dec.blocks.%d.cross_attn.k.weight"   },
    { "decoder.blocks.%d.cross_attn.value.weight", "dec.blocks.%d.cross_attn.v.weight"   },
    { "decoder.blocks.%d.cross_attn.value.bias",   "dec.blocks.%d.cross_attn.v.bias"     },
    { "decoder.blocks.%d.cross_attn.out.weight",   "dec.blocks.%d.cross_attn.out.weight" },
    { "decoder.blocks.%d.cross_attn.out.bias",     "dec.blocks.%d.cross_attn.out.bias"   },
    { "decoder.blocks.%d.mlp_ln.weight",           "dec.blocks.%d.norm_ffn.weight"       },
    { "decoder.blocks.%d.mlp_ln.bias",             "dec.blocks.%d.norm_ffn.bias"         },
    { "decoder.blocks.%d.mlp.0.weight",            "dec.blocks.%d.ffn.fc1.weight"        },
    { "decoder.blocks.%d.mlp.0.bias",              "dec.blocks.%d.ffn.fc1.bias"          },
    { "decoder.blocks.%d.mlp.2.weight",            "dec.blocks.%d.ffn.fc2.weight"        },
    { "decoder.blocks.%d.mlp.2.bias",              "dec.blocks.%d.ffn.fc2.bias"          },
};

std::string fmt1(const char * fmt, int idx) {
    char buf[160];
    std::snprintf(buf, sizeof(buf), fmt, idx);
    return std::string(buf);
}

// Periodic Hann window of length n_fft: w[k] = 0.5*(1 - cos(2*pi*k/N)),
// k=0..N-1 (no division by N-1). .bin files don't carry the window, so compute
// it here (as whisper.cpp does) for install_mel_from_buffers.
std::vector<float> hann_periodic(int n_fft) {
    std::vector<float> w(static_cast<size_t>(n_fft));
    const double       two_pi_over_n = 2.0 * M_PI / static_cast<double>(n_fft);
    for (int k = 0; k < n_fft; ++k) {
        w[static_cast<size_t>(k)] = static_cast<float>(0.5 * (1.0 - std::cos(two_pi_over_n * k)));
    }
    return w;
}

// Synthesize a WhisperHParams from the parsed bin model. Every field the GGUF
// path reads from KVs gets a hardcoded value matching the shipped variants.
void fill_whisper_hparams(const transcribe::bin_loader::WhisperBinModel & bm, WhisperHParams & hp) {
    hp = WhisperHParams{};

    // Encoder.
    hp.enc_n_layers             = bm.hp.n_audio_layer;
    hp.enc_d_model              = bm.hp.n_audio_state;
    hp.enc_n_heads              = bm.hp.n_audio_head;
    hp.enc_ffn_dim              = 4 * bm.hp.n_audio_state;
    hp.enc_num_mel_bins         = bm.hp.n_mels;
    hp.enc_max_source_positions = bm.hp.n_audio_ctx;
    hp.enc_activation           = "gelu";

    // Decoder.
    hp.dec_n_layers             = bm.hp.n_text_layer;
    hp.dec_d_model              = bm.hp.n_text_state;
    hp.dec_n_heads              = bm.hp.n_text_head;
    hp.dec_ffn_dim              = 4 * bm.hp.n_text_state;
    hp.dec_max_target_positions = bm.hp.n_text_ctx;
    hp.dec_vocab_size           = bm.hp.n_vocab;
    hp.dec_activation           = "gelu";
    hp.dec_tie_word_embeddings  = true;
    hp.dec_scale_embedding      = false;

    // Special tokens.
    const WhisperSpecials sp  = compute_specials(bm.hp.n_vocab);
    hp.decoder_start_token_id = sp.sot;
    hp.sot_token_id           = sp.sot;
    hp.no_timestamps_token_id = sp.notimestamps;
    hp.transcribe_token_id    = bm.is_multilingual ? sp.transcribe : -1;
    hp.translate_token_id     = bm.is_multilingual ? sp.translate : -1;
    hp.prev_sot_token_id      = sp.prev;

    // Suppression list — same across all multilingual whisper.cpp .bin
    // variants. For .en, suppressing ids that don't exist in the
    // smaller vocab is a no-op; the runtime check `id < vocab_size`
    // gates application.
    hp.suppress_tokens.assign(k_whisper_suppress_tokens, k_whisper_suppress_tokens + k_whisper_n_suppress);
    hp.begin_suppress_tokens.assign({ 220, sp.eot });

    // Frontend (fixed across whisper variants — no .bin field carries
    // these so we use the canonical preprocessor_config.json values).
    hp.fe_type          = "mel";
    hp.fe_num_mels      = bm.hp.n_mels;
    hp.fe_sample_rate   = 16000;
    hp.fe_n_fft         = 400;
    hp.fe_win_length    = 400;
    hp.fe_hop_length    = 160;
    hp.fe_window        = "hann_periodic";
    hp.fe_normalize     = "whisper_logmel";
    hp.fe_dither        = 0.0f;
    hp.fe_pre_emphasis  = 0.0f;
    hp.fe_f_min         = 0.0f;
    hp.fe_f_max         = 8000.0f;
    hp.fe_pad_mode      = "reflect";
    hp.fe_center        = true;
    hp.fe_mel_norm      = "slaney";
    hp.fe_chunk_length  = 30;
    hp.fe_n_samples     = 480000;
    hp.fe_nb_max_frames = 3000;

    hp.cap_lang_detect = bm.is_multilingual;
    hp.cap_translate   = bm.is_multilingual;
    hp.cap_timestamps  = true;
}

// Resolve every canonical tensor slot from the parsed bin model: look up each
// legacy name, create a ggml_tensor in ctx_meta with the canonical name + ne +
// source type, and record (target, src_offset, src_nbytes) into out_slots for
// the byte-streaming step. Validates that every slot is filled and no source
// tensor is left unconsumed.
ggml_tensor * create_canonical_tensor(ggml_context *                                 ctx_meta,
                                      const std::string &                            canonical_name,
                                      const transcribe::bin_loader::BinTensorEntry & src,
                                      bool                                           collapse_to_1d) {
    int     n_dims = src.n_dims;
    int64_t ne[GGML_MAX_DIMS];
    for (int i = 0; i < GGML_MAX_DIMS; ++i) {
        ne[i] = 1;
    }
    for (int i = 0; i < n_dims; ++i) {
        ne[i] = src.ne[i];
    }

    if (collapse_to_1d) {
        // whisper.cpp conv biases arrive as ne [1, d_model, 1, 1]; the byte
        // payload is d_model contiguous floats, so collapse to 1D ne [d_model].
        int64_t total = 1;
        for (int i = 0; i < n_dims; ++i) {
            total *= ne[i];
        }
        for (int i = 0; i < GGML_MAX_DIMS; ++i) {
            ne[i] = 1;
        }
        ne[0]  = total;
        n_dims = 1;
    }

    ggml_tensor * t = ggml_new_tensor(ctx_meta, src.type, n_dims, ne);
    if (t == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: ggml_new_tensor failed for \"%s\"", kTag, canonical_name.c_str());
        return nullptr;
    }
    ggml_set_name(t, canonical_name.c_str());
    return t;
}

transcribe_status resolve_tensors(const transcribe::bin_loader::WhisperBinModel &      bm,
                                  const WhisperHParams &                               hp,
                                  ggml_context *                                       ctx_meta,
                                  std::vector<transcribe::bin_loader::BinStreamSlot> & out_slots) {
    // Forward index legacy_name → entry pointer. Faster than scanning
    // bm.tensors per slot, and lets us flag stray entries cheaply.
    std::unordered_map<std::string, const transcribe::bin_loader::BinTensorEntry *> by_name;
    by_name.reserve(bm.tensors.size() * 2);
    for (const auto & e : bm.tensors) {
        if (!by_name.emplace(e.name, &e).second) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: duplicate tensor \"%s\" in .bin", kTag, e.name.c_str());
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    std::unordered_set<std::string> consumed;
    consumed.reserve(by_name.size() * 2);

    // frontend.mel_filterbank / frontend.window aren't .bin tensors (the
    // filterbank is a header field, the window is computed at load). Create
    // empty F32 slots to satisfy build_whisper_weights; bytes are filled
    // post-backend-alloc below.
    {
        const int64_t n_fft_half = static_cast<int64_t>(hp.fe_n_fft / 2 + 1);
        const int64_t fb_ne[2]   = { n_fft_half, static_cast<int64_t>(hp.fe_num_mels) };
        ggml_tensor * fb         = ggml_new_tensor(ctx_meta, GGML_TYPE_F32, 2, fb_ne);
        if (fb == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_set_name(fb, "frontend.mel_filterbank");

        const int64_t win_ne[1] = { static_cast<int64_t>(hp.fe_n_fft) };
        ggml_tensor * win       = ggml_new_tensor(ctx_meta, GGML_TYPE_F32, 1, win_ne);
        if (win == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_set_name(win, "frontend.window");
    }

    auto add_slot = [&](const StaticRename & rule) -> transcribe_status {
        auto it = by_name.find(rule.legacy);
        if (it == by_name.end()) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "%s: missing tensor \"%s\" (canonical \"%s\") "
                    "— .bin is incomplete or not a whisper model",
                    kTag, rule.legacy, rule.canonical);
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_tensor * t = create_canonical_tensor(ctx_meta, rule.canonical, *it->second, rule.collapse_to_1d);
        if (t == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        out_slots.push_back({ t, it->second->offset, it->second->nbytes });
        consumed.insert(rule.legacy);
        return TRANSCRIBE_OK;
    };
    auto add_slot_layered = [&](const LayeredRename & rule, int idx) -> transcribe_status {
        StaticRename      s;
        const std::string legacy_s    = fmt1(rule.legacy_fmt, idx);
        const std::string canonical_s = fmt1(rule.canonical_fmt, idx);
        s.legacy                      = legacy_s.c_str();
        s.canonical                   = canonical_s.c_str();
        s.collapse_to_1d              = false;
        return add_slot(s);
    };

    // Top-level slots.
    for (const auto & r : k_static_renames) {
        if (auto st = add_slot(r); st != TRANSCRIBE_OK) {
            return st;
        }
    }
    // Encoder blocks.
    for (int i = 0; i < hp.enc_n_layers; ++i) {
        for (const auto & r : k_enc_block_renames) {
            if (auto st = add_slot_layered(r, i); st != TRANSCRIBE_OK) {
                return st;
            }
        }
    }
    // Decoder blocks.
    for (int i = 0; i < hp.dec_n_layers; ++i) {
        for (const auto & r : k_dec_block_renames) {
            if (auto st = add_slot_layered(r, i); st != TRANSCRIBE_OK) {
                return st;
            }
        }
    }

    // Stray-tensor check: every .bin entry must be consumed. A stray entry
    // usually means an unsupported extension (e.g. tinydiarize's solm head);
    // hard-error rather than silently drop load-bearing weights.
    if (consumed.size() != bm.tensors.size()) {
        for (const auto & e : bm.tensors) {
            if (consumed.find(e.name) == consumed.end()) {
                log_msg(TRANSCRIBE_LOG_LEVEL_INFO,
                        "%s: unconsumed .bin tensor \"%s\" — "
                        "this loader does not yet support it",
                        kTag, e.name.c_str());
            }
        }
        return TRANSCRIBE_ERR_GGUF;
    }

    return TRANSCRIBE_OK;
}

// Capabilities + language list synthesis. Mirrors apply_family_invariants +
// read_capability_kv + read_languages_kv for the GGUF path, but from inferred
// values since there are no GGUF KVs. lang_codes (owned on the model) backs the
// caps.languages pointer table; its lifetime matches the WhisperModel.
void apply_caps_and_languages(WhisperModel & m, const transcribe::bin_loader::WhisperBinModel & bm) {
    apply_family_invariants(m);

    if (!bm.is_multilingual) {
        m.caps.supports_language_detect = false;
        m.caps.supports_translate       = false;
    }
    // timestamps / streaming / initial_prompt etc. inherited from
    // family invariants; matches multilingual GGUFs.

    // Build the language list.
    m.lang_codes.clear();
    if (bm.is_multilingual) {
        const int n = std::min(bm.num_languages, k_whisper_n_lang_codes);
        m.lang_codes.reserve(static_cast<size_t>(n));
        for (int i = 0; i < n; ++i) {
            m.lang_codes.emplace_back(k_whisper_lang_codes[i]);
        }
    } else {
        m.lang_codes.emplace_back("en");
    }
    // lang_token_ids by ID arithmetic: whisper packs language tokens
    // contiguously after <|sot|> (lang_id i -> sot + 1 + i). The .bin vocab
    // doesn't carry the "<|en|>" strings, so tokenizer.find() can't be used.
    m.lang_token_ids.clear();
    if (bm.is_multilingual) {
        const WhisperSpecials sp = compute_specials(bm.hp.n_vocab);
        m.lang_token_ids.reserve(m.lang_codes.size());
        for (size_t i = 0; i < m.lang_codes.size(); ++i) {
            m.lang_token_ids.push_back(static_cast<int32_t>(sp.sot + 1 + static_cast<int>(i)));
        }
    }

    // Publish to the capability surface so transcribe_get_model_capabilities()
    // ->languages/->n_languages matches the GGUF path.
    m.set_languages(m.lang_codes);
}

// Build the Tokenizer via the raw-bytes loader (the .bin vocab is
// tiktoken-style: raw UTF-8, no GPT-2 byte-to-unicode remap).
//
// The .bin stores only the ~50256 base pieces, so synthesize the canonical
// special-token literals ("<|en|>", "<|notimestamps|>", "<|0.00|>", ...) into
// the special-piece map (as whisper.cpp does). Without them the run path's
// initial_prompt rejection check would miss such literals in user text and
// byte-encode them as context noise instead of rejecting symmetrically.
transcribe_status install_tokenizer(WhisperModel & m, const transcribe::bin_loader::WhisperBinModel & bm) {
    transcribe::Tokenizer::DecodeOnlySpecials sp;
    const WhisperSpecials                     s = compute_specials(bm.hp.n_vocab);
    sp.eos_id                                   = s.eot;
    sp.bos_id                                   = s.eot;
    if (auto st = m.tok.load_decode_only_raw_bytes(bm.vocab_tokens, sp); st != TRANSCRIBE_OK) {
        return st;
    }

    // Discrete specials that exist in every Whisper variant.
    m.tok.add_special_piece("<|endoftext|>", s.eot);
    m.tok.add_special_piece("<|startoftranscript|>", s.sot);
    m.tok.add_special_piece("<|startofprev|>", s.prev);
    m.tok.add_special_piece("<|nospeech|>", s.nosp);
    m.tok.add_special_piece("<|notimestamps|>", s.notimestamps);

    // Multilingual-only specials: language tokens + task tokens.
    if (bm.is_multilingual) {
        m.tok.add_special_piece("<|translate|>", s.translate);
        m.tok.add_special_piece("<|transcribe|>", s.transcribe);
        const int n_lang = std::min(bm.num_languages, k_whisper_n_lang_codes);
        for (int i = 0; i < n_lang; ++i) {
            std::string lit = std::string("<|") + k_whisper_lang_codes[i] + "|>";
            m.tok.add_special_piece(lit, s.sot + 1 + i);
        }
    }

    // 1501 timestamp tokens <|0.00|>..<|30.00|> at ids beg+0..beg+1500. Format
    // matches HF's added_tokens convention so such literals are detected.
    char tsbuf[16];
    for (int i = 0; i <= 1500; ++i) {
        std::snprintf(tsbuf, sizeof(tsbuf), "<|%.2f|>", static_cast<double>(i) * 0.02);
        m.tok.add_special_piece(tsbuf, s.beg + i);
    }
    return TRANSCRIBE_OK;
}

}  // namespace

transcribe_status load_from_bin(const char *                                path,
                                const struct transcribe_model_load_params * params,
                                struct transcribe_model **                  out_model) {
    if (out_model == nullptr || path == nullptr || params == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    *out_model = nullptr;

    const int64_t t_load_start = ggml_time_us();

    // ---- Parse the .bin (metadata only) ----
    transcribe::bin_loader::WhisperBinModel bm;
    if (auto st = transcribe::bin_loader::parse_whisper_bin(path, bm); st != TRANSCRIBE_OK) {
        return st;
    }

    auto m       = std::make_unique<WhisperModel>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = detect_variant(bm);
    m->backend.clear();

    // ---- Synthesize hparams ----
    fill_whisper_hparams(bm, m->hparams);

    // ---- Tokenizer (decode-only raw bytes) ----
    if (auto st = install_tokenizer(*m, bm); st != TRANSCRIBE_OK) {
        return st;
    }

    // ---- Capabilities + language list ----
    apply_caps_and_languages(*m, bm);

    // ---- ctx_meta + tensor catalog ----
    // Sized like the GGUF path: roughly n_tensors * tensor_overhead,
    // with slack. Whisper-tiny has ~167 tensors, large-v3 ~1200.
    const size_t n_tensors_est =
        static_cast<size_t>(15 + 15 * m->hparams.enc_n_layers + 24 * m->hparams.dec_n_layers + 32);
    ggml_init_params init_params{};
    init_params.mem_size   = (n_tensors_est + 64) * ggml_tensor_overhead();
    init_params.mem_buffer = nullptr;
    init_params.no_alloc   = true;
    m->ctx_meta            = ggml_init(init_params);
    if (m->ctx_meta == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    std::vector<transcribe::bin_loader::BinStreamSlot> stream_slots;
    stream_slots.reserve(bm.tensors.size());
    if (auto st = resolve_tensors(bm, m->hparams, m->ctx_meta, stream_slots); st != TRANSCRIBE_OK) {
        return st;
    }

    // ---- Validate via the canonical catalog ----
    // Now that ctx_meta holds tensors with canonical names + post-
    // squeeze ne, the standard build_whisper_weights pass succeeds
    // identically to the GGUF path.
    if (auto st = build_whisper_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        return st;
    }

    // ---- Backend plan ----
    if (auto st = transcribe::load_common::init_backends(params->backend, params->gpu_device, "whisper", m->plan);
        st != TRANSCRIBE_OK) {
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    // ---- Allocate backend buffer ----
    ggml_backend_buffer_t buf = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (buf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "%s: ggml_backend_alloc_ctx_tensors failed", kTag);
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = buf;
    ggml_backend_buffer_set_usage(buf, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    // ---- Stream tensor bytes from the .bin ----
    if (auto st = transcribe::bin_loader::stream_tensor_data_from_bin(bm.path, stream_slots, "whisper");
        st != TRANSCRIBE_OK) {
        return st;
    }

    // ---- Frontend: fill the catalog tensors and install MelFrontend ----
    {
        std::vector<float> filterbank = bm.mel_filterbank;  // copy
        std::vector<float> window     = hann_periodic(m->hparams.fe_n_fft);

        // weights.frontend.* are populated for catalog completeness only (the
        // runtime mel pipeline reads from m->mel). The size checks are defense
        // in depth against a future parser regression causing a backend OOB.
        if (m->weights.frontend.mel_filterbank != nullptr) {
            const size_t want = ggml_nbytes(m->weights.frontend.mel_filterbank);
            const size_t have = filterbank.size() * sizeof(float);
            if (have != want) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "%s: mel filterbank size mismatch "
                        "(have %zu bytes, expected %zu)",
                        kTag, have, want);
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_tensor_set(m->weights.frontend.mel_filterbank, filterbank.data(), 0, have);
        }
        if (m->weights.frontend.window != nullptr) {
            const size_t want = ggml_nbytes(m->weights.frontend.window);
            const size_t have = window.size() * sizeof(float);
            if (have != want) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "%s: hann window size mismatch "
                        "(have %zu bytes, expected %zu)",
                        kTag, have, want);
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_tensor_set(m->weights.frontend.window, window.data(), 0, have);
        }

        if (auto st = install_mel_from_buffers(m->hparams, std::move(filterbank), std::move(window), m->mel);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    m->t_load_us = ggml_time_us() - t_load_start;
    *out_model   = m.release();
    return TRANSCRIBE_OK;
}

}  // namespace transcribe::whisper
