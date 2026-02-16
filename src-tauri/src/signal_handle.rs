use crate::TranscriptionCoordinator;
use log::{debug, warn};
use std::thread;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::{SIGUSR1, SIGUSR2};
#[cfg(unix)]
use signal_hook::iterator::Signals;

#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle, mut signals: Signals) {
    debug!("Signal handlers registered (SIGUSR1, SIGUSR2)");
    thread::spawn(move || {
        for sig in signals.forever() {
            let (binding_id, signal_name) = match sig {
                SIGUSR1 => ("transcribe_with_post_process", "SIGUSR1"),
                SIGUSR2 => ("transcribe", "SIGUSR2"),
                _ => continue,
            };
            debug!("Received {signal_name}");
            if let Some(c) = app_handle.try_state::<TranscriptionCoordinator>() {
                c.send_input(binding_id, signal_name, true, false);
            } else {
                warn!("TranscriptionCoordinator not initialized");
            }
        }
    });
}
