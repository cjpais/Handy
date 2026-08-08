use crate::actions::ACTION_MAP;
use crate::managers::audio::AudioRecordingManager;
use log::{debug, error, warn};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const DEBOUNCE: Duration = Duration::from_millis(30);
const RELEASE_GRACE: Duration = Duration::from_millis(50);
/// Hold-or-double-tap: a press held at least this long is push-to-talk;
/// anything shorter is a tap.
const HOLD_THRESHOLD: Duration = Duration::from_millis(300);
/// Hold-or-double-tap: how long after a tap's release a second tap may still
/// lock the session on. When it elapses the recording is discarded.
const SECOND_TAP_WINDOW: Duration = Duration::from_millis(150);

/// How a transcribe binding's key events drive the recording session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Press starts recording, pressing again stops it.
    Toggle,
    /// Hold to record, release to stop.
    PushToTalk,
    /// Hold for push-to-talk; double-tap to start an ongoing session
    /// (stopped by the next press); a lone tap discards the recording.
    HoldOrDoubleTap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PttAction {
    Passthrough,
    DeferRelease,
    CancelRelease,
}

#[derive(Clone, Copy)]
enum PendingKind {
    /// A push-to-talk release deferred by `RELEASE_GRACE` to absorb X11
    /// auto-repeat.
    PttRelease,
    /// A hold-or-double-tap release deferred by `RELEASE_GRACE`; classified
    /// as hold or tap once the grace elapses.
    HybridRelease {
        pressed_at: Instant,
        released_at: Instant,
    },
    /// A tap happened; recording continues until a second tap locks the
    /// session on or the window elapses and the recording is discarded.
    SecondTap,
}

struct Pending {
    binding_id: String,
    hotkey_string: String,
    deadline: Instant,
    kind: PendingKind,
}

/// Commands processed sequentially by the coordinator thread.
enum Command {
    Input {
        binding_id: String,
        hotkey_string: String,
        is_pressed: bool,
        mode: InputMode,
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

fn classify_ptt_event(
    pending_release_binding: Option<&str>,
    is_pressed: bool,
    push_to_talk: bool,
    binding_id: &str,
    recording_binding: Option<&str>,
) -> PttAction {
    if !push_to_talk {
        return PttAction::Passthrough;
    }

    if is_pressed {
        if pending_release_binding == Some(binding_id) {
            PttAction::CancelRelease
        } else {
            PttAction::Passthrough
        }
    } else if recording_binding == Some(binding_id) && pending_release_binding.is_none() {
        PttAction::DeferRelease
    } else {
        PttAction::Passthrough
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
                let mut pending: Option<Pending> = None;
                // Hold-or-double-tap bookkeeping for the current recording.
                let mut hold_started: Option<Instant> = None;
                let mut session_locked = false;

                loop {
                    let cmd = if let Some(p) = &pending {
                        match rx.recv_timeout(p.deadline.saturating_duration_since(Instant::now()))
                        {
                            Ok(cmd) => cmd,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                if let Some(expired) = pending.take() {
                                    pending = expire_pending(&app, &mut stage, expired);
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match rx.recv() {
                            Ok(cmd) => cmd,
                            Err(_) => break,
                        }
                    };

                    match cmd {
                        Command::Input {
                            binding_id,
                            hotkey_string,
                            is_pressed,
                            mode,
                        } => {
                            let recording_binding = match &stage {
                                Stage::Recording(id) => Some(id.as_str()),
                                _ => None,
                            };

                            match mode {
                                InputMode::Toggle | InputMode::PushToTalk => {
                                    let pending_binding =
                                        pending.as_ref().map(|p| p.binding_id.as_str());
                                    match classify_ptt_event(
                                        pending_binding,
                                        is_pressed,
                                        mode == InputMode::PushToTalk,
                                        &binding_id,
                                        recording_binding,
                                    ) {
                                        PttAction::CancelRelease => {
                                            pending = None;
                                            continue;
                                        }
                                        PttAction::DeferRelease => {
                                            pending = Some(Pending {
                                                binding_id,
                                                hotkey_string,
                                                deadline: Instant::now() + RELEASE_GRACE,
                                                kind: PendingKind::PttRelease,
                                            });
                                            continue;
                                        }
                                        PttAction::Passthrough => {}
                                    }
                                }
                                InputMode::HoldOrDoubleTap => {
                                    let (press_cancels_grace, press_is_second_tap) = match &pending
                                    {
                                        Some(p) if is_pressed && p.binding_id == binding_id => (
                                            matches!(p.kind, PendingKind::HybridRelease { .. }),
                                            matches!(p.kind, PendingKind::SecondTap),
                                        ),
                                        _ => (false, false),
                                    };
                                    if press_cancels_grace {
                                        // Auto-repeat press inside the release
                                        // grace: the key is still held.
                                        pending = None;
                                        continue;
                                    }
                                    if press_is_second_tap {
                                        debug!("Second tap for '{binding_id}': session locked on");
                                        pending = None;
                                        session_locked = true;
                                        continue;
                                    }
                                    if !is_pressed {
                                        if !session_locked
                                            && pending.is_none()
                                            && recording_binding == Some(binding_id.as_str())
                                        {
                                            // Defer the release; hold vs tap is
                                            // classified when the grace elapses (an
                                            // auto-repeat press may cancel it first).
                                            pending = Some(Pending {
                                                binding_id,
                                                hotkey_string,
                                                deadline: Instant::now() + RELEASE_GRACE,
                                                kind: PendingKind::HybridRelease {
                                                    pressed_at: hold_started
                                                        .unwrap_or_else(Instant::now),
                                                    released_at: Instant::now(),
                                                },
                                            });
                                        }
                                        continue;
                                    }
                                }
                            }

                            // Debounce rapid-fire press events (key repeat / double-tap).
                            // Push-to-talk releases may be deferred above to absorb X11 auto-repeat.
                            if is_pressed {
                                let now = Instant::now();
                                if last_press.is_some_and(|t| now.duration_since(t) < DEBOUNCE) {
                                    debug!("Debounced press for '{binding_id}'");
                                    continue;
                                }
                                last_press = Some(now);
                            }

                            match mode {
                                InputMode::PushToTalk => {
                                    if is_pressed && matches!(stage, Stage::Idle) {
                                        start(&app, &mut stage, &binding_id, &hotkey_string);
                                    } else if !is_pressed
                                        && matches!(&stage, Stage::Recording(id) if id == &binding_id)
                                    {
                                        stop(&app, &mut stage, &binding_id, &hotkey_string);
                                    }
                                }
                                InputMode::Toggle => {
                                    if is_pressed {
                                        match &stage {
                                            Stage::Idle => {
                                                start(
                                                    &app,
                                                    &mut stage,
                                                    &binding_id,
                                                    &hotkey_string,
                                                );
                                            }
                                            Stage::Recording(id) if id == &binding_id => {
                                                stop(&app, &mut stage, &binding_id, &hotkey_string);
                                            }
                                            _ => {
                                                debug!(
                                                    "Ignoring press for '{binding_id}': pipeline busy"
                                                )
                                            }
                                        }
                                    }
                                }
                                InputMode::HoldOrDoubleTap => {
                                    // Releases were consumed above; this is a press.
                                    match &stage {
                                        Stage::Idle => {
                                            start(&app, &mut stage, &binding_id, &hotkey_string);
                                            if matches!(stage, Stage::Recording(_)) {
                                                hold_started = Some(Instant::now());
                                                session_locked = false;
                                            }
                                        }
                                        Stage::Recording(id) if id == &binding_id => {
                                            if session_locked {
                                                stop(&app, &mut stage, &binding_id, &hotkey_string);
                                            } else {
                                                debug!(
                                                    "Ignoring press for '{binding_id}' while held"
                                                )
                                            }
                                        }
                                        _ => {
                                            debug!(
                                                "Ignoring press for '{binding_id}': pipeline busy"
                                            )
                                        }
                                    }
                                }
                            }
                        }
                        Command::Cancel {
                            recording_was_active,
                        } => {
                            pending = None;
                            // Don't reset during processing — wait for the pipeline to finish.
                            if !matches!(stage, Stage::Processing)
                                && (recording_was_active || matches!(stage, Stage::Recording(_)))
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
    /// For signal-based toggles, use `is_pressed: true` and `InputMode::Toggle`.
    pub fn send_input(
        &self,
        binding_id: &str,
        hotkey_string: &str,
        is_pressed: bool,
        mode: InputMode,
    ) {
        if self
            .tx
            .send(Command::Input {
                binding_id: binding_id.to_string(),
                hotkey_string: hotkey_string.to_string(),
                is_pressed,
                mode,
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

/// Resolve a pending deadline that elapsed with no cancelling input, returning
/// the follow-up pending state (a tap awaiting its second press), if any.
fn expire_pending(app: &AppHandle, stage: &mut Stage, expired: Pending) -> Option<Pending> {
    match &*stage {
        Stage::Recording(id) if *id == expired.binding_id => {}
        _ => return None,
    }
    match expired.kind {
        PendingKind::PttRelease => {
            stop(app, stage, &expired.binding_id, &expired.hotkey_string);
            None
        }
        PendingKind::HybridRelease {
            pressed_at,
            released_at,
        } => {
            if released_at.saturating_duration_since(pressed_at) >= HOLD_THRESHOLD {
                stop(app, stage, &expired.binding_id, &expired.hotkey_string);
                None
            } else {
                Some(Pending {
                    deadline: released_at + SECOND_TAP_WINDOW,
                    kind: PendingKind::SecondTap,
                    ..expired
                })
            }
        }
        PendingKind::SecondTap => {
            debug!(
                "Lone tap for '{}': discarding recording",
                expired.binding_id
            );
            crate::utils::cancel_current_operation(app);
            *stage = Stage::Idle;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_to_talk_release_while_recording_defers_release() {
        assert_eq!(
            classify_ptt_event(None, false, true, "transcribe", Some("transcribe")),
            PttAction::DeferRelease
        );
    }

    #[test]
    fn push_to_talk_press_matching_pending_release_cancels_release() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                true,
                "transcribe",
                Some("transcribe")
            ),
            PttAction::CancelRelease
        );
    }

    #[test]
    fn toggle_mode_press_and_release_pass_through() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                false,
                "transcribe",
                Some("transcribe")
            ),
            PttAction::Passthrough
        );
        assert_eq!(
            classify_ptt_event(None, false, false, "transcribe", Some("transcribe")),
            PttAction::Passthrough
        );
    }

    #[test]
    fn press_for_different_binding_than_pending_release_passes_through() {
        assert_eq!(
            classify_ptt_event(
                Some("transcribe"),
                true,
                true,
                "transcribe_with_post_process",
                Some("transcribe")
            ),
            PttAction::Passthrough
        );
    }

    #[test]
    fn press_matching_pending_release_cancels_without_recording_state() {
        assert_eq!(
            classify_ptt_event(Some("transcribe"), true, true, "transcribe", None),
            PttAction::CancelRelease
        );
    }

    // ---------------------------------------------------------------------
    // Sequence-level regression coverage for issue #1539.
    //
    // Under X11 key auto-repeat, holding a push-to-talk key does not emit one
    // long press. It emits the initial press followed by a stream of
    // synthesized release/press pairs, then a single genuine release on key-up.
    // Before the fix, every synthesized release passed straight through and
    // stopped recording, so holding the key "rapidly toggled" recording on and
    // off. The fix defers each release for a short grace window and cancels it
    // when the matching auto-repeat press arrives.
    //
    // The unit tests above assert `classify_ptt_event` in isolation. The
    // simulator below threads that classifier through the same `pending_release`
    // / `stage` state transitions the coordinator loop performs (lines that
    // handle `Command::Input` and the `recv_timeout` grace expiry), so a whole
    // event burst can be exercised deterministically without a Tauri AppHandle
    // or real timers.
    // ---------------------------------------------------------------------

    const BINDING: &str = "transcribe";

    #[derive(Clone, Copy)]
    enum Ev {
        /// A key-down event (real initial press or a synthesized auto-repeat press).
        Press,
        /// A key-up event (synthesized auto-repeat release or the genuine key-up).
        Release,
        /// The `RELEASE_GRACE` window elapsed with no cancelling press arriving.
        Grace,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum SimStage {
        Idle,
        Recording,
        Processing,
    }

    struct SimResult {
        starts: u32,
        stops: u32,
        stage: SimStage,
    }

    /// Mirror of the coordinator loop's decision logic for a single push-to-talk
    /// binding: it calls the real `classify_ptt_event` and applies the exact same
    /// Defer / Cancel / debounce / start / stop transitions.
    fn simulate(events: &[Ev]) -> SimResult {
        let mut stage = SimStage::Idle;
        let mut pending: Option<String> = None;
        let mut last_press_ms: Option<u64> = None;
        let mut clock_ms: u64 = 0;
        let mut starts = 0u32;
        let mut stops = 0u32;
        let debounce_ms = DEBOUNCE.as_millis() as u64;

        for ev in events {
            // Auto-repeat events arrive a few ms apart, well inside DEBOUNCE.
            clock_ms += 5;

            match ev {
                Ev::Grace => {
                    // Coordinator's `RecvTimeoutError::Timeout` arm: fire the
                    // deferred release iff we are still recording that binding.
                    if let Some(pending_binding) = pending.take() {
                        if stage == SimStage::Recording && pending_binding == BINDING {
                            stage = SimStage::Processing;
                            stops += 1;
                        }
                    }
                }
                Ev::Press | Ev::Release => {
                    let is_pressed = matches!(ev, Ev::Press);
                    let pending_binding = pending.as_deref();
                    let recording_binding = if stage == SimStage::Recording {
                        Some(BINDING)
                    } else {
                        None
                    };

                    match classify_ptt_event(
                        pending_binding,
                        is_pressed,
                        true, // push_to_talk
                        BINDING,
                        recording_binding,
                    ) {
                        PttAction::CancelRelease => {
                            pending = None;
                            continue;
                        }
                        PttAction::DeferRelease => {
                            pending = Some(BINDING.to_string());
                            continue;
                        }
                        PttAction::Passthrough => {}
                    }

                    if is_pressed {
                        if last_press_ms.is_some_and(|t| clock_ms - t < debounce_ms) {
                            continue;
                        }
                        last_press_ms = Some(clock_ms);
                    }

                    if is_pressed && stage == SimStage::Idle {
                        stage = SimStage::Recording;
                        starts += 1;
                    } else if !is_pressed && stage == SimStage::Recording {
                        stage = SimStage::Processing;
                        stops += 1;
                    }
                }
            }
        }

        SimResult {
            starts,
            stops,
            stage,
        }
    }

    /// Initial press plus several synthesized release/press pairs, as X11 emits
    /// while a push-to-talk key is held down.
    fn autorepeat_burst() -> Vec<Ev> {
        let mut events = vec![Ev::Press];
        for _ in 0..6 {
            events.push(Ev::Release);
            events.push(Ev::Press);
        }
        events
    }

    /// Regression for #1539: a burst of X11 auto-repeat release/press pairs must
    /// not stop recording. Before the fix the first synthesized release stopped
    /// recording immediately (stops == 1, stage left Recording), which produced
    /// the rapid on/off toggling. With the fix the releases are coalesced and
    /// recording stays continuously active for the whole burst.
    #[test]
    fn x11_autorepeat_burst_does_not_toggle_recording() {
        let result = simulate(&autorepeat_burst());
        assert_eq!(result.starts, 1, "recording should start exactly once");
        assert_eq!(
            result.stops, 0,
            "synthesized auto-repeat releases must not stop recording mid-burst"
        );
        assert_eq!(
            result.stage,
            SimStage::Recording,
            "recording must remain active across the entire auto-repeat burst"
        );
    }

    /// Complements the burst test: once the key is genuinely released and the
    /// grace window elapses with no re-press, recording stops exactly once. This
    /// proves the debounce only coalesces synthesized releases and does not wedge
    /// the coordinator or swallow the real key-up.
    #[test]
    fn genuine_release_after_grace_stops_recording_once() {
        let mut events = autorepeat_burst();
        events.push(Ev::Release); // genuine key-up
        events.push(Ev::Grace); // grace window elapses, no cancelling press
        let result = simulate(&events);
        assert_eq!(result.starts, 1, "recording should start exactly once");
        assert_eq!(
            result.stops, 1,
            "a genuine release should stop recording exactly once"
        );
        assert_eq!(result.stage, SimStage::Processing);
    }

    // ---------------------------------------------------------------------
    // Hold-or-double-tap mode. Like `simulate` above, `simulate_hybrid`
    // mirrors the coordinator loop's HoldOrDoubleTap arms (pending grace /
    // tap-window state and stage transitions) on a synthetic millisecond
    // clock, firing elapsed deadlines exactly as `expire_pending` does.
    // ---------------------------------------------------------------------

    #[derive(Clone, Copy)]
    enum HEv {
        Press,
        Release,
    }

    enum HPending {
        Grace { pressed_at: u64, released_at: u64 },
        TapWindow,
    }

    struct HybridResult {
        starts: u32,
        stops: u32,
        cancels: u32,
        recording: bool,
    }

    /// Run timestamped press/release events through the hybrid state machine,
    /// firing pending deadlines that fall due, then flush any remaining
    /// deadline at `end_ms`.
    fn simulate_hybrid(events: &[(u64, HEv)], end_ms: u64) -> HybridResult {
        let grace = RELEASE_GRACE.as_millis() as u64;
        let hold = HOLD_THRESHOLD.as_millis() as u64;
        let window = SECOND_TAP_WINDOW.as_millis() as u64;
        let debounce = DEBOUNCE.as_millis() as u64;

        let mut recording = false;
        let mut locked = false;
        let mut hold_started: u64 = 0;
        let mut pending: Option<(u64, HPending)> = None; // (deadline, kind)
        let mut last_press: Option<u64> = None;
        let mut result = HybridResult {
            starts: 0,
            stops: 0,
            cancels: 0,
            recording: false,
        };

        // Fire every pending deadline that falls due by `now`. A grace expiry
        // may arm the tap window, so keep firing until nothing is due — the
        // real loop gets this for free by re-entering `recv_timeout`.
        let fire_deadlines = |now: u64,
                              pending: &mut Option<(u64, HPending)>,
                              recording: &mut bool,
                              result: &mut HybridResult| {
            while let Some((deadline, kind)) = pending.take() {
                if deadline > now {
                    *pending = Some((deadline, kind));
                    return;
                }
                if !*recording {
                    return;
                }
                match kind {
                    HPending::Grace {
                        pressed_at,
                        released_at,
                    } => {
                        if released_at - pressed_at >= hold {
                            *recording = false;
                            result.stops += 1;
                        } else {
                            *pending = Some((released_at + window, HPending::TapWindow));
                        }
                    }
                    HPending::TapWindow => {
                        *recording = false;
                        result.cancels += 1;
                    }
                }
            }
        };

        for &(t, ev) in events {
            fire_deadlines(t, &mut pending, &mut recording, &mut result);
            match ev {
                HEv::Press => {
                    match pending {
                        Some((_, HPending::Grace { .. })) => {
                            // Auto-repeat press inside the grace: still held.
                            pending = None;
                            continue;
                        }
                        Some((_, HPending::TapWindow)) => {
                            pending = None;
                            locked = true;
                            continue;
                        }
                        None => {}
                    }
                    if last_press.is_some_and(|p| t - p < debounce) {
                        continue;
                    }
                    last_press = Some(t);
                    if !recording {
                        recording = true;
                        locked = false;
                        hold_started = t;
                        result.starts += 1;
                    } else if locked {
                        recording = false;
                        result.stops += 1;
                    }
                }
                HEv::Release => {
                    if recording && !locked && pending.is_none() {
                        pending = Some((
                            t + grace,
                            HPending::Grace {
                                pressed_at: hold_started,
                                released_at: t,
                            },
                        ));
                    }
                }
            }
        }
        fire_deadlines(end_ms, &mut pending, &mut recording, &mut result);
        result.recording = recording;
        result
    }

    /// A press held past HOLD_THRESHOLD is plain push-to-talk: release stops
    /// and transcribes once the grace elapses.
    #[test]
    fn hybrid_hold_stops_on_release() {
        let r = simulate_hybrid(&[(0, HEv::Press), (500, HEv::Release)], 2000);
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 1);
        assert_eq!(r.cancels, 0);
        assert!(!r.recording);
    }

    /// A lone tap is an accident: the recording is discarded when the second
    /// tap window elapses, and nothing is transcribed.
    #[test]
    fn hybrid_lone_tap_is_discarded() {
        let r = simulate_hybrid(&[(0, HEv::Press), (100, HEv::Release)], 2000);
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 0);
        assert_eq!(r.cancels, 1);
        assert!(!r.recording);
    }

    /// A second-tap press timed to land inside the tap window (past the
    /// auto-repeat grace, before the window closes), robust to tuning of
    /// `SECOND_TAP_WINDOW`.
    fn second_tap_at(released_ms: u64) -> u64 {
        let grace = RELEASE_GRACE.as_millis() as u64;
        let window = SECOND_TAP_WINDOW.as_millis() as u64;
        released_ms + grace + (window - grace) / 2
    }

    /// Tap-tap locks an ongoing session that a later single press stops.
    #[test]
    fn hybrid_double_tap_locks_session_until_next_press() {
        let tap2 = second_tap_at(100);
        let r = simulate_hybrid(
            &[
                (0, HEv::Press),
                (100, HEv::Release),
                (tap2, HEv::Press),        // second tap: session locked on
                (tap2 + 80, HEv::Release), // ignored while locked
                (2000, HEv::Press),        // ends the session
            ],
            3000,
        );
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 1);
        assert_eq!(r.cancels, 0);
        assert!(!r.recording);
    }

    /// A locked session survives arbitrary release events and stays recording
    /// until the next press.
    #[test]
    fn hybrid_locked_session_ignores_releases() {
        let r = simulate_hybrid(
            &[
                (0, HEv::Press),
                (100, HEv::Release),
                (second_tap_at(100), HEv::Press),
                (900, HEv::Release), // long-held second press releasing changes nothing
            ],
            3000,
        );
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 0);
        assert_eq!(r.cancels, 0);
        assert!(r.recording, "locked session must stay recording");
    }

    /// X11 auto-repeat while holding: synthesized release/press pairs arrive a
    /// few ms apart and must neither stop the recording nor register as taps.
    #[test]
    fn hybrid_autorepeat_during_hold_is_absorbed() {
        let r = simulate_hybrid(
            &[
                (0, HEv::Press),
                (400, HEv::Release), // synthesized
                (405, HEv::Press),   // cancels the grace: still held
                (700, HEv::Release), // synthesized
                (705, HEv::Press),
                (1000, HEv::Release), // genuine key-up
            ],
            3000,
        );
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 1, "only the genuine release stops the recording");
        assert_eq!(r.cancels, 0);
        assert!(!r.recording);
    }

    /// The hold classification uses the initial press, not the last auto-repeat
    /// press: a tap-length gap between synthesized events must not reclassify a
    /// long hold as a tap.
    #[test]
    fn hybrid_hold_duration_measured_from_initial_press() {
        let r = simulate_hybrid(
            &[
                (0, HEv::Press),
                (200, HEv::Release), // synthesized before HOLD_THRESHOLD
                (210, HEv::Press),   // cancels the grace: still held
                (600, HEv::Release), // genuine key-up: held 600ms total
            ],
            3000,
        );
        assert_eq!(r.starts, 1);
        assert_eq!(r.stops, 1, "600ms hold must stop-and-transcribe, not tap");
        assert_eq!(r.cancels, 0);
    }
}
