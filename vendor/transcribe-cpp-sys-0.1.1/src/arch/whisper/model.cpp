// arch/whisper/model.cpp - Whisper ASR family handler.
//
// Normal inference uses the C++ mel frontend and the autoregressive decoder
// path below; TRANSCRIBE_MEL_FROM_REF can inject a reference mel tensor
// to isolate encoder/decoder drift during numerical validation.

#include "decoder.h"
#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-env.h"
#include "transcribe-flash-policy.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-meta.h"
#include "weights.h"
#include "whisper.h"

// miniz (vendored, src/third_party/miniz) replaces system zlib for the
// compression-ratio heuristic below. Built trimmed with
// MINIZ_NO_ZLIB_COMPATIBLE_NAMES, so we call the mz_-prefixed native API.
#include "third_party/miniz/miniz.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <ios>
#include <memory>
#include <random>
#include <string>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

namespace transcribe::whisper {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, WhisperModel>);
static_assert(std::is_base_of_v<transcribe_session, WhisperSession>);

WhisperModel::~WhisperModel() {
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

WhisperSession::~WhisperSession() {
    kv_cache.free();
    enc_out.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
    compute_ctx_size = 0;
}

bool enc_out_init(WhisperEncOut & enc_out, ggml_backend_t backend, int d_model, int T_enc) {
    enc_out.free();

    const size_t     ctx_size = ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    enc_out.ctx = ggml_init(params);
    if (enc_out.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper enc_out: ggml_init failed");
        return false;
    }

    enc_out.tensor = ggml_new_tensor_2d(enc_out.ctx, GGML_TYPE_F32, d_model, T_enc);
    ggml_set_name(enc_out.tensor, "enc_out");

    enc_out.buffer = ggml_backend_alloc_ctx_tensors(enc_out.ctx, backend);
    if (enc_out.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper enc_out: buffer alloc failed");
        ggml_free(enc_out.ctx);
        enc_out.ctx    = nullptr;
        enc_out.tensor = nullptr;
        return false;
    }

    enc_out.d_model = d_model;
    enc_out.T_enc   = T_enc;
    return true;
}

int kv_pad_self_attn(transcribe::BackendKind kind, bool use_flash) {
    if (!use_flash) {
        return 1;
    }
    switch (kind) {
        // Match whisper.cpp's whisper_kv_cache_get_padding (Metal+FA).
        case transcribe::BackendKind::Metal:
            return 32;
        // CUDA uses 256 in whisper.cpp; not exercised here yet.
        default:
            return 1;
    }
}

bool kv_cache_init(WhisperKvCache & cache,
                   ggml_backend_t   backend,
                   int              n_ctx,
                   int              T_enc,
                   int              d_model,
                   int              n_layer,
                   ggml_type        kv_type) {
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper kv_cache: unsupported kv_type=%d "
                "(only F16/F32)",
                static_cast<int>(kv_type));
        return false;
    }

    const size_t     ctx_size = 4 * ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper kv_cache: ggml_init failed");
        return false;
    }

    // Cross K/V allocated at GGML_PAD(T_enc, 256) rows per layer so
    // the FA op consumes a sequence dim that is a multiple of the
    // Metal kernel's block size. Only the first T_enc rows are
    // written by build_cross_kv_graph; the trailing rows stay zero
    // (buffer_clear below). With K=V=0 in the padded slots, the
    // unmasked FA cross-attn output picks up a small dilution
    // factor — whisper.cpp ships with this trade-off.
    const int     T_enc_pad      = static_cast<int>(GGML_PAD(T_enc, k_cross_kv_pad));
    const int64_t self_elements  = static_cast<int64_t>(d_model) * n_layer * n_ctx;
    const int64_t cross_elements = static_cast<int64_t>(d_model) * n_layer * T_enc_pad;

    cache.self_k  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.self_v  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.cross_k = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    cache.cross_v = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);

    ggml_set_name(cache.self_k, "kv_self_k");
    ggml_set_name(cache.self_v, "kv_self_v");
    ggml_set_name(cache.cross_k, "kv_cross_k");
    ggml_set_name(cache.cross_v, "kv_cross_v");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper kv_cache: buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx           = n_ctx;
    cache.n_batch         = 1;
    cache.T_enc           = T_enc;
    cache.T_enc_pad       = T_enc_pad;
    cache.n               = 0;
    cache.head            = 0;
    cache.cross_populated = false;

    return true;
}

bool kv_cache_init_batched(WhisperKvCache & cache,
                           ggml_backend_t   backend,
                           int              n_ctx,
                           int              T_enc,
                           int              d_model,
                           int              n_layer,
                           int              n_batch,
                           ggml_type        kv_type) {
    if (n_batch <= 1) {
        // Degenerate batch — defer to the single-shot layout so callers
        // that accidentally pass n_batch==1 stay byte-identical.
        return kv_cache_init(cache, backend, n_ctx, T_enc, d_model, n_layer, kv_type);
    }
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper kv_cache(batched): unsupported kv_type=%d",
                static_cast<int>(kv_type));
        return false;
    }

    cache.free();

    const size_t     ctx_size = 4 * ggml_tensor_overhead() + 256;
    ggml_init_params params{};
    params.mem_size   = ctx_size;
    params.mem_buffer = nullptr;
    params.no_alloc   = true;

    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper kv_cache(batched): ggml_init failed");
        return false;
    }

    // No 256-row cross pad in the batched layout: every short-form
    // utterance has T_enc == 1500 and the batched cross graph writes the
    // full slab; the per-utterance cross-pad mask gates invalid columns.
    const int64_t self_elements  = static_cast<int64_t>(d_model) * n_ctx * n_batch * n_layer;
    const int64_t cross_elements = static_cast<int64_t>(d_model) * T_enc * n_batch * n_layer;

    cache.self_k  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.self_v  = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.cross_k = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    cache.cross_v = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);

    ggml_set_name(cache.self_k, "kv_self_k_b");
    ggml_set_name(cache.self_v, "kv_self_v_b");
    ggml_set_name(cache.cross_k, "kv_cross_k_b");
    ggml_set_name(cache.cross_v, "kv_cross_v_b");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper kv_cache(batched): buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx           = n_ctx;
    cache.n_batch         = n_batch;
    cache.T_enc           = T_enc;
    cache.T_enc_pad       = T_enc;  // no extra pad in batched layout
    cache.n               = 0;
    cache.head            = 0;
    cache.cross_populated = false;

    return true;
}

namespace {

constexpr const char k_default_variant[] = "whisper";

// Prepare cc->compute_ctx for a graph build of capacity at most `mem`. If a
// context already exists and is large enough, ggml_reset clears its metadata
// in-place (cheap); only a too-small context triggers ggml_free + ggml_init.
bool ensure_compute_ctx(WhisperSession * cc, size_t mem) {
    if (cc->compute_ctx != nullptr) {
        if (cc->compute_ctx_size >= mem) {
            ggml_reset(cc->compute_ctx);
            return true;
        }
        ggml_free(cc->compute_ctx);
        cc->compute_ctx      = nullptr;
        cc->compute_ctx_size = 0;
    }

    ggml_init_params p{};
    p.mem_size      = mem;
    p.mem_buffer    = nullptr;
    p.no_alloc      = true;
    cc->compute_ctx = ggml_init(p);
    if (cc->compute_ctx == nullptr) {
        return false;
    }
    cc->compute_ctx_size = mem;
    return true;
}

// Print the per-stage performance summary collected during the most
// recent whisper_run. Opt-in via TRANSCRIBE_PERF_DEBUG — gated
// by the caller, this helper just formats the counters. Output is one
// summary line per stage (encoder / cross / prompt / step) plus a
// per-step average for the steady-state generation loop, which is the
// metric most directly comparable to whisper.cpp's bench numbers.
void print_whisper_perf(const WhisperPerf & p) {
    auto avg_us = [](const WhisperPerfStage & s) -> double {
        return s.count > 0 ? static_cast<double>(s.total_us) / static_cast<double>(s.count) : 0.0;
    };
    auto ms = [](int64_t us) -> double {
        return static_cast<double>(us) / 1000.0;
    };
    auto stage_total = [&](const WhisperPerfStage & s) {
        return s.total_us;
    };

    const int64_t enc_total    = stage_total(p.enc_build) + stage_total(p.enc_alloc) + stage_total(p.enc_compute) +
                                 stage_total(p.enc_tensor_get);
    const int64_t cross_total  = stage_total(p.cross_build) + stage_total(p.cross_alloc) + stage_total(p.cross_compute);
    const int64_t prompt_total = stage_total(p.prompt_build) + stage_total(p.prompt_alloc) +
                                 stage_total(p.prompt_compute) + stage_total(p.prompt_tensor_get) +
                                 stage_total(p.prompt_cpu);
    const int64_t step_total   = stage_total(p.step_build) + stage_total(p.step_alloc) + stage_total(p.step_compute) +
                                 stage_total(p.step_tensor_get) + stage_total(p.step_cpu);

    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[whisper-perf] chunks=%d  encs=%d  crosses=%d  prompts=%d  steps=%d", p.chunks,
            p.enc_compute.count, p.cross_compute.count, p.prompt_compute.count, p.step_compute.count);
    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
            "[whisper-perf] enc    total=%7.2f ms  build=%7.2f  alloc=%7.2f  "
            "compute=%7.2f  tget=%7.2f",
            ms(enc_total), ms(p.enc_build.total_us), ms(p.enc_alloc.total_us), ms(p.enc_compute.total_us),
            ms(p.enc_tensor_get.total_us));
    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
            "[whisper-perf] cross  total=%7.2f ms  build=%7.2f  alloc=%7.2f  "
            "compute=%7.2f",
            ms(cross_total), ms(p.cross_build.total_us), ms(p.cross_alloc.total_us), ms(p.cross_compute.total_us));
    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
            "[whisper-perf] prompt total=%7.2f ms  build=%7.2f  alloc=%7.2f  "
            "compute=%7.2f  tget=%7.2f  cpu=%7.2f",
            ms(prompt_total), ms(p.prompt_build.total_us), ms(p.prompt_alloc.total_us), ms(p.prompt_compute.total_us),
            ms(p.prompt_tensor_get.total_us), ms(p.prompt_cpu.total_us));
    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
            "[whisper-perf] step   total=%7.2f ms  build=%7.2f  alloc=%7.2f  "
            "compute=%7.2f  tget=%7.2f  cpu=%7.2f",
            ms(step_total), ms(p.step_build.total_us), ms(p.step_alloc.total_us), ms(p.step_compute.total_us),
            ms(p.step_tensor_get.total_us), ms(p.step_cpu.total_us));
    log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
            "[whisper-perf] step avg us  build=%6.1f  alloc=%6.1f  "
            "compute=%6.1f  tget=%6.1f  cpu=%6.1f",
            avg_us(p.step_build), avg_us(p.step_alloc), avg_us(p.step_compute), avg_us(p.step_tensor_get),
            avg_us(p.step_cpu));

    // CPU sub-section breakdown — opt-in via TRANSCRIBE_PERF_DEBUG values
    // that contain "cpu" or "all". Keeps the default profile output
    // unchanged for existing benchmarks while still surfacing
    // suppress/timestamp/sample/logprob splits when needed.
    const char * profile_env        = transcribe::env::str("TRANSCRIBE_PERF_DEBUG");
    bool         show_cpu_breakdown = false;
    if (profile_env != nullptr) {
        const std::string v = profile_env;
        if (v.find("cpu") != std::string::npos || v.find("all") != std::string::npos) {
            show_cpu_breakdown = true;
        }
    }
    if (show_cpu_breakdown) {
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[whisper-perf] prompt cpu  suppress=%7.2f  ts=%7.2f  "
                "sample=%7.2f  logprob=%7.2f  (ms)",
                ms(p.prompt_cpu_suppress.total_us), ms(p.prompt_cpu_timestamp.total_us),
                ms(p.prompt_cpu_sample.total_us), ms(p.prompt_cpu_logprob.total_us));
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[whisper-perf] step   cpu  suppress=%7.2f  ts=%7.2f  "
                "sample=%7.2f  logprob=%7.2f  (ms)",
                ms(p.step_cpu_suppress.total_us), ms(p.step_cpu_timestamp.total_us), ms(p.step_cpu_sample.total_us),
                ms(p.step_cpu_logprob.total_us));
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[whisper-perf] step avg us  suppress=%6.1f  ts=%6.1f  "
                "sample=%6.1f  logprob=%6.1f",
                avg_us(p.step_cpu_suppress), avg_us(p.step_cpu_timestamp), avg_us(p.step_cpu_sample),
                avg_us(p.step_cpu_logprob));
    }
}

bool whisper_perf_enabled() {
    return transcribe::env::flag("TRANSCRIBE_PERF_DEBUG");
}

// load
transcribe_status whisper_load(Loader &                             loader,
                               const transcribe_model_load_params * params,
                               transcribe_model **                  out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<WhisperModel>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = loader.variant().empty() ? k_default_variant : loader.variant();
    m->backend.clear();

    // Hparams first: the frontend / encoder config and the decoder
    // vocab size come from here.
    if (const transcribe_status st = read_whisper_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Tokenizer. Whisper GGUFs carry a "gpt2"-family (HF byte-level BPE)
    // tokenizer. DO NOT overwrite vocab_size from the tokenizer: the decoder
    // vocab (51865 multilingual) exceeds tokenizer.tokens (50258) because
    // language/timestamp/task specials are model-emitted but not in the tokens
    // array. That gap is by design.
    if (const transcribe_status st = m->tok.load(loader.gguf()); st != TRANSCRIBE_OK) {
        return st;
    }
    // Whisper uses the canonical GPT-2 pretokenizer regex (digit runs
    // merged, lowercase-only contractions). Force it here so older
    // converter outputs without tokenizer.ggml.pre still tokenize
    // text correctly — the absent-key default "qwen2" would split
    // multi-digit runs and diverge from the HF reference.
    if (m->tok.pretokenizer() != "gpt2") {
        m->tok.set_pretokenizer("gpt2");
    }

    // Mel frontend. Whisper-style 80-bin log-mel at 16 kHz, with
    // per-utterance log10 + clamp(max-8) + (+4)/4 normalization,
    // periodic Hann window, and reflect padding around the centered
    // STFT. Filterbank + window come baked in the GGUF (the converter
    // serializes them verbatim from preprocessor_config.json) so the
    // C++ frontend matches the WhisperFeatureExtractor reference
    // bit-for-bit modulo fp32 reduction-order drift.
    {
        // Filterbank + window from the GGUF (frontend.mel_filterbank,
        // frontend.window). Both are F32 arrays whose shapes match
        // what MelConfig wants (row-major [num_mels, n_fft/2+1] and
        // [n_fft] respectively). When present, MelFrontend uses them
        // verbatim instead of reconstructing from cfg constants.
        using R = transcribe::load_common::ReadF32Result;
        std::vector<float> fb_buf;
        std::vector<float> win_buf;

        const size_t fb_elems =
            static_cast<size_t>(m->hparams.fe_num_mels) * static_cast<size_t>(m->hparams.fe_n_fft / 2 + 1);
        const auto fb_rc = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "whisper", fb_buf);
        if (fb_rc != R::Ok && fb_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }
        const size_t win_elems = static_cast<size_t>(m->hparams.fe_n_fft);
        const auto   win_rc    = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.window", win_elems, "whisper", win_buf);
        if (win_rc != R::Ok && win_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }

        if (const transcribe_status st =
                install_mel_from_buffers(m->hparams, std::move(fb_buf), std::move(win_buf), m->mel);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    // Reopen with no_alloc to build the tensor catalog.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = build_whisper_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Backend plan.
    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "whisper", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "whisper");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Capabilities: apply family invariants, then let the GGUF KV
    // override, then install the language list from general.languages.
    apply_family_invariants(*m);
    m->caps.n_languages = 0;
    m->caps.languages   = nullptr;

    if (const transcribe_status st = read_capability_kv(loader.gguf(), m->caps); st != TRANSCRIBE_OK) {
        return st;
    }
    if (const transcribe_status st = read_languages_kv(loader.gguf(), *m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Resolve per-language token ids and keep an owned copy of the language
    // list (decoder code reads m->lang_codes directly rather than walking the
    // capability chain). Hydrating here surfaces a vocab drift at load.
    //
    // .en models advertise ["en"] but their vocab has no "<|en|>" token (the
    // .en prefix is bare <|sot|>), so keep lang_codes (to accept
    // params.language == "en") but leave lang_token_ids empty; the run path
    // gates <|lang|>/<|task|> emission on supports_language_detect.
    m->lang_codes.clear();
    m->lang_token_ids.clear();
    const bool is_multilingual = m->caps.supports_language_detect;
    if (m->caps.languages != nullptr && m->caps.n_languages > 0) {
        m->lang_codes.reserve(static_cast<size_t>(m->caps.n_languages));
        if (is_multilingual) {
            m->lang_token_ids.reserve(static_cast<size_t>(m->caps.n_languages));
        }
        for (int i = 0; i < m->caps.n_languages; ++i) {
            const char * code = m->caps.languages[i];
            if (code == nullptr || code[0] == '\0') {
                continue;
            }
            m->lang_codes.emplace_back(code);
            if (!is_multilingual) {
                continue;
            }
            const std::string piece = std::string("<|") + code + "|>";
            const int         id    = m->tok.find(piece);
            if (id < 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "whisper: language '%s' has no '%s' token "
                        "in tokenizer vocab",
                        code, piece.c_str());
                return TRANSCRIBE_ERR_GGUF;
            }
            m->lang_token_ids.push_back(static_cast<int32_t>(id));
        }
    }

    m->t_load_us = ggml_time_us() - t_load_start;
    *out_model   = m.release();
    return TRANSCRIBE_OK;
}

// init_context
transcribe_status whisper_init_context(transcribe_model *                model,
                                       const transcribe_session_params * params,
                                       transcribe_session **             out_ctx) {
    if (model->arch != &arch) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto cc       = std::make_unique<WhisperSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;

    // Whisper defaults: both flash-on (see WhisperSession). Env overrides below.
    cc->encoder_use_flash = true;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// run

namespace {

// Run the encoder on one mel window, leaving the output in the backend-resident
// cc->enc_out.tensor (F32 [d_enc, T_enc]) that downstream graphs read via views
// (no host roundtrip). cc->enc_host is populated on demand only for language
// detection's prefill graph. Publishes T_enc on the context.
//
// mel_data is in encoder layout [n_mels, n_mel_frames] (n_mels innermost).
// Shipped variants use n_mel_frames=3000; shorter long-form windows must be
// zero-padded on the right before calling.
//
// allow_dumps gates encoder-intermediate emission: true for the first chunk
// (and short-form), false afterwards so the long-form loop doesn't overwrite
// the first chunk's dumps.
transcribe_status run_whisper_encoder_on_window(WhisperSession * cc,
                                                WhisperModel *   cm,
                                                const float *    mel_data,
                                                int              n_mels,
                                                int              n_mel_frames,
                                                bool             allow_dumps,
                                                int &            out_T_enc) {
    const int64_t t_enc_build_start = ggml_time_us();
    if (!ensure_compute_ctx(cc, 8 * 1024 * 1024)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: ensure_compute_ctx (encoder) failed");
        return TRANSCRIBE_ERR_GGUF;
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, n_mel_frames,
                                          cc->encoder_use_flash, cm->backend.c_str());
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Backend-resident encoder output: allocate the persistent F32 tensor once
    // (re-allocate on T_enc / d_model change), then append a ggml_cpy from
    // eb.out into a view of it — a single intra-backend memcpy that stays off
    // the host. Subsequent graphs read cc->enc_out.tensor directly.
    {
        const int d_enc_g = static_cast<int>(eb.out->ne[0]);
        const int T_enc_g = static_cast<int>(eb.out->ne[1]);
        if (cc->enc_out.tensor == nullptr || cc->enc_out.d_model != d_enc_g || cc->enc_out.T_enc != T_enc_g) {
            if (!enc_out_init(cc->enc_out, cm->plan.primary, d_enc_g, T_enc_g)) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: enc_out_init failed");
                return TRANSCRIBE_ERR_GGUF;
            }
        }
        ggml_tensor * enc_out_view =
            ggml_view_2d(cc->compute_ctx, cc->enc_out.tensor, d_enc_g, T_enc_g, cc->enc_out.tensor->nb[1], 0);
        ggml_build_forward_expand(eb.graph, ggml_cpy(cc->compute_ctx, eb.out, enc_out_view));
    }
    cc->perf.enc_build.add(ggml_time_us() - t_enc_build_start);

    // Allocate + compute encoder graph.
    const int64_t t_enc_alloc_start = ggml_time_us();
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        // Apply the caller's CPU thread count once at sched creation; it
        // persists across the reused encoder/decoder graphs. GPU backends
        // ignore it, CPU/BLAS use it. Without this, compute ran at ggml's
        // default count regardless of params.n_threads.
        transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper run: ggml_backend_sched_alloc_graph failed "
                "(encoder)");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Upload mel.
    const size_t mel_bytes = static_cast<size_t>(n_mels) * static_cast<size_t>(n_mel_frames) * sizeof(float);
    ggml_backend_tensor_set(eb.mel_in, mel_data, 0, mel_bytes);
    cc->perf.enc_alloc.add(ggml_time_us() - t_enc_alloc_start);

    // Compute.
    const int64_t t_enc_compute_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: encoder graph compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->perf.enc_compute.add(ggml_time_us() - t_enc_compute_start);

    // Dumps. Only the first (or only) chunk emits intermediates so the
    // reference comparison sees stable output.
    if (allow_dumps) {
        auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
            if (t != nullptr) {
                transcribe::debug::dump_tensor(name, t, stage);
            }
        };
        try_dump("enc.mel.in", eb.dumps.mel_in, "encoder.mel");
        try_dump("enc.conv1.out", eb.dumps.conv1_out, "encoder.conv1");
        try_dump("enc.conv2.out", eb.dumps.conv2_out, "encoder.conv2");
        try_dump("enc.pos_emb", eb.dumps.pos_emb, "encoder.pos_emb");
        try_dump("enc.embed.out", eb.dumps.embed_out, "encoder.embed");
        for (size_t i = 0; i < eb.dumps.block_outs.size(); ++i) {
            char bname[64], stage[64];
            std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
            std::snprintf(stage, sizeof(stage), "encoder.block%zu.out", i);
            try_dump(bname, eb.dumps.block_outs[i], stage);
        }
        try_dump("enc.final", eb.dumps.final_out, "encoder.final");
    }

    // enc_out is now populated; cross-KV reads it directly. Lang-detect
    // materializes to enc_host lazily (see chunk loop).
    const int d_enc = static_cast<int>(eb.out->ne[0]);
    out_T_enc       = static_cast<int>(eb.out->ne[1]);
    cc->enc_T       = out_T_enc;
    (void) d_enc;
    return TRANSCRIBE_OK;
}

// HF _retrieve_compression_ratio (generation_whisper.py:1948-1954).
// Packs each token id as little-endian with width = floor(log2(V)/8) + 1
// bytes, deflate-compresses, returns len(raw)/len(compressed). For Whisper's
// vocab (≤ 65536) the width is 2. ratio > threshold ⇒ decoder is
// repeating token ids, triggers temperature escalation.
//
// HF uses Python's zlib; we use vendored miniz (also level-6 deflate). The
// compressed byte counts can differ by a few bytes between the two deflate
// implementations, so this ratio is not bit-identical to the HF reference.
// Accepted intentionally: the metric only gates a coarse 2.4 repetition
// threshold, not numeric output, so small deltas do not move WER.
//
// Zero-length input returns 0 (no raw bytes to score).
float compute_compression_ratio_hf(const std::vector<int32_t> & tokens, int64_t vocab_size) {
    if (tokens.empty()) {
        return 0.0f;
    }
    int bytes_per_token = 1;
    if (vocab_size > 1) {
        const double lg2 = std::log2(static_cast<double>(vocab_size));
        bytes_per_token  = static_cast<int>(std::floor(lg2 / 8.0)) + 1;
    }

    std::vector<uint8_t> raw(tokens.size() * static_cast<size_t>(bytes_per_token));
    for (size_t i = 0; i < tokens.size(); ++i) {
        uint64_t v = static_cast<uint64_t>(static_cast<uint32_t>(tokens[i]));  // Whisper ids fit in u32
        for (int b = 0; b < bytes_per_token; ++b) {
            raw[i * bytes_per_token + b] = static_cast<uint8_t>((v >> (8 * b)) & 0xFF);
        }
    }

    mz_ulong                   dest_len = mz_compressBound(static_cast<mz_ulong>(raw.size()));
    std::vector<unsigned char> compressed(dest_len);
    const int rc = mz_compress(compressed.data(), &dest_len, raw.data(), static_cast<mz_ulong>(raw.size()));
    if (rc != MZ_OK || dest_len == 0) {
        // Degenerate: treat as unratio-able so thresholds pass.
        return 0.0f;
    }
    return static_cast<float>(raw.size()) / static_cast<float>(dest_len);
}

// Sample from a distribution defined by last_logits at temperature T.
// T == 0 ⇒ argmax (deterministic). T > 0 ⇒ multinomial sample over
// softmax(logits / T). Returns the sampled token id.
//
// Matches HF's sampling semantics (probability ∝ exp(logit/T)) in the
// numerically stable form with max-subtracted exponentials.
//
// -INFINITY logits (from suppress/timestamp rules) contribute 0 mass.
//
// `scratch` is reused across calls to avoid the per-step double[vocab] alloc on
// the T>0 hot path; sized lazily.
int sample_from_logits(const std::vector<float> & logits,
                       float                      temperature,
                       std::mt19937 &             rng,
                       std::vector<double> *      scratch = nullptr) {
    const int n = static_cast<int>(logits.size());
    if (temperature <= 0.0f) {
        int   best_id = 0;
        float best    = logits[0];
        for (int i = 1; i < n; ++i) {
            if (logits[i] > best) {
                best    = logits[i];
                best_id = i;
            }
        }
        return best_id;
    }

    // Multinomial sampling.
    // Stabilise: subtract max finite value before exp.
    float max_l = -INFINITY;
    for (int i = 0; i < n; ++i) {
        const float l = logits[i];
        if (std::isfinite(l) && l > max_l) {
            max_l = l;
        }
    }
    if (!std::isfinite(max_l)) {
        // All -INF (shouldn't happen); fall back to argmax.
        return 0;
    }

    std::vector<double>   local_probs;
    std::vector<double> & probs = (scratch != nullptr) ? *scratch : local_probs;
    if (probs.size() < static_cast<size_t>(n)) {
        probs.resize(static_cast<size_t>(n));
    }
    // Zero only the prefix we will read in the cumulative pass; the
    // tail past n is irrelevant.
    std::fill_n(probs.begin(), n, 0.0);

    double sum = 0.0;
    for (int i = 0; i < n; ++i) {
        const float l = logits[i];
        if (std::isfinite(l)) {
            const double p                = std::exp(static_cast<double>((l - max_l) / temperature));
            probs[static_cast<size_t>(i)] = p;
            sum += p;
        }
    }
    if (sum <= 0.0) {
        // Defensive: argmax fallback.
        int best_id = 0;
        for (int i = 1; i < n; ++i) {
            if (logits[i] > logits[best_id]) {
                best_id = i;
            }
        }
        return best_id;
    }

    std::uniform_real_distribution<double> u(0.0, sum);
    const double                           r   = u(rng);
    double                                 acc = 0.0;
    for (int i = 0; i < n; ++i) {
        acc += probs[static_cast<size_t>(i)];
        if (r < acc) {
            return i;
        }
    }
    return n - 1;  // numerical edge
}

// Fused argmax + log-softmax for the T == 0 (greedy) path.
//
// Returns the argmax token id; *out_logprob receives logprob_of_token_hf
// of that token at temperature 0 (which uses rescale_T = 1.0). Two
// passes total: pass 1 finds max_l + argmax, pass 2 sums exp.
//
// Numerics: identical to running sample_from_logits(T=0) +
// logprob_of_token_hf(T=0) on the same buffer. -INFINITY logits
// contribute zero mass to the partition; if every entry is -INF
// (shouldn't happen) the function returns id 0 with logprob -inf,
// matching the existing fallback.
int sample_argmax_and_logprob(const std::vector<float> & logits, float * out_logprob) {
    const int n       = static_cast<int>(logits.size());
    int       best_id = 0;
    float     best_l  = logits[0];
    float     max_l   = std::isfinite(best_l) ? best_l : -INFINITY;
    for (int i = 1; i < n; ++i) {
        const float l = logits[i];
        if (l > best_l) {
            best_l  = l;
            best_id = i;
        }
        if (std::isfinite(l) && l > max_l) {
            max_l = l;
        }
    }

    if (out_logprob != nullptr) {
        if (!std::isfinite(max_l)) {
            *out_logprob = -INFINITY;
        } else {
            double sum_exp = 0.0;
            for (int i = 0; i < n; ++i) {
                const float l = logits[i];
                if (std::isfinite(l)) {
                    sum_exp += std::exp(static_cast<double>(l - max_l));
                }
            }
            if (sum_exp <= 0.0) {
                *out_logprob = -INFINITY;
            } else {
                const float log_Z = max_l + static_cast<float>(std::log(sum_exp));
                *out_logprob      = best_l - log_Z;
            }
        }
    }
    return best_id;
}

// Compute log_softmax(logits * rescale_T)[token_id] in the numerically
// stable form (subtract max before exp). rescale_T = T if T > 0 else 1
// — matches HF _retrieve_avg_logprobs scaling convention (the logits
// stay the same shape but their relative gaps widen at low T, which is
// the HF semantic choice).
float logprob_of_token_hf(const std::vector<float> & logits, int token_id, float temperature) {
    const int n = static_cast<int>(logits.size());
    if (token_id < 0 || token_id >= n) {
        return -INFINITY;
    }
    const float rescale_T = (temperature > 0.0f) ? temperature : 1.0f;
    float       max_l     = -INFINITY;
    for (int i = 0; i < n; ++i) {
        const float l = logits[i] * rescale_T;
        if (std::isfinite(l) && l > max_l) {
            max_l = l;
        }
    }
    if (!std::isfinite(max_l)) {
        return -INFINITY;
    }
    double sum_exp = 0.0;
    for (int i = 0; i < n; ++i) {
        const float l = logits[i] * rescale_T;
        if (std::isfinite(l)) {
            sum_exp += std::exp(static_cast<double>(l - max_l));
        }
    }
    if (sum_exp <= 0.0) {
        return -INFINITY;
    }
    const float log_Z = max_l + static_cast<float>(std::log(sum_exp));
    return logits[token_id] * rescale_T - log_Z;
}

#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
// Load 3000 * 80 f32 floats from <dir>/enc.mel.in.f32. Layout on disk
// matches the reference dump: row-major (3000, 80) — exactly what we
// need to upload into the ggml ne=[80, 3000] input tensor via a flat
// memcpy.
transcribe_status load_mel_from_ref(const char * ref_dir, int n_mels, int n_mel_frames, std::vector<float> & out) {
    if (ref_dir == nullptr || ref_dir[0] == '\0') {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    std::string path = ref_dir;
    if (!path.empty() && path.back() != '/') {
        path += '/';
    }
    path += "enc.mel.in.f32";

    std::ifstream f(path, std::ios::binary);
    if (!f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: cannot open mel ref '%s'", path.c_str());
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }

    const size_t n_elems = static_cast<size_t>(n_mels) * static_cast<size_t>(n_mel_frames);
    const size_t n_bytes = n_elems * sizeof(float);

    out.assign(n_elems, 0.0f);
    f.read(reinterpret_cast<char *>(out.data()), static_cast<std::streamsize>(n_bytes));
    if (static_cast<size_t>(f.gcount()) != n_bytes) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper run: short read on mel ref '%s' "
                "(got %lld bytes, want %zu)",
                path.c_str(), static_cast<long long>(f.gcount()), n_bytes);
        return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_OK;
}
#endif  // TRANSCRIBE_ENABLE_VALIDATION_HOOKS

}  // namespace

namespace {

// Whisper timestamp logits processor, shared by whisper_run and
// whisper_run_batch so per-step masking is identical. Mirrors transformers'
// WhisperTimeStampLogitsProcessor. Mutates `logits` in place; no-op when
// want_segment_timestamps is false.
void apply_whisper_timestamp_rules(std::vector<float> &         logits,
                                   const std::vector<int32_t> & generated_ids,
                                   bool                         want_segment_timestamps,
                                   int                          no_timestamps_token_id,
                                   int                          timestamp_begin,
                                   int64_t                      vocab_size,
                                   int                          eos_id,
                                   int                          max_initial_timestamp_index) {
    if (!want_segment_timestamps) {
        return;
    }
    auto token_is_timestamp = [&](int id) {
        return id >= timestamp_begin && id < static_cast<int>(vocab_size);
    };
    if (no_timestamps_token_id >= 0 && no_timestamps_token_id < vocab_size) {
        logits[static_cast<size_t>(no_timestamps_token_id)] = -INFINITY;
    }

    const bool last_ts   = !generated_ids.empty() && token_is_timestamp(generated_ids.back());
    const bool penult_ts = generated_ids.size() < 2 || token_is_timestamp(generated_ids[generated_ids.size() - 2]);

    if (last_ts) {
        if (penult_ts) {
            for (int i = timestamp_begin; i < static_cast<int>(vocab_size); ++i) {
                logits[static_cast<size_t>(i)] = -INFINITY;
            }
        } else {
            const int mask_hi = std::min(eos_id, static_cast<int>(vocab_size));
            for (int i = 0; i < mask_hi; ++i) {
                logits[static_cast<size_t>(i)] = -INFINITY;
            }
        }
    }

    int last_ts_id = -1;
    for (auto it = generated_ids.rbegin(); it != generated_ids.rend(); ++it) {
        if (token_is_timestamp(*it)) {
            last_ts_id = *it;
            break;
        }
    }
    if (last_ts_id >= 0) {
        const int ts_low   = (last_ts && !penult_ts) ? last_ts_id : last_ts_id + 1;
        const int clamp_hi = std::min(ts_low, static_cast<int>(vocab_size));
        for (int i = timestamp_begin; i < clamp_hi; ++i) {
            logits[static_cast<size_t>(i)] = -INFINITY;
        }
    }

    if (generated_ids.empty()) {
        for (int i = 0; i < timestamp_begin; ++i) {
            logits[static_cast<size_t>(i)] = -INFINITY;
        }
        if (max_initial_timestamp_index >= 0) {
            const int last_allowed = timestamp_begin + max_initial_timestamp_index;
            for (int i = last_allowed + 1; i < static_cast<int>(vocab_size); ++i) {
                logits[static_cast<size_t>(i)] = -INFINITY;
            }
        }
    }

    float max_ts = -INFINITY;
    for (int i = timestamp_begin; i < static_cast<int>(vocab_size); ++i) {
        max_ts = std::max(max_ts, logits[static_cast<size_t>(i)]);
    }
    float ts_logsumexp = -INFINITY;
    if (std::isfinite(max_ts)) {
        double sum = 0.0;
        for (int i = timestamp_begin; i < static_cast<int>(vocab_size); ++i) {
            const float v = logits[static_cast<size_t>(i)];
            if (std::isfinite(v)) {
                sum += std::exp(static_cast<double>(v - max_ts));
            }
        }
        if (sum > 0.0) {
            ts_logsumexp = max_ts + static_cast<float>(std::log(sum));
        }
    }
    float max_text = -INFINITY;
    for (int i = 0; i < timestamp_begin; ++i) {
        max_text = std::max(max_text, logits[static_cast<size_t>(i)]);
    }
    if (ts_logsumexp > max_text) {
        for (int i = 0; i < timestamp_begin; ++i) {
            logits[static_cast<size_t>(i)] = -INFINITY;
        }
    }
}

// Result of _retrieve_segment: the emitted segments (only when timestamps
// requested), the per-segment token slices used to seed the next chunk's
// prev-context, and how far `seek` should advance (long-form only).
struct WhisperSegmentResult {
    std::vector<transcribe_session::SegmentEntry> segments;
    std::vector<std::vector<int32_t>>             prev_chunk_segments;
    int                                           segment_offset_frames = 0;
};

// Port of HF generation_whisper.py:_retrieve_segment, shared by serial +
// batched decode. Pure: reads generated_ids + the tokenizer, returns the
// segments/slices/offset instead of mutating session state. (Short-form
// callers pass time_offset_ms 0 and consume only `.segments`.)
WhisperSegmentResult whisper_retrieve_segment(const std::vector<int32_t> &  generated_ids,
                                              const transcribe::Tokenizer & tok,
                                              int64_t                       time_offset_ms,
                                              int                           seek_num_frames,
                                              bool                          want_segment_timestamps,
                                              int                           timestamp_begin,
                                              int64_t                       vocab_size) {
    WhisperSegmentResult out;
    out.segment_offset_frames = seek_num_frames;

    auto token_is_timestamp = [&](int id) {
        return id >= timestamp_begin && id < static_cast<int>(vocab_size);
    };
    auto trim_ascii = [](std::string s) {
        size_t start = 0;
        while (start < s.size() && (s[start] == ' ' || s[start] == '\t' || s[start] == '\n' || s[start] == '\r')) {
            ++start;
        }
        size_t end = s.size();
        while (end > start && (s[end - 1] == ' ' || s[end - 1] == '\t' || s[end - 1] == '\n' || s[end - 1] == '\r')) {
            --end;
        }
        return s.substr(start, end - start);
    };
    auto decode_range = [&](int begin, int end) -> std::string {
        if (begin >= end) {
            return std::string();
        }
        std::vector<int> ids_int;
        ids_int.reserve(static_cast<size_t>(end - begin));
        for (int i = begin; i < end; ++i) {
            const int32_t id = generated_ids[i];
            if (id >= 0 && id < 50257 && !token_is_timestamp(id)) {
                ids_int.push_back(id);
            }
        }
        if (ids_int.empty()) {
            return std::string();
        }
        return trim_ascii(tok.decode(ids_int.data(), static_cast<int>(ids_int.size())));
    };

    const int gn = static_cast<int>(generated_ids.size());

    std::vector<int> slices;
    for (int i = 0; i + 1 < gn; ++i) {
        if (token_is_timestamp(generated_ids[i]) && token_is_timestamp(generated_ids[i + 1])) {
            slices.push_back(i + 1);
        }
    }
    const bool single_timestamp_ending =
        gn >= 2 && !token_is_timestamp(generated_ids[gn - 2]) && token_is_timestamp(generated_ids[gn - 1]);

    if (!slices.empty()) {
        if (single_timestamp_ending) {
            slices.push_back(gn);
        } else {
            slices.back() += 1;
        }

        int last_slice = 0;
        for (size_t k = 0; k < slices.size(); ++k) {
            const int  current_slice = slices[k];
            const bool is_last       = (k + 1 == slices.size());
            if (current_slice <= last_slice) {
                last_slice = current_slice;
                continue;
            }
            const int32_t first_tok = generated_ids[last_slice];
            if (!token_is_timestamp(first_tok)) {
                last_slice = current_slice;
                continue;
            }
            const int end_token_idx = (!is_last || single_timestamp_ending) ? current_slice - 1 : current_slice - 2;
            if (end_token_idx < last_slice || end_token_idx >= gn) {
                last_slice = current_slice;
                continue;
            }
            const int32_t last_tok = generated_ids[end_token_idx];
            if (!token_is_timestamp(last_tok)) {
                last_slice = current_slice;
                continue;
            }
            const int64_t start_ts_pos = static_cast<int64_t>(first_tok - timestamp_begin);
            const int64_t end_ts_pos   = static_cast<int64_t>(last_tok - timestamp_begin);

            if (want_segment_timestamps) {
                transcribe_session::SegmentEntry seg{};
                seg.t0_ms = time_offset_ms + start_ts_pos * 20;
                seg.t1_ms = time_offset_ms + end_ts_pos * 20;
                if (seg.t1_ms < seg.t0_ms) {
                    seg.t1_ms = seg.t0_ms;
                }
                seg.text = decode_range(last_slice, current_slice);
                if (!seg.text.empty()) {
                    out.segments.push_back(std::move(seg));
                }
            }
            out.prev_chunk_segments.emplace_back(generated_ids.begin() + last_slice,
                                                 generated_ids.begin() + current_slice);
            last_slice = current_slice;
        }

        if (single_timestamp_ending) {
            out.segment_offset_frames = seek_num_frames;
        } else if (last_slice >= 2 && last_slice - 2 < gn && token_is_timestamp(generated_ids[last_slice - 2])) {
            const int last_ts_pos     = generated_ids[last_slice - 2] - timestamp_begin;
            out.segment_offset_frames = last_ts_pos * 2;  // input_stride = 2
        }
    } else {
        int64_t last_ts_pos = static_cast<int64_t>(seek_num_frames) / 2;
        for (int i = gn - 1; i >= 0; --i) {
            if (token_is_timestamp(generated_ids[i]) && generated_ids[i] != timestamp_begin) {
                last_ts_pos = static_cast<int64_t>(generated_ids[i] - timestamp_begin);
                break;
            }
        }
        if (want_segment_timestamps && gn > 0) {
            transcribe_session::SegmentEntry seg{};
            seg.t0_ms = time_offset_ms;
            seg.t1_ms = time_offset_ms + last_ts_pos * 20;
            seg.text  = decode_range(0, gn);
            if (!seg.text.empty()) {
                out.segments.push_back(std::move(seg));
            }
        }
        if (gn > 0) {
            out.prev_chunk_segments.emplace_back(generated_ids.begin(), generated_ids.end());
        }
        out.segment_offset_frames = seek_num_frames;
    }
    return out;
}

}  // namespace

transcribe_status whisper_run(transcribe_session *          session,
                              const float *                 pcm,
                              int                           n_samples,
                              const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * cc = static_cast<WhisperSession *>(session);
    auto * cm = static_cast<WhisperModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    cc->perf.reset();

    // ----- Mel frontend -----
    //
    //   1. TRANSCRIBE_MEL_FROM_REF=<dir> — validation hook (compiled in only
    //      with -DTRANSCRIBE_ENABLE_VALIDATION_HOOKS) that reads the reference
    //      mel dump from disk so encoder drift can be isolated from C++ mel
    //      drift. The default path exercises the production frontend.
    //
    //   2. C++ MelFrontend (default):
    //      - Short-form (n_samples <= fe_n_samples): pad PCM to fe_n_samples
    //        (480000) -> exactly fe_nb_max_frames (3000) mel frames.
    //      - Long-form (> fe_n_samples): compute mel on raw audio (HF does the
    //        same — only short-form pads to 30s). The seek loop slices
    //        3000-frame windows and zero-pads the trailing short window.
    const int  n_mels                 = cm->hparams.enc_num_mel_bins;
    const int  n_mel_frames_per_chunk = cm->hparams.fe_nb_max_frames > 0 ? cm->hparams.fe_nb_max_frames : 3000;
    const int  n_samples_per_chunk    = cm->hparams.fe_n_samples > 0 ? cm->hparams.fe_n_samples : 480000;
    // Short-form = fits a single 30s window; bypasses the dynamic-stride
    // machinery. HF's generate() takes a different code path for is_shortform,
    // so this is a numeric + behavioral parity gate (not just an optimization).
    const bool is_short_form          = (n_samples <= n_samples_per_chunk);

    const int64_t t_mel_start      = ggml_time_us();
    int           total_mel_frames = 0;
    bool          mel_from_ref     = false;
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    if (const char * ref_dir = transcribe::env::str("TRANSCRIBE_MEL_FROM_REF")) {
        if (const transcribe_status st = load_mel_from_ref(ref_dir, n_mels, n_mel_frames_per_chunk, cc->mel_buf);
            st != TRANSCRIBE_OK) {
            return st;
        }
        total_mel_frames = n_mel_frames_per_chunk;
        mel_from_ref     = true;
    }
#endif
    if (!mel_from_ref) {
        if (!cm->mel.has_value()) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "whisper run: model has no MelFrontend "
                    "(load skipped?)");
            return TRANSCRIBE_ERR_GGUF;
        }

        std::vector<float> pcm_work;
        const float *      pcm_in   = pcm;
        size_t             pcm_in_n = static_cast<size_t>(n_samples);
        if (is_short_form) {
            pcm_work.assign(static_cast<size_t>(n_samples_per_chunk), 0.0f);
            const int n_copy = std::min(n_samples, n_samples_per_chunk);
            std::memcpy(pcm_work.data(), pcm, static_cast<size_t>(n_copy) * sizeof(float));
            pcm_in   = pcm_work.data();
            pcm_in_n = pcm_work.size();
        }

        int                mel_n_mels = 0, mel_n_frames = 0;
        std::vector<float> mel_mn;  // [n_mels, n_frames] (MelFrontend layout)
        if (const transcribe_status mst = cm->mel->compute(pcm_in, pcm_in_n, mel_mn, mel_n_mels, mel_n_frames);
            mst != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: MelFrontend::compute failed (%d)", static_cast<int>(mst));
            return mst;
        }
        if (mel_n_mels != n_mels) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: mel n_mels %d != expected %d", mel_n_mels, n_mels);
            return TRANSCRIBE_ERR_GGUF;
        }
        if (is_short_form && mel_n_frames != n_mel_frames_per_chunk) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "whisper run: short-form mel has %d frames, "
                    "expected %d",
                    mel_n_frames, n_mel_frames_per_chunk);
            return TRANSCRIBE_ERR_GGUF;
        }

        // Transpose [n_mels, n_frames] → [n_frames, n_mels] so that the
        // slice for chunk k is a contiguous row span
        //   mel_buf[k*3000*n_mels : (k+1)*3000*n_mels).
        cc->mel_buf.resize(static_cast<size_t>(mel_n_mels) * static_cast<size_t>(mel_n_frames));
        for (int t = 0; t < mel_n_frames; ++t) {
            for (int m = 0; m < mel_n_mels; ++m) {
                cc->mel_buf[static_cast<size_t>(t) * mel_n_mels + m] =
                    mel_mn[static_cast<size_t>(m) * mel_n_frames + t];
            }
        }
        total_mel_frames = mel_n_frames;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    if (total_mel_frames <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- Run-scoped constants -----
    const int64_t n_ctx_decoder    = cm->hparams.dec_max_target_positions;
    const int64_t vocab_size       = cm->hparams.dec_vocab_size;
    const int     eos_id           = cm->tok.eos_id() >= 0 ? cm->tok.eos_id() : 50257;
    const int     timestamp_begin  = cm->hparams.no_timestamps_token_id + 1;
    constexpr int k_max_new_tokens = 256;

    const transcribe_timestamp_kind requested_timestamps =
        params != nullptr ? params->timestamps : TRANSCRIBE_TIMESTAMPS_NONE;
    if (requested_timestamps == TRANSCRIBE_TIMESTAMPS_WORD) {
        return TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS;
    }
    const bool want_segment_timestamps =
        requested_timestamps == TRANSCRIBE_TIMESTAMPS_AUTO || requested_timestamps == TRANSCRIBE_TIMESTAMPS_SEGMENT;

    // Multilingual variants emit <|lang|> + <|task|> in the decoder prefix;
    // .en variants have just <|sot|> and no translate/transcribe/language
    // tokens. supports_language_detect is the canonical signal (.en = false).
    const bool is_multilingual = cm->caps.supports_language_detect;

    int32_t task_token = -1;
    if (is_multilingual) {
        task_token = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE) ? cm->hparams.translate_token_id :
                                                                                        cm->hparams.transcribe_token_id;
        if (task_token < 0) {
            return TRANSCRIBE_ERR_GGUF;
        }
    } else if (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper run: this model does not support translate "
                "(non-multilingual variant)");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Language hint wins; otherwise we run a SOT-only prefill inside the
    // first chunk's loop iteration (once we have that chunk's enc_host).
    // For non-multilingual (.en) models, the only accepted language is
    // "en" and there is no language token to resolve — the prefix omits
    // the <|lang|> slot entirely.
    int32_t lang_token = -1;
    if (params != nullptr && params->language != nullptr && params->language[0] != '\0') {
        const std::string lang_code = params->language;
        if (!is_multilingual) {
            if (lang_code != "en") {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "whisper run: language '%s' is not supported by "
                        "this non-multilingual model (only 'en')",
                        lang_code.c_str());
                return TRANSCRIBE_ERR_INVALID_ARG;
            }
            // Accepted; lang_token stays -1 by design.
        } else {
            for (size_t i = 0; i < cm->lang_codes.size(); ++i) {
                if (cm->lang_codes[i] == lang_code) {
                    lang_token = cm->lang_token_ids[i];
                    break;
                }
            }
            if (lang_token < 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "whisper run: language '%s' not in model's language "
                        "table — cannot build decoder prompt",
                        lang_code.c_str());
                return TRANSCRIBE_ERR_INVALID_ARG;
            }
        }
    }

    auto new_compute_ctx = [&](size_t mem) -> bool {
        return ensure_compute_ctx(cc, mem);
    };
    auto suppress_in_place = [&](std::vector<float> & logits) {
        for (int32_t id : cm->hparams.suppress_tokens) {
            if (id >= 0 && id < vocab_size) {
                logits[static_cast<size_t>(id)] = -INFINITY;
            }
        }
    };
    auto token_is_timestamp = [&](int id) {
        return id >= timestamp_begin && id < static_cast<int>(vocab_size);
    };

    // ----- Chunk loop -----
    cc->clear_result();
    cc->chunk_traces.clear();
    std::vector<int32_t> all_text_ids;
    all_text_ids.reserve(256);

    std::vector<float> last_logits(static_cast<size_t>(vocab_size));
    int64_t            t_decode_start   = 0;
    bool               t_decode_started = false;

    // Finalize the context's result state. Called at normal completion and at
    // every mid-run abort exit (the public contract requires partial
    // segments/text before TRANSCRIBE_ERR_ABORTED to stay readable via the
    // normal accessors). Decodes all_text_ids to UTF-8, strips whitespace,
    // pushes the text-only segment for TIMESTAMPS_NONE mode, flips has_result.
    auto commit_result = [&]() {
        cc->t_decode_us = t_decode_started ? ggml_time_us() - t_decode_start : 0;

        std::string text;
        if (!all_text_ids.empty()) {
            std::vector<int> ids_int(all_text_ids.begin(), all_text_ids.end());
            text         = cm->tok.decode(ids_int.data(), static_cast<int>(ids_int.size()));
            size_t start = 0;
            while (start < text.size() &&
                   (text[start] == ' ' || text[start] == '\t' || text[start] == '\n' || text[start] == '\r')) {
                ++start;
            }
            size_t end = text.size();
            while (end > start &&
                   (text[end - 1] == ' ' || text[end - 1] == '\t' || text[end - 1] == '\n' || text[end - 1] == '\r')) {
                --end;
            }
            text = text.substr(start, end - start);
        }

        if (!want_segment_timestamps) {
            transcribe_session::SegmentEntry seg{};
            seg.text = text;
            cc->segments.push_back(std::move(seg));
        }
        cc->full_text   = std::move(text);
        cc->result_kind = want_segment_timestamps ? TRANSCRIBE_TIMESTAMPS_SEGMENT : TRANSCRIBE_TIMESTAMPS_NONE;
        cc->has_result  = true;

        if (whisper_perf_enabled()) {
            print_whisper_perf(cc->perf);
        }
    };

    // Run-scoped Whisper run-ext pointer + RNG.
    //
    // The whisper knobs live on a kind-tagged family extension reached via
    // transcribe_run_params::family; NULL selects the shipping defaults from
    // transcribe_whisper_run_ext_init(). `default_wp` must outlive the chunk
    // loop because `wp` aliases it. transcribe_ext_check repeats the
    // dispatcher's validation as defense in depth before casting. Pointer
    // fields (initial_prompt, prompt_tokens) are copied into context state
    // here, so the caller may free them right after the call.
    //
    // RNG is run-scoped, not chunk-scoped: HF threads a single generator
    // through the whole seek loop, so reseeding per chunk would replay the
    // same prefix each window and destroy long-form determinism. seed == 0
    // draws OS entropy once, then the rng advances across chunks and tiers.
    if (const transcribe_status st =
            transcribe_ext_check(params != nullptr ? params->family : nullptr, TRANSCRIBE_EXT_KIND_WHISPER_RUN,
                                 sizeof(struct transcribe_whisper_run_ext));
        st != TRANSCRIBE_OK) {
        return st;
    }
    transcribe_whisper_run_ext default_wp;
    transcribe_whisper_run_ext_init(&default_wp);
    const transcribe_whisper_run_ext * wp = (params != nullptr && params->family != nullptr) ?
                                                reinterpret_cast<const transcribe_whisper_run_ext *>(params->family) :
                                                &default_wp;
    std::mt19937                       rng(wp->seed != 0 ? wp->seed : std::random_device{}());

    // Prompt + condition_on_prev_tokens setup. HF raises ValueError when
    // prompt_condition_type=="all-segments" without condition_on_prev_tokens;
    // mirror as INVALID_ARG before any compute runs.
    if (wp->prompt_condition == TRANSCRIBE_WHISPER_PROMPT_ALL_SEGMENTS && !wp->condition_on_prev_tokens) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper run: prompt_condition=ALL_SEGMENTS requires "
                "condition_on_prev_tokens=true (HF parity)");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Resolve <|startofprev|> id. The converter writes
    // stt.whisper.prev_sot_token_id into the GGUF; tokenizer fallback
    // covers older artifacts that pre-date that key.
    int32_t prev_sot_id = cm->hparams.prev_sot_token_id;
    if (prev_sot_id < 0) {
        prev_sot_id = cm->tok.find("<|startofprev|>");
    }
    const bool prompt_requested = (wp->prompt_tokens != nullptr && wp->n_prompt_tokens > 0) ||
                                  (wp->initial_prompt != nullptr && wp->initial_prompt[0] != '\0');
    if (prev_sot_id < 0 && (prompt_requested || wp->condition_on_prev_tokens)) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "whisper run: model has no <|startofprev|> token; "
                "initial_prompt / condition_on_prev_tokens unavailable");
        return TRANSCRIBE_ERR_GGUF;
    }

    // Resolve initial prompt -> text-only token ids (library prepends
    // <|startofprev|>). Two paths: prompt_tokens (caller-owned, verbatim,
    // text-side only); or initial_prompt string, tokenized as HF's
    // get_prompt_ids form ("<|startofprev|> " + strip) with any special token
    // (id >= eos_id) in the text rejected (tokenization_whisper.py).
    const int max_prev_cap =
        wp->max_prev_context_tokens > 0 ? wp->max_prev_context_tokens : (cm->hparams.dec_max_target_positions / 2 - 1);
    std::vector<int32_t> prompt_text_ids;
    if (wp->prompt_tokens != nullptr && wp->n_prompt_tokens > 0) {
        // The library prepends <|startofprev|>; a leading prev_sot id from the
        // caller would double-prepend, so reject it as INVALID_ARG.
        if (prev_sot_id >= 0 && wp->prompt_tokens[0] == prev_sot_id) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "whisper run: prompt_tokens must not include "
                    "<|startofprev|> (id %d) at index 0; the library "
                    "prepends it. See transcribe_whisper_run_ext docs.",
                    prev_sot_id);
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        prompt_text_ids.assign(wp->prompt_tokens, wp->prompt_tokens + wp->n_prompt_tokens);
    } else if (wp->initial_prompt != nullptr && wp->initial_prompt[0] != '\0') {
        std::string s(wp->initial_prompt);
        auto        issp = [](char c) {
            return c == ' ' || c == '\t' || c == '\n' || c == '\r';
        };
        size_t a = 0, b = s.size();
        while (a < b && issp(s[a])) {
            ++a;
        }
        while (b > a && issp(s[b - 1])) {
            --b;
        }
        if (b > a) {
            std::string text(" ");
            text.append(s.data() + a, b - a);

            // Pre-check for special-token literals (`<|...|>`) before
            // BPE-encoding. HF's get_prompt_ids relies on the
            // tokenizer's added-token recognition to surface specials
            // as single ids and rejects any with id >=
            // all_special_ids[0] (== eos_id). Our gpt-2 BPE encoder
            // doesn't recognize specials, so without this scan a
            // literal "<|en|>" in user text would silently BPE-encode
            // byte-by-byte and slip through. Mirror HF's intent by
            // checking each "<|...|>" substring against the vocab
            // directly.
            for (size_t i = 0; i + 1 < text.size();) {
                if (text[i] == '<' && text[i + 1] == '|') {
                    const size_t end = text.find("|>", i + 2);
                    if (end != std::string::npos) {
                        const size_t close = end + 2;
                        std::string  piece = text.substr(i, close - i);
                        const int    id    = cm->tok.find(piece);
                        if (id >= eos_id) {
                            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "whisper run: initial_prompt contains "
                                    "disallowed special token \"%s\" (id %d)",
                                    piece.c_str(), id);
                            return TRANSCRIBE_ERR_INVALID_ARG;
                        }
                        i = close;
                        continue;
                    }
                }
                ++i;
            }

            if (cm->tok.encode(text, prompt_text_ids) != TRANSCRIBE_OK) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                        "whisper run: tokenizer.encode failed on "
                        "initial_prompt");
                return TRANSCRIBE_ERR_GGUF;
            }
            for (int32_t id : prompt_text_ids) {
                if (id >= eos_id) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "whisper run: initial_prompt contains "
                            "disallowed special token id %d",
                            id);
                    return TRANSCRIBE_ERR_INVALID_ARG;
                }
            }
        }
    }
    // Cap prompt tokens to max_prev_cap (left-truncate, keep most-recent).
    if (static_cast<int>(prompt_text_ids.size()) > max_prev_cap) {
        prompt_text_ids.erase(prompt_text_ids.begin(), prompt_text_ids.end() - max_prev_cap);
    }

    // History stored as segment token slices (not one flat vector) because
    // skip_ending_double_timestamps applies per-segment. FIRST_SEGMENT puts the
    // prompt at the head; ALL_SEGMENTS starts empty and re-prepends per chunk.
    std::vector<std::vector<int32_t>> prev_history_segments;
    if (wp->prompt_condition == TRANSCRIBE_WHISPER_PROMPT_FIRST_SEGMENT && !prompt_text_ids.empty()) {
        prev_history_segments.push_back(prompt_text_ids);
    }

    // Per-chunk; HF auto-disables when the previous chunk's accepted
    // temperature was hot (>= 0.5). Init to the user knob; flipped at chunk end.
    bool do_condition_on_prev_tokens = wp->condition_on_prev_tokens;

    int seek = 0;
    while (seek < total_mel_frames) {
        if (cc->poll_abort()) {
            commit_result();
            return TRANSCRIBE_ERR_ABORTED;
        }
        cc->perf.chunks += 1;

        const bool    is_first_chunk = (seek == 0);
        const int64_t time_offset_ms = static_cast<int64_t>(seek) * 10;

        // Slice + zero-pad the mel window to n_mel_frames_per_chunk (pad after
        // the real frames, matching HF's F.pad). seek_num_frames is the real
        // (unpadded) frame count and the max `seek` can advance per iteration.
        std::vector<float> chunk_mel(static_cast<size_t>(n_mels) * static_cast<size_t>(n_mel_frames_per_chunk), 0.0f);
        const int          seek_num_frames = std::min(total_mel_frames - seek, n_mel_frames_per_chunk);
        const int          frames_avail    = seek_num_frames;
        std::memcpy(chunk_mel.data(), &cc->mel_buf[static_cast<size_t>(seek) * static_cast<size_t>(n_mels)],
                    static_cast<size_t>(frames_avail) * static_cast<size_t>(n_mels) * sizeof(float));

        // Encoder (per chunk). enc_host is rewritten per call.
        int           T_enc_local = 0;
        const int64_t t_enc_start = ggml_time_us();
        if (const transcribe_status st =
                run_whisper_encoder_on_window(cc, cm, chunk_mel.data(), n_mels, n_mel_frames_per_chunk,
                                              /*allow_dumps=*/is_first_chunk, T_enc_local);
            st != TRANSCRIBE_OK) {
            return st;
        }
        if (is_first_chunk) {
            cc->t_encode_us  = ggml_time_us() - t_enc_start;
            t_decode_start   = ggml_time_us();
            t_decode_started = true;
        }

        // Language detection: once, first chunk, only if no hint and the model
        // has language tokens (.en has none, so it's skipped automatically).
        if (is_first_chunk && is_multilingual && lang_token < 0 && !cm->lang_token_ids.empty()) {
            // Lazily materialize encoder output to host for the prefill
            // graph's encoder_out_in input (one-shot per run; the hot path
            // reads cc->enc_out.tensor directly).
            const int d_enc_lang = cc->enc_out.d_model;
            const int T_enc_lang = cc->enc_out.T_enc;
            cc->enc_host.resize(static_cast<size_t>(d_enc_lang) * static_cast<size_t>(T_enc_lang));
            ggml_backend_tensor_get(cc->enc_out.tensor, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

            if (!new_compute_ctx(16 * 1024 * 1024)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            DecoderBuild det_db = build_decoder_prefill_graph(cc->compute_ctx, cm->weights, cm->hparams,
                                                              /*seq_len=*/1, T_enc_local, cc->decoder_use_flash);
            if (det_db.out == nullptr || det_db.graph == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, det_db.graph)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const int32_t sot = cm->hparams.decoder_start_token_id;
            ggml_backend_tensor_set(det_db.token_ids_in, &sot, 0, sizeof(int32_t));
            ggml_backend_tensor_set(det_db.encoder_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
            if (det_db.causal_mask_in != nullptr) {
                const float zero = 0.0f;
                ggml_backend_tensor_set(det_db.causal_mask_in, &zero, 0, sizeof(float));
            }
            if (ggml_backend_sched_graph_compute(cc->sched, det_db.graph) != GGML_STATUS_SUCCESS) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const size_t row_bytes = static_cast<size_t>(vocab_size) * sizeof(float);
            ggml_backend_tensor_get(det_db.dumps.logits_raw, last_logits.data(), 0, row_bytes);

            float best       = -INFINITY;
            int   best_index = -1;
            for (size_t i = 0; i < cm->lang_token_ids.size(); ++i) {
                const int32_t id = cm->lang_token_ids[i];
                if (id >= 0 && id < static_cast<int>(vocab_size)) {
                    const float v = last_logits[static_cast<size_t>(id)];
                    if (v > best) {
                        best       = v;
                        lang_token = id;
                        best_index = static_cast<int>(i);
                    }
                }
            }
            // Publish the detected ISO code only here, not in the user-hint
            // branch: the field means "what the model picked", not the hint.
            if (best_index >= 0 && static_cast<size_t>(best_index) < cm->lang_codes.size()) {
                cc->detected_language = cm->lang_codes[static_cast<size_t>(best_index)];
            }
        }
        if (is_multilingual && lang_token < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "whisper run: could not resolve a decoder language "
                    "token");
            return TRANSCRIBE_ERR_GGUF;
        }

        // Per-chunk prefix assembly. Mirrors HF
        // _prepare_decoder_input_ids: decoder input is prev_tokens + init.
        //   condition_on_prev + history: bos + history[-cut_off:], where bos is
        //     [<|startofprev|>(, prompt for ALL_SEGMENTS)]; drop a trailing
        //     double-timestamp per skip_ending_double_timestamps.
        //   else if initial prompt: [<|startofprev|>, prompt_text_ids...]
        //     (ALL_SEGMENTS every chunk; FIRST_SEGMENT first chunk only).
        //   else: empty.
        // We diverge from HF for FIRST_SEGMENT (the default): prime only the
        // first window, matching whisper.cpp / OpenAI.
        std::vector<int32_t> prev_tokens;
        if (do_condition_on_prev_tokens && !prev_history_segments.empty() && prev_sot_id >= 0) {
            prev_tokens.push_back(prev_sot_id);
            if (wp->prompt_condition == TRANSCRIBE_WHISPER_PROMPT_ALL_SEGMENTS && !prompt_text_ids.empty()) {
                prev_tokens.insert(prev_tokens.end(), prompt_text_ids.begin(), prompt_text_ids.end());
            }
            std::vector<int32_t> hist;
            for (const auto & seg : prev_history_segments) {
                size_t n = seg.size();
                if (n > 2 && token_is_timestamp(seg[n - 2])) {
                    // skip_ending_double_timestamps: drop the last token of any
                    // segment whose penultimate token is a timestamp.
                    --n;
                }
                hist.insert(hist.end(), seg.begin(), seg.begin() + n);
            }
            const int cap = std::min<int>(static_cast<int>(hist.size()), max_prev_cap);
            prev_tokens.insert(prev_tokens.end(), hist.end() - cap, hist.end());
        } else if (!prompt_text_ids.empty() && prev_sot_id >= 0 &&
                   (is_first_chunk || wp->prompt_condition == TRANSCRIBE_WHISPER_PROMPT_ALL_SEGMENTS)) {
            // FIRST_SEGMENT primes the initial prompt on the first window
            prev_tokens.push_back(prev_sot_id);
            prev_tokens.insert(prev_tokens.end(), prompt_text_ids.begin(), prompt_text_ids.end());
        }

        // Prefix for this chunk:
        //   multilingual: prev_tokens + [SOT, lang, task, notimestamps?]
        //   .en:          prev_tokens + [SOT,             notimestamps?]
        // .en vocab has no <|lang|>/<|task|> tokens; emitting them would land
        // on a garbage id.
        std::vector<int32_t> prompt_ids;
        prompt_ids.reserve(prev_tokens.size() + 4);
        prompt_ids.insert(prompt_ids.end(), prev_tokens.begin(), prev_tokens.end());
        prompt_ids.push_back(cm->hparams.decoder_start_token_id);
        if (is_multilingual) {
            prompt_ids.push_back(lang_token);
            prompt_ids.push_back(task_token);
        }
        if (!want_segment_timestamps) {
            prompt_ids.push_back(cm->hparams.no_timestamps_token_id);
        }
        const int seq_len = static_cast<int>(prompt_ids.size());

        // Position of the SOT token within the prefix. Used to read the
        // no-speech logits row from the prompt-pass (HF's
        // begin_index - start_of_trans_offset == len(prev_tokens)).
        const int sot_index = static_cast<int>(prev_tokens.size());

        // Guard: prefix + 1 generated token must fit the decoder self-attention
        // window (only a pathological prompt_tokens > 444 trips this).
        if (seq_len + 1 > static_cast<int>(n_ctx_decoder)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                    "whisper run: prefix length %d exceeds decoder "
                    "context %lld",
                    seq_len, static_cast<long long>(n_ctx_decoder));
            return TRANSCRIBE_ERR_INVALID_ARG;
        }

        // Per-chunk generated state.
        std::vector<int32_t> generated_ids;
        std::vector<int32_t> generated_text_ids;
        generated_ids.reserve(128);
        generated_text_ids.reserve(64);

        auto consume_generated_token = [&](int id) {
            generated_ids.push_back(static_cast<int32_t>(id));
            if (!token_is_timestamp(id) && id >= 0 && id < 50257) {
                generated_text_ids.push_back(static_cast<int32_t>(id));
            }
        };

        // max_initial_timestamp_index: HF WhisperTimeStampLogitsProcessor masks
        // timestamps above timestamp_begin + this index on the first generated
        // token only. Default 1.0s / 20ms-per-token = index 50, so the opening
        // cut lands at most 1.0s in. Negative/non-finite disables the cap.
        const int max_initial_timestamp_index =
            (std::isfinite(wp->max_initial_timestamp) && wp->max_initial_timestamp >= 0.0f) ?
                static_cast<int>(std::floor(static_cast<double>(wp->max_initial_timestamp) / 0.02)) :
                -1;

        // Mirrors transformers' WhisperTimeStampLogitsProcessor
        // (generation/logits_process.py). Five rules, in order:
        //   1. Always mask <|notimestamps|>.
        //   2. Pairing: timestamps come in pairs. After <text TS> the
        //      next token must close the pair with a TS (or EOS). After
        //      <TS TS> the pair is complete, the next token must be text.
        //   3. Monotonicity: timestamps never decrease.
        //   4. First-generated token must be a timestamp.
        //   5. If log-sum-exp over all timestamps > max single text
        //      logit, force a timestamp.
        //   6. On the first generated token only, cap the timestamp at
        //      timestamp_begin + max_initial_timestamp_index (HF
        //      logits_process.py:2040-2042).
        auto apply_timestamp_rules = [&](std::vector<float> & logits) {
            apply_whisper_timestamp_rules(logits, generated_ids, want_segment_timestamps,
                                          cm->hparams.no_timestamps_token_id, timestamp_begin, vocab_size, eos_id,
                                          max_initial_timestamp_index);
        };

        int next_id = 0;

        // Temperature fallback setup. Per-chunk tuple [t0, t0+dt, ...] up to
        // 1.0; default [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]. A tier is accepted when
        // compression_ratio < thold AND avg_logprob > thold; if all fail, keep
        // the last tier's output. Thresholds use INF sentinels to mean disabled.
        std::vector<float> temperatures;
        temperatures.push_back(wp->temperature);
        if (wp->temperature_inc > 0.0f) {
            for (float T = wp->temperature + wp->temperature_inc; T <= 1.0f + 1e-4f; T += wp->temperature_inc) {
                temperatures.push_back(T);
            }
        }

        // Accepted-tier output (commits every tier so the last-fallback
        // is returned when no tier passes thresholds).
        std::vector<int32_t> accepted_generated_ids;
        std::vector<int32_t> accepted_generated_text_ids;
        float                accepted_T           = 0.0f;
        float                accepted_compression = 0.0f;
        float                accepted_avg_logprob = 0.0f;
        int                  accepted_n_fallbacks = 0;

        // No-speech: probability captured from the SOT-position row of the
        // prompt-pass logits (tier 0 only), matching HF WhisperNoSpeechDetection
        // which reads logits[:, begin_index - start_of_trans_offset]. The token
        // id is positional: no_speech = notimestamps - 1 (multilingual 50362,
        // .en 50361). When the no-speech skip fires, the chunk's output is
        // discarded and seek advances a full window (no fallback attempted).
        const int no_speech_token_id         = cm->hparams.no_timestamps_token_id - 1;
        float     no_speech_prob             = 0.0f;
        bool      no_speech_prob_captured    = false;
        bool      no_speech_fired_this_chunk = false;

        // HF _retrieve_compression_ratio convention: the ratio is computed over
        // the full generated tail including timestamps, specials, and EOS when
        // present (fallback decides before EOS is stripped). We assemble the
        // metric input as generated_ids + EOS-if-sampled; no EOS marker when
        // the loop hit k_max_new_tokens first.
        bool tier_hit_eos = false;

        // KV cache allocation is tier-independent — done once per chunk
        // out here, then self-attention state (n, head) is reset per
        // tier below.
        const int  n_layers  = cm->hparams.dec_n_layers;
        const bool need_init = cc->kv_cache.ctx == nullptr || cc->kv_cache.n_batch != 1 ||
                               cc->kv_cache.n_ctx != n_ctx_decoder || cc->kv_cache.T_enc != T_enc_local;
        if (need_init) {
            cc->kv_cache.free();
            // AUTO: match cache dtype to the weight dtype (F32 GGUF -> F32
            // zero-loss cache; F16 and quants -> F16). Explicit --kv-type wins.
            ggml_type kv_type_g;
            if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
                kv_type_g = GGML_TYPE_F32;
            } else if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
                kv_type_g = GGML_TYPE_F16;
            } else {
                const ggml_tensor * probe =
                    !cm->weights.dec_blocks.empty() ? cm->weights.dec_blocks[0].self_q_w : nullptr;
                kv_type_g = (probe != nullptr && probe->type == GGML_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
            }
            if (!kv_cache_init(cc->kv_cache, cm->plan.primary, static_cast<int>(n_ctx_decoder), T_enc_local,
                               cm->hparams.dec_d_model, n_layers, kv_type_g)) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: KV cache init failed");
                return TRANSCRIBE_ERR_BACKEND;
            }
        }

        // ---- Cross-attention K/V precompute (chunk-scoped) ----
        // Cross-K/V depends only on the encoder output and cross-attn weights
        // (both tier-invariant), so compute once per chunk and reuse across
        // fallback tiers.
        {
            const int64_t t_cross_build_start = ggml_time_us();
            if (!new_compute_ctx(8 * 1024 * 1024)) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: ggml_init for cross_kv failed");
                return TRANSCRIBE_ERR_GGUF;
            }
            DecoderBuild cross_db = build_cross_kv_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                                         cc->enc_out.tensor, T_enc_local);
            if (cross_db.graph == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            cc->perf.cross_build.add(ggml_time_us() - t_cross_build_start);

            const int64_t t_cross_alloc_start = ggml_time_us();
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, cross_db.graph)) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: alloc_graph failed (cross_kv)");
                return TRANSCRIBE_ERR_GGUF;
            }
            // No tensor_set: cross-KV reads cc->enc_out.tensor via
            // a view inside build_cross_kv_graph, populated by the
            // encoder graph's final ggml_cpy.
            cc->perf.cross_alloc.add(ggml_time_us() - t_cross_alloc_start);

            const int64_t t_cross_compute_start = ggml_time_us();
            if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, cross_db.graph);
                gs != GGML_STATUS_SUCCESS) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: cross_kv compute failed (%d)", static_cast<int>(gs));
                return TRANSCRIBE_ERR_GGUF;
            }
            cc->perf.cross_compute.add(ggml_time_us() - t_cross_compute_start);
            cc->kv_cache.cross_populated = true;
        }

        // ----- Tier loop (temperature fallback) -----
        for (size_t ti = 0; ti < temperatures.size(); ++ti) {
            const float tier_T = temperatures[ti];

            // Per-tier reset. Self-cache resets every tier (each tier
            // generates its own token sequence); cross-cache stays
            // populated — its contents are tier-invariant.
            cc->kv_cache.n    = 0;
            cc->kv_cache.head = 0;
            generated_ids.clear();
            generated_text_ids.clear();
            tier_hit_eos             = false;
            double sum_logprob       = 0.0;
            int    n_logprob_samples = 0;

            // Dump gate: references were captured at T=0, so only tier 0 of the
            // first chunk emits intermediates.
            const bool emit_tier_dumps = is_first_chunk && (ti == 0);
            auto       tier_try_dump   = [&](const char * name, ggml_tensor * t, const char * stage) {
                if (t != nullptr && emit_tier_dumps) {
                    transcribe::debug::dump_tensor(name, t, stage);
                }
            };

            // ---- Prompt pass (emit dumps, n_past=0) -------------------
            {
                const int64_t t_prompt_build_start = ggml_time_us();
                if (!new_compute_ctx(16 * 1024 * 1024)) {
                    return TRANSCRIBE_ERR_GGUF;
                }
                const int    kv_pad = kv_pad_self_attn(cm->plan.primary_kind, cc->decoder_use_flash);
                DecoderBuild db     = build_decoder_graph_kv(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                                             /*n_tokens=*/seq_len, /*n_past=*/0, T_enc_local,
                                                             /*kv_pad=*/kv_pad,
                                                             /*skip_log_softmax=*/false, cc->decoder_use_flash);
                if (db.out == nullptr || db.graph == nullptr) {
                    return TRANSCRIBE_ERR_GGUF;
                }
                cc->perf.prompt_build.add(ggml_time_us() - t_prompt_build_start);

                const int64_t t_prompt_alloc_start = ggml_time_us();
                ggml_backend_sched_reset(cc->sched);
                if (!ggml_backend_sched_alloc_graph(cc->sched, db.graph)) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: alloc_graph failed (prompt)");
                    return TRANSCRIBE_ERR_GGUF;
                }

                ggml_backend_tensor_set(db.token_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
                std::vector<int32_t> pos_ids(seq_len);
                for (int i = 0; i < seq_len; ++i) {
                    pos_ids[i] = i;
                }
                ggml_tensor * pos_in = ggml_graph_get_tensor(db.graph, "dec.pos_ids");
                ggml_backend_tensor_set(pos_in, pos_ids.data(), 0, pos_ids.size() * sizeof(int32_t));

                if (db.causal_mask_in != nullptr) {
                    // Mask shape: [n_kv, n_tokens] = [n_kv, seq_len].
                    // db.causal_mask_in carries the runtime n_kv (may
                    // be padded beyond seq_len for FA alignment).
                    const int          n_kv_mask = static_cast<int>(db.causal_mask_in->ne[0]);
                    std::vector<float> mask(static_cast<size_t>(n_kv_mask) * seq_len);
                    for (int q = 0; q < seq_len; ++q) {
                        for (int k = 0; k < n_kv_mask; ++k) {
                            // Causal in [0, seq_len), -inf for padded
                            // slots in [seq_len, n_kv_mask).
                            mask[static_cast<size_t>(q) * n_kv_mask + k] = (k < seq_len && k <= q) ? 0.0f : -1e9f;
                        }
                    }
                    ggml_backend_tensor_set(db.causal_mask_in, mask.data(), 0, mask.size() * sizeof(float));
                }

                if (db.cross_mask_in != nullptr) {
                    // Cross mask shape [T_enc_pad, n_tokens]. Zero
                    // for k in [0, T_enc), -inf for trailing padded
                    // slots — same for every query row.
                    const int          n_kv_cross = static_cast<int>(db.cross_mask_in->ne[0]);
                    std::vector<float> mask(static_cast<size_t>(n_kv_cross) * seq_len);
                    for (int q = 0; q < seq_len; ++q) {
                        for (int k = 0; k < n_kv_cross; ++k) {
                            mask[static_cast<size_t>(q) * n_kv_cross + k] = (k < T_enc_local) ? 0.0f : -1e9f;
                        }
                    }
                    ggml_backend_tensor_set(db.cross_mask_in, mask.data(), 0, mask.size() * sizeof(float));
                }
                cc->perf.prompt_alloc.add(ggml_time_us() - t_prompt_alloc_start);

                const int64_t t_prompt_compute_start = ggml_time_us();
                if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, db.graph);
                    gs != GGML_STATUS_SUCCESS) {
                    return TRANSCRIBE_ERR_GGUF;
                }
                cc->perf.prompt_compute.add(ggml_time_us() - t_prompt_compute_start);
                cc->kv_cache.n    = seq_len;
                cc->kv_cache.head = seq_len;

                tier_try_dump("dec.token_emb", db.dumps.token_emb, "decoder.embedding");
                tier_try_dump("dec.pos_emb", db.dumps.pos_emb, "decoder.position_embedding");
                tier_try_dump("dec.embed_sum", db.dumps.embed_sum, "decoder.embed_sum");
                for (size_t i = 0; i < db.dumps.block_outs.size(); ++i) {
                    char bname[64], stage[64];
                    std::snprintf(bname, sizeof(bname), "dec.block.%zu.out", i);
                    std::snprintf(stage, sizeof(stage), "decoder.block%zu.out", i);
                    tier_try_dump(bname, db.dumps.block_outs[i], stage);
                }
                tier_try_dump("dec.out_before_head", db.dumps.out_before_head, "decoder.output_before_head");
                tier_try_dump("dec.logits_raw", db.dumps.logits_raw, "decoder.logits_raw");
                tier_try_dump("dec.logits", db.dumps.logits, "decoder.logits");

                const size_t row_bytes = static_cast<size_t>(vocab_size) * sizeof(float);

                // Capture no_speech_prob from the RAW SOT-position row, matching
                // HF WhisperNoSpeechDetection: logits[:, begin_index -
                // start_of_trans_offset] collapses to logits[:, sot_index]
                // (sot_index = position of <|startoftranscript|> in the prefix =
                // len(prev_tokens), 0 with no prev tokens). Tier 0 only. Read
                // into a temp buffer so the suppress / timestamp rules applied
                // to last_logits below don't contaminate the measurement.
                if (ti == 0 && !no_speech_prob_captured && no_speech_token_id >= 0 &&
                    no_speech_token_id < static_cast<int>(vocab_size)) {
                    std::vector<float> sot_logits(static_cast<size_t>(vocab_size));
                    const int64_t      t_prompt_tget_sot = ggml_time_us();
                    ggml_backend_tensor_get(db.dumps.logits_raw, sot_logits.data(),
                                            row_bytes * static_cast<size_t>(sot_index), row_bytes);
                    cc->perf.prompt_tensor_get.add(ggml_time_us() - t_prompt_tget_sot);
                    float max_l = -std::numeric_limits<float>::infinity();
                    for (auto l : sot_logits) {
                        if (std::isfinite(l) && l > max_l) {
                            max_l = l;
                        }
                    }
                    double sum = 0.0;
                    double ns  = 0.0;
                    if (std::isfinite(max_l)) {
                        for (size_t i = 0; i < sot_logits.size(); ++i) {
                            const float l = sot_logits[i];
                            if (std::isfinite(l)) {
                                const double e = std::exp(static_cast<double>(l - max_l));
                                sum += e;
                                if (static_cast<int>(i) == no_speech_token_id) {
                                    ns = e;
                                }
                            }
                        }
                    }
                    no_speech_prob          = (sum > 0.0) ? static_cast<float>(ns / sum) : 0.0f;
                    no_speech_prob_captured = true;
                }

                // Last-row logits → first generated token.
                const int64_t t_prompt_tget_last = ggml_time_us();
                ggml_backend_tensor_get(db.dumps.logits_raw, last_logits.data(),
                                        row_bytes * static_cast<size_t>(seq_len - 1), row_bytes);
                cc->perf.prompt_tensor_get.add(ggml_time_us() - t_prompt_tget_last);

                const int64_t t_prompt_cpu_start      = ggml_time_us();
                const int64_t t_prompt_suppress_start = ggml_time_us();
                suppress_in_place(last_logits);
                for (int32_t id : cm->hparams.begin_suppress_tokens) {
                    if (id >= 0 && id < vocab_size) {
                        last_logits[static_cast<size_t>(id)] = -INFINITY;
                    }
                }
                cc->perf.prompt_cpu_suppress.add(ggml_time_us() - t_prompt_suppress_start);

                const int64_t t_prompt_ts_start = ggml_time_us();
                apply_timestamp_rules(last_logits);
                cc->perf.prompt_cpu_timestamp.add(ggml_time_us() - t_prompt_ts_start);

                if (tier_T <= 0.0f) {
                    // T==0: fused argmax + log-softmax (logprob computed free).
                    const int64_t t_prompt_sample_start = ggml_time_us();
                    float         lp                    = 0.0f;
                    next_id                             = sample_argmax_and_logprob(last_logits, &lp);
                    sum_logprob += lp;
                    cc->perf.prompt_cpu_sample.add(ggml_time_us() - t_prompt_sample_start);
                } else {
                    const int64_t t_prompt_sample_start = ggml_time_us();
                    next_id = sample_from_logits(last_logits, tier_T, rng, &cc->sample_scratch);
                    cc->perf.prompt_cpu_sample.add(ggml_time_us() - t_prompt_sample_start);

                    const int64_t t_prompt_lp_start = ggml_time_us();
                    sum_logprob += logprob_of_token_hf(last_logits, next_id, tier_T);
                    cc->perf.prompt_cpu_logprob.add(ggml_time_us() - t_prompt_lp_start);
                }
                n_logprob_samples += 1;
                cc->perf.prompt_cpu.add(ggml_time_us() - t_prompt_cpu_start);
            }

            // ---- Step loop (n_tokens=1) ----
            // Two variants:
            //   GPU: build_step_graph — one static-topology graph per tier (KV
            //     writes via ggml_set_rows at runtime kv_idx, FA reads a fixed
            //     max_n_kv window with a runtime mask).
            //   CPU/debug: per-step build_decoder_graph_kv with dynamic n_kv.
            int        n_past         = seq_len;
            const bool primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                        cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                        cm->plan.primary_kind != transcribe::BackendKind::Unknown;
            const bool use_step_graph = primary_is_gpu && !transcribe::debug::enabled();

            // Sized to fit prompt + max generated tail, padded to next pow2,
            // capped at n_ctx_decoder (448).
            int max_n_kv = 256;
            while (max_n_kv < seq_len + k_max_new_tokens) {
                max_n_kv *= 2;
            }
            if (max_n_kv > static_cast<int>(n_ctx_decoder)) {
                max_n_kv = static_cast<int>(n_ctx_decoder);
            }

            // Step graph + persistent host buffers (use_step_graph path only).
            StepBuild                sb{};
            std::vector<ggml_fp16_t> step_mask;
            std::vector<float>       step_cross_mask;
            const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
            const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);

            if (use_step_graph) {
                if (!new_compute_ctx(8 * 1024 * 1024)) {
                    return TRANSCRIBE_ERR_GGUF;
                }
                sb = build_step_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, max_n_kv, T_enc_local,
                                      cc->decoder_use_flash);
                if (sb.graph == nullptr || sb.logits_out == nullptr) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: build_step_graph failed");
                    return TRANSCRIBE_ERR_GGUF;
                }
                ggml_backend_sched_reset(cc->sched);
                if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run: sched_alloc_graph failed (step)");
                    return TRANSCRIBE_ERR_GGUF;
                }

                // Self-attn mask: [0, seq_len) populated by prompt pass
                // are attendable; [seq_len, max_n_kv) start as -inf.
                step_mask.assign(max_n_kv, mask_neg_inf);
                for (int p = 0; p < seq_len; ++p) {
                    step_mask[p] = mask_zero;
                }

                // Cross mask is invariant across steps within a chunk —
                // upload once and reuse.
                if (sb.cross_mask_in != nullptr) {
                    const int n_kv_cross = static_cast<int>(sb.cross_mask_in->ne[0]);
                    step_cross_mask.assign(static_cast<size_t>(n_kv_cross), 0.0f);
                    for (int k = T_enc_local; k < n_kv_cross; ++k) {
                        step_cross_mask[static_cast<size_t>(k)] = -1e9f;
                    }
                    ggml_backend_tensor_set(sb.cross_mask_in, step_cross_mask.data(), 0,
                                            step_cross_mask.size() * sizeof(float));
                }
            }

            for (int step = 0; step < k_max_new_tokens; ++step) {
                if (next_id == eos_id) {
                    tier_hit_eos = true;
                    break;
                }
                if (cc->poll_abort()) {
                    commit_result();
                    return TRANSCRIBE_ERR_ABORTED;
                }
                consume_generated_token(next_id);

                if (n_past + 1 > static_cast<int>(n_ctx_decoder)) {
                    break;
                }
                if (use_step_graph && n_past + 1 > max_n_kv) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_INFO, "whisper run: hit max_n_kv=%d at n_past=%d", max_n_kv, n_past);
                    break;
                }

                const size_t row_bytes = static_cast<size_t>(vocab_size) * sizeof(float);

                if (use_step_graph) {
                    const int64_t t_step_alloc_start = ggml_time_us();
                    int32_t       tok                = next_id;
                    int32_t       pos_val            = n_past;
                    int64_t       kv_val             = n_past;
                    ggml_backend_tensor_set(sb.token_id_in, &tok, 0, sizeof(int32_t));
                    ggml_backend_tensor_set(sb.pos_id_in, &pos_val, 0, sizeof(int32_t));
                    ggml_backend_tensor_set(sb.kv_idx_in, &kv_val, 0, sizeof(int64_t));

                    step_mask[n_past] = mask_zero;
                    ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0,
                                            static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));
                    cc->perf.step_alloc.add(ggml_time_us() - t_step_alloc_start);

                    const int64_t t_step_compute_start = ggml_time_us();
                    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph);
                        gs != GGML_STATUS_SUCCESS) {
                        return TRANSCRIBE_ERR_GGUF;
                    }
                    cc->perf.step_compute.add(ggml_time_us() - t_step_compute_start);

                    n_past += 1;
                    cc->kv_cache.n    = n_past;
                    cc->kv_cache.head = n_past;

                    const int64_t t_step_tget_start = ggml_time_us();
                    ggml_backend_tensor_get(sb.logits_out, last_logits.data(), 0, row_bytes);
                    cc->perf.step_tensor_get.add(ggml_time_us() - t_step_tget_start);
                } else {
                    const int64_t t_step_build_start = ggml_time_us();
                    if (!new_compute_ctx(4 * 1024 * 1024)) {
                        return TRANSCRIBE_ERR_GGUF;
                    }
                    const int    kv_pad = kv_pad_self_attn(cm->plan.primary_kind, cc->decoder_use_flash);
                    DecoderBuild step_db =
                        build_decoder_graph_kv(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                               /*n_tokens=*/1, /*n_past=*/n_past, T_enc_local,
                                               /*kv_pad=*/kv_pad,
                                               /*skip_log_softmax=*/true, cc->decoder_use_flash);
                    if (step_db.out == nullptr) {
                        return TRANSCRIBE_ERR_GGUF;
                    }
                    cc->perf.step_build.add(ggml_time_us() - t_step_build_start);

                    const int64_t t_step_alloc_start = ggml_time_us();
                    ggml_backend_sched_reset(cc->sched);
                    if (!ggml_backend_sched_alloc_graph(cc->sched, step_db.graph)) {
                        return TRANSCRIBE_ERR_GGUF;
                    }

                    int32_t tok = next_id;
                    int32_t pos = n_past;
                    ggml_backend_tensor_set(step_db.token_ids_in, &tok, 0, sizeof(int32_t));
                    ggml_tensor * pos_in = ggml_graph_get_tensor(step_db.graph, "dec.pos_ids");
                    ggml_backend_tensor_set(pos_in, &pos, 0, sizeof(int32_t));

                    // Padded step: [0, n_past+1) is 0, trailing slots -inf so
                    // FA ignores stale K/V from previous tiers/chunks.
                    if (step_db.causal_mask_in != nullptr) {
                        const int          n_kv_mask = static_cast<int>(step_db.causal_mask_in->ne[0]);
                        std::vector<float> mask(static_cast<size_t>(n_kv_mask), 0.0f);
                        const int          n_valid = n_past + 1;
                        for (int k = n_valid; k < n_kv_mask; ++k) {
                            mask[static_cast<size_t>(k)] = -1e9f;
                        }
                        ggml_backend_tensor_set(step_db.causal_mask_in, mask.data(), 0, mask.size() * sizeof(float));
                    }

                    if (step_db.cross_mask_in != nullptr) {
                        const int          n_kv_cross = static_cast<int>(step_db.cross_mask_in->ne[0]);
                        std::vector<float> mask(static_cast<size_t>(n_kv_cross), 0.0f);
                        for (int k = T_enc_local; k < n_kv_cross; ++k) {
                            mask[static_cast<size_t>(k)] = -1e9f;
                        }
                        ggml_backend_tensor_set(step_db.cross_mask_in, mask.data(), 0, mask.size() * sizeof(float));
                    }
                    cc->perf.step_alloc.add(ggml_time_us() - t_step_alloc_start);

                    const int64_t t_step_compute_start = ggml_time_us();
                    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, step_db.graph);
                        gs != GGML_STATUS_SUCCESS) {
                        return TRANSCRIBE_ERR_GGUF;
                    }
                    cc->perf.step_compute.add(ggml_time_us() - t_step_compute_start);

                    n_past += 1;
                    cc->kv_cache.n    = n_past;
                    cc->kv_cache.head = n_past;

                    // Mid-generation dump: step 19's logits exercise the
                    // n_past>0 KV read/write path the prompt-pass dump
                    // cannot reach. Captured on tier 0 of the first chunk
                    // only (tolerance references were generated at T=0).
                    if (step == 19) {
                        tier_try_dump("dec.logits_raw.gen20", step_db.out, "decoder.logits_raw.gen20");
                    }

                    const int64_t t_step_tget_start = ggml_time_us();
                    ggml_backend_tensor_get(step_db.out, last_logits.data(), 0, row_bytes);
                    cc->perf.step_tensor_get.add(ggml_time_us() - t_step_tget_start);
                }

                const int64_t t_step_cpu_start      = ggml_time_us();
                const int64_t t_step_suppress_start = ggml_time_us();
                suppress_in_place(last_logits);
                cc->perf.step_cpu_suppress.add(ggml_time_us() - t_step_suppress_start);

                const int64_t t_step_ts_start = ggml_time_us();
                apply_timestamp_rules(last_logits);
                cc->perf.step_cpu_timestamp.add(ggml_time_us() - t_step_ts_start);

                if (tier_T <= 0.0f) {
                    const int64_t t_step_sample_start = ggml_time_us();
                    float         lp                  = 0.0f;
                    next_id                           = sample_argmax_and_logprob(last_logits, &lp);
                    sum_logprob += lp;
                    cc->perf.step_cpu_sample.add(ggml_time_us() - t_step_sample_start);
                } else {
                    const int64_t t_step_sample_start = ggml_time_us();
                    next_id = sample_from_logits(last_logits, tier_T, rng, &cc->sample_scratch);
                    cc->perf.step_cpu_sample.add(ggml_time_us() - t_step_sample_start);

                    const int64_t t_step_lp_start = ggml_time_us();
                    sum_logprob += logprob_of_token_hf(last_logits, next_id, tier_T);
                    cc->perf.step_cpu_logprob.add(ggml_time_us() - t_step_lp_start);
                }
                n_logprob_samples += 1;
                cc->perf.step_cpu.add(ggml_time_us() - t_step_cpu_start);
            }

            // ---- Compute tier metrics ----
            // Compression ratio runs over the full generated tail (all ids
            // including timestamps/specials/EOS, prompt header stripped),
            // matching HF's _retrieve_compression_ratio. Using filtered
            // text-only ids here would diverge enough to flip the fallback
            // accept/escalate boundary.
            std::vector<int32_t> comp_tail;
            comp_tail.reserve(generated_ids.size() + 1);
            comp_tail.insert(comp_tail.end(), generated_ids.begin(), generated_ids.end());
            if (tier_hit_eos) {
                comp_tail.push_back(static_cast<int32_t>(eos_id));
            }
            const float tier_comp_ratio  = compute_compression_ratio_hf(comp_tail, vocab_size);
            const float tier_avg_logprob = n_logprob_samples > 0 ? static_cast<float>(sum_logprob / n_logprob_samples) :
                                                                   -std::numeric_limits<float>::infinity();

            // Thresholds. HF _need_fallback falls back strictly on
            // `comp_ratio > thold` or `avg_logprob < thold`; equality does NOT
            // trigger, so a tier is accepted under `<= / >=`. INF sentinels work
            // (<= +INF and >= -INF are always true: disabled thresholds pass).
            const bool comp_ok = tier_comp_ratio <= wp->compression_ratio_thold;
            const bool lp_ok   = tier_avg_logprob >= wp->logprob_thold;

            // HF _need_fallback sets should_skip (and halts fallback) when BOTH
            // no_speech_prob > no_speech_thold AND avg_logprob < logprob_thold.
            // no_speech_prob is the tier-0 capture (checked for every tier);
            // logprob is per-tier. Sentinels: no_speech_thold=+INF or
            // logprob_thold=-INF disables the skip.
            const bool no_speech_should_skip =
                no_speech_prob > wp->no_speech_thold && tier_avg_logprob < wp->logprob_thold;

            // Commit the tier's output unconditionally so the last-tried tier
            // wins when no tier passes (matches HF generate_with_fallback). A
            // no_speech_should_skip is discarded below after recording metrics.
            accepted_generated_ids      = generated_ids;
            accepted_generated_text_ids = generated_text_ids;
            accepted_T                  = tier_T;
            accepted_compression        = tier_comp_ratio;
            accepted_avg_logprob        = tier_avg_logprob;
            accepted_n_fallbacks        = static_cast<int>(ti);

            if (no_speech_should_skip) {
                no_speech_fired_this_chunk = true;
                break;
            }
            if (comp_ok && lp_ok) {
                break;
            }
        }

        // Hand off the accepted tier to segment emission. A no-speech-fired
        // chunk discards its output but still advances seek a full window.
        generated_ids      = accepted_generated_ids;
        generated_text_ids = accepted_generated_text_ids;
        if (no_speech_fired_this_chunk) {
            generated_ids.clear();
            generated_text_ids.clear();
        }

        // Commit per-chunk trace over [time_offset_ms, +seek_num_frames*10ms):
        // what the fallback loop decided and what the no-speech gate saw.
        {
            transcribe_whisper_chunk_trace trace;
            transcribe_whisper_chunk_trace_init(&trace);
            trace.t0_ms               = time_offset_ms;
            trace.t1_ms               = time_offset_ms + static_cast<int64_t>(seek_num_frames) * 10;
            trace.temperature_used    = accepted_T;
            trace.compression_ratio   = accepted_compression;
            trace.avg_logprob         = accepted_avg_logprob;
            trace.no_speech_prob      = no_speech_prob;
            trace.no_speech_triggered = no_speech_fired_this_chunk;
            trace.n_fallbacks         = accepted_n_fallbacks;
            cc->chunk_traces.push_back(trace);
        }

        // condition_on_prev_tokens gate for the NEXT chunk: HF disables it when
        // the accepted temperature was hot (>= 0.5, presumed hallucinated) to
        // avoid propagating into the next chunk. History is updated after
        // _retrieve_segment (HF carries only the surviving segment tokens).
        do_condition_on_prev_tokens = wp->condition_on_prev_tokens && (accepted_T < 0.5f);

        // _retrieve_segment (port of HF). Splits generated_ids on consecutive-
        // timestamp pairs (one SegmentEntry per pair) and returns
        // segment_offset_frames (seek advance):
        //   - single trailing ts (pair not closed): advance the full chunk,
        //     preserving the tail for the next decoder.
        //   - >=1 closed pair, not single-ended: discard the unfinished tail,
        //     advance by the last closed ts position * input_stride(2) frames.
        //   - no pairs: emit one full-chunk segment, advance seek_num_frames.
        // TIMESTAMPS_NONE includes <|notimestamps|>, so no ts tokens -> "no
        // pairs" branch, full-chunk advance, no per-chunk segment emission.
        WhisperSegmentResult seg_res = whisper_retrieve_segment(generated_ids, cm->tok, time_offset_ms, seek_num_frames,
                                                                want_segment_timestamps, timestamp_begin, vocab_size);
        for (auto & seg : seg_res.segments) {
            cc->segments.push_back(std::move(seg));
        }
        int                                 segment_offset_frames = seg_res.segment_offset_frames;
        std::vector<std::vector<int32_t>> & prev_chunk_segments   = seg_res.prev_chunk_segments;

        // Update prev-context history from the _retrieve_segment slices, not
        // accepted_generated_ids directly: the closed-pair branch discards the
        // unfinished tail, and carrying it would leak into the next prompt.
        if (!no_speech_fired_this_chunk && !prev_chunk_segments.empty()) {
            prev_history_segments.insert(prev_history_segments.end(), prev_chunk_segments.begin(),
                                         prev_chunk_segments.end());
        }

        all_text_ids.insert(all_text_ids.end(), generated_text_ids.begin(), generated_text_ids.end());

        // Seek advance.
        //   Short-form: full-chunk advance. PCM is padded to exactly
        //     n_samples_per_chunk, so the silent tail would hallucinate
        //     "you you you" if re-entered; single-pass it (matches HF's
        //     separate is_shortform path).
        //   Long-form: dynamic advance via segment_offset_frames (last closed
        //     timestamp pair), guarded against 0-advance infinite loops.
        //   No-speech fired: declared empty, skip the whole window.
        if (is_short_form) {
            seek += seek_num_frames;
        } else if (no_speech_fired_this_chunk) {
            seek += seek_num_frames;
        } else {
            if (segment_offset_frames <= 0) {
                segment_offset_frames = seek_num_frames;
            }
            seek += segment_offset_frames;
        }
    }

    commit_result();

    return TRANSCRIBE_OK;
}

// Offline batched decode (transcribe_run_batch).
//
// Encoder stays SERIAL per utterance (compute-bound) while the bandwidth-bound
// autoregressive DECODE is batched across B utterances in lockstep. The batched
// fast path covers only the common, parity-exact case and peels everything else
// to the serial whisper_run():
//   batched: short-form (<= 30s) at tier 0 (T=0 greedy), TIMESTAMPS_NONE, no
//            initial_prompt.
//   serial : long-form, segment-timestamps, T > 0, prompt/prompt_tokens,
//            invalid inputs, and any utterance whose tier-0 output fails the
//            fallback thresholds (so the temperature ladder runs exactly).
//
// A clip that passes tier 0 (the common case) is bit-for-bit the serial tier-0
// result, so batch_parity holds; anything that would diverge runs serially.

// Serial fallback: run each utterance through whisper_run and snapshot it.
transcribe_status whisper_run_batch_serial(WhisperSession *              cc,
                                           const float * const *         pcm,
                                           const int *                   n_samples,
                                           int                           n,
                                           const transcribe_run_params * params) {
    for (int i = 0; i < n; ++i) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        const transcribe_status st = (pcm[i] == nullptr || n_samples[i] <= 0) ?
                                         TRANSCRIBE_ERR_INVALID_ARG :
                                         whisper_run(cc, pcm[i], n_samples[i], params);
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

transcribe_status whisper_run_batch(transcribe_session *          session,
                                    const float * const *         pcm,
                                    const int *                   n_samples,
                                    int                           n,
                                    const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<WhisperSession *>(session);
    auto * cm = static_cast<WhisperModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Resolve the whisper run-ext (NULL → shipping defaults).
    if (const transcribe_status st =
            transcribe_ext_check(params != nullptr ? params->family : nullptr, TRANSCRIBE_EXT_KIND_WHISPER_RUN,
                                 sizeof(struct transcribe_whisper_run_ext));
        st != TRANSCRIBE_OK) {
        return st;
    }
    transcribe_whisper_run_ext default_wp;
    transcribe_whisper_run_ext_init(&default_wp);
    const transcribe_whisper_run_ext * wp = (params != nullptr && params->family != nullptr) ?
                                                reinterpret_cast<const transcribe_whisper_run_ext *>(params->family) :
                                                &default_wp;

    // ---- Global gates: only what the batched graph genuinely can't do. ----
    // Segment timestamps, temperature>0 fallback, and initial_prompt are now
    // all handled in-batch; what still peels the WHOLE call to serial is the
    // device/topology constraints + word timestamps (unsupported) + the
    // ALL_SEGMENTS-without-condition error (let serial surface it).
    const bool                      primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                                     cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                                     cm->plan.primary_kind != transcribe::BackendKind::Unknown;
    const transcribe_timestamp_kind req_ts = params != nullptr ? params->timestamps : TRANSCRIBE_TIMESTAMPS_NONE;
    const bool want_ts = req_ts == TRANSCRIBE_TIMESTAMPS_AUTO || req_ts == TRANSCRIBE_TIMESTAMPS_SEGMENT;
    if (n == 1 || !cc->decoder_use_flash || !primary_is_gpu || transcribe::debug::enabled() ||
        req_ts == TRANSCRIBE_TIMESTAMPS_WORD ||
        (wp->prompt_condition == TRANSCRIBE_WHISPER_PROMPT_ALL_SEGMENTS && !wp->condition_on_prev_tokens)) {
        return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto &  hp                     = cm->hparams;
    const int     d_model                = hp.dec_d_model;
    const int     n_layer                = hp.dec_n_layers;
    const int64_t vocab_size             = hp.dec_vocab_size;
    const int     eos_id                 = cm->tok.eos_id() >= 0 ? cm->tok.eos_id() : 50257;
    const int     timestamp_begin        = hp.no_timestamps_token_id + 1;
    const int     no_speech_token_id     = hp.no_timestamps_token_id - 1;
    const int     n_ctx_decoder          = hp.dec_max_target_positions;
    const int     n_mels                 = hp.enc_num_mel_bins;
    const int     n_mel_frames_per_chunk = hp.fe_nb_max_frames > 0 ? hp.fe_nb_max_frames : 3000;
    const int     n_samples_per_chunk    = hp.fe_n_samples > 0 ? hp.fe_n_samples : 480000;
    const bool    is_multilingual        = cm->caps.supports_language_detect;
    constexpr int k_max_new              = 256;
    // Short-form: PCM padded to one 30s window, so the whole chunk is one
    // seek of n_mel_frames_per_chunk frames at time offset 0.
    const int     seek_num_frames        = n_mel_frames_per_chunk;

    // Task token (multilingual only). Non-resolvable → whole-batch serial.
    int32_t task_token = -1;
    if (is_multilingual) {
        task_token = (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE) ? hp.translate_token_id :
                                                                                        hp.transcribe_token_id;
        if (task_token < 0) {
            return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
        }
    } else if (params != nullptr && params->task == TRANSCRIBE_TASK_TRANSLATE) {
        return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
    }

    // Optional language hint (uniform). Unresolvable → serial.
    int32_t lang_hint = -1;
    if (params != nullptr && params->language != nullptr && params->language[0] != '\0') {
        const std::string lang_code = params->language;
        if (!is_multilingual) {
            if (lang_code != "en") {
                return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
            }
        } else {
            for (size_t i = 0; i < cm->lang_codes.size(); ++i) {
                if (cm->lang_codes[i] == lang_code) {
                    lang_hint = cm->lang_token_ids[i];
                    break;
                }
            }
            if (lang_hint < 0) {
                return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
            }
        }
    }

    // ---- Shared prev-context prefix from initial_prompt / prompt_tokens. ----
    // Uniform across the batch (one run_params). Happy-path only: any malformed
    // prompt routes the whole call to serial, which surfaces the exact error.
    int32_t prev_sot_id = hp.prev_sot_token_id;
    if (prev_sot_id < 0) {
        prev_sot_id = cm->tok.find("<|startofprev|>");
    }
    const bool           prompt_requested = (wp->prompt_tokens != nullptr && wp->n_prompt_tokens > 0) ||
                                            (wp->initial_prompt != nullptr && wp->initial_prompt[0] != '\0');
    std::vector<int32_t> prev_tokens;
    if (prompt_requested) {
        if (prev_sot_id < 0) {
            return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
        }
        const int max_prev_cap =
            wp->max_prev_context_tokens > 0 ? wp->max_prev_context_tokens : (hp.dec_max_target_positions / 2 - 1);
        std::vector<int32_t> ptext;
        if (wp->prompt_tokens != nullptr && wp->n_prompt_tokens > 0) {
            if (wp->prompt_tokens[0] == prev_sot_id) {
                return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
            }
            ptext.assign(wp->prompt_tokens, wp->prompt_tokens + wp->n_prompt_tokens);
        } else {
            std::string s(wp->initial_prompt);
            auto        issp = [](char c) {
                return c == ' ' || c == '\t' || c == '\n' || c == '\r';
            };
            size_t a = 0, b = s.size();
            while (a < b && issp(s[a])) {
                ++a;
            }
            while (b > a && issp(s[b - 1])) {
                --b;
            }
            if (b > a) {
                std::string text(" ");
                text.append(s.data() + a, b - a);
                for (size_t i = 0; i + 1 < text.size();) {
                    if (text[i] == '<' && text[i + 1] == '|') {
                        const size_t end = text.find("|>", i + 2);
                        if (end != std::string::npos) {
                            std::string piece = text.substr(i, end + 2 - i);
                            if (cm->tok.find(piece) >= eos_id) {
                                return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
                            }
                            i = end + 2;
                            continue;
                        }
                    }
                    ++i;
                }
                if (cm->tok.encode(text, ptext) != TRANSCRIBE_OK) {
                    return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
                }
                for (int32_t id : ptext) {
                    if (id >= eos_id) {
                        return whisper_run_batch_serial(cc, pcm, n_samples, n, params);
                    }
                }
            }
        }
        if (static_cast<int>(ptext.size()) > max_prev_cap) {
            ptext.erase(ptext.begin(), ptext.end() - max_prev_cap);
        }
        if (!ptext.empty()) {
            prev_tokens.push_back(prev_sot_id);
            prev_tokens.insert(prev_tokens.end(), ptext.begin(), ptext.end());
        }
    }
    const int sot_index = static_cast<int>(prev_tokens.size());

    // Temperature ladder (same recipe as serial). Tier 0 == wp->temperature.
    std::vector<float> temperatures;
    temperatures.push_back(wp->temperature);
    if (wp->temperature_inc > 0.0f) {
        for (float T = wp->temperature + wp->temperature_inc; T <= 1.0f + 1e-4f; T += wp->temperature_inc) {
            temperatures.push_back(T);
        }
    }
    const int max_initial_timestamp_index =
        (std::isfinite(wp->max_initial_timestamp) && wp->max_initial_timestamp >= 0.0f) ?
            static_cast<int>(std::floor(static_cast<double>(wp->max_initial_timestamp) / 0.02)) :
            -1;

    // Per-utterance result slots, filled in input order, pushed at the end.
    std::vector<transcribe_session::ResultSet> results(static_cast<size_t>(n));
    std::vector<char>                          have_result(static_cast<size_t>(n), 0);
    std::vector<char>                          needs_serial(static_cast<size_t>(n), 0);

    // ---- Pass 0: classify + parallel mel for batchable (short-form) inputs. ----
    std::vector<char>               valid(static_cast<size_t>(n), 0);
    std::vector<std::vector<float>> mel_chunks(static_cast<size_t>(n));  // [frames, n_mels]
    for (int b = 0; b < n; ++b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            results[b].status = TRANSCRIBE_ERR_INVALID_ARG;
            have_result[b]    = 1;
            continue;
        }
        if (n_samples[b] > n_samples_per_chunk) {
            needs_serial[b] = 1;
            continue;
        }
        valid[b] = 1;
    }
    int n_threads = cc->n_threads;
    if (n_threads <= 0) {
        n_threads = transcribe::default_n_threads();
    }
    int64_t       mel_us = 0, enc_us = 0, dec_us = 0;
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_threads, [&](int b) {
        if (!valid[b]) {
            return true;
        }
        std::vector<float> pcm_work(static_cast<size_t>(n_samples_per_chunk), 0.0f);
        const int          n_copy = std::min(n_samples[b], n_samples_per_chunk);
        std::memcpy(pcm_work.data(), pcm[b], static_cast<size_t>(n_copy) * sizeof(float));
        std::vector<float> mel_mn;
        int                nm = 0, nf = 0;
        if (cm->mel->compute(pcm_work.data(), pcm_work.size(), mel_mn, nm, nf,
                             /*n_threads=*/1) != TRANSCRIBE_OK ||
            nm != n_mels || nf <= 0) {
            valid[b] = 0;
            return true;
        }
        std::vector<float> & chunk = mel_chunks[b];
        chunk.assign(static_cast<size_t>(nm) * nf, 0.0f);
        for (int t = 0; t < nf; ++t) {
            for (int m = 0; m < nm; ++m) {
                chunk[static_cast<size_t>(t) * nm + m] = mel_mn[static_cast<size_t>(m) * nf + t];
            }
        }
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;
    for (int b = 0; b < n; ++b) {
        if (!needs_serial[b] && !have_result[b] && !valid[b]) {
            needs_serial[b] = 1;
        }
    }

    // ---- Pass 1: serial per-utterance encoder + (optional) lang detect + prompt. ----
    std::vector<std::vector<float>>   enc_hosts(static_cast<size_t>(n));
    std::vector<int>                  T_enc(static_cast<size_t>(n), 0);
    std::vector<std::vector<int32_t>> prompts(static_cast<size_t>(n));
    std::vector<std::string>          det_lang(static_cast<size_t>(n));
    int                               T_enc_max = 0;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        int           T_enc_local = 0;
        const int64_t t_e0        = ggml_time_us();
        if (run_whisper_encoder_on_window(cc, cm, mel_chunks[b].data(), n_mels, n_mel_frames_per_chunk,
                                          /*allow_dumps=*/false, T_enc_local) != TRANSCRIBE_OK ||
            T_enc_local <= 0) {
            valid[b]        = 0;
            needs_serial[b] = 1;
            continue;
        }
        enc_us += ggml_time_us() - t_e0;

        const int d_enc = cc->enc_out.d_model;
        enc_hosts[b].resize(static_cast<size_t>(d_enc) * T_enc_local);
        ggml_backend_tensor_get(cc->enc_out.tensor, enc_hosts[b].data(), 0, enc_hosts[b].size() * sizeof(float));
        T_enc[b]  = T_enc_local;
        T_enc_max = std::max(T_enc_max, T_enc_local);

        int32_t lang_token = lang_hint;
        if (is_multilingual && lang_token < 0 && !cm->lang_token_ids.empty()) {
            cc->enc_host.resize(enc_hosts[b].size());
            std::memcpy(cc->enc_host.data(), enc_hosts[b].data(), enc_hosts[b].size() * sizeof(float));
            if (!ensure_compute_ctx(cc, 16 * 1024 * 1024)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            DecoderBuild det = build_decoder_prefill_graph(cc->compute_ctx, cm->weights, hp, /*seq_len=*/1, T_enc_local,
                                                           cc->decoder_use_flash);
            if (det.out == nullptr || det.graph == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, det.graph)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const int32_t sot = hp.decoder_start_token_id;
            ggml_backend_tensor_set(det.token_ids_in, &sot, 0, sizeof(int32_t));
            ggml_backend_tensor_set(det.encoder_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
            if (det.causal_mask_in != nullptr) {
                const float zero = 0.0f;
                ggml_backend_tensor_set(det.causal_mask_in, &zero, 0, sizeof(float));
            }
            if (ggml_backend_sched_graph_compute(cc->sched, det.graph) != GGML_STATUS_SUCCESS) {
                return TRANSCRIBE_ERR_GGUF;
            }
            std::vector<float> ll(static_cast<size_t>(vocab_size));
            ggml_backend_tensor_get(det.dumps.logits_raw, ll.data(), 0,
                                    static_cast<size_t>(vocab_size) * sizeof(float));
            float best     = -INFINITY;
            int   best_idx = -1;
            for (size_t i = 0; i < cm->lang_token_ids.size(); ++i) {
                const int32_t id = cm->lang_token_ids[i];
                if (id >= 0 && id < static_cast<int>(vocab_size) && ll[static_cast<size_t>(id)] > best) {
                    best       = ll[static_cast<size_t>(id)];
                    lang_token = id;
                    best_idx   = static_cast<int>(i);
                }
            }
            if (best_idx >= 0 && static_cast<size_t>(best_idx) < cm->lang_codes.size()) {
                det_lang[b] = cm->lang_codes[static_cast<size_t>(best_idx)];
            }
        }
        if (is_multilingual && lang_token < 0) {
            valid[b]        = 0;
            needs_serial[b] = 1;
            continue;
        }

        // Per-utterance prompt: prev_tokens + SOT [lang task] [notimestamps?].
        std::vector<int32_t> & pr = prompts[b];
        pr                        = prev_tokens;
        pr.push_back(hp.decoder_start_token_id);
        if (is_multilingual) {
            pr.push_back(lang_token);
            pr.push_back(task_token);
        }
        if (!want_ts) {
            pr.push_back(hp.no_timestamps_token_id);
        }
    }

    int prompt_len = -1, n_batch_active = 0;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        if (prompt_len < 0) {
            prompt_len = static_cast<int>(prompts[b].size());
        }
        ++n_batch_active;
    }
    if (n_batch_active == 0 || T_enc_max <= 0) {
        for (int b = 0; b < n; ++b) {
            if (have_result[b]) {
                continue;
            }
            const transcribe_status st = (pcm[b] == nullptr || n_samples[b] <= 0) ?
                                             TRANSCRIBE_ERR_INVALID_ARG :
                                             whisper_run(cc, pcm[b], n_samples[b], params);
            results[b]        = (st == TRANSCRIBE_OK) ? cc->capture_result(st) : transcribe_session::ResultSet{};
            results[b].status = st;
            have_result[b]    = 1;
        }
        for (int b = 0; b < n; ++b) {
            cc->batch_results.push_back(std::move(results[b]));
        }
        return TRANSCRIBE_OK;
    }

    const int B = n;

    // ---- Allocate the batched KV cache. ----
    int max_n_kv = 256;
    while (max_n_kv < prompt_len + k_max_new) {
        max_n_kv *= 2;
    }
    if (max_n_kv > n_ctx_decoder) {
        max_n_kv = n_ctx_decoder;
    }

    ggml_type kv_type_g;
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        kv_type_g = GGML_TYPE_F32;
    } else if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        kv_type_g = GGML_TYPE_F16;
    } else {
        const ggml_tensor * probe = !cm->weights.dec_blocks.empty() ? cm->weights.dec_blocks[0].self_q_w : nullptr;
        kv_type_g                 = (probe != nullptr && probe->type == GGML_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    }
    if (cc->kv_cache.buffer != nullptr &&
        (cc->kv_cache.n_batch != B || cc->kv_cache.T_enc != T_enc_max || cc->kv_cache.n_ctx != max_n_kv)) {
        cc->kv_cache.free();
    }
    if (cc->kv_cache.buffer == nullptr) {
        if (!kv_cache_init_batched(cc->kv_cache, cm->plan.primary, max_n_kv, T_enc_max, d_model, n_layer, B,
                                   kv_type_g)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "whisper run_batch: kv_cache_init_batched failed");
            return TRANSCRIBE_ERR_BACKEND;
        }
    } else {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        cc->kv_cache.n               = 0;
        cc->kv_cache.head            = 0;
        cc->kv_cache.cross_populated = false;
    }

    auto new_compute_ctx = [&](size_t mem) -> bool {
        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        cc->compute_ctx_size = 0;
        ggml_init_params ip{};
        ip.mem_size     = mem;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx != nullptr) {
            cc->compute_ctx_size = mem;
        }
        return cc->compute_ctx != nullptr;
    };

    const int64_t t_dec0 = ggml_time_us();

    // ---- Batched cross-attention K/V (tier-invariant; computed once). ----
    {
        if (!new_compute_ctx(16 * 1024 * 1024)) {
            return TRANSCRIBE_ERR_GGUF;
        }
        DecoderBuild cross = build_cross_kv_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_enc_max, B);
        if (cross.graph == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, cross.graph)) {
            return TRANSCRIBE_ERR_GGUF;
        }
        std::vector<float> packed(static_cast<size_t>(d_model) * T_enc_max * B, 0.0f);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            std::memcpy(packed.data() + static_cast<size_t>(b) * T_enc_max * d_model, enc_hosts[b].data(),
                        static_cast<size_t>(d_model) * T_enc[b] * sizeof(float));
        }
        ggml_backend_tensor_set(cross.encoder_out_in, packed.data(), 0, packed.size() * sizeof(float));
        if (ggml_backend_sched_graph_compute(cc->sched, cross.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.cross_populated = true;
    }

    // ---- Batched step graph + growing self-attention window. ----
    const ggml_fp16_t f16_zero = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t f16_ninf = ggml_fp32_to_fp16(-INFINITY);

    std::vector<ggml_fp16_t> cmask(static_cast<size_t>(T_enc_max) * B, f16_ninf);
    for (int b = 0; b < n; ++b) {
        const int     real = valid[b] ? T_enc[b] : 1;
        ggml_fp16_t * base = cmask.data() + static_cast<size_t>(b) * T_enc_max;
        std::fill(base, base + std::min(real, T_enc_max), f16_zero);
    }

    int kv_window = 64;
    while (kv_window > max_n_kv) {
        kv_window /= 2;
    }
    if (kv_window < 1) {
        kv_window = max_n_kv;
    }

    StepBuildBatched sb{};
    auto             rebuild_step = [&](int win) -> bool {
        if (!new_compute_ctx(32 * 1024 * 1024)) {
            return false;
        }
        sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, win, T_enc_max, B,
                                      cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.logits_out == nullptr) {
            return false;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            return false;
        }
        ggml_backend_tensor_set(sb.cross_mask_in, cmask.data(), 0, cmask.size() * sizeof(ggml_fp16_t));
        return true;
    };
    if (!rebuild_step(kv_window)) {
        return TRANSCRIBE_ERR_GGUF;
    }

    std::vector<ggml_fp16_t> smask(static_cast<size_t>(kv_window) * B, f16_ninf);
    std::vector<int32_t>     tok_buf(B, 0), pos_buf(B, 0);
    std::vector<int64_t>     kvidx_buf(B, 0);
    std::vector<float>       logits_host(static_cast<size_t>(vocab_size) * B);

    auto run_step = [&](int posv) -> transcribe_status {
        for (int b = 0; b < n; ++b) {
            pos_buf[b]                                       = posv;
            kvidx_buf[b]                                     = posv;
            smask[static_cast<size_t>(b) * kv_window + posv] = f16_zero;
        }
        ggml_backend_tensor_set(sb.token_ids_in, tok_buf.data(), 0, B * sizeof(int32_t));
        ggml_backend_tensor_set(sb.pos_ids_in, pos_buf.data(), 0, B * sizeof(int32_t));
        ggml_backend_tensor_set(sb.kv_idx_in, kvidx_buf.data(), 0, B * sizeof(int64_t));
        ggml_backend_tensor_set(sb.self_mask_in, smask.data(), 0, smask.size() * sizeof(ggml_fp16_t));
        if (ggml_backend_sched_graph_compute(cc->sched, sb.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
        return TRANSCRIBE_OK;
    };
    auto read_logits = [&]() {
        ggml_backend_tensor_get(sb.logits_out, logits_host.data(), 0,
                                static_cast<size_t>(vocab_size) * B * sizeof(float));
    };
    auto ensure_window = [&](int posv) -> bool {
        if (posv + 1 <= kv_window) {
            return true;
        }
        int win = kv_window;
        while (win < posv + 1 && win < max_n_kv) {
            win *= 2;
        }
        if (win > max_n_kv) {
            win = max_n_kv;
        }
        if (win == kv_window) {
            return true;
        }
        std::vector<ggml_fp16_t> wider(static_cast<size_t>(win) * B, f16_ninf);
        for (int b = 0; b < n; ++b) {
            std::fill(wider.data() + static_cast<size_t>(b) * win, wider.data() + static_cast<size_t>(b) * win + posv,
                      f16_zero);
        }
        smask.swap(wider);
        kv_window = win;
        return rebuild_step(kv_window);
    };

    std::vector<float> lb(static_cast<size_t>(vocab_size));
    auto               suppress_row = [&](std::vector<float> & lg, bool begin) {
        for (int32_t id : hp.suppress_tokens) {
            if (id >= 0 && id < vocab_size) {
                lg[static_cast<size_t>(id)] = -INFINITY;
            }
        }
        if (begin) {
            for (int32_t id : hp.begin_suppress_tokens) {
                if (id >= 0 && id < vocab_size) {
                    lg[static_cast<size_t>(id)] = -INFINITY;
                }
            }
        }
    };
    auto token_is_timestamp = [&](int id) {
        return id >= timestamp_begin && id < static_cast<int>(vocab_size);
    };

    // Per-utterance working + accepted state.
    std::vector<std::vector<int32_t>> gen(static_cast<size_t>(n)), gen_text(static_cast<size_t>(n));
    std::vector<double>               sumlp(static_cast<size_t>(n), 0.0);
    std::vector<int>                  nlp(static_cast<size_t>(n), 0);
    std::vector<char>                 fin(static_cast<size_t>(n), 0), hit_eos(static_cast<size_t>(n), 0);
    std::vector<int32_t>              next_tok(static_cast<size_t>(n), 0);
    std::vector<float>                ns_prob(static_cast<size_t>(n), 0.0f);
    std::vector<std::vector<int32_t>> acc_gen(static_cast<size_t>(n)), acc_gen_text(static_cast<size_t>(n));
    std::vector<char>                 acc_hit_eos(static_cast<size_t>(n), 0);
    std::vector<char>                 accepted_done(static_cast<size_t>(n), 0);
    std::vector<std::mt19937>         rng(static_cast<size_t>(n));
    {
        const uint32_t base_seed = wp->seed != 0 ? wp->seed : 0x5733B17Eu;
        for (int b = 0; b < n; ++b) {
            rng[b].seed(base_seed + static_cast<uint32_t>(b));
        }
    }

    auto sample_one = [&](int b, float T, bool begin) -> int {
        const float * row = logits_host.data() + static_cast<size_t>(b) * vocab_size;
        std::memcpy(lb.data(), row, static_cast<size_t>(vocab_size) * sizeof(float));
        suppress_row(lb, begin);
        apply_whisper_timestamp_rules(lb, gen[b], want_ts, hp.no_timestamps_token_id, timestamp_begin, vocab_size,
                                      eos_id, max_initial_timestamp_index);
        float lp = 0.0f;
        int   id;
        if (T <= 0.0f) {
            id = sample_argmax_and_logprob(lb, &lp);
        } else {
            id = sample_from_logits(lb, T, rng[b], &cc->sample_scratch);
            lp = logprob_of_token_hf(lb, id, T);
        }
        sumlp[b] += lp;
        nlp[b] += 1;
        return id;
    };

    // Decode one temperature tier over all valid slots (lockstep). Fills the
    // working state; the caller reads metrics for the still-active utterances.
    auto decode_tier = [&](float T, size_t ti) -> transcribe_status {
        std::fill(smask.begin(), smask.end(), f16_ninf);
        for (int b = 0; b < n; ++b) {
            gen[b].clear();
            gen_text[b].clear();
            sumlp[b]    = 0.0;
            nlp[b]      = 0;
            hit_eos[b]  = 0;
            fin[b]      = !valid[b];
            next_tok[b] = 0;
        }
        int pos = 0;
        for (; pos < prompt_len; ++pos) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            if (!ensure_window(pos)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            for (int b = 0; b < n; ++b) {
                tok_buf[b] = valid[b] ? prompts[b][pos] : eos_id;
            }
            if (run_step(pos) != TRANSCRIBE_OK) {
                return TRANSCRIBE_ERR_GGUF;
            }
            if (ti == 0 && pos == sot_index && no_speech_token_id >= 0 &&
                no_speech_token_id < static_cast<int>(vocab_size)) {
                read_logits();
                for (int b = 0; b < n; ++b) {
                    if (!valid[b]) {
                        continue;
                    }
                    const float * row   = logits_host.data() + static_cast<size_t>(b) * vocab_size;
                    float         max_l = -INFINITY;
                    for (int i = 0; i < vocab_size; ++i) {
                        if (std::isfinite(row[i]) && row[i] > max_l) {
                            max_l = row[i];
                        }
                    }
                    double sum = 0.0, ns = 0.0;
                    if (std::isfinite(max_l)) {
                        for (int i = 0; i < vocab_size; ++i) {
                            if (std::isfinite(row[i])) {
                                const double e = std::exp(static_cast<double>(row[i] - max_l));
                                sum += e;
                                if (i == no_speech_token_id) {
                                    ns = e;
                                }
                            }
                        }
                    }
                    ns_prob[b] = (sum > 0.0) ? static_cast<float>(ns / sum) : 0.0f;
                }
            }
        }
        read_logits();
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            const int id = sample_one(b, T, /*begin=*/true);
            next_tok[b]  = id;
            if (id == eos_id) {
                fin[b]     = 1;
                hit_eos[b] = 1;
            } else {
                gen[b].push_back(id);
                if (!token_is_timestamp(id) && id >= 0 && id < 50257) {
                    gen_text[b].push_back(id);
                }
            }
        }
        for (int produced = 1; produced < k_max_new; ++produced, ++pos) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            bool all_done = true;
            for (int b = 0; b < n; ++b) {
                if (!fin[b]) {
                    all_done = false;
                    break;
                }
            }
            if (all_done || pos + 1 > max_n_kv) {
                break;
            }
            if (!ensure_window(pos)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            for (int b = 0; b < n; ++b) {
                tok_buf[b] = fin[b] ? eos_id : next_tok[b];
            }
            if (run_step(pos) != TRANSCRIBE_OK) {
                return TRANSCRIBE_ERR_GGUF;
            }
            read_logits();
            for (int b = 0; b < n; ++b) {
                if (fin[b]) {
                    continue;
                }
                const int id = sample_one(b, T, /*begin=*/false);
                next_tok[b]  = id;
                if (id == eos_id) {
                    fin[b]     = 1;
                    hit_eos[b] = 1;
                } else {
                    gen[b].push_back(id);
                    if (!token_is_timestamp(id) && id >= 0 && id < 50257) {
                        gen_text[b].push_back(id);
                    }
                }
            }
        }
        return TRANSCRIBE_OK;
    };

    // ---- Tier loop with batched fallback ladder. ----
    const int valid_count = std::max(1, n_batch_active);
    auto      trim_ws     = [](std::string s) {
        size_t a = 0, b = s.size();
        auto   sp = [](char c) {
            return c == ' ' || c == '\t' || c == '\n' || c == '\r';
        };
        while (a < b && sp(s[a])) {
            ++a;
        }
        while (b > a && sp(s[b - 1])) {
            --b;
        }
        return s.substr(a, b - a);
    };
    auto finalize = [&](int b, const std::vector<int32_t> & g, const std::vector<int32_t> & gt) {
        transcribe_session::ResultSet rs;
        std::vector<int>              tids(gt.begin(), gt.end());
        std::string                   text =
            tids.empty() ? std::string() : trim_ws(cm->tok.decode(tids.data(), static_cast<int>(tids.size())));
        if (want_ts) {
            WhisperSegmentResult sr =
                whisper_retrieve_segment(g, cm->tok, /*time_offset_ms=*/0, seek_num_frames,
                                         /*want_segment_timestamps=*/true, timestamp_begin, vocab_size);
            rs.segments    = std::move(sr.segments);
            rs.result_kind = TRANSCRIBE_TIMESTAMPS_SEGMENT;
        } else {
            transcribe_session::SegmentEntry seg{};
            seg.text = text;
            rs.segments.push_back(std::move(seg));
            rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        }
        rs.full_text         = text;
        rs.detected_language = det_lang[b];
        rs.has_result        = true;
        rs.status            = TRANSCRIBE_OK;
        rs.t_mel_us          = mel_us / valid_count;
        rs.t_encode_us       = enc_us / valid_count;
        rs.t_decode_us       = dec_us / valid_count;
        results[b]           = std::move(rs);
        have_result[b]       = 1;
        accepted_done[b]     = 1;
    };

    for (size_t ti = 0; ti < temperatures.size(); ++ti) {
        bool any_active = false;
        for (int b = 0; b < n; ++b) {
            if (valid[b] && !accepted_done[b]) {
                any_active = true;
                break;
            }
        }
        if (!any_active) {
            break;
        }

        const transcribe_status st = decode_tier(temperatures[ti], ti);
        if (st != TRANSCRIBE_OK) {
            return st;
        }

        for (int b = 0; b < n; ++b) {
            if (!valid[b] || accepted_done[b]) {
                continue;
            }
            std::vector<int32_t> comp_tail = gen[b];
            if (hit_eos[b]) {
                comp_tail.push_back(static_cast<int32_t>(eos_id));
            }
            const float comp_ratio = compute_compression_ratio_hf(comp_tail, vocab_size);
            const float avg_lp =
                nlp[b] > 0 ? static_cast<float>(sumlp[b] / nlp[b]) : -std::numeric_limits<float>::infinity();
            // Record this tier's output (last tier wins if none accepts).
            acc_gen[b]      = gen[b];
            acc_gen_text[b] = gen_text[b];
            acc_hit_eos[b]  = hit_eos[b];

            const bool comp_ok = comp_ratio <= wp->compression_ratio_thold;
            const bool lp_ok   = avg_lp >= wp->logprob_thold;
            const bool ns_skip = ns_prob[b] > wp->no_speech_thold && avg_lp < wp->logprob_thold;
            if (ns_skip) {
                finalize(b, std::vector<int32_t>{}, std::vector<int32_t>{});
            } else if (comp_ok && lp_ok) {
                finalize(b, acc_gen[b], acc_gen_text[b]);
            }
            // else: escalate to the next tier.
        }
    }
    // Tiers exhausted: keep the last-tier output for anyone still unaccepted.
    for (int b = 0; b < n; ++b) {
        if (!valid[b] || accepted_done[b]) {
            continue;
        }
        finalize(b, acc_gen[b], acc_gen_text[b]);
    }
    dec_us += ggml_time_us() - t_dec0;
    // Patch decode timings now that dec_us is known (finalize ran earlier).
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            results[b].t_decode_us = dec_us / valid_count;
        }
    }

    // ---- Serial fallback for long-form / mel-failed / invalid-encode. ----
    for (int b = 0; b < n; ++b) {
        if (have_result[b] || !needs_serial[b]) {
            continue;
        }
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        const transcribe_status st = (pcm[b] == nullptr || n_samples[b] <= 0) ?
                                         TRANSCRIBE_ERR_INVALID_ARG :
                                         whisper_run(cc, pcm[b], n_samples[b], params);
        if (st == TRANSCRIBE_OK) {
            results[b] = cc->capture_result(st);
        } else {
            results[b]        = transcribe_session::ResultSet{};
            results[b].status = st;
        }
        have_result[b] = 1;
    }

    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        int n_serial = 0;
        for (int b = 0; b < n; ++b) {
            n_serial += needs_serial[b] ? 1 : 0;
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "whisper run_batch: n=%d batched=%d serial=%d tiers=%d want_ts=%d "
                "T_enc_max=%d kv_window=%d (cap %d) prompt=%d\n"
                "  mel=%.1fms (parallel)  enc=%.1fms (serial x%d)  decode=%.1fms (batched)",
                n, n_batch_active, n_serial, static_cast<int>(temperatures.size()), want_ts ? 1 : 0, T_enc_max,
                kv_window, max_n_kv, prompt_len, mel_us / 1000.0, enc_us / 1000.0, n_batch_active, dec_us / 1000.0);
    }

    for (int b = 0; b < n; ++b) {
        cc->batch_results.push_back(std::move(results[b]));
    }
    return TRANSCRIBE_OK;
}

// Kind+slot probe. Whisper ships only the WHISPER_RUN run-extension and
// has no streaming surface, so the _STREAM slot is always false and the
// _RUN slot accepts only WHISPER_RUN. There is currently no whisper
// variant that ships without the run-ext surface.
static bool whisper_accepts_ext_kind(const transcribe_model * model, transcribe_ext_slot slot, uint32_t kind) {
    (void) model;
    if (slot != TRANSCRIBE_EXT_SLOT_RUN) {
        return false;
    }
    return kind == TRANSCRIBE_EXT_KIND_WHISPER_RUN;
}

// Pre-clear validation for the _RUN slot (see Arch::run_validate). Enforces the
// per-kind minimum (full transcribe_whisper_run_ext) before the prior result
// snapshot is cleared, so a too-small run ext is rejected without destroying
// the prior transcript. whisper_run repeats this (defense in depth) before the
// cast.
//
// Validates SHAPE only. The run-ext VALUE checks (ALL_SEGMENTS without
// condition_on_prev_tokens, prompt_tokens re-including <|startofprev|>,
// disallowed specials in initial_prompt) live in whisper_run() and reject after
// the snapshot is cleared — an accepted gap, since run() is one-shot with no
// accumulating transcript to protect.
static transcribe_status whisper_run_validate(const transcribe_session * ctx, const transcribe_run_params * params) {
    (void) ctx;
    return transcribe_ext_check(params != nullptr ? params->family : nullptr, TRANSCRIBE_EXT_KIND_WHISPER_RUN,
                                sizeof(struct transcribe_whisper_run_ext));
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "whisper",
    /* .load             = */ whisper_load,
    /* .init_context     = */ whisper_init_context,
    /* .run              = */ whisper_run,
    /* .run_batch        = */ whisper_run_batch,
    /* .stream_validate  = */ nullptr,
    /* .stream_begin     = */ nullptr,
    /* .stream_feed      = */ nullptr,
    /* .stream_finalize  = */ nullptr,
    /* .stream_reset     = */ nullptr,
    /* .accepts_ext_kind = */ whisper_accepts_ext_kind,
    /* .run_validate     = */ whisper_run_validate,
};

}  // namespace transcribe::whisper
