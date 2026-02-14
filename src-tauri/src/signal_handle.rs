use crate::TranscriptionCoordinator;
use log::{debug, warn};
use std::thread;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;

#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle, mut signals: Signals) {
    debug!("SIGUSR2 signal handler registered");
    thread::spawn(move || {
        for sig in signals.forever() {
            if sig == SIGUSR2 {
                debug!("Received SIGUSR2");
                if let Some(c) = app_handle.try_state::<TranscriptionCoordinator>() {
                    c.send_input("transcribe", "SIGUSR2", true, false);
                } else {
                    warn!("TranscriptionCoordinator not initialized");
                }
            }
        }
    });
}
