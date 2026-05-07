use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::{get_settings, TranscriptionBackend};
use log::{debug, error, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(30);

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
    RecordingStarted {
        binding_id: String,
    },
    StartFailed {
        binding_id: String,
    },
    ProcessingFinished,
}

/// Pipeline lifecycle, owned exclusively by the coordinator thread.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Idle,
    Starting {
        binding_id: String,
        backend: TranscriptionBackend,
    },
    Recording {
        binding_id: String,
        backend: TranscriptionBackend,
    },
    Processing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputAction {
    Start,
    Stop,
    Ignore,
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
                                if last_press.map_or(false, |t| now.duration_since(t) < DEBOUNCE) {
                                    debug!("Debounced press for '{binding_id}'");
                                    continue;
                                }
                                last_press = Some(now);
                            }

                            let action =
                                input_action(&stage, &binding_id, is_pressed, push_to_talk);
                            match action {
                                InputAction::Start => {
                                    start(&app, &mut stage, &binding_id, &hotkey_string);
                                }
                                InputAction::Stop => {
                                    stop(&app, &mut stage, &binding_id, &hotkey_string);
                                }
                                InputAction::Ignore => {
                                    debug!("Ignoring input for '{binding_id}': pipeline busy");
                                }
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => {
                            // Don't reset during processing — wait for the pipeline to finish.
                            if !matches!(stage, Stage::Processing)
                                && (recording_was_active
                                    || matches!(
                                        stage,
                                        Stage::Starting { .. } | Stage::Recording { .. }
                                    ))
                            {
                                stage = Stage::Idle;
                            }
                        }
                        Command::RecordingStarted { binding_id } => {
                            apply_recording_started(&mut stage, &binding_id);
                        }
                        Command::StartFailed { binding_id } => {
                            apply_start_failed(&mut stage, &binding_id);
                        }
                        Command::ProcessingFinished => {
                            apply_processing_finished(&mut stage);
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

    pub fn notify_recording_started(&self, binding_id: &str) {
        if self
            .tx
            .send(Command::RecordingStarted {
                binding_id: binding_id.to_string(),
            })
            .is_err()
        {
            warn!("Transcription coordinator channel closed");
        }
    }

    pub fn notify_start_failed(&self, binding_id: &str) {
        if self
            .tx
            .send(Command::StartFailed {
                binding_id: binding_id.to_string(),
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

fn audio_is_recording(app: &AppHandle) -> bool {
    app.try_state::<Arc<AudioRecordingManager>>()
        .map_or(false, |manager| manager.is_recording())
}

fn livestt_is_active(app: &AppHandle) -> bool {
    app.try_state::<Arc<crate::livestt::session::LiveSttSessionManager>>()
        .map_or(false, |manager| manager.is_active())
}

fn pipeline_started_for_backend(app: &AppHandle, backend: TranscriptionBackend) -> bool {
    audio_is_recording(app) || (backend == TranscriptionBackend::LiveStt && livestt_is_active(app))
}

fn binding_matches(stage_binding_id: &str, binding_id: &str) -> bool {
    stage_binding_id == binding_id
}

fn input_action(
    stage: &Stage,
    binding_id: &str,
    is_pressed: bool,
    push_to_talk: bool,
) -> InputAction {
    if push_to_talk {
        return match (is_pressed, stage) {
            (true, Stage::Idle) => InputAction::Start,
            (
                false,
                Stage::Starting {
                    binding_id: active_id,
                    ..
                }
                | Stage::Recording {
                    binding_id: active_id,
                    ..
                },
            ) if binding_matches(active_id, binding_id) => InputAction::Stop,
            _ => InputAction::Ignore,
        };
    }

    if !is_pressed {
        return InputAction::Ignore;
    }

    match stage {
        Stage::Idle => InputAction::Start,
        Stage::Starting {
            binding_id: active_id,
            ..
        }
        | Stage::Recording {
            binding_id: active_id,
            ..
        } if binding_matches(active_id, binding_id) => InputAction::Stop,
        _ => InputAction::Ignore,
    }
}

fn apply_recording_started(stage: &mut Stage, binding_id: &str) {
    if let Stage::Starting {
        binding_id: active_id,
        backend,
    } = stage
    {
        if binding_matches(active_id, binding_id) {
            *stage = Stage::Recording {
                binding_id: active_id.clone(),
                backend: *backend,
            };
        }
    }
}

fn apply_start_failed(stage: &mut Stage, binding_id: &str) {
    if matches!(
        stage,
        Stage::Starting {
            binding_id: active_id,
            ..
        } if binding_matches(active_id, binding_id)
    ) {
        *stage = Stage::Idle;
    }
}

fn apply_processing_finished(stage: &mut Stage) {
    *stage = Stage::Idle;
}

fn stage_backend(stage: &Stage, fallback: TranscriptionBackend) -> TranscriptionBackend {
    match stage {
        Stage::Starting { backend, .. } | Stage::Recording { backend, .. } => *backend,
        Stage::Idle | Stage::Processing => fallback,
    }
}

fn apply_stop_result(
    stage: &mut Stage,
    backend: TranscriptionBackend,
    pipeline_started_after_stop: bool,
) {
    if backend == TranscriptionBackend::LiveStt && !pipeline_started_after_stop {
        *stage = Stage::Idle;
    } else {
        *stage = Stage::Processing;
    }
}

fn start(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };

    let backend = get_settings(app).transcription_backend;
    action.start(app, binding_id, hotkey_string);

    if backend == TranscriptionBackend::LiveStt {
        *stage = Stage::Starting {
            binding_id: binding_id.to_string(),
            backend,
        };
        return;
    }

    if pipeline_started_for_backend(app, backend) {
        *stage = Stage::Recording {
            binding_id: binding_id.to_string(),
            backend,
        };
    } else {
        debug!("Start for '{binding_id}' did not begin recording; staying idle");
    }
}

fn stop(app: &AppHandle, stage: &mut Stage, binding_id: &str, hotkey_string: &str) {
    let Some(action) = ACTION_MAP.get(binding_id) else {
        warn!("No action in ACTION_MAP for '{binding_id}'");
        return;
    };

    let backend = stage_backend(stage, get_settings(app).transcription_backend);
    action.stop(app, binding_id, hotkey_string);

    apply_stop_result(stage, backend, pipeline_started_for_backend(app, backend));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starting(binding_id: &str) -> Stage {
        Stage::Starting {
            binding_id: binding_id.to_string(),
            backend: TranscriptionBackend::LiveStt,
        }
    }

    fn recording(binding_id: &str) -> Stage {
        Stage::Recording {
            binding_id: binding_id.to_string(),
            backend: TranscriptionBackend::LiveStt,
        }
    }

    #[test]
    fn livestt_toggle_idle_press_starts() {
        assert_eq!(
            input_action(&Stage::Idle, "transcribe", true, false),
            InputAction::Start
        );
    }

    #[test]
    fn livestt_toggle_starting_same_binding_press_stops() {
        assert_eq!(
            input_action(&starting("transcribe"), "transcribe", true, false),
            InputAction::Stop
        );
    }

    #[test]
    fn livestt_recording_started_moves_starting_to_recording() {
        let mut stage = starting("transcribe");

        apply_recording_started(&mut stage, "transcribe");

        assert_eq!(stage, recording("transcribe"));
    }

    #[test]
    fn livestt_start_failed_moves_starting_to_idle() {
        let mut stage = starting("transcribe");

        apply_start_failed(&mut stage, "transcribe");

        assert_eq!(stage, Stage::Idle);
    }

    #[test]
    fn livestt_recording_same_binding_press_stops() {
        assert_eq!(
            input_action(&recording("transcribe"), "transcribe", true, false),
            InputAction::Stop
        );
    }

    #[test]
    fn livestt_starting_stop_without_pipeline_returns_idle() {
        let mut stage = starting("transcribe");

        apply_stop_result(&mut stage, TranscriptionBackend::LiveStt, false);

        assert_eq!(stage, Stage::Idle);
    }

    #[test]
    fn livestt_starting_stop_with_pipeline_moves_processing() {
        let mut stage = starting("transcribe");

        apply_stop_result(&mut stage, TranscriptionBackend::LiveStt, true);

        assert_eq!(stage, Stage::Processing);
    }

    #[test]
    fn livestt_recording_stop_with_pipeline_moves_processing() {
        let mut stage = recording("transcribe");

        apply_stop_result(&mut stage, TranscriptionBackend::LiveStt, true);

        assert_eq!(stage, Stage::Processing);
    }

    #[test]
    fn processing_finished_returns_idle() {
        let mut stage = Stage::Processing;

        apply_processing_finished(&mut stage);

        assert_eq!(stage, Stage::Idle);
    }

    #[test]
    fn push_to_talk_release_while_starting_stops() {
        assert_eq!(
            input_action(&starting("transcribe"), "transcribe", false, true),
            InputAction::Stop
        );
    }

    #[test]
    fn different_binding_while_busy_is_ignored() {
        assert_eq!(
            input_action(
                &starting("transcribe"),
                "transcribe_with_post_process",
                true,
                false
            ),
            InputAction::Ignore
        );
        assert_eq!(
            input_action(
                &recording("transcribe"),
                "transcribe_with_post_process",
                false,
                true
            ),
            InputAction::Ignore
        );
    }
}
