use anyhow::{Context, Result};
use log::debug;
use rodio::Source;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;

use super::FrameResampler;

/// Decode an audio file (MP3, WAV, FLAC, etc.) to mono PCM samples at 16kHz
///
/// This function:
/// - Decodes various audio formats using rodio
/// - Converts stereo/multi-channel to mono by averaging
/// - Resamples to WHISPER_SAMPLE_RATE (16kHz) if needed
///
/// # Arguments
/// * `path` - Path to the audio file to decode
///
/// # Returns
/// * `Result<Vec<f32>>` - Mono PCM samples at 16kHz, normalized to [-1.0, 1.0]
pub async fn decode_audio_file<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let path = path.as_ref();
    debug!("Decoding audio file: {:?}", path);

    // Open the file
    let file = File::open(path)
        .with_context(|| format!("Failed to open audio file: {:?}", path))?;
    let buf_reader = BufReader::new(file);

    // Decode using rodio
    let source = rodio::Decoder::new(buf_reader)
        .with_context(|| format!("Failed to decode audio file: {:?}", path))?;

    let source_sample_rate = source.sample_rate();
    let source_channels = source.channels();

    debug!(
        "Source audio: {} Hz, {} channel(s)",
        source_sample_rate, source_channels
    );

    // Collect all samples from the decoder (rodio returns f32 samples)
    let samples: Vec<f32> = source.collect();

    debug!("Decoded {} samples", samples.len());

    // Average channels to mono if needed
    let mono_samples = convert_to_mono(&samples, source_channels as usize);

    debug!("Converted to {} mono samples", mono_samples.len());

    // Resample to WHISPER_SAMPLE_RATE if needed
    let resampled = if source_sample_rate != WHISPER_SAMPLE_RATE {
        debug!(
            "Resampling from {} Hz to {} Hz",
            source_sample_rate, WHISPER_SAMPLE_RATE
        );
        resample_to_whisper_rate(&mono_samples, source_sample_rate)?
    } else {
        mono_samples
    };

    debug!(
        "Final audio: {} samples at {} Hz",
        resampled.len(),
        WHISPER_SAMPLE_RATE
    );

    Ok(resampled)
}

/// Convert interleaved f32 samples to mono by averaging channels
fn convert_to_mono(samples: &[f32], num_channels: usize) -> Vec<f32> {
    if num_channels == 1 {
        // Already mono, just return a copy
        samples.to_vec()
    } else {
        // Average channels to create mono
        let num_frames = samples.len() / num_channels;
        let mut mono = Vec::with_capacity(num_frames);

        for frame_idx in 0..num_frames {
            let mut sum = 0.0f32;
            for ch in 0..num_channels {
                sum += samples[frame_idx * num_channels + ch];
            }
            mono.push(sum / num_channels as f32);
        }

        mono
    }
}

/// Resample audio to WHISPER_SAMPLE_RATE using FrameResampler
fn resample_to_whisper_rate(samples: &[f32], source_sample_rate: u32) -> Result<Vec<f32>> {
    let mut resampler = FrameResampler::new(
        source_sample_rate as usize,
        WHISPER_SAMPLE_RATE as usize,
        std::time::Duration::from_millis(20), // 20ms frame duration
    );

    let mut output = Vec::new();

    // Push all samples through the resampler
    resampler.push(samples, |frame| {
        output.extend_from_slice(frame);
    });

    // Finish resampling (process any remaining samples)
    resampler.finish(|frame| {
        output.extend_from_slice(frame);
    });

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_mono() {
        let samples = vec![0.0f32, 0.5f32, 1.0f32, -1.0f32];
        let result = convert_to_mono(&samples, 1);

        assert_eq!(result.len(), 4);
        assert_eq!(result, samples);
    }

    #[test]
    fn test_convert_stereo_to_mono() {
        // Stereo samples: [L1, R1, L2, R2]
        let samples = vec![0.1f32, 0.3f32, 0.5f32, 0.7f32];
        let result = convert_to_mono(&samples, 2);

        assert_eq!(result.len(), 2);
        // First frame average: (0.1 + 0.3) / 2 = 0.2
        // Second frame average: (0.5 + 0.7) / 2 = 0.6
        assert!((result[0] - 0.2).abs() < 0.001);
        assert!((result[1] - 0.6).abs() < 0.001);
    }
}
