// transcribe-kaldi-fbank.cpp - kaldi HTK fbank + LFR (+ optional CMVN).
//
// Pure host code (no ggml). See transcribe-kaldi-fbank.h for the
// high-level contract.

#include "transcribe-kaldi-fbank.h"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <utility>
#include <vector>

namespace transcribe {

namespace {

constexpr float kPreemphasis = 0.97f;  // kaldi default
constexpr float kMelLowFreq  = 20.0f;  // kaldi default
constexpr float kMelHighFreq = 0.0f;   // 0 → use sample_rate / 2

int next_pow2(int n) {
    int p = 1;
    while (p < n) {
        p <<= 1;
    }
    return p;
}

// HTK mel formula. The constant 1127.x = 1000 / log10(1 + 1000/700) up
// to ~ulp; using 1127.0 here matches kaldi's `mel-computations.h`.
double hz_to_mel(double f) {
    return 1127.0 * std::log(1.0 + f / 700.0);
}

double mel_to_hz(double m) {
    return 700.0 * (std::exp(m / 1127.0) - 1.0);
}

// HTK mel filterbank: [n_mels, n_freq] row-major f32, n_freq =
// padded_n_fft/2 + 1. Triangular bins peak at 1.0 with no area
// normalization (HTK semantics; differs from slaney).
std::vector<float> build_htk_mel_filterbank(int   sample_rate,
                                            int   padded_n_fft,
                                            int   n_mels,
                                            float low_freq,
                                            float high_freq) {
    const int n_freq = padded_n_fft / 2 + 1;
    if (high_freq <= 0.0f) {
        high_freq = static_cast<float>(sample_rate) * 0.5f;
    }
    const double mel_low  = hz_to_mel(low_freq);
    const double mel_high = hz_to_mel(high_freq);
    const double mel_step = (mel_high - mel_low) / (n_mels + 1);

    // kaldi-style fft_bin_width = sample_rate / padded_n_fft.
    const double fft_bin_width = static_cast<double>(sample_rate) / static_cast<double>(padded_n_fft);

    std::vector<float> fb(static_cast<size_t>(n_mels) * n_freq, 0.0f);
    for (int m = 0; m < n_mels; ++m) {
        const double left_mel   = mel_low + m * mel_step;
        const double center_mel = mel_low + (m + 1) * mel_step;
        const double right_mel  = mel_low + (m + 2) * mel_step;
        const double left_hz    = mel_to_hz(left_mel);
        const double center_hz  = mel_to_hz(center_mel);
        const double right_hz   = mel_to_hz(right_mel);

        for (int b = 0; b < n_freq; ++b) {
            const double bin_hz = b * fft_bin_width;
            double       w      = 0.0;
            if (bin_hz > left_hz && bin_hz < right_hz) {
                if (bin_hz <= center_hz) {
                    w = (bin_hz - left_hz) / (center_hz - left_hz);
                } else {
                    w = (right_hz - bin_hz) / (right_hz - center_hz);
                }
            }
            if (w < 0.0) {
                w = 0.0;
            }
            fb[static_cast<size_t>(m) * n_freq + b] = static_cast<float>(w);
        }
    }
    return fb;
}

// Symmetric kaldi hamming window: 0.54 - 0.46·cos(2πn/(N-1)).
std::vector<float> build_hamming_window(int N) {
    std::vector<float> w(static_cast<size_t>(N));
    if (N <= 1) {
        for (auto & v : w) {
            v = 1.0f;
        }
        return w;
    }
    const double a = 2.0 * M_PI / (N - 1);
    for (int i = 0; i < N; ++i) {
        w[static_cast<size_t>(i)] = static_cast<float>(0.54 - 0.46 * std::cos(a * i));
    }
    return w;
}

// Bit-reversed radix-2 FFT in-place. `data` is interleaved
// [re_0, im_0, re_1, im_1, ...] of length 2n. n must be a power of 2.
void fft_radix2(double * data, int n) {
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

KaldiFbankFrontend::KaldiFbankFrontend(KaldiFbankParams params) :
    params_(std::move(params)),
    padded_n_fft_(next_pow2(params_.win_length)) {
    mel_fb_ = build_htk_mel_filterbank(params_.sample_rate, padded_n_fft_, params_.n_mels, kMelLowFreq, kMelHighFreq);
    window_ = build_hamming_window(params_.win_length);
}

int KaldiFbankFrontend::compute(const float * pcm, size_t n_samples, std::vector<float> & out_features) const {
    const int win_length = params_.win_length;
    const int hop_length = params_.hop_length;
    const int n_mels     = params_.n_mels;
    const int lfr_m      = params_.lfr_m;
    const int lfr_n      = params_.lfr_n;
    const int d_input    = params_.d_input;

    if (pcm == nullptr || n_samples < static_cast<size_t>(win_length)) {
        out_features.clear();
        return 0;
    }

    // Snip-edges framing: T_frames = 1 + floor((N - win_length) / hop).
    const int T_frames =
        1 + static_cast<int>((n_samples - static_cast<size_t>(win_length)) / static_cast<size_t>(hop_length));
    if (T_frames <= 0) {
        out_features.clear();
        return 0;
    }

    const int   n_freq  = padded_n_fft_ / 2 + 1;
    const float upscale = params_.upscale_samples ? 32768.0f : 1.0f;

    // Per-frame buffers reused across the loop.
    std::vector<double> frame(static_cast<size_t>(padded_n_fft_) * 2, 0.0);
    std::vector<float>  frame_f(static_cast<size_t>(win_length), 0.0f);
    std::vector<float>  power(static_cast<size_t>(n_freq), 0.0f);

    // Mel features [T_frames, n_mels] row-major.
    std::vector<float> mel(static_cast<size_t>(T_frames) * n_mels, 0.0f);

    for (int t = 0; t < T_frames; ++t) {
        // Step 1: read frame, upscale.
        const size_t base = static_cast<size_t>(t) * hop_length;
        for (int i = 0; i < win_length; ++i) {
            frame_f[static_cast<size_t>(i)] = pcm[base + i] * upscale;
        }

        // Step 2: remove DC offset (mean subtraction).
        double sum = 0.0;
        for (int i = 0; i < win_length; ++i) {
            sum += frame_f[static_cast<size_t>(i)];
        }
        const float mean = static_cast<float>(sum / win_length);
        for (int i = 0; i < win_length; ++i) {
            frame_f[static_cast<size_t>(i)] -= mean;
        }

        // Step 3: kaldi preemphasis with special x[0] handling. The
        // operation must not be done in-place forward (each x[i]
        // depends on x[i-1] from BEFORE preemph) — kaldi works backward
        // i = N-1 .. 1, then handles i=0.
        for (int i = win_length - 1; i >= 1; --i) {
            frame_f[static_cast<size_t>(i)] -= kPreemphasis * frame_f[static_cast<size_t>(i - 1)];
        }
        frame_f[0] *= (1.0f - kPreemphasis);

        // Step 4: apply hamming window.
        for (int i = 0; i < win_length; ++i) {
            frame_f[static_cast<size_t>(i)] *= window_[static_cast<size_t>(i)];
        }

        // Step 5: pad to padded_n_fft and FFT.
        for (int i = 0; i < padded_n_fft_; ++i) {
            const float re   = i < win_length ? frame_f[static_cast<size_t>(i)] : 0.0f;
            frame[2 * i]     = static_cast<double>(re);
            frame[2 * i + 1] = 0.0;
        }
        fft_radix2(frame.data(), padded_n_fft_);

        // Step 6: power spectrum.
        for (int b = 0; b < n_freq; ++b) {
            const double re               = frame[2 * b];
            const double im               = frame[2 * b + 1];
            power[static_cast<size_t>(b)] = static_cast<float>(re * re + im * im);
        }

        // Step 7-8: HTK mel filterbank + log with energy_floor. funasr
        // passes energy_floor=0.0; with that, kaldi falls back to its
        // hard-coded mel-energy floor of `FLT_EPSILON` (≈ 1.19e-7), NOT
        // the underflow-floor `FLT_MIN`. log of that floor is ~ -15.94.
        float * mel_row = mel.data() + static_cast<size_t>(t) * n_mels;
        for (int m = 0; m < n_mels; ++m) {
            const float * fb_row = mel_fb_.data() + static_cast<size_t>(m) * n_freq;
            float         acc    = 0.0f;
            for (int b = 0; b < n_freq; ++b) {
                acc += fb_row[b] * power[static_cast<size_t>(b)];
            }
            constexpr float kMelEpsilon     = 1.1920928955078125e-07f;
            const float     floored         = std::max(acc, kMelEpsilon);
            mel_row[static_cast<size_t>(m)] = std::log(floored);
        }
    }

    // Step 9: LFR stacking. Mirrors funasr.frontends.wav_frontend.apply_lfr.
    // Left-pad mel by (lfr_m - 1) / 2 copies of mel frame 0 BEFORE
    // striding (so the first LFR row sees a centered context around
    // mel frame 0). Row i then covers padded[i·lfr_n : i·lfr_n + lfr_m];
    // when a row reads past the last real mel frame, the source index
    // is clamped to T_frames - 1 (right-edge replication).
    //
    // T_lfr = ceil(T_frames / lfr_n) — based on the ORIGINAL mel frame
    // count, NOT the left-padded count. This matches apply_lfr.
    const int left_pad = (lfr_m - 1) / 2;
    const int T_lfr    = (T_frames + lfr_n - 1) / lfr_n;
    out_features.assign(static_cast<size_t>(T_lfr) * d_input, 0.0f);
    for (int t_lfr = 0; t_lfr < T_lfr; ++t_lfr) {
        float * out_row = out_features.data() + static_cast<size_t>(t_lfr) * d_input;
        for (int k = 0; k < lfr_m; ++k) {
            const int padded_idx = t_lfr * lfr_n + k;
            // Map padded index back to real mel frame.
            int       src;
            if (padded_idx < left_pad) {
                src = 0;
            } else {
                src = padded_idx - left_pad;
                if (src >= T_frames) {
                    src = T_frames - 1;
                }
            }
            const float * src_row = mel.data() + static_cast<size_t>(src) * n_mels;
            std::memcpy(out_row + static_cast<size_t>(k) * n_mels, src_row, sizeof(float) * n_mels);
        }
        // Step 10: optional per-feature CMVN: (x + shift) * scale.
        if (params_.apply_cmvn) {
            const float * shift = params_.cmvn_shift.data();
            const float * scale = params_.cmvn_scale.data();
            for (int j = 0; j < d_input; ++j) {
                out_row[j] = (out_row[j] + shift[j]) * scale[j];
            }
        }
    }

    return T_lfr;
}

}  // namespace transcribe
