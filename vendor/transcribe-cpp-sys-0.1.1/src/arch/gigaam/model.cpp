// arch/gigaam/model.cpp - GigaAM family handler.
//
// load() reads hparams, validates the tensor catalog, streams weights into a
// backend buffer, and builds the mel frontend + host decoder mirror. run()
// drives the mel -> encoder graph -> host RNN-T/CTC greedy decode pipeline.

#include "decoder.h"
#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "gigaam.h"
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
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <ios>
#include <limits>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::gigaam {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, GigaamModel>);
static_assert(std::is_base_of_v<transcribe_session, GigaamSession>);

GigaamSession::~GigaamSession() {
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

GigaamModel::~GigaamModel() {
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

constexpr const char k_default_variant[] = "gigaam-v3-e2e-rnnt";

// Input-length contract (see docs/input-limits.md). GigaAM is a SOFT-WINDOW
// family: the Conformer encoder has no hard architectural cap, but every
// published variant was trained on utterances up to ~25 s, beyond which
// accuracy degrades. Upstream rejects over-length audio outright; we
// WARN-and-PROCEED so the caller keeps control of the degradation.
//
// No hparam encodes the window, so hardcode the upstream 25 s limit.
// k_safe_audio_ms is reported via transcribe_capabilities::max_audio_ms and is
// the run() soft-window WARN threshold; k_safe_s is the same value in seconds,
// used only in the WARN text.
constexpr int     k_safe_s        = 25;
constexpr int64_t k_safe_audio_ms = 25000;

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<GigaamModel>();
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
    if (auto st = read_gigaam_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the advisory soft-window now that the frontend hparams are
    // known (apply_family_invariants ran before the hparams were read, so it
    // could not set this). GigaAM warns-and-proceeds past this window rather
    // than rejecting; see docs/input-limits.md.
    m->caps.max_audio_ms = k_safe_audio_ms;

    // Reopen for tensor metadata.
    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_gigaam_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "gigaam", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "gigaam");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Build host mirror of the predictor + joint (RNN-T) or CTC head (CTC).
    if (auto st = build_host_decoder_weights(m->weights, m->hparams, m->host_decoder); st != TRANSCRIBE_OK) {
        return st;
    }

    // Initialize the family mel frontend. The Hann window and HTK
    // filterbank are baked into the GGUF by the converter; mel.init
    // copies them out of backend memory rather than recomputing.
    m->mel.init(m->hparams, m->weights);

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

    auto gc       = std::make_unique<GigaamSession>();
    gc->model     = model;
    gc->n_threads = params->n_threads;
    gc->kv_type   = params->kv_type;

    *out_ctx = gc.release();
    return TRANSCRIBE_OK;
}

namespace {

// Load reference mel sidecar from <dir>/frontend.mel.out.f32. Disk
// layout is row-major (n_mels, T_mel) — byte i is at numpy
// (m=i/T_mel, t=i%T_mel). The encoder mel_in tensor has
// ne=[T_mel, n_mels] (fast=T_mel, slow=n_mels), and ggml's
// fast-to-slow layout maps byte i to ne_index (t=i%T_mel, m=i/T_mel).
// Both byte orders are identical — just a memcpy is needed.
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
transcribe_status load_ref_mel(const std::string & dir, int n_mels, int & n_mel_frames, std::vector<float> & out) {
    std::string path = dir;
    if (!path.empty() && path.back() != '/') {
        path += '/';
    }
    path += "frontend.mel.out.f32";

    std::ifstream f(path, std::ios::binary | std::ios::ate);
    if (!f) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: cannot open mel ref '%s'", path.c_str());
        return TRANSCRIBE_ERR_FILE_NOT_FOUND;
    }
    const std::streamsize total_bytes = f.tellg();
    f.seekg(0, std::ios::beg);
    if (total_bytes <= 0 || (static_cast<size_t>(total_bytes) % (sizeof(float) * n_mels)) != 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "gigaam: mel ref '%s' size %lld not divisible by "
                "n_mels=%d * 4",
                path.c_str(), static_cast<long long>(total_bytes), n_mels);
        return TRANSCRIBE_ERR_GGUF;
    }

    const size_t n_elems = static_cast<size_t>(total_bytes) / sizeof(float);
    n_mel_frames         = static_cast<int>(n_elems / n_mels);
    out.assign(n_elems, 0.0f);
    f.read(reinterpret_cast<char *>(out.data()), static_cast<std::streamsize>(total_bytes));
    if (static_cast<std::streamsize>(f.gcount()) != total_bytes) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: short read on mel ref");
        return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_OK;
}
#endif  // TRANSCRIBE_ENABLE_VALIDATION_HOOKS

// Host-side RNN-T / CTC greedy decode of one utterance's encoder slice,
// publishing the transcript into the session scratch slot (full_text /
// tokens / has_result / result_kind / t_decode_us). `encoded` is the
// pre-final-transpose encoder output for a single utterance, laid out
// T-major [T_enc * d_enc] (element (t, d) at offset t*d_enc + d) — the
// same layout the graph's `rnnt.encoded` tensor materializes and that
// decode_{rnnt,ctc}_greedy expect. `utt_index` tags the per-utterance
// `dec.enc_out` dump for the batch parity gate: -1 (single-shot) writes
// `dec.enc_out`, b >= 0 writes `dec.enc_out.b{b}`. Shared by run() and
// run_batch_encode().
transcribe_status decode_and_populate(GigaamSession * gc,
                                      GigaamModel *   gm,
                                      const float *   encoded,
                                      int             T_enc,
                                      int             d_enc,
                                      int             utt_index) {
    const int64_t t_dec_start = ggml_time_us();

    // Per-utterance encoder-output dump (single-shot == reference; batched ==
    // batched-equals-single-shot gate). Matches parakeet's `dec.enc_out`.
    if (transcribe::debug::enabled()) {
        std::string dump_name = "dec.enc_out";
        if (utt_index >= 0) {
            dump_name += ".b" + std::to_string(utt_index);
        }
        const long long enc_shape[2] = { T_enc, d_enc };
        transcribe::debug::dump_host_f32(dump_name.c_str(), encoded, static_cast<long long>(T_enc) * d_enc, enc_shape,
                                         2, "decoder.enc_out");
    }

    std::vector<int> tokens;
    std::vector<int> frames;

    if (gm->hparams.head_kind == HeadKind::RNNT) {
        if (auto st = decode_rnnt_greedy(gm->host_decoder, gm->hparams, encoded, T_enc,
                                         /*max_symbols=*/10, tokens, frames);
            st != TRANSCRIBE_OK) {
            return st;
        }
    } else {  // CTC
        if (auto st = decode_ctc_greedy(gm->host_decoder, gm->hparams, encoded, T_enc, tokens, frames);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }
    gc->t_decode_us = ggml_time_us() - t_dec_start;

    std::string text = gm->tok.decode(tokens.data(), static_cast<int>(tokens.size()));
    // SentencePiece convention: the first token's leading ▁ is stripped on
    // decode. Our shared SP detokenizer maps every ▁ to a space, so trim a
    // single leading space here. Mirrors parakeet's post-decode.
    if (!text.empty() && text.front() == ' ') {
        text.erase(text.begin());
    }

    gc->full_text   = text;
    gc->has_result  = true;
    gc->result_kind = TRANSCRIBE_TIMESTAMPS_TOKEN;

    // Tokens. Subsampling factor 4 + hop 160 + sample_rate 16000 gives
    // 40 ms per encoder frame. Token spans one frame.
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

transcribe_status run(transcribe_session * session, const float * pcm, int n_samples, const transcribe_run_params *) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * gc = static_cast<GigaamSession *>(session);
    auto * gm = static_cast<GigaamModel *>(gc->model);
    if (gm == nullptr || gm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (gc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();

    // Soft-window advisory: warn once past the ~25 s training window and
    // proceed unchanged (never reject, never alter numerics). See
    // docs/input-limits.md.
    {
        const int sr = gm->hparams.fe_sample_rate;
        if (sr > 0) {
            const int64_t audio_ms = static_cast<int64_t>(n_samples) * 1000 / sr;
            if (audio_ms > k_safe_audio_ms) {
                const double seconds = static_cast<double>(audio_ms) / 1000.0;
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                    "gigaam run: audio is %.1f s, beyond the ~%d s window this "
                                    "model was trained on; transcription may be degraded. "
                                    "Split long audio into <=%d s segments. See "
                                    "transcribe_capabilities.max_audio_ms.",
                                    seconds, k_safe_s, k_safe_s);
            }
        }
    }

    // Mel acquisition: the family mel (HTK + power=2 + log-clamp +
    // center=False + periodic Hann). The env-var injection is a debug knob
    // for tensor-parity isolation.
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
        if (auto st = gm->mel.compute(pcm, static_cast<size_t>(n_samples), gc->mel_buf, mel_n_frames);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }
    gc->t_mel_us = ggml_time_us() - t_mel_start;

    {
        const long long mel_shape[2] = { gm->hparams.fe_num_mels, mel_n_frames };
        transcribe::debug::dump_host_f32("frontend.mel.out", gc->mel_buf.data(),
                                         static_cast<long long>(gc->mel_buf.size()), mel_shape, 2, "frontend.mel");
    }

    if (mel_n_frames <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Reset per-call compute state.
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
                                "gigaam run: compute context allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // Build encoder graph.
    EncoderBuild eb = build_encoder_graph(gc->compute_ctx, gm->weights, gm->hparams, mel_n_frames,
                                          /*kv_type=*/GGML_TYPE_F32, gm->backend.c_str());
    if (eb.mel_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Scheduler.
    if (gc->sched == nullptr) {
        gc->sched = ggml_backend_sched_new(gm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(gm->plan.scheduler_list.size()),
                                           /*graph_size=*/8192, /*parallel=*/false, /*op_offload=*/true);
        if (gc->sched == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "gigaam run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(gc->sched);
    if (!ggml_backend_sched_alloc_graph(gc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "gigaam run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, gc->mel_buf.data(), 0, gc->mel_buf.size() * sizeof(float));
    transcribe::debug::dump_tensor("enc.mel.in", eb.mel_in, "encoder.mel");

    // Upload positions [0, 1, ..., T_enc-1] as int32.
    if (eb.dumps.pos_emb != nullptr) {
        const int64_t        T_enc = eb.dumps.pos_emb->ne[0];
        std::vector<int32_t> pos(T_enc);
        for (int64_t i = 0; i < T_enc; ++i) {
            pos[i] = static_cast<int32_t>(i);
        }
        ggml_backend_tensor_set(eb.dumps.pos_emb, pos.data(), 0, pos.size() * sizeof(int32_t));
    }

    // Compute.
    const int64_t t_enc_start = ggml_time_us();
    if (ggml_backend_sched_graph_compute(gc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: graph_compute failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    gc->t_encode_us = ggml_time_us() - t_enc_start;

    // Dump intermediates.
    if (eb.dumps.pre_encode_out) {
        transcribe::debug::dump_tensor("enc.subsample.out", eb.dumps.pre_encode_out, "encoder.subsample");
    }
    if (eb.dumps.block0_after_attn) {
        transcribe::debug::dump_tensor("enc.block.0.after_attn", eb.dumps.block0_after_attn, "encoder.block.0");
    }
    if (eb.dumps.block0_after_conv) {
        transcribe::debug::dump_tensor("enc.block.0.after_conv", eb.dumps.block0_after_conv, "encoder.block.0");
    }
    if (eb.dumps.block0_out) {
        transcribe::debug::dump_tensor("enc.block.0.out", eb.dumps.block0_out, "encoder.block.0");
    }
    if (eb.dumps.mid_block_out && eb.dumps.mid_block_idx >= 0) {
        char nm[32];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", eb.dumps.mid_block_idx);
        transcribe::debug::dump_tensor(nm, eb.dumps.mid_block_out, "encoder.block.mid");
    }
    if (eb.dumps.last_block_out && eb.dumps.last_block_idx >= 0) {
        char nm[32];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", eb.dumps.last_block_idx);
        transcribe::debug::dump_tensor(nm, eb.dumps.last_block_out, "encoder.block.last");
    }
    if (eb.dumps.final_out) {
        transcribe::debug::dump_tensor("enc.out", eb.dumps.final_out, "encoder.out");
    }

    gc->encoder_out = eb.out;

    // Decoder (RNN-T or CTC greedy). Both heads consume the
    // pre-final-transpose encoder tensor (ne=[d_model, T_enc, 1]), named
    // `rnnt.encoded` on the graph. CTC variants skip the `rnnt.encoded` dump
    // (the reference dumper does not emit it for CTC).
    {
        ggml_tensor * enc_t = eb.dumps.rnnt_encoded;
        if (enc_t == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam: rnnt.encoded missing");
            return TRANSCRIBE_ERR_GGUF;
        }
        const int T_enc = static_cast<int>(enc_t->ne[1]);
        const int D     = static_cast<int>(enc_t->ne[0]);
        gc->enc_host.assign(static_cast<size_t>(T_enc) * D, 0.0f);
        ggml_backend_tensor_get(enc_t, gc->enc_host.data(), 0, gc->enc_host.size() * sizeof(float));

        if (gm->hparams.head_kind == HeadKind::RNNT) {
            const long long enc_shape[2] = { T_enc, D };
            transcribe::debug::dump_host_f32("rnnt.encoded", gc->enc_host.data(),
                                             static_cast<long long>(gc->enc_host.size()), enc_shape, 2,
                                             "decoder.rnnt.encoded");
        }

        if (auto st = decode_and_populate(gc, gm, gc->enc_host.data(), T_enc, D, /*utt_index=*/-1);
            st != TRANSCRIBE_OK) {
            return st;
        }
    }

    return TRANSCRIBE_OK;
}

// Offline batch (transcribe_run_batch): B utterances through ONE Conformer
// encoder dispatch (batch on the activation's ne[2] axis), then host-decode
// each utterance's RNN-T / CTC slice. Same-length batches run mask-free
// (bit-identical to single-shot per utterance); variable-length batches pad
// to T_max and apply per-utterance masks. Full-read encoder output +
// host-slice (NOT per-utt offset reads, which are unreliable across backends).

// One stride-2 conv with symmetric (k-1)/2 padding (matches encoder.cpp).
static int batch_pre_encode_t_out(int in) {
    return (in - 1) / 2 + 1;
}

transcribe_status run_batch_encode(GigaamSession *                         gc,
                                   GigaamModel *                           gm,
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

    // Pack mels into [T_max, n_mels, n], zero-padding each along time
    // (channel-major source [n_mels, nf[b]] -> per-utterance [T_max, n_mels]).
    transcribe::pack_pad_channel_major(gc->mel_buf, mels, nf, n_mels, T_max);

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
                                "gigaam run: compute context allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    EncoderBuild eb = build_encoder_graph(gc->compute_ctx, gm->weights, gm->hparams, T_max,
                                          /*kv_type=*/GGML_TYPE_F32, gm->backend.c_str(),
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
                                "gigaam run: scheduler allocation failed — out of memory. "
                                "Split long audio into shorter segments (see "
                                "transcribe_capabilities.max_audio_ms).");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ggml_backend_sched_reset(gc->sched);
    if (!ggml_backend_sched_alloc_graph(gc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "gigaam run: encoder graph allocation failed — out of memory. "
                            "Split long audio into shorter segments (see "
                            "transcribe_capabilities.max_audio_ms).");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, gc->mel_buf.data(), 0, gc->mel_buf.size() * sizeof(float));

    ggml_tensor * enc_t = eb.dumps.rnnt_encoded;  // [d_enc, T_enc, n]
    if (enc_t == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    const int d_enc = static_cast<int>(enc_t->ne[0]);
    const int T_enc = static_cast<int>(enc_t->ne[1]);
    if (d_enc <= 0 || T_enc <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Per-utterance valid encoder-frame count (two stride-2 pre-encode convs).
    std::vector<int> real_tenc(static_cast<size_t>(n), T_enc);
    if (var_len) {
        for (int b = 0; b < n; ++b) {
            int t        = batch_pre_encode_t_out(nf[b]);
            t            = batch_pre_encode_t_out(t);
            real_tenc[b] = std::min(t, T_enc);
        }
        // Attention key-padding mask [T_enc, 1, 1, n] (0 real / -INF padded)
        // and conv valid-frame mask [T_enc, 1, n, 1] (1 real / 0 padded).
        transcribe::fill_keypad_mask(eb.attn_pad_mask_in, real_tenc, T_enc, n);
        transcribe::fill_valid_frame_mask(eb.conv_pad_mask_in, real_tenc, T_enc, n);
        // Pre-encode masked-subsampling masks [T_stage, 1, n]: one per
        // conv-relu stage, valid time = mel length downsampled k times.
        auto fill_pe_mask = [&](ggml_tensor * mask, int n_down) {
            if (mask == nullptr) {
                return;
            }
            const int          H = static_cast<int>(mask->ne[0]);
            std::vector<float> mb(static_cast<size_t>(H) * n, 0.0f);
            for (int b = 0; b < n; ++b) {
                int v = nf[b];
                for (int d = 0; d < n_down; ++d) {
                    v = batch_pre_encode_t_out(v);
                }
                if (v > H) {
                    v = H;
                }
                for (int t = 0; t < v; ++t) {
                    mb[static_cast<size_t>(b) * H + t] = 1.0f;
                }
            }
            ggml_backend_tensor_set(mask, mb.data(), 0, mb.size() * sizeof(float));
        };
        fill_pe_mask(eb.pre_encode_mask_s1_in, 1);  // after conv0 relu
        fill_pe_mask(eb.pre_encode_mask_s2_in, 2);  // after conv2 relu
    }

    // Rotary positions [0, 1, ..., T_enc-1] (batch-independent).
    if (eb.dumps.pos_emb != nullptr) {
        std::vector<int32_t> pos(static_cast<size_t>(T_enc));
        for (int i = 0; i < T_enc; ++i) {
            pos[i] = i;
        }
        ggml_backend_tensor_set(eb.dumps.pos_emb, pos.data(), 0, pos.size() * sizeof(int32_t));
    }

    // Set the per-backend thread count (mirrors single-shot through the
    // scheduler; the encoder is the dominant cost on CPU).
    transcribe::configure_sched_n_threads(gc->sched, gc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    if (ggml_backend_sched_graph_compute(gc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam run_batch: graph_compute failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    gc->t_encode_us = ggml_time_us() - t_enc_start;

    // Debug-only full [d, T, n] intermediates, gated on the env var.
    if (transcribe::debug::enabled() && transcribe::debug::dump_all_blocks_requested()) {
        if (eb.dumps.pre_encode_out != nullptr) {
            transcribe::debug::dump_tensor("enc.subsample.out", eb.dumps.pre_encode_out, "encoder.subsample");
        }
        if (eb.dumps.block0_after_attn != nullptr) {
            transcribe::debug::dump_tensor("enc.block.0.after_attn", eb.dumps.block0_after_attn, "encoder.block.0");
        }
        if (eb.dumps.block0_after_conv != nullptr) {
            transcribe::debug::dump_tensor("enc.block.0.after_conv", eb.dumps.block0_after_conv, "encoder.block.0");
        }
        for (size_t i = 0; i < eb.dumps.all_block_outs.size(); ++i) {
            ggml_tensor * t = eb.dumps.all_block_outs[i];
            if (t == nullptr) {
                continue;
            }
            char nm[64];
            std::snprintf(nm, sizeof(nm), "enc.block.%zu.out", i);
            transcribe::debug::dump_tensor(nm, t, "encoder.block.bisect");
        }
    }

    // Full-read the encoder output, then host-slice per utterance (non-zero
    // offset reads are unreliable across backends).
    const size_t utt_elems = static_cast<size_t>(d_enc) * static_cast<size_t>(T_enc);
    gc->enc_host.assign(utt_elems * static_cast<size_t>(n), 0.0f);
    ggml_backend_tensor_get(enc_t, gc->enc_host.data(), 0, gc->enc_host.size() * sizeof(float));

    // Host-slice the shared encoder output and decode each utterance, with the
    // single shared encode + total mel cost amortized across the batch.
    return transcribe::decode_batch_slices(gc, n, gc->enc_host.data(), utt_elems, gc->t_encode_us, total_mel_us,
                                           [&](int b, const float * enc_b) {
                                               return decode_and_populate(gc, gm, enc_b, real_tenc[b], d_enc,
                                                                          /*utt_index=*/b);
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
    auto * gc = static_cast<GigaamSession *>(session);
    auto * gm = static_cast<GigaamModel *>(gc->model);
    if (gm == nullptr || gm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    transcribe::debug::init();

    // Compute each utterance's mel in parallel (pure host code, no
    // cross-utterance state). A malformed/failed utterance drops the whole
    // call to the per-utterance fallback so the batch tensor stays
    // rectangular and the masks stay well-defined.
    const int                       n_mels = gm->hparams.fe_num_mels;
    std::vector<std::vector<float>> mels(static_cast<size_t>(n));
    std::vector<int>                nf(static_cast<size_t>(n), 0);
    const int64_t                   t_mel_start  = ggml_time_us();
    const bool                      all_ok       = transcribe::parallel_for_all(n, gc->n_threads, [&](int i) -> bool {
        if (pcm[i] == nullptr || n_samples[i] <= 0) {
            return false;
        }
        int                     this_frames = 0;
        const transcribe_status st = gm->mel.compute(pcm[i], static_cast<size_t>(n_samples[i]), mels[i], this_frames);
        if (st != TRANSCRIBE_OK || this_frames <= 0) {
            return false;
        }
        nf[i] = this_frames;
        return true;
    });
    const int64_t                   total_mel_us = ggml_time_us() - t_mel_start;

    if (all_ok) {
        // Per-utterance soft-window advisory before the shared encode; the
        // fallback path re-enters run(), which warns there, so each
        // over-length clip is warned about exactly once.
        const int sr = gm->hparams.fe_sample_rate;
        if (sr > 0) {
            for (int i = 0; i < n; ++i) {
                const int64_t audio_ms = static_cast<int64_t>(n_samples[i]) * 1000 / sr;
                if (audio_ms > k_safe_audio_ms) {
                    const double seconds = static_cast<double>(audio_ms) / 1000.0;
                    transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                        "gigaam run: audio is %.1f s, beyond the ~%d s window "
                                        "this model was trained on; transcription may be "
                                        "degraded. Split long audio into <=%d s segments. See "
                                        "transcribe_capabilities.max_audio_ms.",
                                        seconds, k_safe_s, k_safe_s);
                }
            }
        }

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
    /*.name             =*/"gigaam",
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
};

}  // namespace transcribe::gigaam
