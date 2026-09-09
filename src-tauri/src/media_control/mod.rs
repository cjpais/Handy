//! Pausing whatever media is playing for the duration of a dictation, and
//! resuming exactly what we paused once it is over.
//!
//! Opt-in through `pause_media_while_recording`. The rule the platform
//! backends share: never *start* playback that was not already running. Each
//! backend therefore reports which players it actually paused, and resume only
//! touches those. If nothing was playing, a recording is a no-op.
//!
//! Backends:
//!
//! - Windows: the system media transport controls
//!   (`GlobalSystemMediaTransportControlsSessionManager`), which is what the
//!   media keys drive — it covers browsers, Spotify, and any app that
//!   publishes a media session.
//! - Linux: MPRIS over D-Bus, via `playerctl` when present and plain
//!   `dbus-send` otherwise. Same coverage as the media keys.
//! - macOS: AppleScript against the scriptable players (Music, Spotify, TV,
//!   VLC, QuickTime Player), plus Chromium-family browsers through the
//!   JavaScript their dictionary can run — the browser part only works if the
//!   user has turned on *View > Developer > Allow JavaScript from Apple
//!   Events*, which is off by default. macOS offers nothing better: there is no
//!   public API for another application's media session, and synthetic
//!   media-key events are no longer delivered.
//!
//! The work runs on a dedicated thread: a backend can shell out or block on
//! IPC, and neither must ever be in the path between the shortcut and the
//! microphone.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, warn};
use tauri::AppHandle;

use crate::settings::get_settings;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

/// Backend stub for platforms without media control. Keeps the call sites
/// unconditional instead of sprinkling `cfg` over `actions.rs`.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn pause_playing_media() -> Vec<String> {
        Vec::new()
    }

    pub fn resume_media(_players: &[String]) {}
}

enum Command {
    Pause,
    /// Carries an optional acknowledgement channel, used by the shutdown path
    /// to wait for the resume instead of racing the process exit.
    Resume(Option<Sender<()>>),
}

/// Pauses and resumes external media playback around a dictation.
///
/// Commands are queued on a worker thread, so ordering is guaranteed even
/// though callers never block: a stop that lands while the pause is still
/// running is applied straight after it, not against a stale state.
pub struct MediaController {
    app: AppHandle,
    tx: Option<Sender<Command>>,
    /// The most recently *requested* state. A queued pause reads this before
    /// doing any work, so a recording that ends before the backend got to it
    /// never produces an audible blip.
    pause_requested: Arc<AtomicBool>,
}

impl MediaController {
    pub fn new(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        let pause_requested = Arc::new(AtomicBool::new(false));
        let worker_flag = Arc::clone(&pause_requested);

        let spawned = std::thread::Builder::new()
            .name("media-control".to_string())
            .spawn(move || {
                // Owned by the worker alone, so no lock is needed: it is the
                // only thing that ever pauses or resumes.
                let mut paused: Vec<String> = Vec::new();

                for command in rx {
                    match command {
                        Command::Pause => {
                            // Superseded by a stop that was queued while this
                            // pause waited its turn.
                            if !worker_flag.load(Ordering::SeqCst) {
                                continue;
                            }
                            if !paused.is_empty() {
                                continue;
                            }
                            paused = platform::pause_playing_media();
                            if paused.is_empty() {
                                debug!("No playing media to pause");
                            } else {
                                debug!("Paused media players: {}", paused.join(", "));
                            }
                        }
                        Command::Resume(ack) => {
                            if !paused.is_empty() {
                                debug!("Resuming media players: {}", paused.join(", "));
                                platform::resume_media(&paused);
                                paused.clear();
                            }
                            if let Some(ack) = ack {
                                let _ = ack.send(());
                            }
                        }
                    }
                }
            });

        let tx = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                warn!("Failed to start media control thread: {e}");
                None
            }
        };

        Self {
            app,
            tx,
            pause_requested,
        }
    }

    /// Pauses any currently playing media, if the setting is enabled. Returns
    /// immediately; the backend runs on the worker thread.
    pub fn pause_playing_media(&self) {
        if !get_settings(&self.app).pause_media_while_recording {
            return;
        }
        self.pause_requested.store(true, Ordering::SeqCst);
        let _ = self.send(Command::Pause);
    }

    /// Resumes whatever *we* paused. Always runs, even when the setting has
    /// been turned off mid-recording, so a disabled setting can never strand
    /// the user's music paused.
    pub fn resume_paused_media(&self) {
        self.pause_requested.store(false, Ordering::SeqCst);
        let _ = self.send(Command::Resume(None));
    }

    /// Resumes and waits for it to have happened, up to `timeout`. Used on
    /// shutdown, where a queued resume would otherwise lose the race against
    /// the process exiting and leave the user's music paused for good.
    pub fn resume_paused_media_blocking(&self, timeout: Duration) {
        self.pause_requested.store(false, Ordering::SeqCst);
        let (ack_tx, ack_rx) = mpsc::channel();
        if self.send(Command::Resume(Some(ack_tx))) {
            let _ = ack_rx.recv_timeout(timeout);
        }
    }

    fn send(&self, command: Command) -> bool {
        match &self.tx {
            Some(tx) => tx.send(command).is_ok(),
            None => false,
        }
    }
}
