use crate::actions::{maybe_spawn_summarization, process_transcription_output};
use crate::managers::{
    history::{HistoryManager, PaginatedHistory},
    transcription::TranscriptionManager,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn get_history_entries(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    cursor: Option<i64>,
    limit: Option<usize>,
) -> Result<PaginatedHistory, String> {
    history_manager
        .get_history_entries(cursor, limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_history_entry_saved(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    let new_saved = history_manager
        .toggle_saved_status(id)
        .await
        .map_err(|e| e.to_string())?;

    // Promotion path (Bronze → Silver): trigger summarisation if not already done.
    // Re-promotion skips re-summarising when summary_status is already "completed".
    if new_saved {
        if let Ok(Some(entry)) = history_manager.get_entry_by_id(id).await {
            if entry.summary_status.as_deref() != Some("completed") {
                let summary_input = entry
                    .post_processed_text
                    .unwrap_or(entry.transcription_text);
                maybe_spawn_summarization(&app, Arc::clone(&history_manager), id, summary_input);
            }
        }
    }

    Ok(())
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

    let audio_path = history_manager.get_audio_file_path(&entry.file_name);
    let samples = crate::audio_toolkit::read_wav_samples(&audio_path)
        .map_err(|e| format!("Failed to load audio: {}", e))?;

    if samples.is_empty() {
        return Err("Recording has no audio samples".to_string());
    }

    transcription_manager.initiate_model_load();

    let tm = Arc::clone(&transcription_manager);
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map_err(|e| e.to_string())?;

    if transcription.is_empty() {
        return Err("Recording contains no speech".to_string());
    }

    let processed = process_transcription_output(&app, &transcription).await;
    history_manager
        .update_transcription(
            id,
            transcription,
            processed.post_processed_text,
            processed.post_process_prompt,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Import an audio file recorded elsewhere (iPhone Voice Memos, a dedicated
/// recorder, etc.) and run it through the transcription pipeline as a kept
/// entry. Decodes via symphonia, mixes to mono, resamples to 16 kHz, transcribes
/// with the active ASR model, re-encodes a 16 kHz WAV into `recordings_dir`, and
/// saves the result as a Silver-tier entry (`saved = true`) so summarisation
/// fires automatically — mirroring a live Keep capture.
///
/// Unlike a live recording, nothing is pasted to the active application and the
/// `TranscriptionCoordinator` / overlay are bypassed. Errors return `Err` so the
/// caller can surface a toast; no orphan entry is written on failure because the
/// entry is only persisted once decode + transcription succeed.
#[tauri::command]
#[specta::specta]
pub async fn import_audio_file(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    path: String,
) -> Result<(), String> {
    let src_path = std::path::PathBuf::from(&path);
    let original_name = src_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "imported audio".to_string());

    // Decode + mono-mix + resample off the async runtime thread.
    let decode_path = src_path.clone();
    let samples = tauri::async_runtime::spawn_blocking(move || {
        crate::audio_toolkit::decode_audio_file_to_16k_mono(&decode_path)
    })
    .await
    .map_err(|e| format!("Audio decode task panicked: {}", e))?
    .map_err(|e| format!("Failed to read audio file: {}", e))?;

    if samples.is_empty() {
        return Err("Audio file contains no audio samples".to_string());
    }

    transcription_manager.initiate_model_load();

    let tm = Arc::clone(&transcription_manager);
    let samples_for_tx = samples.clone();
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples_for_tx))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map_err(|e| e.to_string())?;

    if transcription.trim().is_empty() {
        return Err("No speech detected in audio file".to_string());
    }

    // Re-encode the normalised 16 kHz audio into recordings_dir so the entry's
    // AudioPlayer can play it back, then keep the original filename as the source.
    let file_name = format!("import-{}.wav", chrono::Utc::now().timestamp());
    let wav_path = history_manager.recordings_dir().join(&file_name);
    crate::audio_toolkit::save_wav_file(&wav_path, &samples)
        .map_err(|e| format!("Failed to save imported audio: {}", e))?;

    // Shared input-hygiene pass (matches live captures and retry).
    let processed = process_transcription_output(&app, &transcription).await;
    let summary_input = processed
        .post_processed_text
        .clone()
        .unwrap_or_else(|| transcription.clone());

    let entry = history_manager
        .save_entry_with_title(
            file_name,
            Some(original_name),
            transcription,
            true,
            processed.post_processed_text,
            processed.post_process_prompt,
        )
        .map_err(|e| format!("Failed to save imported entry: {}", e))?;

    maybe_spawn_summarization(&app, Arc::clone(&history_manager), entry.id, summary_input);

    Ok(())
}

/// Re-run summarisation for a single entry. Used for failure recovery and for
/// back-filling entries created before summarisation was enabled.
#[tauri::command]
#[specta::specta]
pub async fn summarize_history_entry(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    let summary_input = entry
        .post_processed_text
        .clone()
        .unwrap_or(entry.transcription_text);

    if summary_input.trim().is_empty() {
        return Err("Entry has no text to summarise".to_string());
    }

    history_manager
        .set_summary_status(id, "pending")
        .map_err(|e| e.to_string())?;

    let settings = crate::settings::get_settings(&app);
    match crate::summarize::summarize_text(&settings, &summary_input).await {
        Ok(result) => history_manager
            .update_summary(
                id,
                result.title,
                Some(result.summary),
                result.actions,
                Some(result.prompt),
                "completed",
            )
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(e) => {
            let _ = history_manager.set_summary_status(id, "failed");
            Err(e)
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
