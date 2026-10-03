use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use log::debug;
use std::path::Path;

/// Peak amplitude at or below which captured input is treated as silent.
///
/// CPAL converts every supported device format to full-scale-normalized `f32`
/// samples before this check, so -60 dBFS is an amplitude ratio of `0.001`
/// (`10^(-60/20)`), not a raw integer sample value. Using peak level keeps the
/// check conservative: any quiet but usable excursion above the threshold is
/// still sent to the transcription engine.
pub(crate) const SILENT_INPUT_PEAK: f32 = 0.001;

/// Return whether a normalized PCM buffer is empty or effectively silent.
pub(crate) fn is_effectively_silent(samples: &[f32]) -> bool {
    samples
        .iter()
        .all(|sample| sample.abs() <= SILENT_INPUT_PEAK)
}

/// Read a WAV file and return normalised f32 samples.
pub fn read_wav_samples<P: AsRef<Path>>(file_path: P) -> Result<Vec<f32>> {
    let reader = WavReader::open(file_path.as_ref())?;
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<f32>, _>>()?;
    Ok(samples)
}

/// Verify a WAV file by reading it back and checking the sample count.
pub fn verify_wav_file<P: AsRef<Path>>(file_path: P, expected_samples: usize) -> Result<()> {
    let reader = WavReader::open(file_path.as_ref())?;
    let actual_samples = reader.len() as usize;
    if actual_samples != expected_samples {
        anyhow::bail!(
            "WAV sample count mismatch: expected {}, got {}",
            expected_samples,
            actual_samples
        );
    }
    Ok(())
}

/// Save audio samples as a WAV file
pub fn save_wav_file<P: AsRef<Path>>(file_path: P, samples: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = WavWriter::create(file_path.as_ref(), spec)?;

    // Convert f32 samples to i16 for WAV
    for sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
    }

    writer.finalize()?;
    debug!("Saved WAV file: {:?}", file_path.as_ref());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{is_effectively_silent, SILENT_INPUT_PEAK};
    use cpal::{FromSample, Sample};

    fn normalize<T>(samples: &[T]) -> Vec<f32>
    where
        T: Sample + Copy,
        f32: FromSample<T>,
    {
        samples
            .iter()
            .map(|sample| sample.to_sample::<f32>())
            .collect()
    }

    #[test]
    fn empty_input_is_silent() {
        assert!(is_effectively_silent(&[]));
    }

    #[test]
    fn zero_samples_are_silent() {
        assert!(is_effectively_silent(&[0.0; 160]));
    }

    #[test]
    fn near_zero_samples_are_silent() {
        assert!(is_effectively_silent(&[
            SILENT_INPUT_PEAK / 2.0,
            -SILENT_INPUT_PEAK,
        ]));
    }

    #[test]
    fn valid_quiet_input_is_not_silent() {
        assert!(!is_effectively_silent(&[0.0, SILENT_INPUT_PEAK * 1.1,]));
    }

    #[test]
    fn normal_input_is_not_silent() {
        assert!(!is_effectively_silent(&[0.0, -0.1, 0.25]));
    }

    #[test]
    fn integer_capture_formats_use_normalized_levels() {
        assert!(is_effectively_silent(&normalize(&[128_u8])));
        assert!(is_effectively_silent(&normalize(&[0_i8])));
        assert!(is_effectively_silent(&normalize(&[0_i16])));
        assert!(is_effectively_silent(&normalize(&[0_i32])));
        assert!(is_effectively_silent(&normalize(&[1_i16, -1_i16])));
        assert!(!is_effectively_silent(&normalize(&[1_i8])));
        assert!(!is_effectively_silent(&normalize(&[64_i16])));
    }

    #[test]
    fn float_capture_format_uses_normalized_levels() {
        assert!(is_effectively_silent(&normalize(&[0.0005_f32])));
        assert!(!is_effectively_silent(&normalize(&[0.002_f32])));
    }
}
