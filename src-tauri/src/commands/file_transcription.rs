use crate::audio_toolkit::decode_audio_file;
use crate::file_transcription_window::{
    emit_file_transcription_progress, emit_file_transcription_progress_with_audio_meta,
    FileTranscriptionAudioMeta,
};
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, State};

const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "mp3", "m4a", "opus"];

#[tauri::command]
#[specta::specta]
pub async fn transcribe_audio_file(
    app: AppHandle,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    history_manager: State<'_, Arc<HistoryManager>>,
    path: String,
) -> Result<FileTranscriptionResult, String> {
    let file_path = PathBuf::from(path.clone());
    if let Err(msg) = validate_input_path(&file_path) {
        emit_file_transcription_progress(&app, "failed", &msg, 100, true);
        return Err(msg);
    }

    emit_file_transcription_progress(&app, "starting", "Preparing transcription...", 5, false);

    if !transcription_manager.is_model_loaded() {
        let selected_model = get_settings(&app).selected_model;
        let model_label = selected_model.clone();
        let manager = Arc::clone(transcription_manager.inner());
        emit_file_transcription_progress(&app, "loading_model", "Loading model...", 15, false);
        tauri::async_runtime::spawn_blocking(move || manager.load_model(&selected_model))
            .await
            .map_err(|err| {
                let msg = format!("Model loading task failed: {err}");
                emit_file_transcription_progress(&app, "failed", &msg, 100, true);
                msg
            })?
            .map_err(|err| {
                let msg = format!("Failed to load model '{model_label}': {err}");
                emit_file_transcription_progress(&app, "failed", &msg, 100, true);
                msg
            })?;
    }

    let decode_path = file_path.clone();
    emit_file_transcription_progress(&app, "decoding", "Decoding audio file...", 35, false);
    let decoded = tauri::async_runtime::spawn_blocking(move || decode_audio_file(&decode_path))
        .await
        .map_err(|err| {
            let msg = format!("Audio decoding task failed: {err}");
            emit_file_transcription_progress(&app, "failed", &msg, 100, true);
            msg
        })?
        .map_err(|err| {
            let msg = format!("Failed to decode audio file: {err}");
            emit_file_transcription_progress(&app, "failed", &msg, 100, true);
            msg
        })?;

    let duration_sec = decoded.duration_sec;
    let sample_rate = decoded.sample_rate;
    let source_sample_rate = decoded.source_sample_rate;
    let channels = decoded.channels;
    let source_bitrate_kbps = decoded.source_bitrate_kbps;
    let audio_samples = decoded.samples;
    let audio_for_transcription = audio_samples.clone();
    let manager = Arc::clone(transcription_manager.inner());
    emit_file_transcription_progress_with_audio_meta(
        &app,
        "transcribing",
        "Running speech recognition...",
        65,
        false,
        FileTranscriptionAudioMeta {
            source_sample_rate,
            sample_rate,
            channels,
            duration_sec,
            source_bitrate_kbps,
        },
    );
    let transcription =
        tauri::async_runtime::spawn_blocking(move || manager.transcribe(audio_for_transcription))
            .await
            .map_err(|err| {
                let msg = format!("Transcription task failed: {err}");
                emit_file_transcription_progress(&app, "failed", &msg, 100, true);
                msg
            })?
            .map_err(|err| {
                let msg = format!("Transcription failed: {err}");
                emit_file_transcription_progress(&app, "failed", &msg, 100, true);
                msg
            })?;

    emit_file_transcription_progress(&app, "saving", "Saving history entry...", 85, false);
    history_manager
        .save_transcription(audio_samples, transcription.clone(), None, None)
        .await
        .map_err(|err| {
            let msg = format!("Failed to save history entry: {err}");
            emit_file_transcription_progress(&app, "failed", &msg, 100, true);
            msg
        })?;

    emit_file_transcription_progress(&app, "finalizing", "Finalizing result...", 90, false);

    let model_id = transcription_manager
        .get_current_model()
        .unwrap_or_else(|| "unknown".to_string());

    let result = FileTranscriptionResult {
        source_path: file_path.to_string_lossy().to_string(),
        model_id,
        language: None,
        text: transcription,
        duration_sec,
        sample_rate,
        source_sample_rate,
        channels,
        created_at_unix: Utc::now().timestamp(),
    };
    emit_file_transcription_progress(&app, "completed", "Transcription complete.", 100, true);

    Ok(result)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FileTranscriptionResult {
    pub source_path: String,
    pub model_id: String,
    pub language: Option<String>,
    pub text: String,
    pub duration_sec: f32,
    pub sample_rate: u32,
    pub source_sample_rate: u32,
    pub channels: u16,
    pub created_at_unix: i64,
}

fn validate_input_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| "Input file has no extension".to_string())?;

    if SUPPORTED_EXTENSIONS
        .iter()
        .all(|supported| *supported != extension)
    {
        return Err(format!(
            "Unsupported format: .{}. Supported formats: {}",
            extension,
            SUPPORTED_EXTENSIONS.join(", ")
        ));
    }

    Ok(())
}
