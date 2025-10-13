use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use log::debug;
use std::path::Path;

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

/// Load audio samples from a WAV file
pub fn load_wav_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<f32>> {
    let mut reader = WavReader::open(file_path.as_ref())?;
    let spec = reader.spec();
    
    debug!("Loading WAV file: {:?}, spec: {:?}", file_path.as_ref(), spec);
    
    let samples: Result<Vec<f32>, _> = match spec.sample_format {
        hound::SampleFormat::Int => {
            match spec.bits_per_sample {
                16 => {
                    reader.samples::<i16>()
                        .map(|s| s.map(|sample| sample as f32 / i16::MAX as f32))
                        .collect()
                }
                32 => {
                    reader.samples::<i32>()
                        .map(|s| s.map(|sample| sample as f32 / i32::MAX as f32))
                        .collect()
                }
                _ => return Err(anyhow::anyhow!("Unsupported bit depth: {}", spec.bits_per_sample)),
            }
        }
        hound::SampleFormat::Float => {
            reader.samples::<f32>().collect()
        }
    };
    
    let audio_samples = samples?;
    debug!("Loaded {} samples from WAV file", audio_samples.len());
    Ok(audio_samples)
}
