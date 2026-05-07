use super::buttons::{ButtonEvent, ButtonEventKind, SpeechMikeButton};
use crate::actions::{fire_action, ActionIntent, ActionTriggerSource};
use crate::settings::get_settings;
use tauri::AppHandle;

/// Route a decoded SpeechMike button event to the appropriate recording action.
pub fn dispatch_button_event(app: &AppHandle, event: ButtonEvent) {
    let push_to_talk = get_settings(app).push_to_talk;

    match (&event.button, &event.kind) {
        (SpeechMikeButton::Record, ButtonEventKind::Press | ButtonEventKind::StateOn) => {
            fire_action(
                app,
                ActionIntent::Transcribe,
                true,
                ActionTriggerSource::SpeechMike,
            );
        }
        // Release only stops in push-to-talk mode; toggle mode ignores the release.
        (SpeechMikeButton::Record, ButtonEventKind::Release | ButtonEventKind::StateOff)
            if push_to_talk =>
        {
            fire_action(
                app,
                ActionIntent::Transcribe,
                false,
                ActionTriggerSource::SpeechMike,
            );
        }
        (SpeechMikeButton::Stop, ButtonEventKind::Press | ButtonEventKind::StateOn) => {
            fire_action(
                app,
                ActionIntent::Cancel,
                true,
                ActionTriggerSource::SpeechMike,
            );
        }
        (SpeechMikeButton::Eol, ButtonEventKind::Press | ButtonEventKind::StateOn) => {
            fire_action(
                app,
                ActionIntent::TranscribeWithPostProcess,
                true,
                ActionTriggerSource::SpeechMike,
            );
        }
        // Trigger always behaves as push-to-talk regardless of the global setting.
        (SpeechMikeButton::Trigger, ButtonEventKind::Press) => {
            fire_action(
                app,
                ActionIntent::Transcribe,
                true,
                ActionTriggerSource::SpeechMike,
            );
        }
        (SpeechMikeButton::Trigger, ButtonEventKind::Release) => {
            fire_action(
                app,
                ActionIntent::Transcribe,
                false,
                ActionTriggerSource::SpeechMike,
            );
        }
        (SpeechMikeButton::Unknown(id), _) => {
            log::debug!(
                "SpeechMike unmapped button report_id={:#04x}: {}",
                id,
                event
                    .raw_report
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        // InsertOverwrite, Forward, Rewind: reserved, no-op.
        _ => {}
    }
}
