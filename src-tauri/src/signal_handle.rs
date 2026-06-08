use crate::TranscriptionCoordinator;
#[cfg(unix)]
use log::debug;
use log::warn;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use signal_hook::consts::SIGUSR2;
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use std::thread;

/// Send a transcription input to the coordinator.
/// Used by signal handlers, CLI flags, and any other external trigger.
pub fn send_transcription_input(app: &AppHandle, binding_id: &str, source: &str) {
    if let Some(c) = app.try_state::<TranscriptionCoordinator>() {
        c.send_input(binding_id, source, true, false);
    } else {
        warn!("TranscriptionCoordinator not initialized");
    }
}

#[cfg(unix)]
pub fn setup_signal_handler(
    app_handle: AppHandle,
    mut signals: Signals,
    sig_post_process: i32,
) {
    debug!(
        "Signal handlers registered (SIGRTMIN+1={sig_post_process}, SIGUSR2)"
    );
    thread::spawn(move || {
        for sig in signals.forever() {
            let (binding_id, signal_name) = if sig == sig_post_process {
                ("transcribe_with_post_process", "SIGRTMIN+1")
            } else if sig == SIGUSR2 {
                ("transcribe", "SIGUSR2")
            } else {
                continue;
            };
            debug!("Received {signal_name}");
            send_transcription_input(&app_handle, binding_id, signal_name);
        }
    });
}
