use std::path::PathBuf;

pub fn read_wav_samples(wav_path: &PathBuf) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut reader = hound::WavReader::open(wav_path)?;
    let spec = reader.spec();

    let expected_spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    if spec.channels != expected_spec.channels {
        return Err(format!(
            "Expected {} channels, found {}",
            expected_spec.channels, spec.channels
        )
        .into());
    }

    if spec.sample_rate != expected_spec.sample_rate {
        return Err(format!(
            "Expected {} Hz sample rate, found {} Hz",
            expected_spec.sample_rate, spec.sample_rate
        )
        .into());
    }

    if spec.bits_per_sample != expected_spec.bits_per_sample {
        return Err(format!(
            "Expected {} bits per sample, found {}",
            expected_spec.bits_per_sample, spec.bits_per_sample
        )
        .into());
    }

    if spec.sample_format != expected_spec.sample_format {
        return Err(format!("Expected Int sample format, found {:?}", spec.sample_format).into());
    }

    let samples: Result<Vec<f32>, _> = reader
        .samples::<i16>()
        .map(|sample| sample.map(|s| s as f32 / i16::MAX as f32))
        .collect();

    Ok(samples?)
}