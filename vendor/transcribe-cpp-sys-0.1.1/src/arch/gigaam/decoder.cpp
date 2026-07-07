// arch/gigaam/decoder.cpp - GigaAM RNN-T / CTC greedy decode on host.
//
// The decoder is tiny (1 LSTM layer of 320, single joint linear) so per-step
// compute fits in CPU cache; running on host avoids backend dispatch overhead
// for the inner symbol-loop. PyTorch nn.LSTM stores gate weights concatenated
// [4*hidden, hidden] in (i, f, g, o) order; the converter emits Wx/Wh in that
// layout and collapses bias_ih + bias_hh into one [4*hidden] bias, which the
// host LSTM step reads directly.

#include "decoder.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "gigaam.h"
#include "transcribe-debug.h"
#include "transcribe-log.h"
#include "weights.h"

#if TRANSCRIBE_HAS_BLAS
#    ifdef __APPLE__
#        include <Accelerate/Accelerate.h>
#    else
#        include <cblas.h>
#    endif
#endif

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

namespace transcribe::gigaam {

namespace {

// Copy a ggml tensor's data into a host f32 vector, dequantizing if needed.
// F32 takes the fast path; F16/BF16/Q*/IQ* stage raw bytes off the backend and
// walk ggml's to_float trait. Runs once at load time, so the per-step hot path
// pays nothing.
void readback_f32(const ggml_tensor * t, std::vector<float> & out) {
    if (t == nullptr) {
        out.clear();
        return;
    }
    const size_t n = static_cast<size_t>(ggml_nelements(t));
    out.assign(n, 0.0f);
    if (n == 0) {
        return;
    }

    if (t->type == GGML_TYPE_F32) {
        ggml_backend_tensor_get(t, out.data(), 0, n * sizeof(float));
        return;
    }

    const auto * tt = ggml_get_type_traits(t->type);
    if (tt == nullptr || tt->to_float == nullptr) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR, "gigaam decoder: tensor \"%s\" type %s has no to_float", t->name,
                ggml_type_name(t->type));
        out.clear();
        return;
    }
    const size_t         nbytes = ggml_nbytes(t);
    std::vector<uint8_t> raw(nbytes);
    ggml_backend_tensor_get(t, raw.data(), 0, nbytes);
    tt->to_float(raw.data(), out.data(), static_cast<int64_t>(n));
}

inline float sigmoidf(float x) {
    return 1.0f / (1.0f + std::exp(-x));
}

// y = W @ x + b, where W is [out, in] row-major (numpy shape), x is
// [in], b is [out]. b may be nullptr to skip the add.
void matvec_add(const float * W, const float * b, const float * x, int out, int in_dim, float * y) {
    for (int i = 0; i < out; ++i) {
        float         acc = (b != nullptr) ? b[i] : 0.0f;
        const float * row = W + static_cast<size_t>(i) * in_dim;
        for (int j = 0; j < in_dim; ++j) {
            acc += row[j] * x[j];
        }
        y[i] = acc;
    }
}

}  // namespace

// Build host buffers for the predictor + joint weights. Reads back from
// the backend exactly once at load time; the decode loop then iterates
// the host floats.
transcribe_status build_host_decoder_weights(const GigaamWeights & w,
                                             const GigaamHParams & hp,
                                             HostDecoderWeights &  out) {
    if (hp.head_kind == HeadKind::CTC) {
        readback_f32(w.ctc_head.weight, out.ctc_w);
        readback_f32(w.ctc_head.bias, out.ctc_b);
        return TRANSCRIBE_OK;
    }
    // RNN-T.
    readback_f32(w.predictor.embed_w, out.pred_embed);
    out.lstm_Wx.resize(hp.pred_n_layers);
    out.lstm_Wh.resize(hp.pred_n_layers);
    out.lstm_b.resize(hp.pred_n_layers);
    for (int i = 0; i < hp.pred_n_layers; ++i) {
        readback_f32(w.predictor.lstm[i].Wx, out.lstm_Wx[i]);
        readback_f32(w.predictor.lstm[i].Wh, out.lstm_Wh[i]);
        readback_f32(w.predictor.lstm[i].b, out.lstm_b[i]);
    }
    readback_f32(w.joint.enc_w, out.joint_enc_w);
    readback_f32(w.joint.enc_b, out.joint_enc_b);
    readback_f32(w.joint.pred_w, out.joint_pred_w);
    readback_f32(w.joint.pred_b, out.joint_pred_b);
    readback_f32(w.joint.out_w, out.joint_out_w);
    readback_f32(w.joint.out_b, out.joint_out_b);
    return TRANSCRIBE_OK;
}

// One LSTM step (single batch). Returns the new hidden vector (= LSTM
// output) into `h_out`, and the new cell state into `c_out`. Inputs:
//   x:      [pred_hidden] embedding (or zero on the first step)
//   h_prev: [pred_hidden]
//   c_prev: [pred_hidden]
//   Wx:     [4*pred_hidden, pred_hidden] flat row-major
//   Wh:     [4*pred_hidden, pred_hidden]
//   b:      [4*pred_hidden] (already collapsed bias_ih + bias_hh)
// Gate order is PyTorch (i, f, g, o), each block size `pred_hidden`.
static void lstm_step(const float * x,
                      const float * h_prev,
                      const float * c_prev,
                      const float * Wx,
                      const float * Wh,
                      const float * b,
                      int           H,
                      float *       h_out,
                      float *       c_out,
                      float *       scratch_gates) {
    // scratch_gates: [4*H]. Compute Wx @ x + Wh @ h_prev + b in place.
    for (int i = 0; i < 4 * H; ++i) {
        float         acc    = b[i];
        const float * wx_row = Wx + static_cast<size_t>(i) * H;
        const float * wh_row = Wh + static_cast<size_t>(i) * H;
        for (int j = 0; j < H; ++j) {
            acc += wx_row[j] * x[j] + wh_row[j] * h_prev[j];
        }
        scratch_gates[i] = acc;
    }

    const float * gi = scratch_gates;
    const float * gf = scratch_gates + H;
    const float * gg = scratch_gates + 2 * H;
    const float * go = scratch_gates + 3 * H;

    for (int j = 0; j < H; ++j) {
        const float i_t   = sigmoidf(gi[j]);
        const float f_t   = sigmoidf(gf[j]);
        const float g_t   = std::tanh(gg[j]);
        const float o_t   = sigmoidf(go[j]);
        const float c_new = f_t * c_prev[j] + i_t * g_t;
        c_out[j]          = c_new;
        h_out[j]          = o_t * std::tanh(c_new);
    }
}

// One joint forward pass for batch=1, using a precomputed encoder
// projection (enc_proj = enc_w @ f_t + enc_b, hoisted out of the time
// loop in `decode_rnnt_greedy`):
//   pred_proj = pred_w @ g_t + pred_b          [joint_hidden]
//   logits    = out_w @ relu(enc_proj + pred_proj) + out_b   [n_classes]
//
// Returns the argmax token id.
static int joint_argmax(const HostDecoderWeights & h,
                        const float *              enc_proj,  // [joint_h]
                        const float *              g_t,       // [pred_hidden]
                        int                        pred_d,
                        int                        joint_h,
                        int                        n_classes,
                        std::vector<float> &       scratch_join,  // size joint_h
                        std::vector<float> &       scratch_logits) {
    scratch_join.assign(joint_h, 0.0f);
    scratch_logits.assign(n_classes, 0.0f);

    // pred_proj into scratch_join, then fuse `+ enc_proj` and ReLU.
    matvec_add(h.joint_pred_w.data(), h.joint_pred_b.data(), g_t, joint_h, pred_d, scratch_join.data());
    for (int i = 0; i < joint_h; ++i) {
        float v         = scratch_join[i] + enc_proj[i];
        scratch_join[i] = v > 0.0f ? v : 0.0f;
    }
    // logits = out_w @ relu + out_b. The reference applies log_softmax
    // but argmax is invariant under it, so we skip.
    matvec_add(h.joint_out_w.data(), h.joint_out_b.data(), scratch_join.data(), n_classes, joint_h,
               scratch_logits.data());

    int   best_idx = 0;
    float best_val = scratch_logits[0];
    for (int i = 1; i < n_classes; ++i) {
        if (scratch_logits[i] > best_val) {
            best_val = scratch_logits[i];
            best_idx = i;
        }
    }
    return best_idx;
}

// Run RNN-T greedy decode against the encoder output. `encoded` is a
// host-side buffer laid out [T_enc * d_model] in T-major order (each
// contiguous d_model-sized chunk is one time step's encoder vector) —
// matches the rnnt.encoded reference dump.
//
// Returns the emitted token sequence (excluding blanks) and the
// per-token encoder frame indices.
transcribe_status decode_rnnt_greedy(const HostDecoderWeights & host,
                                     const GigaamHParams &      hp,
                                     const float *              encoded,
                                     int                        T_enc,
                                     int                        max_symbols_per_step,
                                     std::vector<int> &         out_tokens,
                                     std::vector<int> &         out_frames) {
    out_tokens.clear();
    out_frames.clear();

    const int H        = hp.pred_hidden;
    const int enc_d    = hp.enc_d_model;
    const int joint_h  = hp.joint_hidden;
    const int n_class  = hp.joint_n_classes;
    const int blank_id = n_class - 1;

    // Precompute encoder projection for every frame in one BLAS sgemm
    // (or a fallback loop). The joint's enc_w @ f_t + enc_b is independent
    // of decode state, so hoisting it out of the inner loop eliminates
    // O(iters) redundant matvecs and gives BLAS a single large matrix.
    std::vector<float> enc_proj_all(static_cast<size_t>(T_enc) * static_cast<size_t>(joint_h));
#if TRANSCRIBE_HAS_BLAS
    cblas_sgemm(CblasRowMajor, CblasNoTrans, CblasTrans, T_enc, joint_h, enc_d, 1.0f, encoded, enc_d,
                host.joint_enc_w.data(), enc_d, 0.0f, enc_proj_all.data(), joint_h);
#else
    for (int t = 0; t < T_enc; ++t) {
        const float * frame = encoded + static_cast<size_t>(t) * enc_d;
        float *       proj  = enc_proj_all.data() + static_cast<size_t>(t) * joint_h;
        matvec_add(host.joint_enc_w.data(), nullptr, frame, joint_h, enc_d, proj);
    }
#endif
    for (int t = 0; t < T_enc; ++t) {
        float * proj = enc_proj_all.data() + static_cast<size_t>(t) * joint_h;
        for (int j = 0; j < joint_h; ++j) {
            proj[j] += host.joint_enc_b[j];
        }
    }

    std::vector<float> h_cur(H, 0.0f);
    std::vector<float> c_cur(H, 0.0f);
    std::vector<float> h_next(H, 0.0f);
    std::vector<float> c_next(H, 0.0f);
    std::vector<float> gates(4 * H, 0.0f);
    std::vector<float> pred_in(H, 0.0f);  // embed lookup output
    std::vector<float> jh(joint_h, 0.0f);
    std::vector<float> logits(n_class, 0.0f);

    bool fresh = true;  // first step uses zero input + zero state (reference predict(None, None))

    // Predictor-dirty cache: on a blank emission `(prev_token, h_cur, c_cur)`
    // are unchanged, so the next LSTM step would deterministically produce
    // the same `h_next`. Skip the unroll and reuse `h_next`; recompute on
    // emit (where state and last_token change). Bit-exact with the
    // unconditional path.
    bool predictor_dirty = true;

    for (int t = 0; t < T_enc; ++t) {
        const float * enc_proj = enc_proj_all.data() + static_cast<size_t>(t) * joint_h;

        int sym_count = 0;
        for (;;) {
            if (predictor_dirty) {
                if (fresh) {
                    std::fill(pred_in.begin(), pred_in.end(), 0.0f);
                } else {
                    const int prev = out_tokens.back();
                    std::memcpy(pred_in.data(), host.pred_embed.data() + static_cast<size_t>(prev) * H,
                                H * sizeof(float));
                }
                lstm_step(pred_in.data(), h_cur.data(), c_cur.data(), host.lstm_Wx[0].data(), host.lstm_Wh[0].data(),
                          host.lstm_b[0].data(), H, h_next.data(), c_next.data(), gates.data());
                predictor_dirty = false;
            }

            const int tok = joint_argmax(host, enc_proj, h_next.data(), H, joint_h, n_class, jh, logits);

            if (tok == blank_id) {
                // Advance time. Do NOT commit state. Predictor stays clean.
                break;
            }
            // Emit token; commit state.
            out_tokens.push_back(tok);
            out_frames.push_back(t);
            h_cur           = h_next;
            c_cur           = c_next;
            fresh           = false;
            predictor_dirty = true;
            ++sym_count;
            if (max_symbols_per_step > 0 && sym_count >= max_symbols_per_step) {
                break;
            }
        }
    }

    return TRANSCRIBE_OK;
}

// CTC greedy decode. The 1×1 Conv1d head is a per-frame linear:
//   logits[t, :] = ctc_w @ encoded[t, :] + ctc_b
// Layout of host.ctc_w after readback: ggml ne=[1, d_model, n_classes]
// is byte-equivalent to a row-major [n_classes, d_model] matrix (the
// k=1 axis collapses), which is what matvec_add expects.
transcribe_status decode_ctc_greedy(const HostDecoderWeights & host,
                                    const GigaamHParams &      hp,
                                    const float *              encoded,
                                    int                        T_enc,
                                    std::vector<int> &         out_tokens,
                                    std::vector<int> &         out_frames) {
    out_tokens.clear();
    out_frames.clear();

    const int d_model   = hp.enc_d_model;
    const int n_classes = hp.head_n_classes;
    const int blank_id  = n_classes - 1;

    if (encoded == nullptr || T_enc <= 0 || d_model <= 0 || n_classes <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    if (static_cast<int>(host.ctc_w.size()) != n_classes * d_model ||
        static_cast<int>(host.ctc_b.size()) != n_classes) {
        log_msg(TRANSCRIBE_LOG_LEVEL_ERROR,
                "gigaam decoder (ctc): host head shape mismatch "
                "(ctc_w=%zu expected=%d, ctc_b=%zu expected=%d)",
                host.ctc_w.size(), n_classes * d_model, host.ctc_b.size(), n_classes);
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // logits[T_enc, n_classes] row-major.
    std::vector<float> logits(static_cast<size_t>(T_enc) * static_cast<size_t>(n_classes), 0.0f);
    for (int t = 0; t < T_enc; ++t) {
        const float * frame = encoded + static_cast<size_t>(t) * d_model;
        float *       row   = logits.data() + static_cast<size_t>(t) * n_classes;
        matvec_add(host.ctc_w.data(), host.ctc_b.data(), frame, n_classes, d_model, row);
    }

    // Raw logits before log_softmax, [T_enc, n_classes] row-major.
    if (transcribe::debug::enabled()) {
        const long long shape[2] = { T_enc, n_classes };
        transcribe::debug::dump_host_f32("ctc.logits.raw", logits.data(), static_cast<long long>(logits.size()), shape,
                                         2, "decoder.ctc.logits_raw");
    }

    // Per-frame log_softmax in place.
    for (int t = 0; t < T_enc; ++t) {
        float * row   = logits.data() + static_cast<size_t>(t) * n_classes;
        float   max_v = row[0];
        for (int c = 1; c < n_classes; ++c) {
            if (row[c] > max_v) {
                max_v = row[c];
            }
        }
        double sum = 0.0;
        for (int c = 0; c < n_classes; ++c) {
            sum += std::exp(static_cast<double>(row[c] - max_v));
        }
        const float log_sum = static_cast<float>(std::log(sum)) + max_v;
        for (int c = 0; c < n_classes; ++c) {
            row[c] -= log_sum;
        }
    }

    if (transcribe::debug::enabled()) {
        const long long shape[2] = { T_enc, n_classes };
        transcribe::debug::dump_host_f32("ctc.log_probs", logits.data(), static_cast<long long>(logits.size()), shape,
                                         2, "decoder.ctc.log_probs");
    }

    // Greedy collapse: drop runs of the same label, then drop blanks.
    int prev_label = -1;
    for (int t = 0; t < T_enc; ++t) {
        const float * row    = logits.data() + static_cast<size_t>(t) * n_classes;
        int           best   = 0;
        float         best_v = row[0];
        for (int c = 1; c < n_classes; ++c) {
            if (row[c] > best_v) {
                best_v = row[c];
                best   = c;
            }
        }
        if (best == prev_label) {
            continue;  // run-length collapse
        }
        prev_label = best;
        if (best == blank_id) {
            continue;
        }
        out_tokens.push_back(best);
        out_frames.push_back(t);
    }

    return TRANSCRIBE_OK;
}

}  // namespace transcribe::gigaam
