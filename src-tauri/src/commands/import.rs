use crate::audio_toolkit::audio::{decode_and_resample, save_wav_file};
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub async fn import_audio_file(
    app_handle: AppHandle,
    transcription_state: State<'_, Arc<TranscriptionManager>>,
    history_state: State<'_, Arc<HistoryManager>>,
    file_path: String,
) -> Result<(), String> {
    info!("Importing audio file: {}", file_path);
    let _ = app_handle.emit("import-status", "Decoding");

    let source_path = PathBuf::from(&file_path);
    if !source_path.exists() {
        let _ = app_handle.emit("import-status", "Failed");
        return Err(format!("File not found: {}", file_path));
    }

    // 1. Decode and Resample
    let samples = decode_and_resample(source_path.clone()).map_err(|e| {
        let _ = app_handle.emit("import-status", "Failed");
        format!("Failed to decode audio: {}", e)
    })?;

    // 2. Calculate Duration (before transcription to avoid borrow issues)
    let duration = samples.len() as f64 / 16000.0;
    debug!("Audio duration: {:.2}s", duration);

    // 3. Transcribe
    let _ = app_handle.emit("import-status", "Transcribing");
    let transcription_text = transcription_state
        .transcribe(samples.clone())
        .map_err(|e| {
            let _ = app_handle.emit("import-status", "Failed");
            format!("Transcription failed: {}", e)
        })?;

    // 4. Generate Timestamped Filename
    let _ = app_handle.emit("import-status", "Saving");
    let timestamp = Utc::now().timestamp();
    let _new_filename = format!("handy-{}.wav", timestamp); // Using .wav as standard for internal storage

    // Get recordings directory
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let recordings_dir = app_data_dir.join("recordings");

    if !recordings_dir.exists() {
        fs::create_dir_all(&recordings_dir)
            .map_err(|e| format!("Failed to create recordings dir: {}", e))?;
    }

    // 5. Save as new WAV file
    // Instead of moving the original file (which might fail if it's locked or gone),
    // we save the decoded samples as a new, clean 16kHz WAV file.
    let target_path = recordings_dir.join(format!("handy-{}.wav", timestamp));

    debug!("Saving imported audio to {:?}", target_path);
    save_wav_file(&target_path, &samples).await.map_err(|e| {
        let _ = app_handle.emit("import-status", "Failed");
        format!("Failed to save imported audio: {}", e)
    })?;

    // Try to remove the source file (Move behavior), but don't fail if we can't.
    // The user might have deleted it, or it might be read-only.
    // Since we have successfully saved the audio, the import is effectively complete.
    debug!("Attempting to remove source file: {:?}", source_path);
    if let Err(e) = fs::remove_file(&source_path) {
        // Log warning but continue
        log::warn!(
            "Could not remove source file after import: {}. This is non-fatal.",
            e
        );
    }

    // 6. Save to Database
    let title = {
        // We can replicate the title formatting logic or expose it.
        // For now, let's just use a simple format or duplicate the logic locally since it's private in HistoryManager.
        // Actually, let's just use a generic title or formatted date.
        use chrono::{DateTime, Local};
        if let Some(utc_datetime) = DateTime::from_timestamp(timestamp, 0) {
            let local_datetime = utc_datetime.with_timezone(&Local);
            local_datetime.format("%B %e, %Y - %l:%M%p").to_string()
        } else {
            format!("Imported Recording {}", timestamp)
        }
    };

    history_state
        .save_to_database(
            format!("handy-{}.wav", timestamp),
            timestamp,
            title,
            transcription_text,
            None, // post_processed_text
            None, // post_process_prompt
            Some(duration),
        )
        .map_err(|e| format!("Failed to save to database: {}", e))?;

    // Emit history updated event (HistoryManager does this in save_transcription but save_to_database is raw)
    // We should probably emit it here too.
    app_handle
        .emit("history-updated", ())
        .map_err(|e| format!("Failed to emit event: {}", e))?;

    let _ = app_handle.emit("import-status", "Completed");
    info!("Import completed successfully");
    Ok(())
}
