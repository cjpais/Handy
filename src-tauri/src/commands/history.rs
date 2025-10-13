use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::audio_backup::AudioBackupManager;
use crate::managers::transcription::TranscriptionManager;
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
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
pub async fn update_history_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: usize,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.history_limit = limit;
    crate::settings::write_settings(&app, settings);

    history_manager
        .update_history_limit()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_latest_backup_audio(
    _app: AppHandle,
    backup_manager: State<'_, Arc<AudioBackupManager>>,
) -> Result<Option<String>, String> {
    match backup_manager.get_latest_backup() {
        Ok(Some(path)) => Ok(Some(path.to_string_lossy().to_string())),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn retranscribe_backup_audio(
    _app: AppHandle,
    backup_manager: State<'_, Arc<AudioBackupManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    history_manager: State<'_, Arc<HistoryManager>>,
) -> Result<String, String> {
    // Get the latest backup audio file
    let backup_path = match backup_manager.get_latest_backup() {
        Ok(Some(path)) => path,
        Ok(None) => return Err("No backup audio file found".to_string()),
        Err(e) => return Err(format!("Failed to get backup audio: {}", e)),
    };

    // Load audio samples from the backup file
    let audio_samples = match crate::audio_toolkit::load_wav_file(&backup_path) {
        Ok(samples) => samples,
        Err(e) => return Err(format!("Failed to load backup audio: {}", e)),
    };

    // Transcribe the audio
    let transcription_result = transcription_manager
        .transcribe(audio_samples.clone())
        .map_err(|e| format!("Transcription failed: {}", e))?;

    // Save to history
    history_manager
        .save_transcription(audio_samples, transcription_result.clone())
        .await
        .map_err(|e| format!("Failed to save to history: {}", e))?;

    Ok(transcription_result)
}
