//! Bluetooth headset "press to mute" gesture → stop recording (macOS 14+).
//!
//! While Handy captures audio through AirPods / Beats, the headset is in the
//! hands-free (HFP) profile and its stem / Digital Crown press is no longer a
//! media key. macOS delivers it to the process capturing input as an
//! input-mute gesture through `AVAudioApplication`'s input mute state change
//! handler. A process without a handler gets a "Cannot Control Mic with
//! <device>" banner and a rejection chime on every press, and the press does
//! nothing. Handy has no use for a muted microphone mid-dictation, so the
//! gesture means "stop and transcribe".
//!
//! Lifecycle rules, each one learned from a live trace on macOS 26.5 with
//! AirPods Max:
//! - Register only after capture is running. `audiomxd` refuses to add a
//!   process with no input IO as a mute listener, and the handler then never
//!   fires.
//! - The handler also runs for Handy's own `setInputMuted` calls, with
//!   `false`. Only `true` is a headset gesture.
//! - A handled gesture leaves the per-app mute flag set. If it is still set
//!   when the handler is next registered or a capture session goes active,
//!   the system re-applies it and calls the handler with `true`, which would
//!   stop the new recording at once. Clear the flag before the next session
//!   starts; done while no session is active it is stored silently. Clearing
//!   it while a session is active plays a second "unmuted" chime instead.
//! - Clearing the flag is refused unless a handler is set ("input mute
//!   handler not set"), so the handler is kept for the life of the process
//!   once registered rather than cleared at the end of each recording. While
//!   Handy is not recording it is not a mute candidate and the handler is
//!   never invoked for a headset press.
//! - The handler alone is not enough: `audiomxd` rejects the gesture with
//!   "audio app is not allowed to input mute (isOptedIn: 0, hasHandler: 1)"
//!   until the process has also observed
//!   `AVAudioApplicationInputMuteStateChangeNotification`, which is what
//!   flips its `PrefersBluetoothAccessoryMutingMacOS` opt-in. The shared
//!   `AVAudioApplication` must already exist when the observer is added or
//!   the registration goes unnoticed. Handy keeps one such observer for the
//!   life of the process.

use tauri::AppHandle;

/// Call before the capture session for a new recording is opened.
pub fn prepare_for_recording() {
    imp::prepare_for_recording()
}

/// Call once capture for `binding_id` is running. A headset gesture then
/// stops that binding through the same path as the CLI toggle.
pub fn recording_started(app: &AppHandle, binding_id: &str) {
    imp::recording_started(app, binding_id)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;

    use std::sync::Arc;

    use block2::RcBlock;
    use log::{debug, warn};
    use objc2::runtime::Bool;
    use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationInputMuteStateChangeNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use tauri::{AppHandle, Manager};

    use crate::managers::audio::AudioRecordingManager;

    /// A single press can invoke the handler more than once; stop once.
    static GESTURE_HANDLED: AtomicBool = AtomicBool::new(false);
    static OPT_IN: Once = Once::new();

    /// Observe the input mute notification once. The observation itself is
    /// the opt-in that lets the system route headset mute gestures to Handy;
    /// the notification carries nothing Handy needs beyond that. The observer
    /// token is intentionally leaked so the registration lasts as long as the
    /// process.
    fn opt_in_to_accessory_muting() {
        OPT_IN.call_once(|| {
            // Order matters: the shared instance installs the hook that
            // turns the observer registration below into the opt-in.
            let _app = unsafe { AVAudioApplication::sharedInstance() };
            let observer = RcBlock::new(|_notification: NonNull<NSNotification>| {});
            let center = NSNotificationCenter::defaultCenter();
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(AVAudioApplicationInputMuteStateChangeNotification),
                    None,
                    None,
                    &observer,
                )
            };
            std::mem::forget(token);
            debug!("accessory mute: opted in to Bluetooth accessory muting");
        });
    }

    pub fn prepare_for_recording() {
        opt_in_to_accessory_muting();
        GESTURE_HANDLED.store(false, Ordering::SeqCst);
        let shared = unsafe { AVAudioApplication::sharedInstance() };
        if unsafe { shared.isInputMuted() } {
            match unsafe { shared.setInputMuted_error(false) } {
                Ok(()) => {
                    debug!("accessory mute: cleared input mute flag left by the last gesture")
                }
                Err(err) => warn!("accessory mute: clearing stale input mute flag failed: {err}"),
            }
        }
    }

    pub fn recording_started(app: &AppHandle, binding_id: &str) {
        let app = app.clone();
        let binding_id = binding_id.to_string();
        let handler = RcBlock::new(move |input_should_be_muted: Bool| -> Bool {
            if !input_should_be_muted.as_bool() {
                return Bool::YES;
            }
            // The handler outlives the recording (see module docs). With an
            // always-on microphone the mic stays open between dictations, so a
            // press then would reach here too; only a live recording may be
            // stopped, never started.
            let recording = app
                .try_state::<Arc<AudioRecordingManager>>()
                .map(|rm| rm.is_recording())
                .unwrap_or(false);
            if !recording {
                debug!("accessory mute gesture received while not recording; ignored");
                return Bool::YES;
            }
            if !GESTURE_HANDLED.swap(true, Ordering::SeqCst) {
                debug!("accessory mute gesture received; stopping recording for {binding_id}");
                crate::signal_handle::send_transcription_input(&app, &binding_id, "accessory_mute");
            }
            Bool::YES
        });
        let shared = unsafe { AVAudioApplication::sharedInstance() };
        match unsafe { shared.setInputMuteStateChangeHandler_error(Some(&handler)) } {
            Ok(()) => debug!("accessory mute handler registered"),
            Err(err) => warn!("accessory mute handler registration failed: {err}"),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use tauri::AppHandle;

    pub fn prepare_for_recording() {}
    pub fn recording_started(_app: &AppHandle, _binding_id: &str) {}
}
