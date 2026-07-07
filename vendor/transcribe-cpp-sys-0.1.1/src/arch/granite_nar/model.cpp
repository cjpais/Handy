// arch/granite_nar/model.cpp - IBM Granite Speech NLE (NAR) family handler.
//
// Architecture:
//   - Conformer encoder (16 layers, hidden=1024) with self-conditioned
//     CTC bypass at layer N/2 and a BPE-CTC head
//   - EncoderProjectorQFormer (per-encoder-layer LN over the cat of 4
//     captured layers, layer_projector 4096->2048, learned query +
//     window_positions, 2 cross-attn+MLP layers, out_norm + out_linear)
//   - Bidirectional Granite-4 1b base LM (40 layers, GQA 16/4, single
//     forward pass — no KV cache, is_causal=False)
//
// The forward composes in one run() call as:
//   PCM -> mel -> 2-frame stack -> encoder graph (emit cat_out,
//     ctc_logits, ctc_bpe_logits) -> host CTC pool + greedy decode of
//     the BPE head -> initial hypothesis -> add_insertion_slots ->
//     text_ids -> decoder forward graph (with projector audio embeds
//     concatenated to the front of inputs_embeds) -> text_logits ->
//     argmax + collapse + drop EOS -> final transcript.

#include "decoder.h"
#include "encoder.h"
#include "ggml-backend.h"
#include "ggml.h"
#include "gguf.h"
#include "granite_nar.h"
#include "projector.h"
#include "transcribe-arch.h"
#include "transcribe-batch-util.h"
#include "transcribe-debug.h"
#include "transcribe-flash-policy.h"
#include "transcribe-load-common.h"
#include "transcribe-loader.h"
#include "transcribe-log.h"
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::granite_nar {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, GraniteNarModel>);
static_assert(std::is_base_of_v<transcribe_session, GraniteNarSession>);

GraniteNarSession::~GraniteNarSession() {
    if (sched != nullptr) {
        safe_sched_free(sched);
        sched = nullptr;
    }
    if (compute_ctx != nullptr) {
        ggml_free(compute_ctx);
        compute_ctx = nullptr;
    }
}

GraniteNarModel::~GraniteNarModel() {
    if (bn_fused_ctx != nullptr) {
        ggml_free(bn_fused_ctx);
        bn_fused_ctx = nullptr;
    }
    if (bn_fused_buffer != nullptr) {
        safe_buffer_free(bn_fused_buffer);
        bn_fused_buffer = nullptr;
    }
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

constexpr const char k_default_variant[] = "granite-speech-nar";
constexpr float      kBnEps              = 1e-5f;

// Input-length contract (see docs/input-limits.md). granite_nar is a
// hard-context-cap family: the bidirectional editor runs a SINGLE forward
// pass over (audio_tokens + text) and every position uses RoPE bounded by
// dec_max_pos_emb (typically 4096). Past that the RoPE positions silently
// alias past the trained range and the output is garbage. Unlike AR granite
// there is no decode loop / EOS / output truncation, so the only contract is
// the up-front input gate on T_total plus an advisory max_audio_ms. Over-
// length input is rejected with TRANSCRIBE_ERR_INPUT_TOO_LONG before the
// decoder graph is built.

// Representative text budget reserved when advertising max_audio_ms. The
// editor's text side (initial BPE hypothesis + insertion slots) shares the
// decoder context with the audio tokens; reserve room so the advertised
// audio ceiling still leaves space for a typical hypothesis. This is advisory
// only — the actual gate uses the real T_total = n_audio_tokens + n_text.
constexpr int k_text_budget = 256;

// Effective decoder context ceiling, in tokens: the model's trained maximum
// (dec_max_pos_emb), optionally lowered — never raised — by the caller's
// session n_ctx knob.
int granite_nar_context_ceiling(int32_t n_ctx_knob, const GraniteNarHParams & hp) {
    int ceiling = hp.dec_max_pos_emb;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Q-Former audio tokens per encoder window: block_size / downsample_rate
// (== num_queries, 15/5 = 3 for the shipped granite_nar variant).
int granite_nar_num_queries(const GraniteNarHParams & hp) {
    if (hp.prj_downsample_rate > 0 && hp.prj_block_size > 0) {
        return hp.prj_block_size / hp.prj_downsample_rate;
    }
    return 3;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose
// audio tokens plus a representative text hypothesis (k_text_budget) still fit
// the decoder context ceiling. This is the input bound the gate enforces.
// Returns 0 ("unknown / unbounded") if the rate constants are missing, so a
// misconfigured model is never advertised with a wrong finite number. Mirrors
// granite's granite_max_audio_ms, reserving a text budget in place of AR's
// prompt + generation reserve.
int64_t granite_nar_max_audio_ms(const GraniteNarHParams & hp) {
    const int num_queries = granite_nar_num_queries(hp);
    if (num_queries <= 0 || hp.prj_block_size <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0 ||
        hp.dec_max_pos_emb <= 0) {
        return 0;
    }
    const int max_audio_tokens = hp.dec_max_pos_emb - k_text_budget;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    // Invert the encoder/projector rate:
    //   tokens = nblocks * num_queries ; t_enc <= nblocks * block_size
    //   mel_frames = t_enc * 2 ; ms = mel_frames * hop * 1000 / sample_rate
    const int64_t nblocks = max_audio_tokens / num_queries;
    const int64_t t_enc   = nblocks * hp.prj_block_size;
    const int64_t frames  = t_enc * 2;
    return frames * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
}

transcribe_status fuse_batch_norm(GraniteNarModel & m) {
    const size_t n_blocks = m.weights.enc_blocks.size();
    if (n_blocks == 0) {
        return TRANSCRIBE_OK;
    }

    const int64_t inner        = static_cast<int64_t>(m.hparams.enc_hidden) * m.hparams.enc_conv_expansion;
    const size_t  tensor_bytes = static_cast<size_t>(inner) * sizeof(float);

    const size_t     ctx_size = n_blocks * 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params   = { ctx_size, nullptr, /*no_alloc=*/true };
    m.bn_fused_ctx            = ggml_init(params);
    if (m.bn_fused_ctx == nullptr) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar: BatchNorm-fusion context allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b              = m.weights.enc_blocks[i];
        b.conv_bn_fused_scale = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, inner);
        b.conv_bn_fused_bias  = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, inner);
    }

    m.bn_fused_buffer = ggml_backend_alloc_ctx_tensors(m.bn_fused_ctx, m.plan.scheduler_list.back());
    if (m.bn_fused_buffer == nullptr) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar: BatchNorm-fusion buffer allocation failed — "
                            "out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    std::vector<float> bn_w(inner), bn_b(inner), rm(inner), rv(inner);
    std::vector<float> fused_s(inner), fused_b(inner);
    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b = m.weights.enc_blocks[i];
        ggml_backend_tensor_get(b.conv_bn_w, bn_w.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_b, bn_b.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_mean, rm.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_var, rv.data(), 0, tensor_bytes);
        for (int64_t c = 0; c < inner; ++c) {
            const float s = bn_w[c] / std::sqrt(rv[c] + kBnEps);
            fused_s[c]    = s;
            fused_b[c]    = bn_b[c] - rm[c] * s;
        }
        ggml_backend_tensor_set(b.conv_bn_fused_scale, fused_s.data(), 0, tensor_bytes);
        ggml_backend_tensor_set(b.conv_bn_fused_bias, fused_b.data(), 0, tensor_bytes);
    }
    return TRANSCRIBE_OK;
}

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<GraniteNarModel>();
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

    if (const transcribe_status st = read_granite_nar_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    m->hparams.dec_bos_id = m->tok.bos_id();
    m->hparams.dec_eos_id = m->tok.eos_id();

    if (m->hparams.dec_eos_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar: tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->tok.n_tokens() != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar: tokenizer vocab (%d) != dec_vocab_size (%d)",
                m->tok.n_tokens(), m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }

    // Publish the input-length ceiling now that the decoder context window and
    // frontend rate are known (apply_family_invariants ran before the hparams
    // were read, so it could not set this).
    m->caps.max_audio_ms = granite_nar_max_audio_ms(m->hparams);

    // Basis for the session-level limits query (transcribe_session_get_limits):
    // the same rate constants granite_nar_max_audio_ms uses, kept so the
    // effective limit can be recomputed at a lowered session n_ctx. Mirrors
    // granite/model.cpp with granite_nar's field names (prj_block_size /
    // prj_downsample_rate, dec_max_pos_emb) and reserves the text budget in
    // place of AR's prompt + generation reserve — the NAR editor has no
    // autoregressive output, so prompt_overhead is 0 and the whole non-audio
    // reserve lives in gen_reserve (k_text_budget). The guard mirrors
    // granite_nar_max_audio_ms's: every rate field must be present, or the
    // basis stays unset (zero model_max_ctx => the query reports "unbounded").
    {
        const int num_queries = granite_nar_num_queries(m->hparams);
        if (num_queries > 0 && m->hparams.prj_block_size > 0 && m->hparams.fe_hop_length > 0 &&
            m->hparams.fe_sample_rate > 0 && m->hparams.dec_max_pos_emb > 0) {
            m->limits.has_context_cap    = true;
            m->limits.model_max_ctx      = m->hparams.dec_max_pos_emb;
            m->limits.prompt_overhead    = 0;              // text reserved via gen_reserve
            m->limits.gen_reserve        = k_text_budget;  // match granite_nar_max_audio_ms
            // ms per audio token: granite_nar emits num_queries tokens per
            // prj_block_size encoder frames; t_enc = mel_frames/2;
            // mel_frames = ms*sr/(hop*1000). Inverting
            // granite_nar_max_audio_ms's forward rate gives ms-per-audio-token.
            m->limits.ms_per_audio_token = (static_cast<double>(m->hparams.prj_block_size) / num_queries) * 2.0 *
                                           m->hparams.fe_hop_length * 1000.0 / m->hparams.fe_sample_rate;
            m->limits.kv_elems_per_ctx_token =
                (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
        }
    }

    // Mel frontend.
    // The NLE shares the whisper-mode feature extractor with AR Granite
    // but the NAR GGUF doesn't ship pre_emphasis / f_min / f_max KVs (the
    // upstream WhisperFeatureExtractor doesn't expose them; we hardcode
    // the equivalent defaults here).
    {
        transcribe::MelConfig cfg{};
        cfg.sample_rate  = m->hparams.fe_sample_rate;
        cfg.num_mels     = m->hparams.fe_num_mels;
        cfg.n_fft        = m->hparams.fe_n_fft;
        cfg.win_length   = m->hparams.fe_win_length;
        cfg.hop_length   = m->hparams.fe_hop_length;
        cfg.pre_emphasis = 0.0f;
        cfg.f_min        = 0.0f;
        cfg.f_max        = static_cast<float>(m->hparams.fe_sample_rate) / 2.0f;
        cfg.pad_mode     = m->hparams.fe_pad_mode;
        cfg.window_type  = m->hparams.fe_window;
        cfg.normalize    = m->hparams.fe_normalize;

        // Frontend buffers baked by the converter.
        {
            using R               = transcribe::load_common::ReadF32Result;
            const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
            const auto   fb_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.mel_filterbank", fb_elems, "granite_nar", cfg.filterbank);
            if (fb_rc != R::Ok && fb_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
            const size_t win_elems = static_cast<size_t>(cfg.win_length);
            const auto   win_rc    = transcribe::load_common::read_f32_tensor_checked(
                loader.gguf(), loader.path(), "frontend.window", win_elems, "granite_nar", cfg.window);
            if (win_rc != R::Ok && win_rc != R::Absent) {
                return TRANSCRIBE_ERR_GGUF;
            }
        }

        m->mel.emplace(cfg);
    }

    // Tensor catalog.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (const transcribe_status st = build_granite_nar_weights(m->ctx_meta, m->hparams, m->weights);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (const transcribe_status st = transcribe::load_common::init_backends(
            backend_req, (params != nullptr) ? params->gpu_device : 0, "granite_nar", m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar: alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (const transcribe_status st =
            transcribe::load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "granite_nar");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    if (const transcribe_status st = fuse_batch_norm(*m); st != TRANSCRIBE_OK) {
        return st;
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

    auto cc               = std::make_unique<GraniteNarSession>();
    cc->model             = model;
    cc->n_threads         = params->n_threads;
    cc->kv_type           = params->kv_type;
    cc->n_ctx             = transcribe_session_params_n_ctx(params);
    cc->encoder_use_flash = false;
    cc->decoder_use_flash = false;  // bidirectional path doesn't use KV cache
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

namespace {

void apply_thread_count(ggml_backend_sched_t sched, int n_threads) {
    transcribe::configure_sched_n_threads(sched, n_threads);
}

}  // namespace

transcribe_status run(transcribe_session *          ctx_base,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    (void) params;  // NLE has no language / task knobs

    if (ctx_base == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<GraniteNarSession *>(ctx_base);
    auto * cm = static_cast<GraniteNarModel *>(cc->model);

    transcribe::debug::init();

    if (!cm->mel.has_value()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: model has no MelFrontend");
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // Mel + 2-frame stack.
    const int64_t t_mel_start = ggml_time_us();
    int           t_enc       = 0;
    if (const transcribe_status mst =
            compute_mel_encoder_input(*cm->mel, pcm, n_samples, cc->n_threads, cc->mel_buf, t_enc);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: mel/stack failed (%s)", transcribe_status_string(mst));
        return mst;
    }
    cc->t_enc    = t_enc;
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    if (transcribe::debug::enabled()) {
        const long long shape[2] = { t_enc, cm->hparams.enc_input_dim };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "encoder");
    }

    // Reset compute state.
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    {
        ggml_init_params ip{};
        ip.mem_size     = 32 * 1024 * 1024;
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite_nar run: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    // Encoder graph.
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, cm->hparams, t_enc, cc->encoder_use_flash);
    if (eb.graph == nullptr || eb.cat_out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 32768, false, true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    // Upload encoder inputs.
    ggml_backend_tensor_set(eb.mel_in, cc->mel_buf.data(), 0, cc->mel_buf.size() * sizeof(float));
    {
        std::vector<int32_t> dists =
            precompute_attention_dists(cm->hparams.enc_context_size, cm->hparams.enc_max_pos_emb);
        ggml_backend_tensor_set(eb.attention_dists, dists.data(), 0, dists.size() * sizeof(int32_t));
    }
    {
        const int          ctx_size = cm->hparams.enc_context_size;
        const size_t       plane    = static_cast<size_t>(ctx_size) * ctx_size;
        std::vector<float> mask(plane * eb.n_blocks_local, 0.0f);
        const int          rem = eb.last_block_rem;
        if (rem > 0 && rem < ctx_size) {
            std::vector<float> last = precompute_last_block_mask(ctx_size, rem);
            std::memcpy(mask.data() + plane * (eb.n_blocks_local - 1), last.data(), plane * sizeof(float));
        }
        ggml_backend_tensor_set(eb.last_block_mask, mask.data(), 0, mask.size() * sizeof(float));
    }
    if (eb.zero_pad != nullptr) {
        const size_t       n_elems = static_cast<size_t>(eb.zero_pad->ne[0]) * eb.zero_pad->ne[1];
        std::vector<float> zeros(n_elems, 0.0f);
        ggml_backend_tensor_set(eb.zero_pad, zeros.data(), 0, zeros.size() * sizeof(float));
    }

    apply_thread_count(cc->sched, cc->n_threads);

    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: encoder compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.input_linear.out", eb.dumps.input_linear_out, "encoder");
    try_dump("enc.block.0.post_ff1", eb.dumps.block_0_post_ff1, "encoder");
    try_dump("enc.block.0.post_attn", eb.dumps.block_0_post_attn, "encoder");
    try_dump("enc.block.0.post_conv", eb.dumps.block_0_post_conv, "encoder");
    try_dump("enc.block.0.post_ff2", eb.dumps.block_0_post_ff2, "encoder");
    try_dump("enc.block.0.out", eb.dumps.block_0_out, "encoder");
    if (eb.dumps.block_mid_pre) {
        try_dump(eb.dumps.block_mid_pre->name, eb.dumps.block_mid_pre, "encoder");
    }
    if (eb.dumps.block_mid_post) {
        try_dump(eb.dumps.block_mid_post->name, eb.dumps.block_mid_post, "encoder");
    }
    if (eb.dumps.block_last_out) {
        try_dump(eb.dumps.block_last_out->name, eb.dumps.block_last_out, "encoder");
    }
    try_dump("enc.ctc_logits", eb.dumps.ctc_logits, "encoder");

    // Read encoder outputs to host.
    const int64_t cat_h = eb.cat_out->ne[0];
    const int64_t cat_T = eb.cat_out->ne[1];
    cc->enc_cat_host.resize(static_cast<size_t>(cat_h) * cat_T);
    ggml_backend_tensor_get(eb.cat_out, cc->enc_cat_host.data(), 0, cc->enc_cat_host.size() * sizeof(float));

    const int n_ctc_vocab = static_cast<int>(eb.ctc_logits->ne[0]);
    cc->ctc_logits_host.resize(static_cast<size_t>(n_ctc_vocab) * cat_T);
    ggml_backend_tensor_get(eb.ctc_logits, cc->ctc_logits_host.data(), 0, cc->ctc_logits_host.size() * sizeof(float));

    int n_bpe_vocab = 0;
    if (eb.ctc_bpe_logits != nullptr) {
        n_bpe_vocab = static_cast<int>(eb.ctc_bpe_logits->ne[0]);
        cc->ctc_bpe_logits_host.resize(static_cast<size_t>(n_bpe_vocab) * cat_T);
        ggml_backend_tensor_get(eb.ctc_bpe_logits, cc->ctc_bpe_logits_host.data(), 0,
                                cc->ctc_bpe_logits_host.size() * sizeof(float));
    }

    std::vector<float> mid_non_blank;
    if (eb.mid_blank_probs != nullptr) {
        std::vector<float> mid_blank(t_enc);
        ggml_backend_tensor_get(eb.mid_blank_probs, mid_blank.data(), 0, mid_blank.size() * sizeof(float));
        mid_non_blank.resize(t_enc);
        for (int i = 0; i < t_enc; ++i) {
            mid_non_blank[i] = 1.0f - mid_blank[i];
        }
    }

    // ggml stores ne[0]=F as the innermost (fastest-varying) axis. For
    // a tensor with ne=[F, T_enc], the host buffer layout in memory is
    //   buf[t * F + f] = element at ggml indices (f, t)
    // which is the SAME as `[T_enc, F]` numpy row-major (T outer, F
    // inner). No transpose needed: the host-side BPE pool reads
    //   ctc_bpe_logits_host[t * V + v]
    // and gets the v-th logit at frame t correctly.
    (void) n_ctc_vocab;

    // Initial BPE hypothesis.
    std::vector<int32_t> hyp_ids;
    if (n_bpe_vocab > 0 && !mid_non_blank.empty()) {
        compute_bpe_ctc_initial_hypothesis(mid_non_blank, cc->ctc_bpe_logits_host, n_bpe_vocab, t_enc,
                                           cm->hparams.enc_bpe_pool_window,
                                           /*blank_id=*/cm->hparams.enc_bpe_blank_id, hyp_ids);
    }
    if (hyp_ids.empty()) {
        // No tokens — emit empty transcript and return.
        cc->full_text.clear();
        cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        cc->has_result  = true;
        transcribe_session::SegmentEntry seg{};
        seg.text  = cc->full_text;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
        cc->segments.push_back(std::move(seg));
        return TRANSCRIBE_OK;
    }

    // Projector graph.
    ggml_context * proj_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size = 16 * 1024 * 1024;
        ip.no_alloc = true;
        proj_ctx    = ggml_init(ip);
        if (proj_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite_nar run: projector compute context allocation failed "
                                "— out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ProjectorBuild pb = build_projector_graph(proj_ctx, cm->weights, cm->hparams, t_enc);
    if (pb.graph == nullptr || pb.out == nullptr) {
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar run: projector graph allocation failed — "
                            "out of memory.");
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_OOM;
    }

    // The encoder output sits as [cat_h, T_enc] in ggml ne order on the
    // host (i.e., enc_cat_host is feature-major in row-major bytes).
    // ggml expects the same layout for pb.enc_in — pass directly.
    ggml_backend_tensor_set(pb.enc_in, cc->enc_cat_host.data(), 0, cc->enc_cat_host.size() * sizeof(float));
    if (pb.enc_pad != nullptr) {
        const size_t       pad_n = static_cast<size_t>(pb.enc_pad->ne[0]) * pb.enc_pad->ne[1];
        std::vector<float> zeros(pad_n, 0.0f);
        ggml_backend_tensor_set(pb.enc_pad, zeros.data(), 0, zeros.size() * sizeof(float));
    }

    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: projector compute failed (%d)", static_cast<int>(gs));
        ggml_free(proj_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    try_dump("proj.qformer.out", pb.dumps.qformer_out, "projector");
    try_dump("proj.out", pb.dumps.proj_out, "projector");

    // Reference projector emits nblocks*n_query audio tokens (here 37*3
    // = 111 for T_enc=550) but the LLM forward only consumes
    // `projected_length = T_enc / downsample_rate` of them (110), dropping
    // the trailing tokens that came from the zero-padded window. Mirror
    // that truncation so the audio-side n matches the reference.
    const int n_audio_full    = pb.n_audio_tokens;
    const int n_audio_used    = t_enc / cm->hparams.prj_downsample_rate;
    const int n_audio_clipped = std::min(n_audio_used, n_audio_full);
    cc->n_audio_tokens        = n_audio_clipped;

    const int64_t llm_dim_runtime = pb.out->ne[0];
    cc->proj_out_host.resize(static_cast<size_t>(llm_dim_runtime) * n_audio_clipped);
    ggml_backend_tensor_get(pb.out, cc->proj_out_host.data(), 0, cc->proj_out_host.size() * sizeof(float));
    ggml_free(proj_ctx);

    // add_insertion_slots.
    std::vector<int32_t> text_ids;
    add_insertion_slots(hyp_ids, cm->hparams.dec_eos_id, text_ids);
    const int n_text = static_cast<int>(text_ids.size());

    // Input-length gate.
    // The editor is non-autoregressive: a SINGLE bidirectional forward over
    // (audio_tokens + text) where every position's RoPE is bounded by
    // dec_max_pos_emb (optionally lowered by the caller's n_ctx knob). Both
    // token counts are now fixed by the input, so reject an over-length clip
    // here — before building/running the decoder graph — instead of letting
    // RoPE silently alias past the trained range and emit garbage. There is
    // no output budget to reserve (no decode loop / EOS), so the gate is the
    // full T_total against the ceiling.
    const int T_total = cc->n_audio_tokens + n_text;
    const int ceiling = granite_nar_context_ceiling(cc->n_ctx, cm->hparams);
    if (T_total > ceiling) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar run: input too long — %d audio + %d text tokens "
                            "exceed the %d-token context. Shorten audio or see "
                            "transcribe_capabilities.max_audio_ms.",
                            cc->n_audio_tokens, n_text, ceiling);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // Divide projector output by embedding_multiplier so the post-
    // multiplier audio rows round-trip to the original values. We
    // could do this in the graph but keeping it host-side avoids one
    // extra scale op per audio token.
    const float emb_mul = cm->hparams.dec_embedding_multiplier;
    if (emb_mul > 0.0f) {
        const float inv = 1.0f / emb_mul;
        for (auto & v : cc->proj_out_host) {
            v *= inv;
        }
    }

    // Decoder graph.
    ggml_context * dec_ctx = nullptr;
    {
        ggml_init_params ip{};
        ip.mem_size = 128 * 1024 * 1024;  // 40 layers @ bidirectional
        ip.no_alloc = true;
        dec_ctx     = ggml_init(ip);
        if (dec_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "granite_nar run: decoder compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    ForwardBuild fb = build_forward_graph(dec_ctx, cm->weights, cm->hparams, cc->n_audio_tokens, n_text);
    if (fb.graph == nullptr || fb.out == nullptr) {
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, fb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "granite_nar run: decoder graph allocation failed — "
                            "out of memory.");
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(fb.audio_in, cc->proj_out_host.data(), 0, cc->proj_out_host.size() * sizeof(float));
    ggml_backend_tensor_set(fb.text_ids_in, text_ids.data(), 0, text_ids.size() * sizeof(int32_t));
    {
        std::vector<int32_t> positions(fb.T_total);
        for (int i = 0; i < fb.T_total; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(fb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }

    const int64_t t_dec_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, fb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "granite_nar run: decoder compute failed (%d)", static_cast<int>(gs));
        ggml_free(dec_ctx);
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_decode_us = ggml_time_us() - t_dec_start;

    try_dump("dec.flat_embeds", fb.dumps.flat_embeds, "decoder");
    try_dump("dec.text_logits", fb.dumps.text_logits, "decoder");

    // Read text-portion logits. ggml stores ne[0]=vocab as the innermost
    // axis, which means the host buffer layout is [t * vocab + v] —
    // i.e. `[n_text, vocab]` numpy row-major already. No transpose.
    const int64_t      vocab = fb.out->ne[0];
    std::vector<float> logits_host(static_cast<size_t>(vocab) * n_text);
    ggml_backend_tensor_get(fb.out, logits_host.data(), 0, logits_host.size() * sizeof(float));
    ggml_free(dec_ctx);

    std::vector<int32_t> final_ids;
    argmax_collapse_drop_eos(logits_host, static_cast<int>(vocab), n_text, cm->hparams.dec_eos_id, final_ids);

    cc->full_text = cm->tok.decode(final_ids.data(), static_cast<int>(final_ids.size()));

    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;
    transcribe_session::SegmentEntry seg{};
    seg.text  = cc->full_text;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(cm->hparams.fe_sample_rate);
    cc->segments.push_back(std::move(seg));

    return TRANSCRIBE_OK;
}

}  // namespace

extern const Arch arch = {
    /* .name             = */ "granite_speech_nar",
    /* .load             = */ load,
    /* .init_context     = */ init_context,
    /* .run              = */ run,
    /* .run_batch        = */ nullptr,
    /* .stream_validate  = */ nullptr,
    /* .stream_begin     = */ nullptr,
    /* .stream_feed      = */ nullptr,
    /* .stream_finalize  = */ nullptr,
    /* .stream_reset     = */ nullptr,
    /* .accepts_ext_kind = */ nullptr,
};

}  // namespace transcribe::granite_nar
