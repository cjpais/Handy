// arch/moonshine/model.cpp - Moonshine ASR family handler.
//
// Load / init_context / run lifecycle for the Moonshine encoder-decoder
// model. Greedy generation only: the upstream generation_config is
// `do_sample=False, num_beams=1, max_length=194`; there are no
// suppress_tokens, no language tokens, no temperature fallback.

#include "decoder.h"
#include "encoder.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "moonshine.h"
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

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

namespace transcribe::moonshine {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, MoonshineModel>);
static_assert(std::is_base_of_v<transcribe_session, MoonshineSession>);

MoonshineSession::~MoonshineSession() {
    kv_cache.free();
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

MoonshineModel::~MoonshineModel() {
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

// Input-length contract (see docs/input-limits.md): the wall is on *output*,
// not input. The conv-stem encoder takes any PCM length, so a clip is never
// rejected; the greedy decode loop instead stops at the decoder position cap
// (dec_max_position_embeddings = 194) before EOS and the transcript silently
// truncates. On a cap-exit we set transcribe_was_truncated() and WARN (same as
// qwen3_asr). max_audio_ms below is a conservative advisory only.

// Advisory transcribe_capabilities::max_audio_ms: the audio the output budget
// (dec_max_position_embeddings tokens) covers at ~4 output tokens/sec (~48 s
// for 194 tokens). 0 means unknown/unbounded. Advisory only — does not reject.
constexpr int k_tokens_per_sec = 4;  // rough speech-rate estimate; advisory only

int64_t moonshine_max_audio_ms(const MoonshineHParams & hp) {
    if (hp.dec_max_position_embeddings <= 0) {
        return 0;
    }
    return static_cast<int64_t>(hp.dec_max_position_embeddings) * 1000 / k_tokens_per_sec;
}

// KV cache initialization.
bool kv_cache_init(MoonshineKvCache & cache,
                   ggml_backend_t     backend,
                   int                n_ctx,
                   int                T_enc,
                   int                d_model,
                   int                n_layer,
                   ggml_type          kv_type) {
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "moonshine kv_cache: unsupported kv_type=%d "
                "(only F16/F32)",
                static_cast<int>(kv_type));
        return false;
    }

    const size_t     ctx_size = 4 * ggml_tensor_overhead() + 256;
    ggml_init_params params{ ctx_size, nullptr, /*no_alloc=*/true };
    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine kv_cache: ggml_init failed");
        return false;
    }

    const int64_t self_elements  = static_cast<int64_t>(d_model) * n_layer * n_ctx;
    const int64_t cross_elements = static_cast<int64_t>(d_model) * n_layer * T_enc;

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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine kv_cache: buffer alloc failed");
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);

    cache.n_ctx           = n_ctx;
    cache.T_enc           = T_enc;
    cache.n               = 0;
    cache.head            = 0;
    cache.cross_populated = false;

    return true;
}

bool kv_cache_init_batched(MoonshineKvCache & cache,
                           ggml_backend_t     backend,
                           int                n_ctx,
                           int                T_enc,
                           int                d_model,
                           int                n_layer,
                           int                n_batch,
                           ggml_type          kv_type) {
    if (n_batch <= 1) {
        if (!kv_cache_init(cache, backend, n_ctx, T_enc, d_model, n_layer, kv_type)) {
            return false;
        }
        cache.n_batch = 1;
        return true;
    }
    if (kv_type != GGML_TYPE_F16 && kv_type != GGML_TYPE_F32) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine kv_cache(batched): unsupported kv_type");
        return false;
    }
    const size_t     ctx_size = 4 * ggml_tensor_overhead() + 256;
    ggml_init_params params{ ctx_size, nullptr, /*no_alloc=*/true };
    cache.ctx = ggml_init(params);
    if (cache.ctx == nullptr) {
        return false;
    }

    const int64_t self_elements  = static_cast<int64_t>(d_model) * n_layer * n_ctx * n_batch;
    const int64_t cross_elements = static_cast<int64_t>(d_model) * n_layer * T_enc * n_batch;
    cache.self_k                 = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.self_v                 = ggml_new_tensor_1d(cache.ctx, kv_type, self_elements);
    cache.cross_k                = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    cache.cross_v                = ggml_new_tensor_1d(cache.ctx, kv_type, cross_elements);
    ggml_set_name(cache.self_k, "kv_self_k");
    ggml_set_name(cache.self_v, "kv_self_v");
    ggml_set_name(cache.cross_k, "kv_cross_k");
    ggml_set_name(cache.cross_v, "kv_cross_v");

    cache.buffer = ggml_backend_alloc_ctx_tensors(cache.ctx, backend);
    if (cache.buffer == nullptr) {
        ggml_free(cache.ctx);
        cache.ctx = nullptr;
        return false;
    }
    ggml_backend_buffer_clear(cache.buffer, 0);
    cache.n_ctx           = n_ctx;
    cache.T_enc           = T_enc;
    cache.n               = 0;
    cache.head            = 0;
    cache.n_batch         = n_batch;
    cache.cross_populated = false;
    return true;
}

namespace {

constexpr const char k_default_variant[] = "moonshine";

extern transcribe_status load(Loader &, const transcribe_model_load_params *, transcribe_model **);
extern transcribe_status init_context(transcribe_model *, const transcribe_session_params *, transcribe_session **);
extern transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<MoonshineModel>();
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
    if (auto st = read_moonshine_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the advisory window now that the decoder position cap is known
    // (first point it's available after hparams).
    m->caps.max_audio_ms = moonshine_max_audio_ms(m->hparams);

    // Reopen GGUF with no_alloc to wire weight slots.
    gguf_init_params init_params{};
    init_params.no_alloc     = true;
    init_params.ctx          = &m->ctx_meta;
    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_moonshine_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = transcribe::load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0,
                                                         "moonshine", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "moonshine");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

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
    auto cc       = std::make_unique<MoonshineSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;

    // See MoonshineSession for why decoder FA is Metal-only-off.
    auto *     cm         = static_cast<MoonshineModel *>(model);
    const bool is_metal   = (cm->plan.primary_kind == transcribe::BackendKind::Metal);
    cc->encoder_use_flash = true;
    cc->decoder_use_flash = !is_metal;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

// Set per-backend thread count from cc->n_threads (with a sensible default).
void apply_thread_policy(MoonshineSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

bool ensure_compute_ctx(MoonshineSession * cc, size_t mem_size) {
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    ggml_init_params init_params{};
    init_params.mem_size   = mem_size;
    init_params.mem_buffer = nullptr;
    init_params.no_alloc   = true;
    cc->compute_ctx        = ggml_init(init_params);
    return cc->compute_ctx != nullptr;
}

// Search compute_ctx for a tensor by name. Used to locate the causal
// mask input tensor when the prompt pass needs one. Cohere uses the
// same trick.
ggml_tensor * find_tensor_by_name(ggml_context * gctx, const char * name) {
    for (ggml_tensor * t = ggml_get_first_tensor(gctx); t != nullptr; t = ggml_get_next_tensor(gctx, t)) {
        if (std::strcmp(t->name, name) == 0) {
            return t;
        }
    }
    return nullptr;
}

transcribe_status run(transcribe_session *          session,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    (void) params;

    if (session == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<MoonshineSession *>(session);
    auto * cm = static_cast<MoonshineModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();
    cc->clear_result();

    const auto & hp = cm->hparams;

    // Phase timers. Moonshine has no mel/STFT (raw PCM passthrough), so
    // t_mel_us stays 0; t_encode_us covers encoder graph + cross-KV
    // precompute, t_decode_us covers prompt pass + step loop.
    cc->t_mel_us                 = 0;
    cc->t_encode_us              = 0;
    cc->t_decode_us              = 0;
    const int64_t t_encode_start = ggml_time_us();

    ggml_type resolved_kv = GGML_TYPE_COUNT;
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
        resolved_kv = GGML_TYPE_F32;
    }
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        resolved_kv = GGML_TYPE_F16;
    }

    // ----- Encoder: build + compute -----
    if (!ensure_compute_ctx(cc, 8 * 1024 * 1024)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "moonshine run: compute context allocation failed (encoder) — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, n_samples, cc->encoder_use_flash);
    if (eb.audio_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    const int T_enc = eb.T_enc;

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "moonshine run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.audio_in, pcm, 0, static_cast<size_t>(n_samples) * sizeof(float));
    {
        std::vector<int32_t> enc_pos_ids(static_cast<size_t>(T_enc));
        for (int i = 0; i < T_enc; ++i) {
            enc_pos_ids[i] = i;
        }
        ggml_backend_tensor_set(eb.pos_ids_in, enc_pos_ids.data(), 0, enc_pos_ids.size() * sizeof(int32_t));
    }

    apply_thread_policy(cc);

    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: encoder compute failed");
        return TRANSCRIBE_ERR_GGUF;
    }

    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.audio.in", eb.dumps.audio_in, "encoder.audio.in");
    try_dump("enc.conv1.out", eb.dumps.conv1_out, "encoder.conv1");
    try_dump("enc.groupnorm.out", eb.dumps.groupnorm_out, "encoder.groupnorm");
    try_dump("enc.conv2.out", eb.dumps.conv2_out, "encoder.conv2");
    try_dump("enc.conv3.out", eb.dumps.conv3_out, "encoder.conv3");
    for (size_t i = 0; i < eb.dumps.block_outs.size(); ++i) {
        char bname[64], stage[64];
        std::snprintf(bname, sizeof(bname), "enc.block.%zu.out", i);
        std::snprintf(stage, sizeof(stage), "encoder.block%zu.out", i);
        try_dump(bname, eb.dumps.block_outs[i], stage);
    }
    try_dump("enc.final", eb.dumps.final_out, "encoder.final");

    // Read encoder output to host buffer (for re-upload into the
    // cross_kv graph's input tensor — it lives in a fresh compute_ctx).
    const int d_model = hp.dec_d_model;
    cc->enc_T         = T_enc;
    cc->enc_host.assign(static_cast<size_t>(d_model) * static_cast<size_t>(T_enc), 0.0f);
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    // ----- KV cache init -----
    {
        if (cc->kv_cache.buffer != nullptr && cc->kv_cache.T_enc != T_enc) {
            cc->kv_cache.free();
        }
        if (cc->kv_cache.buffer == nullptr) {
            const int n_ctx      = hp.dec_max_position_embeddings > 0 ? hp.dec_max_position_embeddings : 512;
            ggml_type cache_type = resolved_kv;
            // Default the cache to F32 to match moonshine's reference regime.
            if (cache_type == GGML_TYPE_COUNT) {
                cache_type = GGML_TYPE_F32;
            }
            if (!kv_cache_init(cc->kv_cache, cm->plan.primary, n_ctx, T_enc, d_model, hp.dec_n_layers, cache_type)) {
                transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                    "moonshine run: KV cache allocation failed — out of memory.");
                return TRANSCRIBE_ERR_OOM;
            }
        } else {
            cc->kv_cache.n               = 0;
            cc->kv_cache.head            = 0;
            cc->kv_cache.cross_populated = false;
        }
    }

    // ----- Cross-KV precompute -----
    {
        if (!ensure_compute_ctx(cc, 4 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: compute context allocation failed (cross_kv) — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        DecoderBuild cross_db = build_cross_kv_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_enc);
        if (cross_db.graph == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, cross_db.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: cross_kv graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        ggml_backend_tensor_set(cross_db.encoder_out_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
        if (ggml_backend_sched_graph_compute(cc->sched, cross_db.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: cross_kv compute failed");
            return TRANSCRIBE_ERR_GGUF;
        }
        cc->kv_cache.cross_populated = true;
    }

    cc->t_encode_us = ggml_time_us() - t_encode_start;

    // ----- Greedy decoder loop -----
    const int64_t t_decode_start = ggml_time_us();
    const int     decoder_start  = hp.decoder_start_token_id;  // 1
    const int     eos            = hp.eos_token_id;            // 2
    const int     max_pos        = hp.dec_max_position_embeddings;

    std::vector<int32_t> generated_ids;
    int                  next_token = -1;
    int                  n_past     = 0;

    auto run_step = [&](int n_tokens, int n_past_in, int token_id_first, bool dump_prompt,
                        const char * mid_gen_dump_name) -> transcribe_status {
        // Build a fresh compute_ctx + step graph. With dump_prompt=true
        // the graph emits the full log_softmax tensor so the dumper can
        // capture dec.logits; otherwise we skip log_softmax (argmax is
        // invariant) and read raw logits.
        if (!ensure_compute_ctx(cc, 4 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: compute context allocation failed (step) — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        const bool   skip_log_softmax = !dump_prompt;
        DecoderBuild db = build_decoder_graph_kv(cc->compute_ctx, cm->weights, hp, cc->kv_cache, n_tokens, n_past_in,
                                                 T_enc, skip_log_softmax, cc->decoder_use_flash);
        if (db.out == nullptr || db.graph == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, db.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: step graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        // Upload tokens + position ids.
        std::vector<int32_t> token_ids(static_cast<size_t>(n_tokens));
        std::vector<int32_t> pos_ids(static_cast<size_t>(n_tokens));
        for (int i = 0; i < n_tokens; ++i) {
            token_ids[i] = (i == 0) ? token_id_first : 0 /* unused: caller batches one token */;
            pos_ids[i]   = n_past_in + i;
        }
        ggml_backend_tensor_set(db.token_ids_in, token_ids.data(), 0, token_ids.size() * sizeof(int32_t));
        ggml_backend_tensor_set(db.pos_ids_in, pos_ids.data(), 0, pos_ids.size() * sizeof(int32_t));
        // Causal mask only when n_tokens > 1 (every step is single-token).
        if (n_tokens > 1) {
            ggml_tensor * m = find_tensor_by_name(cc->compute_ctx, "dec.causal_mask");
            if (m != nullptr) {
                const int          n_kv = n_past_in + n_tokens;
                std::vector<float> mask(static_cast<size_t>(n_kv) * n_tokens, -1e9f);
                for (int q = 0; q < n_tokens; ++q) {
                    for (int k = 0; k < n_kv; ++k) {
                        if (k <= q + n_past_in) {
                            mask[static_cast<size_t>(q) * n_kv + k] = 0.0f;
                        }
                    }
                }
                ggml_backend_tensor_set(m, mask.data(), 0, mask.size() * sizeof(float));
            }
        }

        if (ggml_backend_sched_graph_compute(cc->sched, db.graph) != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: decoder compute failed (n_past=%d)", n_past_in);
            return TRANSCRIBE_ERR_GGUF;
        }

        if (dump_prompt) {
            try_dump("dec.token_emb", db.dumps.token_emb, "decoder.embedding");
            try_dump("dec.embed_sum", db.dumps.embed_sum, "decoder.embed_sum");
            for (size_t i = 0; i < db.dumps.block_outs.size(); ++i) {
                char bname[64], stage[64];
                std::snprintf(bname, sizeof(bname), "dec.block.%zu.out", i);
                std::snprintf(stage, sizeof(stage), "decoder.block%zu.out", i);
                try_dump(bname, db.dumps.block_outs[i], stage);
            }
            try_dump("dec.out_before_head", db.dumps.out_before_head, "decoder.output_before_head");
            try_dump("dec.logits_raw", db.dumps.logits_raw, "decoder.logits_raw");
            try_dump("dec.logits", db.dumps.logits, "decoder.logits");
        } else if (mid_gen_dump_name != nullptr) {
            // Reference dumper writes pre-softmax logits at gen step 20
            // under the name dec.logits_raw.gen20. The C++ step graph's
            // out tensor IS the raw logits when skip_log_softmax=true.
            try_dump(mid_gen_dump_name, db.out, "decoder.logits_raw.gen");
        }

        // Pick next_token. Fast path: argmax tensor computed on-device
        // (every step pass uses skip_log_softmax=true, so argmax_out is
        // populated). Slow path: prompt pass dumps full log_softmax;
        // argmax in C against the downloaded vocab row.
        if (db.argmax_out != nullptr) {
            int32_t argmax_id = 0;
            ggml_backend_tensor_get(db.argmax_out, &argmax_id, 0, sizeof(int32_t));
            next_token = argmax_id;
        } else {
            const int64_t      vocab_size = db.out->ne[0];
            const int64_t      last_T     = (ggml_n_dims(db.out) == 1) ? 1 : db.out->ne[1];
            std::vector<float> logits_host(static_cast<size_t>(vocab_size) * static_cast<size_t>(last_T));
            ggml_backend_tensor_get(db.out, logits_host.data(), 0, logits_host.size() * sizeof(float));
            const float * last_logits = logits_host.data() + static_cast<size_t>(last_T - 1) * vocab_size;
            int           best_idx    = 0;
            float         best_v      = last_logits[0];
            for (int j = 1; j < static_cast<int>(vocab_size); ++j) {
                if (last_logits[j] > best_v) {
                    best_v   = last_logits[j];
                    best_idx = j;
                }
            }
            next_token = best_idx;
        }

        cc->kv_cache.n    = n_past_in + n_tokens;
        cc->kv_cache.head = cc->kv_cache.n;
        n_past            = cc->kv_cache.n;
        return TRANSCRIBE_OK;
    };

    // Prompt pass: a single decoder_start_token_id (=1, <s>).
    if (auto st = run_step(/*n_tokens=*/1, /*n_past=*/0,
                           /*token_id_first=*/decoder_start,
                           /*dump_prompt=*/true,
                           /*mid_gen_dump_name=*/nullptr);
        st != TRANSCRIBE_OK) {
        return st;
    }
    if (next_token != eos) {
        generated_ids.push_back(next_token);
    }

    // Step loop.
    //
    // Two variants:
    //   GPU (Vulkan/Metal/CUDA/SYCL): build_step_graph — one static-
    //     topology graph for the whole utterance. KV writes via
    //     ggml_set_rows at runtime kv_idx; flash-attn reads a fixed
    //     max_n_kv window with a runtime mask. Removes per-step
    //     graph_build + sched_alloc.
    //   CPU (or debug, which needs the mid-gen dump path): per-step
    //     run_step with build_decoder_graph_kv.
    constexpr int k_mid_gen_step = 20;
    const bool    primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                   cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                   cm->plan.primary_kind != transcribe::BackendKind::Unknown;
    const bool    use_step_graph = primary_is_gpu && !transcribe::debug::enabled();

    if (use_step_graph) {
        // ---------- Static-graph step path (GPU) ----------
        // max_n_kv: pad max_pos to next power of two (no fixed floor —
        // moonshine's max_pos is ~194, so a 1024 floor would waste 5×
        // attention bandwidth per step).
        int max_n_kv = 64;
        while (max_n_kv < max_pos) {
            max_n_kv *= 2;
        }
        if (max_n_kv > cc->kv_cache.n_ctx) {
            max_n_kv = cc->kv_cache.n_ctx;
        }

        if (!ensure_compute_ctx(cc, 8 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: compute context allocation failed (step) — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        StepBuild sb =
            build_step_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, max_n_kv, T_enc, cc->decoder_use_flash);
        if (sb.graph == nullptr || sb.argmax_out == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: build_step_graph failed");
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run: step graph allocation failed — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }

        // Mask buffer: [0, n_past) populated by the prompt pass start
        // attendable; remaining slots -inf until each step flips its
        // newly-written position to attendable.
        const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
        const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> step_mask(max_n_kv, mask_neg_inf);
        for (int p = 0; p < n_past; ++p) {
            step_mask[p] = mask_zero;
        }

        while (next_token != eos && n_past < max_pos) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }
            if (n_past + 1 > max_n_kv) {
                log_msg(TRANSCRIBE_LOG_LEVEL_INFO, "moonshine run: hit max_n_kv=%d at n_past=%d", max_n_kv, n_past);
                break;
            }

            int32_t token_val = next_token;
            int32_t pos_val   = n_past;
            int64_t kv_val    = n_past;
            ggml_backend_tensor_set(sb.token_id_in, &token_val, 0, sizeof(int32_t));
            ggml_backend_tensor_set(sb.pos_id_in, &pos_val, 0, sizeof(int32_t));
            ggml_backend_tensor_set(sb.kv_idx_in, &kv_val, 0, sizeof(int64_t));

            step_mask[n_past] = mask_zero;
            ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0,
                                    static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));

            if (ggml_backend_sched_graph_compute(cc->sched, sb.graph) != GGML_STATUS_SUCCESS) {
                log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "moonshine run: step compute failed (n_past=%d)", n_past);
                return TRANSCRIBE_ERR_GGUF;
            }

            n_past += 1;
            cc->kv_cache.n    = n_past;
            cc->kv_cache.head = n_past;

            int32_t argmax_id = 0;
            ggml_backend_tensor_get(sb.argmax_out, &argmax_id, 0, sizeof(int32_t));
            next_token = argmax_id;

            if (next_token != eos) {
                generated_ids.push_back(next_token);
            }
        }
    } else {
        // ---------- Dynamic-graph step path (CPU / debug) ----------
        while (next_token != eos && n_past < max_pos) {
            if (cc->poll_abort()) {
                return TRANSCRIBE_ERR_ABORTED;
            }

            // The mid-generation dump fires when the cache holds 20 tokens
            // (= prompt + 19 generated, so this step predicts the 21st
            // token = generated_ids[19]) — matching the reference dumper's
            // gen_step_n=20 contract.
            const bool   is_mid_gen = (n_past == k_mid_gen_step);
            const char * dump_name  = is_mid_gen ? "dec.logits_raw.gen20" : nullptr;

            if (auto st = run_step(/*n_tokens=*/1, /*n_past=*/n_past,
                                   /*token_id_first=*/next_token,
                                   /*dump_prompt=*/false,
                                   /*mid_gen_dump_name=*/dump_name);
                st != TRANSCRIBE_OK) {
                return st;
            }
            if (next_token != eos) {
                generated_ids.push_back(next_token);
            }
        }
    }

    // A non-eos last token means the decode hit the position cap before
    // end-of-stream (see the input-length contract above): flag truncation
    // and WARN.
    if (next_token != eos) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "moonshine run: output truncated at %d tokens — decode reached the "
                            "position cap (%d) before end-of-stream; the transcript may be "
                            "incomplete. This model is intended for short utterances. See "
                            "transcribe_capabilities.max_audio_ms.",
                            static_cast<int>(generated_ids.size()), max_pos);
    }

    cc->t_decode_us = ggml_time_us() - t_decode_start;

    if (!generated_ids.empty()) {
        std::string full = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));
        // SentencePiece-style BPE leading ▁ → ' '; the decode helper
        // already replaces those, but the very first token typically
        // produces a leading space. Trim it.
        if (!full.empty() && full.front() == ' ') {
            full.erase(full.begin());
        }

        transcribe_session::SegmentEntry seg;
        seg.t0_ms       = 0;
        seg.t1_ms       = 0;
        seg.first_token = 0;
        seg.n_tokens    = 0;
        seg.first_word  = 0;
        seg.n_words     = 0;
        seg.text        = full;

        cc->segments.push_back(std::move(seg));
        cc->full_text   = std::move(full);
        cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        cc->has_result  = true;
    }

    // Truncation is a hard status; the partial transcript stays readable.
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

// Offline batched decode (transcribe_run_batch). Mirrors src/arch/cohere +
// canary. Encoder serial per utterance (raw PCM, no mel to parallelize);
// decode (self+cross attn, partial RoPE) batched. Requires the flash step
// path, so on Metal (decoder flash OFF — head_dim_padded unsupported) it falls
// back to serial; engages on Vulkan/CUDA.

transcribe_status encode_one_to_host(MoonshineSession *   cc,
                                     MoonshineModel *     cm,
                                     const float *        pcm,
                                     int                  n_samples,
                                     std::vector<float> & enc_out,
                                     int &                T_enc_out,
                                     int64_t &            enc_us) {
    if (pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const auto & hp = cm->hparams;

    if (!ensure_compute_ctx(cc, 8 * 1024 * 1024)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "moonshine encode: compute context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, n_samples, cc->encoder_use_flash);
    if (eb.audio_in == nullptr || eb.out == nullptr || eb.graph == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    const int T_enc = eb.T_enc;
    if (T_enc <= 0) {
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
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "moonshine encode: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.audio_in, pcm, 0, static_cast<size_t>(n_samples) * sizeof(float));
    {
        std::vector<int32_t> enc_pos_ids(static_cast<size_t>(T_enc));
        for (int i = 0; i < T_enc; ++i) {
            enc_pos_ids[i] = i;
        }
        ggml_backend_tensor_set(eb.pos_ids_in, enc_pos_ids.data(), 0, enc_pos_ids.size() * sizeof(int32_t));
    }
    apply_thread_policy(cc);

    const int64_t t0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t0;

    const int d_model = hp.dec_d_model;
    enc_out.assign(static_cast<size_t>(d_model) * T_enc, 0.0f);
    ggml_backend_tensor_get(eb.out, enc_out.data(), 0, enc_out.size() * sizeof(float));
    T_enc_out = T_enc;
    return TRANSCRIBE_OK;
}

transcribe_status run_batch_serial(MoonshineSession *            cc,
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
    auto * cc = static_cast<MoonshineSession *>(session);
    auto * cm = static_cast<MoonshineModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const bool primary_is_gpu = cm->plan.primary_kind != transcribe::BackendKind::Cpu &&
                                cm->plan.primary_kind != transcribe::BackendKind::Accel &&
                                cm->plan.primary_kind != transcribe::BackendKind::Unknown;
    // The batched path always uses flash (unconditional), even on Metal where
    // single-shot defaults decoder flash OFF (base's head_dim_padded=56
    // CPU-spills): at B>1 flash still beats serial (tiny ~1.7×, base ~1.17×).
    if (n == 1 || !primary_is_gpu || transcribe::debug::enabled()) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto &  hp            = cm->hparams;
    const int     d_model       = hp.dec_d_model;
    const int     n_layer       = hp.dec_n_layers;
    const int     decoder_start = hp.decoder_start_token_id;
    const int32_t eos           = hp.eos_token_id;
    const int     max_pos       = hp.dec_max_position_embeddings;

    // ----- Serial per-utterance encoder (no mel; raw PCM) -----
    std::vector<char>               valid(n, 0);
    std::vector<std::vector<float>> enc_hosts(n);
    std::vector<int>                T_enc(n, 0);
    int                             T_enc_max = 0;
    int64_t                         enc_us    = 0;
    for (int b = 0; b < n; ++b) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            continue;
        }
        if (encode_one_to_host(cc, cm, pcm[b], n_samples[b], enc_hosts[b], T_enc[b], enc_us) != TRANSCRIBE_OK ||
            T_enc[b] <= 0) {
            continue;
        }
        valid[b]  = 1;
        T_enc_max = std::max(T_enc_max, T_enc[b]);
    }
    if (T_enc_max <= 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }

    // ----- Batched KV cache -----
    int max_n_kv = 64;
    while (max_n_kv < max_pos) {
        max_n_kv *= 2;
    }
    // Match serial run(): default the cache to F32 (AUTO), F16 only on
    // explicit request. moonshine's reference regime is F32, and ggml's
    // Metal backend lacks a working kernel_pad_f16 for the head-dim pad.
    ggml_type kv_type = GGML_TYPE_F32;
    if (cc->kv_type == TRANSCRIBE_KV_TYPE_F16) {
        kv_type = GGML_TYPE_F16;
    }
    if (cc->kv_cache.buffer != nullptr &&
        (cc->kv_cache.n_batch != n || cc->kv_cache.T_enc != T_enc_max || cc->kv_cache.n_ctx != max_n_kv)) {
        cc->kv_cache.free();
    }
    if (cc->kv_cache.buffer == nullptr) {
        if (!kv_cache_init_batched(cc->kv_cache, cm->plan.primary, max_n_kv, T_enc_max, d_model, n_layer, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run_batch: batched KV cache allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    } else {
        ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        cc->kv_cache.n               = 0;
        cc->kv_cache.head            = 0;
        cc->kv_cache.cross_populated = false;
    }

    const int64_t t_dec0 = ggml_time_us();

    // ----- Batched cross-attention K/V -----
    {
        if (!ensure_compute_ctx(cc, 8 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run_batch: compute context allocation failed "
                                "(cross_kv) — out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        DecoderBuild cross = build_cross_kv_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_enc_max, n);
        if (cross.graph == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, cross.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run_batch: cross_kv graph allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
        std::vector<float> packed(static_cast<size_t>(d_model) * T_enc_max * n, 0.0f);
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

    // ----- Batched step graph + shared greedy step loop -----
    // moonshine is the degenerate enc-dec case: a single decoder_start prompt
    // token and a fixed read window (self-KV spans the whole sequence, so
    // init_window == max_n_kv and the shared driver never grows it).
    const ggml_fp16_t        f16_zero = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        f16_ninf = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> cmask(static_cast<size_t>(T_enc_max) * n, f16_ninf);
    for (int b = 0; b < n; ++b) {
        const int     real = valid[b] ? T_enc[b] : 1;
        ggml_fp16_t * base = cmask.data() + static_cast<size_t>(b) * T_enc_max;
        std::fill(base, base + std::min(real, T_enc_max), f16_zero);
    }

    StepBuildBatched sb{};
    auto             rebuild = [&](int win, transcribe::EncDecStepIO & io) -> bool {
        if (!ensure_compute_ctx(cc, 16 * 1024 * 1024)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run_batch: compute context allocation failed "
                                "(step) — out of memory.");
            return false;
        }
        sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache, win, T_enc_max, n,
                                      /*use_flash=*/true);
        if (sb.graph == nullptr || sb.argmax_out == nullptr) {
            return false;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "moonshine run_batch: step graph allocation failed — "
                                "out of memory.");
            return false;
        }
        ggml_backend_tensor_set(sb.cross_mask_in, cmask.data(), 0, cmask.size() * sizeof(ggml_fp16_t));
        io.token_ids = sb.token_ids_in;
        io.pos_ids   = sb.pos_ids_in;
        io.kv_idx    = sb.kv_idx_in;
        io.self_mask = sb.self_mask_in;
        io.argmax    = sb.argmax_out;
        io.graph     = sb.graph;
        return true;
    };

    const std::vector<int32_t>        prompt_ids = { static_cast<int32_t>(decoder_start) };
    std::vector<std::vector<int32_t>> generated(n);
    std::vector<char>                 truncated;
    if (const transcribe_status st = transcribe::run_batched_encdec_step_loop(
            cc, cc->sched, rebuild, prompt_ids, /*prompt_len=*/1,
            /*init_window=*/max_n_kv, /*max_new=*/max_pos, max_n_kv, static_cast<int32_t>(eos), n, valid, generated,
            /*n_steps_out=*/nullptr, &truncated);
        st != TRANSCRIBE_OK) {
        return st;
    }
    const int64_t dec_us = ggml_time_us() - t_dec0;

    // Batched truncation: the shared step loop marks each valid row that hit
    // the output cap before end-of-stream. Mirror the serial path (WARN + flag).
    {
        int n_truncated = 0;
        for (int b = 0; b < n; ++b) {
            if (b < static_cast<int>(truncated.size()) && truncated[b]) {
                ++n_truncated;
            }
        }
        if (n_truncated > 0) {
            cc->was_truncated = true;
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                                "moonshine run_batch: %d of %d utterances truncated — decode "
                                "reached the position cap (%d) before end-of-stream; those "
                                "transcripts may be incomplete. This model is intended for "
                                "short utterances. See transcribe_capabilities.max_audio_ms.",
                                n_truncated, n, max_pos);
        }
    }

    // ----- Capture -----
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        transcribe_session::ResultSet rs;
        if (!valid[b]) {
            rs.status = TRANSCRIBE_ERR_INVALID_ARG;
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::string full = cm->tok.decode(generated[b].data(), static_cast<int>(generated[b].size()));
        if (!full.empty() && full.front() == ' ') {
            full.erase(full.begin());
        }
        transcribe_session::SegmentEntry seg{};
        seg.t0_ms = 0;
        seg.t1_ms = 0;
        seg.text  = full;
        rs.segments.push_back(std::move(seg));
        rs.full_text   = full;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        rs.status      = TRANSCRIBE_OK;
        // Per-utterance truncation parity with the single-shot path. Only
        // override an otherwise-OK status — never a worse one.
        if (rs.status == TRANSCRIBE_OK && b < static_cast<int>(truncated.size()) && truncated[b]) {
            rs.status = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = 0;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = dec_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }

    if (transcribe::env::flag("TRANSCRIBE_PERF_DEBUG")) {
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "moonshine run_batch: n=%d T_enc_max=%d max_n_kv=%d\n"
                "  enc=%.1fms (serial x%d)  decode=%.1fms (batched)",
                n, T_enc_max, max_n_kv, enc_us / 1000.0, n, dec_us / 1000.0);
    }
    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "moonshine",
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

}  // namespace transcribe::moonshine
