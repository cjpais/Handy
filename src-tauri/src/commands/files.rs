//! Offline batch transcription of user-supplied audio files (the "Files" tab).
//!
//! The frontend passes path strings only and never touches the filesystem: the
//! `fs` capability scope is `$APPDATA`-only with no write permission, so a file
//! in `~/Music` is unreadable from JS and a ZIP is unwritable from JS. All I/O
//! happens here.
//!
//! Files are transcribed strictly sequentially. `TranscriptionManager::transcribe`
//! takes the engine out of its mutex for the duration of inference, so a second
//! concurrent caller sees `None` and fails with "Model is not loaded for
//! transcription." — hence the recording/streaming rejection up front and the
//! suspended global shortcuts for the life of the batch.

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::actions::process_transcription_output;
use crate::audio_toolkit::decode_audio_file;
use crate::managers::{audio::AudioRecordingManager, transcription::TranscriptionManager};
use crate::settings::get_settings;
use crate::shortcut::{resume_all_shortcuts, suspend_all_shortcuts};

/// Whether a batch is in flight. Module-level rather than Tauri state because
/// nothing outside this module needs it.
static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);
/// Set by `cancel_file_transcription`, read between files by the worker.
static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

/// Progress for a batch run. Internally tagged so the frontend can switch on
/// `status`, following the `HistoryUpdatePayload` precedent.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FileTranscriptionEvent {
    Started { path: String },
    Completed { path: String, text: String },
    Failed { path: String, error: String },
    BatchFinished { cancelled: bool },
}

/// Restores global shortcuts on every worker exit path, including a panic.
struct ShortcutGuard(AppHandle);

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        resume_all_shortcuts(&self.0);
    }
}

/// Clears the batch flags on every worker exit path, so a panic mid-batch
/// cannot wedge the tab into a permanently "running" state.
struct BatchGuard;

impl Drop for BatchGuard {
    fn drop(&mut self) {
        BATCH_CANCEL.store(false, Ordering::Release);
        BATCH_RUNNING.store(false, Ordering::Release);
    }
}

/// Start transcribing `paths` sequentially with the currently selected model.
///
/// Returns as soon as the batch is accepted; everything after that is reported
/// through [`FileTranscriptionEvent`]. All rejections happen synchronously so
/// the frontend gets an `Err` it can surface immediately.
#[tauri::command]
#[specta::specta]
pub async fn transcribe_audio_files(
    app: AppHandle,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    audio_manager: State<'_, Arc<AudioRecordingManager>>,
    paths: Vec<String>,
    post_process: bool,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("No files selected".to_string());
    }

    if audio_manager.is_recording() {
        return Err("Cannot transcribe files while recording is in progress".to_string());
    }

    if transcription_manager.is_streaming() {
        return Err("Cannot transcribe files while a live transcription is running".to_string());
    }

    if get_settings(&app).selected_model.is_empty() {
        return Err("No model selected — pick one in the Models tab.".to_string());
    }

    // Claim the batch slot last, so a rejected request never leaves it set.
    if BATCH_RUNNING.swap(true, Ordering::AcqRel) {
        return Err("A batch transcription is already running".to_string());
    }
    BATCH_CANCEL.store(false, Ordering::Release);

    let tm = Arc::clone(&transcription_manager);
    tauri::async_runtime::spawn(async move {
        // Scoped so both guards drop — releasing the batch slot and restoring
        // shortcuts — before `BatchFinished` tells the UI it may start another.
        let cancelled = {
            let _batch_guard = BatchGuard;
            suspend_all_shortcuts(&app);
            let _shortcut_guard = ShortcutGuard(app.clone());

            run_batch(&app, &tm, paths, post_process).await
        };

        if let Err(e) = (FileTranscriptionEvent::BatchFinished { cancelled }).emit(&app) {
            log::error!("Failed to emit BatchFinished: {}", e);
        }
    });

    Ok(())
}

/// Transcribe each file in turn. Returns whether the batch was cancelled.
async fn run_batch(
    app: &AppHandle,
    tm: &Arc<TranscriptionManager>,
    paths: Vec<String>,
    post_process: bool,
) -> bool {
    for path in paths {
        // Inference is synchronous and uninterruptible, so cancellation can only
        // be honoured between files — the in-flight one always finishes.
        if BATCH_CANCEL.load(Ordering::Acquire) {
            return true;
        }

        if let Err(e) = (FileTranscriptionEvent::Started { path: path.clone() }).emit(app) {
            log::error!("Failed to emit Started for {}: {}", path, e);
        }

        // One file failing never aborts the batch — it fails that row only.
        let event = match transcribe_one(app, tm, &path, post_process).await {
            Ok(text) => {
                log::info!("Transcribed {} ({} chars)", path, text.len());
                FileTranscriptionEvent::Completed { path, text }
            }
            Err(error) => {
                log::warn!("Failed to transcribe {}: {}", path, error);
                FileTranscriptionEvent::Failed { path, error }
            }
        };

        if let Err(e) = event.emit(app) {
            log::error!("Failed to emit file transcription result: {}", e);
        }
    }

    false
}

/// Decode → transcribe → post-process a single file.
async fn transcribe_one(
    app: &AppHandle,
    tm: &Arc<TranscriptionManager>,
    path: &str,
    post_process: bool,
) -> Result<String, String> {
    let decode_path = path.to_string();
    let samples = tauri::async_runtime::spawn_blocking(move || decode_audio_file(&decode_path))
        .await
        .map_err(|e| format!("Decode task panicked: {}", e))?
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

    if samples.is_empty() {
        return Err("File contains no audio".to_string());
    }

    // No-op when the model is already loaded. This is also what makes the
    // "unload immediately" timeout work across a batch: `transcribe()` waits on
    // the loading condvar until the reload finishes.
    tm.initiate_model_load();

    let tm = Arc::clone(tm);
    let transcription = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map_err(|e| e.to_string())?;

    if transcription.trim().is_empty() {
        return Err("No speech detected in this file".to_string());
    }

    Ok(
        process_transcription_output(app, &transcription, post_process)
            .await
            .final_text,
    )
}

/// Ask the running batch to stop. Takes effect after the current file finishes.
#[tauri::command]
#[specta::specta]
pub fn cancel_file_transcription() {
    if BATCH_RUNNING.load(Ordering::Acquire) {
        BATCH_CANCEL.store(true, Ordering::Release);
        log::info!("File transcription batch cancellation requested");
    }
}

/// One `.txt` entry in the exported archive. `name` is the source file name,
/// which is sanitised here rather than trusted.
#[derive(Debug, Deserialize, Type)]
pub struct TranscriptExport {
    pub name: String,
    pub text: String,
}

/// Write the completed transcripts to a user-chosen `.zip`.
///
/// Handy never writes next to the user's source audio: nothing lands on disk
/// until the user picks a destination in the save dialog.
#[tauri::command]
#[specta::specta]
pub async fn export_transcripts_zip(
    dest: String,
    entries: Vec<TranscriptExport>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Err("No transcripts to export".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || write_zip(&dest, &entries))
        .await
        .map_err(|e| format!("Export task panicked: {}", e))?
        .map_err(|e| format!("Failed to write archive: {}", e))
}

fn write_zip(dest: &str, entries: &[TranscriptExport]) -> std::io::Result<()> {
    let mut writer = ZipWriter::new(File::create(dest)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // Keyed case-insensitively: "Notes.txt" and "notes.txt" collide once the
    // archive is extracted on a case-insensitive filesystem.
    let mut taken: HashSet<String> = HashSet::new();

    for entry in entries {
        let stem = sanitize_stem(&entry.name);
        let mut file_name = format!("{}.txt", stem);
        let mut suffix = 1;
        while !taken.insert(file_name.to_lowercase()) {
            file_name = format!("{} ({}).txt", stem, suffix);
            suffix += 1;
        }

        writer.start_file(file_name.as_str(), options)?;
        writer.write_all(entry.text.as_bytes())?;
    }

    writer.finish()?;
    Ok(())
}

/// Reduce an arbitrary source file name to a bare, safe archive entry stem.
/// Strips any directory component so an entry can never escape the archive
/// root, drops the extension, and removes characters that are illegal or
/// meaningless in a filename.
fn sanitize_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(base);

    let cleaned: String = stem
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .collect();

    // A leading/trailing dot or an all-dots name ("..") is not a usable stem.
    let cleaned = cleaned.trim().trim_matches('.').trim();

    if cleaned.is_empty() {
        "transcript".to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_directories_and_extensions() {
        assert_eq!(sanitize_stem("/home/user/Music/notes.mp3"), "notes");
        assert_eq!(sanitize_stem(r"C:\Users\me\voice memo.m4a"), "voice memo");
        assert_eq!(sanitize_stem("plain"), "plain");
    }

    #[test]
    fn sanitize_neutralises_traversal_and_illegal_names() {
        assert_eq!(sanitize_stem("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_stem(".."), "transcript");
        assert_eq!(sanitize_stem(""), "transcript");
        assert_eq!(sanitize_stem("a:b*c?.wav"), "abc");
    }

    #[test]
    fn duplicate_names_are_disambiguated() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.zip");
        let entries = vec![
            TranscriptExport {
                name: "/a/notes.mp3".to_string(),
                text: "first".to_string(),
            },
            TranscriptExport {
                name: "/b/notes.mp3".to_string(),
                text: "second".to_string(),
            },
            TranscriptExport {
                name: "/c/NOTES.wav".to_string(),
                text: "third".to_string(),
            },
        ];

        write_zip(dest.to_str().unwrap(), &entries).unwrap();

        let archive = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

        assert_eq!(names.len(), 3, "every transcript should get its own entry");
        assert!(names.contains(&"notes.txt".to_string()));
        assert!(names.contains(&"notes (1).txt".to_string()));
        assert!(names.contains(&"NOTES (2).txt".to_string()));
    }
}
