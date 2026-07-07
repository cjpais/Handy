// arch/gigaam/decoder.h - GigaAM RNN-T / CTC greedy decoders.

#pragma once

#include "transcribe.h"

#include <cstdint>
#include <vector>

namespace transcribe::gigaam {

struct GigaamHParams;
struct GigaamWeights;

struct GigaamWeights;
struct GigaamHParams;

// Host mirror of the predictor + joint (RNN-T) or CTC head (CTC). The
// decode loop runs on host so per-step latency is dominated by L2 cache,
// not backend dispatch overhead. Populated at load() time; const after
// that.
struct HostDecoderWeights {
    // RNN-T fields.
    std::vector<float>              pred_embed;  // [pred_vocab, pred_hidden]
    std::vector<std::vector<float>> lstm_Wx;     // per layer
    std::vector<std::vector<float>> lstm_Wh;
    std::vector<std::vector<float>> lstm_b;
    std::vector<float>              joint_enc_w;
    std::vector<float>              joint_enc_b;
    std::vector<float>              joint_pred_w;
    std::vector<float>              joint_pred_b;
    std::vector<float>              joint_out_w;
    std::vector<float>              joint_out_b;
    // CTC fields.
    std::vector<float>              ctc_w;  // [n_classes, d_model]
    std::vector<float>              ctc_b;
};

// Read the predictor + joint (or CTC head) tensors from backend memory
// into the host mirror. Called once at load() time.
transcribe_status build_host_decoder_weights(const GigaamWeights & w,
                                             const GigaamHParams & hp,
                                             HostDecoderWeights &  out);

// RNN-T greedy decode. `encoded` is a host buffer laid out
// [T_enc * d_model] in T-major order — element (t, d) at offset
// t*d_model + d. Matches the reference's `encoded.transpose(1, 2)`
// flat layout.
transcribe_status decode_rnnt_greedy(const HostDecoderWeights & host,
                                     const GigaamHParams &      hp,
                                     const float *              encoded,
                                     int                        T_enc,
                                     int                        max_symbols_per_step,
                                     std::vector<int> &         out_tokens,
                                     std::vector<int> &         out_frames);

// CTC greedy decode. `encoded` matches the RNN-T layout
// ([T_enc * d_model] T-major). The 1×1 Conv1d head reduces to a
// per-frame linear projection (logits = W @ frame + b), followed by
// log_softmax along the class axis. Dumps `ctc.logits.raw` (pre
// log_softmax) and `ctc.log_probs` (post) when debug dumps are enabled.
// Greedy collapse: drop runs of the same class, then drop blanks.
transcribe_status decode_ctc_greedy(const HostDecoderWeights & host,
                                    const GigaamHParams &      hp,
                                    const float *              encoded,
                                    int                        T_enc,
                                    std::vector<int> &         out_tokens,
                                    std::vector<int> &         out_frames);

}  // namespace transcribe::gigaam
