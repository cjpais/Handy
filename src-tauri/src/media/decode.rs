use crate::audio_toolkit::audio::FrameResampler;
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
use symphonia::core::units::TimeBase;

const TARGET_SAMPLE_RATE: usize = 16_000;
const RESAMPLE_FRAME_MS: u64 = 30;
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "flac", "ogg"];
const PREPARATION_PROGRESS_STEP: u8 = 5;

#[derive(Debug)]
pub struct CancelledError;

impl std::fmt::Display for CancelledError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Studio job cancelled")
    }
}

impl std::error::Error for CancelledError {}

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
}

struct OpenedMedia {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    time_base: Option<TimeBase>,
    codec_label: Option<String>,
    duration_ms: Option<i64>,
}

pub fn probe(path: &Path) -> Result<MediaMetadata> {
    validate_input_path(path)?;

    let file_size_bytes = path.metadata()?.len();
    let mut opened = open_media(path)?;

    let duration_ms = opened
        .duration_ms
        .take()
        .unwrap_or_else(|| scan_duration_from_packets(&mut opened).unwrap_or(0));

    Ok(MediaMetadata {
        duration_ms,
        file_size_bytes,
        container_format: container_label(path),
        audio_codec: opened.codec_label,
        audio_sample_rate_hz: Some(opened.sample_rate),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn normalize_to_wav(path: &Path, output_path: &Path) -> Result<()> {
    normalize_to_wav_with_progress(path, output_path, |_, _, _| {}, || false)
}

pub fn normalize_to_wav_with_progress<F, C>(
    path: &Path,
    output_path: &Path,
    mut emit_progress: F,
    should_cancel: C,
) -> Result<()>
where
    F: FnMut(&str, &str, Option<u8>),
    C: Fn() -> bool,
{
    validate_input_path(path)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let result = (|| -> Result<()> {
        check_cancelled(&should_cancel)?;
        emit_progress("opening_file", "Opening file", Some(5));
        let mut opened = open_media(path)?;

        check_cancelled(&should_cancel)?;
        emit_progress("decoding_audio", "Decoding audio", Some(10));
        let decoded = decode_all_packets_with_progress(&mut opened, &should_cancel, |progress| {
            emit_progress(
                "decoding_audio",
                &format!("Decoding audio ({progress}%)"),
                Some(progress),
            )
        })?;

        check_cancelled(&should_cancel)?;
        emit_progress("resampling_audio", "Resampling audio", Some(75));
        let samples = resample_to_target_with_progress(&decoded, &should_cancel, |progress| {
            emit_progress(
                "resampling_audio",
                &format!("Resampling audio ({progress}%)"),
                Some(progress),
            )
        })?;

        check_cancelled(&should_cancel)?;
        emit_progress(
            "writing_normalized_audio",
            "Writing normalized audio",
            Some(92),
        );
        save_wav_file_with_progress(output_path, &samples, &should_cancel, |progress| {
            emit_progress(
                "writing_normalized_audio",
                &format!("Writing normalized audio ({progress}%)"),
                Some(progress),
            )
        })?;

        emit_progress(
            "writing_normalized_audio",
            "Writing normalized audio",
            Some(100),
        );
        Ok(())
    })();

    if result.is_err() && output_path.exists() {
        let _ = std::fs::remove_file(output_path);
    }

    result
}

#[cfg_attr(not(test), allow(dead_code))]
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
    if !is_supported_extension(path) {
        bail!(unsupported_extension_message());
    }
    Ok(())
}

pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.is::<CancelledError>()
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
    let time_base = track.codec_params.time_base;

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
        time_base,
        codec_label,
        duration_ms,
    })
}

fn decode_all_packets(opened: &mut OpenedMedia) -> Result<DecodedAudio> {
    decode_all_packets_with_progress(opened, &|| false, |_| {})
}

fn scan_duration_from_packets(opened: &mut OpenedMedia) -> Result<i64> {
    let mut duration_ms = 0i64;

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

        if let Some(packet_duration_ms) =
            packet_duration_ms(opened.time_base, packet.ts(), packet.dur())
        {
            duration_ms = packet_duration_ms.max(duration_ms);
        }
    }

    Ok(duration_ms)
}

fn decode_all_packets_with_progress<F, C>(
    opened: &mut OpenedMedia,
    should_cancel: &C,
    mut emit_progress: F,
) -> Result<DecodedAudio>
where
    F: FnMut(u8),
    C: Fn() -> bool,
{
    let mut mono_samples = Vec::new();
    let mut last_progress = 10u8;

    loop {
        check_cancelled(should_cancel)?;

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

        if let Some(progress) = packet_progress_percent(opened, packet.ts(), packet.dur()) {
            if progress >= last_progress.saturating_add(PREPARATION_PROGRESS_STEP) || progress == 74
            {
                last_progress = progress;
                emit_progress(progress);
            }
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

    emit_progress(74);
    Ok(DecodedAudio {
        samples: mono_samples,
        sample_rate: opened.sample_rate,
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

#[cfg_attr(not(test), allow(dead_code))]
fn resample_to_target(decoded: &DecodedAudio) -> Vec<f32> {
    resample_to_target_with_progress(decoded, &|| false, |_| {})
        .expect("resampling without cancellation should succeed")
}

fn resample_to_target_with_progress<F, C>(
    decoded: &DecodedAudio,
    should_cancel: &C,
    mut emit_progress: F,
) -> Result<Vec<f32>>
where
    F: FnMut(u8),
    C: Fn() -> bool,
{
    if decoded.sample_rate as usize == TARGET_SAMPLE_RATE {
        emit_progress(91);
        return Ok(decoded.samples.clone());
    }

    let mut normalized = Vec::new();
    let mut resampler = FrameResampler::new(
        decoded.sample_rate as usize,
        TARGET_SAMPLE_RATE,
        Duration::from_millis(RESAMPLE_FRAME_MS),
    );
    let total_samples = decoded.samples.len().max(1);
    let progress_chunk = (decoded.sample_rate as usize).max(4096);
    let mut processed = 0usize;
    let mut last_progress = 75u8;

    for chunk in decoded.samples.chunks(progress_chunk) {
        check_cancelled(should_cancel)?;
        resampler.push(chunk, |frame| normalized.extend_from_slice(frame));
        processed += chunk.len();
        let progress =
            scale_progress(processed as f64 / total_samples as f64, 75, 91).clamp(75, 91);
        if progress >= last_progress.saturating_add(PREPARATION_PROGRESS_STEP) || progress == 91 {
            last_progress = progress;
            emit_progress(progress);
        }
    }

    check_cancelled(should_cancel)?;
    resampler.finish(|frame| normalized.extend_from_slice(frame));
    emit_progress(91);
    Ok(normalized)
}

fn save_wav_file_with_progress<F, C>(
    output_path: &Path,
    samples: &[f32],
    should_cancel: &C,
    mut emit_progress: F,
) -> Result<()>
where
    F: FnMut(u8),
    C: Fn() -> bool,
{
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, spec)?;
    let total_samples = samples.len().max(1);

    for (index, sample) in samples.iter().enumerate() {
        if index % 16_384 == 0 {
            check_cancelled(should_cancel)?;
            let progress =
                scale_progress(index as f64 / total_samples as f64, 92, 100).clamp(92, 100);
            emit_progress(progress);
        }

        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
    }

    check_cancelled(should_cancel)?;
    writer.finalize()?;
    emit_progress(100);
    Ok(())
}

fn packet_progress_percent(opened: &OpenedMedia, packet_ts: u64, packet_dur: u64) -> Option<u8> {
    let total_duration_ms = opened.duration_ms?;
    let packet_ms = packet_duration_ms(opened.time_base, packet_ts, packet_dur)?;
    let ratio = (packet_ms as f64 / total_duration_ms as f64).clamp(0.0, 1.0);
    Some(scale_progress(ratio, 10, 74))
}

fn packet_duration_ms(time_base: Option<TimeBase>, packet_ts: u64, packet_dur: u64) -> Option<i64> {
    let time_base = time_base?;
    let packet_end = packet_ts.saturating_add(packet_dur);
    let time = time_base.calc_time(packet_end);
    Some(((time.seconds as f64 + time.frac) * 1000.0).round() as i64)
}

fn scale_progress(ratio: f64, start: u8, end: u8) -> u8 {
    if end <= start {
        return end;
    }

    let clamped = ratio.clamp(0.0, 1.0);
    let range = (end - start) as f64;
    (start as f64 + clamped * range).round() as u8
}

fn check_cancelled<C>(should_cancel: &C) -> Result<()>
where
    C: Fn() -> bool,
{
    if should_cancel() {
        bail!(CancelledError);
    }

    Ok(())
}

fn container_label(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .map(|ext| match ext.as_str() {
            "m4a" => "M4A".to_string(),
            "mp3" => "MP3".to_string(),
            "wav" => "WAV".to_string(),
            "flac" => "FLAC".to_string(),
            "ogg" => "OGG".to_string(),
            other => other.to_ascii_uppercase(),
        })
}

fn is_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()))
}

fn unsupported_extension_message() -> &'static str {
    "Studio currently supports MP3, WAV, M4A, FLAC, and OGG files. Please convert this file before trying again."
}

fn unsupported_format_error() -> anyhow::Error {
    anyhow!(unsupported_extension_message())
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
    use super::{is_cancelled, normalize_to_wav, normalize_to_wav_with_progress, probe};
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

    #[test]
    fn probe_rejects_unsupported_extension_before_decode() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let mp4_path = temp_dir.path().join("example.mp4");
        std::fs::write(&mp4_path, b"not-a-supported-studio-input").expect("temp file");

        let error = probe(&mp4_path).expect_err("mp4 should be rejected by allowlist");
        assert!(error
            .to_string()
            .contains("Studio currently supports MP3, WAV, M4A, FLAC, and OGG files"));
    }

    #[test]
    fn normalize_to_wav_can_cancel_before_decode() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let output_path = temp_dir.path().join("cancelled.wav");

        let error =
            normalize_to_wav_with_progress(&sample_wav_path(), &output_path, |_, _, _| {}, || true)
                .expect_err("cancel should stop normalization");

        assert!(is_cancelled(&error));
        assert!(!output_path.exists());
    }
}
