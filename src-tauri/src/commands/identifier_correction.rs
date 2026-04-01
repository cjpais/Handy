//! Tauri commands for the identifier correction feature.

use crate::identifier_correction::IdentifierCorrectionManager;
use crate::settings::{get_settings, write_settings};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Resolve a pending picker request from the frontend.
///
/// Called by the picker overlay when the user has selected a replacement for
/// each ambiguous token in the batch.
///
/// * `request_id` – the ID from the `IdentifierPickNeededEvent`.
/// * `selections`  – map of `{ original_token: chosen_replacement }`.
#[tauri::command]
#[specta::specta]
pub fn confirm_identifier_pick(
    correction_manager: State<Arc<IdentifierCorrectionManager>>,
    request_id: String,
    selections: HashMap<String, String>,
) -> Result<(), String> {
    correction_manager.resolve_pick(&request_id, selections);
    Ok(())
}

/// Rebuild the symbol index from the currently configured project root.
///
/// Returns the number of symbols indexed, or an error if no project root is set.
#[tauri::command]
#[specta::specta]
pub fn rebuild_identifier_index(
    app: AppHandle,
    correction_manager: State<Arc<IdentifierCorrectionManager>>,
) -> Result<usize, String> {
    let settings = get_settings(&app);
    let root = settings
        .identifier_correction_project_root
        .ok_or("No project root configured")?;
    let path = PathBuf::from(&root);
    if !path.is_dir() {
        return Err(format!("Project root does not exist or is not a directory: {}", root));
    }
    let count = correction_manager.rebuild_index(&path);
    Ok(count)
}

/// Persist updated identifier correction settings and optionally trigger a
/// re-index if the project root changed.
#[tauri::command]
#[specta::specta]
pub fn set_identifier_correction_settings(
    app: AppHandle,
    correction_manager: State<Arc<IdentifierCorrectionManager>>,
    enabled: bool,
    project_root: Option<String>,
    threshold: f64,
) -> Result<usize, String> {
    let mut settings = get_settings(&app);

    let root_changed = settings.identifier_correction_project_root != project_root;
    settings.identifier_correction_enabled = enabled;
    settings.identifier_correction_project_root = project_root.clone();
    settings.identifier_correction_threshold = threshold.clamp(0.0, 1.0);
    write_settings(&app, settings);

    // Re-index if the root changed and we now have a valid path.
    if root_changed {
        if let Some(root) = project_root {
            let path = PathBuf::from(&root);
            if path.is_dir() {
                let count = correction_manager.rebuild_index(&path);
                return Ok(count);
            }
        }
    }

    Ok(correction_manager.symbol_count())
}

/// Return how many symbols are currently in the index.
#[tauri::command]
#[specta::specta]
pub fn get_identifier_index_size(
    correction_manager: State<Arc<IdentifierCorrectionManager>>,
) -> usize {
    correction_manager.symbol_count()
}
