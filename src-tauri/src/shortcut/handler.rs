//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::{debug, warn};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::ManagedToggleState;
use crate::TranscriptionState;

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
static LAST_EVENT: Mutex<Option<Instant>> = Mutex::new(None);
const DEBOUNCE: Duration = Duration::from_millis(30);

pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    // Debounce rapid-fire key press events (e.g. key repeat / accidental double-tap).
    // Only debounce presses — releases must always pass through for push-to-talk.
    if is_pressed {
        let mut last = LAST_EVENT.lock().unwrap();
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < DEBOUNCE {
                debug!(
                    "Debounced shortcut event for '{}' ({}ms since last)",
                    binding_id,
                    now.duration_since(prev).as_millis()
                );
                return;
            }
        }
        *last = Some(now);
    }

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

    // Push-to-talk mode: start on press, stop on release.
    // Transcribe bindings use TranscriptionState to prevent a new cycle
    // from starting while the previous async pipeline is still pasting.
    if settings.push_to_talk {
        if binding_id == "transcribe" || binding_id == "transcribe_with_post_process" {
            let ts = app.state::<TranscriptionState>();
            if is_pressed {
                if ts.try_start() {
                    action.start(app, binding_id, hotkey_string);
                } else {
                    debug!("Ignoring push-to-talk press: transcription pipeline busy");
                }
            } else if ts.try_stop() {
                action.stop(app, binding_id, hotkey_string);
            }
        } else {
            if is_pressed {
                action.start(app, binding_id, hotkey_string);
            } else {
                action.stop(app, binding_id, hotkey_string);
            }
        }
        return;
    }

    // Toggle mode: toggle state on press only
    if is_pressed {
        // Transcribe bindings use the shared TranscriptionState which
        // tracks Idle → Recording → Processing to prevent races.
        if binding_id == "transcribe" || binding_id == "transcribe_with_post_process" {
            let ts = app.state::<TranscriptionState>();
            match ts.current() {
                TranscriptionState::IDLE => {
                    if ts.try_start() {
                        action.start(app, binding_id, hotkey_string);
                    }
                }
                TranscriptionState::RECORDING => {
                    if ts.try_stop() {
                        action.stop(app, binding_id, hotkey_string);
                    }
                }
                TranscriptionState::PROCESSING => {
                    debug!("Ignoring shortcut: transcription pipeline in progress");
                }
                _ => {}
            }
        } else {
            // Non-transcribe bindings use the simple toggle map.
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

            if should_start {
                action.start(app, binding_id, hotkey_string);
            } else {
                action.stop(app, binding_id, hotkey_string);
            }
        }
    }
}
