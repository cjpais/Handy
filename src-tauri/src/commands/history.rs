use crate::actions::post_process_transcription;
use crate::managers::history::{HistoryEntry, HistoryManager, TranscriptionVersion};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn get_history_entries(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
) -> Result<Vec<HistoryEntry>, String> {
    history_manager
        .get_history_entries()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_history_entry_saved(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .toggle_saved_status(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_audio_file_path(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    file_name: String,
) -> Result<String, String> {
    let path = history_manager.get_audio_file_path(&file_name);
    path.to_str()
        .ok_or_else(|| "Invalid file path".to_string())
        .map(|s| s.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_entry(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .delete_entry(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn update_history_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: usize,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.history_limit = limit;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_recording_retention_period(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    period: String,
) -> Result<(), String> {
    use crate::settings::RecordingRetentionPeriod;

    let retention_period = match period.as_str() {
        "never" => RecordingRetentionPeriod::Never,
        "preserve_limit" => RecordingRetentionPeriod::PreserveLimit,
        "days3" => RecordingRetentionPeriod::Days3,
        "weeks2" => RecordingRetentionPeriod::Weeks2,
        "months3" => RecordingRetentionPeriod::Months3,
        _ => return Err(format!("Invalid retention period: {}", period)),
    };

    let mut settings = crate::settings::get_settings(&app);
    settings.recording_retention_period = retention_period;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn change_history_post_process_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.history_post_process_enabled = enabled;
    crate::settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn post_process_history_entry(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<String, String> {
    // Enforce three-level feature gate on the backend
    let settings = crate::settings::get_settings(&app);
    if !settings.experimental_enabled
        || !settings.post_process_enabled
        || !settings.history_post_process_enabled
    {
        return Err("HISTORY_POST_PROCESS_DISABLED".to_string());
    }

    // Get the history entry
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    if entry.transcription_text.trim().is_empty() {
        return Err("TRANSCRIPTION_EMPTY".to_string());
    }

    // Run post-processing (reuses settings from feature gate check above)
    let processed_text = post_process_transcription(&settings, &entry.transcription_text)
        .await
        .ok_or_else(|| "POST_PROCESS_FAILED".to_string())?;

    // Get the prompt that was used
    let prompt_text = settings
        .post_process_selected_prompt_id
        .as_ref()
        .and_then(|prompt_id| {
            settings
                .post_process_prompts
                .iter()
                .find(|p| &p.id == prompt_id)
                .map(|p| p.prompt.clone())
        })
        .unwrap_or_default();

    // Save version and update entry atomically
    history_manager
        .save_version_and_update(id, &processed_text, &prompt_text)
        .map_err(|e| e.to_string())?;

    Ok(processed_text)
}

#[tauri::command]
#[specta::specta]
pub async fn get_transcription_versions(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    entry_id: i64,
) -> Result<Vec<TranscriptionVersion>, String> {
    history_manager
        .get_versions(entry_id)
        .map_err(|e| e.to_string())
}
