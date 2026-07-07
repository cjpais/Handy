use crate::actions::process_transcription_output;
use crate::managers::{
    history::{HistoryManager, PaginatedHistory},
    transcription::TranscriptionManager,
};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn process_local_file(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    path: String,
    action: String, // "transcribe" or "meeting"
) -> Result<i64, String> {
    let source_path = std::path::Path::new(&path);
    if !source_path.exists() {
        return Err(format!("File does not exist: {}", path));
    }

    let file_name = format!("thegai-{}.wav", chrono::Utc::now().timestamp());
    let dest_path = history_manager.recordings_dir().join(&file_name);

    // Decode MP3/FLAC/WAV/etc. using Symphonia and resample to 16kHz mono.
    let samples = crate::audio_toolkit::read_any_audio_file(&source_path)
        .map_err(|e| format!("Failed to read audio file: {}", e))?;

    if samples.is_empty() {
        return Err("Audio file contains no samples".to_string());
    }

    // Save as WAV into our recordings folder
    crate::audio_toolkit::save_wav_file(&dest_path, &samples)
        .map_err(|e| format!("Failed to save audio to recordings: {}", e))?;

    let is_meeting = action == "meeting";

    // Create the history entry initially with empty text
    history_manager
        .save_entry(
            file_name.clone(),
            String::new(),
            is_meeting,
            None,
            if is_meeting {
                Some("default_meeting_summary".to_string())
            } else {
                None
            },
        )
        .map_err(|e| format!("Failed to create history entry: {}", e))?;

    // Transcribe
    transcription_manager.initiate_model_load();
    let tm = Arc::clone(&transcription_manager);
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map(|r| r.text)
        .map_err(|e| e.to_string())?;

    let (post_processed_text, post_process_prompt) = if is_meeting {
        // For meetings, we want to force post-processing with the summary prompt.
        let settings = crate::settings::get_settings(&app);
        let prompt_id = if settings.google_oauth_token.is_some() {
            "default_meeting_notes_with_actions"
        } else {
            "default_meeting_summary"
        };
        let summary_opt =
            crate::actions::run_specific_llm_prompt(&app, &settings, prompt_id, &transcription).await;
        (summary_opt, Some(prompt_id.to_string()))
    } else {
        let processed = process_transcription_output(&app, &transcription, false).await;
        (processed.post_processed_text, processed.post_process_prompt)
    };

    // Update the entry in the DB. Since we don't have the ID easily, we can find it by file_name.
    // We query the latest entries to find the one we just created.
    if let Ok(paginated) = history_manager.get_history_entries(None, Some(20)).await {
        if let Some(entry) = paginated
            .entries
            .into_iter()
            .find(|e| e.file_name == file_name)
        {
            history_manager
                .update_transcription(
                    entry.id,
                    transcription,
                    post_processed_text,
                    post_process_prompt,
                )
                .map_err(|e| e.to_string())?;
            return Ok(entry.id);
        }
    }

    Ok(-1)
}

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
    let samples = crate::audio_toolkit::read_any_audio_file(&audio_path)
        .map_err(|e| format!("Failed to load audio: {}", e))?;

    if samples.is_empty() {
        return Err("Recording has no audio samples".to_string());
    }

    transcription_manager.initiate_model_load();

    let tm = Arc::clone(&transcription_manager);
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map(|r| r.text)
        .map_err(|e| e.to_string())?;

    if transcription.is_empty() {
        return Err("Recording contains no speech".to_string());
    }

    let processed =
        process_transcription_output(&app, &transcription, entry.post_process_requested).await;
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
pub async fn ask_meeting_question(
    app: AppHandle,
    transcript: String,
    question: String,
) -> Result<String, String> {
    let settings = crate::settings::get_settings(&app);
    let provider = match settings.active_post_process_provider().cloned() {
        Some(provider) => provider,
        None => return Err("No LLM provider selected".to_string()),
    };

    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    if model.trim().is_empty() {
        return Err(format!(
            "No model configured for provider '{}'",
            provider.id
        ));
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    let system_prompt = format!(
        "You are a helpful meeting assistant. Use the following meeting transcript as context to answer the user's question accurately. If the information is not in the transcript, say so.\n\nTRANSCRIPT:\n{}",
        transcript
    );

    // Reuse reasoning config logic from actions.rs
    let (reasoning_effort, reasoning) = match provider.id.as_str() {
        "custom" | "google" => (Some("none".to_string()), None),
        "openrouter" => (
            None,
            Some(crate::llm_client::ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
        ),
        _ => (None, None),
    };

    match crate::llm_client::send_chat_completion_with_schema(
        &provider,
        api_key,
        &model,
        question,
        Some(system_prompt),
        None, // No schema, we want natural language response
        reasoning_effort,
        reasoning,
    )
    .await
    {
        Ok(Some(content)) => Ok(content),
        Ok(None) => Err("LLM returned an empty response".to_string()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn regenerate_history_entry_summary(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    if entry.transcription_text.is_empty() {
        return Err("No transcription available to summarize".to_string());
    }

    let settings = crate::settings::get_settings(&app);
    
    // Determine the prompt ID to use. If it's a meeting entry, we use the meeting prompt,
    // otherwise we use standard post-processing.
    let is_meeting = entry.post_process_prompt.as_deref() == Some("default_meeting_summary")
        || entry.post_process_prompt.as_deref() == Some("default_meeting_notes_with_actions");

    let (post_processed_text, post_process_prompt) = if is_meeting {
        let prompt_id = if settings.google_oauth_token.is_some() {
            "default_meeting_notes_with_actions"
        } else {
            "default_meeting_summary"
        };
        let summary_opt =
            crate::actions::run_specific_llm_prompt(&app, &settings, prompt_id, &entry.transcription_text).await;
        (summary_opt, Some(prompt_id.to_string()))
    } else {
        let processed = process_transcription_output(&app, &entry.transcription_text, entry.post_process_requested).await;
        (processed.post_processed_text, processed.post_process_prompt)
    };

    history_manager
        .update_transcription(
            id,
            entry.transcription_text,
            post_processed_text,
            post_process_prompt,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

