use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use log::{debug, error, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(30);
const MODIFIER_TAP_MAX_DURATION: Duration = Duration::from_millis(250);

/// Commands processed sequentially by the coordinator thread.
enum Command {
    Input {
        binding_id: String,
        hotkey_string: String,
        is_pressed: bool,
        push_to_talk: bool,
    },
    Cancel {
        recording_was_active: bool,
    },
    ProcessingFinished,
}

/// Pipeline lifecycle, owned exclusively by the coordinator thread.
enum Stage {
    Idle,
    Recording(String), // binding_id
    Processing,
}

/// Serialises all transcription lifecycle events through a single thread
/// to eliminate race conditions between keyboard shortcuts, signals, and
/// the async transcribe-paste pipeline.
pub struct TranscriptionCoordinator {
    tx: Sender<Command>,
}

pub fn is_transcribe_binding(id: &str) -> bool {
    id == "transcribe" || id == "transcribe_with_post_process"
}

impl TranscriptionCoordinator {
    pub fn new(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut stage = Stage::Idle;
                let mut last_press: Option<Instant> = None;
                let mut modifier_only_press: Option<(String, Instant)> = None;

                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Command::Input {
                            binding_id,
                            hotkey_string,
                            is_pressed,
                            push_to_talk,
                        } => {
                            // Debounce rapid-fire press events (key repeat / double-tap).
                            // Releases always pass through for push-to-talk.
                            if is_pressed {
                                let now = Instant::now();
                                if last_press.is_some_and(|t| now.duration_since(t) < DEBOUNCE) {
                                    debug!("Debounced press for '{binding_id}'");
                                    continue;
                                }
                                last_press = Some(now);
                            }

                            if push_to_talk {
                                if is_pressed && matches!(stage, Stage::Idle) {
                                    start(&app, &mut stage, &binding_id, &hotkey_string);
                                } else if !is_pressed
                                    && matches!(&stage, Stage::Recording(id) if id == &binding_id)
                                {
                                    stop(&app, &mut stage, &binding_id, &hotkey_string);
                                }
                            } else if is_modifier_only_hotkey(&hotkey_string) {
                                if is_pressed {
                                    modifier_only_press =
                                        Some((binding_id.clone(), Instant::now()));
                                    continue;
                                }

                                let Some((pressed_binding_id, pressed_at)) =
                                    modifier_only_press.as_ref()
                                else {
                                    continue;
                                };

                                if pressed_binding_id != &binding_id {
                                    debug!(
                                        "Ignoring release for '{binding_id}' because the active modifier tap belongs to '{pressed_binding_id}'"
                                    );
                                    continue;
                                }

                                let pressed_at = *pressed_at;
                                modifier_only_press = None;

                                let now = Instant::now();
                                if !is_quick_modifier_tap(pressed_at, now) {
                                    debug!(
                                        "Ignoring modifier-only shortcut hold for '{binding_id}' after {:?}",
                                        now.duration_since(pressed_at)
                                    );
                                    continue;
                                }

                                toggle(&app, &mut stage, &binding_id, &hotkey_string);
                            } else if is_pressed {
                                toggle(&app, &mut stage, &binding_id, &hotkey_string);
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => {
                            modifier_only_press = None;
                            // Don't reset during processing — wait for the pipeline to finish.
                            if !matches!(stage, Stage::Processing)
                                && (recording_was_active || matches!(stage, Stage::Recording(_)))
                            {
                                stage = Stage::Idle;
                            }
                        }
                        Command::ProcessingFinished => {
                            modifier_only_press = None;
                            stage = Stage::Idle;
                        }
                    }
                }
                debug!("Transcription coordinator exited");
            }));
            if let Err(e) = result {
                error!("Transcription coordinator panicked: {e:?}");
            }
        });

        Self { tx }
    }

    /// Send a keyboard/signal input event for a transcribe binding.
    /// For signal-based toggles, use `is_pressed: true` and `push_to_talk: false`.
    pub fn send_input(
        &self,
        binding_id: &str,
        hotkey_string: &str,
        is_pressed: bool,
        push_to_talk: bool,
    ) {
        if self
            .tx
            .send(Command::Input {
                binding_id: binding_id.to_string(),
                hotkey_string: hotkey_string.to_string(),
                is_pressed,
                push_to_talk,
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_cancel(&self, recording_was_active: bool) {
        if self
            .tx
            .send(Command::Cancel {
                recording_was_active,
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_processing_finished(&self) {
        if self.tx.send(Command::ProcessingFinished).is_err() {
            warn!("Transcription coordinator channel closed");
        }
    }
}

fn toggle(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    match &stage {
        Stage::Idle => {
            start(app, stage, binding_id, hotkey_string);
        }
        Stage::Recording(id) if id == binding_id => {
            stop(app, stage, binding_id, hotkey_string);
        }
        _ => debug!("Ignoring press for '{binding_id}': pipeline busy"),
    }
}

fn is_modifier_only_hotkey(hotkey_string: &str) -> bool {
    hotkey_string
        .parse::<handy_keys::Hotkey>()
        .map(|hotkey| hotkey.key.is_none())
        .unwrap_or(false)
}

fn is_quick_modifier_tap(pressed_at: Instant, released_at: Instant) -> bool {
    released_at.duration_since(pressed_at) <= MODIFIER_TAP_MAX_DURATION
}

fn start(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };
    action.start(app, binding_id, hotkey_string);
    if app
        .try_state::<Arc<AudioRecordingManager>>()
        .is_some_and(|a| a.is_recording())
    {
        *stage = Stage::Recording(binding_id.to_string());
    } else {
        debug!("Start for '{binding_id}' did not begin recording; staying idle");
    }
}

fn stop(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };
    action.stop(app, binding_id, hotkey_string);
    *stage = Stage::Processing;
}
