use crate::managers::model::{ModelManager, MODEL_NAME, MODEL_SIZE_BYTES};
use crate::managers::transcription::TranscriptionManager;
use log::error;
use serde::Serialize;
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// What the settings window shows about the one model.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModelStatus {
    pub name: String,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub is_loaded: bool,
}

#[tauri::command]
#[specta::specta]
pub fn get_model_status(
    model_manager: State<'_, Arc<ModelManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> ModelStatus {
    ModelStatus {
        name: MODEL_NAME.to_string(),
        size_mb: MODEL_SIZE_BYTES / (1024 * 1024),
        is_downloaded: model_manager.is_downloaded(),
        is_downloading: model_manager.is_downloading(),
        is_loaded: transcription_manager.is_model_loaded(),
    }
}

/// Make the model ready: download it if it is missing, then load it. Used at
/// startup and by the settings window's retry button.
pub async fn ensure_model_ready(app: &AppHandle) -> Result<(), String> {
    let model_manager = app.state::<Arc<ModelManager>>();
    let transcription_manager = app.state::<Arc<TranscriptionManager>>();

    match model_manager.download().await {
        Ok(true) => {}
        // Cancelled by the user: leave the model for the next attempt.
        Ok(false) => return Ok(()),
        Err(e) => {
            // Log as well as emit: the toast is transient, and failed downloads
            // have historically been undiagnosable because logs showed
            // nothing (#1579).
            error!("Model download failed: {e}");
            let _ = app.emit("model-download-failed", e.to_string());
            return Err(e.to_string());
        }
    }
    transcription_manager.initiate_model_load();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn download_model(app: AppHandle) -> Result<(), String> {
    ensure_model_ready(&app).await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_download(model_manager: State<'_, Arc<ModelManager>>) {
    model_manager.cancel_download();
}
