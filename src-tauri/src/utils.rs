use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::ManagedToggleState;
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
    println!("Initiating operation cancellation...");

    // First, reset all shortcut toggle states and call stop actions
    // This is critical for non-push-to-talk mode where shortcuts toggle on/off
    let toggle_state_manager = app.state::<ManagedToggleState>();
    if let Ok(mut states) = toggle_state_manager.lock() {
        // For each currently active toggle, call its stop action and reset state
        let active_bindings: Vec<String> = states
            .active_toggles
            .iter()
            .filter(|(_, &is_active)| is_active)
            .map(|(binding_id, _)| binding_id.clone())
            .collect();

        for binding_id in active_bindings {
            println!("Stopping active action for binding: {}", binding_id);

            // Retrieve the language for this binding
            let language = states.active_languages.get(&binding_id).cloned();

            // Call the action's stop method to ensure proper cleanup
            if let Some(action) = ACTION_MAP.get(&binding_id) {
                action.stop(app, &binding_id, "cancelled", language.flatten());
            }

            // Reset the toggle state
            if let Some(is_active) = states.active_toggles.get_mut(&binding_id) {
                *is_active = false;
            }
            // Clean up language state
            states.active_languages.remove(&binding_id);
        }
    } else {
        eprintln!("Warning: Failed to lock toggle state manager during cancellation");
    }

    // Cancel any ongoing recording
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    audio_manager.cancel_recording();

    // Update tray icon and menu to idle state
    change_tray_icon(app, crate::tray::TrayIconState::Idle);

    println!("Operation cancellation completed - returned to idle state");
}
