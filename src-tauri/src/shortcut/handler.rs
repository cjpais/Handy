//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::warn;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::transcription_coordinator::{
    is_action_binding, is_transcribe_binding, parse_action_key,
};
use crate::TranscriptionCoordinator;

const CANCEL_CONFIRM_TIMEOUT_MS: u128 = 1500;
static CANCEL_PENDING: Mutex<Option<Instant>> = Mutex::new(None);

pub fn reset_cancel_confirmation() {
    if let Ok(mut pending) = CANCEL_PENDING.lock() {
        *pending = None;
    }
}

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

    // Transcribe bindings are handled by the coordinator.
    if is_transcribe_binding(binding_id) {
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            coordinator.send_input(binding_id, hotkey_string, is_pressed, settings.push_to_talk);
        } else {
            warn!("TranscriptionCoordinator is not initialized");
        }
        return;
    }

    // Action bindings (1-9): only fires when recording and key is pressed
    if is_action_binding(binding_id) {
        if is_pressed {
            if let Some(key) = parse_action_key(binding_id) {
                if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
                    coordinator.select_action(key);
                }
            }
        }
        return;
    }

    // Pause binding: toggle pause when recording and key is pressed
    if binding_id == "pause" {
        if is_pressed {
            let audio_manager = app.state::<Arc<AudioRecordingManager>>();
            if audio_manager.is_recording() {
                let paused = audio_manager.toggle_pause();
                crate::overlay::emit_recording_paused(app, paused);
            }
        }
        return;
    }

    // Show History: open main window and navigate to the History tab
    if binding_id == "show_history" {
        if is_pressed {
            crate::show_main_window(app);
            let _ = app.emit("navigate-to-section", "history");
        }
        return;
    }

    // Copy Latest History: copy the most recent transcription to clipboard
    if binding_id == "copy_latest_history" {
        if is_pressed {
            crate::tray::copy_last_transcript(app);
        }
        return;
    }

    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!(
            "No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', Pressed: {}",
            binding_id, hotkey_string, is_pressed
        );
        return;
    };

    // Cancel binding: requires double-press confirmation when recording
    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        if audio_manager.is_recording() && is_pressed {
            let should_cancel = {
                let mut pending = CANCEL_PENDING.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(first_press) = *pending {
                    if first_press.elapsed().as_millis() < CANCEL_CONFIRM_TIMEOUT_MS {
                        *pending = None;
                        true
                    } else {
                        *pending = Some(Instant::now());
                        false
                    }
                } else {
                    *pending = Some(Instant::now());
                    false
                }
            };
            if should_cancel {
                action.start(app, binding_id, hotkey_string);
            } else {
                let _ = app.emit("cancel-pending", ());
            }
        }
        return;
    }

    // Remaining bindings (e.g. "test") use simple start/stop on press/release.
    if is_pressed {
        action.start(app, binding_id, hotkey_string);
    } else {
        action.stop(app, binding_id, hotkey_string);
    }
}
