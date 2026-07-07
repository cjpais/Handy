// transcribe-kaldi-fbank.h - kaldi-style HTK mel fbank + LFR stack
// + optional per-feature CMVN, host-side, single-utterance.
//
// Pure host code (no ggml). Mirrors `torchaudio.compliance.kaldi.fbank`
// with the parameters FunASR's WavFrontend passes (window=hamming,
// dither=0, energy_floor=0, snip_edges=True, num_mel_bins=80,
// frame_length=25ms, frame_shift=10ms), plus FunASR's LFR stack and the
// optional per-feature CMVN. Used by `arch/sensevoice` (apply_cmvn=true)
// and `arch/funasr_nano` (apply_cmvn=false).
//
// Key behaviors:
//   * Hamming window (kaldi's symmetric "hamming": 0.54 - 0.46·cos(2πn/(N-1))).
//   * Per-frame preemphasis coefficient 0.97 with kaldi's special
//     boundary `x[0] = x[0] * (1 - 0.97)`.
//   * Per-frame DC offset removal before preemph.
//   * Sample upscale ×32768 before framing when `upscale_samples=true`
//     (WavFrontend treats audio as int16 magnitude in float storage).
//   * snip_edges=True: drop trailing partial frames.
//   * HTK mel filterbank (triangular bins peak=1, no area
//     normalization), low_freq=20 Hz, high_freq=Nyquist.
//   * round_to_power_of_two=True: per-frame FFT size = next_pow2(win_length).
//   * Low-frame-rate stack (m, n): output frame i is the concatenation
//     of input frames [n·i .. n·i + m), with kaldi-style centered left
//     padding ((m-1)/2 copies of frame 0) and right-edge clamping.
//   * Per-feature CMVN: (x + shift) * scale, applied after LFR. Shift
//     and scale are length d_input.
//
// The frontend is built once at load time (window + mel filterbank
// cached) and run once per transcribe_run. Construction is `O(n_mels ·
// padded_n_fft)`; compute is `O(T · n_mels · padded_n_fft)`.

#pragma once

#include <cstddef>
#include <vector>

namespace transcribe {

struct KaldiFbankParams {
    int                n_mels          = 0;      // 80
    int                sample_rate     = 0;      // 16000
    int                win_length      = 0;      // 400
    int                hop_length      = 0;      // 160
    int                lfr_m           = 0;      // 7
    int                lfr_n           = 0;      // 6
    int                d_input         = 0;      // n_mels * lfr_m, e.g. 560
    bool               upscale_samples = true;   // ×32768 PCM scale before framing
    bool               apply_cmvn      = false;  // gate the trailing CMVN step
    // Required when apply_cmvn=true; ignored otherwise. Length d_input.
    std::vector<float> cmvn_shift;
    std::vector<float> cmvn_scale;
};

class KaldiFbankFrontend {
  public:
    explicit KaldiFbankFrontend(KaldiFbankParams params);

    // Compute LFR (+ optional CMVN) features from raw [-1, 1] f32 PCM.
    // Output is row-major [T_lfr, d_input] stored in `out_features`.
    // Returns the number of LFR frames; 0 if the input is too short to
    // produce any LFR output (which means the model cannot run on it).
    int compute(const float * pcm, size_t n_samples, std::vector<float> & out_features) const;

    int d_input() const { return params_.d_input; }

    int n_mels() const { return params_.n_mels; }

    int lfr_m() const { return params_.lfr_m; }

    int lfr_n() const { return params_.lfr_n; }

  private:
    KaldiFbankParams   params_;
    int                padded_n_fft_ = 0;  // next_pow2(win_length)
    std::vector<float> mel_fb_;            // [n_mels, padded_n_fft/2 + 1]
    std::vector<float> window_;            // [win_length]
};

}  // namespace transcribe
