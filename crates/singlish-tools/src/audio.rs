//! WAV loading with downmix and resampling to 16 kHz mono.

use std::path::Path;

pub const TARGET_RATE: u32 = 16_000;

/// Reads any PCM/float WAV and returns 16 kHz mono f32 samples in [-1, 1].
pub fn load_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, _) => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        (hound::SampleFormat::Int, bits) => {
            let scale = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    let mono = downmix(&interleaved, channels);
    Ok(resample(&mono, spec.sample_rate, TARGET_RATE))
}

pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    }
}

/// Windowed-sinc resampler (Hann window). Low-passes at the lower of the two
/// Nyquist rates when downsampling, so there is no aliasing.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64; // input samples per output sample
    let cutoff = (1.0 / ratio).min(1.0); // relative to input Nyquist
    let half_width = 16.0 / cutoff; // taps on each side, in input samples
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let center = n as f64 * ratio;
        let lo = (center - half_width).ceil().max(0.0) as usize;
        let hi = ((center + half_width).floor() as usize).min(input.len() - 1);
        let mut acc = 0.0f64;
        for (i, &x) in input.iter().enumerate().take(hi + 1).skip(lo) {
            let d = i as f64 - center;
            let window = 0.5 * (1.0 + (std::f64::consts::PI * d / half_width).cos());
            acc += x as f64 * cutoff * sinc(d * cutoff) * window;
        }
        out.push(acc as f32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn same_rate_is_identity() {
        let x = sine(440.0, 16_000, 0.1);
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn downsample_length() {
        let x = sine(440.0, 48_000, 1.0);
        assert_eq!(resample(&x, 48_000, 16_000).len(), 16_000);
    }

    #[test]
    fn preserves_in_band_tone() {
        let y = resample(&sine(440.0, 44_100, 1.0), 44_100, 16_000);
        let mid = &y[2_000..14_000];
        assert!((rms(mid) - 0.7071).abs() < 0.02, "rms {}", rms(mid));
    }

    #[test]
    fn removes_out_of_band_tone() {
        // 12 kHz would alias at 16 kHz output; it should be filtered out.
        let y = resample(&sine(12_000.0, 48_000, 1.0), 48_000, 16_000);
        assert!(rms(&y[2_000..14_000]) < 0.05);
    }

    #[test]
    fn upsample_length() {
        assert_eq!(
            resample(&sine(100.0, 8_000, 1.0), 8_000, 16_000).len(),
            16_000
        );
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }
}
