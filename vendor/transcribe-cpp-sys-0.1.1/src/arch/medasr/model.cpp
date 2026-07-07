// arch/medasr/model.cpp - MedASR (Google LASR-CTC) family handler.
//
// load() reads hparams, validates the tensor catalog, streams weight
// bytes into the backend buffer, fuses the per-block BatchNorm at the
// host level, and initializes the LASR mel frontend.
//
// init_context() allocates the per-context state.
//
// run() builds the encoder graph, uploads the mel (or the reference
// mel via TRANSCRIBE_MEL_FROM_REF), allocates and uploads the
// per-block fused-BN scale/bias inputs, computes the graph, reads
// back the CTC logits, performs greedy collapse (drop blanks, collapse
// repeats), and populates the result snapshot.

#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "medasr.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-env.h"
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
#include <fstream>
#include <ios>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::medasr {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, MedAsrModel>);
static_assert(std::is_base_of_v<transcribe_session, MedAsrSession>);

MedAsrSession::~MedAsrSession() {
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
    encoder_out = nullptr;
}

MedAsrModel::~MedAsrModel() {
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

constexpr const char k_default_variant[] = "medasr";

// ---------------------------------------------------------------------------
// Input-length contract (see docs/input-limits.md). MedASR is a soft-window
// family: the encoder is a CTC Conformer with no hard context cap, but its
// RoPE was trained to enc_max_pos_emb encoder positions. Past that the rotary
// embeddings extrapolate and accuracy degrades, so over-window audio is WARNED
// and PROCESSED — never rejected, never altered. The decode numerics are
// untouched: this contract only observes the frame count and logs.
// ---------------------------------------------------------------------------

// Time downsampling from mel frames to encoder frames: each of the
// sub_n_layers subsampling convs strides by sub_stride (validated to 2x2 in
// read_medasr_hparams), so the effective factor is sub_stride ^ sub_n_layers
// (== 4 for the shipped checkpoint). Falls back to 4 if either hparam is
// missing so we never divide by an unset factor.
int medasr_subsample_factor(const MedAsrHParams & hp) {
    if (hp.enc_sub_stride <= 0 || hp.enc_sub_n_layers <= 0) {
        return 4;
    }
    int factor = 1;
    for (int i = 0; i < hp.enc_sub_n_layers; ++i) {
        factor *= hp.enc_sub_stride;
    }
    return factor;
}

// Milliseconds of 16 kHz audio per encoder frame: one mel frame is
// hop_length / sample_rate seconds, and the subsampling stack keeps one
// encoder frame per `subsample_factor` mel frames. For the shipped hparams
// (hop=160, sr=16000, 4x) this is 160 * 4 * 1000 / 16000 = 40 ms/frame
// (100 fps mel -> 25 fps encoder), matching the per-token stride used by the
// CTC decoder below. Returns 0 if a rate hparam is missing.
int64_t medasr_ms_per_encoder_frame(const MedAsrHParams & hp) {
    if (hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0) {
        return 0;
    }
    return static_cast<int64_t>(hp.fe_hop_length) * medasr_subsample_factor(hp) * 1000 / hp.fe_sample_rate;
}

// Advisory transcribe_capabilities::max_audio_ms: the RoPE-trained window,
// enc_max_pos_emb encoder frames * ms_per_encoder_frame. This is the point
// past which RoPE extrapolates; longer audio is accepted with a WARN (see
// run()), not rejected. Returns 0 ("unknown / unbounded") if enc_max_pos_emb
// or a frontend rate is missing, so a misconfigured model is never advertised
// with a wrong finite number. For the shipped hparams this is
// 10000 * 40 ms = 400000 ms (~400 s).
int64_t medasr_max_audio_ms(const MedAsrHParams & hp) {
    const int64_t ms_per_frame = medasr_ms_per_encoder_frame(hp);
    if (hp.enc_max_pos_emb <= 0 || ms_per_frame <= 0) {
        return 0;
    }
    return static_cast<int64_t>(hp.enc_max_pos_emb) * ms_per_frame;
}

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<MedAsrModel>();
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
    if (auto st = read_medasr_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the soft-window input-length advisory now that the RoPE range
    // and frontend rate are known (apply_family_invariants ran before the
    // hparams were read, so it could not set this). MedASR never rejects on
    // length; this is the window past which RoPE degrades. See
    // docs/input-limits.md.
    m->caps.max_audio_ms = medasr_max_audio_ms(m->hparams);

    // Reopen for tensor metadata.
    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_medasr_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "medasr", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "medasr");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Fuse BatchNorm into host-resident scale/bias vectors. After this
    // point the raw bn.{weight,bias,running_mean,running_var} tensors
    // are no longer consulted.
    if (auto st = fuse_batch_norm(gguf_data, m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    gguf_free(gguf_data);

    // LASR mel frontend (window + filterbank baked into the GGUF). The
    // shared MelFrontend handles n_fft=512 with Accelerate vDSP / cblas
    // primitives; medasr opts in via pad_mode="none" (center=False) and
    // log_clamp_min=1e-5 (natural log with floor-clamp, vs NeMo's
    // log(x+eps)). The GGUF filterbank is laid out [n_freq, n_mels]
    // row-major (ggml ne=[n_mels, n_freq]); MelConfig wants [n_mels,
    // n_freq] row-major, so we transpose once at load.
    {
        const MedAsrHParams & hp     = m->hparams;
        const int             n_freq = hp.fe_n_fft / 2 + 1;
        transcribe::MelConfig cfg{};
        cfg.sample_rate   = hp.fe_sample_rate;
        cfg.num_mels      = hp.fe_num_mels;
        cfg.n_fft         = hp.fe_n_fft;
        cfg.win_length    = hp.fe_win_length;
        cfg.hop_length    = hp.fe_hop_length;
        cfg.pre_emphasis  = 0.0f;
        cfg.f_min         = hp.fe_mel_lower_hz;
        cfg.f_max         = hp.fe_mel_upper_hz;
        cfg.pad_mode      = "none";
        cfg.window_type   = "hann_symmetric";
        cfg.normalize     = "none";
        cfg.log_clamp_min = hp.fe_log_clamp_min;

        if (m->weights.frontend_window != nullptr) {
            cfg.window.assign(hp.fe_win_length, 0.0f);
            ggml_backend_tensor_get(m->weights.frontend_window, cfg.window.data(), 0,
                                    cfg.window.size() * sizeof(float));
        }
        if (m->weights.frontend_mel_filterbank != nullptr) {
            std::vector<float> raw(static_cast<size_t>(hp.fe_num_mels) * n_freq);
            ggml_backend_tensor_get(m->weights.frontend_mel_filterbank, raw.data(), 0, raw.size() * sizeof(float));
            // raw is [n_freq, n_mels] row-major (mel fast). Transpose
            // into [n_mels, n_freq] row-major (freq fast) for MelConfig.
            cfg.filterbank.assign(static_cast<size_t>(hp.fe_num_mels) * n_freq, 0.0f);
            for (int k = 0; k < n_freq; ++k) {
                for (int mi = 0; mi < hp.fe_num_mels; ++mi) {
                    cfg.filterbank[static_cast<size_t>(mi) * n_freq + k] =
                        raw[static_cast<size_t>(k) * hp.fe_num_mels + mi];
                }
            }
        }
        m->mel.emplace(cfg);
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

    auto gc       = std::make_unique<MedAsrSession>();
    gc->model     = model;
    gc->n_threads = params->n_threads;
    gc->kv_type   = params->kv_type;

    *out_ctx = gc.release();
    return TRANSCRIBE_OK;
}

#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
// Load reference mel sidecar from <dir>/mel.in.f32 (or
// frontend.mel.out.f32).
transcribe_status load_ref_mel(const std::string & dir, int n_mels, int & n_mel_frames, std::vector<float> & out) {
    auto try_open = [&](const std::string & path) -> std::ifstream {
        return std::ifstream(path, std::ios::binary | std::ios::ate);
    };
    std::string base = dir;
    if (!base.empty() && base.back() != '/') {
        base += '/';
    }

    std::ifstream f    = try_open(base + "mel.in.f32");
    std::string   used = base + "mel.in.f32";
    if (!f) {
        f    = try_open(base + "frontend.mel.out.f32");
        used = base + "frontend.mel.out.f32";
    }
    if (!f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr: cannot open mel ref under '%s' "
                "(expected mel.in.f32 or frontend.mel.out.f32)",
                dir.c_str());
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }

    const std::streamsize total_bytes = f.tellg();
    f.seekg(0, std::ios::beg);
    if (total_bytes <= 0 || (static_cast<size_t>(total_bytes) % (sizeof(float) * n_mels)) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "medasr: mel ref '%s' size %lld not divisible by "
                "n_mels=%d * 4",
                used.c_str(), static_cast<long long>(total_bytes), n_mels);
        return TRANSCRIBE_ERR_GGUF;
    }
    const size_t n_elems = static_cast<size_t>(total_bytes) / sizeof(float);
    n_mel_frames         = static_cast<int>(n_elems / n_mels);
    out.assign(n_elems, 0.0f);
    f.read(reinterpret_cast<char *>(out.data()), static_cast<std::streamsize>(total_bytes));
    if (static_cast<std::streamsize>(f.gcount()) != total_bytes) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr: short read on mel ref");
        return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_OK;
}
#endif  // TRANSCRIBE_ENABLE_VALIDATION_HOOKS

// Greedy CTC decode: argmax per frame, collapse repeats, drop blanks
// AND skip the LASR special tokens (<s>=1, </s>=2, <unk>=3) — matching
// the reference's `processor.batch_decode(skip_special_tokens=True)`.
// Without this, a final frame that argmaxes to id 2 (`</s>`) leaks the
// literal `</s>` into the transcript, scoring as a +1 insertion (~0.2pp
// WER on test-clean). `<epsilon>` (id 0) is the standard blank-skip.
void decode_ctc_greedy(const float *      logits,
                       int                vocab,
                       int                T_enc,
                       int                blank_id,
                       std::vector<int> & tokens,
                       std::vector<int> & frames) {
    tokens.clear();
    frames.clear();
    int prev = -1;
    for (int t = 0; t < T_enc; ++t) {
        const float * row    = logits + static_cast<size_t>(t) * vocab;
        int           best   = 0;
        float         best_v = row[0];
        for (int v = 1; v < vocab; ++v) {
            if (row[v] > best_v) {
                best_v = row[v];
                best   = v;
            }
        }
        if (best == blank_id) {
            prev = -1;
            continue;
        }
        if (best == prev) {
            continue;
        }
        prev = best;
        // skip_special_tokens=True: drop ids 1 (<s>), 2 (</s>), 3 (<unk>).
        if (best == 1 || best == 2 || best == 3) {
            continue;
        }
        tokens.push_back(best);
        frames.push_back(t);
    }
}

transcribe_status run(transcribe_session * session, const float * pcm, int n_samples, const transcribe_run_params *) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * gc = static_cast<MedAsrSession *>(session);
    auto * gm = static_cast<MedAsrModel *>(gc->model);
    if (gm == nullptr || gm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (gc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    // ----- Mel acquisition -----------------------------------------------
    int           mel_n_frames = 0;
    bool          mel_from_ref = false;
    const int64_t t_mel_start  = ggml_time_us();
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    if (const char * ref_dir = transcribe::env::str("TRANSCRIBE_MEL_FROM_REF")) {
        if (auto st = load_ref_mel(ref_dir, gm->hparams.fe_num_mels, mel_n_frames, gc->mel_buf); st != TRANSCRIBE_OK) {
            return st;
        }
        mel_from_ref = true;
    }
#endif
    if (!mel_from_ref) {
        int                out_n_mels = 0;
        std::vector<float> m_major;
        if (auto st =
                gm->mel->compute(pcm, static_cast<size_t>(n_samples), m_major, out_n_mels, mel_n_frames, gc->n_threads);
            st != TRANSCRIBE_OK) {
            return st;
        }
        // Shared MelFrontend writes [n_mels, T_mel] row-major (m * T + t);
        // medasr's encoder mel_in tensor (ne=[n_mels, T_mel, 1, B]) expects
        // T-major bytes (each frame is n_mels contiguous floats). One-shot
        // transpose into mel_buf.
        gc->mel_buf.assign(static_cast<size_t>(out_n_mels) * mel_n_frames, 0.0f);
        for (int mi = 0; mi < out_n_mels; ++mi) {
            const float * src = m_major.data() + static_cast<size_t>(mi) * mel_n_frames;
            for (int t = 0; t < mel_n_frames; ++t) {
                gc->mel_buf[static_cast<size_t>(t) * out_n_mels + mi] = src[t];
            }
        }
    }
    gc->t_mel_us = ggml_time_us() - t_mel_start;
    if (mel_n_frames <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- Soft-window input-length advisory (see docs/input-limits.md) -----
    // MedASR has no hard context cap, but its RoPE was trained to
    // enc_max_pos_emb encoder frames. Predict the encoder frame count from the
    // mel frames via the two stride-2 subsampling convs (same formula as the
    // run_batch path's subsampling_t_out, applied twice). If it exceeds the
    // trained range, WARN once and PROCEED — we do not reject and do not touch
    // the numerics; accuracy may simply degrade past this point.
    {
        const int sub_k = gm->hparams.enc_sub_kernel;
        const int sub_s = gm->hparams.enc_sub_stride;
        auto      sub_t = [&](int t_in) {
            return (t_in < sub_k) ? 0 : (t_in - sub_k) / sub_s + 1;
        };
        const int enc_frames = sub_t(sub_t(mel_n_frames));
        if (enc_frames > gm->hparams.enc_max_pos_emb) {
            const int64_t ms_per_frame = medasr_ms_per_encoder_frame(gm->hparams);
            const double  audio_s      = static_cast<double>(enc_frames) * static_cast<double>(ms_per_frame) / 1000.0;
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                "medasr run: audio is %.1f s (%d encoder frames), beyond the "
                                "%d-frame RoPE range this model was trained on; transcription "
                                "may be degraded past this point. See "
                                "transcribe_capabilities.max_audio_ms.",
                                audio_s, enc_frames, gm->hparams.enc_max_pos_emb);
        }
    }

    // ----- Reset per-call compute state ---------------------------------
    if (gc->compute_ctx != nullptr) {
        ggml_free(gc->compute_ctx);
        gc->compute_ctx = nullptr;
    }
    gc->encoder_out = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size     = 4 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        gc->compute_ctx = ggml_init(ip);
        if (gc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "medasr run: compute context allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // ----- Build encoder graph ------------------------------------------
    EncoderBuild eb = build_encoder_graph(gc->compute_ctx, gm->weights, gm->hparams, mel_n_frames, gm->backend.c_str(),
                                          /*n_batch=*/1, /*batch_var_len=*/false);
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- Scheduler ----------------------------------------------------
    if (gc->sched == nullptr) {
        gc->sched = ggml_backend_sched_new(gm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(gm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (gc->sched == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "medasr run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(gc->sched);
    if (!ggml_backend_sched_alloc_graph(gc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "medasr run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    // ----- Upload inputs ------------------------------------------------
    ggml_backend_tensor_set(eb.mel_in, gc->mel_buf.data(), 0, gc->mel_buf.size() * sizeof(float));
    transcribe::debug::dump_tensor("mel.in", eb.mel_in, "encoder.mel");

    if (eb.positions != nullptr) {
        const int64_t        T_enc = eb.positions->ne[0];
        std::vector<int32_t> pos(T_enc);
        for (int64_t i = 0; i < T_enc; ++i) {
            pos[i] = static_cast<int32_t>(i);
        }
        ggml_backend_tensor_set(eb.positions, pos.data(), 0, pos.size() * sizeof(int32_t));
    }

    // Per-block fused-BN scale + bias.
    const int    n_layers        = gm->hparams.enc_n_layers;
    const int    d_model         = gm->hparams.enc_hidden;
    const size_t bytes_per_layer = d_model * sizeof(float);
    for (int i = 0; i < n_layers; ++i) {
        if (i < static_cast<int>(eb.bn_scale_inputs.size()) && eb.bn_scale_inputs[i] != nullptr) {
            ggml_backend_tensor_set(eb.bn_scale_inputs[i], gm->weights.fused_bn_scale_storage[i].data(), 0,
                                    bytes_per_layer);
        }
        if (i < static_cast<int>(eb.bn_bias_inputs.size()) && eb.bn_bias_inputs[i] != nullptr) {
            ggml_backend_tensor_set(eb.bn_bias_inputs[i], gm->weights.fused_bn_bias_storage[i].data(), 0,
                                    bytes_per_layer);
        }
    }

    // ----- Compute ------------------------------------------------------
    const int64_t t_enc_start = ggml_time_us();
    if (ggml_backend_sched_graph_compute(gc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr: graph_compute failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    gc->t_encode_us = ggml_time_us() - t_enc_start;

    // ----- Dump intermediates -------------------------------------------
    auto try_dump = [&](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.subsampling.out", eb.dumps.subsampling_out, "encoder.subsampling");
    try_dump("enc.block.0.post_ff1", eb.dumps.block0_post_ff1, "encoder.block.0");
    try_dump("enc.block.0.post_attn", eb.dumps.block0_post_attn, "encoder.block.0");
    try_dump("enc.block.0.post_conv", eb.dumps.block0_post_conv, "encoder.block.0");
    try_dump("enc.block.0.post_ff2", eb.dumps.block0_post_ff2, "encoder.block.0");
    for (int i = 0; i < n_layers; ++i) {
        if (eb.dumps.all_block_outs[i] == nullptr) {
            continue;
        }
        char nm[32];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", i);
        try_dump(nm, eb.dumps.all_block_outs[i], "encoder.block");
    }
    try_dump("enc.out_norm.out", eb.dumps.out_norm_out, "encoder.out_norm");
    try_dump("enc.ctc_logits", eb.dumps.ctc_logits_for_dump, "encoder.ctc_logits");

    // Per-utterance encoder-output dump (single-shot baseline for the
    // batched-equals-single-shot gate). Matches the parakeet / gigaam name.
    if (transcribe::debug::enabled() && eb.dumps.out_norm_out != nullptr) {
        ggml_tensor * t          = eb.dumps.out_norm_out;
        const int     d_enc      = static_cast<int>(t->ne[0]);
        const int     T_enc_dump = static_cast<int>(t->ne[1]);
        gc->enc_host.assign(static_cast<size_t>(d_enc) * T_enc_dump, 0.0f);
        ggml_backend_tensor_get(t, gc->enc_host.data(), 0, gc->enc_host.size() * sizeof(float));
        const long long shape[2] = { T_enc_dump, d_enc };
        transcribe::debug::dump_host_f32("dec.enc_out", gc->enc_host.data(),
                                         static_cast<long long>(gc->enc_host.size()), shape, 2, "decoder.enc_out");
    }

    gc->encoder_out = eb.out;

    // ----- Greedy CTC decode --------------------------------------------
    const int64_t t_dec_start = ggml_time_us();
    ggml_tensor * logits_t    = eb.dumps.ctc_logits;
    if (logits_t == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    const int vocab = static_cast<int>(logits_t->ne[0]);
    const int T_enc = static_cast<int>(logits_t->ne[1]);
    if (vocab != gm->hparams.ctc_vocab_size || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr: ctc_logits shape mismatch (vocab=%d T_enc=%d)", vocab, T_enc);
        return TRANSCRIBE_ERR_GGUF;
    }
    gc->logits_buf.assign(static_cast<size_t>(vocab) * T_enc, 0.0f);
    ggml_backend_tensor_get(logits_t, gc->logits_buf.data(), 0, gc->logits_buf.size() * sizeof(float));

    std::vector<int> tokens;
    std::vector<int> frames;
    decode_ctc_greedy(gc->logits_buf.data(), vocab, T_enc, gm->hparams.ctc_blank_id, tokens, frames);
    gc->t_decode_us = ggml_time_us() - t_dec_start;

    std::string text = gm->tok.decode(tokens.data(), static_cast<int>(tokens.size()));
    if (!text.empty() && text.front() == ' ') {
        text.erase(text.begin());
    }
    gc->full_text   = text;
    gc->has_result  = true;
    gc->result_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    // Tokens at 40 ms per frame (16 kHz / hop=160 = 100 fps; subsampling
    // 4x reduces to 25 fps = 40 ms).
    gc->tokens.clear();
    gc->tokens.reserve(tokens.size());
    for (size_t i = 0; i < tokens.size(); ++i) {
        transcribe_session::TokenEntry te{};
        te.id    = tokens[i];
        te.text  = gm->tok.token(tokens[i]);
        te.t0_ms = static_cast<int64_t>(frames[i]) * 40;
        te.t1_ms = te.t0_ms + 40;
        gc->tokens.push_back(te);
    }

    return TRANSCRIBE_OK;
}

}  // namespace

// ---------------------------------------------------------------------------
// Offline batch (transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// Batches B utterances through one Conformer encoder dispatch (the batch
// rides ne[3] of the mel tensor + every activation in between), then
// host-decodes each utterance's CTC slice. The mel front-end and greedy
// decode are identical to single-shot; only the encoder is fused.
//
// Same-length batches run mask-free (bit-identical to single-shot per
// utterance). Variable-length batches pad to T_max along time and apply
// per-utterance masks (attn_pad_mask + conv_pad_mask), keeping a real
// query from attending to another utterance's padded tail.

namespace {

// Per-utterance post-subsampling frame count.
//   T_mel -> floor((T_mel - sub_kernel) / sub_stride) + 1     (conv_0)
//         -> floor((T1 - sub_kernel) / sub_stride) + 1        (conv_1)
// With sub_kernel=5 and sub_stride=2, padding=0, two stride-2 convs.
int subsampling_t_out(int T_in, int sub_kernel, int sub_stride) {
    if (T_in < sub_kernel) {
        return 0;
    }
    return (T_in - sub_kernel) / sub_stride + 1;
}

// medasr pack: source [b] is [T_b, n_mels] row-major (medasr mel
// orientation); destination is contiguous [n_mels-fast, T_max-slow,
// 1, n] byte-equivalent to numpy [n, T_max, n_mels]. Per utterance b
// the slab is at offset b * n_mels * T_max; valid time [0, T_b) is
// a memcpy of T_b*n_mels floats, tail stays zero. Matches the encoder
// mel_in tensor ne = [n_mels, T_max, 1, n].
void pack_pad_time_major(std::vector<float> &                    dst,
                         const std::vector<std::vector<float>> & src,
                         const std::vector<int> &                lens,
                         int                                     n_mels,
                         int                                     T_max) {
    const size_t n = src.size();
    dst.assign(static_cast<size_t>(n_mels) * T_max * n, 0.0f);
    for (size_t b = 0; b < n; ++b) {
        const size_t slab_off    = b * n_mels * T_max;
        const int    T_b         = lens[static_cast<size_t>(b)];
        const size_t copy_floats = static_cast<size_t>(T_b) * n_mels;
        if (T_b > 0 && copy_floats <= src[b].size()) {
            std::memcpy(dst.data() + slab_off, src[b].data(), copy_floats * sizeof(float));
        }
        // Tail (t >= T_b) stays zero from the assign above.
    }
}

transcribe_status decode_one_utterance(MedAsrSession * gc,
                                       MedAsrModel *   gm,
                                       const float *   enc_logits,
                                       int             T_enc_valid,
                                       int             vocab,
                                       int /*utt_index*/) {
    const int64_t t_dec_start = ggml_time_us();

    std::vector<int> tokens;
    std::vector<int> frames;
    decode_ctc_greedy(enc_logits, vocab, T_enc_valid, gm->hparams.ctc_blank_id, tokens, frames);

    gc->t_decode_us = ggml_time_us() - t_dec_start;

    std::string text = gm->tok.decode(tokens.data(), static_cast<int>(tokens.size()));
    if (!text.empty() && text.front() == ' ') {
        text.erase(text.begin());
    }
    gc->full_text   = text;
    gc->has_result  = true;
    gc->result_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    gc->tokens.clear();
    gc->tokens.reserve(tokens.size());
    for (size_t i = 0; i < tokens.size(); ++i) {
        transcribe_session::TokenEntry te{};
        te.id    = tokens[i];
        te.text  = gm->tok.token(tokens[i]);
        te.t0_ms = static_cast<int64_t>(frames[i]) * 40;
        te.t1_ms = te.t0_ms + 40;
        gc->tokens.push_back(te);
    }
    return TRANSCRIBE_OK;
}

transcribe_status run_batch_encode(MedAsrSession *                         gc,
                                   MedAsrModel *                           gm,
                                   const std::vector<std::vector<float>> & mels,
                                   const std::vector<int> &                nf,
                                   int                                     n_mels,
                                   int                                     T_max,
                                   int64_t                                 total_mel_us) {
    const int n       = static_cast<int>(mels.size());
    bool      var_len = false;
    for (int b = 0; b < n; ++b) {
        if (nf[b] != T_max) {
            var_len = true;
            break;
        }
    }

    pack_pad_time_major(gc->mel_buf, mels, nf, n_mels, T_max);

    if (gc->compute_ctx != nullptr) {
        ggml_free(gc->compute_ctx);
        gc->compute_ctx = nullptr;
    }
    gc->encoder_out = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size     = 4 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        gc->compute_ctx = ggml_init(ip);
        if (gc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "medasr run: compute context allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    EncoderBuild eb = build_encoder_graph(gc->compute_ctx, gm->weights, gm->hparams, T_max, gm->backend.c_str(),
                                          /*n_batch=*/n, /*batch_var_len=*/var_len);
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (gc->sched == nullptr) {
        gc->sched = ggml_backend_sched_new(gm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(gm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (gc->sched == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "medasr run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(gc->sched);
    if (!ggml_backend_sched_alloc_graph(gc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "medasr run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload mel + positions.
    ggml_backend_tensor_set(eb.mel_in, gc->mel_buf.data(), 0, gc->mel_buf.size() * sizeof(float));
    if (eb.positions != nullptr) {
        const int64_t        T_enc = eb.positions->ne[0];
        std::vector<int32_t> pos(T_enc);
        for (int64_t i = 0; i < T_enc; ++i) {
            pos[i] = static_cast<int32_t>(i);
        }
        ggml_backend_tensor_set(eb.positions, pos.data(), 0, pos.size() * sizeof(int32_t));
    }

    // Per-layer fused-BN scale + bias (1-D, broadcast over the batch).
    const int    n_layers        = gm->hparams.enc_n_layers;
    const int    d_model         = gm->hparams.enc_hidden;
    const size_t bytes_per_layer = d_model * sizeof(float);
    for (int i = 0; i < n_layers; ++i) {
        if (i < static_cast<int>(eb.bn_scale_inputs.size()) && eb.bn_scale_inputs[i] != nullptr) {
            ggml_backend_tensor_set(eb.bn_scale_inputs[i], gm->weights.fused_bn_scale_storage[i].data(), 0,
                                    bytes_per_layer);
        }
        if (i < static_cast<int>(eb.bn_bias_inputs.size()) && eb.bn_bias_inputs[i] != nullptr) {
            ggml_backend_tensor_set(eb.bn_bias_inputs[i], gm->weights.fused_bn_bias_storage[i].data(), 0,
                                    bytes_per_layer);
        }
    }

    // Per-utterance encoder-frame count after the two stride-2 subsampling
    // convs.
    std::vector<int> real_tenc(static_cast<size_t>(n), 0);
    const int        sub_k = gm->hparams.enc_sub_kernel;
    const int        sub_s = gm->hparams.enc_sub_stride;
    for (int b = 0; b < n; ++b) {
        int t        = subsampling_t_out(nf[b], sub_k, sub_s);
        t            = subsampling_t_out(t, sub_k, sub_s);
        real_tenc[b] = t;
    }

    // ----- Soft-window input-length advisory (see docs/input-limits.md) -----
    // Same warn-and-proceed contract as single-shot run(), applied per
    // utterance on the fused batched encode path so over-window clips in a
    // batch are not silently degraded. real_tenc[b] is the encoder frame count
    // (the same double-subsampling derivation run() uses); compare it to the
    // RoPE-trained range and WARN once per over-window utterance. The serial
    // fallback in run_batch() re-enters run(), which warns there, so each clip
    // is warned exactly once. Never reject, never touch the numerics.
    {
        const int64_t ms_per_frame = medasr_ms_per_encoder_frame(gm->hparams);
        for (int b = 0; b < n; ++b) {
            if (real_tenc[b] > gm->hparams.enc_max_pos_emb) {
                const double audio_s = static_cast<double>(real_tenc[b]) * static_cast<double>(ms_per_frame) / 1000.0;
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                    "medasr run: utterance %d: audio is %.1f s (%d encoder "
                                    "frames), beyond the %d-frame RoPE range this model was "
                                    "trained on; transcription may be degraded past this point. "
                                    "See transcribe_capabilities.max_audio_ms.",
                                    b, audio_s, real_tenc[b], gm->hparams.enc_max_pos_emb);
            }
        }
    }

    if (var_len) {
        const int T_enc_max = real_tenc[0];
        int       T_max_eff = T_enc_max;
        for (int b = 1; b < n; ++b) {
            if (real_tenc[b] > T_max_eff) {
                T_max_eff = real_tenc[b];
            }
        }
        transcribe::fill_keypad_mask(eb.attn_pad_mask_in, real_tenc, T_max_eff, n);
        transcribe::fill_valid_frame_mask(eb.conv_pad_mask_in, real_tenc, T_max_eff, n);
    }

    // Per-backend thread count (matching single-shot through the scheduler).
    transcribe::configure_sched_n_threads(gc->sched, gc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    if (ggml_backend_sched_graph_compute(gc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "medasr run_batch: graph_compute failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    gc->t_encode_us = ggml_time_us() - t_enc_start;

    // Per-utterance encoder-output dump for the batched-equals-single-shot
    // gate. Read the whole [d_enc, T_enc, 1, n] tensor once, then host-slice
    // and dump each utterance under `dec.enc_out.b{i}`.
    if (transcribe::debug::enabled() && eb.dumps.out_norm_out != nullptr) {
        ggml_tensor * t          = eb.dumps.out_norm_out;
        const int     d_enc      = static_cast<int>(t->ne[0]);
        const int     T_enc_dump = static_cast<int>(t->ne[1]);
        const size_t  utt_n      = static_cast<size_t>(d_enc) * T_enc_dump;
        gc->enc_host.assign(utt_n * static_cast<size_t>(n), 0.0f);
        ggml_backend_tensor_get(t, gc->enc_host.data(), 0, gc->enc_host.size() * sizeof(float));
        const long long shape[2] = { T_enc_dump, d_enc };
        for (int b = 0; b < n; ++b) {
            const std::string nm = "dec.enc_out.b" + std::to_string(b);
            transcribe::debug::dump_host_f32(nm.c_str(), gc->enc_host.data() + b * utt_n, static_cast<long long>(utt_n),
                                             shape, 2, "decoder.enc_out");
        }
    }

    // The decode tensor is eb.dumps.ctc_logits (ne=[vocab, T_enc, 1, n]),
    // not the transposed dump tensor. Byte layout per utterance b:
    // slab b at offset b * T_enc * vocab; within the slab, frame t at
    // offset t * vocab, then per-frame argmax over `vocab` floats.
    ggml_tensor * logits_t = eb.dumps.ctc_logits;
    if (logits_t == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    const int vocab = static_cast<int>(logits_t->ne[0]);
    const int T_enc = static_cast<int>(logits_t->ne[1]);
    if (vocab != gm->hparams.ctc_vocab_size || T_enc <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    const size_t utt_elems = static_cast<size_t>(vocab) * T_enc;
    gc->logits_buf.assign(utt_elems * static_cast<size_t>(n), 0.0f);
    ggml_backend_tensor_get(logits_t, gc->logits_buf.data(), 0, gc->logits_buf.size() * sizeof(float));

    // Host-decode each utterance with shared encode + total mel amortized.
    return transcribe::decode_batch_slices(gc, n, gc->logits_buf.data(), utt_elems, gc->t_encode_us, total_mel_us,
                                           [&](int b, const float * enc_b) {
                                               // enc_b points at utterance b's full T_enc slab; the decode
                                               // loop reads `real_tenc[b]` valid frames and ignores the padded
                                               // tail (per-utterance valid-frame count from the
                                               // subsampling-output formula above).
                                               return decode_one_utterance(gc, gm, enc_b, real_tenc[b], vocab, b);
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
    auto * gc = static_cast<MedAsrSession *>(session);
    auto * gm = static_cast<MedAsrModel *>(gc->model);
    if (gm == nullptr || gm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    transcribe::debug::init();

    // Per-utterance mel in parallel.
    const int                       n_mels = gm->hparams.fe_num_mels;
    std::vector<std::vector<float>> mels(static_cast<size_t>(n));
    std::vector<int>                nf(static_cast<size_t>(n), 0);
    const int64_t                   t_mel_start  = ggml_time_us();
    const bool                      all_ok       = transcribe::parallel_for_all(n, gc->n_threads, [&](int i) -> bool {
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            return false;
        }
        int                     this_frames = 0;
        int                     out_n_mels  = 0;
        std::vector<float>      m_major;
        const transcribe_status st =
            gm->mel->compute(pcm[i], static_cast<size_t>(n_samples[i]), m_major, out_n_mels, this_frames, 1);
        if (st != TRANSCRIBE_OK || this_frames <= 0) {
            return false;
        }
        // Transpose [n_mels, T_mel] -> T-major to match the
        // encoder's mel_in tensor layout. See run() for the
        // detailed comment.
        mels[i].assign(static_cast<size_t>(out_n_mels) * this_frames, 0.0f);
        for (int mi = 0; mi < out_n_mels; ++mi) {
            const float * src = m_major.data() + static_cast<size_t>(mi) * this_frames;
            for (int t = 0; t < this_frames; ++t) {
                mels[i][static_cast<size_t>(t) * out_n_mels + mi] = src[t];
            }
        }
        nf[i] = this_frames;
        return true;
    });
    const int64_t                   total_mel_us = ggml_time_us() - t_mel_start;

    if (all_ok) {
        int T_max = 0;
        for (int i = 0; i < n; ++i) {
            T_max = std::max(T_max, nf[i]);
        }
        return run_batch_encode(gc, gm, mels, nf, n_mels, T_max, total_mel_us);
    }

    // Per-utterance fallback (also the malformed-input path).
    for (int i = 0; i < n; ++i) {
        if (gc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            transcribe_session::ResultSet rs;
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            gc->batch_results.push_back(std::move(rs));
            continue;
        }
        gc->clear_result();
        const transcribe_status st = run(session, pcm[i], n_samples[i], params);
        gc->batch_results.push_back(gc->capture_result(st));
    }
    return TRANSCRIBE_OK;
}

}  // namespace

const Arch arch = {
    /*.name             =*/"medasr",
    /*.load             =*/load,
    /*.init_context     =*/init_context,
    /*.run              =*/run,
    /*.run_batch        =*/run_batch,
    /*.stream_validate  =*/nullptr,
    /*.stream_begin     =*/nullptr,
    /*.stream_feed      =*/nullptr,
    /*.stream_finalize  =*/nullptr,
    /*.stream_reset     =*/nullptr,
    /*.accepts_ext_kind =*/nullptr,
    /*.run_validate     =*/nullptr,
};

}  // namespace transcribe::medasr
