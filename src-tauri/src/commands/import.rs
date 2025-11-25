use crate::audio_toolkit::audio::decode_and_resample;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub async fn import_audio_file(
    app_handle: AppHandle,
    transcription_state: State<'_, TranscriptionManager>,
    history_state: State<'_, HistoryManager>,
    file_path: String,
) -> Result<(), String> {
    info!("Importing audio file: {}", file_path);

    let source_path = PathBuf::from(&file_path);
    if !source_path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // 1. Decode and Resample
    let samples = decode_and_resample(source_path.clone())
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

    // 2. Calculate Duration
    let duration = samples.len() as f64 / 16000.0;
    debug!("Audio duration: {:.2}s", duration);

    // 3. Transcribe
    let transcription_text = transcription_state
        .transcribe(samples)
        .map_err(|e| format!("Transcription failed: {}", e))?;

    // 4. Generate Timestamped Filename
    let timestamp = Utc::now().timestamp();
    let new_filename = format!("handy-{}.wav", timestamp); // Using .wav as standard for internal storage

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

    // 5. Move (Rename) File
    // Note: Since we decoded the file, we might want to save the *resampled* audio
    // instead of the original if we want consistency (16kHz WAV).
    // However, the requirement says "Move, Don't Copy".
    // If the original is NOT a WAV, simply renaming it to .wav is bad practice.
    // BUT, the user requirement explicitly said: "Move, Don't Copy: Use fs::rename to move the file."
    // AND "Generate a new filename... formatted to match Handy’s existing convention".
    //
    // If the source is MP3 and we rename to .wav, players might fail.
    // Let's preserve the extension if possible, OR just move it.
    // Handy's history manager seems to expect "handy-{timestamp}.wav" in `save_transcription`.
    // But `save_to_database` takes a filename string.

    // Let's check the source extension.
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");

    let new_filename_with_ext = format!("handy-{}.{}", timestamp, ext);
    let target_path_with_ext = recordings_dir.join(&new_filename_with_ext);

    debug!(
        "Moving file from {:?} to {:?}",
        source_path, target_path_with_ext
    );

    if let Err(e) = fs::rename(&source_path, &target_path_with_ext) {
        // Fallback to copy + delete if rename fails (e.g. cross-device)
        debug!("Rename failed ({}), trying copy+delete", e);
        fs::copy(&source_path, &target_path_with_ext)
            .map_err(|e| format!("Failed to copy file: {}", e))?;
        fs::remove_file(&source_path)
            .map_err(|e| format!("Failed to remove source file: {}", e))?;
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
            new_filename_with_ext,
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

    info!("Import completed successfully");
    Ok(())
}
