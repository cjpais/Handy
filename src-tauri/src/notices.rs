//! Notices: how Handy tells the user that something went wrong in the
//! background. [`raise`] is the one way to report such a failure.
//!
//! Don't add a new event, toast or overlay state for a background failure.
//! Things done in the settings window keep their toast or inline state.
//!
//! A dictation notice takes over the overlay when it's raised, so a failure
//! site calls [`raise`] instead of hiding the overlay. If another session owns
//! the overlay by then, call [`raise_without_overlay`].
//!
//! # Adding a new error
//!
//! 1. Add a [`NoticeCode`], with its level in [`NoticeCode::level`] and its
//!    title in [`NoticeCode::tray_title`].
//! 2. Add its title under `errors.*` in `src/i18n/locales/en/translation.json`,
//!    and the same key in every other locale.
//! 3. Map the code to its title, explanation and page in `src/lib/notices.ts`.
//! 4. Call [`raise`] where the failure happens:
//!
//! ```ignore
//! notices::raise(
//!     app,
//!     Notice::new(NoticeCode::PasteFailed, NoticeSource::Dictation),
//! );
//! ```
//!
//! Rust never sends display strings: the frontend maps each code to its text.
//! Use [`Notice::with_detail`] for what only Rust knows, such as a raw error.
//!
//! # Lifetime
//!
//! Handy keeps one notice per [`NoticeSource`], and a newer one replaces it.
//! Call [`clear`] when that source next succeeds. A session clears first and
//! then raises its own, so a warning about a success survives that success.
//! Nothing persists across restarts.

use crate::overlay;
use crate::tray;
use crate::tray_i18n::TrayStrings;
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    /// Nothing came out, or Handy can't do its job.
    Error,
    /// The user got a result, but not the one they set up.
    Warning,
}

/// What a notice belongs to: each source has one slot in the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum NoticeSource {
    Dictation,
    File,
}

/// Every error Handy reports. The frontend maps each one to its text in
/// `src/lib/notices.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum NoticeCode {
    TranscriptionFailed,
    PasteFailed,
    NoMicrophone,
    MicPermissionDenied,
    RecordingFailed,
    ModelLoadFailed,
}

impl NoticeCode {
    fn level(self) -> NoticeLevel {
        match self {
            Self::TranscriptionFailed
            | Self::PasteFailed
            | Self::NoMicrophone
            | Self::MicPermissionDenied
            | Self::RecordingFailed
            | Self::ModelLoadFailed => NoticeLevel::Error,
        }
    }

    /// The title in the tray menu, from the same `errors.*Title` string the
    /// frontend shows (see `build.rs`).
    pub fn tray_title(self, strings: &TrayStrings) -> &str {
        match self {
            Self::TranscriptionFailed => &strings.transcription_failed_title,
            Self::PasteFailed => &strings.paste_failed_title,
            Self::NoMicrophone => &strings.no_input_device_title,
            Self::MicPermissionDenied => &strings.mic_permission_denied_title,
            Self::RecordingFailed => &strings.recording_failed_title,
            Self::ModelLoadFailed => &strings.model_load_failed_title,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Notice {
    pub code: NoticeCode,
    pub level: NoticeLevel,
    pub source: NoticeSource,
    /// A model name or a raw error, shown alongside the translated text.
    pub detail: Option<String>,
}

impl Notice {
    pub fn new(code: NoticeCode, source: NoticeSource) -> Self {
        Self {
            code,
            level: code.level(),
            source,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<Option<String>>) -> Self {
        self.detail = detail.into();
        self
    }
}

/// Emitted whenever the active notices change.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct NoticesChanged {
    /// Every active notice, newest first.
    pub notices: Vec<Notice>,
    /// The notice that caused this change. `None` when one was cleared.
    pub raised: Option<Notice>,
}

/// The active notices, newest first. Managed by Tauri from startup.
#[derive(Default)]
pub struct NoticeStore(Mutex<Vec<Notice>>);

impl NoticeStore {
    fn lock(&self) -> MutexGuard<'_, Vec<Notice>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn insert(active: &mut Vec<Notice>, notice: Notice) {
    remove(active, notice.source);
    active.insert(0, notice);
}

/// Whether `source` had a notice to remove.
fn remove(active: &mut Vec<Notice>, source: NoticeSource) -> bool {
    let before = active.len();
    active.retain(|notice| notice.source != source);
    active.len() != before
}

/// The notice the tray shows: the newest at the highest level.
fn headline(active: &[Notice]) -> Option<&Notice> {
    active
        .iter()
        .find(|notice| notice.level == NoticeLevel::Error)
        .or(active.first())
}

/// Sent under the store's lock, so the last event always matches the store.
fn publish(app: &AppHandle, active: &[Notice], raised: Option<Notice>) {
    let event = NoticesChanged {
        notices: active.to_vec(),
        raised,
    };
    if let Err(e) = event.emit(app) {
        warn!("Failed to emit notices-changed: {e}");
    }
}

/// Report a problem to the user. This is the one way to do it: see the module
/// docs. Replaces the source's previous notice and tells the frontend. A
/// dictation notice also replaces the overlay.
pub fn raise(app: &AppHandle, notice: Notice) {
    if notice.source == NoticeSource::Dictation {
        overlay::show_overlay_notice(app, &notice);
    }
    raise_without_overlay(app, notice);
}

/// [`raise`] for a failure that isn't the session on the overlay, such as a
/// late paste from an earlier one.
pub fn raise_without_overlay(app: &AppHandle, notice: Notice) {
    {
        let store = app.state::<NoticeStore>();
        let mut active = store.lock();
        insert(&mut active, notice.clone());
        publish(app, &active, Some(notice));
    }
    tray::refresh_tray_icon(app);
}

/// Drop `source`'s notice because it has just succeeded.
pub fn clear(app: &AppHandle, source: NoticeSource) {
    let cleared = {
        let store = app.state::<NoticeStore>();
        let mut active = store.lock();
        let cleared = remove(&mut active, source);
        if cleared {
            publish(app, &active, None);
        }
        cleared
    };
    if cleared {
        tray::refresh_tray_icon(app);
    }
}

/// The notice the tray should show, if any.
pub fn tray_headline(app: &AppHandle) -> Option<Notice> {
    let store = app.state::<NoticeStore>();
    let active = store.lock();
    headline(&active).cloned()
}

/// The active notices, newest first, for a window that has just opened.
#[tauri::command]
#[specta::specta]
pub fn get_active_notices(store: State<'_, NoticeStore>) -> Vec<Notice> {
    store.lock().clone()
}

/// A click on the overlay notice. The notice stays active everywhere else.
#[tauri::command]
#[specta::specta]
pub fn dismiss_overlay_notice(app: AppHandle) {
    overlay::dismiss_overlay_notice(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dictation(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::Dictation)
    }

    fn file(code: NoticeCode) -> Notice {
        Notice::new(code, NoticeSource::File)
    }

    #[test]
    fn level_comes_from_the_code() {
        let notice = dictation(NoticeCode::PasteFailed);
        assert_eq!(notice.level, NoticeLevel::Error);
    }

    #[test]
    fn detail_accepts_a_string_or_an_option() {
        let notice = dictation(NoticeCode::RecordingFailed);
        assert_eq!(
            notice.clone().with_detail("raw".to_string()).detail,
            Some("raw".to_string())
        );
        assert_eq!(notice.with_detail(None).detail, None);
    }

    #[test]
    fn a_newer_notice_replaces_the_one_from_its_source() {
        let mut active = Vec::new();
        insert(&mut active, dictation(NoticeCode::PasteFailed));
        insert(&mut active, dictation(NoticeCode::TranscriptionFailed));

        assert_eq!(active, vec![dictation(NoticeCode::TranscriptionFailed)]);
    }

    #[test]
    fn each_source_keeps_its_own_notice_newest_first() {
        let mut active = Vec::new();
        insert(&mut active, file(NoticeCode::TranscriptionFailed));
        insert(&mut active, dictation(NoticeCode::PasteFailed));
        assert_eq!(
            active,
            vec![
                dictation(NoticeCode::PasteFailed),
                file(NoticeCode::TranscriptionFailed),
            ]
        );

        insert(&mut active, file(NoticeCode::RecordingFailed));
        assert_eq!(
            active,
            vec![
                file(NoticeCode::RecordingFailed),
                dictation(NoticeCode::PasteFailed),
            ]
        );
    }

    #[test]
    fn clearing_one_source_leaves_the_other() {
        let mut active = Vec::new();
        insert(&mut active, file(NoticeCode::TranscriptionFailed));
        insert(&mut active, dictation(NoticeCode::PasteFailed));

        assert!(remove(&mut active, NoticeSource::Dictation));
        assert_eq!(active, vec![file(NoticeCode::TranscriptionFailed)]);

        assert!(remove(&mut active, NoticeSource::File));
        assert!(active.is_empty());
    }

    #[test]
    fn clearing_an_empty_source_changes_nothing() {
        let mut active = vec![file(NoticeCode::TranscriptionFailed)];

        assert!(!remove(&mut active, NoticeSource::Dictation));
        assert_eq!(active, vec![file(NoticeCode::TranscriptionFailed)]);
    }

    #[test]
    fn headline_is_the_newest_notice() {
        let mut active = Vec::new();
        assert_eq!(headline(&active), None);

        insert(&mut active, file(NoticeCode::TranscriptionFailed));
        insert(&mut active, dictation(NoticeCode::PasteFailed));
        assert_eq!(headline(&active), active.first());
    }

    #[test]
    fn a_session_that_clears_first_keeps_what_it_raises() {
        let mut active = vec![dictation(NoticeCode::PasteFailed)];

        remove(&mut active, NoticeSource::Dictation);
        insert(&mut active, dictation(NoticeCode::RecordingFailed));

        assert_eq!(active, vec![dictation(NoticeCode::RecordingFailed)]);
    }
}
