// arch/voxtral_realtime/model.cpp - Voxtral Realtime (2602) family handler.
//
// Streaming audio-LLM: causal RoPE sliding-window encoder -> projector ->
// Ministral LM with ADDITIVE audio fusion and delay-conditioned adaptive-norm.
// The prompt is [BOS] + [STREAMING_PAD]*(n_left_pad+num_delay); the projector
// audio embeddings are ADDED onto every sequence position. Greedy decode is
// clamped to ceil(mel_frames / audio_length_per_tok) positions.

#include "causal_lm/causal_lm.h"
#include "decoder.h"
#include "encoder.h"
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
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "transcribe/voxtral_realtime.h"  // public stream-ext surface
#include "voxtral_realtime.h"
#include "weights.h"

#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <unordered_map>
#include <vector>

namespace transcribe::voxtral_realtime {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, Model>);
static_assert(std::is_base_of_v<transcribe_session, Session>);

Session::~Session() {
    kv_cache.free();
    kv_cache_batch.free();
    enc_kv.free();
    ada_scale.clear();
    if (ada_buffer != nullptr) {
        safe_buffer_free(ada_buffer);
        ada_buffer = nullptr;
    }
    if (ada_ctx != nullptr) {
        ggml_free(ada_ctx);
        ada_ctx = nullptr;
    }
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

Model::~Model() {
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

constexpr const char k_default_variant[] = "voxtral-mini-4b-realtime-2602";

// Resolve BOS / STREAMING_PAD / EOS against the loaded tokenizer.
transcribe_status resolve_specials(const transcribe::Tokenizer & tok, const HParams & hp, PromptSpecials & out) {
    out.bos           = tok.bos_id();
    out.eos           = tok.eos_id();
    out.streaming_pad = hp.streaming_pad_token_id;
    out.n_left_pad    = 32;  // tekken streaming_n_left_pad_tokens
    if (out.bos < 0 || out.eos < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: tokenizer missing bos/eos id");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (out.streaming_pad < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: invalid streaming_pad token id");
        return TRANSCRIBE_ERR_GGUF;
    }
    return TRANSCRIBE_OK;
}

void apply_threads(ggml_backend_sched_t sched, int n_threads) {
    transcribe::configure_sched_n_threads(sched, n_threads);
}

#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
// Read a reference enc.mel.in.f32 ([n_mels, n_frames] mel-major) for numerical
// isolation. Returns true and fills mel_buf (mel-major) + n_frames on success.
bool read_ref_mel(const std::string & dir, int n_mels, std::vector<float> & mel_buf, int & n_frames) {
    const std::string path = dir + "/enc.mel.in.f32";
    FILE *            f    = std::fopen(path.c_str(), "rb");
    if (f == nullptr) {
        return false;
    }
    std::fseek(f, 0, SEEK_END);
    const long bytes = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    const long n = bytes / static_cast<long>(sizeof(float));
    if (n <= 0 || (n % n_mels) != 0) {
        std::fclose(f);
        return false;
    }
    mel_buf.resize(static_cast<size_t>(n));
    const size_t got = std::fread(mel_buf.data(), sizeof(float), static_cast<size_t>(n), f);
    std::fclose(f);
    if (got != static_cast<size_t>(n)) {
        return false;
    }
    n_frames = static_cast<int>(n / n_mels);
    return true;
}
#endif  // TRANSCRIBE_ENABLE_VALIDATION_HOOKS

// Input-length contract (see docs/input-limits.md). Chunked/unbounded family:
// the encoder is causal + sliding-window (750 frames) advanced on a
// constant-memory ring, and the decoder KV is itself a sliding-window ring
// (dec_sliding_window = 8192). The only hard wall is the dec_max_position
// (131072) RoPE cap; at the 80 ms/token grid (audio_length_per_tok=8 mel
// frames, hop 1280 @ 16 kHz) that is ~2.9 h. n_ctx does not lower it.

// Absolute decoder position cap, in tokens (dec_max_position). Past this the
// decoder's RoPE positions alias, so every decode path hard-stops here. Not
// lowered by the session n_ctx knob (chunked/unbounded family; the decoder KV
// slides). See docs/input-limits.md.
int voxtral_realtime_abs_position_cap(const HParams & hp) {
    return hp.dec_max_position;
}

// Advisory transcribe_capabilities::max_audio_ms. Returns 0 (unbounded): the
// decoder KV slides and the encoder is causal/streaming, so the only reject is
// the model's absolute position wall (~2.9 h), handled as a safety cap.
int64_t voxtral_realtime_max_audio_ms(const HParams & hp) {
    (void) hp;
    return 0;
}

}  // namespace

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<Model>();
    m->arch      = &arch;
    m->t_load_us = 0;
    m->variant   = loader.variant().empty() ? k_default_variant : loader.variant();
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
    if (auto st = read_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window is
    // known (apply_family_invariants ran before hparams). Chunked/unbounded:
    // 0 = no practical limit. See docs/input-limits.md.
    m->caps.max_audio_ms = voxtral_realtime_max_audio_ms(m->hparams);

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();
    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (auto st = resolve_specials(m->tok, m->hparams, m->specials); st != TRANSCRIBE_OK) {
        return st;
    }

    // Mel frontend (streaming log-mel, fixed global max).
    {
        transcribe::MelConfig cfg{};
        cfg.sample_rate        = m->hparams.fe_sample_rate;
        cfg.num_mels           = m->hparams.fe_num_mels;
        cfg.n_fft              = m->hparams.fe_n_fft;
        cfg.win_length         = m->hparams.fe_win_length;
        cfg.hop_length         = m->hparams.fe_hop_length;
        cfg.pre_emphasis       = m->hparams.fe_pre_emphasis;
        cfg.f_min              = m->hparams.fe_f_min;
        cfg.f_max              = m->hparams.fe_f_max;
        cfg.pad_mode           = m->hparams.fe_pad_mode;
        cfg.window_type        = m->hparams.fe_window;     // "hann_periodic"
        cfg.normalize          = m->hparams.fe_normalize;  // "global"
        cfg.global_log_mel_max = m->hparams.fe_global_log_mel_max;

        using R               = transcribe::load_common::ReadF32Result;
        const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
        const auto   fb_rc    = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "voxtral_realtime", cfg.filterbank);
        if (fb_rc != R::Ok && fb_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }
        const auto win_rc = transcribe::load_common::read_f32_tensor_checked(
            loader.gguf(), loader.path(), "frontend.window", static_cast<size_t>(cfg.win_length), "voxtral_realtime",
            cfg.window);
        if (win_rc != R::Ok && win_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }

        m->mel.emplace(cfg);
    }

    // Tensor catalog + backend alloc + data stream.
    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "voxtral_realtime", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "voxtral_realtime");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Pack gate+up for the DECODER (the causal_lm block uses ffn_gate_up_w). The
    // encoder FFN runs unpacked (one-shot, not the hot path).
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "voxtral_realtime")) {
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

    auto cc       = std::make_unique<Session>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    // Captured for base-class uniformity only: this family ignores n_ctx (the
    // decoder KV ring is constant-memory; the wall is dec_max_position).
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    auto * cm = static_cast<Model *>(model);

    // Encoder + decoder flash attention ON by default on every backend. Flash is
    // the numerical source-of-truth for the encoder. Override TRANSCRIBE_NO_FLASH=1.
    cc->encoder_use_flash = true;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, /*n_ctx=*/2048, cm->hparams.dec_n_kv_heads,
                                        cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kv_type)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime init_context: KV cache allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Precompute the per-layer adaptive-norm FFN scale s_l = 1 + ada(t_cond) for
// the active num_delay_tokens. t_cond = [cos(d*inv_freq), sin(d*inv_freq)];
// ada_l = linear2(gelu(linear1(t_cond))). Result stored in ada_scale_all
// [dec_hidden, n_layers] (F32, persistent in the session).
transcribe_status compute_ada_scales(Session * cc, Model * cm, int num_delay) {
    if (cc->ada_scale_all != nullptr && cc->ada_num_delay == num_delay) {
        return TRANSCRIBE_OK;
    }

    const int hidden  = cm->hparams.dec_hidden;
    const int n_layer = cm->hparams.dec_n_layers;
    const int half    = cm->hparams.time_embed_dim / 2;

    // inv_freq -> host -> t_cond.
    std::vector<float> inv_freq(static_cast<size_t>(half));
    ggml_backend_tensor_get(cm->weights.time_inv_freq, inv_freq.data(), 0, inv_freq.size() * sizeof(float));
    std::vector<float> t_cond(static_cast<size_t>(hidden));
    for (int i = 0; i < half; ++i) {
        const double e   = static_cast<double>(num_delay) * static_cast<double>(inv_freq[i]);
        t_cond[i]        = static_cast<float>(std::cos(e));
        t_cond[half + i] = static_cast<float>(std::sin(e));
    }

    // Persistent ada_scale_all.
    if (cc->ada_buffer != nullptr) {
        safe_buffer_free(cc->ada_buffer);
        cc->ada_buffer = nullptr;
    }
    if (cc->ada_ctx != nullptr) {
        ggml_free(cc->ada_ctx);
        cc->ada_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size = ggml_tensor_overhead() * 4;
        ip.no_alloc = true;
        cc->ada_ctx = ggml_init(ip);
        if (cc->ada_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime: ada-scale context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    cc->ada_scale_all = ggml_new_tensor_2d(cc->ada_ctx, GGML_TYPE_F32, hidden, n_layer);
    ggml_set_name(cc->ada_scale_all, "ada.scale_all");
    cc->ada_buffer = ggml_backend_alloc_ctx_tensors(cc->ada_ctx, cm->plan.primary);
    if (cc->ada_buffer == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // Build a one-shot compute graph: per layer ada_l = linear2(gelu(linear1(t))).
    ggml_init_params cip{};
    cip.mem_size       = 16 * 1024 * 1024;
    cip.no_alloc       = true;
    ggml_context * ctx = ggml_init(cip);
    if (ctx == nullptr) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime: ada-scale compute context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_tensor * t_in = ggml_new_tensor_1d(ctx, GGML_TYPE_F32, hidden);
    ggml_set_input(t_in);
    ggml_tensor * acc = nullptr;
    for (int il = 0; il < n_layer; ++il) {
        const DecBlock & b = cm->weights.dec_blocks[il];
        ggml_tensor *    a = ggml_mul_mat(ctx, b.ada_linear_1_w, t_in);  // [ada_hidden]
        a                  = ggml_gelu_erf(ctx, a);
        a                  = ggml_mul_mat(ctx, b.ada_linear_2_w, a);     // [hidden]
        a                  = ggml_reshape_2d(ctx, a, hidden, 1);
        acc                = (acc == nullptr) ? a : ggml_concat(ctx, acc, a, /*dim=*/1);
    }
    ggml_set_output(acc);
    ggml_cgraph * gf = ggml_new_graph(ctx);
    ggml_build_forward_expand(gf, acc);

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            ggml_free(ctx);
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, gf)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime: ada-scale graph allocation failed — "
                            "out of memory.");
        ggml_free(ctx);
        return TRANSCRIBE_ERR_OOM;
    }
    ggml_backend_tensor_set(t_in, t_cond.data(), 0, t_cond.size() * sizeof(float));
    apply_threads(cc->sched, cc->n_threads);
    if (ggml_backend_sched_graph_compute(cc->sched, gf) != GGML_STATUS_SUCCESS) {
        ggml_free(ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    std::vector<float> ada(static_cast<size_t>(hidden) * n_layer);
    ggml_backend_tensor_get(acc, ada.data(), 0, ada.size() * sizeof(float));
    ggml_free(ctx);
    for (auto & v : ada) {
        v += 1.0f;  // s_l = 1 + ada
    }
    // Fold the per-layer FFN-norm weight into the ada scale and store
    // (s_l ⊙ norm_ffn_w): the decode block passes this as the FFN-norm weight so
    // the fused rms_norm(·weight) kernel applies the ada scale with no separate
    // ggml_mul. Exact: s⊙(w⊙x̂) == (s⊙w)⊙x̂.
    {
        std::vector<float> wbuf(static_cast<size_t>(hidden));
        for (int il = 0; il < n_layer; ++il) {
            ggml_backend_tensor_get(cm->weights.dec_blocks[il].norm_ffn_w, wbuf.data(), 0, wbuf.size() * sizeof(float));
            float * s = ada.data() + static_cast<size_t>(il) * hidden;
            for (int i = 0; i < hidden; ++i) {
                s[i] *= wbuf[i];
            }
        }
    }
    ggml_backend_tensor_set(cc->ada_scale_all, ada.data(), 0, ada.size() * sizeof(float));

    cc->ada_num_delay = num_delay;
    return TRANSCRIBE_OK;
}

// Core forward: mel -> encoder/projector -> autoregressive decode -> detok.
// Shared by the offline run() and the streaming hooks. `num_delay` drives the
// audio right-pad, the prompt length, and the adaptive-norm scales (they MUST
// agree). On success out_text holds the trimmed transcript. Does not touch the
// result snapshot (segments / full_text / has_result) — the caller owns that.
transcribe_status forward_buffer(Session *     cc,
                                 Model *       cm,
                                 const float * pcm,
                                 int           n_samples,
                                 int           num_delay,
                                 bool          dumps_on,
                                 int           k_drafts,
                                 std::string & out_text) {
    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // ----- Mel (or reference-injected mel for numerical isolation) -----
    const int     n_mels       = cm->hparams.enc_num_mel_bins;
    int           mel_n_frames = 0;
    bool          mel_from_ref = false;
    const int64_t t_mel_start  = ggml_time_us();
#ifdef TRANSCRIBE_ENABLE_VALIDATION_HOOKS
    if (const char * ref_mel_dir = transcribe::env::str("TRANSCRIBE_MEL_FROM_REF")) {
        if (!read_ref_mel(ref_mel_dir, n_mels, cc->mel_buf, mel_n_frames)) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: failed to read ref mel from %s", ref_mel_dir);
            return TRANSCRIBE_ERR_GGUF;
        }
        mel_from_ref = true;
    }
#endif
    if (!mel_from_ref) {
        // Streaming offline audio padding (mistral-common encode_transcription,
        // StreamingMode.OFFLINE): pad the audio into the 12.5 Hz token grid as
        //   [n_left_pad zeros] + [raw rounded up to raw_per_tok] + [n_right zeros]
        // raw_per_tok = audio_length_per_tok * hop = 1280 (= sr / frame_rate);
        // n_right = num_delay + 1 (BOS) + OFFLINE_STREAMING_BUFFER_TOKENS(10).
        const int          raw_per_tok = cm->hparams.audio_length_per_tok * cm->hparams.fe_hop_length;
        const int          n_left_tok  = cm->specials.n_left_pad;
        const int          n_right_tok = num_delay + 1 + 10;
        const size_t       left        = static_cast<size_t>(n_left_tok) * raw_per_tok;
        const size_t       right       = static_cast<size_t>(n_right_tok) * raw_per_tok;
        const size_t       raw_pad = ((static_cast<size_t>(n_samples) + raw_per_tok - 1) / raw_per_tok) * raw_per_tok;
        const size_t       total   = left + raw_pad + right;
        std::vector<float> padded(total, 0.0f);
        std::memcpy(padded.data() + left, pcm, static_cast<size_t>(n_samples) * sizeof(float));

        int mm = 0;
        if (auto st = cm->mel->compute(padded.data(), total, cc->mel_buf, mm, mel_n_frames, cc->n_threads);
            st != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: mel compute failed (%s)",
                    transcribe_status_string(st));
            return st;
        }
        if (mm != n_mels) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: mel bins %d != %d", mm, n_mels);
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    if (dumps_on) {
        const long long shape[2] = { n_mels, mel_n_frames };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "frontend.mel");
    }

    // ----- Encoder + projector (whole clip) -----
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    const int     dec_h       = cm->hparams.dec_hidden;
    int           n_audio     = 0;
    const int64_t t_enc_start = ggml_time_us();
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        EncoderBuild eb =
            build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, mel_n_frames, cc->encoder_use_flash);
        if (eb.graph == nullptr || eb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        n_audio = eb.n_audio;

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run: encoder graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        // mel_buf is mel-major [n_mels, n_frames]; the encoder input is
        // frame-major [n_mels, n_mel_frames] (ne[0]=n_mels innermost).
        std::vector<float> mel_frame_major(static_cast<size_t>(n_mels) * mel_n_frames);
        for (int t = 0; t < mel_n_frames; ++t) {
            for (int mm = 0; mm < n_mels; ++mm) {
                mel_frame_major[static_cast<size_t>(t) * n_mels + mm] =
                    cc->mel_buf[static_cast<size_t>(mm) * mel_n_frames + t];
            }
        }
        ggml_backend_tensor_set(eb.mel_in, mel_frame_major.data(), 0, mel_frame_major.size() * sizeof(float));

        std::vector<int32_t> positions(eb.T_enc);
        for (int i = 0; i < eb.T_enc; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(eb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));

        // Sliding-window-causal mask [T_enc(key), T_enc(query)] f16.
        {
            const int                T = eb.T_enc, win = cm->hparams.enc_sliding_window;
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(T) * T, mn);
            for (int q = 0; q < T; ++q) {
                for (int k = 0; k <= q; ++k) {
                    if (q - k < win) {
                        mask[static_cast<size_t>(q) * T + k] = mz;
                    }
                }
            }
            ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        apply_threads(cc->sched, cc->n_threads);
        if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: encoder compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        if (dumps_on) {
            for (size_t i = 0; i < eb.dumps.block_outs.size(); ++i) {
                char nm[64];
                std::snprintf(nm, sizeof(nm), "enc.block.%zu.out", i);
                transcribe::debug::dump_tensor(nm, eb.dumps.block_outs[i], "enc.block");
            }
            if (eb.dumps.embedder_out) {
                transcribe::debug::dump_tensor("enc.embedder.out", eb.dumps.embedder_out, "enc");
            }
            if (eb.dumps.enc_out) {
                transcribe::debug::dump_tensor("enc.out", eb.dumps.enc_out, "enc");
            }
            if (eb.dumps.proj_out) {
                transcribe::debug::dump_tensor("proj.out", eb.dumps.proj_out, "proj");
            }
        }

        cc->enc_host.assign(static_cast<size_t>(dec_h) * n_audio, 0.0f);
        ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    // ----- Adaptive-norm scales for the active delay -----
    if (auto st = compute_ada_scales(cc, cm, num_delay); st != TRANSCRIBE_OK) {
        return st;
    }

    // ----- Prompt: [BOS] + [STREAMING_PAD]*(n_left_pad + num_delay) -----
    const PromptSpecials & sp = cm->specials;
    std::vector<int32_t>   prompt_ids;
    prompt_ids.push_back(sp.bos);
    const int n_pad = sp.n_left_pad + num_delay;
    for (int i = 0; i < n_pad; ++i) {
        prompt_ids.push_back(sp.streaming_pad);
    }
    const int T_prompt = static_cast<int>(prompt_ids.size());
    if (T_prompt >= n_audio) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime run: prompt %d >= n_audio %d (clip too short)", T_prompt, n_audio);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // RoPE is valid only up to dec_max_position and a clip needs one KV slot per
    // audio token, so a horizon past the cap cannot be decoded — reject up front
    // with INPUT_TOO_LONG (defensive: ~2.9 h of audio OOMs the KV first).
    const int abs_cap = voxtral_realtime_abs_position_cap(cm->hparams);
    if (n_audio + 1 > abs_cap) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime run: clip needs %d positions > model max %d "
                            "(dec_max_position, ~2.9 h). See transcribe_capabilities.max_audio_ms.",
                            n_audio + 1, abs_cap);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // Grow KV to fit n_audio positions.
    if (cc->kv_cache.n_ctx < n_audio + 1) {
        const ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
        int             want    = 2048;
        while (want < n_audio + 1) {
            want *= 2;
        }
        cc->kv_cache.free();
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, want, cm->hparams.dec_n_kv_heads,
                                            cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run: KV cache allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    const int     vocab  = cm->hparams.dec_vocab_size;
    const int32_t eos_id = cm->hparams.eos_token_id;
    auto          argmax = [](const std::vector<float> & v) -> int32_t {
        int32_t best = 0;
        float   bv   = v[0];
        for (int32_t i = 1; i < static_cast<int32_t>(v.size()); ++i) {
            if (v[i] > bv) {
                bv   = v[i];
                best = i;
            }
        }
        return best;
    };

    // ----- Autoregressive generation (prompt prefill + steps) -----
    const int64_t t_dec_start = ggml_time_us();
    if (cc->kv_cache.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
    }
    cc->kv_cache.n    = 0;
    cc->kv_cache.head = 0;

    std::vector<int32_t> generated_ids;
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                              cc->ada_scale_all, T_prompt, cc->decoder_use_flash,
                                              /*want_all_logits=*/false);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run: prefill graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
        ggml_backend_tensor_set(pb.audio_in, cc->enc_host.data(), 0,
                                static_cast<size_t>(dec_h) * T_prompt * sizeof(float));
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
        {
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, mn);
            for (int r = 0; r < T_prompt; ++r) {
                for (int c = 0; c <= r; ++c) {
                    mask[static_cast<size_t>(r) * T_prompt + c] = mz;
                }
            }
            ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        apply_threads(cc->sched, cc->n_threads);
        if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: prefill compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.n    = T_prompt;
        cc->kv_cache.head = T_prompt;

        std::vector<float> logits(vocab);
        ggml_backend_tensor_get(pb.out, logits.data(), 0, logits.size() * sizeof(float));
        generated_ids.push_back(argmax(logits));
    }

    // Steps: 1-gram-lookup speculative decode (k_drafts > 0; mechanism in
    // build_verify_graph) or plain autoregression (k_drafts == 0). For both, the
    // attention read window is shrunk from kv_cache.n_ctx to n_audio (the actual
    // clip horizon): the dropped slots [n_audio, n_ctx) were always -inf-masked,
    // so the shrink is bit-identical and saves KV bandwidth on short clips.
    {
        const int max_n_kv = n_audio;

        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 128 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        const ggml_fp16_t mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);

        int32_t next_tok = generated_ids.back();
        int     cur_pos  = T_prompt;

        if (k_drafts == 0) {
            // ---- Plain autoregression (zero spec overhead) ----
            StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache, cc->ada_scale_all,
                                            max_n_kv, cc->decoder_use_flash);
            if (sb.graph == nullptr || sb.out == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "voxtral_realtime run: step graph allocation failed — "
                                    "out of memory.");
                return TRANSCRIBE_ERR_OOM;
            }
            apply_threads(cc->sched, cc->n_threads);

            std::vector<ggml_fp16_t> step_mask(max_n_kv, mn);
            while (next_tok != eos_id && cur_pos < n_audio) {
                if (cc->poll_abort()) {
                    return TRANSCRIBE_ERR_ABORTED;
                }
                ggml_backend_tensor_set(sb.input_id_in, &next_tok, 0, sizeof(int32_t));
                ggml_backend_tensor_set(sb.audio_in, cc->enc_host.data() + static_cast<size_t>(cur_pos) * dec_h, 0,
                                        static_cast<size_t>(dec_h) * sizeof(float));
                const int32_t pos_val = cur_pos;
                ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
                const int64_t kv_idx = cur_pos;
                ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx, 0, sizeof(int64_t));
                if (cur_pos == T_prompt) {
                    std::fill(step_mask.begin(), step_mask.begin() + cur_pos + 1, mz);
                } else {
                    step_mask[cur_pos] = mz;
                }
                ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0,
                                        static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));
                if (ggml_backend_sched_graph_compute(cc->sched, sb.graph) != GGML_STATUS_SUCCESS) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: step compute failed");
                    return TRANSCRIBE_ERR_GGUF;
                }
                int32_t tok = 0;
                ggml_backend_tensor_get(sb.out, &tok, 0, sizeof(int32_t));
                next_tok = tok;
                cur_pos += 1;
                cc->kv_cache.n    = cur_pos + 1;
                cc->kv_cache.head = cur_pos + 1;
                if (next_tok == eos_id) {
                    break;
                }
                generated_ids.push_back(next_tok);
            }
        } else {
            // ---- 1-gram-lookup speculative decode ----
            const int K_DRAFTS = k_drafts;
            const int T_verify = K_DRAFTS + 1;

            // Pad enc_host with K_DRAFTS extra zero frames so verify can safely
            // read audio for columns past n_audio in the tail iterations. The
            // garbage predictions at those columns are never committed.
            cc->enc_host.resize(static_cast<size_t>(dec_h) * (n_audio + K_DRAFTS), 0.0f);

            VerifyBuild vb = build_verify_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                                cc->ada_scale_all, T_verify, max_n_kv, cc->decoder_use_flash);
            if (vb.graph == nullptr || vb.out == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_sched_reset(cc->sched);
            if (!ggml_backend_sched_alloc_graph(cc->sched, vb.graph)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "voxtral_realtime run: verify graph allocation failed — "
                                    "out of memory.");
                return TRANSCRIBE_ERR_OOM;
            }
            apply_threads(cc->sched, cc->n_threads);

            // all_ids[p] = committed token at position p. last_pos_by_tok[t]
            // = latest position p with all_ids[p] == t (the 1-gram suffix
            // map). Initial state: prompt + first generated token from prefill.
            std::vector<int32_t> all_ids;
            all_ids.reserve(static_cast<size_t>(n_audio));
            all_ids.insert(all_ids.end(), prompt_ids.begin(), prompt_ids.end());
            all_ids.push_back(generated_ids.back());

            std::unordered_map<int32_t, int> last_pos_by_tok;
            last_pos_by_tok.reserve(static_cast<size_t>(n_audio));
            for (int p = 0; p < T_prompt; ++p) {
                last_pos_by_tok[prompt_ids[p]] = p;
            }

            std::vector<int32_t>     in_ids(T_verify, 0);
            std::vector<int32_t>     positions(T_verify, 0);
            std::vector<int64_t>     kv_idxs(T_verify, 0);
            std::vector<float>       audio_buf(static_cast<size_t>(dec_h) * T_verify);
            std::vector<ggml_fp16_t> verify_mask(static_cast<size_t>(max_n_kv) * T_verify, mn);
            std::vector<int32_t>     predicted(T_verify, 0);
            std::vector<int32_t>     draft(K_DRAFTS, 0);

            while (next_tok != eos_id && cur_pos < n_audio) {
                if (cc->poll_abort()) {
                    return TRANSCRIBE_ERR_ABORTED;
                }

                // Build draft via 1-gram suffix lookup. Fallback when no
                // match: K copies of STREAMING_PAD (id 32), the dominant
                // token in the stream — gives free acceptance during silence.
                int  draft_origin = -1;
                auto it           = last_pos_by_tok.find(next_tok);
                if (it != last_pos_by_tok.end()) {
                    draft_origin = it->second;
                }
                for (int k = 0; k < K_DRAFTS; ++k) {
                    const int src = (draft_origin >= 0) ? (draft_origin + 1 + k) : -1;
                    draft[k] =
                        (src >= 0 && src < static_cast<int>(all_ids.size())) ? all_ids[src] : 32 /* STREAMING_PAD */;
                }

                in_ids[0]    = next_tok;
                positions[0] = cur_pos;
                kv_idxs[0]   = cur_pos;
                for (int c = 1; c < T_verify; ++c) {
                    in_ids[c]    = draft[c - 1];
                    positions[c] = cur_pos + c;
                    kv_idxs[c]   = cur_pos + c;
                }
                std::memcpy(audio_buf.data(), cc->enc_host.data() + static_cast<size_t>(cur_pos) * dec_h,
                            static_cast<size_t>(dec_h) * T_verify * sizeof(float));

                // Per-column causal mask. Cheap to rebuild each iter
                // (max_n_kv × T_verify × 2 B, kilobytes-scale).
                std::fill(verify_mask.begin(), verify_mask.end(), mn);
                for (int c = 0; c < T_verify; ++c) {
                    const int last_valid = std::min(cur_pos + c, max_n_kv - 1);
                    for (int slot = 0; slot <= last_valid; ++slot) {
                        verify_mask[static_cast<size_t>(c) * max_n_kv + slot] = mz;
                    }
                }

                ggml_backend_tensor_set(vb.input_ids_in, in_ids.data(), 0,
                                        static_cast<size_t>(T_verify) * sizeof(int32_t));
                ggml_backend_tensor_set(vb.audio_in, audio_buf.data(), 0,
                                        static_cast<size_t>(dec_h) * T_verify * sizeof(float));
                ggml_backend_tensor_set(vb.positions_in, positions.data(), 0,
                                        static_cast<size_t>(T_verify) * sizeof(int32_t));
                ggml_backend_tensor_set(vb.kv_idx_in, kv_idxs.data(), 0,
                                        static_cast<size_t>(T_verify) * sizeof(int64_t));
                ggml_backend_tensor_set(vb.mask_in, verify_mask.data(), 0, verify_mask.size() * sizeof(ggml_fp16_t));

                if (ggml_backend_sched_graph_compute(cc->sched, vb.graph) != GGML_STATUS_SUCCESS) {
                    log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: verify compute failed");
                    return TRANSCRIBE_ERR_GGUF;
                }

                ggml_backend_tensor_get(vb.out, predicted.data(), 0, static_cast<size_t>(T_verify) * sizeof(int32_t));

                // Accept the longest matched prefix; cap so we never commit
                // a position past n_audio (matches the original step loop's
                // horizon).
                int n_accept = 0;
                while (n_accept < K_DRAFTS && predicted[n_accept] == draft[n_accept] &&
                       (cur_pos + 1 + (n_accept + 1)) <= n_audio) {
                    n_accept++;
                }

                const int prev_size = static_cast<int>(all_ids.size());
                bool      hit_eos   = false;
                for (int i = 0; i <= n_accept; ++i) {
                    const int32_t tok = predicted[i];
                    const int     pos = cur_pos + 1 + i;
                    if (pos > n_audio) {
                        break;
                    }
                    if (tok == eos_id) {
                        hit_eos = true;
                        break;
                    }
                    all_ids.push_back(tok);
                    generated_ids.push_back(tok);
                    last_pos_by_tok[tok] = pos;
                }

                // Pin next_tok at cur_pos (it was committed by a prior iter
                // / by prefill but isn't yet a "previous occurrence" key).
                last_pos_by_tok[next_tok] = cur_pos;

                // Forward-progress safety: if no token committed (e.g. eos
                // at predicted[0]) the outer loop condition would otherwise
                // spin on the same cur_pos.
                if (static_cast<int>(all_ids.size()) == prev_size) {
                    if (!hit_eos) {
                        log_msg(TRANSCRIBE_LOG_LEVEL_INFO,
                                "voxtral_realtime spec: no commit at cur_pos=%d "
                                "(n_audio=%d, predicted[0]=%d) — breaking",
                                cur_pos, n_audio, predicted[0]);
                    }
                    break;
                }

                cur_pos           = static_cast<int>(all_ids.size()) - 1;
                next_tok          = all_ids.back();
                cc->kv_cache.n    = cur_pos + 1;
                cc->kv_cache.head = cur_pos + 1;

                if (hit_eos) {
                    next_tok = eos_id;
                    break;
                }
            }
        }
    }
    cc->t_decode_us = ggml_time_us() - t_dec_start;

    // ----- Dump path: teacher-forced forward over the full n_audio sequence -----
    if (dumps_on) {
        std::vector<int32_t> full_ids = prompt_ids;
        full_ids.insert(full_ids.end(), generated_ids.begin(), generated_ids.end());
        const int pad_id = (cm->hparams.pad_token_id >= 0) ? cm->hparams.pad_token_id : 11;
        if (static_cast<int>(full_ids.size()) < n_audio) {
            full_ids.resize(n_audio, pad_id);
        } else {
            full_ids.resize(n_audio);
        }

        if (cc->kv_cache.buffer != nullptr) {
            ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        }
        cc->kv_cache.n    = 0;
        cc->kv_cache.head = 0;

        if (cc->compute_ctx != nullptr) {
            ggml_free(cc->compute_ctx);
            cc->compute_ctx = nullptr;
        }
        ggml_init_params ip{};
        ip.mem_size     = 256 * 1024 * 1024;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        PrefillBuild tf = build_prefill_graph(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache,
                                              cc->ada_scale_all, n_audio, cc->decoder_use_flash,
                                              /*want_all_logits=*/true);
        if (tf.graph == nullptr || tf.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, tf.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run: tail-forward graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        ggml_backend_tensor_set(tf.input_ids_in, full_ids.data(), 0, full_ids.size() * sizeof(int32_t));
        ggml_backend_tensor_set(tf.audio_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
        std::vector<int32_t> positions(n_audio);
        for (int i = 0; i < n_audio; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(tf.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
        {
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(n_audio) * n_audio, mn);
            for (int r = 0; r < n_audio; ++r) {
                for (int c = 0; c <= r; ++c) {
                    mask[static_cast<size_t>(r) * n_audio + c] = mz;
                }
            }
            ggml_backend_tensor_set(tf.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        apply_threads(cc->sched, cc->n_threads);
        if (ggml_backend_sched_graph_compute(cc->sched, tf.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run: teacher-forced dump compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        auto dump = [&](const char * n, ggml_tensor * t, const char * s) {
            if (t != nullptr) {
                transcribe::debug::dump_tensor(n, t, s);
            }
        };
        dump("dec.token_emb", tf.dumps.token_emb, "dec");
        dump("dec.audio_injected", tf.dumps.audio_injected, "dec");
        dump("dec.block.0.out", tf.dumps.block_0_out, "dec");
        {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers / 2);
            dump(nm, tf.dumps.block_mid_out, "dec");
        }
        {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", cm->hparams.dec_n_layers - 1);
            dump(nm, tf.dumps.block_last_out, "dec");
        }
        dump("dec.out_before_head", tf.dumps.out_before_head, "dec");

        // gen0 / gen8 logits: columns (T_prompt-1) and (T_prompt-1+8) of [vocab, n_audio].
        std::vector<float> logits_all(static_cast<size_t>(vocab) * n_audio);
        ggml_backend_tensor_get(tf.dumps.logits_all, logits_all.data(), 0, logits_all.size() * sizeof(float));
        const long long lshape[1] = { vocab };
        const int       g0 = T_prompt - 1, g8 = T_prompt - 1 + 8;
        if (g0 >= 0 && g0 < n_audio) {
            transcribe::debug::dump_host_f32("dec.logits_raw", logits_all.data() + static_cast<size_t>(g0) * vocab,
                                             vocab, lshape, 1, "dec");
        }
        if (g8 >= 0 && g8 < n_audio) {
            transcribe::debug::dump_host_f32("dec.logits_raw.gen8", logits_all.data() + static_cast<size_t>(g8) * vocab,
                                             vocab, lshape, 1, "dec");
        }
    }

    // ----- Decode transcript -----
    // Drop streaming control tokens ([STREAMING_PAD], [STREAMING_WORD], ...);
    // the model interleaves them with the byte-level word tokens (which carry
    // their own leading spaces). Mirrors mistral-common skip_special_tokens.
    std::vector<int32_t> text_ids;
    text_ids.reserve(generated_ids.size());
    for (int32_t id : generated_ids) {
        if (!cm->tok.is_control(id)) {
            text_ids.push_back(id);
        }
    }
    std::string text = cm->tok.decode(text_ids.data(), static_cast<int>(text_ids.size()));
    size_t      b = 0, e = text.size();
    while (b < e && std::isspace(static_cast<unsigned char>(text[b]))) {
        ++b;
    }
    while (e > b && std::isspace(static_cast<unsigned char>(text[e - 1]))) {
        --e;
    }
    text = text.substr(b, e - b);

    out_text = std::move(text);
    return TRANSCRIBE_OK;
}

// Offline one-shot entry point. Thin wrapper over forward_buffer that owns the
// result snapshot (full_text + a single text-only segment).
transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<Session *>(session);
    auto * cm = static_cast<Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();
    const bool dumps_on = transcribe::debug::enabled();

    // params->spec_k_drafts: -1 = family default (=1), 0 = disabled,
    // 1..VOXTRAL_REALTIME_SPEC_K_MAX = explicit. Clamp into range so a
    // misconfigured caller doesn't ask for an unbounded verify graph. Default
    // K=1 is the robust setting across backends (K>=2 costs long-form decode;
    // 1-gram acceptance is too low to amortize the T=K+1 verify graph). See
    // docs/models/voxtral-realtime.md.
    constexpr int VOXTRAL_REALTIME_SPEC_K_MAX = 8;
    int           k_drafts                    = 1;
    if (params != nullptr &&
        params->struct_size >= offsetof(transcribe_run_params, spec_k_drafts) + sizeof(params->spec_k_drafts)) {
        const int requested = params->spec_k_drafts;
        if (requested == 0) {
            k_drafts = 0;
        } else if (requested > 0) {
            k_drafts = std::min(requested, VOXTRAL_REALTIME_SPEC_K_MAX);
        }
        // requested == -1 keeps the family default; requested < -1 falls
        // through to the family default (matches the silent-ignore semantics).
    }

    std::string text;
    if (auto st =
            forward_buffer(cc, cm, pcm, n_samples, cm->hparams.default_num_delay_tokens, dumps_on, k_drafts, text);
        st != TRANSCRIBE_OK) {
        return st;
    }

    cc->full_text   = text;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;
    transcribe_session::SegmentEntry seg{};
    seg.text  = text;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
    cc->segments.push_back(std::move(seg));
    return TRANSCRIBE_OK;
}

// ---- Streaming -------------------------------------------------------------
// The encoder is causal + sliding-window and the mel uses a FIXED global
// normalization, so the projector audio embeddings are append-only and stable:
// a forward over the accumulated audio yields embeddings identical to the
// offline whole-clip forward. The incremental scheduler below (conv padding
// cache + encoder StaticCache ring + decoder sliding-KV) is therefore numerically
// identical to the offline path; stream_finalize matches transcribe_run.

constexpr int k_default_min_decode_interval_ms = 1000;

// Resolve the stream extension (delay + decode throttle). Reads only the
// caller-owned stream_params; mutates nothing.
transcribe_status resolve_stream_ext(const transcribe_stream_params * sp,
                                     const Model *                    cm,
                                     int *                            out_num_delay,
                                     int *                            out_min_decode_ms) {
    int                    num_delay     = cm->hparams.default_num_delay_tokens;
    int                    min_decode_ms = k_default_min_decode_interval_ms;
    const transcribe_ext * fam           = (sp != nullptr) ? sp->family : nullptr;
    if (const transcribe_status st = transcribe_ext_check(fam, TRANSCRIBE_EXT_KIND_VOXTRAL_REALTIME_STREAM,
                                                          sizeof(transcribe_voxtral_realtime_stream_ext));
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (fam != nullptr) {
        const auto *  vx = reinterpret_cast<const transcribe_voxtral_realtime_stream_ext *>(fam);
        // num_delay_tokens: -1 = model default (6 = 480 ms). Otherwise a
        // publisher-validated delay: tokens 1..15 (80..1200 ms) or 30 (2400 ms).
        // One token = 80 ms (12.5 Hz grid).
        const int32_t nt = vx->num_delay_tokens;
        if (nt != -1 && !((nt >= 1 && nt <= 15) || nt == 30)) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        if (nt >= 0) {
            num_delay = nt;
        }
        if (vx->min_decode_interval_ms < -1) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        if (vx->min_decode_interval_ms >= 0) {
            min_decode_ms = vx->min_decode_interval_ms;
        }
    }
    if (out_num_delay) {
        *out_num_delay = num_delay;
    }
    if (out_min_decode_ms) {
        *out_min_decode_ms = min_decode_ms;
    }
    return TRANSCRIBE_OK;
}

// Install the result snapshot (full_text + single text-only segment) from a
// forward over `n_avail` accumulated samples. Shared by feed (tentative) and
// finalize (final).
void publish_stream_text(Session * cc, Model * cm, std::string text, int64_t audio_ms) {
    cc->full_text   = text;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;
    cc->segments.clear();
    transcribe_session::SegmentEntry seg{};
    seg.text  = std::move(text);
    seg.t0_ms = 0;
    seg.t1_ms = audio_ms;
    cc->segments.push_back(std::move(seg));
    (void) cm;
}

transcribe_status stream_validate(const transcribe_session * session,
                                  const transcribe_run_params * /*run_params*/,
                                  const transcribe_stream_params * stream_params) {
    const auto * cc = static_cast<const Session *>(session);
    const auto * cm = static_cast<const Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    int nd = 0, md = 0;
    return resolve_stream_ext(stream_params, cm, &nd, &md);
}

// Detokenize the emitted ids -> trimmed transcript (mirrors forward_buffer).
std::string detok_generated(Model * cm, const std::vector<int32_t> & ids) {
    std::vector<int32_t> text_ids;
    text_ids.reserve(ids.size());
    for (int32_t id : ids) {
        if (!cm->tok.is_control(id)) {
            text_ids.push_back(id);
        }
    }
    std::string text = cm->tok.decode(text_ids.data(), static_cast<int>(text_ids.size()));
    size_t      b = 0, e = text.size();
    while (b < e && std::isspace(static_cast<unsigned char>(text[b]))) {
        ++b;
    }
    while (e > b && std::isspace(static_cast<unsigned char>(text[e - 1]))) {
        --e;
    }
    return text.substr(b, e - b);
}

// Allocate + compute a graph on the session scheduler. Inputs are set by
// `set_inputs` AFTER allocation (the alloc assigns tensor data pointers).
bool stream_run_graph(Session *                     cc,
                      Model *                       cm,
                      ggml_cgraph *                 gf,
                      const std::function<void()> & set_inputs,
                      int64_t *                     out_compute_us = nullptr) {
    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            return false;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, gf)) {  // graph alloc = overhead
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime: stream graph allocation failed — "
                            "out of memory.");
        return false;
    }
    set_inputs();
    apply_threads(cc->sched, cc->n_threads);
    const int64_t tc0 = ggml_time_us();
    const bool    ok  = ggml_backend_sched_graph_compute(cc->sched, gf) == GGML_STATUS_SUCCESS;
    if (out_compute_us) {
        *out_compute_us = ggml_time_us() - tc0;  // pure graph_compute
    }
    return ok;
}

// Encoder KV ring geometry. Keep the last `sliding_window`(750) frames: hold a
// contiguous cache of k_enc_ring_ctx slots, append at the write head, and
// periodically COMPACT (copy the last 750 frames to the front) so the ring never
// overflows. k_enc_max_batch bounds frames per chunk (must be <= ring -
// sliding_window and a multiple of proj_downsample).
constexpr int k_enc_ring_ctx  = 1536;
constexpr int k_enc_max_batch = 512;

// (decoder sliding-window ring size is read from the GGUF: hp.dec_sliding_window)

// Compact the encoder KV ring: move the last `keep` frames (slots
// [from_slot-keep, from_slot)) to the front [0, keep), per layer for K and V.
// Host round-trip (a separate buffer, so correct even when src/dst overlap;
// infrequent — every ~k_enc_max_batch frames). K/V store RoPE baked at ABSOLUTE
// positions, so moving the bytes preserves correctness; the caller updates
// stream_enc_abs_base so the windowed mask stays consistent.
void compact_enc_ring(Session * cc, Model * cm, int keep, int from_slot) {
    const int          C        = cc->enc_kv.n_ctx;
    const size_t       kv_dim   = static_cast<size_t>(cm->hparams.enc_n_heads) * cm->hparams.enc_head_dim;
    const int          n_layers = cm->hparams.enc_n_layers;
    const size_t       elem     = ggml_element_size(cc->enc_kv.self_k);
    std::vector<float> buf(static_cast<size_t>(keep) * kv_dim);
    ggml_tensor *      tens[2] = { cc->enc_kv.self_k, cc->enc_kv.self_v };
    for (int L = 0; L < n_layers; ++L) {
        for (ggml_tensor * t : tens) {
            const size_t src = (static_cast<size_t>(L) * C + (from_slot - keep)) * kv_dim;
            const size_t dst = (static_cast<size_t>(L) * C) * kv_dim;
            ggml_backend_tensor_get(t, buf.data(), elem * src, buf.size() * elem);
            ggml_backend_tensor_set(t, buf.data(), elem * dst, buf.size() * elem);
        }
    }
}

// The incremental streaming driver. Recomputes the cheap mel + conv stem over
// the accumulated (left-padded) buffer, then advances the EXPENSIVE encoder
// transformer (StaticCache ring) and decoder (persistent KV) one audio
// frame/token at a time — exactly the reference generate() mechanism. On
// is_final the full offline-padded buffer is processed, so the emitted ids equal
// the offline path (and the `stream` oracle). Sets *out_changed on advance.
transcribe_status stream_process(Session * cc, Model * cm, bool is_final, bool * out_changed) {
    if (out_changed) {
        *out_changed = false;
    }
    const HParams &   hp          = cm->hparams;
    const bool        dumps_on    = transcribe::debug::enabled();
    const int         n_mels      = hp.enc_num_mel_bins;
    const int         hop         = hp.fe_hop_length;
    const int         win         = hp.fe_win_length;
    const int         down        = hp.proj_downsample;
    const int         alpt        = hp.audio_length_per_tok;
    const int         dec_h       = hp.dec_hidden;
    const int         raw_per_tok = alpt * hop;                  // 1280
    const int         n_left_tok  = cm->specials.n_left_pad;     // 32
    const int         num_delay   = cc->stream_num_delay;
    const int         T_prompt    = 1 + n_left_tok + num_delay;  // BOS + pads
    const std::string prev_text   = cc->full_text;

    if (!cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->stream_pcm.empty() && !is_final) {
        return TRANSCRIBE_OK;
    }

    // ---- 1. Build ONLY the window of the (left-padded) buffer the streaming mel
    //         needs: absolute samples [ws_sample, total_abs). ws_sample backs off
    //         GUARD frames before the first uncommitted mel frame; center=True
    //         reflects at the window start, so the first ceil(pad/hop)=2 window
    //         frames are discarded (GUARD=4). Same samples the whole-buffer mel
    //         saw -> bit-identical. `mel_off` = absolute frame of window-frame 0;
    //         `mel_n` = window frame count (stride into mel_buf). ----
    constexpr int GUARD       = 4;
    const size_t  left        = static_cast<size_t>(n_left_tok) * raw_per_tok;
    const int     mel_off     = std::max(0, cc->stream_n_mel_committed - GUARD);
    const size_t  ws_sample   = static_cast<size_t>(mel_off) * hop;
    const size_t  drop        = static_cast<size_t>(cc->stream_pcm_drop);
    const size_t  n_aud_total = drop + cc->stream_pcm.size();  // total real audio samples
    size_t        total_abs;
    if (is_final) {
        const size_t raw_pad = ((n_aud_total + raw_per_tok - 1) / raw_per_tok) * raw_per_tok;
        const size_t right   = static_cast<size_t>(num_delay + 1 + 10) * raw_per_tok;
        total_abs            = left + raw_pad + right;
    } else {
        total_abs = left + n_aud_total;
    }
    if (total_abs <= ws_sample) {
        return TRANSCRIBE_OK;
    }
    const size_t       win_len = total_abs - ws_sample;
    std::vector<float> buffer(win_len, 0.0f);             // buffer[j] == absolute sample ws_sample+j
    const size_t       a_lo = std::max(ws_sample, left);  // real-audio overlap of the window
    const size_t       a_hi = std::min(total_abs, left + n_aud_total);
    if (a_hi > a_lo) {
        std::memcpy(buffer.data() + (a_lo - ws_sample), cc->stream_pcm.data() + (a_lo - left - drop),
                    (a_hi - a_lo) * sizeof(float));
    }
    // Drop PCM the window has moved past — every future feed's ws_sample >= this
    // one's, so [0, ws_sample) is never read again. Keep [ws_sample, end); the
    // erase shifts only the small retained window, so memory stays bounded.
    if (ws_sample > left + drop) {
        const size_t keep_from = ws_sample - left - drop;
        cc->stream_pcm.erase(cc->stream_pcm.begin(), cc->stream_pcm.begin() + static_cast<std::ptrdiff_t>(keep_from));
        cc->stream_pcm_drop += static_cast<int64_t>(keep_from);
    }

    // ---- 2. Streaming mel over the window. ----
    int           mm = 0, mel_n = 0;
    const int64_t t_mel0 = ggml_time_us();
    if (auto st = cm->mel->compute(buffer.data(), win_len, cc->mel_buf, mm, mel_n, cc->n_threads);
        st != TRANSCRIBE_OK) {
        return st;
    }
    cc->stream_t_mel_us += ggml_time_us() - t_mel0;
    if (mm != n_mels) {
        return TRANSCRIBE_ERR_GGUF;
    }

    int n_mel_ready;  // ABSOLUTE stable-frame count (window covers [mel_off, mel_off+mel_n))
    if (is_final) {
        n_mel_ready = mel_off + mel_n;
    } else {
        // Conservative: a centered frame is stable once its full window is
        // within real samples (so it equals the final buffer's same-index frame).
        const long fr = (static_cast<long>(total_abs) - win) / hop;
        n_mel_ready   = static_cast<int>(std::max(0L, fr + 1));
        n_mel_ready   = std::min(n_mel_ready, mel_off + mel_n);
    }
    if (n_mel_ready <= 4) {
        return TRANSCRIBE_OK;
    }
    const int T_emb_ready = (n_mel_ready - 2) / 2 + 1;
    if (is_final) {
        cc->stream_n_audio_clamp = T_emb_ready / down;
    }

    // ---- 3. Conv stem with the streaming PADDING CACHE: feed only the NEW mel
    //         frames (token-aligned, a multiple of 2*down=8) against the carried
    //         conv caches -> M_emb = M/2 new embedder frames. Bit-identical to the
    //         whole-buffer left-pad conv, but O(new) per feed. ----
    const int ed = hp.enc_d_model;
    const int M  = ((n_mel_ready - cc->stream_n_mel_committed) / (2 * down)) * (2 * down);
    if (M > 0) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        const int          emb_base_abs = cc->stream_n_enc_committed;  // abs index of emb_host[0]
        const int          M_emb        = M / 2;                       // new embedder frames (down-aligned)
        const int          m0           = cc->stream_n_mel_committed;  // first new mel frame (absolute)
        const int          mrel         = m0 - mel_off;                // ... as a window-relative index
        std::vector<float> emb_host(static_cast<size_t>(ed) * M_emb);
        const int64_t      t_conv0 = ggml_time_us();
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
                                    "voxtral_realtime: compute context allocation failed — "
                                    "out of memory.");
                return TRANSCRIBE_ERR_OOM;
            }
            EmbedderChunkBuild emb = build_embedder_chunk_graph(cc->compute_ctx, cm->weights, hp, M);
            if (emb.graph == nullptr || emb.out == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
            // new mel frames [m0, m0+M), mel-major buffer -> frame-major [n_mels, M].
            std::vector<float> mel_fm(static_cast<size_t>(n_mels) * M);
            for (int t = 0; t < M; ++t) {
                for (int k = 0; k < n_mels; ++k) {
                    mel_fm[static_cast<size_t>(t) * n_mels + k] =
                        cc->mel_buf[static_cast<size_t>(k) * mel_n + (mrel + t)];
                }
            }
            if (!stream_run_graph(cc, cm, emb.graph, [&] {
                    ggml_backend_tensor_set(emb.mel_in, mel_fm.data(), 0, mel_fm.size() * sizeof(float));
                    ggml_backend_tensor_set(emb.cache1_in, cc->stream_conv0_cache.data(), 0,
                                            cc->stream_conv0_cache.size() * sizeof(float));
                    ggml_backend_tensor_set(emb.cache2_in, cc->stream_conv1_cache.data(), 0,
                                            cc->stream_conv1_cache.size() * sizeof(float));
                })) {
                return TRANSCRIBE_ERR_GGUF;
            }
            ggml_backend_tensor_get(emb.out, emb_host.data(), 0, emb_host.size() * sizeof(float));
            // Carry conv caches for the next chunk: conv0 ← last 2 NEW mel frames
            // (frame-major [n_mels,2]); conv1 ← the chunk's last conv0-output frame.
            for (int k = 0; k < n_mels; ++k) {
                cc->stream_conv0_cache[static_cast<size_t>(0) * n_mels + k] =
                    cc->mel_buf[static_cast<size_t>(k) * mel_n + (mrel + M - 2)];
                cc->stream_conv0_cache[static_cast<size_t>(1) * n_mels + k] =
                    cc->mel_buf[static_cast<size_t>(k) * mel_n + (mrel + M - 1)];
            }
            ggml_backend_tensor_get(emb.cache2_out, cc->stream_conv1_cache.data(), 0,
                                    cc->stream_conv1_cache.size() * sizeof(float));
            cc->stream_n_mel_committed += M;
        }
        cc->stream_t_conv_us += ggml_time_us() - t_conv0;

        // ---- 4. Encoder ring: push the M_emb new frames in <=k_enc_max_batch batches.
        int               remaining = M_emb;
        const int         C         = cc->enc_kv.n_ctx;
        const int         W         = hp.enc_sliding_window;
        const ggml_fp16_t mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
        const int64_t     t_enc0 = ggml_time_us();
        while (remaining > 0) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            const int batch = std::min(remaining, k_enc_max_batch);
            // Compact the ring so this batch fits (keeps the last W frames).
            if (cc->stream_enc_slot + batch > C) {
                const int keep = std::min(cc->stream_enc_slot, W);
                compact_enc_ring(cc, cm, keep, cc->stream_enc_slot);
                cc->stream_enc_abs_base += cc->stream_enc_slot - keep;
                cc->stream_enc_slot = keep;
            }
            const int write_slot = cc->stream_enc_slot;
            const int abs0       = cc->stream_n_enc_committed;  // abs of first query
            const int read_start = std::max(0, write_slot - (W - 1));
            const int read_len   = (write_slot + batch) - read_start;

            if (cc->compute_ctx != nullptr) {
                ggml_free(cc->compute_ctx);
                cc->compute_ctx = nullptr;
            }
            ggml_init_params ip{};
            ip.mem_size     = 256 * 1024 * 1024;
            ip.no_alloc     = true;
            cc->compute_ctx = ggml_init(ip);
            if (cc->compute_ctx == nullptr) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "voxtral_realtime: compute context allocation failed — "
                                    "out of memory.");
                return TRANSCRIBE_ERR_OOM;
            }
            EncoderChunkBuild eb = build_encoder_chunk_graph(cc->compute_ctx, cm->weights, hp, cc->enc_kv, batch,
                                                             write_slot, read_start, read_len, cc->encoder_use_flash);
            if (eb.graph == nullptr || eb.out == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }

            const int          rel0 = abs0 - emb_base_abs;  // offset into emb_host
            std::vector<float> chunk_in(static_cast<size_t>(ed) * batch);
            for (int i = 0; i < batch; ++i) {
                for (int d = 0; d < ed; ++d) {
                    chunk_in[static_cast<size_t>(i) * ed + d] = emb_host[static_cast<size_t>(rel0 + i) * ed + d];
                }
            }
            std::vector<int32_t> pos(batch);
            for (int i = 0; i < batch; ++i) {
                pos[i] = abs0 + i;
            }
            // Sliding-window-causal mask [read_len(key), batch(query)], ABSOLUTE.
            // key read-index k -> abs = stream_enc_abs_base + read_start + k.
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(read_len) * batch, mn);
            for (int qi = 0; qi < batch; ++qi) {
                const int qabs = abs0 + qi;
                for (int k = 0; k < read_len; ++k) {
                    const int kabs = cc->stream_enc_abs_base + read_start + k;
                    if (kabs <= qabs && (qabs - kabs) < W) {
                        mask[static_cast<size_t>(qi) * read_len + k] = mz;
                    }
                }
            }
            int64_t enc_compute_us = 0;
            if (!stream_run_graph(
                    cc, cm, eb.graph,
                    [&] {
                        ggml_backend_tensor_set(eb.embed_in, chunk_in.data(), 0, chunk_in.size() * sizeof(float));
                        ggml_backend_tensor_set(eb.positions_in, pos.data(), 0, pos.size() * sizeof(int32_t));
                        ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
                    },
                    &enc_compute_us)) {
                return TRANSCRIBE_ERR_GGUF;
            }
            cc->stream_t_enc_compute_us += enc_compute_us;

            const int    n_audio_new = eb.n_audio_new;
            const size_t base        = static_cast<size_t>(cc->stream_n_tok_ready - cc->stream_audio_base) * dec_h;
            cc->stream_audio_embeds.resize(base + static_cast<size_t>(n_audio_new) * dec_h);
            ggml_backend_tensor_get(eb.out, cc->stream_audio_embeds.data() + base, 0,
                                    static_cast<size_t>(n_audio_new) * dec_h * sizeof(float));
            cc->stream_enc_slot += batch;
            cc->stream_n_enc_committed += batch;
            cc->stream_n_tok_ready += n_audio_new;
            remaining -= batch;
        }
        cc->stream_t_enc_us += ggml_time_us() - t_enc0;
    }

    // ---- 5. Decoder: prompt prefill once, then one step per ready token. ----
    const int64_t t_dec0 = ggml_time_us();
    const int     vocab  = hp.dec_vocab_size;
    const int32_t eos_id = hp.eos_token_id;
    auto          argmax = [](const std::vector<float> & v) -> int32_t {
        int32_t best = 0;
        float   bv   = v[0];
        for (int32_t i = 1; i < static_cast<int32_t>(v.size()); ++i) {
            if (v[i] > bv) {
                bv   = v[i];
                best = i;
            }
        }
        return best;
    };

    if (!cc->stream_prompt_done && cc->stream_n_tok_ready >= T_prompt) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (cc->kv_cache.buffer != nullptr) {
            ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        }
        cc->kv_cache.n    = 0;
        cc->kv_cache.head = 0;

        std::vector<int32_t> prompt_ids;
        prompt_ids.push_back(cm->specials.bos);
        for (int i = 0; i < T_prompt - 1; ++i) {
            prompt_ids.push_back(cm->specials.streaming_pad);
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, cc->ada_scale_all,
                                              T_prompt, cc->decoder_use_flash, /*want_all_logits=*/false);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, ggml_fp32_to_fp16(-INFINITY));
        const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f);
        for (int r = 0; r < T_prompt; ++r) {
            for (int c = 0; c <= r; ++c) {
                mask[static_cast<size_t>(r) * T_prompt + c] = mz;
            }
        }
        if (!stream_run_graph(cc, cm, pb.graph, [&] {
                ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
                ggml_backend_tensor_set(pb.audio_in, cc->stream_audio_embeds.data(), 0,
                                        static_cast<size_t>(dec_h) * T_prompt * sizeof(float));
                ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
                ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
            })) {
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.n    = T_prompt;
        cc->kv_cache.head = T_prompt;
        std::vector<float> logits(vocab);
        ggml_backend_tensor_get(pb.out, logits.data(), 0, logits.size() * sizeof(float));
        if (dumps_on) {
            cc->stream_gen0_logits = logits;  // stream.logits_raw
        }
        const int32_t g0 = argmax(logits);
        cc->stream_generated.push_back(g0);
        cc->stream_next_tok    = g0;
        cc->stream_dec_pos     = T_prompt;
        cc->stream_prompt_done = true;
    }

    if (cc->stream_prompt_done && !cc->stream_eos && cc->stream_dec_pos < cc->stream_n_tok_ready) {
        const int max_n_kv = cc->kv_cache.n_ctx;
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, cc->ada_scale_all, max_n_kv,
                                        cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        if (cc->sched == nullptr) {
            cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                               static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
            if (cc->sched == nullptr) {
                return TRANSCRIBE_ERR_GGUF;
            }
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime: stream step graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        apply_threads(cc->sched, cc->n_threads);

        const int                swin    = hp.dec_sliding_window;  // 8192
        const int                kv_ring = max_n_kv;               // ring size == n_ctx (== swin here)
        // Hard absolute-position cap = dec_max_position; a streaming caller hits
        // memory/latency long before it (~2.9 h).
        const int                max_pos = voxtral_realtime_abs_position_cap(hp);
        const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> step_mask(max_n_kv, mn);
        int                      cap         = (is_final && cc->stream_n_audio_clamp >= 0) ?
                                                   std::min(cc->stream_n_tok_ready, cc->stream_n_audio_clamp) :
                                                   cc->stream_n_tok_ready;
        // `hit_pos_cap`: this chunk's decode was limited by the hard position cap
        // (not by stream_n_tok_ready / the finalize clamp), so the truncation WARN
        // below fires only at the genuine hard cap.
        const bool               hit_pos_cap = (max_pos < cap);
        cap                                  = std::min(cap, max_pos);
        while (!cc->stream_eos && cc->stream_dec_pos < cap) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            const int cur = cc->stream_dec_pos;
            ggml_backend_tensor_set(sb.input_id_in, &cc->stream_next_tok, 0, sizeof(int32_t));
            ggml_backend_tensor_set(
                sb.audio_in, cc->stream_audio_embeds.data() + static_cast<size_t>(cur - cc->stream_audio_base) * dec_h,
                0, static_cast<size_t>(dec_h) * sizeof(float));
            const int32_t pos_val = cur;
            ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
            // Sliding-window ring write: token `cur` lands at slot `cur % kv_ring`,
            // overwriting token `cur - kv_ring` (exactly one past the window, so it
            // is already evicted). RoPE position stays ABSOLUTE (`cur`).
            const int64_t kv_idx = cur % kv_ring;
            ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx, 0, sizeof(int64_t));
            // Reveal keys [max(0,cur-swin+1) .. cur] at their ring slots (t % kv_ring).
            // For cur < kv_ring this is the identity slot map (== the pre-ring path);
            // once full, the `swin` in-window tokens occupy all `kv_ring` slots 1:1.
            std::fill(step_mask.begin(), step_mask.end(), mn);
            for (int t = std::max(0, cur - swin + 1); t <= cur; ++t) {
                step_mask[t % kv_ring] = mz;
            }
            ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0,
                                    static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));
            if (ggml_backend_sched_graph_compute(cc->sched, sb.graph) != GGML_STATUS_SUCCESS) {
                return TRANSCRIBE_ERR_GGUF;
            }
            if (dumps_on && cur == T_prompt - 1 + 8) {  // stream.logits_raw.gen8
                cc->stream_gen8_logits.assign(vocab, 0.0f);
                ggml_backend_tensor_get(sb.logits, cc->stream_gen8_logits.data(), 0,
                                        static_cast<size_t>(vocab) * sizeof(float));
            }
            int32_t tok = 0;
            ggml_backend_tensor_get(sb.out, &tok, 0, sizeof(int32_t));
            cc->stream_next_tok = tok;
            cc->stream_dec_pos  = cur + 1;
            // Bookkeeping only (block_step reads n_ctx / max_n_kv, not n / head);
            // keep them ring-consistent rather than letting them run past n_ctx.
            cc->kv_cache.n      = std::min(cur + 2, kv_ring);
            cc->kv_cache.head   = (cur + 1) % kv_ring;
            if (tok == eos_id) {
                cc->stream_eos = true;
                break;
            }
            cc->stream_generated.push_back(tok);
        }

        // Truncation at the hard context cap without an EOS: flag via
        // transcribe_was_truncated() + a single WARN (guarded by !was_truncated
        // so it fires at most once across the per-feed/finalize re-entries).
        if (hit_pos_cap && !cc->stream_eos && cc->stream_dec_pos >= cap && !cc->was_truncated) {
            cc->was_truncated = true;
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                "voxtral_realtime: output truncated at the %d-token context cap "
                                "before end-of-stream; transcript may be incomplete.",
                                max_pos);
        }
    }

    cc->stream_t_dec_us += ggml_time_us() - t_dec0;

    // Release consumed audio embeds [audio_base, dec_pos) — the decoder never
    // re-reads past positions, so memory stays bounded. Skipped while dumping so
    // the finalize proj.out dump keeps the full [0, n_tok_ready) history.
    if (!dumps_on && cc->stream_dec_pos > cc->stream_audio_base) {
        const size_t drop_n = static_cast<size_t>(cc->stream_dec_pos - cc->stream_audio_base) * dec_h;
        cc->stream_audio_embeds.erase(cc->stream_audio_embeds.begin(),
                                      cc->stream_audio_embeds.begin() + static_cast<std::ptrdiff_t>(drop_n));
        cc->stream_audio_base = cc->stream_dec_pos;
    }

    // Per-component timing (diagnostic): printed once at finalize.
    if (is_final && transcribe::env::flag("TRANSCRIBE_VOXTRAL_REALTIME_STREAM_TIMING")) {
        const double mel = cc->stream_t_mel_us / 1000.0, conv = cc->stream_t_conv_us / 1000.0;
        const double enc = cc->stream_t_enc_us / 1000.0, dec = cc->stream_t_dec_us / 1000.0;
        const double enc_c = cc->stream_t_enc_compute_us / 1000.0;  // pure graph_compute
        const double enc_o = enc - enc_c;                           // build + alloc + host prep
        const double tot   = mel + conv + enc + dec;
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[stream-timing] mel=%.0fms (%.1f%%)  conv=%.0fms (%.1f%%)  enc=%.0fms (%.1f%%)"
                " [compute=%.0f overhead=%.0f]  dec=%.0fms (%.1f%%)  sum=%.0fms",
                mel, tot > 0 ? 100.0 * mel / tot : 0.0, conv, tot > 0 ? 100.0 * conv / tot : 0.0, enc,
                tot > 0 ? 100.0 * enc / tot : 0.0, enc_c, enc_o, dec, tot > 0 ? 100.0 * dec / tot : 0.0, tot);
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[stream-mem] retained PCM=%zu samples (%.1fs)  audio-embeds=%zu frames  (vs %d total tokens)",
                cc->stream_pcm.size(), static_cast<double>(cc->stream_pcm.size()) / std::max(1, hp.fe_sample_rate),
                cc->stream_audio_embeds.size() / std::max<size_t>(1, dec_h), cc->stream_n_tok_ready);
    }

    // ---- 6. Numerical-validation dumps (finalize only). ----
    if (dumps_on && is_final) {
        // stream proj.out = the incremental-encoder audio embeds [dec_h, n_tok].
        if (cc->stream_n_tok_ready > 0) {
            const long long shp[2] = { dec_h, cc->stream_n_tok_ready };
            transcribe::debug::dump_host_f32("proj.out", cc->stream_audio_embeds.data(),
                                             static_cast<long long>(cc->stream_audio_embeds.size()), shp, 2, "proj");
        }
        const long long lshp[1] = { vocab };
        if (!cc->stream_gen0_logits.empty()) {
            transcribe::debug::dump_host_f32("stream.logits_raw", cc->stream_gen0_logits.data(), vocab, lshp, 1,
                                             "stream");
        }
        if (!cc->stream_gen8_logits.empty()) {
            transcribe::debug::dump_host_f32("stream.logits_raw.gen8", cc->stream_gen8_logits.data(), vocab, lshp, 1,
                                             "stream");
        }
    }

    // ---- 7. Publish transcript. ----
    std::string   text     = detok_generated(cm, cc->stream_generated);
    const int     sr       = std::max(1, hp.fe_sample_rate);
    const int64_t audio_ms = static_cast<int64_t>(n_aud_total) * 1000 / sr;
    publish_stream_text(cc, cm, text, audio_ms);
    if (out_changed) {
        *out_changed = (text != prev_text) || is_final;
    }
    return TRANSCRIBE_OK;
}

transcribe_status stream_begin(transcribe_session * session,
                               const transcribe_run_params * /*run_params*/,
                               const transcribe_stream_params * stream_params) {
    auto * cc = static_cast<Session *>(session);
    auto * cm = static_cast<Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    int nd = 0, md = 0;
    if (auto st = resolve_stream_ext(stream_params, cm, &nd, &md); st != TRANSCRIBE_OK) {
        return st;
    }

    cc->stream_pcm.clear();
    cc->stream_num_delay           = nd;
    cc->stream_min_decode_ms       = md;
    cc->stream_last_decode_samples = 0;

    // Reset incremental encoder/decoder state.
    cc->stream_n_enc_committed = 0;
    cc->stream_enc_slot        = 0;
    cc->stream_enc_abs_base    = 0;
    cc->stream_conv0_cache.assign(static_cast<size_t>(cm->hparams.enc_num_mel_bins) * 2, 0.0f);
    cc->stream_conv1_cache.assign(static_cast<size_t>(cm->hparams.enc_d_model), 0.0f);
    cc->stream_n_mel_committed = 0;
    cc->stream_pcm_drop        = 0;
    cc->stream_audio_base      = 0;
    cc->stream_t_mel_us = cc->stream_t_conv_us = cc->stream_t_enc_us = cc->stream_t_dec_us = 0;
    cc->stream_t_enc_compute_us                                                            = 0;
    cc->stream_audio_embeds.clear();
    cc->stream_n_tok_ready = 0;
    cc->stream_prompt_done = false;
    cc->stream_dec_pos     = 0;
    cc->stream_next_tok    = 0;
    cc->stream_eos         = false;
    cc->stream_generated.clear();
    cc->stream_n_audio_clamp = -1;

    // Encoder StaticCache ring (F32; MHA so n_kv_heads == n_heads). Fixed
    // k_enc_ring_ctx slots; compaction keeps the last sliding_window(750) frames
    // so any stream length runs in constant memory (the reference mechanism).
    cc->enc_kv.free();
    if (!transcribe::causal_lm::kv_init(cc->enc_kv, cm->plan.primary, /*n_ctx=*/k_enc_ring_ctx, cm->hparams.enc_n_heads,
                                        cm->hparams.enc_head_dim, cm->hparams.enc_n_layers, GGML_TYPE_F32)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "voxtral_realtime stream_begin: encoder KV cache allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }
    if (cc->enc_kv.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->enc_kv.buffer, 0);
    }

    // Decoder KV: a sliding-window RING sized to the model's own `sliding_window`
    // (from the GGUF, 8192 here). The step loop writes token `cur` at slot
    // `cur % n_ctx`, so the cache holds the last `swin` tokens for any stream
    // length in constant memory. `sliding_window` is a trained-in constant, not
    // an inference knob. Sized once per session; never shrinks a larger ctx a
    // prior offline run may have left.
    const int dec_ring = cm->hparams.dec_sliding_window;
    if (cc->kv_cache.n_ctx < dec_ring) {
        const ggml_type kt = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
        cc->kv_cache.free();
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, dec_ring, cm->hparams.dec_n_kv_heads,
                                            cm->hparams.dec_head_dim, cm->hparams.dec_n_layers, kt)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime stream_begin: decoder KV cache allocation "
                                "failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    if (cc->kv_cache.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
    }
    cc->kv_cache.n    = 0;
    cc->kv_cache.head = 0;

    // Adaptive-norm scales for the active delay (persist across feeds).
    if (auto st = compute_ada_scales(cc, cm, nd); st != TRANSCRIBE_OK) {
        return st;
    }

    transcribe::debug::init();
    return TRANSCRIBE_OK;
}

transcribe_status stream_feed(transcribe_session *       session,
                              const float *              pcm,
                              int                        n_samples,
                              transcribe_stream_update * update) {
    auto * cc = static_cast<Session *>(session);
    auto * cm = static_cast<Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    if (n_samples > 0 && pcm != nullptr) {
        cc->stream_pcm.insert(cc->stream_pcm.end(), pcm, pcm + n_samples);
    }

    const int sr              = std::max(1, cm->hparams.fe_sample_rate);
    cc->stream_audio_input_us = static_cast<int64_t>(cc->stream_pcm.size()) * 1000000 / sr;

    bool          result_changed   = false;
    const int64_t since            = static_cast<int64_t>(cc->stream_pcm.size()) - cc->stream_last_decode_samples;
    const int64_t interval_samples = static_cast<int64_t>(cc->stream_min_decode_ms) * sr / 1000;
    if (since >= interval_samples) {
        if (auto st = stream_process(cc, cm, /*is_final=*/false, &result_changed); st != TRANSCRIBE_OK) {
            return st;
        }
        cc->stream_last_decode_samples = static_cast<int64_t>(cc->stream_pcm.size());
    }

    if (update != nullptr) {
        update->result_changed     = result_changed;
        update->revision           = cc->stream_revision;
        update->input_received_ms  = cc->stream_audio_input_us / 1000;
        update->audio_committed_ms = cc->stream_audio_committed_us / 1000;
        update->buffered_ms        = 0;
    }
    return TRANSCRIBE_OK;
}

transcribe_status stream_finalize(transcribe_session * session, transcribe_stream_update * update) {
    auto * cc = static_cast<Session *>(session);
    auto * cm = static_cast<Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    bool changed = false;
    if (auto st = stream_process(cc, cm, /*is_final=*/true, &changed); st != TRANSCRIBE_OK) {
        return st;
    }

    const int     sr       = std::max(1, cm->hparams.fe_sample_rate);
    const int64_t audio_ms = static_cast<int64_t>(cc->stream_pcm.size()) * 1000 / sr;
    if (update != nullptr) {
        update->result_changed     = true;
        update->revision           = cc->stream_revision;
        update->input_received_ms  = audio_ms;
        update->audio_committed_ms = audio_ms;
        update->buffered_ms        = 0;
    }
    return TRANSCRIBE_OK;
}

void stream_reset(transcribe_session * session) {
    auto * cc = static_cast<Session *>(session);
    cc->stream_pcm.clear();
    cc->stream_last_decode_samples = 0;
    cc->stream_num_delay           = -1;
    cc->enc_kv.free();
    cc->stream_n_enc_committed = 0;
    cc->stream_enc_slot        = 0;
    cc->stream_enc_abs_base    = 0;
    cc->stream_conv0_cache.clear();
    cc->stream_conv1_cache.clear();
    cc->stream_n_mel_committed = 0;
    cc->stream_pcm_drop        = 0;
    cc->stream_audio_base      = 0;
    cc->stream_audio_embeds.clear();
    cc->stream_n_tok_ready = 0;
    cc->stream_prompt_done = false;
    cc->stream_dec_pos     = 0;
    cc->stream_next_tok    = 0;
    cc->stream_eos         = false;
    cc->stream_generated.clear();
    cc->stream_n_audio_clamp = -1;
}

bool accepts_ext_kind(const transcribe_model * model, transcribe_ext_slot slot, uint32_t kind) {
    (void) model;
    if (slot != TRANSCRIBE_EXT_SLOT_STREAM) {
        return false;
    }
    return kind == TRANSCRIBE_EXT_KIND_VOXTRAL_REALTIME_STREAM;
}

// ---------------------------------------------------------------------------
// Offline batched decode (transcribe_run_batch)
// ---------------------------------------------------------------------------
//
// Batched audio tower: every clip's mel is right-padded to the batch max and
// stacked on ne[2]; the causal encoder isolates each row's real frames (no
// per-row mask). vs voxtral's run_batch: a single whole-clip encoder (not 30 s
// chunks), TIED head, the ada ffn_scale, ADDITIVE audio at every prefill position
// AND every decode step, and a per-row n_audio clamp. Falls back to serial for
// n==1, dump mode, or no decoder flash (the batched blocks are flash-only).

transcribe_status run_batch_serial(Session *                     cc,
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

// Lockstep batched greedy decode. Like causal_lm::run_batched_step_loop but adds
// (1) per-step audio injection (a fresh audio embed per row at its position) and
// (2) a per-row n_audio clamp. Mirrors forward_buffer's single-utterance step
// loop: open the KV window [0, n_past] cumulatively (no sliding-window eviction;
// offline clips fit the window), never push EOS, stop a row at EOS or n_audio_b.
transcribe_status run_batch_step_loop(Session *                               cc,
                                      const StepBuildBatched &                sb,
                                      int                                     n,
                                      int                                     max_n_kv,
                                      int32_t                                 eos_id,
                                      int                                     dec_h,
                                      const std::vector<std::vector<float>> & enc_hosts,
                                      const std::vector<int> &                n_audio_b,
                                      std::vector<char> &                     valid,
                                      std::vector<int32_t> &                  next_tok,
                                      std::vector<int> &                      n_past,
                                      std::vector<std::vector<int32_t>> &     generated) {
    const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
    std::vector<int32_t>     ids_buf(n, 0), pos_buf(n, 0), out_buf(n, 0);
    std::vector<int64_t>     kvidx_buf(n, 0);
    std::vector<float>       audio_buf(static_cast<size_t>(dec_h) * n, 0.0f);
    std::vector<ggml_fp16_t> mask_buf(static_cast<size_t>(max_n_kv) * n, mn);
    std::vector<char>        finished(n, 1);
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            finished[b] = (next_tok[b] == eos_id || n_past[b] >= n_audio_b[b]);
        }
    }
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        const size_t base = static_cast<size_t>(b) * max_n_kv;
        for (int c = 0; c <= n_past[b] && c < max_n_kv; ++c) {
            mask_buf[base + c] = mz;
        }
    }

    bool all_done = false;
    while (!all_done) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        for (int b = 0; b < n; ++b) {
            ids_buf[b]   = next_tok[b];
            pos_buf[b]   = n_past[b];
            kvidx_buf[b] = n_past[b];
            int ac       = n_past[b];  // audio embed for this position
            if (ac >= n_audio_b[b]) {
                ac = (n_audio_b[b] > 0 ? n_audio_b[b] - 1 : 0);
            }
            float * dst = audio_buf.data() + static_cast<size_t>(b) * dec_h;
            if (valid[b] && !enc_hosts[b].empty()) {
                std::memcpy(dst, enc_hosts[b].data() + static_cast<size_t>(ac) * dec_h,
                            static_cast<size_t>(dec_h) * sizeof(float));
            } else {
                std::memset(dst, 0, static_cast<size_t>(dec_h) * sizeof(float));
            }
        }
        ggml_backend_tensor_set(sb.input_ids_in, ids_buf.data(), 0, ids_buf.size() * sizeof(int32_t));
        ggml_backend_tensor_set(sb.audio_in, audio_buf.data(), 0, audio_buf.size() * sizeof(float));
        ggml_backend_tensor_set(sb.position_in, pos_buf.data(), 0, pos_buf.size() * sizeof(int32_t));
        ggml_backend_tensor_set(sb.kv_idx_in, kvidx_buf.data(), 0, kvidx_buf.size() * sizeof(int64_t));
        ggml_backend_tensor_set(sb.mask_in, mask_buf.data(), 0, mask_buf.size() * sizeof(ggml_fp16_t));
        if (ggml_backend_sched_graph_compute(cc->sched, sb.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run_batch: step compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_tensor_get(sb.out, out_buf.data(), 0, out_buf.size() * sizeof(int32_t));

        all_done = true;
        for (int b = 0; b < n; ++b) {
            if (finished[b] || !valid[b]) {
                continue;
            }
            const int32_t tok = out_buf[b];
            next_tok[b]       = tok;
            n_past[b] += 1;
            if (tok == eos_id) {
                finished[b] = 1;
                continue;
            }  // don't push EOS
            generated[b].push_back(tok);
            if (n_past[b] >= n_audio_b[b]) {
                finished[b] = 1;
                continue;
            }  // clamp (token pushed)
            const size_t base = static_cast<size_t>(b) * max_n_kv;
            if (n_past[b] < max_n_kv) {
                mask_buf[base + n_past[b]] = mz;  // open next position
            }
            all_done = false;
        }
    }
    return TRANSCRIBE_OK;
}

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    auto * cc = static_cast<Session *>(session);
    auto * cm = static_cast<Model *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // The batched causal_lm decoder blocks are flash-only; dump mode and n==1 take
    // the established single-shot path. The batched encoder honors encoder_use_flash
    // (default non-flash = CPU source of truth), so it is not gated here.
    if (!cc->decoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const int              num_delay = cm->hparams.default_num_delay_tokens;
    const int              n_mels    = cm->hparams.enc_num_mel_bins;
    const int              dec_h     = cm->hparams.dec_hidden;
    const int              down      = cm->hparams.proj_downsample;
    const int32_t          eos_id    = cm->hparams.eos_token_id;
    const PromptSpecials & sp        = cm->specials;

    // ----- Pass 0: per-utterance offline-padded mel -----
    std::vector<char>               valid(n, 1);
    // Per-utterance terminal status for rejected rows (default INVALID_ARG;
    // over-context rows below upgrade to INPUT_TOO_LONG).
    std::vector<transcribe_status>  fail_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<float>> mel_bufs(n);  // mel-major [n_mels, mel_nf]
    std::vector<int>                mel_nf(n, 0);
    int                             max_mel = 0;
    const int64_t                   t_mel0  = ggml_time_us();
    for (int b = 0; b < n; ++b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            valid[b] = 0;
            continue;
        }
        const int    raw_per_tok = cm->hparams.audio_length_per_tok * cm->hparams.fe_hop_length;
        const size_t left        = static_cast<size_t>(sp.n_left_pad) * raw_per_tok;
        const size_t right       = static_cast<size_t>(num_delay + 1 + 10) * raw_per_tok;
        const size_t raw_pad     = ((static_cast<size_t>(n_samples[b]) + raw_per_tok - 1) / raw_per_tok) * raw_per_tok;
        const size_t total       = left + raw_pad + right;
        std::vector<float> padded(total, 0.0f);
        std::memcpy(padded.data() + left, pcm[b], static_cast<size_t>(n_samples[b]) * sizeof(float));
        int mm = 0, nf = 0;
        if (cm->mel->compute(padded.data(), total, mel_bufs[b], mm, nf, cc->n_threads) != TRANSCRIBE_OK ||
            mm != n_mels) {
            valid[b] = 0;
            continue;
        }
        mel_nf[b] = nf;
        if (nf > max_mel) {
            max_mel = nf;
        }
    }
    const int64_t mel_us = ggml_time_us() - t_mel0;

    auto emit_all_invalid = [&]() {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    };
    if (max_mel <= 4) {
        return emit_all_invalid();
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    // ----- Pass 1: batched encoder (mels right-padded to max_mel) -----
    std::vector<std::vector<float>> enc_hosts(n);  // [b] -> [dec_h * n_audio_b]
    std::vector<int>                n_audio_b(n, 0);
    const int64_t                   t_enc0 = ggml_time_us();
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        EncoderBuildBatched eb =
            build_encoder_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, max_mel, n, cc->encoder_use_flash);
        if (eb.graph == nullptr || eb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        const int T_enc_max   = eb.T_enc;
        const int n_audio_max = eb.n_audio;

        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run_batch: encoder graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        std::vector<float> mel_in(static_cast<size_t>(n_mels) * max_mel * n, 0.0f);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            const int    nf   = mel_nf[b];
            const size_t boff = static_cast<size_t>(b) * n_mels * max_mel;
            for (int t = 0; t < nf; ++t) {
                for (int mmi = 0; mmi < n_mels; ++mmi) {
                    mel_in[boff + static_cast<size_t>(t) * n_mels + mmi] =
                        mel_bufs[b][static_cast<size_t>(mmi) * nf + t];
                }
            }
        }
        ggml_backend_tensor_set(eb.mel_in, mel_in.data(), 0, mel_in.size() * sizeof(float));

        std::vector<int32_t> positions(T_enc_max);
        for (int i = 0; i < T_enc_max; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(eb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));

        {
            const int                T = T_enc_max, win = cm->hparams.enc_sliding_window;
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(T) * T, mn);
            for (int q = 0; q < T; ++q) {
                for (int k = 0; k <= q; ++k) {
                    if (q - k < win) {
                        mask[static_cast<size_t>(q) * T + k] = mz;
                    }
                }
            }
            ggml_backend_tensor_set(eb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }
        apply_threads(cc->sched, cc->n_threads);
        if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run_batch: encoder compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }

        std::vector<float> proj(static_cast<size_t>(dec_h) * n_audio_max * n);
        ggml_backend_tensor_get(eb.out, proj.data(), 0, proj.size() * sizeof(float));
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            const int T_enc_b = (mel_nf[b] - 2) / 2 + 1;
            int       na      = T_enc_b / down;
            if (na <= 0) {
                valid[b] = 0;
                continue;
            }
            if (na > n_audio_max) {
                na = n_audio_max;
            }
            n_audio_b[b]      = na;
            const size_t boff = static_cast<size_t>(b) * dec_h * n_audio_max;
            enc_hosts[b].assign(proj.begin() + boff, proj.begin() + boff + static_cast<size_t>(dec_h) * na);
        }
    }
    const int64_t enc_us = ggml_time_us() - t_enc0;

    // ----- Pass 2: ada scales (shared; num_delay uniform across the batch) -----
    if (compute_ada_scales(cc, cm, num_delay) != TRANSCRIBE_OK) {
        return TRANSCRIBE_ERR_GGUF;
    }

    // ----- Prompt (uniform) + short-clip gating -----
    std::vector<int32_t> prompt_ids;
    prompt_ids.push_back(sp.bos);
    for (int i = 0; i < sp.n_left_pad + num_delay; ++i) {
        prompt_ids.push_back(sp.streaming_pad);
    }
    const int T_prompt = static_cast<int>(prompt_ids.size());

    // model_max = dec_max_position, the absolute RoPE wall (n_ctx never narrows
    // it). One KV slot per audio token, so a clip whose horizon exceeds model_max
    // can't be partially decoded by clamping the cache — reject up front with
    // INPUT_TOO_LONG, same contract as run().
    const int model_max = voxtral_realtime_abs_position_cap(cm->hparams);
    int       max_audio = 0;
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            continue;
        }
        if (T_prompt >= n_audio_b[b]) {
            valid[b] = 0;
            continue;
        }  // clip too short
        if (model_max > 0 && n_audio_b[b] + 1 > model_max) {  // clip too long
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run_batch: utterance %d input too long — %d "
                                "audio tokens exceed the model's %d-token absolute position cap "
                                "(dec_max_position, ~2.9 h). Shorten the audio. See "
                                "transcribe_capabilities.max_audio_ms.",
                                b, n_audio_b[b], model_max);
            valid[b]       = 0;
            fail_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
            continue;
        }
        if (n_audio_b[b] > max_audio) {
            max_audio = n_audio_b[b];
        }
    }
    if (max_audio <= T_prompt) {
        return emit_all_invalid();
    }

    // ----- Batched KV cache (one slab per row) -----
    // Every valid row now satisfies n_audio_b[b] + 1 <= model_max, so the
    // pow2 width is safely clamped to the ceiling without cutting any decode.
    int max_n_kv = 1024;
    while (max_n_kv < max_audio + 1) {
        max_n_kv *= 2;
    }
    if (model_max > 0 && max_n_kv > model_max) {
        max_n_kv = model_max;
    }
    // Step attention reads only the valid window: the batch's actual max audio
    // length, not the power-of-2 allocation. min() preserves the full window if
    // the alloc was capped. Bit-identical (extra slots are masked).
    const int       read_n_kv = std::min(max_audio, max_n_kv);
    const ggml_type kv_type   = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx < max_n_kv) {
        cc->kv_cache_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_cache_batch, cm->plan.primary, max_n_kv,
                                                    cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                                    cm->hparams.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run_batch: batched KV cache allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        cc->kv_batch_cap   = n;
        cc->kv_batch_n_ctx = max_n_kv;
    }
    if (cc->kv_cache_batch.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache_batch.buffer, 0);
    }

    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, T_prompt);
    std::vector<std::vector<int32_t>> generated(n);

    // ----- Pass 3: batched prefill (rectangular, additive fusion) -----
    const int64_t t_dec0 = ggml_time_us();
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        PrefillBuildBatched pb =
            build_prefill_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache_batch,
                                        cc->ada_scale_all, T_prompt, n, cc->decoder_use_flash);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run_batch: prefill graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        std::vector<int32_t> ids(static_cast<size_t>(T_prompt) * n);
        for (int b = 0; b < n; ++b) {
            for (int t = 0; t < T_prompt; ++t) {
                ids[static_cast<size_t>(b) * T_prompt + t] = prompt_ids[t];
            }
        }
        ggml_backend_tensor_set(pb.input_ids_in, ids.data(), 0, ids.size() * sizeof(int32_t));

        std::vector<float> audio(static_cast<size_t>(dec_h) * T_prompt * n, 0.0f);
        for (int b = 0; b < n; ++b) {
            if (!valid[b]) {
                continue;
            }
            std::memcpy(audio.data() + static_cast<size_t>(b) * T_prompt * dec_h, enc_hosts[b].data(),
                        static_cast<size_t>(dec_h) * T_prompt * sizeof(float));
        }
        ggml_backend_tensor_set(pb.audio_dense_in, audio.data(), 0, audio.size() * sizeof(float));

        std::vector<int32_t> positions(T_prompt);
        for (int t = 0; t < T_prompt; ++t) {
            positions[t] = t;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));

        {
            const ggml_fp16_t        mz = ggml_fp32_to_fp16(0.0f), mn = ggml_fp32_to_fp16(-INFINITY);
            std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, mn);
            for (int r = 0; r < T_prompt; ++r) {
                for (int c = 0; c <= r; ++c) {
                    mask[static_cast<size_t>(r) * T_prompt + c] = mz;
                }
            }
            ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
        }

        std::vector<int64_t> kvidx(static_cast<size_t>(T_prompt) * n);
        for (int b = 0; b < n; ++b) {
            for (int t = 0; t < T_prompt; ++t) {
                kvidx[static_cast<size_t>(b) * T_prompt + t] = t;
            }
        }
        ggml_backend_tensor_set(pb.kv_idx_in, kvidx.data(), 0, kvidx.size() * sizeof(int64_t));

        std::vector<int32_t> last_idx(n, T_prompt - 1);
        ggml_backend_tensor_set(pb.last_idx_in, last_idx.data(), 0, last_idx.size() * sizeof(int32_t));

        apply_threads(cc->sched, cc->n_threads);
        if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime run_batch: prefill compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }
        std::vector<int32_t> first(n, 0);
        ggml_backend_tensor_get(pb.out, first.data(), 0, first.size() * sizeof(int32_t));
        for (int b = 0; b < n; ++b) {
            next_tok[b] = first[b];
            if (valid[b]) {
                generated[b].push_back(first[b]);
            }
        }
    }

    // ----- Pass 4: batched step loop -----
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
                                "voxtral_realtime: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        StepBuildBatched sb = build_step_graph_batched(cc->compute_ctx, cm->weights, cm->hparams, cc->kv_cache_batch,
                                                       cc->ada_scale_all, read_n_kv, n, cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "voxtral_realtime run_batch: step graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        apply_threads(cc->sched, cc->n_threads);

        const transcribe_status st = run_batch_step_loop(cc, sb, n, read_n_kv, eos_id, dec_h, enc_hosts, n_audio_b,
                                                         valid, next_tok, n_past, generated);
        if (st != TRANSCRIBE_OK) {
            return st;
        }
    }
    const int64_t dec_us = ggml_time_us() - t_dec0;

    // ----- Capture per-utterance results -----
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            // INVALID_ARG (malformed / clip-too-short) or INPUT_TOO_LONG
            // (rejected over-context row), per fail_status.
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::string                   text = detok_generated(cm, generated[b]);
        transcribe_session::ResultSet rs;
        rs.full_text   = text;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        // Valid rows fit the context (over-context rows were rejected up front
        // as INPUT_TOO_LONG), so a completed row is OK.
        rs.status      = TRANSCRIBE_OK;
        transcribe_session::SegmentEntry seg{};
        seg.text  = text;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(n_samples[b]) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
        rs.segments.push_back(std::move(seg));
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = dec_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }
    return TRANSCRIBE_OK;
}

extern const Arch arch = {
    /* .name             = */ "voxtral_realtime",
    /* .load             = */ load,
    /* .init_context     = */ init_context,
    /* .run              = */ run,
    /* .run_batch        = */ run_batch,
    /* .stream_validate  = */ stream_validate,
    /* .stream_begin     = */ stream_begin,
    /* .stream_feed      = */ stream_feed,
    /* .stream_finalize  = */ stream_finalize,
    /* .stream_reset     = */ stream_reset,
    /* .accepts_ext_kind = */ accepts_ext_kind,
};

}  // namespace transcribe::voxtral_realtime

// Public streaming-extension initializer (C ABI). Mirrors the per-family
// pattern used by parakeet / moonshine_streaming.
extern "C" void transcribe_voxtral_realtime_stream_ext_init(struct transcribe_voxtral_realtime_stream_ext * p) {
    if (p == nullptr) {
        return;
    }
    std::memset(p, 0, sizeof(*p));
    p->ext.size               = sizeof(*p);
    p->ext.kind               = TRANSCRIBE_EXT_KIND_VOXTRAL_REALTIME_STREAM;
    p->num_delay_tokens       = -1;
    p->min_decode_interval_ms = -1;
}
