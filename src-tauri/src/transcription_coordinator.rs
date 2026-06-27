use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use log::{debug, error, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(30);
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

/// Commands processed sequentially by the coordinator thread.
enum Command {
    Input {
        binding_id: String,
        hotkey_string: String,
        is_pressed: bool,
        push_to_talk: bool,
        double_tap_activation: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputAction {
    Ignore,
    Start,
    Stop,
}

/// Decide how a transcribe binding press/release should affect the pipeline.
fn evaluate_input(
    stage: &Stage,
    binding_id: &str,
    is_pressed: bool,
    push_to_talk: bool,
    double_tap_activation: bool,
    pending_double_tap: &mut Option<Instant>,
    now: Instant,
) -> InputAction {
    if push_to_talk {
        if is_pressed && matches!(stage, Stage::Idle) {
            return InputAction::Start;
        }
        if !is_pressed && matches!(stage, Stage::Recording(id) if id == binding_id) {
            return InputAction::Stop;
        }
        return InputAction::Ignore;
    }

    if !is_pressed {
        return InputAction::Ignore;
    }

    if double_tap_activation {
        return evaluate_double_tap_press(stage, binding_id, pending_double_tap, now);
    }

    match stage {
        Stage::Idle => InputAction::Start,
        Stage::Recording(id) if id == binding_id => InputAction::Stop,
        _ => InputAction::Ignore,
    }
}

fn evaluate_double_tap_press(
    stage: &Stage,
    binding_id: &str,
    pending_double_tap: &mut Option<Instant>,
    now: Instant,
) -> InputAction {
    match stage {
        Stage::Recording(id) if id == binding_id => {
            *pending_double_tap = None;
            InputAction::Stop
        }
        Stage::Idle => {
            if let Some(first_tap) = *pending_double_tap {
                if now.duration_since(first_tap) <= DOUBLE_TAP_WINDOW {
                    *pending_double_tap = None;
                    InputAction::Start
                } else {
                    *pending_double_tap = Some(now);
                    InputAction::Ignore
                }
            } else {
                *pending_double_tap = Some(now);
                InputAction::Ignore
            }
        }
        _ => InputAction::Ignore,
    }
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
                let mut pending_double_tap: Option<Instant> = None;

                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Command::Input {
                            binding_id,
                            hotkey_string,
                            is_pressed,
                            push_to_talk,
                            double_tap_activation,
                        } => {
                            // Debounce rapid-fire press events (key repeat).
                            // Releases always pass through for push-to-talk.
                            if is_pressed {
                                let now = Instant::now();
                                if last_press.map_or(false, |t| now.duration_since(t) < DEBOUNCE)
                                {
                                    debug!("Debounced press for '{binding_id}'");
                                    continue;
                                }
                                last_press = Some(now);
                            }

                            let action = evaluate_input(
                                &stage,
                                &binding_id,
                                is_pressed,
                                push_to_talk,
                                double_tap_activation,
                                &mut pending_double_tap,
                                Instant::now(),
                            );

                            match action {
                                InputAction::Start => {
                                    start(&app, &mut stage, &binding_id, &hotkey_string);
                                }
                                InputAction::Stop => {
                                    stop(&app, &mut stage, &binding_id, &hotkey_string);
                                }
                                InputAction::Ignore => {}
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => {
                            pending_double_tap = None;
                            // Don't reset during processing — wait for the pipeline to finish.
                            if !matches!(stage, Stage::Processing)
                                && (recording_was_active
                                    || matches!(stage, Stage::Recording(_)))
                            {
                                stage = Stage::Idle;
                            }
                        }
                        Command::ProcessingFinished => {
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
        double_tap_activation: bool,
    ) {
        if self
            .tx
            .send(Command::Input {
                binding_id: binding_id.to_string(),
                hotkey_string: hotkey_string.to_string(),
                is_pressed,
                push_to_talk,
                double_tap_activation,
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

fn start(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };
    action.start(app, binding_id, hotkey_string);
    if app
        .try_state::<Arc<AudioRecordingManager>>()
        .map_or(false, |a| a.is_recording())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_mode_starts_and_stops_on_press() {
        let mut pending = None;
        let idle = Stage::Idle;
        let recording = Stage::Recording("transcribe".to_string());

        assert_eq!(
            evaluate_input(&idle, "transcribe", true, false, false, &mut pending, Instant::now()),
            InputAction::Start
        );
        assert_eq!(
            evaluate_input(
                &recording,
                "transcribe",
                true,
                false,
                false,
                &mut pending,
                Instant::now()
            ),
            InputAction::Stop
        );
    }

    #[test]
    fn double_tap_requires_two_presses_within_window() {
        let mut pending = None;
        let idle = Stage::Idle;
        let t0 = Instant::now();

        assert_eq!(
            evaluate_input(&idle, "transcribe", true, false, true, &mut pending, t0),
            InputAction::Ignore
        );
        assert!(pending.is_some());

        assert_eq!(
            evaluate_input(
                &idle,
                "transcribe",
                true,
                false,
                true,
                &mut pending,
                t0 + Duration::from_millis(200)
            ),
            InputAction::Start
        );
        assert!(pending.is_none());
    }

    #[test]
    fn double_tap_single_press_does_not_activate() {
        let mut pending = None;
        let idle = Stage::Idle;
        let t0 = Instant::now();

        assert_eq!(
            evaluate_input(&idle, "transcribe", true, false, true, &mut pending, t0),
            InputAction::Ignore
        );
        assert_eq!(
            evaluate_input(
                &idle,
                "transcribe",
                true,
                false,
                true,
                &mut pending,
                t0 + DOUBLE_TAP_WINDOW + Duration::from_millis(50)
            ),
            InputAction::Ignore
        );
    }

    #[test]
    fn double_tap_stops_recording_with_single_press() {
        let mut pending = None;
        let recording = Stage::Recording("transcribe".to_string());

        assert_eq!(
            evaluate_input(
                &recording,
                "transcribe",
                true,
                false,
                true,
                &mut pending,
                Instant::now()
            ),
            InputAction::Stop
        );
    }

    #[test]
    fn push_to_talk_uses_press_and_release() {
        let mut pending = None;
        let idle = Stage::Idle;
        let recording = Stage::Recording("transcribe".to_string());
        let now = Instant::now();

        assert_eq!(
            evaluate_input(&idle, "transcribe", true, true, false, &mut pending, now),
            InputAction::Start
        );
        assert_eq!(
            evaluate_input(
                &recording,
                "transcribe",
                false,
                true,
                false,
                &mut pending,
                now
            ),
            InputAction::Stop
        );
    }
}
