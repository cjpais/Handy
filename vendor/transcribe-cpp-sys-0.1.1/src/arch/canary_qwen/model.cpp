// arch/canary_qwen/model.cpp - SALM (FastConformer + Qwen3-1.7B) family handler.
//
// Forward shape:
//   pcm
//     -> mel preprocessor (NeMo AudioToMelSpectrogramPreprocessor;
//        per_feature normalize; trailing-frame masked to zero)
//     -> FastConformer encoder (32 blocks; identical to canary-1b-flash)
//     -> perception projection (Linear(1024, 2048) + bias)
//   prompt = HF chat template applied to
//            "Transcribe the following: <|audioplaceholder|>"
//     -> token ids (15 ids for the JFK case)
//   audio scatter: replace single <|audioplaceholder|> position with
//                  T_enc=138 perception rows -> input_embeds (T_prompt, hidden=2048)
//     -> Qwen3-1.7B causal LM (28 blocks)
//     -> tied lm_head -> greedy autoregressive loop until EOS or max_new.
//
// On CPU primary backend all BF16 linear weights are promoted to F32 at load
// (matches the reference's f32 regime); see promote_linears_bf16_to_f32_on_cpu.

#include "canary_qwen.h"
#include "causal_lm/causal_lm.h"
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
#include "transcribe-mel.h"
#include "transcribe-meta.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <memory>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace transcribe::canary_qwen {

extern const Arch arch;

static_assert(std::is_base_of_v<transcribe_model, CanaryQwenModel>);
static_assert(std::is_base_of_v<transcribe_session, CanaryQwenSession>);

CanaryQwenSession::~CanaryQwenSession() {
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

CanaryQwenModel::~CanaryQwenModel() {
    if (ctx_meta != nullptr) {
        ggml_free(ctx_meta);
        ctx_meta = nullptr;
    }
    if (backend_buffer != nullptr) {
        safe_buffer_free(backend_buffer);
        backend_buffer = nullptr;
    }
    if (bn_fused_buffer != nullptr) {
        safe_buffer_free(bn_fused_buffer);
        bn_fused_buffer = nullptr;
    }
    if (bn_fused_ctx != nullptr) {
        ggml_free(bn_fused_ctx);
        bn_fused_ctx = nullptr;
    }
    if (conv_pw_f32_buffer != nullptr) {
        safe_buffer_free(conv_pw_f32_buffer);
        conv_pw_f32_buffer = nullptr;
    }
    if (conv_pw_f32_ctx != nullptr) {
        ggml_free(conv_pw_f32_ctx);
        conv_pw_f32_ctx = nullptr;
    }
    if (linear_f32_buffer != nullptr) {
        safe_buffer_free(linear_f32_buffer);
        linear_f32_buffer = nullptr;
    }
    if (linear_f32_ctx != nullptr) {
        ggml_free(linear_f32_ctx);
        linear_f32_ctx = nullptr;
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

constexpr const char k_default_variant[] = "canary-qwen-2.5b";
constexpr float      kBnEps              = 1e-5f;

// Input-length contract (see docs/input-limits.md). canary_qwen is a
// hard-context-cap family: audio tokens + chat prompt + generation share the
// Qwen3 decoder's context window (decoder.max_position_embeddings), clamped to
// that ceiling. Over-length input is rejected with
// TRANSCRIBE_ERR_INPUT_TOO_LONG; a transcript that fills the generation budget
// before end-of-stream is flagged via transcribe_was_truncated().

// Per-run generation budget. Keep in sync with the single-utterance and
// batched step loops below.
constexpr int k_max_new = 256;

// Effective decoder context ceiling, in tokens: the model's trained maximum
// (decoder.max_position_embeddings, e.g. 40960), optionally lowered — never
// raised — by the caller's session n_ctx knob.
int canary_qwen_context_ceiling(int32_t n_ctx_knob, const CanaryQwenHParams & hp) {
    int ceiling = hp.dec_max_position;
    if (n_ctx_knob > 0 && n_ctx_knob < ceiling) {
        ceiling = n_ctx_knob;
    }
    return ceiling;
}

// Advisory transcribe_capabilities::max_audio_ms: the longest audio whose
// audio tokens plus a representative prompt and the generation reserve fit
// the context ceiling. The FastConformer pre-encode downsamples mel frames by
// enc_subsampling_factor (8 = three stride-2 convs), and the perception
// projection is 1:1 in time, so the LM sees mel_frames / 8 audio tokens.
// Inverting that gives ms. Returns 0 ("unknown / unbounded") if the rate
// constants are missing. Note: even within this bound a long transcript may
// truncate at the generation budget (transcribe_was_truncated) —
// max_audio_ms is the input bound.
int64_t canary_qwen_max_audio_ms(const CanaryQwenHParams & hp) {
    if (hp.dec_max_position <= 0 || hp.enc_subsampling_factor <= 0 || hp.fe_hop_length <= 0 || hp.fe_sample_rate <= 0) {
        return 0;
    }
    // Representative fixed chat-template overhead (prefix + suffix token
    // counts; ~14 for canary_qwen). Advisory headroom, generous enough to
    // cover small template drift.
    constexpr int k_prompt_overhead = 32;
    const int     max_audio_tokens  = hp.dec_max_position - k_prompt_overhead - k_max_new;
    if (max_audio_tokens <= 0) {
        return 0;
    }
    // audio_tokens ≈ mel_frames / subsampling_factor ;
    //   mel_frames = ms * sr / (hop * 1000)
    //   => ms ≈ audio_tokens * subsampling_factor * hop * 1000 / sr
    const int64_t mel_frames = static_cast<int64_t>(max_audio_tokens) * hp.enc_subsampling_factor;
    return mel_frames * hp.fe_hop_length * 1000 / hp.fe_sample_rate;
}

// Chat-template prompt construction. HF's chat template renders the request as:
//   <|im_start|> user \n Transcribe the following: <|audioplaceholder|>
//   <|im_end|> \n <|im_start|> assistant \n
// Verified: tok.encode("user\nTranscribe the following: ")
//   -> [872, 198, 3167, 3114, 279, 2701, 25, 220]. We pre-encode prefix and
// suffix at load; the audio_locator id is replicated T_enc times between them
// at run time.

transcribe_status resolve_chat_tokens(const transcribe::Tokenizer & tok, ChatTokens & out) {
    struct PieceSlot {
        const char * piece;
        int32_t *    slot;
    };

    const PieceSlot pieces[] = {
        { "<|im_start|>", &out.im_start       },
        { "<|im_end|>",   &out.im_end         },
        { "user",         &out.role_user      },
        { "assistant",    &out.role_assistant },
    };
    for (const auto & p : pieces) {
        const int id = tok.find(p.piece);
        if (id < 0) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: chat-template piece \"%s\" not in tokenizer", p.piece);
            return TRANSCRIBE_ERR_GGUF;
        }
        *p.slot = id;
    }
    return TRANSCRIBE_OK;
}

transcribe_status build_static_prompt_segments(CanaryQwenModel & m) {
    if (!m.tok.has_encoder()) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "canary_qwen: tokenizer has no BPE encoder — cannot "
                "tokenize chat template at load time");
        return TRANSCRIBE_ERR_GGUF;
    }

    // prefix = [<|im_start|>] + bpe("user\nTranscribe the following: ")
    m.prompt_prefix_ids.clear();
    m.prompt_prefix_ids.push_back(m.chat_tokens.im_start);
    {
        std::vector<int32_t> ids;
        if (auto st = m.tok.encode("user\nTranscribe the following: ", ids); st != TRANSCRIBE_OK) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: prefix BPE encode failed");
            return st;
        }
        m.prompt_prefix_ids.insert(m.prompt_prefix_ids.end(), ids.begin(), ids.end());
    }

    // suffix = [<|im_end|>] + bpe("\n") + [<|im_start|>] + bpe("assistant\n")
    m.prompt_suffix_ids.clear();
    m.prompt_suffix_ids.push_back(m.chat_tokens.im_end);
    {
        std::vector<int32_t> ids;
        if (auto st = m.tok.encode("\n", ids); st != TRANSCRIBE_OK) {
            return st;
        }
        m.prompt_suffix_ids.insert(m.prompt_suffix_ids.end(), ids.begin(), ids.end());
    }
    m.prompt_suffix_ids.push_back(m.chat_tokens.im_start);
    {
        std::vector<int32_t> ids;
        if (auto st = m.tok.encode("assistant\n", ids); st != TRANSCRIBE_OK) {
            return st;
        }
        m.prompt_suffix_ids.insert(m.prompt_suffix_ids.end(), ids.begin(), ids.end());
    }
    return TRANSCRIBE_OK;
}

// BatchNorm fusion (same math as canary): pre-fused (scale, bias) fall out of
// (running_mean, running_var, weight, bias, eps).
transcribe_status fuse_batch_norm(CanaryQwenModel & m) {
    const size_t n_blocks = m.weights.blocks.size();
    if (n_blocks == 0) {
        return TRANSCRIBE_OK;
    }

    const int64_t d            = m.hparams.enc_d_model;
    const size_t  tensor_bytes = static_cast<size_t>(d) * sizeof(float);

    const size_t     ctx_size = n_blocks * 2 * ggml_tensor_overhead() + 256;
    ggml_init_params params   = { ctx_size, nullptr, true };
    m.bn_fused_ctx            = ggml_init(params);
    if (m.bn_fused_ctx == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b              = m.weights.blocks[i];
        b.conv_bn_fused_scale = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
        b.conv_bn_fused_bias  = ggml_new_tensor_1d(m.bn_fused_ctx, GGML_TYPE_F32, d);
    }

    m.bn_fused_buffer = ggml_backend_alloc_ctx_tensors(m.bn_fused_ctx, m.plan.scheduler_list.back());
    if (m.bn_fused_buffer == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    std::vector<float> bn_w(d), bn_b(d), rm(d), rv(d);
    std::vector<float> fused_s(d), fused_b(d);

    for (size_t i = 0; i < n_blocks; ++i) {
        auto & b = m.weights.blocks[i];
        ggml_backend_tensor_get(b.conv_bn_w, bn_w.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_b, bn_b.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_rm, rm.data(), 0, tensor_bytes);
        ggml_backend_tensor_get(b.conv_bn_rv, rv.data(), 0, tensor_bytes);

        for (int64_t c = 0; c < d; ++c) {
            const float s = bn_w[c] / std::sqrt(rv[c] + kBnEps);
            fused_s[c]    = s;
            fused_b[c]    = bn_b[c] - rm[c] * s;
        }

        ggml_backend_tensor_set(b.conv_bn_fused_scale, fused_s.data(), 0, tensor_bytes);
        ggml_backend_tensor_set(b.conv_bn_fused_bias, fused_b.data(), 0, tensor_bytes);
    }

    return TRANSCRIBE_OK;
}

// F16 → F32 promotion for pointwise AND depthwise conv kernels (any backend).
// The converter stores conv kernels at F16 (canary-qwen-2.5b is BF16-reference;
// the loader rejects BF16 conv). `ggml_conv_2d_dw_direct` silently produces
// wildly wrong output for F16 kernels (verified CPU + Metal): the conv output
// blows up ~1500x on the first block, collapsing the decoder to a single `!`.
// Promoting to F32 bypasses that and matches NeMo to single-percent drift.
// (load_common::promote_conv_pw_f16_to_f32_on_cpu can't be used — it early-outs
// on non-CPU backends.)
transcribe_status promote_conv_pw_to_f32_on_cpu(CanaryQwenModel & m) {
    if (m.plan.primary == nullptr) {
        return TRANSCRIBE_OK;
    }

    struct Slot {
        ggml_tensor ** dst_slot;
        ggml_tensor *  src;
    };

    std::vector<Slot> slots;
    slots.reserve(m.weights.blocks.size() * 3);
    auto add = [&](ggml_tensor ** s) {
        if (s != nullptr && *s != nullptr && (*s)->type == GGML_TYPE_F16) {
            slots.push_back({ s, *s });
        }
    };
    for (auto & b : m.weights.blocks) {
        add(&b.conv_pw1_w);
        add(&b.conv_dw_w);
        add(&b.conv_pw2_w);
    }
    if (slots.empty()) {
        return TRANSCRIBE_OK;
    }

    const size_t     ctx_size    = slots.size() * ggml_tensor_overhead() + 256;
    ggml_init_params init_params = { ctx_size, nullptr, true };
    m.conv_pw_f32_ctx            = ggml_init(init_params);
    if (m.conv_pw_f32_ctx == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    std::vector<ggml_tensor *> replacements;
    replacements.reserve(slots.size());
    for (const auto & s : slots) {
        ggml_tensor * r = ggml_new_tensor(m.conv_pw_f32_ctx, GGML_TYPE_F32, ggml_n_dims(s.src), s.src->ne);
        if (r == nullptr) {
            ggml_free(m.conv_pw_f32_ctx);
            m.conv_pw_f32_ctx = nullptr;
            return TRANSCRIBE_ERR_BACKEND;
        }
        ggml_set_name(r, s.src->name);
        replacements.push_back(r);
    }

    m.conv_pw_f32_buffer = ggml_backend_alloc_ctx_tensors(m.conv_pw_f32_ctx, m.plan.primary);
    if (m.conv_pw_f32_buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: conv F16->F32 promotion buffer alloc failed");
        ggml_free(m.conv_pw_f32_ctx);
        m.conv_pw_f32_ctx = nullptr;
        return TRANSCRIBE_ERR_BACKEND;
    }
    ggml_backend_buffer_set_usage(m.conv_pw_f32_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    const auto * f16_traits = ggml_get_type_traits(GGML_TYPE_F16);
    if (f16_traits == nullptr || f16_traits->to_float == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "canary_qwen: no f16 to_float trait — skipping conv promotion");
        safe_buffer_free(m.conv_pw_f32_buffer);
        m.conv_pw_f32_buffer = nullptr;
        ggml_free(m.conv_pw_f32_ctx);
        m.conv_pw_f32_ctx = nullptr;
        return TRANSCRIBE_OK;
    }

    std::vector<uint8_t> f16_staging;
    std::vector<float>   f32_staging;
    for (size_t i = 0; i < slots.size(); ++i) {
        ggml_tensor * src       = slots[i].src;
        ggml_tensor * dst       = replacements[i];
        const int64_t n_elem    = ggml_nelements(src);
        const size_t  f16_bytes = ggml_nbytes(src);
        const size_t  f32_bytes = static_cast<size_t>(n_elem) * sizeof(float);

        if (f16_staging.size() < f16_bytes) {
            f16_staging.resize(f16_bytes);
        }
        if (f32_staging.size() < static_cast<size_t>(n_elem)) {
            f32_staging.resize(n_elem);
        }

        ggml_backend_tensor_get(src, f16_staging.data(), 0, f16_bytes);
        f16_traits->to_float(f16_staging.data(), f32_staging.data(), n_elem);
        ggml_backend_tensor_set(dst, f32_staging.data(), 0, f32_bytes);

        *slots[i].dst_slot = dst;
    }

    log_msg(TRANSCRIBE_LOG_LEVEL_INFO, "canary_qwen: promoted %zu F16 conv weights to F32", slots.size());
    return TRANSCRIBE_OK;
}

// CPU-only BF16 → F32 promotion of all linear weights, matching the reference's
// F32 inference regime. Replacements live in a dedicated buffer (freed
// independently); graph builders read whatever dtype each tensor reports.
// Non-CPU backends (Metal, Vulkan) keep their own BF16 paths.
transcribe_status promote_linears_bf16_to_f32_on_cpu(CanaryQwenModel & m) {
    if (m.plan.primary_kind != transcribe::BackendKind::Cpu || m.plan.primary == nullptr) {
        return TRANSCRIBE_OK;
    }

    // Collect (slot_pointer, source_tensor) for every BF16 linear we own.
    struct Slot {
        ggml_tensor ** dst_slot;
        ggml_tensor *  src;
    };

    std::vector<Slot> slots;
    auto              add = [&](ggml_tensor ** s) {
        if (s != nullptr && *s != nullptr && (*s)->type == GGML_TYPE_BF16) {
            slots.push_back({ s, *s });
        }
    };

    // Encoder pre-encode out projection.
    add(&m.weights.pre_encode.out_w);

    // Encoder blocks — every linear (FFs, Q/K/V/O/pos).
    for (auto & b : m.weights.blocks) {
        add(&b.ff1_lin1_w);
        add(&b.ff1_lin2_w);
        add(&b.attn_q_w);
        add(&b.attn_k_w);
        add(&b.attn_v_w);
        add(&b.attn_out_w);
        add(&b.attn_pos_w);
        add(&b.ff2_lin1_w);
        add(&b.ff2_lin2_w);
    }

    // Perception projection.
    add(&m.weights.perception_proj.weight);

    // Decoder embedding (also serves as tied lm_head).
    add(&m.weights.dec_embed.token_w);

    // Decoder blocks — Q/K/V/O + gate/up/down.
    for (auto & b : m.weights.dec_blocks) {
        add(&b.attn_q_w);
        add(&b.attn_k_w);
        add(&b.attn_v_w);
        add(&b.attn_o_w);
        add(&b.ffn_gate_w);
        add(&b.ffn_up_w);
        add(&b.ffn_down_w);
    }

    if (slots.empty()) {
        return TRANSCRIBE_OK;
    }

    // Allocate a dedicated ctx + buffer for the F32 replacements.
    const size_t     ctx_size    = slots.size() * ggml_tensor_overhead() + 256;
    ggml_init_params init_params = { ctx_size, nullptr, true };
    m.linear_f32_ctx             = ggml_init(init_params);
    if (m.linear_f32_ctx == nullptr) {
        return TRANSCRIBE_ERR_BACKEND;
    }

    std::vector<ggml_tensor *> replacements;
    replacements.reserve(slots.size());
    for (const auto & s : slots) {
        ggml_tensor * r = ggml_new_tensor(m.linear_f32_ctx, GGML_TYPE_F32, ggml_n_dims(s.src), s.src->ne);
        if (r == nullptr) {
            ggml_free(m.linear_f32_ctx);
            m.linear_f32_ctx = nullptr;
            return TRANSCRIBE_ERR_BACKEND;
        }
        ggml_set_name(r, s.src->name);
        replacements.push_back(r);
    }

    m.linear_f32_buffer = ggml_backend_alloc_ctx_tensors(m.linear_f32_ctx, m.plan.primary);
    if (m.linear_f32_buffer == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: BF16→F32 linear promotion buffer alloc failed");
        ggml_free(m.linear_f32_ctx);
        m.linear_f32_ctx = nullptr;
        return TRANSCRIBE_ERR_BACKEND;
    }
    ggml_backend_buffer_set_usage(m.linear_f32_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    const auto * bf16_traits = ggml_get_type_traits(GGML_TYPE_BF16);
    if (bf16_traits == nullptr || bf16_traits->to_float == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_WARN, "canary_qwen: no bf16 to_float trait — skipping linear promotion");
        safe_buffer_free(m.linear_f32_buffer);
        m.linear_f32_buffer = nullptr;
        ggml_free(m.linear_f32_ctx);
        m.linear_f32_ctx = nullptr;
        return TRANSCRIBE_OK;
    }

    std::vector<uint8_t> bf16_staging;
    std::vector<float>   f32_staging;
    for (size_t i = 0; i < slots.size(); ++i) {
        ggml_tensor * src        = slots[i].src;
        ggml_tensor * dst        = replacements[i];
        const int64_t n_elem     = ggml_nelements(src);
        const size_t  bf16_bytes = ggml_nbytes(src);
        const size_t  f32_bytes  = static_cast<size_t>(n_elem) * sizeof(float);

        if (bf16_staging.size() < bf16_bytes) {
            bf16_staging.resize(bf16_bytes);
        }
        if (f32_staging.size() < static_cast<size_t>(n_elem)) {
            f32_staging.resize(n_elem);
        }

        ggml_backend_tensor_get(src, bf16_staging.data(), 0, bf16_bytes);
        bf16_traits->to_float(bf16_staging.data(), f32_staging.data(), n_elem);
        ggml_backend_tensor_set(dst, f32_staging.data(), 0, f32_bytes);

        *slots[i].dst_slot = dst;
    }

    log_msg(TRANSCRIBE_LOG_LEVEL_INFO, "canary_qwen: promoted %zu BF16 linear weights to F32 for CPU backend",
            slots.size());
    return TRANSCRIBE_OK;
}

// Loader entry points (forward-declared so we can register `arch` after).
extern transcribe_status load(Loader &, const transcribe_model_load_params *, transcribe_model **);
extern transcribe_status init_context(transcribe_model *, const transcribe_session_params *, transcribe_session **);
extern transcribe_status run(transcribe_session *, const float *, int, const transcribe_run_params *);

transcribe_status load(Loader & loader, const transcribe_model_load_params * params, transcribe_model ** out_model) {
    const int64_t t_load_start = ggml_time_us();

    auto m       = std::make_unique<CanaryQwenModel>();
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

    if (auto st = read_canary_qwen_hparams(loader.gguf(), m->hparams); st != TRANSCRIBE_OK) {
        return st;
    }

    // Publish the input-length ceiling now that the decoder context window
    // and frontend rate are known.
    m->caps.max_audio_ms = canary_qwen_max_audio_ms(m->hparams);

    // Basis for transcribe_session_get_limits: the same constants
    // canary_qwen_max_audio_ms uses, so the limit recomputes at a lowered n_ctx.
    if (m->hparams.dec_max_position > 0 && m->hparams.enc_subsampling_factor > 0 && m->hparams.fe_hop_length > 0 &&
        m->hparams.fe_sample_rate > 0) {
        m->limits.has_context_cap    = true;
        m->limits.model_max_ctx      = m->hparams.dec_max_position;
        m->limits.prompt_overhead    = 32;
        m->limits.gen_reserve        = k_max_new;
        // audio_tokens ≈ mel_frames / subsampling_factor ;
        //   mel_frames = ms*sr/(hop*1000)
        m->limits.ms_per_audio_token = static_cast<double>(m->hparams.enc_subsampling_factor) *
                                       m->hparams.fe_hop_length * 1000.0 / m->hparams.fe_sample_rate;
        m->limits.kv_elems_per_ctx_token =
            (int64_t) m->hparams.dec_n_kv_heads * m->hparams.dec_head_dim * m->hparams.dec_n_layers * 2;
    }

    m->hparams.vocab_size   = m->tok.n_tokens();
    m->hparams.bos_token_id = m->tok.bos_id();
    m->hparams.eos_token_id = m->tok.eos_id();

    if (m->hparams.vocab_size != m->hparams.dec_vocab_size) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: tokenizer vocab (%d) != decoder vocab_size (%d)",
                m->hparams.vocab_size, m->hparams.dec_vocab_size);
        return TRANSCRIBE_ERR_GGUF;
    }
    if (m->hparams.eos_token_id < 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: GGUF tokenizer has no eos_token_id");
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = resolve_chat_tokens(m->tok, m->chat_tokens); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = build_static_prompt_segments(*m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Mel frontend.
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
        // NeMo's AudioToMelSpectrogramPreprocessor uses periodic=False
        // (symmetric Hann); the GGUF KV stt.frontend.window only names
        // the window family.
        cfg.window_type  = "hann_symmetric";
        cfg.normalize    = m->hparams.fe_normalize;

        // Pull baked filterbank/window from GGUF if present.
        using R               = load_common::ReadF32Result;
        const size_t fb_elems = static_cast<size_t>(cfg.num_mels) * static_cast<size_t>(cfg.n_fft / 2 + 1);
        const auto fb_rc = load_common::read_f32_tensor_checked(loader.gguf(), loader.path(), "frontend.mel_filterbank",
                                                                fb_elems, "canary_qwen", cfg.filterbank);
        if (fb_rc != R::Ok && fb_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }
        const size_t win_elems = static_cast<size_t>(cfg.win_length);
        const auto   win_rc    = load_common::read_f32_tensor_checked(loader.gguf(), loader.path(), "frontend.window",
                                                                      win_elems, "canary_qwen", cfg.window);
        if (win_rc != R::Ok && win_rc != R::Absent) {
            return TRANSCRIBE_ERR_GGUF;
        }

        m->mel.emplace(cfg);
    }

    // Reopen GGUF with no_alloc to bind the tensor catalog.
    gguf_init_params init_params{};
    init_params.no_alloc = true;
    init_params.ctx      = &m->ctx_meta;

    gguf_context * gguf_data = gguf_init_from_file(loader.path().c_str(), init_params);
    if (gguf_data == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (auto st = build_canary_qwen_weights(m->ctx_meta, m->hparams, m->weights); st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }

    // Backend plan + alloc + stream tensor data.
    const transcribe_backend_request backend_req = (params != nullptr) ? params->backend : TRANSCRIBE_BACKEND_AUTO;
    if (auto st = load_common::init_backends(backend_req, (params != nullptr) ? params->gpu_device : 0, "canary_qwen",
                                             m->plan);
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    m->backend         = ggml_backend_name(m->plan.primary);
    m->primary_backend = m->plan.primary;

    ggml_backend_buffer_t weights_buffer = ggml_backend_alloc_ctx_tensors(m->ctx_meta, m->plan.primary);
    if (weights_buffer == nullptr) {
        gguf_free(gguf_data);
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen: ggml_backend_alloc_ctx_tensors failed");
        return TRANSCRIBE_ERR_GGUF;
    }
    m->backend_buffer = weights_buffer;
    ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);

    if (auto st = load_common::stream_tensor_data(loader.path(), gguf_data, m->ctx_meta, "canary_qwen");
        st != TRANSCRIBE_OK) {
        gguf_free(gguf_data);
        return st;
    }
    gguf_free(gguf_data);

    // Post-load weight transformations.
    if (auto st = fuse_batch_norm(*m); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = promote_conv_pw_to_f32_on_cpu(*m); st != TRANSCRIBE_OK) {
        return st;
    }
    if (auto st = promote_linears_bf16_to_f32_on_cpu(*m); st != TRANSCRIBE_OK) {
        return st;
    }

    // Pack gate + up into one tensor per layer (one mul_mat instead of two).
    {
        std::vector<transcribe::causal_lm::GateUpEntry> entries;
        entries.reserve(m->weights.dec_blocks.size());
        for (auto & b : m->weights.dec_blocks) {
            entries.push_back({ b.ffn_gate_w, b.ffn_up_w, &b.ffn_gate_up_w });
        }
        if (!transcribe::causal_lm::pack_gate_up(m->plan.primary, m->hparams.dec_hidden, m->hparams.dec_intermediate,
                                                 entries, m->packed_gate_up, "canary_qwen")) {
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

    auto cc       = std::make_unique<CanaryQwenSession>();
    cc->model     = model;
    cc->n_threads = params->n_threads;
    cc->kv_type   = params->kv_type;
    cc->n_ctx     = transcribe_session_params_n_ctx(params);

    // Flash defaults: encoder off (the FastConformer rel-pos path has a manual
    // rel_shift trick ggml_flash_attn_ext doesn't subsume), decoder ON (~2.4x
    // decode speedup on jfk.wav, the Qwen3 step graph is dispatch-bound on
    // Metal). TRANSCRIBE_NO_FLASH / TRANSCRIBE_FORCE_FLASH override both stages.
    cc->encoder_use_flash = false;
    cc->decoder_use_flash = true;
    transcribe::flash::apply_env_overrides(cc->encoder_use_flash, cc->decoder_use_flash);

    auto * cm = static_cast<CanaryQwenModel *>(model);
    {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary,
                                            /*n_ctx=*/2048, cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim,
                                            cm->hparams.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary_qwen init_context: KV cache allocation failed "
                                "(n_ctx=2048, %d kv-heads x %d head-dim x %d layers) — "
                                "out of memory.",
                                cm->hparams.dec_n_kv_heads, cm->hparams.dec_head_dim, cm->hparams.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    }

    *out_ctx = cc.release();
    return TRANSCRIBE_OK;
}

void apply_thread_policy(CanaryQwenSession * cc) {
    transcribe::configure_sched_n_threads(cc->sched, cc->n_threads);
}

void build_relpos_emb_host(std::vector<float> & pos_buf, std::vector<float> & div_term, int d_model, int T_enc) {
    // RelPositionalEncoding produces pos_emb of shape (1, 2*T_enc - 1, d_model).
    // The position values descend from (T_enc - 1) at index 0 down to
    // -(T_enc - 1) at index 2*T_enc - 2 (per NeMo's
    // `positions = torch.arange(length - 1, -length, -1, dtype=torch.float32)`).
    const int pos_len = 2 * T_enc - 1;
    pos_buf.assign(static_cast<size_t>(pos_len) * d_model, 0.0f);
    div_term.resize(static_cast<size_t>(d_model / 2));

    // div_term[k] = exp(-2k * ln(10000) / d_model)
    const float ln_10000 = std::log(10000.0f);
    for (int k = 0; k < d_model / 2; ++k) {
        div_term[static_cast<size_t>(k)] =
            std::exp(static_cast<float>(2 * k) * (-ln_10000 / static_cast<float>(d_model)));
    }
    for (int i = 0; i < pos_len; ++i) {
        const float pos = static_cast<float>((T_enc - 1) - i);
        float *     row = pos_buf.data() + static_cast<size_t>(i) * d_model;
        for (int k = 0; k < d_model / 2; ++k) {
            const float div = div_term[static_cast<size_t>(k)];
            row[2 * k]      = std::sin(pos * div);
            row[2 * k + 1]  = std::cos(pos * div);
        }
    }
}

transcribe_status run(transcribe_session *          context,
                      const float *                 pcm,
                      int                           n_samples,
                      const transcribe_run_params * params) {
    if (context == nullptr || pcm == nullptr || n_samples <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    auto * cc = static_cast<CanaryQwenSession *>(context);
    auto * cm = static_cast<CanaryQwenModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    (void) params;

    if (cc->poll_abort()) {
        return TRANSCRIBE_ERR_ABORTED;
    }

    transcribe::debug::init();
    cc->clear_result();

    const auto & hp = cm->hparams;

    // ---- Mel frontend ----
    const int64_t t_mel_start  = ggml_time_us();
    int           mel_n_mels   = 0;
    int           mel_n_frames = 0;
    if (auto mst =
            cm->mel->compute(pcm, static_cast<size_t>(n_samples), cc->mel_buf, mel_n_mels, mel_n_frames, cc->n_threads);
        mst != TRANSCRIBE_OK) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: MelFrontend::compute failed (%s)",
                transcribe_status_string(mst));
        return mst;
    }
    cc->t_mel_us = ggml_time_us() - t_mel_start;

    // Encoder.
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
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: ggml_init failed (encoder)");
            return TRANSCRIBE_ERR_GGUF;
        }
    }

    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, mel_n_frames,
                                          /*kv_type=*/GGML_TYPE_COUNT, cc->encoder_use_flash, cm->backend.c_str());
    if (eb.graph == nullptr || eb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (cc->sched == nullptr) {
        cc->sched = ggml_backend_sched_new(cm->plan.scheduler_list.data(), nullptr,
                                           static_cast<int>(cm->plan.scheduler_list.size()), 16384, /*parallel=*/false,
                                           /*op_offload=*/true);
        if (cc->sched == nullptr) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: ggml_backend_sched_new failed");
            return TRANSCRIBE_ERR_GGUF;
        }
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, eb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "canary_qwen run: encoder graph allocation failed — out of memory.");
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(eb.mel_in, cc->mel_buf.data(), 0, cc->mel_buf.size() * sizeof(float));
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { mel_n_mels, mel_n_frames };
        transcribe::debug::dump_host_f32("enc.mel.in", cc->mel_buf.data(), static_cast<long long>(cc->mel_buf.size()),
                                         shape, 2, "frontend.mel.norm");
    }

    // Upload relative-position embedding.
    if (eb.pos_emb_in != nullptr) {
        const int d_model = hp.enc_d_model;
        const int pos_len = static_cast<int>(eb.pos_emb_in->ne[1]);
        const int T_enc   = (pos_len + 1) / 2;
        build_relpos_emb_host(cc->pos_buf, cc->pos_div_term, d_model, T_enc);
        ggml_backend_tensor_set(eb.pos_emb_in, cc->pos_buf.data(), 0, cc->pos_buf.size() * sizeof(float));
        if (transcribe::debug::enabled()) {
            transcribe::debug::dump_tensor("enc.pos_emb", eb.pos_emb_in, "encoder.pos_emb");
        }
    }

    apply_thread_policy(cc);

    const int64_t t_enc_start = ggml_time_us();
    if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, eb.graph); gs != GGML_STATUS_SUCCESS) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: encoder compute failed (%d)", static_cast<int>(gs));
        return TRANSCRIBE_ERR_GGUF;
    }
    cc->t_encode_us = ggml_time_us() - t_enc_start;

    auto try_dump = [](const char * name, ggml_tensor * t, const char * stage) {
        if (t != nullptr) {
            transcribe::debug::dump_tensor(name, t, stage);
        }
    };
    try_dump("enc.pre_encode.out", eb.dumps.pre_encode_out, "encoder.pre_encode");
    try_dump("enc.block.0.out", eb.dumps.block0_out, "encoder.block0");
    if (eb.dumps.block_mid_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", hp.enc_n_layers / 2);
        try_dump(nm, eb.dumps.block_mid_out, "encoder.block_mid");
    }
    if (eb.dumps.block_last_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "enc.block.%d.out", hp.enc_n_layers - 1);
        try_dump(nm, eb.dumps.block_last_out, "encoder.block_last");
    }
    try_dump("enc.final", eb.dumps.final_out, "encoder.final");
    try_dump("perception.proj.out", eb.dumps.perception_out, "perception.proj.out");

    // Read perception output to host.
    const int hidden = static_cast<int>(eb.out->ne[0]);
    const int T_enc  = static_cast<int>(eb.out->ne[1]);
    cc->enc_host.resize(static_cast<size_t>(hidden) * static_cast<size_t>(T_enc));
    ggml_backend_tensor_get(eb.out, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));

    // Build prompt token-id list.
    std::vector<int32_t> prompt_ids;
    prompt_ids.reserve(cm->prompt_prefix_ids.size() + T_enc + cm->prompt_suffix_ids.size());
    prompt_ids.insert(prompt_ids.end(), cm->prompt_prefix_ids.begin(), cm->prompt_prefix_ids.end());
    const int prefix_len = static_cast<int>(prompt_ids.size());
    for (int i = 0; i < T_enc; ++i) {
        prompt_ids.push_back(hp.audio_locator_id);
    }
    prompt_ids.insert(prompt_ids.end(), cm->prompt_suffix_ids.begin(), cm->prompt_suffix_ids.end());
    const int T_prompt   = static_cast<int>(prompt_ids.size());
    const int suffix_len = T_prompt - prefix_len - T_enc;

    // Input-length gate: audio + prompt + generation must fit the decoder
    // context window. Reject an over-length clip here, before prefill/decode.
    const int ceiling = canary_qwen_context_ceiling(cc->n_ctx, hp);
    if (T_prompt + k_max_new > ceiling) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "canary_qwen run: input too long — %d audio + %d prompt tokens "
                            "leave no room for output within the %d-token context (need %d). "
                            "Shorten the audio (see transcribe_capabilities.max_audio_ms) or "
                            "split it into segments.",
                            T_enc, prefix_len + suffix_len, ceiling, T_prompt + k_max_new);
        return TRANSCRIBE_ERR_INPUT_TOO_LONG;
    }

    // KV cache init (grow-to-fit, clamped to the context ceiling). Size to
    // hold prompt + generation budget, rounded up to a power of two (the step
    // graph's flash-attn path wants pow2 attention width). A pre-allocated
    // smaller cache is freed and re-allocated.
    int want_n_ctx = 1024;
    while (want_n_ctx < T_prompt + k_max_new) {
        want_n_ctx *= 2;
    }
    if (want_n_ctx > ceiling) {
        want_n_ctx = ceiling;
    }
    if (cc->kv_cache.ctx != nullptr && cc->kv_cache.n_ctx < want_n_ctx) {
        cc->kv_cache.free();
    }
    if (cc->kv_cache.ctx == nullptr) {
        ggml_type kv_type = GGML_TYPE_F16;
        if (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) {
            kv_type = GGML_TYPE_F32;
        }
        if (!transcribe::causal_lm::kv_init(cc->kv_cache, cm->plan.primary, want_n_ctx, hp.dec_n_kv_heads,
                                            hp.dec_head_dim, hp.dec_n_layers, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary_qwen run: KV cache allocation failed (n_ctx=%d, "
                                "%d kv-heads x %d head-dim x %d layers) — out of memory. "
                                "Lower transcribe_session_params.n_ctx or shorten the audio.",
                                want_n_ctx, hp.dec_n_kv_heads, hp.dec_head_dim, hp.dec_n_layers);
            return TRANSCRIBE_ERR_OOM;
        }
    } else {
        // Clear stale positions for a fresh prefill.
        if (cc->kv_cache.buffer != nullptr) {
            ggml_backend_buffer_clear(cc->kv_cache.buffer, 0);
        }
        cc->kv_cache.n    = 0;
        cc->kv_cache.head = 0;
    }

    const int64_t t_dec_start = ggml_time_us();

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
                                "canary_qwen run: prefill compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }

    const bool   dumps_on   = transcribe::debug::enabled();
    const bool   slice_last = !dumps_on;
    PrefillBuild pb = build_prefill_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, T_prompt, T_enc, prefix_len,
                                          suffix_len, cc->decoder_use_flash, slice_last);
    if (pb.graph == nullptr || pb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "canary_qwen run: prefill graph allocation failed (T_prompt=%d) — "
                            "out of memory. Lower transcribe_session_params.n_ctx or shorten "
                            "the audio.",
                            T_prompt);
        return TRANSCRIBE_ERR_OOM;
    }

    ggml_backend_tensor_set(pb.input_ids_in, prompt_ids.data(), 0, prompt_ids.size() * sizeof(int32_t));
    if (T_enc > 0 && pb.audio_in != nullptr) {
        ggml_backend_tensor_set(pb.audio_in, cc->enc_host.data(), 0, cc->enc_host.size() * sizeof(float));
    }
    {
        std::vector<int32_t> positions(T_prompt);
        for (int i = 0; i < T_prompt; ++i) {
            positions[i] = i;
        }
        ggml_backend_tensor_set(pb.positions_in, positions.data(), 0, positions.size() * sizeof(int32_t));
    }
    {
        const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
        const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
        std::vector<ggml_fp16_t> mask(static_cast<size_t>(T_prompt) * T_prompt, mask_neg_inf);
        for (int r = 0; r < T_prompt; ++r) {
            for (int c = 0; c <= r; ++c) {
                mask[static_cast<size_t>(r) * T_prompt + c] = mask_zero;
            }
        }
        ggml_backend_tensor_set(pb.mask_in, mask.data(), 0, mask.size() * sizeof(ggml_fp16_t));
    }

    const bool profile_decode       = transcribe::env::flag("TRANSCRIBE_PERF_DEBUG");
    int64_t    t_prefill_us         = 0;
    int64_t    t_prefill_compute_us = 0;
    {
        const int64_t t0 = ggml_time_us();
        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, pb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: prefill compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }
        t_prefill_compute_us = ggml_time_us() - t0;
        t_prefill_us         = t_prefill_compute_us;
    }

    cc->kv_cache.n    = T_prompt;
    cc->kv_cache.head = T_prompt;

    try_dump("dec.token_emb", pb.dumps.token_emb, "decoder.token_emb");
    try_dump("dec.audio_injected", pb.dumps.audio_injected, "decoder.audio_injected");
    try_dump("dec.block.0.out", pb.dumps.block_0_out, "decoder.block0");
    if (pb.dumps.block_mid_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", hp.dec_n_layers / 2);
        try_dump(nm, pb.dumps.block_mid_out, "decoder.block_mid");
    }
    if (pb.dumps.block_last_out != nullptr) {
        char nm[64];
        std::snprintf(nm, sizeof(nm), "dec.block.%d.out", hp.dec_n_layers - 1);
        try_dump(nm, pb.dumps.block_last_out, "decoder.block_last");
    }
    try_dump("dec.out_before_head", pb.dumps.out_before_head, "decoder.out_before_head");
    try_dump("dec.logits_raw.gen0", pb.dumps.logits_raw, "decoder.logits.gen0");

    // ---- Read prefill logits + first argmax ----
    const int          vocab = hp.dec_vocab_size;
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

    // Step loop.
    const int32_t eos_id   = hp.eos_token_id;
    const int     max_new  = k_max_new;
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
        ip.mem_buffer   = nullptr;
        ip.no_alloc     = true;
        cc->compute_ctx = ggml_init(ip);
        if (cc->compute_ctx == nullptr) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary_qwen step: compute context allocation failed — "
                                "out of memory.");
            return TRANSCRIBE_ERR_OOM;
        }
    }
    StepBuild sb = build_step_graph(cc->compute_ctx, cm->weights, hp, cc->kv_cache, max_n_kv, cc->decoder_use_flash);

    if (profile_decode && sb.graph != nullptr) {
        const int n_nodes                  = ggml_graph_n_nodes(sb.graph);
        int       op_counts[GGML_OP_COUNT] = { 0 };
        for (int ni = 0; ni < n_nodes; ++ni) {
            ggml_tensor * t = ggml_graph_node(sb.graph, ni);
            if (t != nullptr) {
                op_counts[t->op] += 1;
            }
        }
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode] step graph: n_nodes=%d", n_nodes);
        int shown = 0;
        for (int o = 0; o < GGML_OP_COUNT && shown < 12; ++o) {
            if (op_counts[o] > 0) {
                log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode]   op %-22s x %4d",
                        ggml_op_name(static_cast<ggml_op>(o)), op_counts[o]);
                ++shown;
            }
        }
    }
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                            "canary_qwen step: decode graph allocation failed — out of memory. "
                            "Lower transcribe_session_params.n_ctx or shorten the audio.");
        return TRANSCRIBE_ERR_OOM;
    }

    const ggml_fp16_t        mask_zero    = ggml_fp32_to_fp16(0.0f);
    const ggml_fp16_t        mask_neg_inf = ggml_fp32_to_fp16(-INFINITY);
    std::vector<ggml_fp16_t> step_mask(max_n_kv, mask_neg_inf);

    // Mid-generation tensor coverage: `dec.logits_raw.gen8` is the logits for
    // the 9th lm_head call (= step iter 7; prefill = 1st call, iter K =
    // (K+2)th call).
    constexpr int gen_dump_step = 7;
    int           n_steps       = 0;

    int64_t              t_step_input_set_us  = 0;
    int64_t              t_step_compute_us    = 0;
    int64_t              t_step_argmax_get_us = 0;
    std::vector<int64_t> per_step_compute_us;
    if (profile_decode) {
        per_step_compute_us.reserve(64);
    }

    while (next_tok != eos_id && static_cast<int32_t>(generated_ids.size()) < max_new && cur_past + 1 <= max_n_kv) {
        const int64_t t_in0 = profile_decode ? ggml_time_us() : 0;
        ggml_backend_tensor_set(sb.input_id_in, &next_tok, 0, sizeof(int32_t));
        const int32_t pos_val = cur_past;
        ggml_backend_tensor_set(sb.position_in, &pos_val, 0, sizeof(int32_t));
        const int64_t kv_idx_val = cur_past;
        ggml_backend_tensor_set(sb.kv_idx_in, &kv_idx_val, 0, sizeof(int64_t));

        if (cur_past == T_prompt) {
            std::fill(step_mask.begin(), step_mask.begin() + cur_past + 1, mask_zero);
        } else {
            step_mask[cur_past] = mask_zero;
        }
        ggml_backend_tensor_set(sb.mask_in, step_mask.data(), 0, static_cast<size_t>(max_n_kv) * sizeof(ggml_fp16_t));
        if (profile_decode) {
            t_step_input_set_us += ggml_time_us() - t_in0;
        }

        const int64_t t_c0 = profile_decode ? ggml_time_us() : 0;
        if (const ggml_status gs = ggml_backend_sched_graph_compute(cc->sched, sb.graph); gs != GGML_STATUS_SUCCESS) {
            log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "canary_qwen run: step compute failed (%d)", static_cast<int>(gs));
            return TRANSCRIBE_ERR_GGUF;
        }
        if (profile_decode) {
            const int64_t dt = ggml_time_us() - t_c0;
            t_step_compute_us += dt;
            per_step_compute_us.push_back(dt);
        }

        const int64_t t_a0       = profile_decode ? ggml_time_us() : 0;
        int32_t       argmax_tok = 0;
        ggml_backend_tensor_get(sb.out, &argmax_tok, 0, sizeof(int32_t));
        if (profile_decode) {
            t_step_argmax_get_us += ggml_time_us() - t_a0;
        }
        next_tok = argmax_tok;
        generated_ids.push_back(next_tok);

        if (n_steps == gen_dump_step && transcribe::debug::enabled()) {
            try_dump("dec.logits_raw.gen8", sb.logits, "decoder.logits.gen8");
        }

        cur_past += 1;
        n_steps += 1;
    }

    // Decode stopped at EOS (complete) or the generation budget / context width
    // (truncated). Surface the latter via transcribe_was_truncated() + WARN.
    if (next_tok != eos_id) {
        cc->was_truncated = true;
        transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_WARN,
                            "canary_qwen run: output truncated at %d tokens — decode reached "
                            "the generation budget before end-of-stream; the transcript may "
                            "be incomplete.",
                            static_cast<int>(generated_ids.size()));
    }

    if (profile_decode) {
        const int n = n_steps;
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode] T_prompt=%d max_n_kv=%d steps=%d use_flash=%d", T_prompt,
                max_n_kv, n, cc->decoder_use_flash ? 1 : 0);
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode] prefill_compute=%.2f ms", t_prefill_compute_us / 1000.0);
        log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                "[profile_decode] step totals: input_set=%.2f ms compute=%.2f ms argmax_get=%.2f ms",
                t_step_input_set_us / 1000.0, t_step_compute_us / 1000.0, t_step_argmax_get_us / 1000.0);
        if (n > 0) {
            std::vector<int64_t> sorted = per_step_compute_us;
            std::sort(sorted.begin(), sorted.end());
            const auto pct = [&](double p) {
                size_t idx = static_cast<size_t>(p * (n - 1));
                if (idx >= sorted.size()) {
                    idx = sorted.size() - 1;
                }
                return sorted[idx] / 1000.0;
            };
            log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG,
                    "[profile_decode] per-step compute ms: mean=%.2f p50=%.2f p90=%.2f p99=%.2f min=%.2f max=%.2f",
                    (t_step_compute_us / 1000.0) / n, pct(0.50), pct(0.90), pct(0.99), sorted.front() / 1000.0,
                    sorted.back() / 1000.0);
            log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode] per-step input_set ms: mean=%.3f",
                    (t_step_input_set_us / 1000.0) / n);
            log_msg(TRANSCRIBE_LOG_LEVEL_DEBUG, "[profile_decode] per-step argmax_get ms: mean=%.3f",
                    (t_step_argmax_get_us / 1000.0) / n);
        }
    }
    (void) n_steps;

    if (!generated_ids.empty() && generated_ids.back() == eos_id) {
        generated_ids.pop_back();
    }

    std::string transcript = cm->tok.decode(generated_ids.data(), static_cast<int>(generated_ids.size()));

    cc->t_decode_us = ggml_time_us() - t_dec_start;

    cc->full_text   = transcript;
    cc->result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
    cc->has_result  = true;

    transcribe_session::SegmentEntry seg{};
    seg.text  = transcript;
    seg.t0_ms = 0;
    seg.t1_ms = static_cast<int64_t>(n_samples) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
    cc->segments.push_back(std::move(seg));

    // A truncated decode returns OUTPUT_TRUNCATED; the partial transcript above
    // stays readable (like an aborted run).
    return cc->was_truncated ? TRANSCRIBE_ERR_OUTPUT_TRUNCATED : TRANSCRIBE_OK;
}

}  // namespace

// ===========================================================================
// Offline batched decode (transcribe_run_batch)
// ===========================================================================
// Serial mel + conformer encoder (incl. perception projection) per utterance
// produce each one's audio embedding [hidden, T_enc]; the prefill and
// autoregressive step loop are then batched via the shared causal_lm
// primitives.

namespace {

transcribe_status reset_ctx(CanaryQwenSession * cc, int mb) {
    if (cc->compute_ctx != nullptr) {
        ggml_free(cc->compute_ctx);
        cc->compute_ctx = nullptr;
    }
    ggml_init_params ip{};
    ip.mem_size     = static_cast<size_t>(mb) * 1024 * 1024;
    ip.mem_buffer   = nullptr;
    ip.no_alloc     = true;
    cc->compute_ctx = ggml_init(ip);
    return cc->compute_ctx != nullptr ? TRANSCRIBE_OK : TRANSCRIBE_ERR_GGUF;
}

// Conformer encoder (+ perception) for one utterance from a PRECOMPUTED mel
// buffer (the mel is computed in parallel by the caller) → [hidden, T_enc].
transcribe_status encode_one(CanaryQwenSession *        cc,
                             CanaryQwenModel *          cm,
                             const std::vector<float> & mel_buf,
                             int                        mel_n_frames,
                             std::vector<float> &       enc_out,
                             int &                      T_enc_out,
                             int64_t &                  enc_us) {
    const auto & hp = cm->hparams;
    if (mel_n_frames <= 0) {
        return TRANSCRIBE_ERR_GGUF;
    }

    if (reset_ctx(cc, 32) != TRANSCRIBE_OK) {
        return TRANSCRIBE_ERR_GGUF;
    }
    EncoderBuild eb = build_encoder_graph(cc->compute_ctx, cm->weights, hp, mel_n_frames,
                                          /*kv_type=*/GGML_TYPE_COUNT, cc->encoder_use_flash, cm->backend.c_str());
    if (eb.graph == nullptr || eb.out == nullptr) {
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
        return TRANSCRIBE_ERR_GGUF;
    }

    ggml_backend_tensor_set(eb.mel_in, mel_buf.data(), 0, mel_buf.size() * sizeof(float));
    if (eb.pos_emb_in != nullptr) {
        const int d_model = hp.enc_d_model;
        const int pos_len = static_cast<int>(eb.pos_emb_in->ne[1]);
        const int T_enc   = (pos_len + 1) / 2;
        build_relpos_emb_host(cc->pos_buf, cc->pos_div_term, d_model, T_enc);
        ggml_backend_tensor_set(eb.pos_emb_in, cc->pos_buf.data(), 0, cc->pos_buf.size() * sizeof(float));
    }
    apply_thread_policy(cc);

    const int64_t t_enc0 = ggml_time_us();
    if (ggml_backend_sched_graph_compute(cc->sched, eb.graph) != GGML_STATUS_SUCCESS) {
        return TRANSCRIBE_ERR_GGUF;
    }
    enc_us += ggml_time_us() - t_enc0;

    const int hidden = static_cast<int>(eb.out->ne[0]);
    T_enc_out        = static_cast<int>(eb.out->ne[1]);
    enc_out.resize(static_cast<size_t>(hidden) * T_enc_out);
    ggml_backend_tensor_get(eb.out, enc_out.data(), 0, enc_out.size() * sizeof(float));
    return TRANSCRIBE_OK;
}

transcribe_status run_batch_serial(CanaryQwenSession *           cc,
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

}  // namespace

transcribe_status run_batch(transcribe_session *          session,
                            const float * const *         pcm,
                            const int *                   n_samples,
                            int                           n,
                            const transcribe_run_params * params) {
    if (session == nullptr || pcm == nullptr || n_samples == nullptr || n <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    auto * cc = static_cast<CanaryQwenSession *>(session);
    auto * cm = static_cast<CanaryQwenModel *>(cc->model);
    if (cm == nullptr || cm->plan.scheduler_list.empty() || !cm->mel.has_value()) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    if (!cc->decoder_use_flash || transcribe::debug::enabled() || n == 1) {
        return run_batch_serial(cc, pcm, n_samples, n, params);
    }

    transcribe::debug::init();
    const auto & hp = cm->hparams;

    // Decoder context ceiling for the input-length gate. Same value the
    // single-utterance run() enforces.
    const int ceiling = canary_qwen_context_ceiling(cc->n_ctx, hp);

    std::vector<char>                 valid(n, 0);
    // Per-utterance failure status. Defaults to INVALID_ARG (bad pcm/mel/
    // encode); the input-length gate upgrades it to INPUT_TOO_LONG.
    std::vector<transcribe_status>    fail_status(n, TRANSCRIBE_ERR_INVALID_ARG);
    std::vector<std::vector<float>>   enc_hosts(n);
    std::vector<int>                  T_enc(n, 0);
    std::vector<std::vector<int32_t>> prompt_ids(n);
    std::vector<int>                  T_prompt(n, 0);
    int                               prefix_len = static_cast<int>(cm->prompt_prefix_ids.size());
    int64_t                           mel_us = 0, enc_us = 0;

    // Pass 0: parallel mel (MelFrontend::compute is thread-safe).
    std::vector<std::vector<float>> mel_bufs(n);
    std::vector<int>                mel_nf(n, 0);
    int                             n_mel_threads = cc->n_threads;
    if (n_mel_threads <= 0) {
        n_mel_threads = transcribe::default_n_threads();
    }
    const int64_t t_mel0 = ggml_time_us();
    transcribe::parallel_for_all(n, n_mel_threads, [&](int b) {
        if (pcm[b] == nullptr || n_samples[b] <= 0) {
            return true;
        }
        int nm = 0, nf = 0;
        if (cm->mel->compute(pcm[b], static_cast<size_t>(n_samples[b]), mel_bufs[b], nm, nf, /*n_threads=*/1) ==
            TRANSCRIBE_OK) {
            mel_nf[b] = nf;
        }
        return true;
    });
    mel_us += ggml_time_us() - t_mel0;

    // Pass 1: per-utterance encoder (serial).
    for (int b = 0; b < n; ++b) {
        if (cc->poll_abort()) {
            return TRANSCRIBE_ERR_ABORTED;
        }
        if (mel_nf[b] <= 0) {
            continue;
        }
        if (encode_one(cc, cm, mel_bufs[b], mel_nf[b], enc_hosts[b], T_enc[b], enc_us) != TRANSCRIBE_OK) {
            continue;
        }
        // Prompt: prefix + audio_locator * T_enc + suffix.
        prompt_ids[b] = cm->prompt_prefix_ids;
        for (int i = 0; i < T_enc[b]; ++i) {
            prompt_ids[b].push_back(hp.audio_locator_id);
        }
        prompt_ids[b].insert(prompt_ids[b].end(), cm->prompt_suffix_ids.begin(), cm->prompt_suffix_ids.end());
        T_prompt[b] = static_cast<int>(prompt_ids[b].size());

        // Input-length gate (same as single-shot run()); reject this utterance,
        // the rest of the batch still runs.
        if (T_prompt[b] + k_max_new > ceiling) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary_qwen run_batch: utterance %d input too long — %d audio "
                                "+ %d prompt tokens leave no room for output within the "
                                "%d-token context (need %d). Shorten the audio (see "
                                "transcribe_capabilities.max_audio_ms) or split it.",
                                b, T_enc[b], T_prompt[b] - T_enc[b], ceiling, T_prompt[b] + k_max_new);
            fail_status[b] = TRANSCRIBE_ERR_INPUT_TOO_LONG;
            continue;
        }
        valid[b] = 1;
    }

    int max_T_prompt = 0, T_enc_max = 0;
    for (int b = 0; b < n; ++b) {
        if (valid[b]) {
            max_T_prompt = std::max(max_T_prompt, T_prompt[b]);
            T_enc_max    = std::max(T_enc_max, T_enc[b]);
        }
    }
    if (max_T_prompt == 0) {
        for (int b = 0; b < n; ++b) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
        }
        return TRANSCRIBE_OK;
    }
    T_enc_max          = std::max(1, T_enc_max);
    const int max_new  = k_max_new;
    int       max_n_kv = 1024;
    while (max_n_kv < max_T_prompt + max_new) {
        max_n_kv *= 2;
    }
    // Clamp the pow2 round-up to the ceiling (the per-utterance gate guarantees
    // every valid row still fits).
    if (max_n_kv > ceiling) {
        max_n_kv = ceiling;
    }

    // Allocate batched KV cache.
    ggml_type kv_type = (cc->kv_type == TRANSCRIBE_KV_TYPE_F32) ? GGML_TYPE_F32 : GGML_TYPE_F16;
    if (cc->kv_cache_batch.self_k == nullptr || cc->kv_batch_cap != n || cc->kv_batch_n_ctx != max_n_kv) {
        cc->kv_cache_batch.free();
        if (!transcribe::causal_lm::kv_init_batched(cc->kv_cache_batch, cm->plan.primary, max_n_kv, hp.dec_n_kv_heads,
                                                    hp.dec_head_dim, hp.dec_n_layers, n, kv_type)) {
            transcribe::log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                                "canary_qwen run_batch: batched KV cache allocation failed "
                                "(n_ctx=%d x %d utterances) — out of memory. Lower "
                                "transcribe_session_params.n_ctx or the batch size.",
                                max_n_kv, n);
            return TRANSCRIBE_ERR_OOM;
        }
        cc->kv_batch_cap   = n;
        cc->kv_batch_n_ctx = max_n_kv;
    } else if (cc->kv_cache_batch.buffer != nullptr) {
        ggml_backend_buffer_clear(cc->kv_cache_batch.buffer, 0);
    }

    // Pass 2: batched prefill.
    std::vector<int32_t>              next_tok(n, 0);
    std::vector<int>                  n_past(n, 0);
    std::vector<std::vector<int32_t>> generated(n);
    {
        if (reset_ctx(cc, 32) != TRANSCRIBE_OK) {
            return TRANSCRIBE_ERR_GGUF;
        }
        PrefillBuildBatched pb = build_prefill_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch,
                                                             max_T_prompt, T_enc_max, n, cc->decoder_use_flash);
        if (pb.graph == nullptr || pb.out == nullptr) {
            return TRANSCRIBE_ERR_GGUF;
        }
        ggml_backend_sched_reset(cc->sched);
        if (!ggml_backend_sched_alloc_graph(cc->sched, pb.graph)) {
            return TRANSCRIBE_ERR_GGUF;
        }

        const int            hidden = hp.dec_hidden;
        std::vector<int32_t> ids(static_cast<size_t>(max_T_prompt) * n, 0);
        // Audio injection by elementwise blend (see decoder.h): audio_dense
        // holds each utterance's audio embeds scattered into their prompt
        // positions, keep is 0 there and 1 elsewhere.
        std::vector<float>   audio_dense(static_cast<size_t>(hidden) * max_T_prompt * n, 0.0f);
        std::vector<float>   keep(static_cast<size_t>(max_T_prompt) * n, 1.0f);
        std::vector<int64_t> kidx(static_cast<size_t>(max_T_prompt) * n);
        std::vector<int32_t> lidx(n, 0);
        for (int b = 0; b < n; ++b) {
            const int ta = valid[b] ? T_enc[b] : 0;
            const int tp = valid[b] ? T_prompt[b] : 0;
            if (valid[b]) {
                std::memcpy(ids.data() + static_cast<size_t>(b) * max_T_prompt, prompt_ids[b].data(),
                            static_cast<size_t>(tp) * sizeof(int32_t));
                // enc_hosts[b] is [hidden, ta] column-major; audio token j lands
                // at prompt position prefix_len+j, flat column b*max_T_prompt+pos.
                for (int j = 0; j < ta; ++j) {
                    const size_t dst_col = static_cast<size_t>(b) * max_T_prompt + (prefix_len + j);
                    std::memcpy(audio_dense.data() + dst_col * hidden,
                                enc_hosts[b].data() + static_cast<size_t>(j) * hidden,
                                static_cast<size_t>(hidden) * sizeof(float));
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
        apply_thread_policy(cc);
        if (ggml_backend_sched_graph_compute(cc->sched, pb.graph) != GGML_STATUS_SUCCESS) {
            return TRANSCRIBE_ERR_GGUF;
        }
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

    // Pass 3: batched step loop (shared causal_lm driver).
    const int32_t eos_id = cm->hparams.eos_token_id;

    if (reset_ctx(cc, 16) != TRANSCRIBE_OK) {
        return TRANSCRIBE_ERR_GGUF;
    }
    StepBuildBatched sb = build_step_graph_batched(cc->compute_ctx, cm->weights, hp, cc->kv_cache_batch, max_n_kv, n,
                                                   cc->decoder_use_flash);
    if (sb.graph == nullptr || sb.out == nullptr) {
        return TRANSCRIBE_ERR_GGUF;
    }
    ggml_backend_sched_reset(cc->sched);
    if (!ggml_backend_sched_alloc_graph(cc->sched, sb.graph)) {
        return TRANSCRIBE_ERR_GGUF;
    }

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
    std::vector<char>                    truncated;
    if (const transcribe_status st = transcribe::causal_lm::run_batched_step_loop(
            cc, cc->sched, io, n, max_n_kv, eos_id, max_new, step_state, generated, &step_stats, &truncated);
        st != TRANSCRIBE_OK) {
        return st;
    }
    const int64_t step_us = step_stats.step_us;

    // Capture results.
    const int valid_count = std::max(1, static_cast<int>(std::count(valid.begin(), valid.end(), char(1))));
    for (int b = 0; b < n; ++b) {
        if (!valid[b]) {
            transcribe_session::ResultSet rs;
            rs.status = fail_status[b];
            cc->batch_results.push_back(std::move(rs));
            continue;
        }
        std::vector<int32_t> gen = generated[b];
        if (!gen.empty() && gen.back() == eos_id) {
            gen.pop_back();
        }
        std::string                   transcript = cm->tok.decode(gen.data(), static_cast<int>(gen.size()));
        transcribe_session::ResultSet rs;
        rs.full_text   = transcript;
        rs.result_kind = TRANSCRIBE_TIMESTAMPS_NONE;
        rs.has_result  = true;
        rs.status      = TRANSCRIBE_OK;
        transcribe_session::SegmentEntry seg{};
        seg.text  = transcript;
        seg.t0_ms = 0;
        seg.t1_ms = static_cast<int64_t>(n_samples[b]) * 1000 / static_cast<int64_t>(hp.fe_sample_rate);
        rs.segments.push_back(std::move(seg));
        // Per-utterance truncation parity with single-shot run(). Only override
        // a TRANSCRIBE_OK status, never a worse one.
        if (b < static_cast<int>(truncated.size()) && truncated[b] && rs.status == TRANSCRIBE_OK) {
            cc->was_truncated = true;
            rs.status         = TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        }
        rs.t_mel_us    = mel_us / valid_count;
        rs.t_encode_us = enc_us / valid_count;
        rs.t_decode_us = step_us / valid_count;
        cc->batch_results.push_back(std::move(rs));
    }
    return TRANSCRIBE_OK;
}

extern const Arch arch = {
    /* .name             = */ "canary_qwen",
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

}  // namespace transcribe::canary_qwen
