use crate::actions::process_transcription_output;
use crate::managers::{
    history::{HistoryEntry, HistoryManager, TranscriptionStatus},
    transcription::TranscriptionManager,
};
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

fn validate_retryable_history_entry(entry: &HistoryEntry) -> Result<(), String> {
    match entry.transcription_status {
        TranscriptionStatus::Pending => {
            Err("Transcription is already in progress for this entry".to_string())
        }
        TranscriptionStatus::Completed => {
            Err("Only failed history entries can be retried".to_string())
        }
        TranscriptionStatus::Failed => Ok(()),
    }
}

async fn fail_retry_history_entry(
    history_manager: &Arc<HistoryManager>,
    id: i64,
    error_message: String,
) -> Result<(), String> {
    history_manager
        .fail_transcription(id, error_message.clone())
        .await
        .map_err(|update_err| {
            format!(
                "{} (also failed to update history entry: {})",
                error_message, update_err
            )
        })
}

#[tauri::command]
#[specta::specta]
pub async fn retry_history_entry_transcription(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    id: i64,
) -> Result<(), String> {
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    validate_retryable_history_entry(&entry)?;

    history_manager
        .set_transcription_pending(id)
        .await
        .map_err(|e| e.to_string())?;

    let samples = match history_manager.load_audio_samples(id).await {
        Ok(samples) => samples,
        Err(err) => {
            let error_message = err.to_string();
            fail_retry_history_entry(&history_manager, id, error_message.clone()).await?;
            return Err(error_message);
        }
    };

    if samples.is_empty() {
        let error_message = "Recording has no audio samples".to_string();
        fail_retry_history_entry(&history_manager, id, error_message.clone()).await?;
        return Err(error_message);
    }

    transcription_manager.initiate_model_load();

    match transcription_manager.transcribe(samples) {
        Ok(transcription) => {
            let processed = process_transcription_output(&app, &transcription, false).await;
            history_manager
                .complete_transcription(
                    id,
                    transcription,
                    processed.post_processed_text,
                    processed.post_process_prompt,
                )
                .await
                .map_err(|e| e.to_string())
        }
        Err(err) => {
            let error_message = err.to_string();
            fail_retry_history_entry(&history_manager, id, error_message.clone()).await?;
            Err(error_message)
        }
    }
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
