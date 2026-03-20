use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use log::debug;
use std::path::Path;

/// Load a 16-bit PCM WAV file and return samples as f32 in [-1, 1].
/// Only mono 16-bit PCM files are supported (the format Handy writes).
pub fn load_wav_samples(path: &Path) -> Result<Vec<f32>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.bits_per_sample == 16 && spec.sample_format == hound::SampleFormat::Int,
        "load_wav_samples: expected 16-bit PCM, got {:?}",
        spec
    );
    let samples: Result<Vec<f32>, _> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect();
    Ok(samples?)
}

/// Return the duration of a WAV file in seconds by reading its header only.
/// Does not decode sample data.
pub fn wav_duration_secs(path: &Path) -> Result<f32> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();
    let duration = reader.duration();
    Ok(duration as f32 / spec.sample_rate as f32)
}

/// Save audio samples as a WAV file
pub async fn save_wav_file<P: AsRef<Path>>(file_path: P, samples: &[f32]) -> Result<()> {
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
