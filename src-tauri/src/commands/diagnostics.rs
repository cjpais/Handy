use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use specta::Type;
use tauri::{AppHandle, Manager};

use crate::diagnostics::{self, ConfigSnapshot};
use crate::managers::model::ModelManager;
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::settings::{get_settings, AppSettings, PasteMethod, ShortcutActivation};

/// Capture status as shown in the UI. Mirrors `diagnostics::Status` without
/// tying the recorder module to Tauri types.
#[derive(Debug, Clone, Copy, serde::Serialize, Type)]
pub struct DiagnosticsStatus {
    pub active: bool,
    pub started_at_ms: Option<u64>,
    pub events: u64,
    pub dropped: u64,
}

impl From<diagnostics::Status> for DiagnosticsStatus {
    fn from(s: diagnostics::Status) -> Self {
        DiagnosticsStatus {
            active: s.active,
            started_at_ms: s.started_at_ms,
            events: s.events,
            dropped: s.dropped,
        }
    }
}

// These commands touch the filesystem and `stop` waits on the writer, so
// they are async and keep the blocking work off both the main thread and
// the tokio workers.
#[tauri::command]
#[specta::specta]
pub async fn start_diagnostics(app: AppHandle) -> Result<DiagnosticsStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = crate::portable::app_data_dir(&app)
            .map_err(|e| format!("Failed to resolve app data directory: {e}"))?
            .join("diagnostics");
        let settings = get_settings(&app);
        let model_manager = app.state::<Arc<ModelManager>>();
        let config = config_snapshot(&settings, &model_manager);
        diagnostics::start(&dir, config)
            .map(DiagnosticsStatus::from)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub async fn stop_diagnostics(destination: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = diagnostics::stop().map_err(|e| e.to_string())?;
        let Some(destination) = destination else {
            return Ok(path.to_string_lossy().to_string());
        };
        let dest = PathBuf::from(&destination);
        // Move the finished trace to the user's chosen location. If the move
        // fails on both attempts the file stays in app data and the error
        // says where, so a completed capture is never lost.
        match fs::rename(&path, &dest) {
            Ok(()) => Ok(dest.to_string_lossy().to_string()),
            Err(rename_err) => match fs::copy(&path, &dest) {
                Ok(_) => {
                    if let Err(remove_err) = fs::remove_file(&path) {
                        // The trace reached its destination; a leftover copy
                        // at the source is a warning, not a failure.
                        log::warn!(
                            "diagnostics: copied trace to {} but removing the original failed: {remove_err}",
                            path.display()
                        );
                    }
                    Ok(dest.to_string_lossy().to_string())
                }
                Err(copy_err) => {
                    // A half-written file at the destination is worse than
                    // none; remove it and keep the original.
                    let _ = fs::remove_file(&dest);
                    Err(format!(
                        "Failed to move trace to {destination}: rename: {rename_err}; copy: {copy_err}. The file is kept at {}",
                        path.display()
                    ))
                }
            },
        }
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub async fn diagnostics_status() -> DiagnosticsStatus {
    diagnostics::status().into()
}

fn config_snapshot(settings: &AppSettings, model_manager: &ModelManager) -> ConfigSnapshot {
    let info = model_manager.get_model_info(&settings.selected_model);
    let streaming = info.as_ref().is_some_and(|i| i.supports_streaming);
    // The same expected-mode computation the start path makes before the
    // model loads (actions.rs): long-form accuracy is unknown then.
    let mode = live_decode(
        ModelTraits {
            streaming,
            long_form: false,
        },
        settings.long_form_chunking,
    );
    ConfigSnapshot {
        // Catalog id, or "custom" so custom model file names never leak
        // into a trace. Unknown model resolves to "unknown".
        model: match &info {
            Some(i) if i.is_custom => "custom".to_string(),
            Some(i) => i.id.clone(),
            None => "unknown".to_string(),
        },
        live_decode: live_decode_name(mode),
        paste_method: paste_method_name(settings.paste_method),
        push_to_talk: matches!(settings.shortcut_activation, ShortcutActivation::PushToTalk),
        post_process_enabled: settings.post_process_enabled,
        vad: settings.vad_enabled,
    }
}

fn live_decode_name(mode: LiveDecode) -> String {
    match mode {
        LiveDecode::Stream => "stream",
        LiveDecode::Chunked => "chunked",
        LiveDecode::None => "none",
    }
    .to_string()
}

fn paste_method_name(method: PasteMethod) -> String {
    match method {
        PasteMethod::CtrlV => "CtrlV",
        PasteMethod::Direct => "Direct",
        PasteMethod::None => "None",
        PasteMethod::ShiftInsert => "ShiftInsert",
        PasteMethod::CtrlShiftV => "CtrlShiftV",
        PasteMethod::ExternalScript => "ExternalScript",
    }
    .to_string()
}
