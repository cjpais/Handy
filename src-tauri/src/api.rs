use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE;
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;

struct ApiState {
    transcription_manager: Arc<TranscriptionManager>,
    #[allow(dead_code)]
    model_manager: Arc<ModelManager>,
}

#[derive(Serialize)]
struct TranscribeResponse {
    text: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

fn error_response(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        status,
        Json(ErrorResponse {
            error: msg.into(),
        }),
    )
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn transcribe(
    State(state): State<Arc<ApiState>>,
    mut multipart: Multipart,
) -> Result<Json<TranscribeResponse>, impl IntoResponse> {
    // Extract audio file from multipart
    let mut audio_bytes: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "audio" {
            match field.bytes().await {
                Ok(bytes) => {
                    audio_bytes = Some(bytes.to_vec());
                }
                Err(e) => {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read file field: {}", e),
                    ));
                }
            }
        }
    }

    let audio_bytes = match audio_bytes {
        Some(bytes) => bytes,
        None => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "No audio file provided. Send a multipart field named 'file' or 'audio'.",
            ));
        }
    };

    if audio_bytes.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Audio file is empty",
        ));
    }

    debug!("Received audio file: {} bytes", audio_bytes.len());

    // Decode audio to f32 samples at 16kHz mono
    let samples = match decode_audio(&audio_bytes) {
        Ok(s) => s,
        Err(e) => {
            // Try ffmpeg as fallback (handles OGG Opus from Telegram, etc.)
            debug!("Symphonia decode failed ({}), trying ffmpeg fallback", e);
            match decode_with_ffmpeg(&audio_bytes) {
                Ok(s) => s,
                Err(ff_err) => {
                    return Err(error_response(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!(
                            "Failed to decode audio. Symphonia: {}. ffmpeg: {}",
                            e, ff_err
                        ),
                    ));
                }
            }
        }
    };

    if samples.is_empty() {
        return Err(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Decoded audio contains no samples",
        ));
    }

    debug!("Decoded {} samples at 16kHz", samples.len());

    // Ensure model is loaded, then transcribe
    // transcribe() is blocking (holds mutex), so use spawn_blocking
    let tm = state.transcription_manager.clone();
    let result = tokio::task::spawn_blocking(move || {
        tm.initiate_model_load();
        tm.transcribe(samples)
    })
    .await;

    match result {
        Ok(Ok(text)) => {
            info!("API transcription result: {}", text);
            Ok(Json(TranscribeResponse { text }))
        }
        Ok(Err(e)) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transcription failed: {}", e),
        )),
        Err(e) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Transcription task panicked: {}", e),
        )),
    }
}

/// Decode audio bytes using symphonia (supports WAV, MP3, FLAC, OGG Vorbis, AAC).
/// Returns mono f32 samples resampled to 16kHz.
fn decode_audio(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let hint = Hint::new();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("Failed to probe audio format: {}", e))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "No audio track found".to_string())?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Unknown sample rate".to_string())?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1);

    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => {
                // Some formats require a reset after seeking
                break;
            }
            Err(e) => return Err(format!("Error reading packet: {}", e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let num_frames = decoded.capacity();
                if num_frames == 0 {
                    continue;
                }
                let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
                sample_buf.copy_interleaved_ref(decoded);
                let samples = sample_buf.samples();

                // Convert to mono by averaging channels
                if channels <= 1 {
                    all_samples.extend_from_slice(samples);
                } else {
                    for chunk in samples.chunks(channels) {
                        let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
                        all_samples.push(mono);
                    }
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(e)) => {
                warn!("Decode error on packet (skipping): {}", e);
                continue;
            }
            Err(e) => return Err(format!("Fatal decode error: {}", e)),
        }
    }

    if all_samples.is_empty() {
        return Err("No audio samples decoded".to_string());
    }

    // Resample to 16kHz if needed
    if sample_rate != WHISPER_SAMPLE_RATE {
        debug!(
            "Resampling from {}Hz to {}Hz ({} samples)",
            sample_rate,
            WHISPER_SAMPLE_RATE,
            all_samples.len()
        );
        resample(&all_samples, sample_rate as usize, WHISPER_SAMPLE_RATE as usize)
    } else {
        Ok(all_samples)
    }
}

/// Decode audio using ffmpeg as a subprocess.
/// This handles formats that symphonia doesn't support (e.g., OGG Opus from Telegram).
/// Outputs mono f32 samples at 16kHz.
fn decode_with_ffmpeg(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            "pipe:0",
            "-f",
            "f32le",
            "-ar",
            &WHISPER_SAMPLE_RATE.to_string(),
            "-ac",
            "1",
            "-loglevel",
            "error",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "ffmpeg not found or failed to start: {}. Install ffmpeg for OGG/Opus support.",
                e
            )
        })?;

    // Write input bytes to ffmpeg's stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(bytes)
            .map_err(|e| format!("Failed to write to ffmpeg stdin: {}", e))?;
        // stdin is dropped here, closing the pipe
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for ffmpeg: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg exited with error: {}", stderr));
    }

    if output.stdout.is_empty() {
        return Err("ffmpeg produced no output".to_string());
    }

    // Convert raw f32le bytes to Vec<f32>
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();

    debug!("ffmpeg decoded {} samples at 16kHz", samples.len());
    Ok(samples)
}

/// Resample audio using rubato FFT resampler.
fn resample(samples: &[f32], from_hz: usize, to_hz: usize) -> Result<Vec<f32>, String> {
    use rubato::{FftFixedIn, Resampler};

    if from_hz == to_hz {
        return Ok(samples.to_vec());
    }

    let chunk_size = 1024;
    let mut resampler = FftFixedIn::<f32>::new(from_hz, to_hz, chunk_size, 1, 1)
        .map_err(|e| format!("Failed to create resampler: {}", e))?;

    let mut output = Vec::with_capacity(samples.len() * to_hz / from_hz + chunk_size);

    for chunk in samples.chunks(chunk_size) {
        let input = if chunk.len() < chunk_size {
            let mut padded = chunk.to_vec();
            padded.resize(chunk_size, 0.0);
            padded
        } else {
            chunk.to_vec()
        };

        match resampler.process(&[&input], None) {
            Ok(result) => {
                if !result.is_empty() {
                    output.extend_from_slice(&result[0]);
                }
            }
            Err(e) => {
                warn!("Resampler error on chunk (skipping): {}", e);
            }
        }
    }

    Ok(output)
}

/// Start the REST API server on the given port.
/// The server binds to 127.0.0.1 only (localhost).
pub fn start_api_server(
    transcription_manager: Arc<TranscriptionManager>,
    model_manager: Arc<ModelManager>,
    port: u16,
) {
    let state = Arc::new(ApiState {
        transcription_manager,
        model_manager,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/transcribe", post(transcribe))
        .with_state(state);

    tauri::async_runtime::spawn(async move {
        let addr = format!("127.0.0.1:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                info!("Transcription API server listening on http://{}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    error!("API server error: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to bind API server to {}: {}", addr, e);
            }
        }
    });
}
