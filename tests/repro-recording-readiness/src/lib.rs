#![cfg(test)]

use loom::sync::{Arc, Mutex};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{atomic::Ordering, mpsc};
use std::time::Duration;

macro_rules! debug { ($($argument:tt)*) => {{ let _ = format_args!($($argument)*); }}; }
macro_rules! warn { ($($argument:tt)*) => {{ let _ = format_args!($($argument)*); }}; }
macro_rules! error { ($($argument:tt)*) => {{ let _ = format_args!($($argument)*); }}; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cue {
    Ui,
    Chime,
}

#[derive(Default, Debug)]
struct Effects {
    invalidated: bool,
    newer_ui: bool,
    stale: Vec<Cue>,
    cues: Vec<Cue>,
    completions: usize,
}

type MainThreadJob = Box<dyn FnOnce() + Send>;
type Hook = std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>>;

#[derive(Clone)]
pub struct AppHandle {
    effects: Arc<Mutex<Effects>>,
    main_thread: Arc<Mutex<Vec<MainThreadJob>>>,
    inline: bool,
    prepare_hook: Hook,
    sleep_hook: Hook,
    feedback_enabled: bool,
}

impl AppHandle {
    fn cue(&self, cue: Cue) {
        let mut effects = self.effects.lock().unwrap();
        if effects.invalidated || (cue == Cue::Ui && effects.newer_ui) {
            effects.stale.push(cue);
        }
        effects.cues.push(cue);
    }

    fn run_on_main_thread(&self, job: impl FnOnce() + Send + 'static) -> Result<(), ()> {
        if self.inline {
            job();
        } else {
            self.main_thread.lock().unwrap().push(Box::new(job));
        }
        Ok(())
    }

    fn emit_to(&self, target: &str, event: &str, _: ()) -> Result<(), ()> {
        assert_eq!((target, event), ("recording_overlay", "recording-ready"));
        self.cue(Cue::Ui);
        Ok(())
    }

    fn drain_main_thread(&self) {
        let jobs = std::mem::take(&mut *self.main_thread.lock().unwrap());
        for job in jobs {
            job();
        }
    }

    fn show_newer(&self) {
        let app = self.clone();
        self.run_on_main_thread(move || app.effects.lock().unwrap().newer_ui = true)
            .unwrap();
    }
}

static OVERLAY_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
mod utils {
    pub(crate) use super::emit_recording_ready;
}
enum SoundType {
    Start,
}

mod settings {
    use super::*;
    pub struct Settings {
        pub audio_feedback: bool,
        pub selected_output_device: Option<String>,
        pub audio_feedback_volume: f32,
    }
    pub fn get_settings(app: &AppHandle) -> Settings {
        if app.feedback_enabled {
            ACTIVE_APP.with(|active| *active.borrow_mut() = Some(app.clone()));
        }
        Settings {
            audio_feedback: app.feedback_enabled,
            selected_output_device: None,
            audio_feedback_volume: 0.5,
        }
    }
}
fn resolve_sound_path(_: &AppHandle, _: &settings::Settings, _: SoundType) -> Option<PathBuf> {
    Some(PathBuf::from("fixture.wav"))
}
loom::thread_local! { static ACTIVE_APP: std::cell::RefCell<Option<AppHandle>> = std::cell::RefCell::new(None); }
fn active_app() -> AppHandle {
    ACTIVE_APP.with(|active| active.borrow_mut().take().unwrap())
}
fn run_hook(hook: &Hook) {
    let callback = hook.lock().unwrap().take();
    if let Some(callback) = callback {
        callback();
    }
}

struct DeviceSinkBuilder(AppHandle);
struct DeviceSink(AppHandle);
struct Device;
struct Host;
impl DeviceSinkBuilder {
    fn from_default_device() -> Result<Self, std::io::Error> {
        Ok(Self(active_app()))
    }
    fn from_device(_: Device) -> Result<Self, std::io::Error> {
        Ok(Self(active_app()))
    }
    fn open_stream(self) -> Result<DeviceSink, std::io::Error> {
        run_hook(&self.0.prepare_hook);
        loom::thread::yield_now();
        Ok(DeviceSink(self.0))
    }
}
impl DeviceSink {
    fn mixer(&self) -> &Self {
        self
    }
}
mod audio_toolkit {
    pub(crate) fn get_cpal_host() -> super::Host {
        super::Host
    }
}
impl Host {
    fn output_devices(&self) -> Result<std::vec::IntoIter<Device>, std::io::Error> {
        Ok(vec![Device].into_iter())
    }
}
fn output_device_name(_: &Device) -> Option<String> {
    Some("fixture".into())
}
struct File;
impl File {
    fn open(_: &Path) -> Result<std::io::Cursor<Vec<u8>>, std::io::Error> {
        Ok(std::io::Cursor::new(vec![]))
    }
}
mod rodio {
    use super::*;
    pub struct Decoder;
    impl Decoder {
        pub fn new(_: BufReader<std::io::Cursor<Vec<u8>>>) -> Result<Self, std::io::Error> {
            loom::thread::yield_now();
            Ok(Self)
        }
    }
    pub struct Player {
        app: AppHandle,
    }
    impl Player {
        pub fn connect_new(mixer: &DeviceSink) -> Self {
            Self {
                app: mixer.0.clone(),
            }
        }
        pub fn set_volume(&self, volume: f32) {
            assert_eq!(volume, 0.5);
        }
        pub fn append(&self, _: Decoder) {
            self.app.cue(Cue::Chime);
        }
        pub fn sleep_until_end(&self) {
            run_hook(&self.app.sleep_hook);
            self.app.effects.lock().unwrap().completions += 1;
        }
    }
}

struct AudioRecordingManager {
    capture_generation: Arc<Mutex<u64>>,
    state: Arc<Mutex<RecordingState>>,
    cancel_generation: Arc<loom::sync::atomic::AtomicU64>,
}
impl AudioRecordingManager {
    fn apply_mute(&self) {}
}
mod anyhow {
    macro_rules! message {
        ($message:literal) => {
            String::from($message)
        };
    }
    pub(crate) use message as anyhow;
}
include!(concat!(env!("OUT_DIR"), "/production.rs"));

fn setup(inline: bool) -> (Arc<AudioRecordingManager>, AppHandle) {
    let app = AppHandle {
        effects: Arc::new(Mutex::new(Effects::default())),
        main_thread: Arc::new(Mutex::new(Vec::new())),
        inline,
        prepare_hook: std::sync::Arc::new(std::sync::Mutex::new(None)),
        sleep_hook: std::sync::Arc::new(std::sync::Mutex::new(None)),
        feedback_enabled: true,
    };
    let audio = Arc::new(AudioRecordingManager {
        capture_generation: Arc::new(Mutex::new(0)),
        state: Arc::new(Mutex::new(RecordingState::Idle)),
        cancel_generation: Arc::new(loom::sync::atomic::AtomicU64::new(0)),
    });
    (audio, app)
}
fn ready(audio: &AudioRecordingManager) -> RecordingReadiness {
    let ownership = audio.begin_recording_readiness();
    let (sender, receiver) = mpsc::channel();
    sender.send(()).unwrap();
    RecordingReadiness {
        receiver,
        ownership,
    }
}
fn invalidate(audio: &AudioRecordingManager, app: &AppHandle) {
    audio.invalidate_recording_readiness();
    app.effects.lock().unwrap().invalidated = true;
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
        let (audio, app) = setup(false);
        let readiness = ready(&audio);
        let ready_audio = audio.clone();
        let ready_app = app.clone();
        let worker =
            loom::thread::spawn(move || production_ready(ready_audio, readiness, ready_app));
        invalidate(&audio, &app);
        app.show_newer();
        worker.join().unwrap();
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert!(
            !effects.stale.contains(&target),
            "ended readiness dispatched {target:?}: {effects:?}"
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
fn current_readiness_dispatches_both_cues() {
    builder().check(|| {
        let (audio, app) = setup(false);
        production_ready(audio.clone(), ready(&audio), app.clone());
        assert_eq!(app.main_thread.lock().unwrap().len(), 1);
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert_eq!(effects.cues, [Cue::Chime, Cue::Ui]);
        assert_eq!(effects.completions, 1);
    });
}
#[test]
fn already_invalidated_readiness_dispatches_nothing() {
    builder().check(|| {
        let (audio, app) = setup(false);
        let readiness = ready(&audio);
        invalidate(&audio, &app);
        production_ready(audio, readiness, app.clone());
        app.drain_main_thread();
        assert_eq!(app.effects.lock().unwrap().cues, []);
    });
}
#[test]
fn queued_readiness_is_revalidated_when_drained() {
    builder().check(|| {
        let (audio, app) = setup(false);
        emit_recording_ready(&app, ready(&audio).ownership());
        assert_eq!(app.main_thread.lock().unwrap().len(), 1);
        invalidate(&audio, &app);
        app.drain_main_thread();
        assert_eq!(app.effects.lock().unwrap().cues, []);
    });
}
#[test]
fn old_ready_cannot_follow_new_arming_show() {
    builder().check(|| {
        let (audio, app) = setup(false);
        let old = ready(&audio).ownership();
        invalidate(&audio, &app);
        app.show_newer();
        emit_recording_ready(&app, old);
        app.drain_main_thread();
        let effects = app.effects.lock().unwrap();
        assert!(effects.newer_ui);
        assert_eq!(effects.cues, []);
    });
}
#[test]
fn inline_main_thread_does_not_deadlock() {
    builder().check(|| {
        let (audio, app) = setup(true);
        production_ready(audio.clone(), ready(&audio), app.clone());
        assert_eq!(app.effects.lock().unwrap().cues, [Cue::Ui, Cue::Chime]);
    });
}
#[test]
fn invalidation_during_sound_preparation_prevents_append() {
    builder().check(|| {
        let (audio, app) = setup(false);
        let ownership = ready(&audio).ownership();
        let ended_app = app.clone();
        *app.prepare_hook.lock().unwrap() = Some(Box::new(move || invalidate(&audio, &ended_app)));
        play_start_feedback_if_current(&app, &ownership);
        let effects = app.effects.lock().unwrap();
        assert!(effects.invalidated);
        assert_eq!(effects.cues, []);
        assert_eq!(effects.completions, 0);
    });
}
#[test]
fn invalidation_does_not_wait_for_playback_completion() {
    builder().check(|| {
        let (audio, app) = setup(false);
        let ownership = ready(&audio).ownership();
        let ended_app = app.clone();
        *app.sleep_hook.lock().unwrap() = Some(Box::new(move || {
            invalidate(&audio, &ended_app);
            assert_eq!(ended_app.effects.lock().unwrap().completions, 0);
        }));
        play_start_feedback_if_current(&app, &ownership);
        let effects = app.effects.lock().unwrap();
        assert!(effects.invalidated);
        assert_eq!(effects.cues, [Cue::Chime]);
        assert!(effects.stale.is_empty());
        assert_eq!(effects.completions, 1);
    });
}
#[test]
fn new_capture_owns_cues_and_invalidation_during_opening_stays_effective() {
    builder().check(|| {
        let (audio, app) = setup(false);
        let old = ready(&audio);
        audio.invalidate_recording_readiness();
        let new = ready(&audio);
        assert!(old.ownership().with_current(|| ()).is_none());
        production_ready(audio.clone(), new, app.clone());
        app.drain_main_thread();
        assert_eq!(app.effects.lock().unwrap().cues, [Cue::Chime, Cue::Ui]);
        let opening = ready(&audio);
        audio.invalidate_recording_readiness();
        assert!(opening.ownership().with_current(|| ()).is_none());
    });
}
#[test]
fn disabled_feedback_keeps_ui_readiness() {
    builder().check(|| {
        let (audio, mut app) = setup(false);
        app.feedback_enabled = false;
        production_ready(audio.clone(), ready(&audio), app.clone());
        app.drain_main_thread();
        assert_eq!(app.effects.lock().unwrap().cues, [Cue::Ui]);
    });
}

fn complete_start(
    ownership: RecordingReadinessOwnership,
    state: &mut RecordingState,
) -> RecordingReadinessOwnership {
    *state = RecordingState::Recording {
        binding_id: "fixture".into(),
    };
    ownership
}

fn admission_race(end: fn(&AudioRecordingManager) -> bool) {
    let saw_pre_mint = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_pre_mint_in_model = saw_pre_mint.clone();
    builder().check(move || {
        let (audio, _) = setup(false);
        let ended_audio = audio.clone();
        let ending = loom::thread::spawn(move || end(&ended_audio));
        let ownership = audio.production_start_admission(complete_start).unwrap();
        let ended_admitted_start = ending.join().unwrap();
        if ended_admitted_start && ownership.generation == 2 {
            saw_pre_mint_in_model.store(true, Ordering::Relaxed);
        }
        assert_eq!(
            ownership.with_current(|| ()).is_some(),
            !ended_admitted_start,
            "start token {} survived cancellation of an admitted start; current generation {}",
            ownership.generation,
            *audio.capture_generation.lock().unwrap()
        );
    });
    assert!(
        saw_pre_mint.load(Ordering::Relaxed),
        "Loom must cover invalidation after state admission but before token minting"
    );
}

#[test]
fn cancel_invalidates_start_admitted_before_token_mint() {
    admission_race(|audio| {
        audio.production_cancel_admission(|state| matches!(state, RecordingState::Recording { .. }))
    });
}

#[test]
fn matching_stop_invalidates_start_admitted_before_token_mint() {
    admission_race(|audio| audio.production_stop_admission("fixture", |_| ()).is_some());
}

#[test]
fn cancel_before_start_admission_allows_new_ownership() {
    builder().check(|| {
        let (audio, _) = setup(false);
        audio.production_cancel_admission(|state| assert!(matches!(state, RecordingState::Idle)));
        let ownership = audio.production_start_admission(complete_start).unwrap();
        assert!(ownership.with_current(|| ()).is_some());
    });
}

#[test]
fn wrong_binding_stop_does_not_repeat_invalidation_after_admission() {
    let saw_pre_mint = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_pre_mint_in_model = saw_pre_mint.clone();
    builder().check(move || {
        let (audio, _) = setup(false);
        let ended_audio = audio.clone();
        let ending = loom::thread::spawn(move || {
            ended_audio.production_stop_admission("another-binding", |_| ())
        });
        let ownership = audio.production_start_admission(complete_start).unwrap();
        assert!(ending.join().unwrap().is_none());
        if ownership.generation == 2 {
            saw_pre_mint_in_model.store(true, Ordering::Relaxed);
            assert!(ownership.with_current(|| ()).is_some());
        }
    });
    assert!(saw_pre_mint.load(Ordering::Relaxed));
}
