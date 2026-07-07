// arch/gigaam/mel.cpp - GigaAM log-mel feature extractor.
//
// Reference: gigaam.preprocess.FeatureExtractor (torchaudio MelSpectrogram,
// htk, n_mels=64, n_fft=win_length=320, hop=160, center=False, power=2,
// periodic Hann) followed by log(clamp(x, 1e-9, 1e9)). n_fft=320 = 2^6 * 5
// uses a mixed-radix Cooley-Tukey FFT (radix-2 down to a naive DFT leaf at N=5).

#include "mel.h"

#include "ggml-backend.h"
#include "ggml.h"
#include "weights.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>

namespace transcribe::gigaam {

namespace {

// Naive O(N²) DFT leaf for the mixed-radix path. Used when N hits the
// odd-factor leaf (N=5 for n_fft=320).
void dft_naive(const float * in, int N, float * out) {
    for (int k = 0; k < N; ++k) {
        double re = 0.0, im = 0.0;
        for (int n = 0; n < N; ++n) {
            const double theta = -2.0 * M_PI * k * n / N;
            re += in[n] * std::cos(theta);
            im += in[n] * std::sin(theta);
        }
        out[2 * k]     = static_cast<float>(re);
        out[2 * k + 1] = static_cast<float>(im);
    }
}

// Mixed-radix Cooley-Tukey FFT. Recursively halves while N is even;
// falls back to dft_naive on the odd leaf.
//
// Buffers:
//   in : 2N floats (first N: input, second N: even/odd scratch).
//   out: 8N floats (top-level result in first 2N).
void mixed_radix_fft(float * in, int N, float * out) {
    if (N == 1) {
        out[0] = in[0];
        out[1] = 0.0f;
        return;
    }
    if (N & 1) {
        dft_naive(in, N, out);
        return;
    }
    const int half_N = N / 2;
    float *   even   = in + N;
    for (int i = 0; i < half_N; ++i) {
        even[i] = in[2 * i];
    }
    float * even_fft = out + 2 * N;
    mixed_radix_fft(even, half_N, even_fft);

    float * odd = even;
    for (int i = 0; i < half_N; ++i) {
        odd[i] = in[2 * i + 1];
    }
    float * odd_fft = even_fft + N;
    mixed_radix_fft(odd, half_N, odd_fft);

    for (int k = 0; k < half_N; ++k) {
        const double theta        = -2.0 * M_PI * k / N;
        const float  w_re         = static_cast<float>(std::cos(theta));
        const float  w_im         = static_cast<float>(std::sin(theta));
        const float  re_odd       = odd_fft[2 * k];
        const float  im_odd       = odd_fft[2 * k + 1];
        out[2 * k]                = even_fft[2 * k] + w_re * re_odd - w_im * im_odd;
        out[2 * k + 1]            = even_fft[2 * k + 1] + w_re * im_odd + w_im * re_odd;
        out[2 * (k + half_N)]     = even_fft[2 * k] - w_re * re_odd + w_im * im_odd;
        out[2 * (k + half_N) + 1] = even_fft[2 * k + 1] - w_re * im_odd - w_im * re_odd;
    }
}

}  // namespace

void GigaamMelFrontend::init(const GigaamHParams & hp, const GigaamWeights & w) {
    n_mels     = hp.fe_num_mels;
    n_fft      = hp.fe_n_fft;
    win_length = hp.fe_win_length;
    hop_length = hp.fe_hop_length;
    n_freq     = n_fft / 2 + 1;

    // Read the baked window and filterbank from the GGUF. The
    // converter emits frontend.window with ne=[win_length] and
    // frontend.mel_filterbank with ggml ne=[n_freq, n_mels] (Python
    // shape [n_mels, n_freq] row-major). Both are pure F32, so flat
    // readback matches numpy row-major directly.
    hann.assign(win_length, 0.0f);
    if (w.frontend_window != nullptr) {
        ggml_backend_tensor_get(w.frontend_window, hann.data(), 0, hann.size() * sizeof(float));
    }
    mel_fb.assign(static_cast<size_t>(n_mels) * n_freq, 0.0f);
    if (w.frontend_mel_filterbank != nullptr) {
        ggml_backend_tensor_get(w.frontend_mel_filterbank, mel_fb.data(), 0, mel_fb.size() * sizeof(float));
    }
}

int GigaamMelFrontend::n_frames_for(size_t n_samples) const {
    if (static_cast<int>(n_samples) < win_length) {
        return 0;
    }
    return static_cast<int>((n_samples - win_length) / hop_length) + 1;
}

transcribe_status GigaamMelFrontend::compute(const float *        pcm,
                                             size_t               n_samples,
                                             std::vector<float> & out_mel,
                                             int &                out_n_frames) const {
    if (pcm == nullptr) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }

    const int n_frames = n_frames_for(n_samples);
    if (n_frames <= 0) {
        return TRANSCRIBE_ERR_INVALID_ARG;
    }
    out_n_frames = n_frames;

    out_mel.assign(static_cast<size_t>(n_mels) * n_frames, 0.0f);

    // Scratch buffers for the FFT. Reused across frames.
    std::vector<float> fft_in(2 * n_fft);
    std::vector<float> fft_out(8 * n_fft);
    std::vector<float> power_spec(n_freq);

    for (int t = 0; t < n_frames; ++t) {
        const size_t start = static_cast<size_t>(t) * hop_length;
        // Window the frame: x[start..start+win_length) * hann.
        // n_fft == win_length for gigaam, so the FFT input is exactly
        // the windowed frame (no zero-pad needed). The pre-zeroed
        // scratch space ahead of the FFT_in[win_length..n_fft) covers
        // the unlikely n_fft > win_length case for completeness.
        std::fill(fft_in.begin(), fft_in.begin() + n_fft, 0.0f);
        for (int k = 0; k < win_length; ++k) {
            fft_in[k] = pcm[start + k] * hann[k];
        }
        mixed_radix_fft(fft_in.data(), n_fft, fft_out.data());

        // power_spec[k] = |X[k]|^2 (power=2).
        for (int k = 0; k < n_freq; ++k) {
            const float re = fft_out[2 * k];
            const float im = fft_out[2 * k + 1];
            power_spec[k]  = re * re + im * im;
        }

        // mel[m] = sum_k filterbank[m, k] * power_spec[k]; then
        // log(clamp(x, 1e-9, 1e9)).
        for (int m = 0; m < n_mels; ++m) {
            const float * row = mel_fb.data() + static_cast<size_t>(m) * n_freq;
            float         acc = 0.0f;
            for (int k = 0; k < n_freq; ++k) {
                acc += row[k] * power_spec[k];
            }
            if (acc < 1.0e-9f) {
                acc = 1.0e-9f;
            }
            // The upper clamp at 1e9 in the reference is unreachable for
            // realistic 16-bit PCM, but mirror it for byte-equivalence.
            if (acc > 1.0e9f) {
                acc = 1.0e9f;
            }
            out_mel[static_cast<size_t>(m) * n_frames + t] = std::log(acc);
        }
    }
    return TRANSCRIBE_OK;
}

}  // namespace transcribe::gigaam
