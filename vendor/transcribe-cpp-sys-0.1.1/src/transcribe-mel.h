// transcribe-mel.h - native C++ log-mel feature extractor.
//
// PRIVATE header (src/, not exported). Pure CPU, no ggml dependency:
// audio in, std::vector<float> mel out; the family encoder builder
// copies the result into a ggml_tensor at the backend boundary.
//
// Matches NeMo's AudioToMelSpectrogramPreprocessor / FilterbankFeatures
// as exported in nemo128.onnx. The per-NeMo numerical constants
// (log_eps = 2^-24, unbiased variance, reflect padding, fp64 STFT,
// mag_power = 2.0) are hardcoded in the .cpp rather than in the GGUF
// schema; they become per-family knobs only when a second family needs
// different values.
//
// Construction is one-shot: the constructor precomputes the hann window
// (zero-padded from win_length to n_fft) and the Slaney-normalized mel
// filterbank. compute() is then a function of (config, audio) only.

#pragma once

#include "transcribe.h"

#include <cstddef>
#include <string>
#include <vector>

namespace transcribe {

// Frontend configuration. Mirrors the stt.frontend.* KV the loader
// reads out of ParakeetHParams::fe_*. Family-agnostic: SenseVoice
// will fill the same struct with different values when it lands.
//
// The defaults are the real Parakeet 0.6B values (matching
// nemo128.onnx). Constructing a default-initialized MelConfig and
// calling MelFrontend(cfg) gives a working extractor for v2 / v3.
struct MelConfig {
    int   sample_rate  = 16000;
    int   num_mels     = 128;
    int   n_fft        = 512;
    int   win_length   = 400;
    int   hop_length   = 160;
    float pre_emphasis = 0.97f;  // 0.0 disables
    float f_min        = 0.0f;
    float f_max        = 8000.0f;

    // STFT input padding mode:
    //   "reflect"  — symmetric reflect-pad by n_fft/2 on both sides
    //                (NeMo default). Frame count: floor(n / hop) + 1.
    //   "constant" — zero-pad by n_fft/2 on both sides.
    //   "none"     — PyTorch center=False: no input padding, window is
    //                left-aligned in the n_fft buffer instead of centered,
    //                frame count = (n_samples - win_length) / hop + 1.
    //                Used by LASR / MedASR.
    std::string pad_mode = "reflect";

    // STFT window shape:
    //   "hann_symmetric" — torch.hann_window(N, periodic=False):
    //                      cos(2*pi*k / (N-1)). Default; used by NeMo
    //                      and Cohere.
    //   "hann_periodic"  — torch.hann_window(N, periodic=True):
    //                      cos(2*pi*k / N). Used by Whisper (and
    //                      Qwen3-ASR's Whisper frontend).
    std::string window_type = "hann_symmetric";

    // Normalization mode:
    //   "per_feature"   — NeMo: per-mel-bin zero-mean / unit-variance
    //                     (unbiased); default.
    //   "per_utterance" — Whisper: log10 base, global clamp to
    //                     max - 8.0, then (x + 4) / 4. Also drops the
    //                     trailing center-pad STFT frame so the output
    //                     has exactly `n_samples / hop_length` frames.
    //   "none"          — emit raw log-mel as-is (NeMo's "NA"/no-op
    //                     normalize; used by streaming-trained variants
    //                     whose feature normalisation is baked into
    //                     training rather than applied at inference).
    //   "global"        — Voxtral Realtime streaming log-mel: like
    //                      "per_utterance" but the log clamp floor uses a
    //                      FIXED max (global_log_mel_max) instead of the
    //                      per-utterance maximum, so each frame is
    //                      causal/streaming-safe. Still drops the trailing
    //                      center-pad STFT frame.
    std::string normalize = "per_feature";

    // Fixed log-mel maximum for normalize == "global" (Voxtral Realtime
    // global_log_mel_max). Unused by other normalize modes.
    float global_log_mel_max = 1.5f;

    // Optional checkpoint-provided mel filterbank [num_mels * (n_fft/2+1)]
    // row-major, Slaney-normalised. When non-empty, used instead of
    // computing from scratch. Set by the loader when the GGUF contains
    // frontend.mel_filterbank.
    std::vector<float> filterbank;

    // When > 0 AND normalize="none", emit log(max(power, log_clamp_min))
    // instead of NeMo's log(power + kLogEps). LASR / MedASR use this with
    // log_clamp_min = 1e-5; NeMo's frontends leave it at 0.0.
    float log_clamp_min = 0.0f;

    // Optional checkpoint-provided window [win_length]. When non-empty,
    // used instead of computing a periodic Hann window.
    std::vector<float> window;
};

// Pure C++ log-mel extractor. Construct once, call compute() any
// number of times. Thread-safety: const after construction; multiple
// threads may call compute() concurrently.
class MelFrontend {
  public:
    explicit MelFrontend(const MelConfig & cfg);

    // Run the full pipeline. pcm must be 16 kHz mono float32 in
    // [-1, 1]; n_samples is the number of input samples. The output
    // mel buffer is resized to num_mels * n_frames in row-major
    // [num_mels, n_frames] layout.
    //
    // n_threads controls STFT parallelism. 0 (default) auto-detects
    // via std::thread::hardware_concurrency() capped at 8. The STFT
    // loop is the dominant cost of compute(); the filterbank matmul
    // is handed to cblas_sgemm and is already multi-threaded on
    // Accelerate / OpenBLAS.
    //
    // Returns:
    //   TRANSCRIBE_OK              normal success.
    //   TRANSCRIBE_ERR_INVALID_ARG pcm is null, or n_samples is too
    //                              short to produce >= 2 frames
    //                              (per-feature normalize divides by
    //                              n_frames - 1, which would NaN).
    transcribe_status compute(const float *        pcm,
                              size_t               n_samples,
                              std::vector<float> & out_mel,
                              int &                out_n_mels,
                              int &                out_n_frames,
                              int                  n_threads = 0) const;

    // Number of mel bins (matches MelConfig::num_mels).
    int num_mels() const { return cfg_.num_mels; }

    // Frame count for a given audio length, before calling compute().
    // Matches NeMo: floor(n_samples / hop_length) + 1.
    int n_frames_for(size_t n_samples) const;

    // Read-only accessors for unit tests. Not part of the runtime
    // API; the goal is to validate the precomputed buffers in
    // isolation without running the full pipeline.
    const std::vector<double> & window() const { return window_; }

    const std::vector<float> & filterbank() const { return mel_fb_; }

    const MelConfig & config() const { return cfg_; }

    int n_freq() const { return n_freq_; }

  private:
    MelConfig           cfg_;
    int                 n_freq_;  // n_fft/2 + 1
    std::vector<double> window_;  // [n_fft], periodic hann zero-padded
    std::vector<float>  mel_fb_;  // [num_mels * n_freq] row-major, Slaney

    // sin/cos LUT for the mixed-radix FFT. Sized to n_fft so that every
    // recursion-level N (n_fft, n_fft/2, n_fft/4, ..., odd leaf) divides
    // the LUT exactly and lookups never fall back to live std::cos/sin.
    // Stored as fp32 to match the FFT precision and avoid a per-load
    // cast in the inner butterfly. Only populated when n_fft is non-pow2
    // (the only path that uses the LUT — pow2 sizes use fft_radix2 /
    // vDSP, which carry their own twiddle factors). Empty when unused.
    std::vector<float> cos_lut_;
    std::vector<float> sin_lut_;
};

}  // namespace transcribe
