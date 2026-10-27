use crate::language_detection::LanguageStatus;
use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings, ModelUnloadTimeout};
use serde::Serialize;
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, State};

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

/// The language transcription on `model_id` will use, for the picker.
#[tauri::command]
#[specta::specta]
pub fn get_language_status(
    app: AppHandle,
    model_manager: State<Arc<ModelManager>>,
    model_id: String,
) -> LanguageStatus {
    let settings = get_settings(&app);
    crate::language_detection::status_for(
        &settings,
        model_manager.get_model_info(&model_id).as_ref(),
        model_manager.language_detector_path().is_some(),
    )
}

#[tauri::command]
#[specta::specta]
pub fn get_model_load_status(
    transcription_manager: State<Arc<TranscriptionManager>>,
) -> Result<ModelLoadStatus, String> {
    Ok(ModelLoadStatus {
        is_loaded: transcription_manager.is_model_loaded(),
        current_model: transcription_manager.get_current_model(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn unload_model_manually(
    transcription_manager: State<Arc<TranscriptionManager>>,
) -> Result<(), String> {
    transcription_manager.ensure_model_unlocked()?;
    transcription_manager.request_unload();
    Ok(())
}
