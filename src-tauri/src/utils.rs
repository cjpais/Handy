use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::shortcut;
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
/// Handles cancelling both recording and transcription operations and updates UI state.
pub fn cancel_current_operation(app: &AppHandle) {
    info!("Initiating operation cancellation...");

    // Unregister the cancel shortcut asynchronously
    shortcut::unregister_cancel_shortcut(app);

    // First, reset all shortcut toggle states.
    // This is critical for non-push-to-talk mode where shortcuts toggle on/off
    let toggle_state_manager = app.state::<ManagedToggleState>();
    if let Ok(mut states) = toggle_state_manager.lock() {
        states.active_toggles.values_mut().for_each(|v| *v = false);
    } else {
        warn!("Failed to lock toggle state manager during cancellation");
    }

    // Cancel any ongoing recording
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    audio_manager.cancel_recording();

    // Update tray icon and hide overlay
    change_tray_icon(app, crate::tray::TrayIconState::Idle);
    hide_recording_overlay(app);

    info!("Operation cancellation completed - returned to idle state");
}

/// Stops transcription when silence timeout is exceeded.
pub fn trigger_auto_stop_transcription(app: &AppHandle) {
    let binding_id = "transcribe";
    let shortcut_string = "auto-stop-silence";

    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    if !audio_manager.is_recording() {
        return;
    }

    let toggle_state_manager = app.state::<ManagedToggleState>();
    if let Ok(mut states) = toggle_state_manager.lock() {
        if let Some(is_active) = states.active_toggles.get_mut(binding_id) {
            if !*is_active {
                return;
            }
            *is_active = false;
        }
    } else {
        warn!("Auto-stop: Failed to lock toggle state");
        return;
    }

    if let Some(action) = ACTION_MAP.get(binding_id) {
        action.stop(app, binding_id, shortcut_string);
        info!("Auto-stop: Transcription stopped due to silence");
    }
}

/// Check if using the Wayland display server protocol
#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false)
}
