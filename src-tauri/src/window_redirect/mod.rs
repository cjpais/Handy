//! Redirecting dictation into a window the user picked.
//!
//! Normally a transcript is pasted wherever the keyboard focus is. With a
//! redirect target set, Handy delivers it to that window instead, in the
//! background: the user's focus never moves. The target is picked by
//! clicking it on screen (see [`picker`]) and kept until the user stops the
//! redirect, the window goes away, or Handy quits; nothing is persisted.
//!
//! The platform half lives in one module per OS and provides:
//!
//! - `WindowHandle`: how the OS identifies a window. `Clone`, `Debug`,
//!   `PartialEq`, `Send` and `Sync`. Tests get one from `test_handle()`.
//! - `SUPPORTED`: whether this build can redirect at all. When it's `false`
//!   the tray hides the feature.
//! - `remember_front()` / `restore_front()`: the application in front when
//!   the picker opens gets its activity back when the picker closes, since a
//!   click on the picker's panel makes Handy active.
//! - `windows_at(x, y)`: the windows under a screen point, front to back,
//!   for the picker's highlight. Cheap: it's called as the pointer moves.
//! - `pick(x, y, depth)`: the one at `depth` of them as a redirect target.
//! - `is_focused(&WindowHandle)`: the target already has the keyboard focus,
//!   so the ordinary paste reaches it.
//! - `prepare(&WindowHandle)`: checks the window is still there and makes it
//!   the one its application types into, without activating the application.
//!   Returns a `Previous` for `restore`, which gives the application back
//!   the window it was typing into before.
//! - `hand_text(&WindowHandle, &str, submit)`: an application's own way of
//!   taking text for a window, for applications that drop keyboard input
//!   while in the background (iTerm2). `Ok(None)` when it has none; otherwise
//!   a word on the way it went in, for the log.
//! - `post_text`, `post_chord`, `post_submit`: keyboard input addressed to
//!   the window's process alone, so the application in front never sees it.
//!
//! Only macOS is implemented so far; the other platforms compile to a backend
//! whose `SUPPORTED` is false, so the tray hides the feature.

use crate::clipboard;
use crate::settings::{get_settings, AppSettings, ClipboardHandling, PasteMethod};
use log::{info, warn};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub mod picker;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
use unsupported as platform;

pub use platform::WindowHandle;

/// One window the user can pick as a redirect target.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    pub handle: WindowHandle,
    /// The owning application's name, as the OS shows it.
    pub app_name: String,
    /// The window title; empty when the window has none.
    pub title: String,
}

impl WindowInfo {
    /// `App — Title`, or just `App` for an untitled window.
    pub fn label(&self) -> String {
        if self.title.is_empty() {
            self.app_name.clone()
        } else {
            format!("{} — {}", self.app_name, self.title)
        }
    }
}

/// A window as the picker sees it on screen: whose it is and where, in
/// logical screen coordinates with the origin at the top left.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenWindow {
    pub app_name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Why the text couldn't be delivered to the target.
#[derive(Debug)]
pub enum DeliverError {
    /// The window or its application is gone.
    WindowGone,
    /// The OS refused; the window may still exist.
    Failed(String),
}

impl std::fmt::Display for DeliverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowGone => write!(f, "the window is gone"),
            Self::Failed(reason) => write!(f, "{reason}"),
        }
    }
}

/// Tauri managed state: the current target.
#[derive(Default)]
pub struct RedirectState {
    target: Mutex<Option<WindowInfo>>,
}

impl RedirectState {
    fn target(&self) -> MutexGuard<'_, Option<WindowInfo>> {
        self.target.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, RedirectState>> {
    app.try_state::<RedirectState>()
}

pub fn supported() -> bool {
    platform::SUPPORTED
}

pub fn target(app: &AppHandle) -> Option<WindowInfo> {
    state(app).and_then(|s| s.target().clone())
}

/// Redirects dictation to `window` from now on.
pub fn start(app: &AppHandle, window: WindowInfo) {
    let Some(state) = state(app) else { return };
    info!("Redirecting dictation to {}", window.label());
    *state.target() = Some(window);
    crate::tray::rebuild_tray_menu(app);
}

/// Dictation goes back to wherever the focus is.
pub fn stop(app: &AppHandle) {
    let Some(state) = state(app) else { return };
    if state.target().take().is_some() {
        info!("Dictation redirect stopped");
    }
    crate::tray::rebuild_tray_menu(app);
}

/// What [`deliver`] did with a transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// No redirect applies: the ordinary paste is the way to type it.
    NotRedirected,
    /// The text is in the target window.
    Delivered,
    /// There is a target but the text couldn't reach it. The ordinary paste
    /// is still owed, and the user should be told the text went elsewhere.
    Lost,
}

/// A beat between the text and the auto-submit key, as the ordinary paste
/// leaves too.
const BEFORE_SUBMIT: Duration = Duration::from_millis(50);
/// How long posted keys get to reach the target window before its
/// application is pointed back at the window it was typing into: the keys
/// are queued, and the application reads them when its event loop gets to
/// them.
const AFTER_KEYS: Duration = Duration::from_millis(200);

/// Delivers `text` to the redirect target, if there is one, without moving
/// the user's focus. A target that is gone drops the redirect, so the user
/// isn't left dictating into the void; one that merely refuses now (a stalled
/// application) is kept for the next dictation.
pub fn deliver(app: &AppHandle, text: &str) -> Delivery {
    let Some(window) = target(app) else {
        return Delivery::NotRedirected;
    };
    let settings = get_settings(app);
    // Nothing to redirect: `None` types nothing and a script decides itself.
    if matches!(
        settings.paste_method,
        PasteMethod::None | PasteMethod::ExternalScript
    ) {
        return Delivery::NotRedirected;
    }
    if platform::is_focused(&window.handle) {
        return Delivery::NotRedirected;
    }
    // Serialised with the ordinary paste: both use the clipboard.
    let _paste = clipboard::paste_lock();
    let text = if settings.append_trailing_space {
        format!("{text} ")
    } else {
        text.to_string()
    };
    match deliver_to(app, &window.handle, &text, &settings) {
        Ok(how) => {
            info!("Delivered dictation to {} as {how}", window.label());
            Delivery::Delivered
        }
        Err(DeliverError::WindowGone) => {
            warn!(
                "Redirect target {} is gone; redirect stopped",
                window.label()
            );
            stop(app);
            Delivery::Lost
        }
        Err(e) => {
            warn!(
                "Redirect target {} didn't take the text ({e}); pasting where the focus is",
                window.label()
            );
            Delivery::Lost
        }
    }
}

/// An application's own way in first, where it has one; otherwise the text
/// goes in as keyboard input addressed to the window's process, following the
/// paste method the user set up for the ordinary paste. Returns a word on
/// the way it went in. (Setting the focused element's `AXSelectedText`
/// looked cleaner, but terminals report success and insert nothing.)
fn deliver_to(
    app: &AppHandle,
    window: &WindowHandle,
    text: &str,
    settings: &AppSettings,
) -> Result<String, DeliverError> {
    let previous = platform::prepare(window)?;
    let how = match platform::hand_text(window, text, settings.auto_submit)? {
        Some(how) => how.to_string(),
        None => {
            match settings.paste_method {
                PasteMethod::Direct => platform::post_text(window, text)?,
                method => clipboard::with_text_on_clipboard(
                    app,
                    text,
                    settings.paste_delay_ms,
                    settings.paste_delay_after_ms,
                    || platform::post_chord(app, window, method).map_err(|e| e.to_string()),
                )
                .map_err(DeliverError::Failed)?,
            }
            // The text is in the target now. What fails from here is a
            // warning, as in the ordinary paste; an error would have it
            // pasted a second time.
            if settings.auto_submit {
                std::thread::sleep(BEFORE_SUBMIT);
                if let Err(e) = platform::post_submit(window, settings.auto_submit_key) {
                    warn!("Delivered, but auto-submit failed: {e}");
                }
            }
            std::thread::sleep(AFTER_KEYS);
            format!("{:?} to its process", settings.paste_method)
        }
    };
    platform::restore(previous);
    if settings.clipboard_handling == ClipboardHandling::CopyToClipboard {
        if let Err(e) = clipboard::write_text_to_clipboard(app, text) {
            warn!("Delivered, but copying to the clipboard failed: {e}");
        }
    }
    Ok(how)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(app_name: &str, title: &str) -> WindowInfo {
        WindowInfo {
            handle: platform::test_handle(),
            app_name: app_name.to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn label_joins_app_and_title() {
        assert_eq!(window("Terminal", "~/dev").label(), "Terminal — ~/dev");
    }

    #[test]
    fn untitled_window_is_labelled_by_its_app() {
        assert_eq!(window("Terminal", "").label(), "Terminal");
    }
}
