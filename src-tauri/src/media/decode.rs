use crate::audio_toolkit::audio::FrameResampler;
use crate::audio_toolkit::save_wav_file;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use specta::Type;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const TARGET_SAMPLE_RATE: usize = 16_000;
const RESAMPLE_FRAME_MS: u64 = 30;

#[derive(Debug, Clone, Serialize, Type)]
pub struct MediaMetadata {
    pub duration_ms: i64,
    pub file_size_bytes: u64,
    pub container_format: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_sample_rate_hz: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub duration_ms: i64,
}

struct OpenedMedia {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    codec_label: Option<String>,
    duration_ms: Option<i64>,
}

pub fn probe(path: &Path) -> Result<MediaMetadata> {
    validate_input_path(path)?;

    let file_size_bytes = path.metadata()?.len();
    let mut opened = open_media(path)?;

    let duration_ms = opened.duration_ms.take().unwrap_or_else(|| {
        decode_all_packets(&mut opened)
            .map(|decoded| decoded.duration_ms)
            .unwrap_or(0)
    });

    Ok(MediaMetadata {
        duration_ms,
        file_size_bytes,
        container_format: container_label(path),
        audio_codec: opened.codec_label,
        audio_sample_rate_hz: Some(opened.sample_rate),
    })
}

pub fn normalize_to_wav(path: &Path, output_path: &Path) -> Result<()> {
    validate_input_path(path)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let decoded = decode_audio_file(path)?;
    let samples = resample_to_target(&decoded);
    save_wav_file(output_path, &samples)
}

pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio> {
    validate_input_path(path)?;
    let mut opened = open_media(path)?;
    decode_all_packets(&mut opened)
}

fn validate_input_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("The selected file does not exist");
    }
    if !path.is_file() {
        bail!("The selected path is not a file");
    }
    Ok(())
}

fn open_media(path: &Path) -> Result<OpenedMedia> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open input file: {}", path.to_string_lossy()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(map_probe_error)?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(unsupported_format_error)?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("The audio stream is missing a sample rate"))?;
    let codec_type = track.codec_params.codec;

    let duration_ms = match (track.codec_params.time_base, track.codec_params.n_frames) {
        (Some(time_base), Some(frame_count)) => {
            let time = time_base.calc_time(frame_count);
            Some(((time.seconds as f64 + time.frac) * 1000.0).round() as i64)
        }
        _ => None,
    };

    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(map_probe_error)?;

    let codec_label = symphonia::default::get_codecs()
        .get_codec(codec_type)
        .map(|codec| codec.short_name.to_uppercase())
        .or_else(|| Some(format!("{}", codec_type)));

    Ok(OpenedMedia {
        format,
        decoder,
        track_id,
        sample_rate,
        codec_label,
        duration_ms,
    })
}

fn decode_all_packets(opened: &mut OpenedMedia) -> Result<DecodedAudio> {
    let mut mono_samples = Vec::new();

    loop {
        let packet = match opened.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                bail!("This file uses a stream layout that Studio does not support yet");
            }
            Err(error) => return Err(map_decode_error(error)),
        };

        if packet.track_id() != opened.track_id {
            continue;
        }

        let decoded = match opened.decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                bail!("This file uses a stream layout that Studio does not support yet");
            }
            Err(error) => return Err(map_decode_error(error)),
        };

        mono_samples.extend(interleaved_to_mono(decoded));
    }

    let duration_ms =
        ((mono_samples.len() as f64 / opened.sample_rate as f64) * 1000.0).round() as i64;

    Ok(DecodedAudio {
        samples: mono_samples,
        sample_rate: opened.sample_rate,
        duration_ms,
    })
}

fn interleaved_to_mono(buffer: AudioBufferRef<'_>) -> Vec<f32> {
    let spec = *buffer.spec();
    let channel_count = spec.channels.count();
    let mut sample_buffer = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
    sample_buffer.copy_interleaved_ref(buffer);

    if channel_count <= 1 {
        return sample_buffer.samples().to_vec();
    }

    sample_buffer
        .samples()
        .chunks(channel_count)
        .map(|frame| frame.iter().copied().sum::<f32>() / channel_count as f32)
        .collect()
}

fn resample_to_target(decoded: &DecodedAudio) -> Vec<f32> {
    if decoded.sample_rate as usize == TARGET_SAMPLE_RATE {
        return decoded.samples.clone();
    }

    let mut normalized = Vec::new();
    let mut resampler = FrameResampler::new(
        decoded.sample_rate as usize,
        TARGET_SAMPLE_RATE,
        Duration::from_millis(RESAMPLE_FRAME_MS),
    );
    resampler.push(&decoded.samples, |frame| {
        normalized.extend_from_slice(frame)
    });
    resampler.finish(|frame| normalized.extend_from_slice(frame));
    normalized
}

fn container_label(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .map(|ext| match ext.as_str() {
            "m4a" => "M4A".to_string(),
            "mp4" => "MP4".to_string(),
            "mp3" => "MP3".to_string(),
            "wav" => "WAV".to_string(),
            "flac" => "FLAC".to_string(),
            "ogg" => "OGG".to_string(),
            other => other.to_ascii_uppercase(),
        })
}

fn unsupported_format_error() -> anyhow::Error {
    anyhow!(
        "This format is not supported yet. Please convert your file to MP3, WAV, M4A, MP4, FLAC, or OGG first."
    )
}

fn map_probe_error(error: SymphoniaError) -> anyhow::Error {
    match error {
        SymphoniaError::Unsupported(_) => unsupported_format_error(),
        other => anyhow!("Could not read audio metadata: {}", other),
    }
}

fn map_decode_error(error: SymphoniaError) -> anyhow::Error {
    match error {
        SymphoniaError::Unsupported(_) => unsupported_format_error(),
        other => anyhow!("Could not decode the audio stream: {}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_to_wav, probe};
    use crate::audio_toolkit::read_wav_samples;
    use std::path::PathBuf;

    fn sample_wav_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/pop_start.wav")
    }

    #[test]
    fn probe_reads_wav_metadata() {
        let reader = hound::WavReader::open(sample_wav_path()).expect("source wav reader");
        let source_sample_rate = reader.spec().sample_rate;
        let metadata = probe(&sample_wav_path()).expect("probe should read bundled wav");
        assert!(metadata.duration_ms > 0);
        assert!(metadata.file_size_bytes > 0);
        assert_eq!(metadata.container_format.as_deref(), Some("WAV"));
        assert_eq!(metadata.audio_sample_rate_hz, Some(source_sample_rate));
    }

    #[test]
    fn normalize_to_wav_creates_16khz_mono_output() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let output_path = temp_dir.path().join("normalized.wav");

        normalize_to_wav(&sample_wav_path(), &output_path)
            .expect("normalize_to_wav should succeed");

        let samples = read_wav_samples(&output_path).expect("normalized wav should be readable");
        assert!(!samples.is_empty());

        let reader = hound::WavReader::open(&output_path).expect("wav reader");
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
    }
}
