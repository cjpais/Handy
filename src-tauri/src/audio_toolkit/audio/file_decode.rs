use anyhow::{anyhow, Context, Result};
use std::borrow::Cow;
use std::env;
use std::fs::File;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Instant;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Clone, Debug)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub source_sample_rate: u32,
    pub channels: u16,
    pub duration_sec: f32,
    pub source_bitrate_kbps: Option<u32>,
}

pub fn decode_audio_file<P: AsRef<Path>>(file_path: P) -> Result<DecodedAudio> {
    let file_path = file_path.as_ref();

    match decode_with_symphonia(file_path) {
        Ok(decoded) => Ok(decoded),
        Err(primary_err) => {
            log::warn!(
                "Symphonia decode failed for {}: {:#}. Trying ffmpeg fallback.",
                file_path.display(),
                primary_err
            );
            let ffmpeg_result = decode_with_ffmpeg(file_path);
            if let Err(ref ffmpeg_err) = ffmpeg_result {
                log::error!(
                    "Audio decode failed for {} via both symphonia and ffmpeg. Symphonia error: {:#}; ffmpeg error: {:#}",
                    file_path.display(),
                    primary_err,
                    ffmpeg_err
                );
            }

            ffmpeg_result.with_context(|| {
                format!(
                    "Failed to decode audio file via both symphonia and ffmpeg (symphonia error: {primary_err})"
                )
            })
        }
    }
}

fn decode_with_symphonia(file_path: &Path) -> Result<DecodedAudio> {
    let started = Instant::now();
    log::debug!(
        "symphonia decode start: path={} ext={:?}",
        file_path.display(),
        file_path.extension().and_then(|v| v.to_str())
    );

    let file = File::open(file_path)
        .with_context(|| format!("Failed to open audio file: {}", file_path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("Failed to probe audio format")?;
    let mut format_reader = probed.format;

    let track = format_reader
        .default_track()
        .ok_or_else(|| anyhow!("No default audio track found"))?;
    let codec_params = track.codec_params.clone();
    let track_id = track.id;
    if codec_params.codec == CODEC_TYPE_NULL {
        return Err(anyhow!("Unsupported audio codec"));
    }

    let source_sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Input audio has unknown sample rate"))?;
    let channels = codec_params
        .channels
        .map(|value| value.count() as u16)
        .unwrap_or(1);
    log::debug!(
        "symphonia probe ok: path={} track_id={} codec={:?} source_sample_rate={} channels={}",
        file_path.display(),
        track_id,
        codec_params.codec,
        source_sample_rate,
        channels
    );

    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .context("Failed to create audio decoder")?;

    let mut interleaved = Vec::<f32>::new();
    let mut packet_count: usize = 0;
    let mut decoded_packet_count: usize = 0;
    let mut ignored_decode_errors: usize = 0;

    loop {
        let packet = match format_reader.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(anyhow!("Failed reading audio packets: {err}")),
        };

        if packet.track_id() != track_id {
            continue;
        }
        packet_count += 1;

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(err)) => {
                ignored_decode_errors += 1;
                if ignored_decode_errors <= 3 || ignored_decode_errors % 50 == 0 {
                    log::debug!(
                        "symphonia decode warning: path={} ignored_decode_errors={} last_error={}",
                        file_path.display(),
                        ignored_decode_errors,
                        err
                    );
                }
                continue;
            }
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(anyhow!("Audio decode failed: {err}")),
        };
        decoded_packet_count += 1;

        let mut sample_buffer =
            SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        sample_buffer.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(sample_buffer.samples());

        if decoded_packet_count % 500 == 0 {
            log::debug!(
                "symphonia progress: path={} decoded_packets={} samples_accumulated={}",
                file_path.display(),
                decoded_packet_count,
                interleaved.len()
            );
        }
    }

    if interleaved.is_empty() {
        return Err(anyhow!("Audio file contains no decodable samples"));
    }

    let mono = downmix_to_mono(&interleaved, channels as usize);
    let normalized = if source_sample_rate == TARGET_SAMPLE_RATE {
        mono
    } else {
        linear_resample(&mono, source_sample_rate, TARGET_SAMPLE_RATE)
    };

    let duration_sec = if normalized.is_empty() {
        0.0
    } else {
        normalized.len() as f32 / TARGET_SAMPLE_RATE as f32
    };
    log::debug!(
        "symphonia decode done: path={} elapsed_ms={} packets={} decoded_packets={} ignored_decode_errors={} samples_in={} samples_out={} resampled={} duration_sec={:.3}",
        file_path.display(),
        started.elapsed().as_millis(),
        packet_count,
        decoded_packet_count,
        ignored_decode_errors,
        interleaved.len(),
        normalized.len(),
        source_sample_rate != TARGET_SAMPLE_RATE,
        duration_sec
    );

    let source_bitrate_kbps = if duration_sec > 0.0 {
        std::fs::metadata(file_path)
            .ok()
            .map(|m| (m.len() as f64 * 8.0 / duration_sec as f64 / 1000.0).round() as u32)
    } else {
        None
    };

    Ok(DecodedAudio {
        samples: normalized,
        sample_rate: TARGET_SAMPLE_RATE,
        source_sample_rate,
        channels,
        duration_sec,
        source_bitrate_kbps,
    })
}

fn decode_with_ffmpeg(file_path: &Path) -> Result<DecodedAudio> {
    let started = Instant::now();
    let source_metadata = match probe_source_audio_metadata(file_path) {
        Ok(metadata) => Some(metadata),
        Err(err) => {
            log::debug!(
                "ffmpeg metadata probe failed for {}: {:#}",
                file_path.display(),
                err
            );
            None
        }
    };

    let file_path_str = file_path.to_string_lossy().to_string();
    let command = [
        "-v",
        "error",
        "-nostdin",
        "-i",
        file_path_str.as_str(),
        "-f",
        "f32le",
        "-acodec",
        "pcm_f32le",
        "-ac",
        "1",
        "-ar",
        "16000",
        "pipe:1",
    ];
    log::debug!(
        "ffmpeg decode start: path={} argv=ffmpeg {}",
        file_path.display(),
        command.join(" ")
    );

    let output = Command::new("ffmpeg")
        .args(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                let path_env = env::var("PATH").unwrap_or_else(|_| "<unavailable>".to_string());
                log::error!(
                    "ffmpeg executable is not available in PATH, cannot decode {} (PATH={})",
                    file_path.display(),
                    path_env
                );
                anyhow!(
                    "Failed to run ffmpeg: executable not found in PATH. Please install ffmpeg."
                )
            }
            _ => anyhow!("Failed to run ffmpeg: {err}"),
        })?;
    log::debug!(
        "ffmpeg exit: path={} elapsed_ms={} status={} stdout_bytes={} stderr_bytes={}",
        file_path.display(),
        started.elapsed().as_millis(),
        output.status,
        output.stdout.len(),
        output.stderr.len()
    );

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let preview = preview_for_log(&stderr, 400);
        if !preview.is_empty() {
            log::debug!(
                "ffmpeg stderr preview: path={} preview={}",
                file_path.display(),
                preview
            );
        }
        log_ffmpeg_failure(file_path, output.status, &stderr);
        let message = match stderr.trim() {
            "" => Cow::Borrowed("ffmpeg exited with non-zero status"),
            text => Cow::Owned(text.to_string()),
        };
        return Err(anyhow!("ffmpeg decode failed: {message}"));
    }

    if output.stdout.is_empty() {
        return Err(anyhow!("ffmpeg produced no audio samples"));
    }

    if output.stdout.len() % 4 != 0 {
        return Err(anyhow!(
            "ffmpeg produced malformed f32 stream ({} bytes)",
            output.stdout.len()
        ));
    }

    let mut samples = Vec::with_capacity(output.stdout.len() / 4);
    for chunk in output.stdout.chunks_exact(4) {
        samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    if samples.is_empty() {
        return Err(anyhow!("Decoded audio stream is empty"));
    }

    let duration_sec = samples.len() as f32 / TARGET_SAMPLE_RATE as f32;
    let source_bitrate_kbps = if duration_sec > 0.0 {
        std::fs::metadata(file_path)
            .ok()
            .map(|m| (m.len() as f64 * 8.0 / duration_sec as f64 / 1000.0).round() as u32)
    } else {
        None
    };
    let (source_sample_rate, channels) = match source_metadata {
        Some(metadata) => metadata,
        None => {
            log::debug!(
                "source metadata unavailable for {}, using fallback source_sample_rate={} channels=1",
                file_path.display(),
                TARGET_SAMPLE_RATE
            );
            (TARGET_SAMPLE_RATE, 1)
        }
    };

    log::debug!(
        "ffmpeg decode done: path={} elapsed_ms={} samples_out={} duration_sec={:.3} source_sample_rate={} source_channels={}",
        file_path.display(),
        started.elapsed().as_millis(),
        samples.len(),
        duration_sec,
        source_sample_rate,
        channels
    );

    Ok(DecodedAudio {
        samples,
        sample_rate: TARGET_SAMPLE_RATE,
        source_sample_rate,
        channels,
        duration_sec,
        source_bitrate_kbps,
    })
}

fn probe_source_audio_metadata(file_path: &Path) -> Result<(u32, u16)> {
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open audio file: {}", file_path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(ext);
    }

    let probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("Failed to probe audio format for source metadata")?;
    let format_reader = probed.format;

    let track = format_reader
        .default_track()
        .ok_or_else(|| anyhow!("No default audio track found for source metadata"))?;
    let source_sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Input audio has unknown sample rate"))?;
    let channels = track
        .codec_params
        .channels
        .map(|value| value.count() as u16)
        .unwrap_or(1);

    Ok((source_sample_rate, channels))
}

fn log_ffmpeg_failure(file_path: &Path, status: ExitStatus, stderr: &str) {
    if stderr.is_empty() {
        log::error!(
            "ffmpeg failed to decode {} with status {} and empty stderr",
            file_path.display(),
            status
        );
    } else {
        log::error!(
            "ffmpeg failed to decode {} with status {}: {}",
            file_path.display(),
            status,
            stderr
        );
    }
}

fn preview_for_log(text: &str, max_chars: usize) -> String {
    if text.is_empty() || max_chars == 0 {
        return String::new();
    }

    let mut preview = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            preview.push_str("...");
            break;
        }
        preview.push(ch);
    }
    preview
}

fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    let frame_count = samples.len() / channels;
    let mut mono = Vec::with_capacity(frame_count);

    for frame in samples.chunks_exact(channels) {
        let sum: f32 = frame.iter().copied().sum();
        mono.push(sum / channels as f32);
    }

    mono
}

fn linear_resample(samples: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if samples.is_empty() || in_rate == out_rate {
        return samples.to_vec();
    }

    let ratio = out_rate as f64 / in_rate as f64;
    let output_len = ((samples.len() as f64) * ratio).round() as usize;
    if output_len == 0 {
        return Vec::new();
    }

    let mut output = Vec::with_capacity(output_len);
    let max_index = samples.len().saturating_sub(1);

    for i in 0..output_len {
        let source_pos = i as f64 / ratio;
        let left_index = source_pos.floor() as usize;
        let right_index = (left_index + 1).min(max_index);
        let frac = (source_pos - left_index as f64) as f32;

        let left = samples[left_index];
        let right = samples[right_index];
        output.push(left + (right - left) * frac);
    }

    output
}
