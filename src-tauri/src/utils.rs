use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::shortcut;
use crate::TranscriptionCoordinator;
use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// Re-export all utility modules for easy access
// pub use crate::audio_feedback::*;
pub use crate::clipboard::*;
pub use crate::overlay::*;
pub use crate::tray::*;

/// Centralized cancellation function that can be called from anywhere in the app.
/// Handles cancelling both recording and transcription operations and updates UI state.
pub fn cancel_current_operation(app: &AppHandle) {
    info!("Initiating operation cancellation...");

    // Unregister the cancel shortcut asynchronously
    shortcut::unregister_cancel_shortcut(app);

    // Cancel any ongoing recording
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    let recording_was_active = audio_manager.is_recording();
    audio_manager.cancel_recording();

    // Abandon any live streaming transcription
    let tm = app.state::<Arc<TranscriptionManager>>();
    tm.cancel_stream();

    // Update tray icon and hide overlay
    change_tray_icon(app, crate::tray::TrayIconState::Idle);
    hide_recording_overlay(app);

    // Unload model if immediate unload is enabled
    tm.maybe_unload_immediately("cancellation");

    // Notify coordinator so it can keep lifecycle state coherent.
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_cancel(recording_was_active);
    }

    info!("Operation cancellation completed - returned to idle state");
}

/// Check if using the Wayland display server protocol
#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false)
}

/// Check if running on KDE Plasma desktop environment
#[cfg(target_os = "linux")]
pub fn is_kde_plasma() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_uppercase().contains("KDE"))
        .unwrap_or(false)
        || std::env::var("KDE_SESSION_VERSION").is_ok()
}

/// Check if running on KDE Plasma with Wayland
#[cfg(target_os = "linux")]
pub fn is_kde_wayland() -> bool {
    is_wayland() && is_kde_plasma()
}

/// Name of the frontmost application (the dictation target). Used for the
/// `${app}` post-processing prompt variable so prompts can adapt tone to the
/// target app.
#[cfg(target_os = "macos")]
pub fn frontmost_app_name() -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    app.localizedName().map(|name| name.to_string())
}

// ponytail: macOS only; Windows (GetForegroundWindow) / Linux when needed
#[cfg(not(target_os = "macos"))]
pub fn frontmost_app_name() -> Option<String> {
    None
}

/// Selected text and full text value of the focused UI element, read via the
/// Accessibility API. Uses the same TCC Accessibility grant the app already
/// requires for shortcuts; any failure (trust revoked, no focused element,
/// non-text element) yields None and callers fall back to the clipboard path.
#[cfg(target_os = "macos")]
pub fn ax_focused_texts() -> (Option<String>, Option<String>) {
    use objc2_application_services::{AXError, AXUIElement};
    use objc2_core_foundation::{CFRetained, CFString, CFType};
    use std::ptr::NonNull;

    fn copy_attr(el: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
        let attr = CFString::from_str(name);
        let mut value: *const CFType = std::ptr::null();
        let err = unsafe { el.copy_attribute_value(&attr, NonNull::from(&mut value)) };
        if err != AXError::Success {
            return None;
        }
        NonNull::new(value.cast_mut()).map(|v| unsafe { CFRetained::from_raw(v) })
    }

    fn as_text(v: CFRetained<CFType>) -> Option<String> {
        let s = v.downcast::<CFString>().ok()?.to_string();
        if s.trim().is_empty() {
            None
        } else {
            Some(s)
        }
    }

    let system = unsafe { AXUIElement::new_system_wide() };
    let Some(focused) =
        copy_attr(&system, "AXFocusedUIElement").and_then(|v| v.downcast::<AXUIElement>().ok())
    else {
        return (None, None);
    };
    let selected = copy_attr(&focused, "AXSelectedText").and_then(as_text);
    // ponytail: no size cap on the field value; truncate if giant text views
    // ever blow the LLM context
    let value = copy_attr(&focused, "AXValue").and_then(as_text);
    (selected, value)
}

#[cfg(not(target_os = "macos"))]
pub fn ax_focused_texts() -> (Option<String>, Option<String>) {
    (None, None) // ponytail: clipboard fallback covers other platforms
}
