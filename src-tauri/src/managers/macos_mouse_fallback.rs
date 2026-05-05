#[cfg(target_os = "macos")]
use std::thread;

#[cfg(target_os = "macos")]
use rdev::{listen, Button, Event, EventType};
#[cfg(target_os = "macos")]
use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Manager;

#[cfg(target_os = "macos")]
use crate::TranscriptionCoordinator;

#[cfg(target_os = "macos")]
const FALLBACK_MOUSE_BUTTONS: [u8; 2] = [3, 4];

#[cfg(target_os = "macos")]
pub fn start_macos_mouse_button_fallback(app: AppHandle) {
    thread::spawn(move || {
        log::info!(
            "Starting macOS mouse button fallback listener for side buttons {:?}",
            FALLBACK_MOUSE_BUTTONS
        );

        let callback = move |event: Event| {
            if let Some((button_code, is_pressed)) = extract_side_button_event(&event) {
                log::info!(
                    "macOS mouse fallback: button {} {}",
                    button_code,
                    if is_pressed { "pressed" } else { "released" }
                );

                if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
                    coordinator.send_input("transcribe", "macos-mouse-fallback", is_pressed, true);
                } else {
                    log::warn!(
                        "macOS mouse fallback dropped event: TranscriptionCoordinator missing"
                    );
                }
            }
        };

        if let Err(error) = listen(callback) {
            log::error!("macOS mouse fallback listener failed: {:?}", error);
        }
    });
}

#[cfg(target_os = "macos")]
fn extract_side_button_event(event: &Event) -> Option<(u8, bool)> {
    match event.event_type {
        EventType::ButtonPress(Button::Unknown(code)) if FALLBACK_MOUSE_BUTTONS.contains(&code) => {
            Some((code, true))
        }
        EventType::ButtonRelease(Button::Unknown(code))
            if FALLBACK_MOUSE_BUTTONS.contains(&code) =>
        {
            Some((code, false))
        }
        _ => None,
    }
}
