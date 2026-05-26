use ndarray::Array2;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

/// Window function type for mel spectrogram computation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowType {
    Hamming,
    Hann,
}

/// Configuration for mel spectrogram / FBANK feature extraction.
#[derive(Debug, Clone)]
pub struct MelConfig {
    pub sample_rate: u32,
    pub num_mels: usize,
    pub n_fft: usize,
    pub hop_length: usize,
    pub window: WindowType,
    pub f_min: f32,
    pub f_max: Option<f32>,
    pub pre_emphasis: Option<f32>,
    pub snip_edges: bool,
    /// If true, input samples are assumed normalized [-1,1] and used as-is.
    /// If false, samples are scaled to [-32768,32767] before processing (SenseVoice default).
    pub normalize_samples: bool,
}

impl Default for MelConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            num_mels: 80,
            n_fft: 400,
            hop_length: 160,
            window: WindowType::Hamming,
            f_min: 20.0,
            f_max: None,
            pre_emphasis: Some(0.97),
            snip_edges: true,
            normalize_samples: true,
        }
    }
}

/// Compute mel spectrogram features from audio samples.
///
/// Always returns an array of shape `[num_frames, num_mels]` (time-major).
pub fn compute_mel(samples: &[f32], config: &MelConfig) -> Array2<f32> {
    let sr = config.sample_rate as f32;
    let f_max = config.f_max.unwrap_or(sr / 2.0);

    if config.pre_emphasis.is_some() {
        compute_fbank(samples, config, sr, f_max)
    } else {
        compute_mel_spectrogram(samples, config, sr, f_max)
    }
}

/// FBANK-style feature extraction (SenseVoice): pre-emphasis + windowed frames + mel filterbank.
/// Returns [num_frames, num_mels].
fn compute_fbank(samples: &[f32], config: &MelConfig, sr: f32, f_max: f32) -> Array2<f32> {
    let frame_length = config.n_fft;
    let frame_shift = config.hop_length;
    let pre_emphasis_coeff = config.pre_emphasis.unwrap_or(0.97);

    // Scale samples if model expects unnormalized input
    let samples: Vec<f32> = if !config.normalize_samples {
        samples.iter().map(|&s| s * 32768.0).collect()
    } else {
        samples.to_vec()
    };

    // Number of frames
    let num_frames = if config.snip_edges {
        if samples.len() < frame_length {
            0
        } else {
            1 + (samples.len() - frame_length) / frame_shift
        }
    } else {
        (samples.len() + frame_shift - 1) / frame_shift
    };

    if num_frames == 0 {
        return Array2::zeros((0, config.num_mels));
    }

    let fft_size = frame_length.next_power_of_two();
    let num_fft_bins = fft_size / 2 + 1;

    let window = make_window(config.window, frame_length);
    let mel_banks = mel_filterbank(config.num_mels, fft_size, sr, config.f_min, f_max);

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut features = Array2::zeros((num_frames, config.num_mels));

    for i in 0..num_frames {
        let start = i * frame_shift;

        let mut frame = vec![0.0f32; frame_length];
        let copy_len = frame_length.min(samples.len().saturating_sub(start));
        frame[..copy_len].copy_from_slice(&samples[start..start + copy_len]);

        // Pre-emphasis
        for j in (1..frame_length).rev() {
            frame[j] -= pre_emphasis_coeff * frame[j - 1];
        }
        frame[0] *= 1.0 - pre_emphasis_coeff;

        // Apply window
        for j in 0..frame_length {
            frame[j] *= window[j];
        }

        // FFT
        let mut fft_input: Vec<Complex<f32>> =
            frame.iter().map(|&x| Complex::new(x, 0.0)).collect();
        fft_input.resize(fft_size, Complex::new(0.0, 0.0));
        fft.process(&mut fft_input);

        // Power spectrum
        let power_spectrum: Vec<f32> = fft_input[..num_fft_bins]
            .iter()
            .map(|c| c.norm_sqr())
            .collect();

        // Apply mel filterbank and take log
        for m in 0..config.num_mels {
            let mut energy: f32 = mel_banks
                .row(m)
                .iter()
                .zip(power_spectrum.iter())
                .map(|(&w, &p)| w * p)
                .sum();

            if energy < 1.0e-10 {
                energy = 1.0e-10;
            }
            features[[i, m]] = energy.ln();
        }
    }

    features
}

/// Standard mel spectrogram (GigaAM-style): windowed STFT + mel filterbank + log.
/// Returns [num_frames, num_mels].
fn compute_mel_spectrogram(
    samples: &[f32],
    config: &MelConfig,
    sr: f32,
    f_max: f32,
) -> Array2<f32> {
    let n_fft = config.n_fft;
    let hop_length = config.hop_length;

    if samples.len() < n_fft {
        return Array2::zeros((0, config.num_mels));
    }

    let n_frames = (samples.len() - n_fft) / hop_length + 1;
    let freq_bins = n_fft / 2 + 1;

    let window = make_window(config.window, n_fft);
    let filterbank = mel_filterbank(config.num_mels, n_fft, sr, config.f_min, f_max);

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n_fft);

    // Compute STFT power spectrogram [freq_bins, n_frames]
    let mut power_spec = Array2::<f32>::zeros((freq_bins, n_frames));

    for frame_idx in 0..n_frames {
        let start = frame_idx * hop_length;
        let mut fft_buf: Vec<Complex<f32>> = (0..n_fft)
            .map(|i| Complex::new(samples[start + i] * window[i], 0.0))
            .collect();

        fft.process(&mut fft_buf);

        for (bin, val) in fft_buf.iter().enumerate().take(freq_bins) {
            power_spec[[bin, frame_idx]] = val.norm_sqr();
        }
    }

    // Apply mel filterbank: [num_mels, freq_bins] @ [freq_bins, n_frames] = [num_mels, n_frames]
    let mel = filterbank.dot(&power_spec);

    // Log scaling with clamping, then transpose to [n_frames, num_mels]
    mel.mapv(|v| v.clamp(1e-9, 1e9).ln()).t().to_owned()
}

/// Generate a window function of the given type and length.
fn make_window(window_type: WindowType, length: usize) -> Vec<f32> {
    match window_type {
        WindowType::Hamming => (0..length)
            .map(|i| 0.54 - 0.46 * (2.0 * PI * i as f32 / (length as f32 - 1.0)).cos())
            .collect(),
        WindowType::Hann => (0..length)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / length as f32).cos()))
            .collect(),
    }
}

/// Compute mel filterbank matrix of shape [num_mels, num_fft_bins].
fn mel_filterbank(
    num_mels: usize,
    fft_size: usize,
    sample_rate: f32,
    low_freq: f32,
    high_freq: f32,
) -> Array2<f32> {
    let num_fft_bins = fft_size / 2 + 1;

    let mel_low = hz_to_mel(low_freq);
    let mel_high = hz_to_mel(high_freq);

    let num_points = num_mels + 2;
    let mel_points: Vec<f32> = (0..num_points)
        .map(|i| mel_low + (mel_high - mel_low) * i as f32 / (num_points - 1) as f32)
        .collect();

    let hz_points: Vec<f32> = mel_points.iter().map(|&m| mel_to_hz(m)).collect();

    let bin_points: Vec<f32> = hz_points
        .iter()
        .map(|&f| f * fft_size as f32 / sample_rate)
        .collect();

    let mut banks = Array2::zeros((num_mels, num_fft_bins));

    for m in 0..num_mels {
        let left = bin_points[m];
        let center = bin_points[m + 1];
        let right = bin_points[m + 2];

        for k in 0..num_fft_bins {
            let kf = k as f32;
            if kf > left && kf < center {
                banks[[m, k]] = (kf - left) / (center - left);
            } else if kf >= center && kf < right {
                banks[[m, k]] = (right - kf) / (right - center);
            }
        }
    }

    banks
}

fn hz_to_mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * ((mel / 1127.0).exp() - 1.0)
}
