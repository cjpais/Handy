// arch/voxtral/decoder.cpp - Voxtral LM prefill/step graph builders.
//
// Reference: VoxtralForConditionalGeneration's inner LlamaForCausalLM. The
// per-block math (pre-LN RMSNorm, GQA, NEOX RoPE, SwiGLU on packed gate_up) is
// the shared causal_lm module with null Q/K-norm slots (Llama has no per-head
// Q/K norm). This file owns graph allocation, audio injection (3-way concat),
// dump naming, and the UNTIED lm_head.

#include "decoder.h"

#include "causal_lm/causal_lm.h"
#include "ggml.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"

#include <cstdio>

namespace transcribe::voxtral {

namespace {

ggml_tensor * named(ggml_tensor * t, const char * name) {
    if (t != nullptr && name != nullptr) {
        ggml_set_name(t, name);
    }
    return t;
}

// Build a BlockView from one decoder-block weight slot. attn_q_norm /
// attn_k_norm are deliberately left null: Voxtral's Llama backbone has
// no per-head Q/K norm, and causal_lm skips the norm when the slot is null.
causal_lm::BlockView to_block_view(const VoxtralDecBlock & b) {
    causal_lm::BlockView v{};
    v.norm_attn_w   = b.norm_attn_w;
    v.norm_ffn_w    = b.norm_ffn_w;
    v.attn_q_w      = b.attn_q_w;
    v.attn_k_w      = b.attn_k_w;
    v.attn_v_w      = b.attn_v_w;
    v.attn_o_w      = b.attn_o_w;
    v.attn_q_norm   = nullptr;  // Llama: no Q-norm
    v.attn_k_norm   = nullptr;  // Llama: no K-norm
    v.ffn_gate_up_w = b.ffn_gate_up_w;
    v.ffn_down_w    = b.ffn_down_w;
    return v;
}

causal_lm::BlockParams to_block_params(const VoxtralHParams & hp) {
    causal_lm::BlockParams p{};
    p.n_heads      = hp.dec_n_heads;
    p.n_kv_heads   = hp.dec_n_kv_heads;
    p.head_dim     = hp.dec_head_dim;
    p.max_position = hp.dec_max_position_embeddings;
    p.rms_eps      = hp.dec_rms_norm_eps;
    p.rope_theta   = hp.dec_rope_theta;
    return p;
}

}  // namespace

PrefillBuild build_prefill_graph(ggml_context *                   ctx,
                                 const VoxtralWeights &           weights,
                                 const VoxtralHParams &           hp,
                                 transcribe::causal_lm::KvCache & kv_cache,
                                 int                              T_prompt,
                                 int                              T_enc,
                                 int                              prefix_len,
                                 int                              suffix_len,
                                 bool                             use_flash,
                                 bool                             slice_last) {
    PrefillBuild pb{};
    pb.T_prompt   = T_prompt;
    pb.T_enc      = T_enc;
    pb.prefix_len = prefix_len;
    pb.suffix_len = suffix_len;

    if (ctx == nullptr || T_prompt <= 0 || T_enc <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral decoder: invalid arg (T_prompt=%d, T_enc=%d)", T_prompt, T_enc);
        return pb;
    }
    if (prefix_len < 0 || suffix_len < 0 || prefix_len + T_enc + suffix_len != T_prompt) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral decoder: prefix(%d)+T_enc(%d)+suffix(%d) != T_prompt(%d)",
                prefix_len, T_enc, suffix_len, T_prompt);
        return pb;
    }
    if (kv_cache.self_k == nullptr || kv_cache.self_v == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral decoder: kv_cache not initialized");
        return pb;
    }
    if (T_prompt > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral decoder: T_prompt=%d exceeds kv_cache.n_ctx=%d", T_prompt,
                kv_cache.n_ctx);
        return pb;
    }

    const int64_t hidden  = hp.dec_hidden;
    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;

    const auto  block_params = to_block_params(hp);
    const float rms_eps      = hp.dec_rms_norm_eps;

    // ---------- Graph inputs ----------
    pb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt);
    named(pb.input_ids_in, "dec.input_ids");
    ggml_set_input(pb.input_ids_in);

    pb.enc_out_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, T_enc);
    named(pb.enc_out_in, "dec.enc_out");
    ggml_set_input(pb.enc_out_in);

    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt);
    named(pb.positions_in, "dec.positions");
    ggml_set_input(pb.positions_in);

    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_prompt, T_prompt);
    named(pb.mask_in, "dec.attn_mask");
    ggml_set_input(pb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral decoder: ggml_new_graph_custom failed");
        return pb;
    }
    pb.graph = gf;

    // ---------- Token embedding (all positions, for the dump) ----------
    ggml_tensor * token_emb_all = ggml_get_rows(ctx, weights.dec_embed.token_w, pb.input_ids_in);
    named(token_emb_all, "dec.token_emb");
    pb.dumps.token_emb = token_emb_all;
    transcribe::debug::mark_tensor_for_dump(token_emb_all);

    // ---------- Audio injection via 3-way concat ----------
    const size_t  emb_elem = ggml_element_size(token_emb_all);
    ggml_tensor * x_prefix = nullptr;
    if (prefix_len > 0) {
        x_prefix = ggml_view_2d(ctx, token_emb_all, hidden, prefix_len, emb_elem * hidden, /*off=*/0);
        x_prefix = ggml_cont(ctx, x_prefix);
    }
    ggml_tensor * x_audio  = pb.enc_out_in;
    ggml_tensor * x_suffix = nullptr;
    if (suffix_len > 0) {
        x_suffix = ggml_view_2d(ctx, token_emb_all, hidden, suffix_len, emb_elem * hidden,
                                emb_elem * hidden * static_cast<size_t>(prefix_len + T_enc));
        x_suffix = ggml_cont(ctx, x_suffix);
    }

    ggml_tensor * x = x_audio;
    if (x_prefix != nullptr) {
        x = ggml_concat(ctx, x_prefix, x, /*dim=*/1);
    }
    if (x_suffix != nullptr) {
        x = ggml_concat(ctx, x, x_suffix, /*dim=*/1);
    }
    named(x, "dec.audio_injected");
    pb.dumps.audio_injected = x;
    transcribe::debug::mark_tensor_for_dump(x);

    // ---------- Block stack ----------
    for (int il = 0; il < n_layer; ++il) {
        causal_lm::BlockOpts opts{};
        opts.use_flash             = use_flash;
        opts.slice_last_before_ffn = slice_last && (il == n_layer - 1);

        x = causal_lm::block_prefill(ctx, gf, x, to_block_view(weights.dec_blocks[il]), block_params, kv_cache, il,
                                     T_prompt, pb.mask_in, pb.positions_in, opts);

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

    // ---------- Final RMSNorm + UNTIED lm_head ----------
    x = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    named(x, "dec.out_before_head");
    pb.dumps.out_before_head = x;
    transcribe::debug::mark_tensor_for_dump(x);

    ggml_tensor * last_x;
    if (slice_last) {
        last_x = x;
    } else {
        last_x = ggml_view_2d(ctx, x, hidden, 1, ggml_element_size(x) * hidden,
                              ggml_element_size(x) * hidden * static_cast<size_t>(T_prompt - 1));
        last_x = ggml_cont(ctx, last_x);
    }

    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.output_w, last_x);
    logits               = ggml_reshape_1d(ctx, logits, vocab);
    named(logits, "dec.logits_raw");
    pb.dumps.logits_raw = logits;
    transcribe::debug::mark_tensor_for_dump(logits);

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
                           const VoxtralWeights &           weights,
                           const VoxtralHParams &           hp,
                           transcribe::causal_lm::KvCache & kv_cache,
                           int                              max_n_kv,
                           bool                             use_flash) {
    StepBuild sb{};
    sb.max_n_kv = max_n_kv;

    if (ctx == nullptr || max_n_kv <= 0) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step: invalid arg (max_n_kv=%d)", max_n_kv);
        return sb;
    }
    if (kv_cache.self_k == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step: kv_cache not initialized");
        return sb;
    }
    if (max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step: max_n_kv=%d exceeds kv_cache.n_ctx=%d", max_n_kv,
                kv_cache.n_ctx);
        return sb;
    }

    const int64_t vocab   = hp.dec_vocab_size;
    const int     n_layer = hp.dec_n_layers;
    const float   rms_eps = hp.dec_rms_norm_eps;

    const auto block_params = to_block_params(hp);

    sb.input_id_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, 1);
    ggml_set_name(sb.input_id_in, "step.input_id");
    ggml_set_input(sb.input_id_in);

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
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step: ggml_new_graph_custom failed");
        return sb;
    }
    sb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w, sb.input_id_in);

    for (int il = 0; il < n_layer; ++il) {
        x = causal_lm::block_step(ctx, gf, x, to_block_view(weights.dec_blocks[il]), block_params, kv_cache, il,
                                  max_n_kv, sb.mask_in, sb.position_in, sb.kv_idx_in, use_flash);
    }

    x                    = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.output_w, x);
    logits               = ggml_reshape_1d(ctx, logits, vocab);
    ggml_set_name(logits, "step.logits");
    // Kept as a graph output so the mid-generation dec.logits_raw.gen<N> dump
    // can read it back after a step compute.
    ggml_set_output(logits);
    sb.logits = logits;

    ggml_tensor * amax = ggml_argmax(ctx, logits);
    ggml_set_name(amax, "step.argmax");

    sb.out = amax;
    ggml_set_output(sb.out);

    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);

    return sb;
}

// ---------------------------------------------------------------------------
// Batched prefill / step (offline transcribe_run_batch). Mirrors
// arch/canary_qwen, swapping in Voxtral's UNTIED lm_head (dec.output.weight)
// and audio injection at the audio_token_id positions.
// ---------------------------------------------------------------------------

PrefillBuildBatched build_prefill_graph_batched(ggml_context *                   ctx,
                                                const VoxtralWeights &           weights,
                                                const VoxtralHParams &           hp,
                                                transcribe::causal_lm::KvCache & kv_cache,
                                                int                              T_prompt_max,
                                                int                              T_audio_max,
                                                int                              n_batch,
                                                bool                             use_flash) {
    PrefillBuildBatched pb{};
    pb.T_prompt_max = T_prompt_max;
    pb.T_audio_max  = T_audio_max;
    pb.n_batch      = n_batch;

    if (ctx == nullptr || T_prompt_max <= 0 || T_audio_max <= 0 || n_batch <= 0 || !use_flash ||
        kv_cache.self_k == nullptr || kv_cache.n_batch != n_batch || T_prompt_max > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral prefill(batched): invalid arg");
        return pb;
    }

    const int64_t hidden       = hp.dec_hidden;
    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const int     B            = n_batch;
    const auto    block_params = to_block_params(hp);

    (void) T_audio_max;  // audio_dense is prompt-sized; T_audio_max no longer used
    pb.input_ids_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, T_prompt_max, B);
    ggml_set_input(pb.input_ids_in);
    // Audio injection is an elementwise blend (see header): x*keep_mask +
    // audio_dense, which crosses the CPU/CUDA split cleanly unlike a set_rows.
    pb.audio_dense_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, hidden, static_cast<int64_t>(T_prompt_max) * B);
    ggml_set_input(pb.audio_dense_in);
    pb.keep_mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, 1, static_cast<int64_t>(T_prompt_max) * B);
    ggml_set_input(pb.keep_mask_in);
    pb.positions_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, T_prompt_max);
    ggml_set_input(pb.positions_in);
    pb.mask_in = ggml_new_tensor_2d(ctx, GGML_TYPE_F16, T_prompt_max, T_prompt_max);
    ggml_set_input(pb.mask_in);
    pb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, T_prompt_max, B);
    ggml_set_input(pb.kv_idx_in);
    pb.last_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I32, 1, B);
    ggml_set_input(pb.last_idx_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/16384, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral prefill(batched): ggml_new_graph_custom failed");
        return pb;
    }
    pb.graph = gf;

    // Token-embed all (right-padded) prompt rows, then blend the audio embeds in
    // elementwise: x = x*keep_mask + audio_dense (keep_mask broadcasts over
    // hidden). Reshape to 3D for the batched block stack.
    ggml_tensor * ids_flat = ggml_reshape_1d(ctx, pb.input_ids_in, static_cast<int64_t>(T_prompt_max) * B);
    ggml_tensor * x        = ggml_get_rows(ctx, weights.dec_embed.token_w, ids_flat);
    x                      = ggml_add(ctx, ggml_mul(ctx, x, pb.keep_mask_in), pb.audio_dense_in);
    x                      = ggml_reshape_3d(ctx, x, hidden, T_prompt_max, B);

    for (int il = 0; il < n_layer; ++il) {
        x = causal_lm::block_prefill_batched(ctx, gf, x, to_block_view(weights.dec_blocks[il]), block_params, kv_cache,
                                             il, T_prompt_max, B, pb.mask_in, pb.positions_in, pb.kv_idx_in, use_flash);
        if (x == nullptr) {
            pb.graph = nullptr;
            return pb;
        }
    }

    // Gather each utterance's real last position, final RMSNorm, UNTIED head.
    ggml_tensor * x_last = ggml_get_rows(ctx, x, pb.last_idx_in);
    x_last               = ggml_reshape_2d(ctx, x_last, hidden, B);
    x_last               = ggml_mul(ctx, ggml_rms_norm(ctx, x_last, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.output_w, x_last);
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    pb.logits            = logits;
    pb.out               = ggml_argmax(ctx, logits);
    ggml_set_output(pb.out);
    ggml_build_forward_expand(gf, pb.out);
    ggml_build_forward_expand(gf, logits);
    return pb;
}

StepBuildBatched build_step_graph_batched(ggml_context *                   ctx,
                                          const VoxtralWeights &           weights,
                                          const VoxtralHParams &           hp,
                                          transcribe::causal_lm::KvCache & kv_cache,
                                          int                              max_n_kv,
                                          int                              n_batch,
                                          bool                             use_flash) {
    StepBuildBatched sb{};
    sb.max_n_kv = max_n_kv;
    sb.n_batch  = n_batch;
    if (ctx == nullptr || max_n_kv <= 0 || n_batch <= 0 || !use_flash || kv_cache.self_k == nullptr ||
        kv_cache.n_batch != n_batch || max_n_kv > kv_cache.n_ctx) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step(batched): invalid arg");
        return sb;
    }

    const int64_t vocab        = hp.dec_vocab_size;
    const int     n_layer      = hp.dec_n_layers;
    const float   rms_eps      = hp.dec_rms_norm_eps;
    const int     B            = n_batch;
    const auto    block_params = to_block_params(hp);

    sb.input_ids_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_input(sb.input_ids_in);
    sb.position_in = ggml_new_tensor_1d(ctx, GGML_TYPE_I32, B);
    ggml_set_input(sb.position_in);
    sb.kv_idx_in = ggml_new_tensor_2d(ctx, GGML_TYPE_I64, 1, B);
    ggml_set_input(sb.kv_idx_in);
    sb.mask_in = ggml_new_tensor_4d(ctx, GGML_TYPE_F16, max_n_kv, 1, 1, B);
    ggml_set_input(sb.mask_in);

    ggml_cgraph * gf = ggml_new_graph_custom(ctx, /*size=*/8192, /*grads=*/false);
    if (gf == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "voxtral step(batched): ggml_new_graph_custom failed");
        return sb;
    }
    sb.graph = gf;

    ggml_tensor * x = ggml_get_rows(ctx, weights.dec_embed.token_w,
                                    sb.input_ids_in);  // [hidden, B]
    for (int il = 0; il < n_layer; ++il) {
        x = causal_lm::block_step_batched(ctx, gf, x, to_block_view(weights.dec_blocks[il]), block_params, kv_cache, il,
                                          max_n_kv, B, sb.mask_in, sb.position_in, sb.kv_idx_in, use_flash);
        if (x == nullptr) {
            sb.graph = nullptr;
            return sb;
        }
    }

    x                    = ggml_mul(ctx, ggml_rms_norm(ctx, x, rms_eps), weights.dec_final.norm_w);
    ggml_tensor * logits = ggml_mul_mat(ctx, weights.dec_embed.output_w, x);
    logits               = ggml_reshape_2d(ctx, logits, vocab, B);
    sb.logits            = logits;
    sb.out               = ggml_argmax(ctx, logits);
    ggml_set_output(sb.out);
    ggml_build_forward_expand(gf, sb.out);
    ggml_build_forward_expand(gf, logits);
    return sb;
}

}  // namespace transcribe::voxtral
