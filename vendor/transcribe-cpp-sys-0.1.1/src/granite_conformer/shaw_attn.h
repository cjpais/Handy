// src/granite_conformer/shaw_attn.h - shared Shaw block-local attention.
//
// Used by src/arch/granite/encoder.cpp (AR audio-LLM) and
// src/arch/granite_nar/encoder.cpp (NAR editor); the only Granite-specific
// Conformer op without an analog in src/conformer/.
//
// Mirrors GraniteSpeechConformerAttention.forward:
//
//   h = pre_norm(x)                                # at T_enc
//   if remainder > 0: h = F.pad(h, (0, 0, 0, pad)) # zero-pad to T_pad
//   q = to_q(h); kv = to_kv(h); k, v = kv.chunk(2, dim=-1)
//   ... block-local attention with Shaw bias + last-block pad mask ...
//   out = to_out(out[:, :num_features, :])         # slice back to T_enc
//
// The slice happens BEFORE to_out (upstream order), so the following conv
// module's depthwise kernel never pulls pad-row garbage into valid frames.

#pragma once

#include "ggml.h"

namespace transcribe::granite_conformer {

// Per-block Shaw attention weights. Field names match GraniteEncBlock and
// GraniteNarEncBlock, packed so the helper has no compile-time dependency on
// either family's weights struct.
struct ShawAttnWeights {
    ggml_tensor * norm_attn_w      = nullptr;  // [hidden]
    ggml_tensor * norm_attn_b      = nullptr;  // [hidden]      (nullable)
    ggml_tensor * attn_q_w         = nullptr;  // [hidden, inner_dim]
    ggml_tensor * attn_kv_w        = nullptr;  // [hidden, 2*inner_dim]  (fused K|V)
    ggml_tensor * attn_rel_pos_emb = nullptr;  // [head_dim, 2*max_pos_emb+1]
    ggml_tensor * attn_out_w       = nullptr;  // [inner_dim, hidden]
    ggml_tensor * attn_out_b       = nullptr;  // [hidden]      (nullable)
};

// Build the Shaw block-local attention sub-graph.
//
// Batch-capable: the input carries an optional utterance batch B on ne[2]
// (B==1 for single-shot). Block-local attention treats each time-block
// independently, so the B utterances fold into the block axis internally
// (num_blocks_eff = num_blocks * B); the caller tiles the per-block pad
// mask across B and sizes zero_pad with the batch.
//
// Inputs:
//   x            : [d_model, T_enc, B] pre-norm input (B==1 single-shot).
//   zero_pad     : [d_model, T_pad - T_enc, B] f32 zeros, or nullptr when
//                  T_enc % context_size == 0.
//   dists        : [context_size * context_size] int32 Shaw lookup
//                  indices (`clamp(c - r + max_pos_emb, 0, 2*max_pos_emb)`,
//                  c = key/column, r = query/row). Shared across B.
//   pad_mask_3d  : [context_size, context_size, num_blocks * B] f32 additive
//                  mask (all-zero except each utterance's last block slice,
//                  which has -INF on pad-K columns). The caller tiles the
//                  per-block pattern across B.
//   w            : per-block weights (see ShawAttnWeights).
//   n_heads, head_dim, context_size, num_blocks, T_enc : shape scalars
//                  (num_blocks is per-utterance; B is read from x->ne[2]).
//   layer_norm_eps : pre-norm eps (both Granite families use 1e-5).
//
// Returns: [d_model, T_enc, B] (matches input), or nullptr on shape error.
ggml_tensor * shaw_block_attn(ggml_context *          ctx,
                              ggml_tensor *           x,
                              ggml_tensor *           zero_pad,
                              ggml_tensor *           dists,
                              ggml_tensor *           pad_mask_3d,
                              const ShawAttnWeights & w,
                              int                     n_heads,
                              int                     head_dim,
                              int                     context_size,
                              int                     num_blocks,
                              int                     T_enc,
                              float                   layer_norm_eps);

}  // namespace transcribe::granite_conformer
