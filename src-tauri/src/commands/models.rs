use crate::managers::model::{ModelInfo, ModelManager};
use crate::managers::transcription::{ModelStateEvent, TranscriptionManager};
use crate::settings::{get_settings, write_settings, AppSettings, ModelUnloadTimeout};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSelectionSnapshot {
    selected_model: String,
    selected_language: String,
    secondary_selected_language: String,
}

fn sanitize_language_for_model(language: &str, model_info: &ModelInfo) -> Option<String> {
    if language == "auto"
        || model_info.supported_languages.is_empty()
        || model_info
            .supported_languages
            .iter()
            .any(|supported| supported == language)
    {
        return None;
    }

    Some("auto".to_string())
}

fn sanitize_transcription_languages(
    settings: &mut AppSettings,
    model_id: &str,
    model_info: &ModelInfo,
) {
    sanitize_setting_language(
        &mut settings.selected_language,
        "language",
        model_id,
        model_info,
    );
    sanitize_setting_language(
        &mut settings.secondary_selected_language,
        "secondary language",
        model_id,
        model_info,
    );
}

fn sanitize_setting_language(
    language: &mut String,
    label: &str,
    model_id: &str,
    model_info: &ModelInfo,
) {
    if let Some(sanitized_language) = sanitize_language_for_model(language, model_info) {
        log::info!(
            "Resetting {} from '{}' to 'auto' (not supported by {})",
            label,
            language,
            model_id
        );
        *language = sanitized_language;
    }
}

fn snapshot_model_selection(settings: &AppSettings) -> ModelSelectionSnapshot {
    ModelSelectionSnapshot {
        selected_model: settings.selected_model.clone(),
        selected_language: settings.selected_language.clone(),
        secondary_selected_language: settings.secondary_selected_language.clone(),
    }
}

fn restore_model_selection(settings: &mut AppSettings, snapshot: &ModelSelectionSnapshot) {
    settings.selected_model = snapshot.selected_model.clone();
    settings.selected_language = snapshot.selected_language.clone();
    settings.secondary_selected_language = snapshot.secondary_selected_language.clone();
}

#[tauri::command]
#[specta::specta]
pub async fn get_available_models(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<Vec<ModelInfo>, String> {
    Ok(model_manager.get_available_models())
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_info(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<Option<ModelInfo>, String> {
    Ok(model_manager.get_model_info(&model_id))
}

#[tauri::command]
#[specta::specta]
pub async fn download_model(
    model_manager: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    model_manager
        .download_model(&model_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    app_handle: AppHandle,
    model_manager: State<'_, Arc<ModelManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    model_id: String,
) -> Result<(), String> {
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

    // Check if model exists and is available
    let model_info = model_manager
        .get_model_info(model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    if !model_info.is_downloaded {
        return Err(format!("Model not downloaded: {}", model_id));
    }

    let settings = get_settings(app);
    let unload_timeout = settings.model_unload_timeout;
    let previous_selection = snapshot_model_selection(&settings);

    // Persist the new selection early so the frontend sees the correct model
    // when it reacts to events emitted by load_model.
    let mut settings = settings;
    settings.selected_model = model_id.to_string();

    // Reset languages to auto if the new model doesn't support them.
    // This prevents stale language settings from causing downstream errors.
    sanitize_transcription_languages(&mut settings, model_id, &model_info);

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
        restore_model_selection(&mut settings, &previous_selection);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::EngineType;
    use crate::settings::get_default_settings;

    fn test_model_info(supported_languages: &[&str]) -> ModelInfo {
        ModelInfo {
            id: "test".to_string(),
            name: "Test Model".to_string(),
            description: String::new(),
            filename: String::new(),
            url: None,
            size_mb: 0,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            is_directory: false,
            engine_type: EngineType::Whisper,
            accuracy_score: 0.0,
            speed_score: 0.0,
            supports_translation: true,
            is_recommended: false,
            supported_languages: supported_languages.iter().map(|s| s.to_string()).collect(),
            supports_language_selection: true,
            is_custom: false,
        }
    }

    #[test]
    fn sanitize_language_keeps_supported_values() {
        let model_info = test_model_info(&["en", "uk"]);

        assert_eq!(sanitize_language_for_model("en", &model_info), None);
        assert_eq!(sanitize_language_for_model("auto", &model_info), None);
    }

    #[test]
    fn sanitize_language_resets_unsupported_values_to_auto() {
        let model_info = test_model_info(&["en"]);

        assert_eq!(
            sanitize_language_for_model("uk", &model_info),
            Some("auto".to_string())
        );
    }

    #[test]
    fn sanitize_transcription_languages_updates_primary_and_secondary() {
        let model_info = test_model_info(&["en"]);
        let mut settings = get_default_settings();
        settings.selected_language = "uk".to_string();
        settings.secondary_selected_language = "fr".to_string();

        sanitize_transcription_languages(&mut settings, "test", &model_info);

        assert_eq!(settings.selected_language, "auto");
        assert_eq!(settings.secondary_selected_language, "auto");
    }

    #[test]
    fn sanitize_transcription_languages_keeps_supported_secondary_language() {
        let model_info = test_model_info(&["en", "uk"]);
        let mut settings = get_default_settings();
        settings.selected_language = "en".to_string();
        settings.secondary_selected_language = "uk".to_string();

        sanitize_transcription_languages(&mut settings, "test", &model_info);

        assert_eq!(settings.selected_language, "en");
        assert_eq!(settings.secondary_selected_language, "uk");
    }

    #[test]
    fn restore_model_selection_restores_model_and_languages() {
        let mut settings = get_default_settings();
        settings.selected_model = "new-model".to_string();
        settings.selected_language = "auto".to_string();
        settings.secondary_selected_language = "auto".to_string();

        let snapshot = ModelSelectionSnapshot {
            selected_model: "old-model".to_string(),
            selected_language: "en".to_string(),
            secondary_selected_language: "uk".to_string(),
        };

        restore_model_selection(&mut settings, &snapshot);

        assert_eq!(settings.selected_model, "old-model");
        assert_eq!(settings.selected_language, "en");
        assert_eq!(settings.secondary_selected_language, "uk");
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_current_model(app_handle: AppHandle) -> Result<String, String> {
    let settings = get_settings(&app_handle);
    Ok(settings.selected_model)
}

#[tauri::command]
#[specta::specta]
pub async fn get_transcription_model_status(
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
) -> Result<Option<String>, String> {
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
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
    let models = model_manager.get_available_models();
    Ok(models.iter().any(|m| m.is_downloaded))
}

#[tauri::command]
#[specta::specta]
pub async fn has_any_models_or_downloads(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<bool, String> {
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
