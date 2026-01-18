//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::warn;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::ManagedToggleState;

/// Handle a shortcut event from either implementation.
///
/// This function contains the shared logic for:
/// - Looking up the action in ACTION_MAP
/// - Handling the cancel binding (only fires when recording)
/// - Handling push-to-talk mode (start on press, stop on release)
/// - Handling toggle mode (toggle state on press only)
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `binding_id` - The ID of the binding (e.g., "transcribe", "cancel")
/// * `hotkey_string` - The string representation of the hotkey
/// * `is_pressed` - Whether this is a key press (true) or release (false)
pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    let settings = get_settings(app);

    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', Pressed: {}",
            binding_id, hotkey_string, is_pressed
        );
        return;
    };

    // Cancel binding: only fires when recording and key is pressed
    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        if audio_manager.is_recording() && is_pressed {
            action.start(app, binding_id, hotkey_string);
        }
        return;
    }

    // Push-to-talk mode: start on press, stop on release
    if settings.push_to_talk {
        if is_pressed {
            action.start(app, binding_id, hotkey_string);
        } else {
            action.stop(app, binding_id, hotkey_string);
        }
        return;
    }

    // Toggle mode: toggle state on press only
    if is_pressed {
        // Determine action and update state while holding the lock,
        // but RELEASE the lock before calling the action to avoid deadlocks.
        // (Actions may need to acquire the lock themselves, e.g., cancel_current_operation)
        let should_start: bool;
        {
            let toggle_state_manager = app.state::<ManagedToggleState>();
            let mut states = toggle_state_manager
                .lock()
                .expect("Failed to lock toggle state manager");

            let is_currently_active = states
                .active_toggles
                .entry(binding_id.to_string())
                .or_insert(false);

            should_start = !*is_currently_active;
            *is_currently_active = should_start;
        } // Lock released here

        // Now call the action without holding the lock
        if should_start {
            action.start(app, binding_id, hotkey_string);
        } else {
            action.stop(app, binding_id, hotkey_string);
        }
    }
}
