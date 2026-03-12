use anyhow::{bail, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use log::debug;
use std::path::Path;

/// Load audio samples from a WAV file.
pub fn load_wav_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<f32>> {
    let mut reader = WavReader::open(file_path.as_ref())?;
    let spec = reader.spec();

    if spec.channels != 1 {
        bail!(
            "Expected mono WAV file, found {} channels in {:?}",
            spec.channels,
            file_path.as_ref()
        );
    }

    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|sample| sample.map(|value| value as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|sample| sample.map(|value| value as f32 / i32::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        _ => {
            bail!(
                "Unsupported WAV format in {:?}: {:?} {}-bit",
                file_path.as_ref(),
                spec.sample_format,
                spec.bits_per_sample
            );
        }
    };

    Ok(samples)
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
