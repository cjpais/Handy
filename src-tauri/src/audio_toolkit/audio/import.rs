use anyhow::{anyhow, Result};
use rubato::{FftFixedIn, Resampler};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Target sample rate the ASR pipeline expects.
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Resampler chunk size, mirroring [`FrameResampler`](super::FrameResampler).
const RESAMPLER_CHUNK_SIZE: usize = 1024;

/// Decode an arbitrary audio file (WAV / MP3 / M4A-AAC), mix it down to mono,
/// and resample it to 16 kHz so it can be fed straight into the transcription
/// pipeline. Returns normalised f32 samples in the same shape as
/// [`read_wav_samples`](super::read_wav_samples).
pub fn decode_audio_file_to_16k_mono<P: AsRef<Path>>(path: P) -> Result<Vec<f32>> {
    let (samples, sample_rate) = decode_to_mono(path.as_ref())?;
    resample_to_16k(samples, sample_rate)
}

/// Decode a media file to interleaved-then-averaged mono f32 samples, returning
/// the samples alongside their native sample rate.
fn decode_to_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("Audio file has no decodable track"))?
        .clone();
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Audio file is missing sample rate information"))?;

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // Clean end-of-stream: symphonia surfaces EOF as an IoError.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let channels = spec.channels.count().max(1);

                let buf = sample_buf.get_or_insert_with(|| {
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)
                });
                buf.copy_interleaved_ref(decoded);

                for frame in buf.samples().chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    samples.push(mono);
                }
            }
            // Decode errors on individual packets are recoverable; skip them.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    Ok((samples, sample_rate))
}

/// Resample mono f32 samples to 16 kHz. A no-op when already at the target rate.
fn resample_to_16k(samples: Vec<f32>, in_hz: u32) -> Result<Vec<f32>> {
    if in_hz == TARGET_SAMPLE_RATE || samples.is_empty() {
        return Ok(samples);
    }

    let mut resampler = FftFixedIn::<f32>::new(
        in_hz as usize,
        TARGET_SAMPLE_RATE as usize,
        RESAMPLER_CHUNK_SIZE,
        1,
        1,
    )?;

    let mut out = Vec::with_capacity(
        samples.len() * TARGET_SAMPLE_RATE as usize / in_hz as usize + RESAMPLER_CHUNK_SIZE,
    );

    let mut pos = 0;
    while pos + RESAMPLER_CHUNK_SIZE <= samples.len() {
        let processed = resampler.process(&[&samples[pos..pos + RESAMPLER_CHUNK_SIZE]], None)?;
        out.extend_from_slice(&processed[0]);
        pos += RESAMPLER_CHUNK_SIZE;
    }

    // Pad the trailing partial chunk with silence so the tail isn't dropped.
    if pos < samples.len() {
        let mut last = samples[pos..].to_vec();
        last.resize(RESAMPLER_CHUNK_SIZE, 0.0);
        let processed = resampler.process(&[&last], None)?;
        out.extend_from_slice(&processed[0]);
    }

    Ok(out)
}
