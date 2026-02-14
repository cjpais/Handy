use crate::actions::ACTION_MAP;
use crate::TranscriptionState;
use log::{debug, info, warn};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;

#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle, mut signals: Signals) {
    let app_handle_for_signal = app_handle.clone();

    debug!("SIGUSR2 signal handler registered successfully");
    const DEBOUNCE: Duration = Duration::from_millis(30);

    thread::spawn(move || {
        debug!("SIGUSR2 signal handler thread started");
        let mut last_signal: Option<Instant> = None;
        for sig in signals.forever() {
            match sig {
                SIGUSR2 => {
                    debug!("Received SIGUSR2 signal (signal number: {sig})");

                    let now = Instant::now();
                    if let Some(prev) = last_signal {
                        if now.duration_since(prev) < DEBOUNCE {
                            debug!(
                                "Debounced SIGUSR2 ({}ms since last)",
                                now.duration_since(prev).as_millis()
                            );
                            continue;
                        }
                    }
                    last_signal = Some(now);

                    let binding_id = "transcribe";
                    let shortcut_string = "SIGUSR2";

                    let Some(action) = ACTION_MAP.get(binding_id) else {
                        warn!("No action defined in ACTION_MAP for binding ID '{binding_id}'");
                        continue;
                    };

                    let ts = app_handle_for_signal.state::<TranscriptionState>();
                    match ts.current() {
                        TranscriptionState::IDLE => {
                            if ts.try_start() {
                                debug!("SIGUSR2: Starting transcription (was idle)");
                                action.start(&app_handle_for_signal, binding_id, shortcut_string);
                                info!("SIGUSR2: Transcription started");
                            }
                        }
                        TranscriptionState::RECORDING => {
                            if ts.try_stop() {
                                debug!("SIGUSR2: Stopping transcription (was recording)");
                                action.stop(&app_handle_for_signal, binding_id, shortcut_string);
                                debug!("SIGUSR2: Transcription stop initiated");
                            }
                        }
                        TranscriptionState::PROCESSING => {
                            debug!("SIGUSR2: Ignoring signal (transcription pipeline in progress)");
                        }
                        _ => {}
                    }
                }
                _ => unreachable!(),
            }
        }
    });
}
