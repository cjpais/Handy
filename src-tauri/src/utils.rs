use crate::managers::audio::AudioRecordingManager;
use crate::ManagedToggleState;
use log::{info, warn};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// Re-export all utility modules for easy access
// pub use crate::audio_feedback::*;
pub use crate::clipboard::*;
pub use crate::overlay::*;
pub use crate::tray::*;

/// Centralized cancellation function that can be called from anywhere in the app.
/// Handles cancelling recording operations and updates UI state.
///
/// IMPORTANT: This function discards the recording without transcribing.
/// It does NOT call action.stop() because that would trigger transcription.
pub fn cancel_current_operation(app: &AppHandle) {
    info!("Cancelling current operation");

    // FIRST: Cancel any ongoing recording (BEFORE touching toggle states!)
    // This ensures audio is discarded and state is set to Idle.
    // Must happen first so that if TranscribeAction.stop() is called later
    // (e.g., user releases PTT key), stop_recording() returns None.
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    audio_manager.cancel_recording();

    // Remove any applied mute (in case mute-while-recording was enabled)
    audio_manager.remove_mute();

    // Reset all shortcut toggle states WITHOUT calling action.stop()
    // We intentionally don't call action.stop() because that would trigger
    // transcription - we want to discard, not complete.
    let toggle_state_manager = app.state::<ManagedToggleState>();
    if let Ok(mut states) = toggle_state_manager.lock() {
        states.active_toggles.values_mut().for_each(|v| *v = false);
    } else {
        warn!("Failed to lock toggle state manager during cancellation");
    }

    // Hide the recording/transcribing overlay
    hide_recording_overlay(app);

    // Update tray icon to idle state
    change_tray_icon(app, TrayIconState::Idle);

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
