use crate::audio_toolkit::audio::decode_and_resample;
use crate::audio_toolkit::save_wav_file;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use log::{debug, info};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tauri::{AppHandle, Emitter, State};

/// Maximum file size for import (10 GB)
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024 * 1024;

/// Progress event emitted during import
#[derive(Serialize, Clone, Debug, Type)]
pub struct ImportProgress {
    pub stage: String,   // "decoding", "transcribing", "saving", "completed", "failed", "cancelled"
    pub percent: u8,     // 0-100
    pub message: String, // User-friendly message
}

/// Cancellation tokens for active imports
pub struct ImportCancellationTokens {
    tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl ImportCancellationTokens {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_token(&self, import_id: &str) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.insert(import_id.to_string(), token.clone());
        }
        token
    }

    pub fn is_cancelled(&self, import_id: &str) -> bool {
        if let Ok(tokens) = self.tokens.lock() {
            if let Some(token) = tokens.get(import_id) {
                return token.load(Ordering::Relaxed);
            }
        }
        false
    }

    pub fn cancel(&self, import_id: &str) {
        if let Ok(tokens) = self.tokens.lock() {
            if let Some(token) = tokens.get(import_id) {
                token.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn cleanup(&self, import_id: &str) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.remove(import_id);
        }
    }
}

impl Default for ImportCancellationTokens {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that a file is a readable audio file before processing
fn validate_audio_file(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    let file_size = fs::metadata(path)
        .map_err(|e| format!("Cannot read file metadata: {}", e))?
        .len();

    if file_size == 0 {
        return Err("File is empty".to_string());
    }

    if file_size > MAX_FILE_SIZE {
        return Err(format!(
            "File is too large ({:.1}GB, max 10GB). Consider using a smaller file.",
            file_size as f64 / (1024.0 * 1024.0 * 1024.0)
        ));
    }

    // Probe the file to verify it's valid audio
    let file = fs::File::open(path).map_err(|e| format!("Cannot open file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            hint.with_extension(ext_str);
        }
    }

    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("File does not appear to be a valid audio file: {}", e))?;

    if let Some(track) = probed.format.tracks().first() {
        debug!(
            "Audio format detected: {} channels, {} Hz",
            track.codec_params.channels.map(|c| c.count()).unwrap_or(1),
            track.codec_params.sample_rate.unwrap_or(0)
        );
    }

    Ok(())
}

/// Emit a progress update event
fn emit_progress(app_handle: &AppHandle, stage: &str, percent: u8, message: &str) {
    let _ = app_handle.emit(
        "import-progress",
        ImportProgress {
            stage: stage.to_string(),
            percent,
            message: message.to_string(),
        },
    );
}

/// Check if import was cancelled
fn check_cancelled(
    app_handle: &AppHandle,
    cancellation_tokens: &ImportCancellationTokens,
    import_id: &str,
) -> Result<(), String> {
    if cancellation_tokens.is_cancelled(import_id) {
        emit_progress(app_handle, "cancelled", 0, "Import cancelled");
        return Err("Import was cancelled".into());
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn import_audio_file(
    app_handle: AppHandle,
    transcription_state: State<'_, Arc<TranscriptionManager>>,
    history_state: State<'_, Arc<HistoryManager>>,
    cancellation_tokens: State<'_, Arc<ImportCancellationTokens>>,
    file_path: String,
) -> Result<(), String> {
    use uuid::Uuid;

    let import_id = Uuid::new_v4().to_string();
    let _cancel_token = cancellation_tokens.create_token(&import_id);

    info!("Importing audio file: {} (ID: {})", file_path, import_id);

    // Emit initial progress
    emit_progress(&app_handle, "starting", 0, "Starting import...");

    let source_path = PathBuf::from(&file_path);

    // Validate file before doing anything
    if let Err(e) = validate_audio_file(&source_path) {
        emit_progress(&app_handle, "failed", 0, &e);
        return Err(e);
    }

    // Check for cancellation after validation
    check_cancelled(&app_handle, &cancellation_tokens, &import_id)?;

    // Stage 1: Decoding
    emit_progress(&app_handle, "decoding", 5, "Decoding audio file...");

    let samples = match decode_and_resample(source_path.clone()) {
        Ok(s) => s,
        Err(e) => {
            emit_progress(
                &app_handle,
                "failed",
                0,
                &format!("Failed to decode audio: {}", e),
            );
            cancellation_tokens.cleanup(&import_id);
            return Err(format!("Failed to decode audio: {}", e));
        }
    };

    // Check for cancellation after decoding
    check_cancelled(&app_handle, &cancellation_tokens, &import_id)?;

    // Stage 2: Transcribing
    emit_progress(&app_handle, "transcribing", 20, "Loading model & transcribing...");

    // Calculate Duration
    let duration = samples.len() as f64 / 16000.0;
    debug!("Audio duration: {:.2}s", duration);

    // Initiate model load (required — transcribe() doesn't auto-load)
    transcription_state.initiate_model_load();

    // Clone samples before moving into blocking thread (we need them later for WAV save)
    let samples_for_transcribe = samples.clone();

    // Run transcription on a blocking thread (model inference is CPU-bound)
    let tm = Arc::clone(&transcription_state);
    let transcription_text = match tauri::async_runtime::spawn_blocking(move || {
        tm.transcribe(samples_for_transcribe)
    })
    .await
    {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            emit_progress(
                &app_handle,
                "failed",
                0,
                &format!("Transcription failed: {}", e),
            );
            cancellation_tokens.cleanup(&import_id);
            return Err(format!("Transcription failed: {}", e));
        }
        Err(e) => {
            emit_progress(
                &app_handle,
                "failed",
                0,
                &format!("Transcription task panicked: {}", e),
            );
            cancellation_tokens.cleanup(&import_id);
            return Err(format!("Transcription task panicked: {}", e));
        }
    };

    // Check for cancellation after transcription
    check_cancelled(&app_handle, &cancellation_tokens, &import_id)?;

    // Stage 3: Saving
    emit_progress(&app_handle, "saving", 90, "Saving to database...");

    let timestamp = chrono::Utc::now().timestamp();

    // Get recordings directory from the history manager
    let recordings_dir = history_state.recordings_dir().to_path_buf();
    if !recordings_dir.exists() {
        fs::create_dir_all(&recordings_dir)
            .map_err(|e| format!("Failed to create recordings dir: {}", e))?;
    }

    // Save as new WAV file
    let file_name = format!("handy-{}.wav", timestamp);
    let target_path = recordings_dir.join(&file_name);

    debug!("Saving imported audio to {:?}", target_path);
    if let Err(e) = save_wav_file(&target_path, &samples) {
        emit_progress(
            &app_handle,
            "failed",
            0,
            &format!("Failed to save audio: {}", e),
        );
        cancellation_tokens.cleanup(&import_id);
        return Err(format!("Failed to save imported audio: {}", e));
    }

    debug!(
        "Import completed, keeping source file at: {:?}",
        source_path
    );

    // Save to database using upstream's save_entry with extra fields
    if let Err(e) = history_state.save_entry_with_import(
        file_name,
        transcription_text,
        false,
        None,
        None,
        Some(duration),
        Some("upload".to_string()),
    ) {
        emit_progress(
            &app_handle,
            "failed",
            0,
            &format!("Failed to save to database: {}", e),
        );
        cancellation_tokens.cleanup(&import_id);
        return Err(format!("Failed to save to database: {}", e));
    }

    // Complete
    emit_progress(
        &app_handle,
        "completed",
        100,
        "Import completed successfully",
    );
    cancellation_tokens.cleanup(&import_id);
    info!("Import completed successfully");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_import(
    cancellation_tokens: State<'_, Arc<ImportCancellationTokens>>,
    import_id: String,
) -> Result<(), String> {
    cancellation_tokens.cancel(&import_id);
    info!("Import {} cancelled by user", import_id);
    Ok(())
}
