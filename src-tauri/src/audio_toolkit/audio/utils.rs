use anyhow::{bail, Result};
use hound::{WavReader, WavSpec, WavWriter};
use log::{debug, warn};
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::FrameResampler;

/// Sample rate every transcription engine expects.
const TARGET_SAMPLE_RATE: usize = 16_000;

/// Frame size handed to [`FrameResampler`]; matches the recording pipeline.
const RESAMPLE_FRAME: Duration = Duration::from_millis(30);

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

/// Decode any container/codec symphonia is built with (wav, mp3, flac, ogg,
/// m4a/aac) into the 16 kHz mono f32 buffer
/// [`crate::managers::transcription::TranscriptionManager::transcribe`] expects.
///
/// Unlike [`read_wav_samples`] — which is correct only for Handy's own 16 kHz
/// mono recordings — this handles arbitrary user-supplied audio, downmixing to
/// mono and resampling to 16 kHz as it goes.
///
/// Decoding streams through the resampler rather than collecting the source
/// first: an hour of 44.1 kHz stereo is over a gigabyte as interleaved f32.
pub fn decode_audio_file<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let path = path.as_ref();

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // The extension is only a hint — symphonia still probes the actual bytes,
    // so a mislabelled file decodes as whatever it really is.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions {
            enable_gapless: true,
            ..Default::default()
        },
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    // Clone out what we need before the packet loop: `next_packet` needs
    // `&mut format`, which conflicts with the borrow `default_track` hands back.
    let (track_id, codec_params) = {
        let track = format
            .default_track()
            .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            })
            .ok_or_else(|| anyhow::anyhow!("No decodable audio track in {}", path.display()))?;
        (track.id, track.codec_params.clone())
    };

    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let mut resampler: Option<FrameResampler> = None;
    let mut source_rate: u32 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut buf_frames: u64 = 0;
    let mut buf_channels: usize = 0;
    let mut mono: Vec<f32> = Vec::new();
    let mut out: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Both mean "the stream is over as far as we're concerned": a reset
            // signals a new stream we have no decoder for, EOF is the normal end.
            Err(SymphoniaError::ResetRequired) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(e.into()),
        };

        // Containers can interleave several tracks; ignore everything but ours.
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A corrupt packet is recoverable — drop it and keep going rather
            // than losing the rest of the file.
            Err(SymphoniaError::DecodeError(e)) => {
                warn!("Skipping undecodable packet in {}: {}", path.display(), e);
                continue;
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(e.into()),
        };

        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let capacity = decoded.capacity() as u64;

        if resampler.is_none() {
            source_rate = spec.rate;
            resampler = Some(FrameResampler::new(
                spec.rate as usize,
                TARGET_SAMPLE_RATE,
                RESAMPLE_FRAME,
            ));
        } else if spec.rate != source_rate {
            warn!(
                "Sample rate changed mid-stream in {} ({} Hz -> {} Hz); continuing at {} Hz",
                path.display(),
                source_rate,
                spec.rate,
                source_rate
            );
        }

        // `copy_interleaved_ref` panics if the destination is too small, so grow
        // it whenever a packet is longer or wider than the last one.
        if sample_buf.is_none() || capacity > buf_frames || channels != buf_channels {
            sample_buf = Some(SampleBuffer::<f32>::new(capacity, spec));
            buf_frames = capacity;
            buf_channels = channels;
        }
        let buf = sample_buf
            .as_mut()
            .expect("sample buffer allocated immediately above");
        buf.copy_interleaved_ref(decoded);

        // Downmix by averaging across channels, matching the recorder
        // (`recorder.rs`) so file and mic audio reach the model the same way.
        let interleaved = buf.samples();
        mono.clear();
        if channels == 1 {
            mono.extend_from_slice(interleaved);
        } else {
            mono.reserve(interleaved.len() / channels);
            for frame in interleaved.chunks_exact(channels) {
                mono.push(frame.iter().sum::<f32>() / channels as f32);
            }
        }

        if let Some(resampler) = resampler.as_mut() {
            resampler.push(&mono, |frame| out.extend_from_slice(frame));
        }
    }

    if let Some(resampler) = resampler.as_mut() {
        resampler.finish(|frame| out.extend_from_slice(frame));
    }

    if out.is_empty() {
        bail!("No audio could be decoded from {}", path.display());
    }

    debug!(
        "Decoded {} into {} samples ({:.1}s at {} Hz)",
        path.display(),
        out.len(),
        out.len() as f64 / TARGET_SAMPLE_RATE as f64,
        TARGET_SAMPLE_RATE
    );

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a stereo 16-bit WAV holding the same tone in both channels, so a
    /// correct downmix (average) preserves the amplitude while an incorrect one
    /// (sum) doubles it.
    fn write_stereo_tone(path: &Path, sample_rate: u32, secs: f64, amplitude: f32) {
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        let frames = (sample_rate as f64 * secs) as usize;
        for i in 0..frames {
            let t = i as f64 / sample_rate as f64;
            let value = (amplitude as f64 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32;
            let encoded = (value * i16::MAX as f32) as i16;
            writer.write_sample(encoded).unwrap();
            writer.write_sample(encoded).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn decodes_stereo_44k_wav_to_mono_16k() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        let secs = 1.0;
        write_stereo_tone(&path, 44_100, secs, 0.5);

        let samples = decode_audio_file(&path).unwrap();

        // Mono at 16 kHz: one sample per output frame, not two.
        let expected = (secs * TARGET_SAMPLE_RATE as f64) as usize;
        let drift = (samples.len() as f64 - expected as f64).abs() / expected as f64;
        assert!(
            drift < 0.05,
            "expected ~{} samples at 16 kHz mono, got {}",
            expected,
            samples.len()
        );

        // Skip the leading/trailing frames: the FFT resampler ramps in and
        // `finish()` zero-pads the tail.
        let body = &samples[480..samples.len() - 480];
        let peak = body.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            (0.4..0.6).contains(&peak),
            "downmix should average channels (peak ~0.5), got peak {}",
            peak
        );
    }

    /// 16 kHz mono needs neither downmix nor resampling; it must still survive
    /// the decode path intact, since that is what Handy's own recordings are.
    #[test]
    fn decodes_mono_16k_wav_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono.wav");

        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for i in 0..16_000 {
            let t = i as f64 / 16_000.0;
            let value = 0.25 * (2.0 * std::f64::consts::PI * 440.0 * t).sin();
            writer
                .write_sample((value as f32 * i16::MAX as f32) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();

        let samples = decode_audio_file(&path).unwrap();

        let drift = (samples.len() as f64 - 16_000.0).abs() / 16_000.0;
        assert!(
            drift < 0.05,
            "expected ~16000 samples, got {}",
            samples.len()
        );

        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            (0.2..0.3).contains(&peak),
            "mono passthrough should preserve amplitude (~0.25), got {}",
            peak
        );
    }

    #[test]
    fn rejects_a_file_that_is_not_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-audio.wav");
        std::fs::write(&path, b"this is definitely not a wav file").unwrap();

        assert!(decode_audio_file(&path).is_err());
    }
}
