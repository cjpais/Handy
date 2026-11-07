//! Shared shortcut event handling logic
//!
//! This module contains the common logic for handling shortcut events,
//! used by both the Tauri and handy-keys implementations.

use log::{debug, warn};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use crate::transcription_coordinator::is_transcribe_binding;
use crate::TranscriptionCoordinator;

/// Upper bound on how long a paste waits for the user to let go of the
/// shortcut. Past it the chord is sent anyway, as it was before the wait
/// existed, so a stuck key can only delay a paste, never drop it.
pub const PASTE_RELEASE_WAIT_MAX: Duration = Duration::from_millis(1000);

const RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Transcribe bindings whose shortcut is currently held, as reported by
/// whichever backend is active. A paste chord injected while these keys are
/// still down reaches the target with the user's modifiers mixed in
/// (Ctrl+Alt+V, fn+Cmd+V) and pastes nothing (#2051).
static HELD_TRANSCRIBE_BINDINGS: HeldBindings = HeldBindings::new();

/// The set of held binding ids. A set rather than a bare counter: a repeated
/// press cannot count twice and a release without a press (the app started
/// with the key down) leaves it empty, so it cannot drift off by one.
struct HeldBindings(Mutex<Vec<String>>);

impl HeldBindings {
    const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn record(&self, binding_id: &str, is_pressed: bool) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|id| id != binding_id);
        if is_pressed {
            held.push(binding_id.to_string());
        }
    }

    fn count(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// Forgets every held binding. Call it when the shortcuts are unregistered
/// as a group: a release that happens while they are unregistered is never
/// reported, and the stale entry would make every paste wait the full bound.
pub(crate) fn clear_held_bindings() {
    HELD_TRANSCRIBE_BINDINGS.clear();
}

/// Result of [`wait_for_shortcut_release`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseWait {
    /// How long the caller was held up; zero when nothing was down.
    pub waited: Duration,
    /// The keys were still down when `max` ran out.
    pub timed_out: bool,
}

/// Blocks until the user has let go of the transcribe shortcut, or `max`
/// elapses. Call it right before injecting a paste chord, and never on the
/// main thread: on macOS the Tauri backend delivers the release through the
/// main run loop, so waiting there would always run to the bound.
pub fn wait_for_shortcut_release(max: Duration) -> ReleaseWait {
    let result = wait_while_held(max, RELEASE_POLL_INTERVAL, shortcut_keys_down);
    if result.timed_out {
        warn!(
            "Shortcut keys still held after {} ms; sending the paste chord anyway",
            result.waited.as_millis()
        );
    } else if !result.waited.is_zero() {
        debug!(
            "Waited {} ms for shortcut release before sending the paste chord",
            result.waited.as_millis()
        );
    }
    result
}

fn shortcut_keys_down() -> bool {
    HELD_TRANSCRIBE_BINDINGS.count() > 0 || physical_modifiers_down()
}

/// Both backends report a combo like Alt+Space as released when its main key
/// goes up, while the modifiers may still be down. The bindings set cannot
/// see that, so on Windows also ask the OS for the modifier state. The
/// handy-keys hook lets modifier events of a key combo through, so they show
/// up here; a modifier-only hotkey is covered by the bindings set instead.
#[cfg(target_os = "windows")]
fn physical_modifiers_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        // The high bit (a negative value) means the key is down right now.
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } < 0)
}

#[cfg(not(target_os = "windows"))]
fn physical_modifiers_down() -> bool {
    false
}

fn wait_while_held(max: Duration, poll: Duration, held: impl Fn() -> bool) -> ReleaseWait {
    if !held() {
        return ReleaseWait {
            waited: Duration::ZERO,
            timed_out: false,
        };
    }
    let start = Instant::now();
    loop {
        let elapsed = start.elapsed();
        if elapsed >= max {
            return ReleaseWait {
                waited: elapsed,
                timed_out: true,
            };
        }
        std::thread::sleep(poll.min(max - elapsed));
        if !held() {
            return ReleaseWait {
                waited: start.elapsed(),
                timed_out: false,
            };
        }
    }
}

/// Handle a shortcut event from either implementation.
///
/// This function contains the shared logic for:
/// - Looking up the action in ACTION_MAP
/// - Handling the cancel binding (only fires when recording)
/// - Routing transcribe bindings to the coordinator, which applies the
///   configured activation mode (toggle / push-to-talk / hold-or-toggle)
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
        HELD_TRANSCRIBE_BINDINGS.record(binding_id, is_pressed);
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            coordinator.send_input(
                binding_id,
                hotkey_string,
                is_pressed,
                settings.shortcut_activation,
                std::time::Duration::from_millis(settings.hold_threshold_ms),
            );
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
    use super::*;

    #[test]
    fn two_presses_then_two_releases_leave_nothing_held() {
        let held = HeldBindings::new();
        held.record("transcribe", true);
        held.record("transcribe_with_post_process", true);
        assert_eq!(held.count(), 2);
        held.record("transcribe", false);
        held.record("transcribe_with_post_process", false);
        assert_eq!(held.count(), 0);
    }

    #[test]
    fn repeated_press_of_one_binding_is_released_by_one_release() {
        let held = HeldBindings::new();
        held.record("transcribe", true);
        held.record("transcribe", true);
        held.record("transcribe", false);
        assert_eq!(held.count(), 0);
    }

    #[test]
    fn release_without_press_stays_at_zero() {
        let held = HeldBindings::new();
        held.record("transcribe", false);
        assert_eq!(held.count(), 0);
        held.record("transcribe", true);
        assert_eq!(held.count(), 1);
    }

    #[test]
    fn clear_forgets_a_held_binding() {
        let held = HeldBindings::new();
        held.record("transcribe", true);
        held.clear();
        assert_eq!(held.count(), 0);
    }

    #[test]
    fn wait_returns_immediately_when_nothing_is_held() {
        let held = HeldBindings::new();
        let result = wait_while_held(Duration::from_secs(5), RELEASE_POLL_INTERVAL, || {
            held.count() > 0
        });
        assert_eq!(
            result,
            ReleaseWait {
                waited: Duration::ZERO,
                timed_out: false
            }
        );
    }

    #[test]
    fn wait_times_out_at_the_bound_while_a_key_is_held() {
        let held = HeldBindings::new();
        held.record("transcribe", true);
        let max = Duration::from_millis(40);
        let result = wait_while_held(max, RELEASE_POLL_INTERVAL, || held.count() > 0);
        assert!(result.timed_out);
        assert!(result.waited >= max);
        assert!(result.waited < max + Duration::from_millis(500));
    }

    #[test]
    fn wait_ends_once_the_key_is_released() {
        let held = Arc::new(HeldBindings::new());
        held.record("transcribe", true);
        let releaser = {
            let held = Arc::clone(&held);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                held.record("transcribe", false);
            })
        };
        let result = wait_while_held(Duration::from_secs(5), RELEASE_POLL_INTERVAL, || {
            held.count() > 0
        });
        releaser.join().unwrap();
        assert!(!result.timed_out);
        assert!(result.waited >= Duration::from_millis(20));
    }
}
