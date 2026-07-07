// arch/sensevoice/model.cpp - SenseVoice family handler.
//
// load() validates every weight and binds the backend buffer; run()
// drives the kaldi fbank frontend host-side, builds + computes the
// encoder + CTC graph, and runs greedy CTC decode (argmax →
// unique_consecutive → drop-blank → SP detokenize) to populate the
// public result hierarchy.

#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "sanm/sanm.h"
#include "sensevoice.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-kaldi-fbank.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::sensevoice {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, SenseVoiceModel>);
static_assert(std::is_base_of_v<transcribe_session, SenseVoiceSession>);

SenseVoiceSession::~SenseVoiceSession() {
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

SenseVoiceModel::~SenseVoiceModel() {
    if (ctx_meta != nullptr) {
        ggml_free(ctx_meta);
        ctx_meta = nullptr;
    }
    if (backend_buffer != nullptr) {
        safe_buffer_free(backend_buffer);
        backend_buffer = nullptr;
    }
    for (auto it = plan.scheduler_list.rbegin(); it != plan.scheduler_list.rend(); ++it) {
        safe_backend_free(*it);
    }
    plan.scheduler_list.clear();
    plan.primary      = nullptr;
    plan.primary_kind = transcribe::BackendKind::Unknown;
}

namespace {

constexpr const char k_default_variant[] = "sensevoice-small";

// ---------------------------------------------------------------------------
// Input-length contract (see docs/input-limits.md). SenseVoice-Small is a
// SOFT-WINDOW family: a single-pass non-autoregressive CTC encoder with no
// hard architectural cap, but trained on short (≤ ~30 s) utterances. There is
// no context window to reject against — over-window audio still runs, but the
// self-attention cost is O(n^2) so accuracy degrades and memory grows
// quadratically. We therefore WARN past the window and PROCEED (never reject,
// never alter numerics); max_audio_ms advertises the window as advisory.
// ---------------------------------------------------------------------------

// Advisory training window. Upstream FunASR's runtime feeds SenseVoice-Small
// from a 30 s VAD segmenter (its default `max_single_segment_time`), so 30 s is
// the longest utterance the model is exercised on. Used both for the run-time
// WARN and for transcribe_capabilities::max_audio_ms.
constexpr int k_safe_audio_ms = 30000;

extern transcribe_status load(Loader &, const transcribe_model_load_params *, transcribe_model **);
extern transcribe_status init_context(transcribe_model *, const transcribe_session_params *, transcribe_session **);
extern transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<SenseVoiceModel>();
    m->arch      = &arch;
    m->t_load_us = 0;

    m->variant = loader.variant().empty() ? k_default_variant : loader.variant();
    m->backend.clear();

    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (auto st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    if (auto st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = read_sensevoice_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }
    m->hparams.vocab_size = static_cast<int32_t>(m->tok.n_tokens());

    // Publish the soft-window advisory now that the frontend sample rate is
    // known (apply_family_invariants ran before the hparams were read, so it
    // set native_sample_rate but left max_audio_ms unset). SenseVoice is a
    // soft-window family: this is the trained window, not a hard limit — run()
    // WARNs and proceeds past it rather than rejecting. See docs/input-limits.md.
    m->caps.max_audio_ms = k_safe_audio_ms;

    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_sensevoice_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "sensevoice", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "sensevoice");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Construct the kaldi fbank frontend now that the CMVN tensors
    // are bound to backend memory. SenseVoice uses the shared
    // host-side frontend with apply_cmvn=true; pull the CMVN tensors
    // off backend storage into host buffers first.
    {
        const auto &                 hp_ = m->hparams;
        transcribe::KaldiFbankParams fe_params;
        fe_params.n_mels          = hp_.fe_num_mels;
        fe_params.sample_rate     = hp_.fe_sample_rate;
        fe_params.win_length      = hp_.fe_win_length;
        fe_params.hop_length      = hp_.fe_hop_length;
        fe_params.lfr_m           = hp_.fe_lfr_m;
        fe_params.lfr_n           = hp_.fe_lfr_n;
        fe_params.d_input         = hp_.enc_d_input;
        fe_params.upscale_samples = hp_.fe_upscale_samples;
        fe_params.apply_cmvn      = true;
        fe_params.cmvn_shift.resize(static_cast<size_t>(hp_.enc_d_input));
        fe_params.cmvn_scale.resize(static_cast<size_t>(hp_.enc_d_input));
        const size_t cmvn_bytes = static_cast<size_t>(hp_.enc_d_input) * sizeof(float);
        ggml_backend_tensor_get(m->weights.cmvn_shift, fe_params.cmvn_shift.data(), 0, cmvn_bytes);
        ggml_backend_tensor_get(m->weights.cmvn_scale, fe_params.cmvn_scale.data(), 0, cmvn_bytes);
        m->frontend = std::make_unique<transcribe::KaldiFbankFrontend>(std::move(fe_params));
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
    auto cc       = std::make_unique<SenseVoiceSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Resolve the requested language (or "auto") to the row index in the
// 16-row prefix-embedding table (= lid_dict[language]).
int resolve_lid_idx(const SenseVoiceHParams & hp, const char * lang_or_null) {
    if (lang_or_null == nullptr || lang_or_null[0] == '\0') {
        return hp.prefix_lang_auto;
    }
    if (std::strcmp(lang_or_null, "auto") == 0) {
        return hp.prefix_lang_auto;
    }
    if (std::strcmp(lang_or_null, "zh") == 0) {
        return hp.prefix_lang_zh;
    }
    if (std::strcmp(lang_or_null, "en") == 0) {
        return hp.prefix_lang_en;
    }
    if (std::strcmp(lang_or_null, "yue") == 0) {
        return hp.prefix_lang_yue;
    }
    if (std::strcmp(lang_or_null, "ja") == 0) {
        return hp.prefix_lang_ja;
    }
    if (std::strcmp(lang_or_null, "ko") == 0) {
        return hp.prefix_lang_ko;
    }
    if (std::strcmp(lang_or_null, "nospeech") == 0) {
        return hp.prefix_lang_nospeech;
    }
    return hp.prefix_lang_auto;
}

void apply_thread_policy(SenseVoiceSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

// Greedy CTC decode + public result-hierarchy build for ONE utterance's CTC
// log-probabilities. Shared by the single-shot path (run) and the batched
// path (run_batch). `log_probs` is the row-major [T_full, vocab] buffer for
// this utterance (row t at log_probs + t*vocab); `lang` is the caller's
// requested language (or null/auto) and `n_samples` sizes the segment
// duration. Writes the session scratch result slot (token_ids / tokens /
// segments / full_text / detected_language / result_kind / has_result) and
// records cc->t_decode_us.
static transcribe_status decode_and_populate(SenseVoiceSession *           cc,
                                             SenseVoiceModel *             cm,
                                             const transcribe_run_params * params,
                                             const float *                 log_probs,
                                             int                           T_full,
                                             int                           vocab,
                                             const char *                  lang,
                                             int                           n_samples) {
    const auto &  hp          = cm->hparams;
    const int64_t t_dec_start = ggml_time_us();

    // ---------- Greedy CTC decode -------------------------------------
    const int blank_id = 0;
    cc->token_ids.clear();
    cc->token_ids.reserve(static_cast<size_t>(T_full));
    int prev_id = -1;
    for (int t = 0; t < T_full; ++t) {
        const float * row     = log_probs + static_cast<size_t>(t) * vocab;
        int           best_id = 0;
        float         best    = row[0];
        for (int v = 1; v < vocab; ++v) {
            if (row[v] > best) {
                best    = row[v];
                best_id = v;
            }
        }
        if (best_id != prev_id) {
            if (best_id != blank_id) {
                cc->token_ids.push_back(best_id);
            }
            prev_id = best_id;
        }
    }

    // ---------- Build the public result ------------------------------
    if (!cc->token_ids.empty()) {
        const transcribe::Tokenizer & tok = cm->tok;

        // Detected language: SenseVoice's CTC head emits the resolved
        // <|lang|> tag as the first non-blank token when the encoder
        // received the <|auto|> prefix. Only surface it when the caller
        // did NOT pass an explicit hint — the public field's contract
        // is "what the model told us," not "what we told the model."
        // <|nospeech|> and <|auto|> are intentionally not surfaced.
        const bool user_supplied_lang = lang != nullptr && lang[0] != '\0' && std::strcmp(lang, "auto") != 0;
        if (!user_supplied_lang) {
            const int first_id = cc->token_ids.front();
            if (tok.is_control(first_id)) {
                std::string s = tok.decode(&first_id, 1);
                if (!s.empty() && s.front() == ' ') {
                    s.erase(s.begin());
                }
                if (s.size() >= 5 && s.front() == '<' && s.back() == '>' && s[1] == '|' && s[s.size() - 2] == '|') {
                    const std::string code = s.substr(2, s.size() - 4);
                    if (code == "zh" || code == "en" || code == "yue" || code == "ja" || code == "ko") {
                        cc->detected_language = code;
                    }
                }
            }
        }

        // Per-token text fragments (for the public token table). These
        // ALWAYS carry the raw piece — the per-token accessors expose
        // the unfiltered token stream so library callers can observe
        // language/event/emotion/itn control tokens regardless of
        // keep_special_tags.
        cc->tokens.reserve(cc->token_ids.size());
        for (int id : cc->token_ids) {
            transcribe_session::TokenEntry te;
            te.id    = id;
            te.text  = tok.decode(&id, 1);
            te.t0_ms = 0;
            te.t1_ms = 0;
            cc->tokens.push_back(std::move(te));
        }

        // Decode the full sequence into the segment / full_text fields.
        // The decode here may filter out CONTROL-typed tokens depending
        // on keep_special_tags so the user-facing text is clean by
        // default but the raw stream stays accessible above.
        const bool       strip = (params == nullptr) ? true : !params->keep_special_tags;
        std::vector<int> ids_for_text;
        if (strip) {
            ids_for_text.reserve(cc->token_ids.size());
            for (int id : cc->token_ids) {
                if (!tok.is_control(id)) {
                    ids_for_text.push_back(id);
                }
            }
        } else {
            ids_for_text.assign(cc->token_ids.begin(), cc->token_ids.end());
        }
        std::string full = ids_for_text.empty() ?
                               std::string() :
                               tok.decode(ids_for_text.data(), static_cast<int>(ids_for_text.size()));
        if (!full.empty() && full.front() == ' ') {
            full.erase(full.begin());
        }

        transcribe_session::SegmentEntry seg;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(
            std::llround(1000.0 * static_cast<double>(n_samples) / static_cast<double>(hp.fe_sample_rate)));
        seg.first_token = 0;
        seg.n_tokens    = static_cast<int>(cc->tokens.size());
        seg.first_word  = 0;
        seg.n_words     = 0;
        seg.text        = full;

        cc->full_text = std::move(full);
        cc->segments.push_back(std::move(seg));

        // Family default: NONE. Caller-requested kinds finer than NONE
        // are clamped per the dispatcher's contract.
        cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        cc->has_result  = true;
    }

    cc->t_decode_us = ggml_time_us() - t_dec_start;
    return TRANSCRIBE_OK;
}

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<SenseVoiceSession *>(session);
    auto * cm = static_cast<SenseVoiceModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || cm->frontend == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();
    cc->clear_result();

    const auto & hp = cm->hparams;

    cc->t_mel_us    = 0;
    cc->t_encode_us = 0;
    cc->t_decode_us = 0;

    // ---------- Soft-window advisory (see docs/input-limits.md) -------
    // SenseVoice has no hard context cap, but past its ~30 s training
    // window the SAN-M self-attention cost is O(n^2): accuracy degrades and
    // memory grows quadratically. WARN once and proceed (never reject, never
    // change numerics) so the degradation is never silent. The shorter-than-
    // a-frame case is handled separately by the T_lfr <= 0 guard below.
    {
        const int64_t audio_ms = static_cast<int64_t>(n_samples) * 1000 / hp.fe_sample_rate;
        if (audio_ms > k_safe_audio_ms) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                "sensevoice run: audio is %.1f s, beyond the ~%d s window this "
                                "model was trained on; accuracy may degrade and memory use "
                                "grows quadratically. Split long audio into <=%d s segments "
                                "(e.g. with VAD). See transcribe_capabilities.max_audio_ms.",
                                static_cast<double>(audio_ms) / 1000.0, k_safe_audio_ms / 1000, k_safe_audio_ms / 1000);
        }
    }

    // ---------- Frontend (host-side) ----------------------------------
    const int64_t t_mel_start = ggml_time_us();
    const int     T_lfr       = cm->frontend->compute(pcm, static_cast<size_t>(n_samples), cc->frontend_buf);
    cc->t_mel_us              = ggml_time_us() - t_mel_start;

    if (T_lfr <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "sensevoice run: input too short for kaldi fbank "
                "(n_samples=%d → T_lfr=0)",
                n_samples);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int T_full = T_lfr + 4;  // 4 prepended prefix tokens

    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params init_params{};
        // 70 SAN-M blocks * ~30 ops + a few hundred frontend / PE / CTC
        // ops easily fits in 8 MB of metadata arena.
        init_params.mem_size   = 8 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        cc->compute_ctx        = ggml_init(init_params);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "sensevoice run: compute context allocation failed — out of "
                                "memory. Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // ---------- Build the encoder graph -------------------------------
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, T_lfr);
    if (eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ---------- Allocate compute tensors via scheduler ---------------
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (cc->sched == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "sensevoice run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "sensevoice run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    // ---------- Upload inputs ----------------------------------------
    // Frontend output tensor: row-major [T_lfr, d_input], byte-identical
    // to ggml ne=[d_input, T_lfr] (d_input is innermost).
    ggml_backend_tensor_set(eb.frontend_in, cc->frontend_buf.data(), 0, cc->frontend_buf.size() * sizeof(float));

    // Prefix indices.
    const char *  lang         = (params != nullptr) ? params->language : nullptr;
    const int32_t lid_idx      = resolve_lid_idx(hp, lang);
    const int32_t event_emo[2] = { 1, 2 };  // literal indices in the embed table

    // ITN slot. Generic transcribe_run_params::itn routes here. DEFAULT maps
    // to the shipped behavior (use_itn=false; matches the family's
    // `itn=False` Python default). OFF / ON override explicitly. The
    // dispatcher's advisory WARN only fires when transcribe_model_supports(
    // model, TRANSCRIBE_FEATURE_ITN) is false; SenseVoice sets
    // TRANSCRIBE_FEATURE_ITN so the probe returns true and no WARN fires here.
    bool use_itn = false;
    if (params != nullptr) {
        switch (params->itn) {
            case TRANSCRIBE_ITN_MODE_DEFAULT:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_OFF:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_ON:
                use_itn = true;
                break;
        }
    }
    const int32_t textnorm_idx = use_itn ? hp.prefix_withitn : hp.prefix_woitn;

    ggml_backend_tensor_set(eb.lid_idx, &lid_idx, 0, sizeof(int32_t));
    ggml_backend_tensor_set(eb.event_emo_idx, event_emo, 0, 2 * sizeof(int32_t));
    ggml_backend_tensor_set(eb.textnorm_idx, &textnorm_idx, 0, sizeof(int32_t));

    // Sinusoidal PE: depth = current width = d_input (NOT d_model).
    transcribe::sanm::build_sinusoidal_pe(cc->pe_buf, hp.enc_d_input, T_full);
    if (eb.pe_in == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice run: pe.in not found in graph");
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_tensor_set(eb.pe_in, cc->pe_buf.data(), 0, cc->pe_buf.size() * sizeof(float));

    // Optional dump of the input (matches reference's frontend dump).
    transcribe::debug::dump_tensor("frontend.in", eb.frontend_in, "frontend.in");

    apply_thread_policy(cc);

    // ---------- Compute -----------------------------------------------
    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice run: graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // ---------- Dump intermediates -----------------------------------
    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("frontend.fbank.lfr.cmvn.out", eb.dumps.frontend_out, "frontend.lfr.cmvn");
    try_dump("enc.prefix.lid_emb", eb.dumps.prefix_lid, "encoder.prefix.lid");
    try_dump("enc.prefix.event_emo_emb", eb.dumps.prefix_event_emo, "encoder.prefix.event_emo");
    try_dump("enc.prefix.textnorm_emb", eb.dumps.prefix_textnorm, "encoder.prefix.textnorm");
    try_dump("enc.input.with_prefix", eb.dumps.input_with_prefix, "encoder.input.with_prefix");
    try_dump("enc.embed.out", eb.dumps.embed_out, "encoder.embed.pos_added");
    try_dump("enc.encoders0.0.out", eb.dumps.encoders0_0_out, "encoder.encoders0.0");
    if (eb.dumps.encoders_first != nullptr) {
        try_dump("enc.encoders.0.out", eb.dumps.encoders_first, "encoder.encoders.0");
    }
    if (eb.dumps.encoders_mid != nullptr) {
        const char * nm = eb.dumps.encoders_mid->name;
        try_dump(nm, eb.dumps.encoders_mid, "encoder.encoders.mid");
    }
    if (eb.dumps.encoders_last != nullptr) {
        const char * nm = eb.dumps.encoders_last->name;
        try_dump(nm, eb.dumps.encoders_last, "encoder.encoders.last");
    }
    try_dump("enc.after_norm.out", eb.dumps.after_norm_out, "encoder.after_norm");
    if (eb.dumps.tp_encoders_first != nullptr) {
        try_dump("enc.tp_encoders.0.out", eb.dumps.tp_encoders_first, "encoder.tp_encoders.0");
    }
    if (eb.dumps.tp_encoders_mid != nullptr) {
        const char * nm = eb.dumps.tp_encoders_mid->name;
        try_dump(nm, eb.dumps.tp_encoders_mid, "encoder.tp_encoders.mid");
    }
    if (eb.dumps.tp_encoders_last != nullptr) {
        const char * nm = eb.dumps.tp_encoders_last->name;
        try_dump(nm, eb.dumps.tp_encoders_last, "encoder.tp_encoders.last");
    }
    try_dump("enc.tp_norm.out", eb.dumps.tp_norm_out, "encoder.tp_norm");
    try_dump("ctc.logits.raw", eb.dumps.ctc_logits, "ctc.logits.raw");
    try_dump("ctc.log_probs", eb.dumps.ctc_log_probs, "ctc.log_probs");

    // ---------- Read CTC log-probs to host ---------------------------
    const int vocab = hp.vocab_size;
    cc->logits_buf.resize(static_cast<size_t>(T_full) * vocab);
    ggml_backend_tensor_get(eb.out, cc->logits_buf.data(), 0, cc->logits_buf.size() * sizeof(float));

    // ---------- Greedy CTC decode + public result --------------------
    return decode_and_populate(cc, cm, params, cc->logits_buf.data(), T_full, vocab, lang, n_samples);
}

// ---------------------------------------------------------------------------
// Offline batch (transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// SenseVoice is encoder-bound: the SAN-M encoder is the whole device cost and
// the CTC "decoder" is a host-side greedy argmax (no autoregressive loop), so
// only the encoder is fused. B utterances are packed along the activation's
// ne[2] axis and run through ONE encoder dispatch; each utterance's CTC
// log-probs slice is then host-decoded independently. Same-length batches run
// the bit-exact flash path with no masks; variable-length batches pad every
// LFR feature to the batch's T_max and add two per-utterance masks (attention
// key-padding + FSMN valid-frame) so a padded tail cannot corrupt a real
// frame. A malformed utterance forces the whole call onto the per-utterance
// fallback (keeps the batch tensor rectangular). Every utterance's result is
// captured into session->batch_results in order.

static transcribe_status run_batch_encode(
    SenseVoiceSession *                     cc,
    SenseVoiceModel *                       cm,
    const std::vector<std::vector<float>> & feats,  // per utt: [T_lfr, d_input] row-major
    const std::vector<int> &                T_lfr,
    int                                     d_input,
    int                                     T_max_lfr,
    const int *                             n_samples,
    int64_t                                 total_mel_us,
    const transcribe_run_params *           params) {
    const auto & hp = cm->hparams;
    const int    n  = static_cast<int>(feats.size());

    bool var_len = false;
    for (int b = 0; b < n; ++b) {
        if (T_lfr[b] != T_max_lfr) {
            var_len = true;
            break;
        }
    }

    // ---------- Soft-window advisory (see docs/input-limits.md) -------
    // Same warn-and-proceed contract as single-shot run(), applied per
    // utterance on the fused batched encode path so over-window clips in a
    // batch are not silently degraded. The serial fallback in run_batch()
    // re-enters run(), which warns there, so each clip is warned exactly once
    // regardless of which path runs. Never reject, never change numerics.
    if (hp.fe_sample_rate > 0 && n_samples != nullptr) {
        for (int b = 0; b < n; ++b) {
            const int64_t audio_ms = static_cast<int64_t>(n_samples[b]) * 1000 / hp.fe_sample_rate;
            if (audio_ms > k_safe_audio_ms) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                    "sensevoice run: utterance %d: audio is %.1f s, beyond the "
                                    "~%d s window this model was trained on; accuracy may "
                                    "degrade and memory use grows quadratically. Split long "
                                    "audio into <=%d s segments (e.g. with VAD). See "
                                    "transcribe_capabilities.max_audio_ms.",
                                    b, static_cast<double>(audio_ms) / 1000.0, k_safe_audio_ms / 1000,
                                    k_safe_audio_ms / 1000);
            }
        }
    }

    const int T_full_max = T_max_lfr + 4;  // 4 prepended prefix tokens

    // ---------- Pack features into [d_input, T_max_lfr, n] ------------
    // Destination element (d, t, b) lives at (b*T_max_lfr + t)*d_input + d;
    // each utterance's row-major [T_lfr, d_input] copies in directly, with the
    // padded tail (t >= T_lfr[b]) left zero.
    const size_t       per = static_cast<size_t>(d_input) * static_cast<size_t>(T_max_lfr);
    std::vector<float> feat_buf(per * static_cast<size_t>(n), 0.0f);
    for (int b = 0; b < n; ++b) {
        const size_t rows = static_cast<size_t>(T_lfr[b]) * d_input;
        std::copy(feats[b].data(), feats[b].data() + rows, feat_buf.data() + static_cast<size_t>(b) * per);
    }

    // ---------- Reset per-call compute state -------------------------
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params init_params{};
        init_params.mem_size   = 8 * 1024 * 1024;
        init_params.mem_buffer = nullptr;
        init_params.no_alloc   = true;
        cc->compute_ctx        = ggml_init(init_params);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "sensevoice run: compute context allocation failed — out of "
                                "memory. Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, T_max_lfr, /*n_batch=*/n,
                                          /*batch_var_len=*/var_len);
    if (eb.out == nullptr || eb.graph == nullptr || eb.frontend_in == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (cc->sched == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "sensevoice run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "sensevoice run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    // ---------- Upload inputs ----------------------------------------
    ggml_backend_tensor_set(eb.frontend_in, feat_buf.data(), 0, feat_buf.size() * sizeof(float));

    const char *  lang         = (params != nullptr) ? params->language : nullptr;
    const int32_t lid_idx      = resolve_lid_idx(hp, lang);
    const int32_t event_emo[2] = { 1, 2 };
    bool          use_itn      = false;
    if (params != nullptr) {
        switch (params->itn) {
            case TRANSCRIBE_ITN_MODE_DEFAULT:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_OFF:
                use_itn = false;
                break;
            case TRANSCRIBE_ITN_MODE_ON:
                use_itn = true;
                break;
        }
    }
    const int32_t textnorm_idx = use_itn ? hp.prefix_withitn : hp.prefix_woitn;

    ggml_backend_tensor_set(eb.lid_idx, &lid_idx, 0, sizeof(int32_t));
    ggml_backend_tensor_set(eb.event_emo_idx, event_emo, 0, 2 * sizeof(int32_t));
    ggml_backend_tensor_set(eb.textnorm_idx, &textnorm_idx, 0, sizeof(int32_t));

    // Sinusoidal PE (depth = d_input, length = T_full_max). Broadcasts over
    // the batch axis in the graph.
    transcribe::sanm::build_sinusoidal_pe(cc->pe_buf, hp.enc_d_input, T_full_max);
    if (eb.pe_in == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_tensor_set(eb.pe_in, cc->pe_buf.data(), 0, cc->pe_buf.size() * sizeof(float));

    // ---------- Variable-length masks --------------------------------
    // Real post-prefix length per utterance = T_lfr[b] + 4. SenseVoice has no
    // conv subsampling (LFR is done in the host frontend), so the encoder time
    // axis equals the input length.
    std::vector<int> real_T_full(static_cast<size_t>(n), T_full_max);
    for (int b = 0; b < n; ++b) {
        real_T_full[b] = T_lfr[b] + 4;
    }

    if (var_len) {
        // Attention key-padding mask [T_full_max, 1, 1, n] (0 real / -INF padded)
        // and FSMN conv valid-frame mask [1, T_full_max, n] (1 real / 0 padded);
        // both share the host ordering index b*T_full_max + t.
        transcribe::fill_keypad_mask(eb.attn_pad_mask_in, real_T_full, T_full_max, n);
        transcribe::fill_valid_frame_mask(eb.conv_pad_mask_in, real_T_full, T_full_max, n);
    }

    apply_thread_policy(cc);

    // ---------- Compute ----------------------------------------------
    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "sensevoice run_batch: graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // ---------- Read CTC log-probs + per-utterance decode ------------
    const int    vocab     = hp.vocab_size;
    const size_t utt_elems = static_cast<size_t>(vocab) * static_cast<size_t>(T_full_max);
    cc->logits_buf.resize(utt_elems * static_cast<size_t>(n));
    ggml_backend_tensor_get(eb.out, cc->logits_buf.data(), 0, cc->logits_buf.size() * sizeof(float));

    // Host-slice the shared CTC log-probs and decode each utterance, with the
    // single shared encode + total mel cost amortized across the batch.
    return transcribe::decode_batch_slices(
        cc, n, cc->logits_buf.data(), utt_elems, cc->t_encode_us, total_mel_us, [&](int b, const float * lp) {
            // Per-utterance CTC log-probs dump for the batch tensor-parity
            // gate. Same vocab-innermost element order as the single-shot
            // ctc.log_probs dump, so the harness can diff slice-for-slice.
            if (transcribe::debug::enabled()) {
                const long long shape[2] = { real_T_full[b], vocab };
                std::string     nm       = "ctc.log_probs.b" + std::to_string(b);
                transcribe::debug::dump_host_f32(nm.c_str(), lp, static_cast<long long>(real_T_full[b]) * vocab, shape,
                                                 2, "ctc.log_probs");
            }
            return decode_and_populate(cc, cm, params, lp, real_T_full[b], vocab, lang, n_samples[b]);
        });
}

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<SenseVoiceSession *>(session);
    auto * cm = static_cast<SenseVoiceModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || cm->frontend == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    transcribe::debug::init();

    const int d_input = cm->frontend->d_input();

    // Compute each utterance's LFR features. The kaldi-fbank frontend is pure
    // host code with no cross-utterance state and `compute()` is const +
    // allocates its own scratch, so the per-utterance mels are independent and
    // computed in parallel across CPU workers (this is the dominant wall cost
    // when the encoder runs on a fast GPU). A malformed utterance means we
    // cannot pack a rectangular batch tensor, so fall back to the
    // per-utterance path for the whole call.
    std::vector<std::vector<float>> feats(static_cast<size_t>(n));
    std::vector<int>                T_lfr(static_cast<size_t>(n), 0);
    const int64_t                   t_mel_start  = ggml_time_us();
    const bool                      all_ok       = transcribe::parallel_for_all(n, cc->n_threads, [&](int i) -> bool {
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            return false;
        }
        const int t = cm->frontend->compute(pcm[i], static_cast<size_t>(n_samples[i]), feats[i]);
        if (t <= 0) {
            return false;
        }
        T_lfr[i] = t;
        return true;
    });
    const int64_t                   total_mel_us = ggml_time_us() - t_mel_start;

    if (all_ok) {
        int T_max_lfr = 0;
        for (int i = 0; i < n; ++i) {
            T_max_lfr = std::max(T_max_lfr, T_lfr[i]);
        }
        return run_batch_encode(cc, cm, feats, T_lfr, d_input, T_max_lfr, n_samples, total_mel_us, params);
    }

    // Per-utterance fallback (also the malformed-input path).
    for (int i = 0; i < n; ++i) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            transcribe_session::ResultSet rs;
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        const transcribe_status st = run(session, pcm[i], n_samples[i], params);
        cc->batch_results.push_back(cc->capture_result(st));
    }
    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "sensevoice",
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

}  // namespace transcribe::sensevoice
