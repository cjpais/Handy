//! Reads the audio of a file brought in for transcription (any common audio
//! format, or the sound track of a video) as 16 kHz mono, the format every
//! recording is transcribed and kept in.

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;

/// Frames the resampler takes per step.
const RESAMPLER_CHUNK_FRAMES: usize = 1024;

/// An open file's first audio track, ready to decode.
struct AudioTrack {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
}

fn open(path: &Path) -> Result<AudioTrack> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Not a supported audio or video file: {}", e))?;
    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("The file has no audio track"))?;
    let track_id = track.id;
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("The file's audio format is not supported: {}", e))?;
    Ok(AudioTrack {
        format,
        decoder,
        track_id,
    })
}

/// Checks that `path` has an audio track Handy can decode, reading only its
/// header.
pub fn probe_audio_file(path: &Path) -> Result<()> {
    open(path).map(|_| ())
}

/// The whole of `path`'s first audio track, downmixed to mono and resampled
/// to 16 kHz.
pub fn decode_audio_file(path: &Path) -> Result<Vec<f32>> {
    let AudioTrack {
        mut format,
        mut decoder,
        track_id,
    } = open(path)?;

    let mut mono: Vec<f32> = Vec::new();
    let mut sample_rate: Option<u32> = None;
    let mut buffer: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // The end of the stream.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(anyhow!("Failed to read the file: {}", e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet loses only its own audio.
            Err(SymphoniaError::DecodeError(e)) => {
                warn!("Skipping an undecodable packet: {}", e);
                continue;
            }
            Err(e) => return Err(anyhow!("Failed to decode the file: {}", e)),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        match sample_rate {
            None => sample_rate = Some(spec.rate),
            Some(rate) if rate != spec.rate => {
                return Err(anyhow!("The file's sample rate changes partway through"));
            }
            Some(_) => {}
        }
        let frames = decoded.capacity() as u64;
        if buffer
            .as_ref()
            .is_none_or(|b| b.capacity() < decoded.capacity() * channels)
        {
            buffer = Some(SampleBuffer::new(frames, spec));
        }
        let buffer = buffer.as_mut().expect("buffer was just sized");
        buffer.copy_interleaved_ref(decoded);
        mono.extend(
            buffer
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }

    let rate = sample_rate.ok_or_else(|| anyhow!("The file has no audio"))?;
    debug!(
        "Decoded {:.1}s of audio at {} Hz from {}",
        mono.len() as f64 / rate as f64,
        rate,
        path.display()
    );
    resample_to_16k(mono, rate as usize)
}

fn resample_to_16k(mono: Vec<f32>, rate: usize) -> Result<Vec<f32>> {
    let target = WHISPER_SAMPLE_RATE as usize;
    if rate == target || mono.is_empty() {
        return Ok(mono);
    }
    let mut resampler = Fft::<f32>::new(rate, target, RESAMPLER_CHUNK_FRAMES, 1, FixedSync::Input)
        .map_err(|e| anyhow!("Failed to set up resampling from {} Hz: {}", rate, e))?;
    let frames = mono.len();
    let input = InterleavedSlice::new(&mono[..], 1, frames)
        .map_err(|e| anyhow!("Failed to resample: {}", e))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|e| anyhow!("Failed to resample: {}", e))?;
    Ok(output.take_data())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};

    fn write_wav(path: &Path, rate: u32, channels: u16, seconds: f32) {
        let spec = WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        let frames = (rate as f32 * seconds) as usize;
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let value = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
            for _ in 0..channels {
                writer.write_sample(value).unwrap();
            }
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn stereo_44k_file_becomes_16k_mono() {
        let dir = std::env::temp_dir().join(format!("handy-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stereo.wav");
        write_wav(&path, 44_100, 2, 2.0);

        probe_audio_file(&path).unwrap();
        let samples = decode_audio_file(&path).unwrap();

        assert_eq!(samples.len(), 32_000);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((0.4..0.6).contains(&peak), "peak {}", peak);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_that_is_not_audio_fails_the_probe() {
        let dir = std::env::temp_dir().join(format!("handy-decode-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, "not audio at all").unwrap();

        assert!(probe_audio_file(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
