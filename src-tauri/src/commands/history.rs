use crate::managers::history::{HistoryEntry, HistoryManager};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

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
    let path = history_manager
        .get_audio_file_path(&file_name)
        .map_err(|e| e.to_string())?;
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

/// Summary returned to the frontend after moving recordings between directories.
#[derive(Serialize, Deserialize, Type, Debug)]
pub struct SetRecordingsDirResult {
    pub moved: usize,
    pub skipped: usize,
    pub failed: usize,
}

fn move_wav_files_between_dirs(old_dir: &Path, new_dir: &Path) -> Result<SetRecordingsDirResult, String> {
    let mut result = SetRecordingsDirResult {
        moved: 0,
        skipped: 0,
        failed: 0,
    };

    let entries = std::fs::read_dir(old_dir)
        .map_err(|e| format!("Failed to read recordings directory: {}", e))?;

    for entry in entries.flatten() {
        let src = entry.path();
        if src.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }

        let file_name = match src.file_name() {
            Some(n) => n,
            None => continue,
        };
        let dest = new_dir.join(file_name);
        if dest.exists() {
            result.skipped += 1;
            continue;
        }

        // Try atomic rename first (same filesystem); fall back to
        // copy + delete for cross-device moves.
        match std::fs::rename(&src, &dest) {
            Ok(_) => result.moved += 1,
            Err(_) => match std::fs::copy(&src, &dest) {
                Ok(_) => {
                    std::fs::remove_file(&src).ok();
                    result.moved += 1;
                }
                Err(_) => result.failed += 1,
            },
        }
    }

    Ok(result)
}

/// Set (or clear) the custom recordings directory.
///
/// - `path = Some(...)` activates a custom folder.
/// - `path = None` reverts to the default `<app_data_dir>/recordings`.
/// - When `move_existing = true` the existing `.wav` files are moved from the
///   old effective directory to the new one. `history.db` is never touched.
#[tauri::command]
#[specta::specta]
pub async fn set_recordings_directory(
    app: AppHandle,
    path: Option<String>,
    move_existing: bool,
) -> Result<SetRecordingsDirResult, String> {
    // Snapshot the old effective directory *before* writing the new setting.
    let old_dir = crate::settings::resolve_recordings_dir(&app)?;

    // Resolve and validate the new target directory.
    let new_dir: PathBuf = if let Some(ref p) = path {
        let candidate = PathBuf::from(p);

        // Reject empty string
        if p.trim().is_empty() {
            return Err("Recordings directory path must not be empty.".to_string());
        }

        // Create it if needed
        std::fs::create_dir_all(&candidate)
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        // Verify it is writable
        let test = candidate.join(".handy_write_test");
        std::fs::write(&test, b"")
            .map_err(|e| format!("Directory is not writable: {}", e))?;
        std::fs::remove_file(&test).ok();

        candidate
    } else {
        // Default path
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to get app data directory: {}", e))?;
        let default_dir = app_data_dir.join("recordings");
        std::fs::create_dir_all(&default_dir)
            .map_err(|e| format!("Failed to create default recordings directory: {}", e))?;
        default_dir
    };

    // Persist the new setting.
    let mut settings = crate::settings::get_settings(&app);
    settings.recordings_custom_dir = path;
    crate::settings::write_settings(&app, settings);

    // Move .wav files if requested and the directories differ.
    let mut result = SetRecordingsDirResult {
        moved: 0,
        skipped: 0,
        failed: 0,
    };

    if move_existing && old_dir != new_dir && old_dir.exists() {
        result = move_wav_files_between_dirs(&old_dir, &new_dir)?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn move_wav_files_between_dirs_moves_only_wav_and_skips_conflicts() {
        let old_dir = TempDir::new().expect("create old dir");
        let new_dir = TempDir::new().expect("create new dir");

        let wav_to_move = old_dir.path().join("a.wav");
        let wav_conflict = old_dir.path().join("b.wav");
        let not_audio = old_dir.path().join("notes.txt");
        let conflict_target = new_dir.path().join("b.wav");

        std::fs::write(&wav_to_move, b"a").expect("write a.wav");
        std::fs::write(&wav_conflict, b"b").expect("write b.wav");
        std::fs::write(&not_audio, b"t").expect("write notes.txt");
        std::fs::write(&conflict_target, b"existing").expect("write conflict file");

        let result = move_wav_files_between_dirs(old_dir.path(), new_dir.path())
            .expect("move wav files");

        assert_eq!(result.moved, 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.failed, 0);

        assert!(!wav_to_move.exists());
        assert!(new_dir.path().join("a.wav").exists());
        assert!(wav_conflict.exists());
        assert!(not_audio.exists());
    }

    #[test]
    fn move_wav_files_between_dirs_returns_error_for_non_directory_source() {
        let old_dir_file = TempDir::new().expect("create dir");
        let src_file = old_dir_file.path().join("not-a-dir");
        std::fs::write(&src_file, b"x").expect("write source file");

        let new_dir = TempDir::new().expect("create destination dir");

        let err = move_wav_files_between_dirs(&src_file, new_dir.path())
            .expect_err("expected read_dir to fail for file path");
        assert!(err.contains("Failed to read recordings directory"));
    }
}
