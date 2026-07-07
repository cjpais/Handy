// arch/voxtral/model.cpp - Voxtral (2507) family handler.
//
// audio-llm: Whisper-large-v3 encoder + 4x frame-group projector + a
// Llama/Ministral causal LM with audio-token injection. Two prompt modes share
// the same encoder/projector/decoder:
//
//   transcription : [BOS][INST][BEGIN_AUDIO][AUDIO]*N[/INST](lang:<l>)?[TRANSCRIBE]
//   instruct      : [BOS][INST][BEGIN_AUDIO][AUDIO]*N BPE(instruction)[/INST]
//
// Instruct mode covers translation (task TRANSLATE synthesizes "Translate this
// to {Language}.") and free-text prompting.

#include "causal_lm/causal_lm.h"
#include "decoder.h"
#include "encoder.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-flash-policy.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "voxtral.h"
#include "weights.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::voxtral {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, VoxtralModel>);
static_assert(std::is_base_of_v<transcribe_session, VoxtralSession>);

VoxtralSession::~VoxtralSession() {
    kv_cache.free();
    kv_cache_batch.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

VoxtralModel::~VoxtralModel() {
    if (ctx_meta != nullptr) {
        ggml_free(ctx_meta);
        ctx_meta = nullptr;
    }
    if (backend_buffer != nullptr) {
        safe_buffer_free(backend_buffer);
        backend_buffer = nullptr;
    }
    packed_gate_up.free();
    for (auto it = plan.scheduler_list.rbegin(); it != plan.scheduler_list.rend(); ++it) {
        safe_backend_free(*it);
    }
    plan.scheduler_list.clear();
    plan.primary      = nullptr;
    plan.primary_kind = transcribe::BackendKind::Unknown;
}

namespace {

constexpr const char k_default_variant[] = "voxtral-mini-3b-2507";

// Floor on the decoder text budget for short clips (also Whisper's per-chunk
// cap). Long audio scales the budget up with the audio length — see run().
constexpr int k_decode_budget_min = 448;

// Decode budget (max new text tokens) for an utterance with `n_audio` audio
// embedding tokens. Speech yields fewer text tokens than audio frames, so the
// audio token count is a safe upper bound; clamp to the context remaining under
// the trained max so prompt+decode fits. Greedy decode stops at EOS well before
// this, so a generous ceiling costs only its KV allocation.
int pick_decode_budget(int n_audio, int t_prompt, int model_max) {
    int       budget = std::max(k_decode_budget_min, n_audio);
    const int room   = model_max - t_prompt;
    if (budget > room) {
        budget = room;
    }
    return budget;
}

// Pick a KV-cache context length that fits `needed` (prompt + decode budget).
// Rounds up to a power of two (so the step graph's `max_n_kv`, computed the same
// way, never exceeds the allocation) and clamps to the trained
// max_position_embeddings — the real ceiling, since RoPE is only valid there.
int pick_kv_ctx(int needed, int model_max) {
    int want = 1024;
    while (want < needed) {
        want *= 2;
    }
    if (want > model_max) {
        want = model_max;
    }
    return want;
}

// Input-length contract (see docs/input-limits.md). Hard-context-cap family:
// audio tokens + prompt + transcript share the decoder context window
// (dec_max_position_embeddings); the Whisper encoder chunks audio into 30 s
// windows, so the decoder context is the binding limit. Over-length input is
// rejected with TRANSCRIBE_ERR_INPUT_TOO_LONG; a transcript that fills the
// generation budget before EOS is flagged via transcribe_was_truncated().

// Generation room the up-front gate reserves, in tokens (the pick_decode_budget
// floor): an accepted clip is always guaranteed at least this many output tokens.
constexpr int k_gen_reserve = k_decode_budget_min;

// Effective decoder context ceiling, in tokens: the model's trained maximum,
// optionally lowered — never raised — by the caller's session n_ctx knob.
int voxtral_context_ceiling(int32_t n_ctx_knob, const VoxtralHParams & hp) {
    int ceiling = hp.dec_max_position_embeddings;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose audio
// tokens + prompt + generation reserve still fit the context ceiling. The
// encoder rate is fixed by the 30 s chunk geometry (ms-per-audio-token =
// chunk_ms / tokens_per_chunk). Returns 0 (unbounded) if the rate hparams are
// missing, so a misconfigured model is never advertised with a wrong number.
int64_t voxtral_max_audio_ms(const VoxtralHParams & hp) {
    const int tokens_per_chunk = hp.audio_tokens_per_chunk();
    if (tokens_per_chunk <= 0 || hp.fe_nb_max_frames <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0 ||
        hp.dec_max_position_embeddings <= 0) {
        return 0;
    }
    // Representative non-audio prompt overhead (instruct/transcription
    // affixes: BOS/INST/BEGIN_AUDIO/[/INST]/TRANSCRIBE plus an optional
    // language hint). Advisory; the real gate uses the actual prompt length.
    constexpr int k_prompt_overhead = 16;
    const int     max_audio_tokens  = hp.dec_max_position_embeddings - k_prompt_overhead - k_gen_reserve;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    // One 30 s chunk = fe_nb_max_frames mel frames; chunk_ms = frames * hop_ms.
    //   ms_per_audio_token = chunk_ms / tokens_per_chunk
    //   max_audio_ms       = max_audio_tokens * ms_per_audio_token
    const int64_t chunk_ms = static_cast<int64_t>(hp.fe_nb_max_frames) * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
    return static_cast<int64_t>(max_audio_tokens) * chunk_ms / tokens_per_chunk;
}

// BCP-47 -> English language name for the synthesized translate
// instruction ("Translate this to {Name}."). Covers Voxtral's advertised
// languages; falls back to the raw code if unknown.
struct LangName {
    const char * bcp47;
    const char * name;
};

constexpr LangName k_lang_names[] = {
    { "en", "English"    },
    { "fr", "French"     },
    { "de", "German"     },
    { "es", "Spanish"    },
    { "it", "Italian"    },
    { "pt", "Portuguese" },
    { "nl", "Dutch"      },
    { "hi", "Hindi"      },
};

const char * lang_name_for(const char * bcp47) {
    if (bcp47 == nullptr || bcp47[0] == '\0') {
        return "English";
    }
    for (const auto & e : k_lang_names) {
        if (std::strcmp(e.bcp47, bcp47) == 0) {
            return e.name;
        }
    }
    return bcp47;
}

// Resolve the transcription/instruct control tokens against the loaded
// tokenizer. Hard-fails if any piece is missing so a vocab reorder
// surfaces at load, not mid-decode.
transcribe_status resolve_specials(const transcribe::Tokenizer & tok, const VoxtralHParams & hp, PromptSpecials & out) {
    struct PieceSlot {
        const char * piece;
        int32_t *    slot;
    };

    const PieceSlot pieces[] = {
        { "[INST]",        &out.inst        },
        { "[BEGIN_AUDIO]", &out.begin_audio },
        { "[/INST]",       &out.end_inst    },
        { "[TRANSCRIBE]",  &out.transcribe  },
    };
    for (const auto & p : pieces) {
        const int id = tok.find(p.piece);
        if (id < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: control token \"%s\" not in tokenizer", p.piece);
            return TRANSCRIBE_ERR_GGUF;
        }
        *p.slot = id;
    }
    out.bos = tok.bos_id();
    out.eos = tok.eos_id();
    if (out.bos < 0 || out.eos < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: tokenizer missing bos/eos id");
        return TRANSCRIBE_ERR_GGUF;
    }
    (void) hp;
    return TRANSCRIBE_OK;
}

// Build the transcription prompt. language may be null/empty (auto).
transcribe_status build_transcription_prompt(const VoxtralModel &   m,
                                             const char *           language,
                                             int                    n_audio,
                                             std::vector<int32_t> & out_ids,
                                             int &                  prefix_len,
                                             int &                  suffix_len) {
    const PromptSpecials & s = m.specials;
    out_ids.clear();
    out_ids.push_back(s.bos);
    out_ids.push_back(s.inst);
    out_ids.push_back(s.begin_audio);
    prefix_len = static_cast<int>(out_ids.size());  // 3
    for (int i = 0; i < n_audio; ++i) {
        out_ids.push_back(m.hparams.audio_token_id);
    }
    out_ids.push_back(s.end_inst);
    if (language != nullptr && language[0] != '\0') {
        std::vector<int32_t> lang_ids;
        const std::string    lang_str = std::string("lang:") + language;
        if (const transcribe_status st = m.tok.encode(lang_str, lang_ids); st != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: failed to encode \"%s\"", lang_str.c_str());
            return st;
        }
        out_ids.insert(out_ids.end(), lang_ids.begin(), lang_ids.end());
    }
    out_ids.push_back(s.transcribe);
    suffix_len = static_cast<int>(out_ids.size()) - prefix_len - n_audio;
    return TRANSCRIBE_OK;
}

// Build the instruct prompt: audio + BPE(instruction) + [/INST].
transcribe_status build_instruct_prompt(const VoxtralModel &   m,
                                        const std::string &    instruction,
                                        int                    n_audio,
                                        std::vector<int32_t> & out_ids,
                                        int &                  prefix_len,
                                        int &                  suffix_len) {
    const PromptSpecials & s = m.specials;
    out_ids.clear();
    out_ids.push_back(s.bos);
    out_ids.push_back(s.inst);
    out_ids.push_back(s.begin_audio);
    prefix_len = static_cast<int>(out_ids.size());  // 3
    for (int i = 0; i < n_audio; ++i) {
        out_ids.push_back(m.hparams.audio_token_id);
    }
    std::vector<int32_t> instr_ids;
    if (const transcribe_status st = m.tok.encode(instruction, instr_ids); st != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: failed to encode instruction text");
        return st;
    }
    out_ids.insert(out_ids.end(), instr_ids.begin(), instr_ids.end());
    out_ids.push_back(s.end_inst);
    suffix_len = static_cast<int>(out_ids.size()) - prefix_len - n_audio;
    return TRANSCRIBE_OK;
}

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<VoxtralModel>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = loader.variant().empty() ? k_default_variant : loader.variant();
    m->backend.clear();

    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (const transcribe_status st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }
    if (const transcribe_status st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }

    if (const transcribe_status st = read_voxtral_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window and
    // the 30 s-chunk encoder rate are known (apply_family_invariants ran before
    // hparams). See docs/input-limits.md.
    m->caps.max_audio_ms = voxtral_max_audio_ms(m->hparams);

    // Basis for transcribe_session_get_limits: the same constants
    // voxtral_max_audio_ms uses, kept so the effective limit can be recomputed at
    // a lowered session n_ctx.
    {
        const int tokens_per_chunk = m->hparams.audio_tokens_per_chunk();
        if (m->hparams.dec_max_position_embeddings > 0 && tokens_per_chunk > 0 && m->hparams.fe_nb_max_frames > 0 &&
            m->hparams.fe_hop_length > 0 && m->hparams.fe_sample_rate > 0) {
            m->limits.has_context_cap    = true;
            m->limits.model_max_ctx      = m->hparams.dec_max_position_embeddings;
            m->limits.prompt_overhead    = 16;  // k_prompt_overhead in
                                                // voxtral_max_audio_ms
            m->limits.gen_reserve        = k_gen_reserve;
            // ms-per-audio-token = chunk_ms / tokens_per_chunk, where one 30 s
            // chunk = fe_nb_max_frames mel frames (same rate the max_audio_ms
            // helper inverts).
            const double chunk_ms        = static_cast<double>(m->hparams.fe_nb_max_frames) * m->hparams.fe_hop_length *
                                           1000.0 / m->hparams.fe_sample_rate;
            m->limits.ms_per_audio_token = chunk_ms / tokens_per_chunk;
            m->limits.kv_elems_per_ctx_token =
                (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
        }
    }

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();

    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: GGUF tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = resolve_specials(m->tok, m->hparams, m->specials); st != TRANSCRIBE_OK) {
        return st;
    }

    // Mel frontend (Whisper 128-bin log-mel at 16 kHz, per-utterance norm).
    {
        transcribe::MelConfig cfg{};
        cfg.sample_rate  = m->hparams.fe_sample_rate;
        cfg.num_mels     = m->hparams.fe_num_mels;
        cfg.n_fft        = m->hparams.fe_n_fft;
        cfg.win_length   = m->hparams.fe_win_length;
        cfg.hop_length   = m->hparams.fe_hop_length;
        cfg.pre_emphasis = m->hparams.fe_pre_emphasis;
        cfg.f_min        = m->hparams.fe_f_min;
        cfg.f_max        = m->hparams.fe_f_max;
        cfg.pad_mode     = m->hparams.fe_pad_mode;
        cfg.window_type  = m->hparams.fe_window;     // "hann_periodic"
        cfg.normalize    = m->hparams.fe_normalize;  // "per_utterance"

        using R               = transcribe::load_common::ReadF32Result;
        const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
        const auto   fb_rc    = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "voxtral", cfg.filterbank);
        if (fb_rc != R::Ok && fb_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }
        const size_t win_elems = static_cast<size_t>(cfg.win_length);
        const auto   win_rc    = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.window", win_elems, "voxtral", cfg.window);
        if (win_rc != R::Ok && win_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }

        m->mel.emplace(cfg);
    }

    // Tensor catalog (no_alloc) + backend alloc + data stream.
    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = build_voxtral_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "voxtral", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "voxtral");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Pack gate+up for one-mul_mat SwiGLU.
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "voxtral")) {
            m->packed_gate_up.free();
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    m->t_load_us = ggml_time_us() - t_load_start;
    *out_model   = m.release();
    return TRANSCRIBE_OK;
}

transcribe_status init_context(transcribe_model *                model,
                               const transcribe_session_params * params,
                               transcribe_session **             out_ctx) {
    if (model->arch != &arch) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto cc       = std::make_unique<VoxtralSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    // Whisper-large-v3 encoder (head_dim 64, full bidirectional, null mask) —
    // flash-supported on every backend. Decoder (Llama head_dim 128) also flash.
    cc->encoder_use_flash = true;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    auto * cm = static_cast<VoxtralModel *>(model);
    {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        // Initial allocation only: run()/run_batch() grow the KV cache per
        // utterance up to the decoder's trained context (131072 for Mini-3B).
        // 4096 covers short clips (~5 min) without a realloc on the hot path.
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary,
                                            /*n_ctx=*/4096, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                            cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral init_context: KV cache allocation failed "
                                "(n_ctx=4096, %d kv-heads x %d head-dim x %d layers) — "
                                "out of memory.",
                                cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim, cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    }

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Apply n_threads to every backend on the scheduler.
void set_sched_threads(ggml_backend_sched_t sched, int n_threads) {
    transcribe::configure_sched_n_threads(sched, n_threads);
}

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<VoxtralSession *>(session);
    auto * cm = static_cast<VoxtralModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    // ----- Prompt mode -----
    const bool  translate = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE);
    std::string instruction;
    if (translate) {
        const char * tgt = (params != nullptr) ? params->target_language : nullptr;
        instruction      = std::string("Translate this to ") + lang_name_for(tgt) + ".";
    }

    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // ----- Chunking: pad PCM to a multiple of 30 s and mel once -----
    int samples_per_chunk = cm->hparams.fe_n_samples;
    if (samples_per_chunk <= 0) {
        samples_per_chunk =
            (cm->hparams.fe_chunk_length > 0 ? cm->hparams.fe_chunk_length : 30) * cm->hparams.fe_sample_rate;
    }
    const int frames_per_chunk = (cm->hparams.fe_nb_max_frames > 0) ? cm->hparams.fe_nb_max_frames :
                                                                      samples_per_chunk / cm->hparams.fe_hop_length;
    const int audio_per_chunk  = cm->hparams.audio_tokens_per_chunk();

    const int    n_chunks = std::max(1, (n_samples + samples_per_chunk - 1) / samples_per_chunk);
    const size_t padded   = static_cast<size_t>(n_chunks) * samples_per_chunk;

    std::vector<float> pcm_padded(padded, 0.0f);
    std::memcpy(pcm_padded.data(), pcm, static_cast<size_t>(n_samples) * sizeof(float));

    const int64_t t_mel_start = ggml_time_us();
    int           mel_n_mels = 0, mel_n_frames = 0;
    if (const transcribe_status mst =
            cm->mel->compute(pcm_padded.data(), padded, cc->mel_buf, mel_n_mels, mel_n_frames, cc->n_threads);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    if (mel_n_mels != cm->hparams.enc_num_mel_bins) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: mel bins %d != %d", mel_n_mels, cm->hparams.enc_num_mel_bins);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (mel_n_frames < n_chunks * frames_per_chunk) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: mel frames %d < %d*%d", mel_n_frames, n_chunks,
                frames_per_chunk);
        return TRANSCRIBE_ERR_GGUF;
    }

    // enc.mel.in dump: mel-major [n_mels, n_frames], matching the reference.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { mel_n_mels, mel_n_frames };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "frontend.mel.norm");
    }

    // ----- Encoder + projector, per 30 s chunk -----
    const int dec_h         = cm->hparams.dec_hidden;
    const int n_audio_total = n_chunks * audio_per_chunk;
    cc->enc_host.assign(static_cast<size_t>(dec_h) * n_audio_total, 0.0f);

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    const int64_t      t_enc_start = ggml_time_us();
    std::vector<float> chunk_mel(static_cast<size_t>(mel_n_mels) * frames_per_chunk);
    for (int c = 0; c < n_chunks; ++c) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }

        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 32 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: ggml_init (encoder) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        EncoderBuild eb =
            build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, frames_per_chunk, cc->encoder_use_flash);
        if (eb.graph == nullptr || eb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run: encoder graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        // Frame-major mel for this chunk: chunk_mel[t*n_mels + m] =
        // mel_buf[m*n_frames + (c*frames_per_chunk + t)].
        for (int t = 0; t < frames_per_chunk; ++t) {
            const int tg = c * frames_per_chunk + t;
            for (int mm = 0; mm < mel_n_mels; ++mm) {
                chunk_mel[static_cast<size_t>(t) * mel_n_mels + mm] =
                    cc->mel_buf[static_cast<size_t>(mm) * mel_n_frames + tg];
            }
        }
        ggml_backend_tensor_set(eb.mel_in, chunk_mel.data(), 0, chunk_mel.size() * sizeof(float));
        set_sched_threads(cc->sched, cc->n_threads);

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: encoder compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }

        // Dump enc.* for the first chunk (matches the single-chunk reference).
        if (c == 0 && transcribe::debug::enabled()) {
            for (size_t i = 0; i < eb.dumps.block_outs.size(); ++i) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.block.%zu.out", i);
                transcribe::debug::dump_tensor(nm, eb.dumps.block_outs[i], "enc.block");
            }
            if (eb.dumps.enc_out) {
                transcribe::debug::dump_tensor("enc.out", eb.dumps.enc_out, "enc");
            }
            if (eb.dumps.proj_out) {
                transcribe::debug::dump_tensor("proj.out", eb.dumps.proj_out, "proj");
            }
        }

        // Read proj.out [dec_h, audio_per_chunk] into the chunk's slot.
        ggml_backend_tensor_get(eb.out, cc->enc_host.data() + static_cast<size_t>(c) * audio_per_chunk * dec_h, 0,
                                static_cast<size_t>(audio_per_chunk) * dec_h * sizeof(float));
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // ----- Prompt construction -----
    std::vector<int32_t> prompt_ids;
    int                  prefix_len = 0, suffix_len = 0;
    if (translate) {
        if (const transcribe_status st =
                build_instruct_prompt(*cm, instruction, n_audio_total, prompt_ids, prefix_len, suffix_len);
            st != TRANSCRIBE_OK) {
            return st;
        }
    } else {
        const char * lang = (params != nullptr) ? params->language : nullptr;
        if (const transcribe_status st =
                build_transcription_prompt(*cm, lang, n_audio_total, prompt_ids, prefix_len, suffix_len);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }
    const int T_prompt = static_cast<int>(prompt_ids.size());

    // ----- Input-length gate (see docs/input-limits.md) -----
    // Auto-size the KV cache to this utterance (grow to fit, capped at the
    // ceiling). The audio-token count is fixed by the input length, so reject an
    // over-length clip up front rather than growing past the trained context and
    // aliasing RoPE. The ceiling honors the caller's n_ctx knob (lowered, never
    // raised).
    const int model_max = voxtral_context_ceiling(cc->n_ctx, cm->hparams);
    if (T_prompt + k_gen_reserve > model_max) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral run: input too long — %d audio tokens + %d prompt exceed "
                            "the %d-token context (need %d incl. generation reserve). Shorten "
                            "the audio (see transcribe_capabilities.max_audio_ms) or split it "
                            "into segments.",
                            n_audio_total, T_prompt - n_audio_total, model_max, T_prompt + k_gen_reserve);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }
    const int max_new  = pick_decode_budget(n_audio_total, T_prompt, model_max);
    const int want_ctx = pick_kv_ctx(T_prompt + max_new, model_max);
    if (cc->kv_cache.n_ctx < want_ctx) {
        const ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
        cc->kv_cache.free();
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, want_ctx, cm->hparams.dec_n_kv_heads,
                                            cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run: KV cache allocation failed (grow to n_ctx=%d) — "
                                "out of memory. Lower transcribe_session_params.n_ctx or shorten "
                                "the audio.",
                                want_ctx);
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // ----- KV reset -----
    const int64_t t_dec_start = ggml_time_us();
    if (cc->kv_cache.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
    }
    cc->kv_cache.n    = 0;
    cc->kv_cache.head = 0;

    // ----- Prefill -----
    const bool dumps_on   = transcribe::debug::enabled();
    const bool slice_last = !dumps_on;
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 64 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: ggml_init (prefill) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, T_prompt,
                                          n_audio_total, prefix_len, suffix_len, cc->decoder_use_flash, slice_last);
    if (pb.graph == nullptr || pb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral run: prefill graph allocation failed — out of memory. "
                            "Lower transcribe_session_params.n_ctx or shorten the audio.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
    ggml_backend_tensor_set(pb.enc_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
    {
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }
    {
        const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
        const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, mn);
        for (int r = 0; r < T_prompt; ++r) {
            for (int col = 0; col <= r; ++col) {
                mask[static_cast<size_t>(r) * T_prompt + col] = mz;
            }
        }
        ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
    }
    set_sched_threads(cc->sched, cc->n_threads);

    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: prefill compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->kv_cache.n    = T_prompt;
    cc->kv_cache.head = T_prompt;

    if (dumps_on) {
        auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
            if (t != nullptr) {
                transcribe::debug::dump_tensor(name, t, stage);
            }
        };
        try_dump("dec.token_emb", pb.dumps.token_emb, "dec.token_emb");
        try_dump("dec.audio_injected", pb.dumps.audio_injected, "dec.audio_injected");
        try_dump("dec.block.0.out", pb.dumps.block_0_out, "dec.block.0");
        {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers / 2);
            try_dump(nm, pb.dumps.block_mid_out, "dec.block.mid");
        }
        {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers - 1);
            try_dump(nm, pb.dumps.block_last_out, "dec.block.last");
        }
        try_dump("dec.out_before_head", pb.dumps.out_before_head, "dec.out_before_head");
        try_dump("dec.logits_raw", pb.dumps.logits_raw, "dec.logits_raw");
    }

    // ----- First token (argmax of prefill logits) -----
    const int          vocab = cm->hparams.dec_vocab_size;
    std::vector<float> logits(vocab);
    ggml_backend_tensor_get(pb.out, logits.data(), 0, logits.size() * sizeof(float));
    auto argmax = [&](const std::vector<float> & v) -> int32_t {
        int32_t best   = 0;
        float   best_v = v[0];
        for (int32_t i = 1; i < static_cast<int32_t>(v.size()); ++i) {
            if (v[i] > best_v) {
                best_v = v[i];
                best   = i;
            }
        }
        return best;
    };
    std::vector<int32_t> generated_ids;
    int32_t              next_tok = argmax(logits);
    generated_ids.push_back(next_tok);

    // ----- Step loop -----
    const int32_t eos_id   = cm->hparams.eos_token_id;
    int           cur_past = T_prompt;

    int max_n_kv = 1024;
    while (max_n_kv < T_prompt + max_new) {
        max_n_kv *= 2;
    }
    if (max_n_kv > cc->kv_cache.n_ctx) {
        max_n_kv = cc->kv_cache.n_ctx;
    }

    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 16 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: ggml_init (step) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    StepBuild sb =
        build_step_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, max_n_kv, cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral run: step graph allocation failed — out of memory. "
                            "Lower transcribe_session_params.n_ctx or shorten the audio.");
        return TRANSCRIBE_ERR_OOM;
    }
    set_sched_threads(cc->sched, cc->n_threads);

    const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> step_mask(max_n_kv, mn);
    while (next_tok != eos_id && static_cast<int32_t>(generated_ids.size()) < max_new && cur_past + 1 <= max_n_kv) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        ggml_backend_tensor_set(sb.input_id_in, &next_tok, 0, sizeof(int32_t));
        const int32_t pos_val = cur_past;
        ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
        const int64_t kv_idx_val = cur_past;
        ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx_val, 0, sizeof(int64_t));
        if (cur_past == T_prompt) {
            std::fill(step_mask.begin(), step_mask.begin() + cur_past + 1, mz);
        } else {
            step_mask[cur_past] = mz;
        }
        ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0, static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));

        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral run: step compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }
        // Mid-generation logits dump: the step at cur_past == T_prompt + 7 is
        // the reference's scores[8] (logits for the 9th generated token).
        if (dumps_on && sb.logits != nullptr && cur_past == T_prompt + 7) {
            transcribe::debug::dump_tensor("dec.logits_raw.gen8", sb.logits, "dec.step");
        }

        int32_t argmax_tok = 0;
        ggml_backend_tensor_get(sb.out, &argmax_tok, 0, sizeof(int32_t));
        next_tok = argmax_tok;
        generated_ids.push_back(next_tok);
        cur_past += 1;
        cc->kv_cache.n    = cur_past + 1;
        cc->kv_cache.head = cur_past + 1;
    }
    cc->t_decode_us = ggml_time_us() - t_dec_start;

    // Decode stopped at EOS (complete) or at the generation budget / context
    // width (truncated). Surface the latter via transcribe_was_truncated() + WARN.
    if (next_tok != eos_id) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "voxtral run: output truncated at %d tokens — decode reached the "
                            "generation budget before end-of-stream; the transcript may be "
                            "incomplete.",
                            static_cast<int>(generated_ids.size()));
    }

    if (!generated_ids.empty() && generated_ids.back() == eos_id) {
        generated_ids.pop_back();
    }

    std::string text = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));
    // Trim leading/trailing whitespace (the assistant turn often starts
    // with a byte-level space token).
    size_t      b = 0, e = text.size();
    while (b < e && std::isspace(static_cast<unsigned char>(text[b]))) {
        ++b;
    }
    while (e > b && std::isspace(static_cast<unsigned char>(text[e - 1]))) {
        --e;
    }
    text = text.substr(b, e - b);

    cc->full_text   = text;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;
    transcribe_session::SegmentEntry seg{};
    seg.text  = text;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
    cc->segments.push_back(std::move(seg));

    // Output truncation is a hard status: the partial transcript stays readable
    // (like an aborted run) but the caller is told, not given a clean OK.
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

// ---------------------------------------------------------------------------
// Offline batched decode (transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// Batched encoder (all equal-length 30 s chunks stacked on ne[2]) + batched
// prefill + the shared causal_lm batched step loop. Ragged prompts are
// right-padded to T_prompt_max; the decoder recipe mirrors arch/canary_qwen.
// Falls back to a serial per-utterance loop for n==1, dump mode, or no-flash.

transcribe_status run_batch_serial(VoxtralSession *              cc,
                                   const float * const *         pcm,
                                   const int *                   n_samples,
                                   int                           n,
                                   const transcribe_run_params * params) {
    for (int i = 0; i < n; ++i) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        const transcribe_status st = (pcm[i] == nullptr || n_samples[i] <= 0) ? TRANSCRIBE_ERR_INVALID_ARG :
                                                                                run(cc, pcm[i], n_samples[i], params);
        if (st == TRANSCRIBE_OK) {
            cc->batch_results.push_back(cc->capture_result(st));
        } else {
            transcribe_session::ResultSet rs;
            rs.status = st;
            cc->batch_results.push_back(std::move(rs));
        }
    }
    return TRANSCRIBE_OK;
}

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<VoxtralSession *>(session);
    auto * cm = static_cast<VoxtralModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // The batched encoder + causal_lm batched blocks are flash-only; dump mode
    // and n==1 take the established single-shot path for byte-parity.
    if (!cc->decoder_use_flash || !cc->encoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto & hp = cm->hparams;

    // ----- Prompt mode (uniform across the batch) -----
    const bool  translate = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE);
    std::string instruction;
    if (translate) {
        const char * tgt = (params != nullptr) ? params->target_language : nullptr;
        instruction      = std::string("Translate this to ") + lang_name_for(tgt) + ".";
    }
    const char * lang = (params != nullptr) ? params->language : nullptr;

    // ----- Chunk geometry -----
    int samples_per_chunk = hp.fe_n_samples;
    if (samples_per_chunk <= 0) {
        samples_per_chunk = (hp.fe_chunk_length > 0 ? hp.fe_chunk_length : 30) * hp.fe_sample_rate;
    }
    const int frames_per_chunk = (hp.fe_nb_max_frames > 0) ? hp.fe_nb_max_frames : samples_per_chunk / hp.fe_hop_length;
    const int audio_per_chunk  = hp.audio_tokens_per_chunk();
    const int n_mels           = hp.enc_num_mel_bins;
    const int dec_h            = hp.dec_hidden;

    // ----- Pass 0: per-utterance chunking + parallel mel -----
    std::vector<char>               valid(n, 0);
    // Utterances rejected by the input-length gate below. Tracked separately
    // from `valid` so the result-capture pass can report the precise
    // TRANSCRIBE_ERR_INPUT_TOO_LONG status rather than a generic invalid-arg.
    std::vector<char>               over_length(n, 0);
    std::vector<int>                n_chunks(n, 0);
    std::vector<std::vector<float>> mel_bufs(n);
    std::vector<int>                mel_nf(n, 0);

    int n_mel_threads = cc->n_threads;
    if (n_mel_threads <= 0) {
        n_mel_threads = transcribe::default_n_threads();
    }

    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_mel_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;
        }
        const int          nc     = std::max(1, (n_samples[b] + samples_per_chunk - 1) / samples_per_chunk);
        const size_t       padded = static_cast<size_t>(nc) * samples_per_chunk;
        std::vector<float> pcm_padded(padded, 0.0f);
        std::memcpy(pcm_padded.data(), pcm[b], static_cast<size_t>(n_samples[b]) * sizeof(float));
        int nm = 0, nf = 0;
        if (cm->mel->compute(pcm_padded.data(), padded, mel_bufs[b], nm, nf,
                             /*n_threads=*/1) == TRANSCRIBE_OK &&
            nm == n_mels && nf >= nc * frames_per_chunk) {
            n_chunks[b] = nc;
            mel_nf[b]   = nf;
        }
        return true;
    });
    int64_t mel_us = ggml_time_us() - t_mel0;

    // Map global chunk index -> (utterance, local chunk).
    std::vector<int> chunk_owner, chunk_local;
    for (int b = 0; b < n; ++b) {
        if (n_chunks[b] <= 0) {
            continue;
        }
        valid[b] = 1;
        for (int lc = 0; lc < n_chunks[b]; ++lc) {
            chunk_owner.push_back(b);
            chunk_local.push_back(lc);
        }
    }
    const int total_chunks = static_cast<int>(chunk_owner.size());
    if (total_chunks == 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }

    // Lazy scheduler (shared with the single-shot path).
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- Pass 1: batched encoder over all chunks -----
    std::vector<std::vector<float>> enc_hosts(n);
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            enc_hosts[b].assign(static_cast<size_t>(dec_h) * n_chunks[b] * audio_per_chunk, 0.0f);
        }
    }

    int64_t enc_us = 0;
    {
        // Frame-major mel slab [n_mels, frames_per_chunk, total_chunks]:
        // slab[(gc*FPC + t)*n_mels + m] = mel_bufs[b][m*mel_nf[b] + lc*FPC + t].
        std::vector<float> slab(static_cast<size_t>(n_mels) * frames_per_chunk * total_chunks, 0.0f);
        for (int gc = 0; gc < total_chunks; ++gc) {
            const int     b   = chunk_owner[gc];
            const int     lc  = chunk_local[gc];
            const float * src = mel_bufs[b].data();
            const int     nf  = mel_nf[b];
            for (int t = 0; t < frames_per_chunk; ++t) {
                const size_t dst_base = (static_cast<size_t>(gc) * frames_per_chunk + t) * n_mels;
                const int    gframe   = lc * frames_per_chunk + t;
                for (int m = 0; m < n_mels; ++m) {
                    slab[dst_base + m] = src[static_cast<size_t>(m) * nf + gframe];
                }
            }
        }

        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 64 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: ggml_init (encoder) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        EncoderBuild eb = build_encoder_graph_batched(cc->compute_ctx, cm->weights, hp, frames_per_chunk, total_chunks,
                                                      cc->encoder_use_flash);
        if (eb.graph == nullptr || eb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: encoder graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        ggml_backend_tensor_set(eb.mel_in, slab.data(), 0, slab.size() * sizeof(float));
        set_sched_threads(cc->sched, cc->n_threads);

        const int64_t t_enc0 = ggml_time_us();
        if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        enc_us = ggml_time_us() - t_enc0;

        // proj.out [dec_h, audio_per_chunk, total_chunks] -> per-utterance host.
        std::vector<float> proj_host(static_cast<size_t>(dec_h) * audio_per_chunk * total_chunks);
        ggml_backend_tensor_get(eb.out, proj_host.data(), 0, proj_host.size() * sizeof(float));
        const size_t chunk_elems = static_cast<size_t>(dec_h) * audio_per_chunk;
        for (int gc = 0; gc < total_chunks; ++gc) {
            const int b  = chunk_owner[gc];
            const int lc = chunk_local[gc];
            std::memcpy(enc_hosts[b].data() + static_cast<size_t>(lc) * chunk_elems,
                        proj_host.data() + static_cast<size_t>(gc) * chunk_elems, chunk_elems * sizeof(float));
        }
    }

    // ----- Prompts (ragged: n_audio differs per utterance) -----
    // Input-length gate (see docs/input-limits.md): per-utterance, the audio
    // tokens + prompt + generation reserve must fit the decoder context ceiling
    // (the trained max, lowered by the caller's n_ctx knob). An over-length
    // utterance is dropped from the batch and flagged so its captured result
    // carries TRANSCRIBE_ERR_INPUT_TOO_LONG.
    const int                         ctx_ceiling = voxtral_context_ceiling(cc->n_ctx, hp);
    std::vector<std::vector<int32_t>> prompt_ids(n);
    std::vector<int>                  T_prompt(n, 0), T_audio(n, 0);
    int                               prefix_len = 0, suffix_len = 0;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        const int               n_audio = n_chunks[b] * audio_per_chunk;
        int                     pfx = 0, sfx = 0;
        const transcribe_status st = translate ?
                                         build_instruct_prompt(*cm, instruction, n_audio, prompt_ids[b], pfx, sfx) :
                                         build_transcription_prompt(*cm, lang, n_audio, prompt_ids[b], pfx, sfx);
        if (st != TRANSCRIBE_OK) {
            valid[b] = 0;
            continue;
        }
        const int t_prompt = static_cast<int>(prompt_ids[b].size());
        if (t_prompt + k_gen_reserve > ctx_ceiling) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: utterance %d input too long — %d audio "
                                "tokens + %d prompt exceed the %d-token context (need %d incl. "
                                "generation reserve). Shorten the audio (see "
                                "transcribe_capabilities.max_audio_ms) or split it into "
                                "segments.",
                                b, n_audio, t_prompt - n_audio, ctx_ceiling, t_prompt + k_gen_reserve);
            valid[b]       = 0;
            over_length[b] = 1;
            continue;
        }
        prefix_len  = pfx;
        suffix_len  = sfx;  // uniform across the batch
        T_prompt[b] = t_prompt;
        T_audio[b]  = n_audio;
    }

    int max_T_prompt = 0, T_audio_max = 0;
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            max_T_prompt = std::max(max_T_prompt, T_prompt[b]);
            T_audio_max  = std::max(T_audio_max, T_audio[b]);
        }
    }
    if (max_T_prompt == 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = over_length[b] ? TRANSCRIBE_ERR_INPUT_TOO_LONG : TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }
    T_audio_max = std::max(1, T_audio_max);

    // Size the batched KV cache to the longest prompt plus the decode budget,
    // clamped to the context ceiling. kv_init_batched grows the cache on demand.
    const int model_max = ctx_ceiling;
    const int max_new   = pick_decode_budget(T_audio_max, max_T_prompt, model_max);
    int       max_n_kv  = 1024;
    while (max_n_kv < max_T_prompt + max_new) {
        max_n_kv *= 2;
    }
    if (max_n_kv > model_max) {
        max_n_kv = model_max;
    }

    // ----- Batched KV cache (reused; re-alloc only on growth) -----
    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx != max_n_kv) {
        cc->kv_cache_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_cache_batch, cm->plan.primary, max_n_kv, hp.dec_n_kv_heads,
                                                    hp.dec_head_dim, hp.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: batched KV cache allocation failed — "
                                "out of memory. Lower transcribe_session_params.n_ctx or "
                                "shorten the audio.");
            return TRANSCRIBE_ERR_OOM;
        }
        cc->kv_batch_cap   = n;
        cc->kv_batch_n_ctx = max_n_kv;
    } else if (cc->kv_cache_batch.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache_batch.buffer, 0);
    }

    // ----- Pass 2: batched prefill -----
    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, 0);
    std::vector<std::vector<int32_t>> generated(n);
    int64_t                           dec_us = 0;
    {
        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 64 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: ggml_init (prefill) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        PrefillBuildBatched pb = build_prefill_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch,
                                                             max_T_prompt, T_audio_max, n, cc->decoder_use_flash);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: prefill graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        std::vector<int32_t> ids(static_cast<size_t>(max_T_prompt) * n, 0);
        // Audio-injection blend buffers: audio_dense holds each utterance's audio
        // embeds at their prompt positions (zero elsewhere); keep is 0 there, 1
        // elsewhere. x = x*keep + audio_dense.
        std::vector<float>   audio_dense(static_cast<size_t>(dec_h) * max_T_prompt * n, 0.0f);
        std::vector<float>   keep(static_cast<size_t>(max_T_prompt) * n, 1.0f);
        std::vector<int64_t> kidx(static_cast<size_t>(max_T_prompt) * n, 0);
        std::vector<int32_t> lidx(n, 0);
        for (int b = 0; b < n; ++b) {
            const int ta = valid[b] ? T_audio[b] : 0;
            const int tp = valid[b] ? T_prompt[b] : 0;
            if (valid[b]) {
                std::memcpy(ids.data() + static_cast<size_t>(b) * max_T_prompt, prompt_ids[b].data(),
                            static_cast<size_t>(tp) * sizeof(int32_t));
                // enc_hosts[b] is [dec_h, ta] column-major; audio token j lands
                // at prompt position prefix_len+j, flat column b*max_T_prompt+pos.
                for (int j = 0; j < ta; ++j) {
                    const size_t dst_col = static_cast<size_t>(b) * max_T_prompt + (prefix_len + j);
                    std::memcpy(audio_dense.data() + dst_col * dec_h,
                                enc_hosts[b].data() + static_cast<size_t>(j) * dec_h,
                                static_cast<size_t>(dec_h) * sizeof(float));
                    keep[dst_col] = 0.0f;
                }
            }
            for (int t = 0; t < max_T_prompt; ++t) {
                kidx[static_cast<size_t>(b) * max_T_prompt + t] = t;
            }
            lidx[b] = valid[b] ? (tp - 1) : 0;
        }
        ggml_backend_tensor_set(pb.input_ids_in, ids.data(), 0, ids.size() * sizeof(int32_t));
        ggml_backend_tensor_set(pb.audio_dense_in, audio_dense.data(), 0, audio_dense.size() * sizeof(float));
        ggml_backend_tensor_set(pb.keep_mask_in, keep.data(), 0, keep.size() * sizeof(float));
        {
            std::vector<int32_t> pos(max_T_prompt);
            for (int t = 0; t < max_T_prompt; ++t) {
                pos[t] = t;
            }
            ggml_backend_tensor_set(pb.positions_in, pos.data(), 0, pos.size() * sizeof(int32_t));
        }
        {
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
            const ggml_fp16_t        mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(max_T_prompt) * max_T_prompt, mn);
            for (int q = 0; q < max_T_prompt; ++q) {
                std::fill(mask.begin() + static_cast<size_t>(q) * max_T_prompt,
                          mask.begin() + static_cast<size_t>(q) * max_T_prompt + q + 1, mz);
            }
            ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        ggml_backend_tensor_set(pb.kv_idx_in, kidx.data(), 0, kidx.size() * sizeof(int64_t));
        ggml_backend_tensor_set(pb.last_idx_in, lidx.data(), 0, lidx.size() * sizeof(int32_t));
        set_sched_threads(cc->sched, cc->n_threads);

        const int64_t t_dec0 = ggml_time_us();
        if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        dec_us += ggml_time_us() - t_dec0;

        std::vector<int32_t> amax(n, 0);
        ggml_backend_tensor_get(pb.out, amax.data(), 0, amax.size() * sizeof(int32_t));
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            n_past[b]   = T_prompt[b];
            next_tok[b] = amax[b];
            generated[b].push_back(amax[b]);
        }
    }

    // ----- Pass 3: batched greedy step loop (shared causal_lm driver) -----
    const int32_t     eos_id = hp.eos_token_id;
    // Per-utterance truncation flags from the step loop (consumed in the
    // result-capture pass below, outside this block's scope).
    std::vector<char> truncated;
    {
        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 32 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: ggml_init (step) failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        StepBuildBatched sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch, max_n_kv,
                                                       n, cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral run_batch: step graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        set_sched_threads(cc->sched, cc->n_threads);

        transcribe::causal_lm::StepBatchedIO io{};
        io.input_ids = sb.input_ids_in;
        io.positions = sb.position_in;
        io.kv_idx    = sb.kv_idx_in;
        io.mask      = sb.mask_in;
        io.argmax    = sb.out;
        io.graph     = sb.graph;

        transcribe::causal_lm::StepBatchedState step_state;
        step_state.valid    = valid;
        step_state.next_tok = next_tok;
        step_state.n_past   = n_past;

        transcribe::causal_lm::StepLoopStats step_stats;
        if (const transcribe_status st = transcribe::causal_lm::run_batched_step_loop(
                cc, cc->sched, io, n, max_n_kv, eos_id, max_new, step_state, generated, &step_stats, &truncated);
            st != TRANSCRIBE_OK) {
            return st;
        }
        dec_us += step_stats.step_us;
    }

    // ----- Capture per-utterance results -----
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            transcribe_session::ResultSet rs;
            rs.status = over_length[b] ? TRANSCRIBE_ERR_INPUT_TOO_LONG : TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::vector<int32_t> gen = generated[b];
        if (!gen.empty() && gen.back() == eos_id) {
            gen.pop_back();
        }
        std::string text = cm->tok.decode(gen.data(), static_cast<int>(gen.size()));
        size_t      s = 0, e = text.size();
        while (s < e && std::isspace(static_cast<unsigned char>(text[s]))) {
            ++s;
        }
        while (e > s && std::isspace(static_cast<unsigned char>(text[e - 1]))) {
            --e;
        }
        text = text.substr(s, e - s);

        transcribe_session::ResultSet rs;
        rs.full_text   = text;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        rs.status      = TRANSCRIBE_OK;
        transcribe_session::SegmentEntry seg{};
        seg.text  = text;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(n_samples[b]) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
        rs.segments.push_back(std::move(seg));
        // Per-utterance truncation parity with single-shot run(): a row cut at
        // the generation budget / KV window before eos reports
        // TRANSCRIBE_ERR_OUTPUT_TRUNCATED (partial transcript retained). Only
        // override a TRANSCRIBE_OK status, never a worse one.
        if (b < static_cast<int>(truncated.size()) && truncated[b] && rs.status == TRANSCRIBE_OK) {
            cc->was_truncated = true;
            rs.status         = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = dec_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }
    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "voxtral",
    /* .load             = */ load,
    /* .init_context     = */ init_context,
    /* .run              = */ run,
    /* .run_batch        = */ run_batch,
    /* .stream_validate  = */ nullptr,
    /* .stream_begin     = */ nullptr,
    /* .stream_feed      = */ nullptr,
    /* .stream_finalize  = */ nullptr,
    /* .stream_reset     = */ nullptr,
    /* .accepts_ext_kind = */ nullptr,
};

}  // namespace transcribe::voxtral
