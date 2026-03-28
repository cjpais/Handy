use crate::audio_toolkit::audio::decode_and_resample;
use crate::audio_toolkit::save_wav_file;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use log::{debug, info, warn};
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

/// Sample rate expected by all transcription engines
const SAMPLE_RATE: usize = 16000;

/// Chunk duration in seconds for splitting long audio.
/// 30 seconds matches Whisper's native context window and works well with
/// other engines too (Parakeet, Moonshine, SenseVoice, etc.).
const CHUNK_DURATION_SECS: usize = 30;

/// Overlap between consecutive chunks in seconds.
/// Prevents word-boundary artifacts at chunk edges.
const CHUNK_OVERLAP_SECS: f32 = 1.0;

/// Audio shorter than this (in seconds) is transcribed in one shot.
/// Audio longer is split into chunks for reliability and progress reporting.
const CHUNK_THRESHOLD_SECS: f64 = 35.0;

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

/// Split audio samples into fixed-size chunks with overlap.
/// Returns a Vec of (start_sample, end_sample) ranges.
fn make_chunks(total_samples: usize) -> Vec<(usize, usize)> {
    let chunk_samples = CHUNK_DURATION_SECS * SAMPLE_RATE;
    let overlap_samples = (CHUNK_OVERLAP_SECS * SAMPLE_RATE as f32) as usize;
    let step = chunk_samples.saturating_sub(overlap_samples);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < total_samples {
        let end = (start + chunk_samples).min(total_samples);
        chunks.push((start, end));
        if end >= total_samples {
            break;
        }
        start += step;
    }
    chunks
}

/// Transcribe audio, using chunked processing for long files.
///
/// Short audio (< CHUNK_THRESHOLD_SECS) is transcribed in a single call.
/// Long audio is split into 30-second overlapping chunks, each transcribed
/// separately, with progress events and cancellation checks between chunks.
///
/// This approach:
/// - Works with ALL upstream engine types (Whisper, Parakeet, Moonshine, etc.)
/// - Doesn't modify TranscriptionManager at all
/// - Provides per-chunk progress for long files
/// - Supports cancellation between chunks
async fn transcribe_with_chunking(
    app_handle: &AppHandle,
    transcription_state: &Arc<TranscriptionManager>,
    cancellation_tokens: &ImportCancellationTokens,
    import_id: &str,
    samples: &[f32],
) -> Result<String, String> {
    let duration_secs = samples.len() as f64 / SAMPLE_RATE as f64;

    // Short audio: single-shot transcription
    if duration_secs <= CHUNK_THRESHOLD_SECS {
        info!(
            "Short audio ({:.1}s), transcribing in one shot",
            duration_secs
        );
        emit_progress(app_handle, "transcribing", 30, "Transcribing...");

        let tm = Arc::clone(transcription_state);
        let audio = samples.to_vec();
        return match tauri::async_runtime::spawn_blocking(move || tm.transcribe(audio)).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(e)) => Err(format!("Transcription failed: {}", e)),
            Err(e) => Err(format!("Transcription task panicked: {}", e)),
        };
    }

    // Long audio: chunked transcription
    let chunks = make_chunks(samples.len());
    let total_chunks = chunks.len();
    info!(
        "Long audio ({:.1}s), splitting into {} chunks of ~{}s each",
        duration_secs, total_chunks, CHUNK_DURATION_SECS
    );

    let mut results: Vec<String> = Vec::new();

    for (i, (start, end)) in chunks.iter().enumerate() {
        // Check cancellation between chunks
        check_cancelled(app_handle, cancellation_tokens, import_id)?;

        let chunk_duration = (*end - *start) as f32 / SAMPLE_RATE as f32;

        // Map chunk progress to 20-85% range (leaving room for decode and save stages)
        let progress_fraction = (i as f32 / total_chunks as f32) * 65.0;
        let overall_percent = (20.0 + progress_fraction) as u8;

        emit_progress(
            app_handle,
            "transcribing",
            overall_percent.min(85),
            &format!(
                "Transcribing chunk {}/{} ({:.0}s)...",
                i + 1,
                total_chunks,
                chunk_duration
            ),
        );

        let chunk_audio = samples[*start..*end].to_vec();
        let tm = Arc::clone(transcription_state);

        let chunk_text =
            match tauri::async_runtime::spawn_blocking(move || tm.transcribe(chunk_audio)).await {
                Ok(Ok(text)) => text,
                Ok(Err(e)) => {
                    warn!(
                        "Chunk {}/{} transcription failed: {}, skipping",
                        i + 1,
                        total_chunks,
                        e
                    );
                    // Don't abort the whole import for one bad chunk
                    continue;
                }
                Err(e) => {
                    warn!(
                        "Chunk {}/{} panicked: {}, skipping",
                        i + 1,
                        total_chunks,
                        e
                    );
                    continue;
                }
            };

        if !chunk_text.trim().is_empty() {
            results.push(chunk_text.trim().to_string());
        }

        info!(
            "Chunk {}/{} done ({:.0}s of audio)",
            i + 1,
            total_chunks,
            chunk_duration
        );
    }

    if results.is_empty() {
        return Err("No speech detected in any chunk".to_string());
    }

    let combined = results.join(" ");
    info!(
        "Chunked transcription complete: {} chunks, {} chars total",
        total_chunks,
        combined.len()
    );
    Ok(combined)
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

    let duration = samples.len() as f64 / SAMPLE_RATE as f64;
    debug!("Audio duration: {:.2}s ({} samples)", duration, samples.len());

    // Stage 2: Load model and transcribe (with chunking for long files)
    emit_progress(
        &app_handle,
        "transcribing",
        15,
        "Loading transcription model...",
    );

    // Initiate model load (required — transcribe() doesn't auto-load)
    transcription_state.initiate_model_load();

    let transcription_text = match transcribe_with_chunking(
        &app_handle,
        &transcription_state,
        &cancellation_tokens,
        &import_id,
        &samples,
    )
    .await
    {
        Ok(text) => text,
        Err(e) => {
            emit_progress(&app_handle, "failed", 0, &e);
            cancellation_tokens.cleanup(&import_id);
            return Err(e);
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

    // Save to database
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
    info!("Import completed successfully ({:.1}s audio)", duration);
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
