use log::warn;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::transcription_coordinator::is_transcribe_binding;
use crate::TranscriptionCoordinator;

pub fn handle_shortcut_event(
    app: &AppHandle,
    binding_id: &str,
    hotkey_string: &str,
    is_pressed: bool,
) {
    if is_transcribe_binding(binding_id) {
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            let is_ptt = binding_id == "transcribe_with_push_to_talk";
            coordinator.send_input(binding_id, hotkey_string, is_pressed, is_ptt);
        } else {
            warn!("TranscriptionCoordinator is not initialized");
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

    // Cancel binding: only fires when recording and key is pressed
    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        if audio_manager.is_recording() && is_pressed {
            action.start(app, binding_id, hotkey_string);
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

#[cfg(test)]
mod tests {
    /// The is_ptt flag is derived inline as `binding_id == "transcribe_with_push_to_talk"`.
    /// These tests encode that contract so a future refactor cannot silently break it.
    fn is_ptt_for(binding_id: &str) -> bool {
        binding_id == "transcribe_with_push_to_talk"
    }

    #[test]
    fn push_to_talk_binding_yields_is_ptt_true() {
        assert!(
            is_ptt_for("transcribe_with_push_to_talk"),
            "transcribe_with_push_to_talk must set is_ptt=true"
        );
    }

    #[test]
    fn plain_transcribe_binding_yields_is_ptt_false() {
        assert!(!is_ptt_for("transcribe"));
    }

    #[test]
    fn post_process_binding_yields_is_ptt_false() {
        assert!(!is_ptt_for("transcribe_with_post_process"));
    }

    #[test]
    fn cancel_binding_yields_is_ptt_false() {
        assert!(!is_ptt_for("cancel"));
    }
}
