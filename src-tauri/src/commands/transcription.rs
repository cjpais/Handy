use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings, ModelUnloadTimeout};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn set_model_unload_timeout(app: AppHandle, timeout: ModelUnloadTimeout) {
    let mut settings = get_settings(&app);
    settings.model_unload_timeout = timeout;
    write_settings(&app, settings);
}

#[tauri::command]
pub fn get_model_load_status(
    transcription_manager: State<TranscriptionManager>,
) -> Result<serde_json::Value, String> {
    let is_loaded = transcription_manager.is_model_loaded();
    let current_model = transcription_manager.get_current_model();

    Ok(serde_json::json!({
        "is_loaded": is_loaded,
        "current_model": current_model
    }))
}

#[tauri::command]
pub fn unload_model_manually(
    transcription_manager: State<TranscriptionManager>,
) -> Result<(), String> {
    transcription_manager
        .unload_model()
        .map_err(|e| format!("Failed to unload model: {}", e))
}
