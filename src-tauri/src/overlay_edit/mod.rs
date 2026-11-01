//! Session-scoped live corrections. Nothing changes until the user clicks Edit.
pub mod focus;
mod merge;
mod state;

use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::tray::{set_tray_state, TrayIconState};
use focus::PasteTarget;
use serde::Serialize;
use specta::Type;
use state::EditorState;
pub use state::OverlayEditPhase;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

#[derive(Clone, Serialize, Type)]
pub struct OverlayEditSnapshot {
    pub session_id: u64,
    pub phase: OverlayEditPhase,
    pub text: String,
    pub asr_text: String,
    pub revision: u64,
    pub user_revision: u64,
    pub conflict: bool,
    pub paste_requested: bool,
    pub supported: bool,
}

#[derive(Default)]
struct Data {
    editor: EditorState,
    available: bool,
    target: Option<PasteTarget>,
    history_id: Option<i64>,
}

#[derive(Default)]
pub struct OverlayEditor(Mutex<Data>);

impl OverlayEditor {
    fn lock(&self) -> MutexGuard<'_, Data> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn active(&self) -> bool {
        self.lock().editor.active()
    }
    pub fn session_id(&self) -> u64 {
        self.lock().editor.session_id
    }
    pub fn requested(&self, id: u64) -> bool {
        let d = self.lock();
        d.editor.session_id == id && d.editor.active()
    }
    pub fn start(&self, app: &AppHandle, id: u64) {
        // Also keep the recording-start destination for accessibility activation
        // that may focus the overlay before its Edit command arrives.
        let original = app
            .get_webview_window("recording_overlay")
            .and_then(|w| PasteTarget::capture(&w).ok());
        {
            let mut d = self.lock();
            if d.editor.start_session(id) {
                d.available = true;
                d.target = original;
                d.history_id = None;
            }
        }
        self.emit(app);
    }
    fn snapshot_locked(d: &Data) -> OverlayEditSnapshot {
        OverlayEditSnapshot {
            session_id: d.editor.session_id,
            phase: d.editor.phase,
            text: d.editor.text.clone(),
            asr_text: d.editor.asr_text.clone(),
            revision: d.editor.revision,
            user_revision: d.editor.user_revision,
            conflict: d.editor.conflict,
            paste_requested: d.editor.paste_requested,
            supported: focus::supported(),
        }
    }
    fn snapshot(&self) -> OverlayEditSnapshot {
        Self::snapshot_locked(&self.lock())
    }
    fn emit(&self, app: &AppHandle) {
        let snapshot = self.snapshot();
        let _ = app.emit_to("recording_overlay", "overlay-edit-state", snapshot);
    }
    pub fn observe(&self, app: &AppHandle, id: u64, text: String) {
        let emit = {
            let mut d = self.lock();
            d.available && d.editor.observe_preview(id, text) && d.editor.active()
        };
        if emit {
            self.emit(app);
        }
    }
    pub fn stopped(&self, app: &AppHandle, id: u64) {
        {
            let mut d = self.lock();
            if d.editor.session_id == id && d.editor.phase == OverlayEditPhase::Recording {
                let _ = d.editor.request_stop(id, false);
            }
        }
        self.emit(app);
    }
    /// Atomically choose review versus automatic paste, preventing late Edit clicks.
    pub fn finalize(
        &self,
        app: &AppHandle,
        id: u64,
        text: String,
        history_id: Option<i64>,
    ) -> bool {
        let review = {
            let mut d = self.lock();
            if d.editor.session_id != id {
                return false;
            }
            d.available = false;
            if !d.editor.active() {
                return false;
            }
            if d.editor.phase == OverlayEditPhase::Recording {
                let _ = d.editor.request_stop(id, false);
            }
            if !d.editor.finalize(id, text) {
                return false;
            }
            d.history_id = history_id;
            // Finishing audio may add words or punctuation. Always let the user
            // review that final merge before a separate, explicit Paste.
            d.editor.paste_requested = false;
            true
        };
        crate::overlay::show_editing_overlay(app);
        self.emit(app);
        review
    }
    pub fn cancel(&self, app: &AppHandle) {
        let target = {
            let mut d = self.lock();
            let target = if d.editor.active() { d.target } else { None };
            d.available = false;
            d.editor.cancel();
            d.target = None;
            d.history_id = None;
            target
        };
        if let Some(target) = target {
            let _ = target.restore(app);
        }
        reset_focus(app);
        self.emit(app);
    }
    pub fn finish_recording(&self, app: &AppHandle, id: u64) {
        // An ASR error must not discard corrections already typed by the user.
        let recover = {
            let mut d = self.lock();
            if d.editor.session_id != id {
                return;
            }
            d.available = false;
            if matches!(
                d.editor.phase,
                OverlayEditPhase::Recording | OverlayEditPhase::Finalizing
            ) {
                if d.editor.phase == OverlayEditPhase::Recording {
                    let _ = d.editor.request_stop(id, false);
                }
                let asr = d.editor.asr_text.clone();
                d.editor.finalize(id, asr);
                d.editor.conflict = true;
                d.editor.paste_requested = false;
                true
            } else {
                false
            }
        };
        if recover {
            crate::overlay::show_editing_overlay(app);
            self.emit(app);
        }
    }
}

fn check_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "recording_overlay" {
        Ok(())
    } else {
        Err("Only the recording overlay can edit a take".into())
    }
}

fn reset_focus(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("recording_overlay") {
        let _ = w.set_focusable(false);
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_overlay_edit_state(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<OverlayEditSnapshot, String> {
    check_window(&window)?;
    Ok(app.state::<OverlayEditor>().snapshot())
}

#[tauri::command]
#[specta::specta]
pub fn begin_overlay_edit(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<OverlayEditSnapshot, String> {
    check_window(&window)?;
    if crate::settings::get_settings(&app).overlay_style != crate::settings::OverlayStyle::Live {
        return Err("Live overlay is not enabled".into());
    }
    let editor = app.state::<OverlayEditor>();
    let target =
        PasteTarget::capture(&window).or_else(|error| editor.lock().target.ok_or(error))?;
    let recording = app
        .try_state::<std::sync::Arc<AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording());
    {
        let mut d = editor.lock();
        if !d.available {
            return Err("This take is no longer available for editing".into());
        }
        let id = d.editor.session_id;
        d.editor.request(id, recording)?;
        d.target = Some(target);
    }
    crate::overlay::show_editing_overlay(&app);
    if let Err(e) = window.set_focusable(true).and_then(|()| window.set_focus()) {
        editor.cancel(&app);
        return Err(e.to_string());
    }
    editor.emit(&app);
    Ok(editor.snapshot())
}

#[tauri::command]
#[specta::specta]
pub fn update_overlay_edit_draft(
    app: AppHandle,
    window: WebviewWindow,
    session_id: u64,
    user_revision: u64,
    base_asr: String,
    text: String,
) -> Result<(), String> {
    check_window(&window)?;
    let editor = app.state::<OverlayEditor>();
    let changed = editor
        .lock()
        .editor
        .update(session_id, user_revision, base_asr, text)?;
    if changed {
        editor.emit(&app);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn submit_overlay_edit(
    app: AppHandle,
    window: WebviewWindow,
    session_id: u64,
    user_revision: u64,
    base_asr: String,
    text: String,
) -> Result<(), String> {
    check_window(&window)?;
    let phase = {
        let editor = app.state::<OverlayEditor>();
        let mut d = editor.lock();
        if d.editor.session_id != session_id {
            return Err("Editor session is stale".into());
        }
        let phase = d.editor.phase;
        if phase == OverlayEditPhase::Recording || phase == OverlayEditPhase::Finalizing {
            if d.editor.paste_requested {
                return Err("Paste is already pending".into());
            }
            d.editor
                .update(session_id, user_revision, base_asr.clone(), text.clone())?;
            if phase == OverlayEditPhase::Recording {
                d.editor.request_stop(session_id, true)?;
            } else {
                d.editor.paste_requested = true;
                d.editor.revision = d.editor.revision.saturating_add(1);
            }
        }
        phase
    };
    if phase == OverlayEditPhase::Recording || phase == OverlayEditPhase::Finalizing {
        app.state::<OverlayEditor>().emit(&app);
        if phase == OverlayEditPhase::Recording {
            app.state::<crate::TranscriptionCoordinator>()
                .stop_for_edit();
        }
        Ok(())
    } else {
        paste_review(app, session_id, user_revision, base_asr, text).await
    }
}

async fn paste_review(
    app: AppHandle,
    id: u64,
    revision: u64,
    base_asr: String,
    text: String,
) -> Result<(), String> {
    let (target, submitted, history_id) = {
        let editor = app.state::<OverlayEditor>();
        let mut d = editor.lock();
        let target = d.target.ok_or("No destination captured")?;
        if d.editor.phase == OverlayEditPhase::Review && base_asr != d.editor.asr_text {
            d.editor.update(id, revision, base_asr, text)?;
            drop(d);
            editor.emit(&app);
            return Err("Latest recognition changed; review the updated draft".into());
        }
        d.editor.begin_paste(id, revision, base_asr, text)?;
        (target, d.editor.text.clone(), d.history_id)
    };
    app.state::<OverlayEditor>().emit(&app);
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        // Cancel can race the request before this worker begins.
        if !handle.state::<OverlayEditor>().requested(id) {
            return Err("Editor was cancelled".into());
        }
        if let Some(history_id) = history_id {
            if let Err(error) = handle
                .state::<std::sync::Arc<HistoryManager>>()
                .update_review_text(history_id, submitted.clone())
            {
                // History may be disabled or the row removed while reviewing.
                // Keep the existing best-effort history policy for dictation.
                log::warn!("Could not save reviewed text to history: {error}");
            }
        }
        if !submitted.is_empty() {
            crate::clipboard::paste_to_target(submitted, handle.clone(), target, id)?;
        } else {
            target.restore(&handle)?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "Paste worker failed".to_string())
    .and_then(|r| r);
    let editor = app.state::<OverlayEditor>();
    if let Err(error) = result {
        log::warn!("Reviewed paste failed for session {id}: {error}");
        let retained = editor.lock().editor.paste_failed(id);
        if retained {
            editor.emit(&app);
            crate::overlay::show_editing_overlay(&app);
        }
        return Err(error);
    }
    let completed = editor.lock().editor.complete(id);
    if completed {
        reset_focus(&app);
        editor.emit(&app);
        crate::overlay::hide_recording_overlay(&app);
        set_tray_state(&app, TrayIconState::Idle);
    }
    Ok(())
}
