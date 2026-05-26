use crate::managers::model::{EngineType, ModelInfo, ModelManager};
use crate::managers::transcription::{ModelStateEvent, TranscriptionManager};
use crate::settings::{
    get_settings, has_custom_transcription_endpoint, write_settings, AppSettings,
    ModelUnloadTimeout,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

const CUSTOM_TRANSCRIPTION_MODEL_ID: &str = "custom-transcription-endpoint";
const CUSTOM_TRANSCRIPTION_MODEL_NAME: &str = "Custom Endpoint";

fn custom_transcription_model_info(settings: &AppSettings) -> ModelInfo {
    let model = settings.custom_transcription_model.trim();
    let name = if model.is_empty() {
        CUSTOM_TRANSCRIPTION_MODEL_NAME.to_string()
    } else {
        model.to_string()
    };
    ModelInfo {
        id: CUSTOM_TRANSCRIPTION_MODEL_ID.to_string(),
        name,
        description: String::new(),
        filename: String::new(),
        url: None,
        sha256: None,
        size_mb: 0,
        is_downloaded: true,
        is_downloading: false,
        partial_size: 0,
        is_directory: false,
        engine_type: EngineType::Whisper,
        accuracy_score: 0.0,
        speed_score: 0.0,
        supports_translation: false,
        is_recommended: false,
        supported_languages: Vec::new(),
        supports_language_selection: true,
        is_custom: true,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_models(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<Vec<ModelInfo>, String> {
    let settings = get_settings(&app_handle);
    let mut models = model_manager.get_available_models();
    if has_custom_transcription_endpoint(&settings) {
        models.push(custom_transcription_model_info(&settings));
    }
    Ok(models)
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_info(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<Option<ModelInfo>, String> {
    let settings = get_settings(&app_handle);
    if model_id == CUSTOM_TRANSCRIPTION_MODEL_ID && has_custom_transcription_endpoint(&settings) {
        return Ok(Some(custom_transcription_model_info(&settings)));
    }

    Ok(model_manager.get_model_info(&model_id))
}

#[tauri::command]
#[specta::specta]
pub async fn download_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    let result = model_manager
        .download_model(&model_id)
        .await
        .map_err(|e| e.to_string());

    if let Err(ref error) = result {
        let _ = app_handle.emit(
            "model-download-failed",
            serde_json::json!({ "model_id": &model_id, "error": error }),
        );
    }

    result
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    if model_id == CUSTOM_TRANSCRIPTION_MODEL_ID {
        let mut settings = get_settings(&app_handle);
        settings.custom_transcription_endpoint = None;
        if settings.selected_model == CUSTOM_TRANSCRIPTION_MODEL_ID {
            settings.selected_model = String::new();
        }
        write_settings(&app_handle, settings);
        let _ = app_handle.emit("model-deleted", model_id);
        return Ok(());
    }

    // If deleting the active model, unload it and clear the setting
    let settings = get_settings(&app_handle);
    if settings.selected_model == model_id {
        transcription_manager
            .unload_model()
            .map_err(|e| format!("Failed to unload model: {}", e))?;

        let mut settings = get_settings(&app_handle);
        settings.selected_model = String::new();
        write_settings(&app_handle, settings);
    }

    model_manager
        .delete_model(&model_id)
        .map_err(|e| e.to_string())
}

/// Shared logic for switching the active model, used by both the Tauri command
/// and the tray menu handler.
///
/// Validates the model, updates the persisted setting, and loads the model
/// unless the unload timeout is set to "Immediately" (in which case the model
/// will be loaded on-demand during the next transcription).
pub fn switch_active_model(app: &AppHandle, model_id: &str) -> Result<(), String> {
    let model_manager = app.state::<Arc<ModelManager>>();
    let transcription_manager = app.state::<Arc<TranscriptionManager>>();

    // Atomically claim the loading slot — prevents concurrent model loads
    // from tray double-clicks or overlapping commands. The guard resets the
    // flag on drop (including early returns, errors, and panics).
    let _loading_guard = transcription_manager
        .try_start_loading()
        .ok_or_else(|| "Model load already in progress".to_string())?;

    if model_id == CUSTOM_TRANSCRIPTION_MODEL_ID {
        let mut settings = get_settings(app);
        if !has_custom_transcription_endpoint(&settings) {
            return Err("Custom transcription endpoint is not configured".to_string());
        }

        settings.selected_model = model_id.to_string();
        write_settings(app, settings);
        let _ = app.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_completed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(CUSTOM_TRANSCRIPTION_MODEL_NAME.to_string()),
                error: None,
            },
        );
        return Ok(());
    }

    // Check if model exists and is available
    let model_info = model_manager
        .get_model_info(model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    if !model_info.is_downloaded {
        return Err(format!("Model not downloaded: {}", model_id));
    }

    let settings = get_settings(app);
    let unload_timeout = settings.model_unload_timeout;
    let old_model = settings.selected_model.clone();

    // Persist the new selection early so the frontend sees the correct model
    // when it reacts to events emitted by load_model.
    let mut settings = settings;
    settings.selected_model = model_id.to_string();

    // Reset language to auto if the new model doesn't support the currently selected language.
    // This prevents stale language settings from causing errors (e.g. Canary receiving zh-Hans)
    // and stops downstream processing (e.g. OpenCC) from running on an irrelevant language.
    if settings.selected_language != "auto"
        && !model_info.supported_languages.is_empty()
        && !model_info
            .supported_languages
            .contains(&settings.selected_language)
    {
        log::info!(
            "Resetting language from '{}' to 'auto' (not supported by {})",
            settings.selected_language,
            model_id
        );
        settings.selected_language = "auto".to_string();
    }

    write_settings(app, settings);

    // Skip eager loading if unload is set to "Immediately" — the model
    // will be loaded on-demand during the next transcription.
    if unload_timeout == ModelUnloadTimeout::Immediately {
        // Notify frontend — load_model won't be called so no events
        // would otherwise be emitted.
        let _ = app.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "selection_changed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                error: None,
            },
        );
        log::info!(
            "Model selection changed to {} (not loading — unload set to Immediately).",
            model_id
        );
        return Ok(());
    }

    // Load the model. On failure, revert the persisted selection.
    if let Err(e) = transcription_manager.load_model(model_id) {
        let mut settings = get_settings(app);
        settings.selected_model = old_model;
        write_settings(app, settings);
        return Err(e.to_string());
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_active_model(
    app_handle: AppHandle,
    _model_manager: State<'_, Arc<ModelManager>>,
    _transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
    switch_active_model(&app_handle, &model_id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_current_model(app_handle: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app_handle);
    if has_custom_transcription_endpoint(&settings) {
        return Ok(CUSTOM_TRANSCRIPTION_MODEL_ID.to_string());
    }

    Ok(settings.selected_model)
}

#[tauri::command]
#[specta::specta]
pub async fn get_transcription_model_status(
    app_handle: AppHandle,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<Option<String>, String> {
    let settings = get_settings(&app_handle);
    if has_custom_transcription_endpoint(&settings) {
        return Ok(Some(CUSTOM_TRANSCRIPTION_MODEL_ID.to_string()));
    }

    Ok(transcription_manager.get_current_model())
}

#[tauri::command]
#[specta::specta]
pub async fn is_model_loading(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<bool, String> {
    // Check if transcription manager has a loaded model
    let current_model = transcription_manager.get_current_model();
    Ok(current_model.is_none())
}

#[tauri::command]
#[specta::specta]
pub async fn has_any_models_available(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
    let settings = get_settings(&app_handle);
    if has_custom_transcription_endpoint(&settings) {
        return Ok(true);
    }

    let models = model_manager.get_available_models();
    Ok(models.iter().any(|m| m.is_downloaded))
}

#[tauri::command]
#[specta::specta]
pub async fn has_any_models_or_downloads(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
    let settings = get_settings(&app_handle);
    if has_custom_transcription_endpoint(&settings) {
        return Ok(true);
    }

    let models = model_manager.get_available_models();
    // Return true if any models are downloaded OR if any downloads are in progress
    Ok(models.iter().any(|m| m.is_downloaded))
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_download(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    model_manager
        .cancel_download(&model_id)
        .map_err(|e| e.to_string())
}
