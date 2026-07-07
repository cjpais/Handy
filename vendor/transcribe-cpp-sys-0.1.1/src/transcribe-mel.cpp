// transcribe-mel.cpp - native C++ log-mel feature extractor.
//
// Matches NeMo's AudioToMelSpectrogramPreprocessor
// (FilterbankFeatures.forward in features.py) as exported in the
// Parakeet 0.6B nemo128.onnx preprocessor. Constants / ordering:
//
//   * Pre-emphasis alpha: MelConfig::pre_emphasis (0.97; 0.0 disables).
//   * Reflect padding by n_fft/2 on each side. The NeMo source says
//     pad_mode="constant" but the ONNX export (and the released
//     checkpoint) use reflect; trust the ONNX.
//   * STFT in fp32 for the mixed-radix path (whisper, n_fft=400),
//     fp64 for the radix-2 vDSP path (parakeet/cohere, n_fft=512).
//     fp32 mixed-radix matches whisper.cpp and is WER-neutral.
//   * Window: symmetric Hann length win_length zero-padded to n_fft
//     (NeMo passes periodic=False, features.py:316).
//   * Power spectrum: re^2 + im^2 (mag_power=2.0 folded into squaring).
//   * Mel filterbank: Slaney-normalized librosa formula, ~3e-7 vs ONNX.
//   * Log epsilon: 2^-24 (NeMo log_zero_guard_value, features.py:256).
//   * Per-feature normalize: UNBIASED variance, divides by (n-1)
//     (NeMo normalize_batch, features.py:79); biased drifts ~1/n.
//   * Normalize epsilon: 1e-5 (NeMo CONSTANT).
//   * Dither: never applied (NeMo gates it on self.training).
//
// Output: [num_mels, n_frames] row-major float32; backend-free.

#include "transcribe-mel.h"

#include "transcribe-batch-util.h"

#ifdef __APPLE__
#    include <Accelerate/Accelerate.h>
#elif TRANSCRIBE_HAS_BLAS
#    include <cblas.h>
#endif

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <limits>
#include <thread>
#include <vector>

#ifndef M_PI
#    define M_PI 3.14159265358979323846
#endif

namespace transcribe {

namespace {

// Hardcoded NeMo numerical constants. See file-level comment.
constexpr float kLogEps  = 5.9604644775390625e-08f;  // 2^-24
constexpr float kNormEps = 1.0e-05f;                 // NeMo CONSTANT

// ---------- Slaney mel scale (matches librosa.filters.mel) ----------

constexpr double kSlaneyFsp      = 200.0 / 3.0;
constexpr double kSlaneyMinLogHz = 1000.0;

// kSlaneyLogStep = log(6.4) / 27.0; not constexpr because std::log
// isn't constexpr in C++17. Computed inline below.

inline double slaney_hz_to_mel(double hz) {
    const double min_log_mel = kSlaneyMinLogHz / kSlaneyFsp;
    const double logstep     = std::log(6.4) / 27.0;
    if (hz < kSlaneyMinLogHz) {
        return hz / kSlaneyFsp;
    }
    return min_log_mel + std::log(hz / kSlaneyMinLogHz) / logstep;
}

inline double slaney_mel_to_hz(double mel) {
    const double min_log_mel = kSlaneyMinLogHz / kSlaneyFsp;
    const double logstep     = std::log(6.4) / 27.0;
    if (mel < min_log_mel) {
        return mel * kSlaneyFsp;
    }
    return kSlaneyMinLogHz * std::exp(logstep * (mel - min_log_mel));
}

// Build the librosa Slaney-normalized mel filterbank.
// Output: [num_mels * n_freq] row-major float32.
// Matches librosa.filters.mel(sr, n_fft, n_mels, fmin, fmax,
//                              norm='slaney') to within ~3e-7 in
// fp32 (verified against the ONNX-baked filterbank in
// nemo128.onnx).
void build_mel_filterbank_slaney(int sr, int n_fft, int n_mels, double fmin, double fmax, std::vector<float> & out) {
    const int n_freq = n_fft / 2 + 1;
    out.assign(static_cast<size_t>(n_mels) * n_freq, 0.0f);

    // FFT bin frequencies in Hz: linspace(0, sr/2, n_freq) = k * sr / n_fft
    // (librosa uses np.linspace which gives identical values for this case).
    std::vector<double> fft_freqs(n_freq);
    for (int k = 0; k < n_freq; ++k) {
        fft_freqs[k] = static_cast<double>(sr) * k / static_cast<double>(n_fft);
    }

    // Mel boundary frequencies: n_mels + 2 points spaced uniformly in mel
    // scale. The first and last bound the entire bank; the middle n_mels
    // are the centers of the triangular filters.
    const double        mel_min = slaney_hz_to_mel(fmin);
    const double        mel_max = slaney_hz_to_mel(fmax);
    std::vector<double> hz_freqs(n_mels + 2);
    for (int m = 0; m < n_mels + 2; ++m) {
        const double mel = mel_min + (mel_max - mel_min) * m / static_cast<double>(n_mels + 1);
        hz_freqs[m]      = slaney_mel_to_hz(mel);
    }

    // Triangular filters with Slaney area normalization.
    // For mel bin m: rises linearly from hz_freqs[m] to hz_freqs[m+1],
    // then falls linearly to hz_freqs[m+2]. The Slaney enorm scales
    // by 2 / (right - left) so each filter has unit area.
    std::vector<double> fdiff(n_mels + 1);
    for (int m = 0; m < n_mels + 1; ++m) {
        fdiff[m] = hz_freqs[m + 1] - hz_freqs[m];
    }

    for (int m = 0; m < n_mels; ++m) {
        const double enorm = 2.0 / (hz_freqs[m + 2] - hz_freqs[m]);
        for (int k = 0; k < n_freq; ++k) {
            const double lower                       = (fft_freqs[k] - hz_freqs[m]) / fdiff[m];
            const double upper                       = (hz_freqs[m + 2] - fft_freqs[k]) / fdiff[m + 1];
            const double w                           = std::max(0.0, std::min(lower, upper));
            out[static_cast<size_t>(m) * n_freq + k] = static_cast<float>(w * enorm);
        }
    }
}

// Build a Hann window of length win_length, zero-padded to n_fft on
// both sides. `periodic` selects between torch.hann_window(N, periodic=
// False) (symmetric, default — cos(2πk/(N-1))) and periodic=True
// (Whisper — cos(2πk/N)).
void build_hann_window_symmetric_padded(int win_length, int n_fft, bool periodic, std::vector<double> & out) {
    out.assign(n_fft, 0.0);
    const int    pad_each = (n_fft - win_length) / 2;
    const double denom    = periodic ? static_cast<double>(win_length) : static_cast<double>(win_length - 1);
    for (int k = 0; k < win_length; ++k) {
        out[pad_each + k] = 0.5 - 0.5 * std::cos(2.0 * M_PI * k / denom);
    }
}

// In-place radix-2 Cooley-Tukey FFT, fp64. Operates on n complex
// numbers stored as interleaved (re, im, re, im, ...). n must be a
// power of 2. For n=512 (Parakeet) this is ~80 lines and runs in
// ~10 us per call; the entire jfk.wav frontend (1101 frames) takes
// well under 50 ms on M-series.
//
// Why not vendor pocketfft / KISS FFT: n_fft=512 is small, the
// frontend is one-time per audio buffer (not on the inference hot
// path), and a single hand-rolled radix-2 keeps the dependency
// surface zero. Drop in pocketfft only if profiling later shows the
// frontend is a bottleneck.
// Per-MelFrontend sin/cos cache, indexed by 2π*i / lut_size. The mixed-
// radix path is the only consumer; lut_size == n_fft so every recursion
// size N (n_fft → n_fft/2 → … → odd leaf) divides it. Stride at level N
// is lut_size / N, and (k*n*stride) % lut_size lands on the right phase.

// Naive O(N^2) DFT leaf for the mixed-radix path. Used when N hits an
// odd factor during recursive halving (e.g. N=400 → 200 → 100 → 50 → 25
// odd → leaf). Uses the per-frontend fp32 LUT (lut_size divisible by N
// by construction).
void dft_naive_f32(const float * in, int N, const float * cos_lut, const float * sin_lut, int lut_size, float * out) {
    const int stride = lut_size / N;
    for (int k = 0; k < N; ++k) {
        float re = 0.0f;
        float im = 0.0f;
        for (int n = 0; n < N; ++n) {
            const int idx = (k * n * stride) % lut_size;
            re += in[n] * cos_lut[idx];
            im -= in[n] * sin_lut[idx];
        }
        out[2 * k]     = re;
        out[2 * k + 1] = im;
    }
}

// Cooley-Tukey mixed-radix FFT. Recursively halves while N is even and
// falls back to naive DFT on odd leaves. Algorithm lifted from
// whisper.cpp (src/whisper.cpp::fft); lines up with our need to STFT
// Whisper's n_fft=400 = 2^4 * 5^2 efficiently (25-point DFT leaves,
// four levels of radix-2 butterflies above).
//
// Buffer contract:
//   in    : 2*N floats. First N hold the real input; second N are
//           used as scratch for the even/odd split at this level.
//   out   : 8*N floats. The top-level result lands in the first 2*N;
//           recursion uses the remainder as intermediate output.
void mixed_radix_fft_f32(float * in, int N, const float * cos_lut, const float * sin_lut, int lut_size, float * out) {
    if (N == 1) {
        out[0] = in[0];
        out[1] = 0.0f;
        return;
    }
    if (N & 1) {
        dft_naive_f32(in, N, cos_lut, sin_lut, lut_size, out);
        return;
    }
    const int half_N = N / 2;
    float *   even   = in + N;
    for (int i = 0; i < half_N; ++i) {
        even[i] = in[2 * i];
    }
    float * even_fft = out + 2 * N;
    mixed_radix_fft_f32(even, half_N, cos_lut, sin_lut, lut_size, even_fft);

    float * odd = even;
    for (int i = 0; i < half_N; ++i) {
        odd[i] = in[2 * i + 1];
    }
    float * odd_fft = even_fft + N;
    mixed_radix_fft_f32(odd, half_N, cos_lut, sin_lut, lut_size, odd_fft);

    const int step = lut_size / N;
    for (int k = 0; k < half_N; ++k) {
        const int   idx           = k * step;
        const float w_re          = cos_lut[idx];
        const float w_im          = -sin_lut[idx];
        const float re_odd        = odd_fft[2 * k];
        const float im_odd        = odd_fft[2 * k + 1];
        out[2 * k]                = even_fft[2 * k] + w_re * re_odd - w_im * im_odd;
        out[2 * k + 1]            = even_fft[2 * k + 1] + w_re * im_odd + w_im * re_odd;
        out[2 * (k + half_N)]     = even_fft[2 * k] - w_re * re_odd + w_im * im_odd;
        out[2 * (k + half_N) + 1] = even_fft[2 * k + 1] - w_re * im_odd - w_im * re_odd;
    }
}

void fft_radix2(double * data, int n) {
    // Bit-reversal permutation.
    int j = 0;
    for (int i = 1; i < n; ++i) {
        int bit = n >> 1;
        for (; j & bit; bit >>= 1) {
            j ^= bit;
        }
        j ^= bit;
        if (i < j) {
            std::swap(data[2 * i], data[2 * j]);
            std::swap(data[2 * i + 1], data[2 * j + 1]);
        }
    }
    // Cooley-Tukey butterflies. Twiddle factors are computed
    // incrementally per butterfly group; this drifts by a few ULPs
    // for very large n but is bit-stable for n=512.
    for (int len = 2; len <= n; len <<= 1) {
        const double ang     = -2.0 * M_PI / static_cast<double>(len);
        const double wlen_re = std::cos(ang);
        const double wlen_im = std::sin(ang);
        for (int i = 0; i < n; i += len) {
            double w_re = 1.0;
            double w_im = 0.0;
            for (int k = 0; k < len / 2; ++k) {
                const int    a      = 2 * (i + k);
                const int    b      = 2 * (i + k + len / 2);
                const double u_re   = data[a];
                const double u_im   = data[a + 1];
                const double v_re   = data[b] * w_re - data[b + 1] * w_im;
                const double v_im   = data[b] * w_im + data[b + 1] * w_re;
                data[a]             = u_re + v_re;
                data[a + 1]         = u_im + v_im;
                data[b]             = u_re - v_re;
                data[b + 1]         = u_im - v_im;
                const double tmp_re = w_re * wlen_re - w_im * wlen_im;
                const double tmp_im = w_re * wlen_im + w_im * wlen_re;
                w_re                = tmp_re;
                w_im                = tmp_im;
            }
        }
    }
}

}  // namespace

// ---------- MelFrontend ----------

MelFrontend::MelFrontend(const MelConfig & cfg) : cfg_(cfg) {
    n_freq_ = cfg.n_fft / 2 + 1;

    // Use checkpoint-provided window if available, else compute.
    if (!cfg.window.empty()) {
        // Convert f32 checkpoint window to f64 and zero-pad to n_fft.
        // pad_mode="none" left-aligns the window inside the n_fft buffer
        // (PyTorch center=False semantics: window applies to pcm[start..
        // start+win_length), remaining slots zero); the centered/symmetric
        // modes pad equally on both sides (NeMo/whisper default).
        window_.resize(cfg.n_fft, 0.0);
        const int total_pad = cfg.n_fft - cfg.win_length;
        const int left_pad  = (cfg.pad_mode == "none") ? 0 : (total_pad / 2);
        for (int i = 0; i < cfg.win_length && i < static_cast<int>(cfg.window.size()); ++i) {
            window_[left_pad + i] = static_cast<double>(cfg.window[i]);
        }
    } else {
        const bool periodic = (cfg.window_type == "hann_periodic");
        build_hann_window_symmetric_padded(cfg.win_length, cfg.n_fft, periodic, window_);
    }

    // Use checkpoint-provided mel filterbank if available, else compute.
    if (!cfg.filterbank.empty()) {
        mel_fb_ = cfg.filterbank;
    } else {
        build_mel_filterbank_slaney(cfg.sample_rate, cfg.n_fft, cfg.num_mels, static_cast<double>(cfg.f_min),
                                    static_cast<double>(cfg.f_max), mel_fb_);
    }

    // Sin/cos LUT for the mixed-radix FFT. Only the non-pow2 path
    // consumes it; pow2 sizes go through fft_radix2 (Linux) or vDSP
    // (Apple), both of which carry their own twiddle factors.
    const bool n_fft_is_pow2 = ((cfg.n_fft > 0) && ((cfg.n_fft & (cfg.n_fft - 1)) == 0));
    if (!n_fft_is_pow2) {
        cos_lut_.resize(cfg.n_fft);
        sin_lut_.resize(cfg.n_fft);
        for (int i = 0; i < cfg.n_fft; ++i) {
            // Compute in fp64 for accuracy, store as fp32 to match the
            // FFT precision. The downcast is one-time per construction.
            const double theta = 2.0 * M_PI * i / cfg.n_fft;
            cos_lut_[i]        = static_cast<float>(std::cos(theta));
            sin_lut_[i]        = static_cast<float>(std::sin(theta));
        }
    }
}

int MelFrontend::n_frames_for(size_t n_samples) const {
    if (cfg_.pad_mode == "none") {
        // PyTorch center=False: n_frames = (n - win)/hop + 1.
        if (static_cast<int>(n_samples) < cfg_.win_length) {
            return 0;
        }
        return static_cast<int>((n_samples - static_cast<size_t>(cfg_.win_length)) /
                                static_cast<size_t>(cfg_.hop_length)) +
               1;
    }
    // Matches NeMo features_lens = (waveforms_lens / hop_length) + 1.
    // The +1 accounts for the centered first frame.
    return static_cast<int>(n_samples / static_cast<size_t>(cfg_.hop_length)) + 1;
}

transcribe_status MelFrontend::compute(const float *        pcm,
                                       size_t               n_samples,
                                       std::vector<float> & out_mel,
                                       int &                out_n_mels,
                                       int &                out_n_frames,
                                       int                  n_threads) const {
    if (pcm == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    const int  n_fft  = cfg_.n_fft;
    const int  hop    = cfg_.hop_length;
    const int  n_mels = cfg_.num_mels;
    const int  n_freq = n_freq_;
    const int  win    = cfg_.win_length;
    const int  pad    = n_fft / 2;
    const bool no_pad = (cfg_.pad_mode == "none");

    const int n_frames =
        no_pad ? (static_cast<int>(n_samples) < win ?
                      0 :
                      static_cast<int>((n_samples - static_cast<size_t>(win)) / static_cast<size_t>(hop)) + 1) :
                 static_cast<int>(n_samples / static_cast<size_t>(hop)) + 1;

    // Per-feature normalize divides by (n_frames - 1) and the
    // reflect pad needs at least pad+1 input samples to reflect
    // without going off the end. pad_mode="none" requires at least one
    // full window in the input.
    const bool use_reflect = (!no_pad && cfg_.pad_mode != "constant");
    if (n_frames < (no_pad ? 1 : 2) || (use_reflect && n_samples < static_cast<size_t>(pad + 1)) ||
        (no_pad && static_cast<int>(n_samples) < win)) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    // ---- 1. Pad + pre-emphasis ----
    //
    // The non-pow2 (mixed-radix) path runs fp32 throughout, so build the
    // padded buffer directly in fp32 and skip the intermediate emph copy.
    // The pow2 paths (vDSP / hand-rolled radix-2) stay fp64 since their
    // FFT routines are fp64-native.
    const bool n_fft_is_pow2 = ((n_fft > 0) && ((n_fft & (n_fft - 1)) == 0));

    std::vector<float>  padded_f32;
    std::vector<float>  window_f32;
    std::vector<double> padded;

    if (!n_fft_is_pow2) {
        // FP32 path: pre-emphasis (or copy) directly into the middle of
        // padded_f32, then reflect/zero-pad in place. window_ is
        // downcast once for the worker.
        padded_f32.resize(n_samples + 2 * static_cast<size_t>(pad));
        if (cfg_.pre_emphasis != 0.0f) {
            const float alpha = cfg_.pre_emphasis;
            padded_f32[pad]   = pcm[0];
            for (size_t i = 1; i < n_samples; ++i) {
                padded_f32[pad + i] = pcm[i] - alpha * pcm[i - 1];
            }
        } else {
            std::memcpy(padded_f32.data() + pad, pcm, n_samples * sizeof(float));
        }
        if (use_reflect) {
            // padded[i] = src[pad - i] for i in [0, pad). After writing
            // src into padded_f32[pad..pad+n_samples), this becomes
            // padded_f32[i] = padded_f32[2*pad - i].
            for (int i = 0; i < pad; ++i) {
                padded_f32[i] = padded_f32[2 * pad - i];
            }
            for (int i = 0; i < pad; ++i) {
                padded_f32[static_cast<size_t>(pad) + n_samples + i] =
                    padded_f32[static_cast<size_t>(pad) + n_samples - 2 - i];
            }
        } else {
            std::memset(padded_f32.data(), 0, static_cast<size_t>(pad) * sizeof(float));
            std::memset(padded_f32.data() + pad + n_samples, 0, static_cast<size_t>(pad) * sizeof(float));
        }

        window_f32.resize(n_fft);
        for (int i = 0; i < n_fft; ++i) {
            window_f32[i] = static_cast<float>(window_[i]);
        }
    } else {
        // FP64 path (Parakeet, Cohere): unchanged. emph buffer + fp64
        // padded, identical to the original implementation.
        std::vector<double> emph(n_samples);
        if (cfg_.pre_emphasis != 0.0f) {
            const double alpha = static_cast<double>(cfg_.pre_emphasis);
            emph[0]            = static_cast<double>(pcm[0]);
            for (size_t i = 1; i < n_samples; ++i) {
                emph[i] = static_cast<double>(pcm[i]) - alpha * static_cast<double>(pcm[i - 1]);
            }
        } else {
            for (size_t i = 0; i < n_samples; ++i) {
                emph[i] = static_cast<double>(pcm[i]);
            }
        }

        if (no_pad) {
            // PyTorch center=False: no input padding. Sized so that the
            // last frame (start=(n_frames-1)*hop) can read n_fft samples
            // without overflowing. The trailing (n_fft - win_length) slots
            // beyond the data are read but always multiplied by zero in
            // the left-aligned window, so their value is irrelevant.
            padded.resize(n_samples + static_cast<size_t>(n_fft - win), 0.0);
            std::memcpy(padded.data(), emph.data(), n_samples * sizeof(double));
        } else {
            padded.resize(n_samples + 2 * static_cast<size_t>(pad));
            if (use_reflect) {
                for (int i = 0; i < pad; ++i) {
                    padded[i] = emph[static_cast<size_t>(pad - i)];
                }
                std::memcpy(padded.data() + pad, emph.data(), n_samples * sizeof(double));
                for (int i = 0; i < pad; ++i) {
                    padded[static_cast<size_t>(pad) + n_samples + i] = emph[n_samples - 2 - static_cast<size_t>(i)];
                }
            } else {
                std::memset(padded.data(), 0, static_cast<size_t>(pad) * sizeof(double));
                std::memcpy(padded.data() + pad, emph.data(), n_samples * sizeof(double));
                std::memset(padded.data() + pad + n_samples, 0, static_cast<size_t>(pad) * sizeof(double));
            }
        }
    }

    // ---- 3. STFT + mel filterbank matmul + log ----
    //
    // Output: log_mel[n_mels, n_frames] row-major float32. Two paths by
    // n_fft factorization:
    //
    //   non-pow2 (Whisper, Qwen3-ASR): FFT worker fuses the per-frame mel
    //     matmul + log directly into log_mel (no shared power[]), matmul
    //     parallelized across stft_threads. Same as whisper.cpp; a win on
    //     no-BLAS CPUs.
    //   pow2 (Parakeet, Cohere): FFT worker writes shared power[], then a
    //     post-pass does matmul + log via cblas_sgemm (much faster than a
    //     per-frame scalar matmul). vDSP / fft_radix2 are fp64-native, so
    //     the fp64 power[] avoids cast traffic.
    //
    // log mode (whisper_mode): per_utterance writes log10(max(x, 1e-10));
    // per_feature writes log(x + 2^-24) (natural log; base is irrelevant
    // to the per-bin mean/std normalize that follows).
    std::vector<float> log_mel(static_cast<size_t>(n_mels) * static_cast<size_t>(n_frames));

    const bool whisper_mode = (cfg_.normalize == "per_utterance" || cfg_.normalize == "global");

    int stft_threads = n_threads;
    if (stft_threads <= 0) {
        stft_threads = default_n_threads();
    }
    if (stft_threads > n_frames) {
        stft_threads = std::max(1, n_frames);
    }

    // Per-thread worker dispatcher. Strided frame assignment (stride =
    // stft_threads) keeps load balanced and per-thread memory access
    // patterns mostly sequential.
    auto run_threaded = [&](auto && worker) {
        if (stft_threads <= 1) {
            worker(0);
            return;
        }
        std::vector<std::thread> pool;
        pool.reserve(static_cast<size_t>(stft_threads - 1));
        for (int tid = 1; tid < stft_threads; ++tid) {
            pool.emplace_back(worker, tid);
        }
        worker(0);
        for (auto & th : pool) {
            th.join();
        }
    };

    if (!n_fft_is_pow2) {
        // Fused worker: window mul → mixed-radix FFT → power → mel
        // matmul + log → log_mel, all fp32, all per-frame on this thread.
        // Matmul uses a 4-way-unrolled fp64 accumulator for numerical
        // stability; cast back to fp32 at the log boundary.
        auto worker = [&](int tid) {
            std::vector<float> fft_in(2 * static_cast<size_t>(n_fft), 0.0f);
            std::vector<float> fft_out(8 * static_cast<size_t>(n_fft), 0.0f);
            std::vector<float> power_scratch(static_cast<size_t>(n_freq));
            for (int t = tid; t < n_frames; t += stft_threads) {
                const size_t start = static_cast<size_t>(t) * static_cast<size_t>(hop);
                for (int n = 0; n < n_fft; ++n) {
                    fft_in[n] = padded_f32[start + n] * window_f32[n];
                }
                mixed_radix_fft_f32(fft_in.data(), n_fft, cos_lut_.data(), sin_lut_.data(),
                                    static_cast<int>(cos_lut_.size()), fft_out.data());
                for (int k = 0; k < n_freq; ++k) {
                    const float re   = fft_out[2 * k];
                    const float im   = fft_out[2 * k + 1];
                    power_scratch[k] = re * re + im * im;
                }
                for (int m = 0; m < n_mels; ++m) {
                    const float * fb_row = mel_fb_.data() + static_cast<size_t>(m) * n_freq;
                    double        sum    = 0.0;
                    int           k      = 0;
                    for (; k < n_freq - 3; k += 4) {
                        sum += static_cast<double>(fb_row[k]) * static_cast<double>(power_scratch[k]) +
                               static_cast<double>(fb_row[k + 1]) * static_cast<double>(power_scratch[k + 1]) +
                               static_cast<double>(fb_row[k + 2]) * static_cast<double>(power_scratch[k + 2]) +
                               static_cast<double>(fb_row[k + 3]) * static_cast<double>(power_scratch[k + 3]);
                    }
                    for (; k < n_freq; ++k) {
                        sum += static_cast<double>(fb_row[k]) * static_cast<double>(power_scratch[k]);
                    }
                    float result;
                    if (whisper_mode) {
                        if (sum < 1.0e-10) {
                            sum = 1.0e-10;
                        }
                        result = static_cast<float>(std::log10(sum));
                    } else {
                        result = static_cast<float>(std::log(sum + static_cast<double>(kLogEps)));
                    }
                    log_mel[static_cast<size_t>(m) * n_frames + t] = result;
                }
            }
        };
        run_threaded(worker);
    } else {
        // pow2 paths (Parakeet, Cohere): unchanged structure. FFT
        // worker writes shared power[]; post-pass does the matmul +
        // log.
        std::vector<float> power(static_cast<size_t>(n_frames) * static_cast<size_t>(n_freq));

#ifdef __APPLE__
        // vDSP real-input FFT (fp64, vectorized via Accelerate SIMD).
        int log2n = 0;
        {
            int tmp = n_fft;
            while (tmp > 1) {
                tmp >>= 1;
                ++log2n;
            }
        }

        // A single FFTSetupD is shared across threads: it stores
        // precomputed twiddle factors, not transient state. vDSP's
        // forward FFT is thread-safe given per-thread split-complex
        // buffers.
        FFTSetupD    fft_setup = vDSP_create_fftsetupD(log2n, FFT_RADIX2);
        const size_t half_n    = static_cast<size_t>(n_fft / 2);

        auto worker = [&](int tid) {
            std::vector<double>   fft_real(half_n);
            std::vector<double>   fft_imag(half_n);
            DSPDoubleSplitComplex split = { fft_real.data(), fft_imag.data() };

            for (int t = tid; t < n_frames; t += stft_threads) {
                const size_t start = static_cast<size_t>(t) * static_cast<size_t>(hop);
                for (size_t k = 0; k < half_n; ++k) {
                    fft_real[k] = padded[start + 2 * k] * window_[2 * k];
                    fft_imag[k] = padded[start + 2 * k + 1] * window_[2 * k + 1];
                }
                vDSP_fft_zripD(fft_setup, &split, 1, log2n, FFT_FORWARD);
                // vDSP output is 2x the standard DFT, so power is 4x.
                // realp[0]=DC*2, imagp[0]=Nyquist*2, otherwise complex
                // pairs in (realp[k], imagp[k]).
                float * pwr_row    = power.data() + static_cast<size_t>(t) * n_freq;
                pwr_row[0]         = static_cast<float>(fft_real[0] * fft_real[0] * 0.25);
                pwr_row[n_fft / 2] = static_cast<float>(fft_imag[0] * fft_imag[0] * 0.25);
                for (size_t k = 1; k < half_n; ++k) {
                    pwr_row[k] = static_cast<float>((fft_real[k] * fft_real[k] + fft_imag[k] * fft_imag[k]) * 0.25);
                }
            }
        };
        run_threaded(worker);
        vDSP_destroy_fftsetupD(fft_setup);
#else
        // Hand-rolled radix-2 Cooley-Tukey FFT fallback for non-Apple.
        auto worker = [&](int tid) {
            std::vector<double> frame(2 * static_cast<size_t>(n_fft));
            for (int t = tid; t < n_frames; t += stft_threads) {
                const size_t start = static_cast<size_t>(t) * static_cast<size_t>(hop);
                for (int k = 0; k < n_fft; ++k) {
                    frame[2 * static_cast<size_t>(k)]     = padded[start + static_cast<size_t>(k)] * window_[k];
                    frame[2 * static_cast<size_t>(k) + 1] = 0.0;
                }
                fft_radix2(frame.data(), n_fft);
                float * pwr_row = power.data() + static_cast<size_t>(t) * n_freq;
                for (int k = 0; k < n_freq; ++k) {
                    const double re = frame[2 * static_cast<size_t>(k)];
                    const double im = frame[2 * static_cast<size_t>(k) + 1];
                    pwr_row[k]      = static_cast<float>(re * re + im * im);
                }
            }
        };
        run_threaded(worker);
#endif

        // pow2 post-pass matmul + log → log_mel.
#if defined(__APPLE__) || TRANSCRIBE_HAS_BLAS
        // C = A @ B^T  where A=[n_mels, n_freq], B=[n_frames, n_freq].
        cblas_sgemm(CblasRowMajor, CblasNoTrans, CblasTrans, n_mels, n_frames, n_freq, 1.0f, mel_fb_.data(), n_freq,
                    power.data(), n_freq, 0.0f, log_mel.data(), n_frames);
        {
            const size_t total = static_cast<size_t>(n_mels) * static_cast<size_t>(n_frames);
            if (whisper_mode) {
                for (size_t i = 0; i < total; ++i) {
                    double v = static_cast<double>(log_mel[i]);
                    if (v < 1.0e-10) {
                        v = 1.0e-10;
                    }
                    log_mel[i] = static_cast<float>(std::log10(v));
                }
            } else if (cfg_.log_clamp_min > 0.0f) {
                // LASR/MedASR: natural log with explicit floor-clamp
                // (vs NeMo's log(x + kLogEps)). Differs only in the small-
                // power regime; the clamp keeps the result bounded at
                // log(clamp_min) instead of plunging to -log(kLogEps).
                const float clamp = cfg_.log_clamp_min;
                for (size_t i = 0; i < total; ++i) {
                    float v = log_mel[i];
                    if (v < clamp) {
                        v = clamp;
                    }
                    log_mel[i] = std::log(v);
                }
            } else {
                for (size_t i = 0; i < total; ++i) {
                    log_mel[i] = std::log(log_mel[i] + kLogEps);
                }
            }
        }
#else
        // Scalar fused matmul + log fallback (no BLAS available).
        for (int t = 0; t < n_frames; ++t) {
            const float * pwr = power.data() + static_cast<size_t>(t) * n_freq;
            for (int m = 0; m < n_mels; ++m) {
                const float * fb_row = mel_fb_.data() + static_cast<size_t>(m) * n_freq;
                double        sum    = 0.0;
                int           k      = 0;
                for (; k < n_freq - 3; k += 4) {
                    sum += static_cast<double>(fb_row[k]) * static_cast<double>(pwr[k]) +
                           static_cast<double>(fb_row[k + 1]) * static_cast<double>(pwr[k + 1]) +
                           static_cast<double>(fb_row[k + 2]) * static_cast<double>(pwr[k + 2]) +
                           static_cast<double>(fb_row[k + 3]) * static_cast<double>(pwr[k + 3]);
                }
                for (; k < n_freq; ++k) {
                    sum += static_cast<double>(fb_row[k]) * static_cast<double>(pwr[k]);
                }
                float result;
                if (whisper_mode) {
                    if (sum < 1.0e-10) {
                        sum = 1.0e-10;
                    }
                    result = static_cast<float>(std::log10(sum));
                } else if (cfg_.log_clamp_min > 0.0f) {
                    const double clamp = static_cast<double>(cfg_.log_clamp_min);
                    if (sum < clamp) {
                        sum = clamp;
                    }
                    result = static_cast<float>(std::log(sum));
                } else {
                    result = static_cast<float>(std::log(sum + static_cast<double>(kLogEps)));
                }
                log_mel[static_cast<size_t>(m) * n_frames + t] = result;
            }
        }
#endif
    }

    std::vector<double>().swap(padded);
    std::vector<float>().swap(padded_f32);
    std::vector<float>().swap(window_f32);

    // ---- 5n. No-op normalize (NeMo "NA"/none) ----
    // Emit raw log-mel as-is: streaming Conformer variants (e.g.
    // nemotron-speech-streaming-en-0.6b) bake feature normalization into
    // the encoder weights, matching NeMo's normalize_batch fall-through.
    //
    // Trailing-frame masking (the canonical rationale): NeMo's
    // FilterbankFeatures.forward masks frames beyond get_seq_len =
    // floor(n_samples / hop_length) to pad_value (zero), after normalize,
    // regardless of normalize_type. The reflect-pad STFT emits one extra
    // frame (n_frames = floor(n/hop) + 1), so the last frame is a padding
    // artifact and we zero it to mirror NeMo. Without this the encoder
    // sees a real log-mel value there and the drift dominates the early
    // pre_encode output (max_abs ~13 on that single frame).
    if (cfg_.normalize == "none") {
        out_mel      = std::move(log_mel);
        out_n_mels   = n_mels;
        out_n_frames = n_frames;
        // pad_mode="none" emits exactly (n - win)/hop + 1 frames with no
        // padding artifact, so skip the trailing mask (it would zero a
        // legitimate final frame). NeMo-style reflect/constant padding
        // needs the mask — see the trailing-frame rationale above.
        if (!no_pad) {
            const int valid = static_cast<int>(n_samples / static_cast<size_t>(cfg_.hop_length));
            for (int m = 0; m < n_mels; ++m) {
                float * row = out_mel.data() + static_cast<size_t>(m) * n_frames;
                for (int t = valid; t < n_frames; ++t) {
                    row[t] = 0.0f;
                }
            }
        }
        return TRANSCRIBE_OK;
    }

    // ---- 5a. Whisper-style per-utterance normalization ----
    // log_mel already holds log10(max(x, 1e-10)). Find the global max,
    // clamp to max - 8.0, then scale (x + 4) / 4. Drops the trailing
    // center-pad STFT frame (output has n_samples / hop_length frames).
    if (cfg_.normalize == "per_utterance") {
        const int n_out = n_frames - 1;
        if (n_out <= 0) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }

        double global_max = -std::numeric_limits<double>::infinity();
        for (int m = 0; m < n_mels; ++m) {
            const float * src = log_mel.data() + static_cast<size_t>(m) * n_frames;
            for (int t = 0; t < n_out; ++t) {
                const double v = static_cast<double>(src[t]);
                if (v > global_max) {
                    global_max = v;
                }
            }
        }
        const double floor_val = global_max - 8.0;

        out_mel.resize(static_cast<size_t>(n_mels) * static_cast<size_t>(n_out));
        for (int m = 0; m < n_mels; ++m) {
            const float * src = log_mel.data() + static_cast<size_t>(m) * n_frames;
            float *       dst = out_mel.data() + static_cast<size_t>(m) * n_out;
            for (int t = 0; t < n_out; ++t) {
                double v = static_cast<double>(src[t]);
                if (v < floor_val) {
                    v = floor_val;
                }
                dst[t] = static_cast<float>((v + 4.0) / 4.0);
            }
        }

        out_n_mels   = n_mels;
        out_n_frames = n_out;
        return TRANSCRIBE_OK;
    }

    // ---- 5b. Voxtral Realtime streaming log-mel (fixed global max) ----
    // Like per_utterance but the clamp floor uses a FIXED max
    // (global_log_mel_max), making per-frame normalization causal/
    // streaming-safe. Drops the trailing center-pad frame.
    if (cfg_.normalize == "global") {
        const int n_out = n_frames - 1;
        if (n_out <= 0) {
            return TRANSCRIBE_ERR_INVALID_ARG;
        }
        const double floor_val = static_cast<double>(cfg_.global_log_mel_max) - 8.0;

        out_mel.resize(static_cast<size_t>(n_mels) * static_cast<size_t>(n_out));
        for (int m = 0; m < n_mels; ++m) {
            const float * src = log_mel.data() + static_cast<size_t>(m) * n_frames;
            float *       dst = out_mel.data() + static_cast<size_t>(m) * n_out;
            for (int t = 0; t < n_out; ++t) {
                double v = static_cast<double>(src[t]);
                if (v < floor_val) {
                    v = floor_val;
                }
                dst[t] = static_cast<float>((v + 4.0) / 4.0);
            }
        }
        out_n_mels   = n_mels;
        out_n_frames = n_out;
        return TRANSCRIBE_OK;
    }

    // ---- 5. Per-feature normalize, unbiased ----
    // For each mel bin m: subtract the time mean, divide by (std + 1e-5).
    // Variance uses (n_norm - 1) (Bessel's correction) to match NeMo's
    // normalize_batch. fp64 accumulators throughout, cast to fp32 at
    // storage.
    //
    // pad_mode "constant" (Cohere): the last STFT frame is a padding
    // artifact; stats are over the first seq_len = n/hop frames and the
    // last is zeroed after normalize (processing_cohere_asr.py
    // get_seq_len / normalize_batch).
    //
    // pad_mode "reflect" (Parakeet / Canary): all n_frames are valid.
    // Unconditionally masking the trailing frame regresses long-form
    // Canary decode (test-clean 672-122797-0008, 30.8s, falls into a "the
    // boy was a man of the world" loop): for reflect-padded single-segment
    // offline runs NeMo's seq_len == n_frames so it masks nothing — the
    // pad_mode gate preserves that.
    //
    // resize() not assign(): the loop writes every element once.
    const bool mask_last = (cfg_.pad_mode == "constant");
    const int  n_norm    = mask_last ? (n_frames - 1) : n_frames;

    out_mel.resize(static_cast<size_t>(n_mels) * static_cast<size_t>(n_frames));
    for (int m = 0; m < n_mels; ++m) {
        const float * src = log_mel.data() + static_cast<size_t>(m) * n_frames;
        float *       dst = out_mel.data() + static_cast<size_t>(m) * n_frames;

        double sum = 0.0;
        for (int t = 0; t < n_norm; ++t) {
            sum += static_cast<double>(src[t]);
        }
        const double mean = sum / static_cast<double>(n_norm);

        double sumsq = 0.0;
        for (int t = 0; t < n_norm; ++t) {
            const double d = static_cast<double>(src[t]) - mean;
            sumsq += d * d;
        }
        const double var    = sumsq / static_cast<double>(n_norm - 1);
        const double stddev = std::sqrt(var);
        const double inv    = 1.0 / (stddev + static_cast<double>(kNormEps));

        for (int t = 0; t < n_norm; ++t) {
            dst[t] = static_cast<float>((static_cast<double>(src[t]) - mean) * inv);
        }

        // Zero the last frame when it's a padding artifact.
        if (mask_last) {
            dst[n_norm] = 0.0f;
        }
    }

    out_n_mels   = n_mels;
    out_n_frames = n_frames;
    return TRANSCRIBE_OK;
}

}  // namespace transcribe
