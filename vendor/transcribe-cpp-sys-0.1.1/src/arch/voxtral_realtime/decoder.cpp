// arch/voxtral_realtime/decoder.cpp - Ministral LM prefill/step builders.
//
// Reference: VoxtralRealtimeForConditionalGeneration's inner language_model
// (VoxtralRealtimeTextForCausalLM). Per-block math via shared causal_lm with
// null Q/K-norm and a per-layer ffn_scale = (1 + ada(t_cond)). ADDITIVE audio
// fusion and TIED lm_head are owned here.

#include "decoder.h"

#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"

#include <cstdio>

namespace transcribe::voxtral_realtime {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

// `ffn_norm_folded` is the per-layer (1+ada(t_cond)) ⊙ norm_ffn_w precomputed per
// session. Passing it as the FFN-norm weight lets the fused
// rms_norm(·weight) apply the ada scale — no separate per-layer ggml_mul. Null
// falls back to the raw FFN-norm weight (ada not yet computed).
causal_lm::BlockView to_block_view(const DecBlock & b, ggml_tensor * ffn_norm_folded) {
    causal_lm::BlockView v{};
    v.norm_attn_w   = b.norm_attn_w;
    v.norm_ffn_w    = (ffn_norm_folded != nullptr) ? ffn_norm_folded : b.norm_ffn_w;
    v.attn_q_w      = b.attn_q_w;
    v.attn_k_w      = b.attn_k_w;
    v.attn_v_w      = b.attn_v_w;
    v.attn_o_w      = b.attn_o_w;
    v.attn_q_norm   = nullptr;  // Ministral: no Q/K norm
    v.attn_k_norm   = nullptr;
    v.ffn_gate_up_w = b.ffn_gate_up_w;
    v.ffn_down_w    = b.ffn_down_w;
    v.ffn_scale     = nullptr;  // ada scale folded into norm_ffn_w
    return v;
}

causal_lm::BlockParams to_block_params(const HParams & hp) {
    causal_lm::BlockParams p{};
    p.n_heads      = hp.dec_n_heads;
    p.n_kv_heads   = hp.dec_n_kv_heads;
    p.head_dim     = hp.dec_head_dim;
    p.max_position = hp.dec_max_position;
    p.rms_eps      = hp.dec_rms_norm_eps;
    p.rope_theta   = hp.dec_rope_theta;
    return p;
}

// Per-layer ffn_scale view into ada_scale_all [hidden, n_layers].
ggml_tensor * ada_scale_for(ggml_context * ctx, ggml_tensor * ada_scale_all, int64_t hidden, int layer) {
    if (ada_scale_all == nullptr) {
        return nullptr;
    }
    return ggml_view_1d(ctx, ada_scale_all, hidden,
                        ggml_element_size(ada_scale_all) * hidden * static_cast<size_t>(layer));
}

}  // namespace

PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const Weights &                  weights,
                                 const HParams &                  hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 ggml_tensor *                    ada_scale_all,
                                 int                              T,
                                 bool                             use_flash,
                                 bool                             want_all_logits) {
    PrefillBuild pb{};
    pb.T = T;
    if (ctx == nullptr || T <= 0 || kv_cache.self_k == nullptr || T > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime prefill: invalid arg (T=%d)", T);
        return pb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const auto    block_params = to_block_params(hp);

    pb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T);
    named(pb.input_ids_in, "dec.input_ids");
    ggml_set_input(pb.input_ids_in);

    pb.audio_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T);
    named(pb.audio_in, "dec.audio_in");
    ggml_set_input(pb.audio_in);

    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T);
    named(pb.positions_in, "dec.positions");
    ggml_set_input(pb.positions_in);

    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T, T);
    named(pb.mask_in, "dec.attn_mask");
    ggml_set_input(pb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime prefill: graph alloc failed");
        return pb;
    }
    pb.graph = gf;

    // Token embedding + additive audio overlay.
    ggml_tensor * token_emb = ggml_get_rows(ctx, weights.dec_embed.token_w, pb.input_ids_in);
    named(token_emb, "dec.token_emb");
    pb.dumps.token_emb = token_emb;
    transcribe::debug::mark_tensor_for_dump(token_emb);

    ggml_tensor * x = ggml_add(ctx, token_emb, pb.audio_in);
    named(x, "dec.audio_injected");
    pb.dumps.audio_injected = x;
    transcribe::debug::mark_tensor_for_dump(x);

    for (int il = 0; il < n_layer; ++il) {
        causal_lm::BlockOpts opts{};
        opts.use_flash      = use_flash;
        ggml_tensor * scale = ada_scale_for(ctx, ada_scale_all, hidden, il);
        x = causal_lm::block_prefill(ctx, gf, x, to_block_view(weights.dec_blocks[il], scale), block_params, kv_cache,
                                     il, T, pb.mask_in, pb.positions_in, opts);
        if (il == 0) {
            named(x, "dec.block.0.out");
            pb.dumps.block_0_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        } else if (il == n_layer / 2) {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", il);
            named(x, nm);
            pb.dumps.block_mid_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        } else if (il == n_layer - 1) {
            char nm[64];
            std::snprintf(nm, sizeof(nm), "dec.block.%d.out", il);
            named(x, nm);
            pb.dumps.block_last_out = x;
            transcribe::debug::mark_tensor_for_dump(x);
        }
    }

    x = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    named(x, "dec.out_before_head");
    pb.dumps.out_before_head = x;
    transcribe::debug::mark_tensor_for_dump(x);

    ggml_tensor * logits;
    if (want_all_logits) {
        // TIED lm_head over every position: [vocab, T].
        logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, x);
        named(logits, "dec.logits_all");
        pb.dumps.logits_all = logits;
    } else {
        ggml_tensor * last_x = ggml_view_2d(ctx, x, hidden, 1, ggml_element_size(x) * hidden,
                                            ggml_element_size(x) * hidden * static_cast<size_t>(T - 1));
        last_x               = ggml_cont(ctx, last_x);
        logits               = ggml_mul_mat(ctx, weights.dec_embed.token_w, last_x);
        logits               = ggml_reshape_1d(ctx, logits, vocab);
        named(logits, "dec.logits_raw");
    }
    pb.out = logits;
    ggml_set_output(pb.out);

    ggml_build_forward_expand(gf, pb.out);
    if (pb.dumps.token_emb) {
        ggml_build_forward_expand(gf, pb.dumps.token_emb);
    }
    if (pb.dumps.audio_injected) {
        ggml_build_forward_expand(gf, pb.dumps.audio_injected);
    }
    if (pb.dumps.block_0_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_0_out);
    }
    if (pb.dumps.block_mid_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_mid_out);
    }
    if (pb.dumps.block_last_out) {
        ggml_build_forward_expand(gf, pb.dumps.block_last_out);
    }
    if (pb.dumps.out_before_head) {
        ggml_build_forward_expand(gf, pb.dumps.out_before_head);
    }

    return pb;
}

StepBuild build_step_graph(ggml_context *                   ctx,
                           const Weights &                  weights,
                           const HParams &                  hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           ggml_tensor *                    ada_scale_all,
                           int                              max_n_kv,
                           bool                             use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;
    if (ctx == nullptr || max_n_kv <= 0 || kv_cache.self_k == nullptr || max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime step: invalid arg (max_n_kv=%d)", max_n_kv);
        return sb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const auto    block_params = to_block_params(hp);

    sb.input_id_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.input_id_in, "step.input_id");
    ggml_set_input(sb.input_id_in);
    sb.audio_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, 1);
    ggml_set_name(sb.audio_in, "step.audio_in");
    ggml_set_input(sb.audio_in);
    sb.position_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.position_in, "step.position");
    ggml_set_input(sb.position_in);
    sb.kv_idx_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I64, 1);
    ggml_set_name(sb.kv_idx_in, "step.kv_idx");
    ggml_set_input(sb.kv_idx_in);
    sb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, max_n_kv, 1);
    ggml_set_name(sb.mask_in, "step.mask");
    ggml_set_input(sb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime step: graph alloc failed");
        return sb;
    }
    sb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w, sb.input_id_in);
    x               = ggml_add(ctx, x, sb.audio_in);

    for (int il = 0; il < n_layer; ++il) {
        ggml_tensor * scale = ada_scale_for(ctx, ada_scale_all, hidden, il);
        x = causal_lm::block_step(ctx, gf, x, to_block_view(weights.dec_blocks[il], scale), block_params, kv_cache, il,
                                  max_n_kv, sb.mask_in, sb.position_in, sb.kv_idx_in, use_flash);
    }

    x                    = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, x);
    logits               = ggml_reshape_1d(ctx, logits, vocab);
    ggml_set_name(logits, "step.logits");
    ggml_set_output(logits);
    sb.logits = logits;

    sb.out = ggml_argmax(ctx, logits);
    ggml_set_name(sb.out, "step.argmax");
    ggml_set_output(sb.out);

    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);
    return sb;
}

VerifyBuild build_verify_graph(ggml_context *                   ctx,
                               const Weights &                  weights,
                               const HParams &                  hp,
                               transcribe::causal_lm::KvCache & kv_cache,
                               ggml_tensor *                    ada_scale_all,
                               int                              T_verify,
                               int                              max_n_kv,
                               bool                             use_flash) {
    VerifyBuild vb{};
    vb.T_verify = T_verify;
    vb.max_n_kv = max_n_kv;
    if (ctx == nullptr || T_verify <= 0 || max_n_kv <= 0 || kv_cache.self_k == nullptr || max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime verify: invalid arg (T_verify=%d max_n_kv=%d)", T_verify,
                max_n_kv);
        return vb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const auto    block_params = to_block_params(hp);

    vb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_verify);
    ggml_set_name(vb.input_ids_in, "verify.input_ids");
    ggml_set_input(vb.input_ids_in);
    vb.audio_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T_verify);
    ggml_set_name(vb.audio_in, "verify.audio");
    ggml_set_input(vb.audio_in);
    vb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_verify);
    ggml_set_name(vb.positions_in, "verify.positions");
    ggml_set_input(vb.positions_in);
    vb.kv_idx_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I64, T_verify);
    ggml_set_name(vb.kv_idx_in, "verify.kv_idx");
    ggml_set_input(vb.kv_idx_in);
    vb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, max_n_kv, T_verify);
    ggml_set_name(vb.mask_in, "verify.mask");
    ggml_set_input(vb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime verify: graph alloc failed");
        return vb;
    }
    vb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w, vb.input_ids_in);
    x               = ggml_add(ctx, x, vb.audio_in);

    for (int il = 0; il < n_layer; ++il) {
        ggml_tensor * scale = ada_scale_for(ctx, ada_scale_all, hidden, il);
        x = causal_lm::block_step_n(ctx, gf, x, to_block_view(weights.dec_blocks[il], scale), block_params, kv_cache,
                                    il, T_verify, max_n_kv, vb.mask_in, vb.positions_in, vb.kv_idx_in, use_flash);
    }

    x                    = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, x);
    ggml_set_name(logits, "verify.logits");
    ggml_set_output(logits);
    vb.logits = logits;

    vb.out = ggml_argmax(ctx, logits);
    ggml_set_name(vb.out, "verify.argmax");
    ggml_set_output(vb.out);

    ggml_build_forward_expand(gf, vb.out);
    ggml_build_forward_expand(gf, logits);
    return vb;
}

// ---------------------------------------------------------------------------
// Batched prefill / step (offline transcribe_run_batch)
// ---------------------------------------------------------------------------

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const Weights &                  weights,
                                                const HParams &                  hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                ggml_tensor *                    ada_scale_all,
                                                int                              T_prompt,
                                                int                              n_batch,
                                                bool                             use_flash) {
    PrefillBuildBatched pb{};
    pb.T_prompt = T_prompt;
    pb.n_batch  = n_batch;
    if (ctx == nullptr || T_prompt <= 0 || n_batch <= 0 || !use_flash || kv_cache.self_k == nullptr ||
        kv_cache.n_batch != n_batch || T_prompt > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime prefill(batched): invalid arg (T_prompt=%d, n_batch=%d)",
                T_prompt, n_batch);
        return pb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const int     B            = n_batch;
    const auto    block_params = to_block_params(hp);

    pb.input_ids_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, T_prompt, B);
    named(pb.input_ids_in, "dec.batched.input_ids");
    ggml_set_input(pb.input_ids_in);
    pb.audio_dense_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, static_cast<int64_t>(T_prompt) * B);
    named(pb.audio_dense_in, "dec.batched.audio_dense");
    ggml_set_input(pb.audio_dense_in);
    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt);
    named(pb.positions_in, "dec.batched.positions");
    ggml_set_input(pb.positions_in);
    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_prompt, T_prompt);
    named(pb.mask_in, "dec.batched.mask");
    ggml_set_input(pb.mask_in);
    pb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, T_prompt, B);
    named(pb.kv_idx_in, "dec.batched.kv_idx");
    ggml_set_input(pb.kv_idx_in);
    pb.last_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, 1, B);
    named(pb.last_idx_in, "dec.batched.last_idx");
    ggml_set_input(pb.last_idx_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime prefill(batched): graph alloc failed");
        return pb;
    }
    pb.graph = gf;

    // Token-embed the (rectangular) prompt rows, then ADD the audio embeds at
    // every position. Pure additive fusion, no keep_mask: the realtime prompt is
    // all placeholder tokens and the projector audio overlays every position.
    ggml_tensor * ids_flat = ggml_reshape_1d(ctx, pb.input_ids_in, static_cast<int64_t>(T_prompt) * B);
    ggml_tensor * x        = ggml_get_rows(ctx, weights.dec_embed.token_w, ids_flat);  // [hidden, T_prompt*B]
    x                      = ggml_add(ctx, x, pb.audio_dense_in);
    x                      = ggml_reshape_3d(ctx, x, hidden, T_prompt, B);

    for (int il = 0; il < n_layer; ++il) {
        ggml_tensor * scale = ada_scale_for(ctx, ada_scale_all, hidden, il);
        x = causal_lm::block_prefill_batched(ctx, gf, x, to_block_view(weights.dec_blocks[il], scale), block_params,
                                             kv_cache, il, T_prompt, B, pb.mask_in, pb.positions_in, pb.kv_idx_in,
                                             use_flash);
        if (x == nullptr) {
            pb.graph = nullptr;
            return pb;
        }
    }

    // Gather each row's last real position (= T_prompt-1), final RMSNorm, TIED head.
    ggml_tensor * x_last = ggml_get_rows(ctx, x, pb.last_idx_in);
    x_last               = ggml_reshape_2d(ctx, x_last, hidden, B);
    x_last               = ggml_mul(ctx, ggml_rms_norm(ctx, x_last, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, x_last);  // TIED
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    pb.logits            = logits;
    pb.out               = ggml_argmax(ctx, logits);
    named(pb.out, "dec.batched.argmax");
    ggml_set_output(pb.out);
    ggml_build_forward_expand(gf, pb.out);
    ggml_build_forward_expand(gf, logits);
    return pb;
}

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const Weights &                  weights,
                                          const HParams &                  hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          ggml_tensor *                    ada_scale_all,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || n_batch <= 0 || !use_flash || kv_cache.self_k == nullptr ||
        kv_cache.n_batch != n_batch || max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime step(batched): invalid arg (max_n_kv=%d, n_batch=%d)",
                max_n_kv, n_batch);
        return sb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const int     B            = n_batch;
    const auto    block_params = to_block_params(hp);

    sb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    named(sb.input_ids_in, "step.batched.input_ids");
    ggml_set_input(sb.input_ids_in);
    sb.audio_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, B);
    named(sb.audio_in, "step.batched.audio_in");
    ggml_set_input(sb.audio_in);
    sb.position_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    named(sb.position_in, "step.batched.position");
    ggml_set_input(sb.position_in);
    sb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, 1, B);
    named(sb.kv_idx_in, "step.batched.kv_idx");
    ggml_set_input(sb.kv_idx_in);
    sb.mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F16, max_n_kv, 1, 1, B);
    named(sb.mask_in, "step.batched.mask");
    ggml_set_input(sb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral_realtime step(batched): graph alloc failed");
        return sb;
    }
    sb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w, sb.input_ids_in);  // [hidden, B]
    x               = ggml_add(ctx, x, sb.audio_in);  // per-step additive audio overlay (1 frame/row)

    for (int il = 0; il < n_layer; ++il) {
        ggml_tensor * scale = ada_scale_for(ctx, ada_scale_all, hidden, il);
        x = causal_lm::block_step_batched(ctx, gf, x, to_block_view(weights.dec_blocks[il], scale), block_params,
                                          kv_cache, il, max_n_kv, B, sb.mask_in, sb.position_in, sb.kv_idx_in,
                                          use_flash);
        if (x == nullptr) {
            sb.graph = nullptr;
            return sb;
        }
    }

    x                    = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.token_w, x);  // TIED
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    sb.logits            = logits;
    sb.out               = ggml_argmax(ctx, logits);
    named(sb.out, "step.batched.argmax");
    ggml_set_output(sb.out);
    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);
    return sb;
}

}  // namespace transcribe::voxtral_realtime
