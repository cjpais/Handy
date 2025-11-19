use crate::actions::ACTION_MAP;
use crate::ManagedToggleState;
use log::{debug, info, warn};
use std::thread;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;

#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle, mut signals: Signals) {
    let app_handle_for_signal = app_handle.clone();

    debug!("SIGUSR2 signal handler registered successfully");
    thread::spawn(move || {
        debug!("SIGUSR2 signal handler thread started");
        for sig in signals.forever() {
            match sig {
                SIGUSR2 => {
                    debug!("Received SIGUSR2 signal (signal number: {sig})");

                    let binding_id = "transcribe";
                    let shortcut_string = "SIGUSR2";

                    if let Some(action) = ACTION_MAP.get(binding_id) {
                        let toggle_state_manager =
                            app_handle_for_signal.state::<ManagedToggleState>();

                        let mut states = match toggle_state_manager.lock() {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("Failed to lock toggle state manager: {e}");
                                continue;
                            }
                        };

                        let is_currently_active = states
                            .active_toggles
                            .entry(binding_id.to_string())
                            .or_insert(false);

                        if *is_currently_active {
                            debug!("SIGUSR2: Stopping transcription (currently active)");
                            action.stop(&app_handle_for_signal, binding_id, shortcut_string);
                            *is_currently_active = false; // Update state to inactive
                            debug!("SIGUSR2: Transcription stopped");
                        } else {
                            debug!("SIGUSR2: Starting transcription (currently inactive)");
                            action.start(&app_handle_for_signal, binding_id, shortcut_string);
                            *is_currently_active = true; // Update state to active
                            info!("SIGUSR2: Transcription started");
                        }
                    } else {
                        warn!("No action defined in ACTION_MAP for binding ID '{binding_id}'");
                    }
                }
                _ => unreachable!(),
            }
        }
    });
}
