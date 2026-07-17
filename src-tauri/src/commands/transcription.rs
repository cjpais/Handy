use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings, ModelUnloadTimeout};
use serde::Serialize;
use specta::Type;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Chunk length for the non-streaming fallback path (Parakeet et al.). 30 s
/// keeps the attention matrix small enough to stay off swap while still giving
/// the model enough context per call.
const BATCH_CHUNK_SAMPLES: usize = 30 * 16_000;

#[derive(Serialize, Type)]
pub struct ModelLoadStatus {
    is_loaded: bool,
    current_model: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn set_model_unload_timeout(app: AppHandle, timeout: ModelUnloadTimeout) {
    let mut settings = get_settings(&app);
    settings.model_unload_timeout = timeout;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn get_model_load_status(
    transcription_manager: State<TranscriptionManager>,
) -> Result<ModelLoadStatus, String> {
    Ok(ModelLoadStatus {
        is_loaded: transcription_manager.is_model_loaded(),
        current_model: transcription_manager.get_current_model(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn unload_model_manually(
    transcription_manager: State<TranscriptionManager>,
) -> Result<(), String> {
    transcription_manager
        .unload_model()
        .map_err(|e| format!("Failed to unload model: {}", e))
}

/// Progress payload emitted on the `transcribe-file-progress` event as a file
/// is processed.
#[derive(Clone, Serialize, Type)]
pub struct TranscribeFileProgress {
    pub fed_samples: usize,
    pub total_samples: usize,
    pub fraction: f64,
}

/// Locate an ffmpeg binary. GUI apps launched from Finder have a minimal PATH
/// that usually excludes Homebrew, so probe the common absolute locations first
/// before falling back to a bare `ffmpeg` (which relies on PATH).
fn find_ffmpeg() -> PathBuf {
    for c in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("ffmpeg")
}

/// Decode an arbitrary audio file to 16 kHz mono f32 samples via ffmpeg, matching
/// the format the recorder produces. Returns the decoded PCM.
fn decode_to_samples(file_path: &str) -> Result<Vec<f32>, String> {
    let input = PathBuf::from(file_path);
    if !input.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let ffmpeg = find_ffmpeg();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_wav = std::env::temp_dir().join(format!("handy_transcribe_{}.wav", nanos));

    let output = StdCommand::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&input)
        .args(["-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"])
        .arg(&tmp_wav)
        .output()
        .map_err(|e| {
            format!(
                "Failed to launch ffmpeg ({}). Install it first — e.g. `brew install ffmpeg` on macOS",
                e
            )
        })?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp_wav);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "ffmpeg could not decode the file (unsupported format?): {}",
            stderr.trim()
        ));
    }

    let samples = crate::audio_toolkit::read_wav_samples(&tmp_wav)
        .map_err(|e| format!("Failed to read decoded audio: {}", e));
    let _ = std::fs::remove_file(&tmp_wav);
    let samples = samples?;

    if samples.is_empty() {
        return Err("No audio found in the file".to_string());
    }
    Ok(samples)
}

/// Non-streaming fallback for engines that can't stream (e.g. Parakeet): split
/// the buffer into fixed windows and transcribe each on its own so no single
/// call ever sees the whole file. Prevents the quadratic-attention blowup that
/// made an hour of audio allocate tens of GB.
fn transcribe_chunked(
    tm: &Arc<TranscriptionManager>,
    app: &AppHandle,
    model_id: &str,
    samples: &[f32],
) -> Result<String, String> {
    let total = samples.len();
    let mut out: Vec<String> = Vec::new();
    let mut fed = 0usize;

    for chunk in samples.chunks(BATCH_CHUNK_SAMPLES) {
        // `transcribe` unloads the model right after each call when the unload
        // setting is Immediately — without this reload the run would die at
        // chunk 2. With that setting a long file pays a load per 30s window,
        // which is what the setting asks for; with any other setting this is a
        // cheap already-loaded check.
        tm.ensure_model_loaded(model_id)
            .map_err(|e| format!("Failed to load model '{}': {}", model_id, e))?;
        let text = tm
            .transcribe(chunk.to_vec())
            .map_err(|e| format!("Chunk transcription failed: {}", e))?;
        let text = text.trim();
        if !text.is_empty() {
            out.push(text.to_string());
        }
        fed += chunk.len();
        let _ = app.emit(
            "transcribe-file-progress",
            TranscribeFileProgress {
                fed_samples: fed,
                total_samples: total,
                fraction: fed as f64 / total.max(1) as f64,
            },
        );
    }
    Ok(out.join(" "))
}

fn transcribe_file_inner(app: &AppHandle, file_path: &str) -> Result<String, String> {
    let samples = decode_to_samples(file_path)?;

    let model_id = get_settings(app).selected_model;
    if model_id.is_empty() {
        return Err("No model selected. Pick one in the Models tab.".to_string());
    }

    let tm = app.state::<Arc<TranscriptionManager>>().inner().clone();
    // Serialized with every other load path (mic hotkey, model switch) via the
    // is_loading slot — a bare load here could run concurrently with another
    // loader and hold two multi-GB models at once.
    tm.ensure_model_loaded(&model_id)
        .map_err(|e| format!("Failed to load model '{}': {}", model_id, e))?;

    let app_for_progress = app.clone();
    let total = samples.len();
    let progress = move |fed: usize, total: usize| {
        let _ = app_for_progress.emit(
            "transcribe-file-progress",
            TranscribeFileProgress {
                fed_samples: fed,
                total_samples: total,
                fraction: fed as f64 / total.max(1) as f64,
            },
        );
    };

    // Streaming engines (Nemotron, Whisper, …) keep memory flat regardless of
    // length. If the loaded model can't stream, fall back to chunked batch.
    let result = match tm.transcribe_buffer_streaming(&samples, &progress) {
        Ok(Some(text)) => Ok(text),
        Ok(None) => {
            log::info!(
                "File transcription: model '{}' can't stream, using chunked batch ({} samples)",
                model_id,
                total
            );
            transcribe_chunked(&tm, app, &model_id, &samples)
        }
        Err(e) => Err(format!("Transcription failed: {}", e)),
    };
    // The streaming path never unloads on its own — honor the Immediately
    // unload setting the same way the mic path does after its run.
    tm.maybe_unload_immediately("file transcription");
    result
}

/// Transcribe an audio file (any format ffmpeg can decode: mp3/m4a/wav/...).
/// Returns the recognized text. Heavy work runs on a blocking thread so the UI
/// stays responsive; progress is reported via the `transcribe-file-progress`
/// event.
#[tauri::command]
#[specta::specta]
pub async fn transcribe_audio_file(app: AppHandle, file_path: String) -> Result<String, String> {
    let app_for_blocking = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        transcribe_file_inner(&app_for_blocking, &file_path)
    })
    .await
    .map_err(|e| format!("Internal task error: {}", e))?
}
