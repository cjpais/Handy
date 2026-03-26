use crate::utils::send_transcription_input;
#[cfg(unix)]
use log::debug;
use tauri::AppHandle;

#[cfg(unix)]
use signal_hook::consts::{SIGUSR1, SIGUSR2};
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use std::thread;

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
            send_transcription_input(&app_handle, binding_id, signal_name);
        }
    });
}
