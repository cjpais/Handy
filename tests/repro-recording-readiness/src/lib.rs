#![cfg(test)]

use loom::sync::atomic::{AtomicU64, Ordering};
use loom::sync::{Arc, Mutex};
use std::time::Duration;

macro_rules! debug {
    ($($argument:tt)*) => {{ let _ = format_args!($($argument)*); }};
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cue {
    Ui,
    Chime,
    Mute,
}

#[derive(Default, Debug)]
struct Effects {
    newer_session: bool,
    newer_ui: bool,
    stale: Vec<Cue>,
    cues: Vec<Cue>,
    trace: Vec<String>,
    system_muted: Option<bool>,
    mute_enabled: bool,
    snapshots: usize,
    restores: usize,
}

type MainThreadJob = Box<dyn FnOnce() + Send>;

#[derive(Clone)]
pub struct AppHandle {
    effects: Arc<Mutex<Effects>>,
    main_thread: Arc<Mutex<Vec<MainThreadJob>>>,
}

impl AppHandle {
    fn cue(&self, cue: Cue) {
        let mut effects = self.effects.lock().unwrap();
        let stale = match cue {
            Cue::Ui => effects.newer_ui,
            Cue::Chime | Cue::Mute => effects.newer_session,
        };
        if stale {
            effects.stale.push(cue);
        }
        effects.cues.push(cue);
        effects.trace.push(format!("dispatch {cue:?}"));
    }

    fn run_on_main_thread(&self, job: impl FnOnce() + Send + 'static) -> Result<(), ()> {
        self.main_thread.lock().unwrap().push(Box::new(job));
        Ok(())
    }

    fn emit_to(&self, target: &str, event: &str, _: ()) -> Result<(), ()> {
        assert_eq!(target, "recording_overlay");
        assert_eq!(event, "recording-ready");
        self.cue(Cue::Ui);
        Ok(())
    }

    fn drain_main_thread(&self) {
        let jobs = std::mem::take(&mut *self.main_thread.lock().unwrap());
        for job in jobs {
            job();
        }
    }

    fn get_mute(&self) -> Option<bool> {
        let mut effects = self.effects.lock().unwrap();
        effects.snapshots += 1;
        effects.system_muted
    }

    fn set_mute(&self, muted: bool) {
        assert!(muted, "apply_mute must request mute");
        self.cue(Cue::Mute);
        self.effects.lock().unwrap().system_muted = Some(muted);
    }

    fn restore_mute(&self, previous: Option<bool>) {
        let mut effects = self.effects.lock().unwrap();
        effects.restores += 1;
        if previous != Some(true) {
            effects.system_muted = Some(false);
        }
    }
}

static OVERLAY_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

mod utils {
    pub use super::emit_recording_ready;
}

enum SoundType {
    Start,
}

fn play_feedback_sound_blocking(app: &AppHandle, _: SoundType) {
    app.cue(Cue::Chime);
}

struct Settings {
    mute_while_recording: bool,
}

fn get_settings(app: &AppHandle) -> Settings {
    Settings {
        mute_while_recording: app.effects.lock().unwrap().mute_enabled,
    }
}

struct Ready;

impl Ready {
    fn wait(&self) -> bool {
        true
    }
}

struct AudioRecordingManager {
    capture_generation: Arc<AtomicU64>,
    app_handle: AppHandle,
    is_open: Arc<Mutex<bool>>,
    mute_state: Arc<Mutex<MuteState>>,
}

include!(concat!(env!("OUT_DIR"), "/production.rs"));

fn setup(
    previous: Option<bool>,
    enabled: bool,
    open: bool,
) -> (Arc<AudioRecordingManager>, AppHandle) {
    let app = AppHandle {
        effects: Arc::new(Mutex::new(Effects {
            system_muted: previous,
            mute_enabled: enabled,
            ..Effects::default()
        })),
        main_thread: Arc::new(Mutex::new(Vec::new())),
    };
    let audio = Arc::new(AudioRecordingManager {
        capture_generation: Arc::new(AtomicU64::new(0)),
        app_handle: app.clone(),
        is_open: Arc::new(Mutex::new(open)),
        mute_state: Arc::new(Mutex::new(MuteState::default())),
    });
    (audio, app)
}

fn builder() -> loom::model::Builder {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 1_000;
    builder.max_permutations = None;
    builder.max_duration = None;
    builder.preemption_bound = None;
    builder.checkpoint_file = None;
    builder
}

fn race(target: Cue) {
    builder().check(move || {
        let (audio, app) = setup(Some(false), true, true);
        let ready_audio = Arc::clone(&audio);
        let ready_app = app.clone();
        let ready = loom::thread::spawn(move || production_ready(ready_audio, 0, ready_app));
        let ended_app = app.clone();
        let ended = loom::thread::spawn(move || {
            audio.invalidate_recording_readiness();
            audio.remove_mute();
            {
                let mut effects = ended_app.effects.lock().unwrap();
                effects.newer_session = true;
                effects.trace.push("invalidation and remove_mute completed; newer session owns effects".into());
            }
            let shown_app = ended_app.clone();
            ended_app.run_on_main_thread(move || {
                let mut effects = shown_app.effects.lock().unwrap();
                effects.newer_ui = true;
                effects.trace.push("simulate newer recording show-overlay on main thread".into());
            }).unwrap();
        });
        ended.join().unwrap();
        ready.join().unwrap();
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert!(
            !effects.stale.contains(&target),
            "ended readiness dispatched {target:?} after invalidation and restore: {:?}; system_muted={:?}",
            effects.trace, effects.system_muted
        );
    });
}

#[test]
fn readiness_ui_never_changes_newer_session() {
    race(Cue::Ui);
}

#[test]
fn readiness_chime_never_follows_ended_session() {
    race(Cue::Chime);
}

#[test]
fn readiness_mute_never_changes_newer_session() {
    race(Cue::Mute);
}

#[test]
fn current_readiness_dispatches_all_effects() {
    builder().check(|| {
        let (audio, app) = setup(Some(false), true, true);
        production_ready(audio, 0, app.clone());
        assert_eq!(app.main_thread.lock().unwrap().len(), 1);
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert_eq!(effects.cues, [Cue::Chime, Cue::Mute, Cue::Ui]);
        assert_eq!(effects.system_muted, Some(true));
    });
}

#[test]
fn already_invalidated_readiness_dispatches_nothing() {
    builder().check(|| {
        let (audio, app) = setup(Some(false), true, true);
        audio.invalidate_recording_readiness();
        production_ready(audio, 0, app.clone());
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert!(effects.cues.is_empty());
        assert_eq!(effects.system_muted, Some(false));
    });
}

#[test]
fn mute_current_duplicate_and_restore_controls() {
    builder().check(|| {
        for previous in [Some(false), Some(true), None] {
            let (audio, app) = setup(previous, true, true);
            audio.apply_mute();
            audio.apply_mute();
            assert_eq!(audio.mute_state.lock().unwrap().prev_muted, previous);
            audio.remove_mute();
            audio.remove_mute();
            let effects = app.effects.lock().unwrap();
            assert_eq!(effects.snapshots, 1);
            assert_eq!(effects.restores, 1);
            assert_eq!(effects.cues, [Cue::Mute]);
            assert_eq!(effects.system_muted, Some(previous == Some(true)));
        }
    });
}

#[test]
fn disabled_mute_and_closed_stream_do_not_mute() {
    builder().check(|| {
        for (enabled, open) in [(false, true), (true, false)] {
            let (audio, app) = setup(Some(false), enabled, open);
            audio.apply_mute();
            audio.remove_mute();
            let effects = app.effects.lock().unwrap();
            assert!(effects.cues.is_empty());
            assert_eq!(effects.snapshots, 0);
            assert_eq!(effects.restores, 0);
            assert_eq!(effects.system_muted, Some(false));
        }
    });
}
