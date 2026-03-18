use crate::managers::audio::AudioRecordingManager;
use crate::settings::get_settings;
use log::{debug, error, info, warn};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
#[path = "media_control_windows.rs"]
mod platform_windows;

#[cfg(target_os = "linux")]
#[path = "media_control_linux.rs"]
mod platform_linux;

#[derive(Clone)]
pub struct MediaControlManager {
    state: Arc<Mutex<SessionState>>,
    backend: Arc<dyn MediaControlBackend>,
}

#[derive(Default)]
struct SessionState {
    generation: u64,
    pause_in_flight: bool,
    paused_playback: Option<PausedPlaybackState>,
    /// Set when `resume_after_recording` is called while a pause is still in-flight.
    /// Stores whether playback should be resumed once the in-flight pause result arrives.
    stale_resume_play: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PausedPlaybackState {
    #[cfg(target_os = "macos")]
    Global,
    #[cfg(any(target_os = "windows", target_os = "linux", test))]
    Session(String),
}

trait MediaControlBackend: Send + Sync {
    fn pause_playback(&self) -> Result<Option<PausedPlaybackState>, String>;
    fn resume_playback(&self, paused_playback: PausedPlaybackState) -> Result<(), String>;
}

struct PlatformMediaControlBackend;

impl MediaControlManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState::default())),
            backend: Arc::new(PlatformMediaControlBackend),
        }
    }

    #[cfg(test)]
    fn with_backend(backend: Arc<dyn MediaControlBackend>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState::default())),
            backend,
        }
    }

    pub fn pause_for_recording(&self, app: &AppHandle) {
        let settings = get_settings(app);
        let recording_active = app
            .try_state::<Arc<AudioRecordingManager>>()
            .map(|audio_manager| audio_manager.is_recording())
            .unwrap_or(true);

        self.pause_for_recording_inner(settings.pause_while_recording, recording_active);
    }

    pub fn resume_after_recording(&self, app: &AppHandle) {
        self.resume_after_recording_inner(get_settings(app).play_after_recording);
    }

    fn pause_for_recording_inner(&self, pause_enabled: bool, recording_active: bool) {
        if !pause_enabled {
            debug!("Skipping media pause because Pause While Recording is disabled");
            return;
        }

        if !recording_active {
            debug!("Skipping media pause because recording is no longer active");
            return;
        }

        let generation = {
            let mut state = self.state.lock().unwrap();
            if state.pause_in_flight || state.paused_playback.is_some() {
                debug!("Pause While Recording is already active for the current recording session");
                return;
            }

            state.generation = state.generation.wrapping_add(1);
            state.pause_in_flight = true;
            state.generation
        };

        let pause_result = self.backend.pause_playback();
        let mut state = self.state.lock().unwrap();

        if state.generation != generation {
            debug!("Discarding stale media pause result after recording session changed");
            let should_resume = state.stale_resume_play;
            state.stale_resume_play = false;
            drop(state);
            // The recording session already ended while we were pausing.  If we actually
            // paused something and the caller wanted playback resumed, do it now so media
            // doesn't stay stuck in the paused state.
            if should_resume {
                if let Ok(Some(paused_playback)) = pause_result {
                    match self.backend.resume_playback(paused_playback) {
                        Ok(()) => info!("Resumed media playback after stale recording pause"),
                        Err(err) => warn!("Failed to resume stale media pause: {err}"),
                    }
                }
            }
            return;
        }

        state.pause_in_flight = false;

        match pause_result {
            Ok(Some(paused_playback)) => {
                state.paused_playback = Some(paused_playback);
                info!("Paused media playback for recording");
            }
            Ok(None) => {
                debug!("Skipping media pause because there was no active playback to pause");
            }
            Err(err) => warn!("Failed to pause media playback for recording: {err}"),
        }
    }

    fn resume_after_recording_inner(&self, play_after_recording: bool) {
        let paused_playback = {
            let mut state = self.state.lock().unwrap();
            state.generation = state.generation.wrapping_add(1);
            // If the pause backend call is still in-flight, record the user's preference so
            // the pause thread can resume immediately when its result eventually arrives.
            state.stale_resume_play = state.pause_in_flight && play_after_recording;
            state.pause_in_flight = false;
            state.paused_playback.take()
        };

        let Some(paused_playback) = paused_playback else {
            debug!("Skipping media resume because Handy did not pause anything");
            return;
        };

        if !play_after_recording {
            info!("Skipping media resume because Play After Recording is disabled");
            return;
        }

        match self.backend.resume_playback(paused_playback) {
            Ok(()) => info!("Resumed media playback after recording"),
            Err(err) => error!("Failed to resume media playback after recording: {err}"),
        }
    }
}

impl Default for MediaControlManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaControlBackend for PlatformMediaControlBackend {
    fn pause_playback(&self) -> Result<Option<PausedPlaybackState>, String> {
        platform_pause_playback()
    }

    fn resume_playback(&self, paused_playback: PausedPlaybackState) -> Result<(), String> {
        platform_resume_playback(paused_playback)
    }
}

#[cfg(target_os = "macos")]
fn platform_pause_playback() -> Result<Option<PausedPlaybackState>, String> {
    match crate::media_remote::any_application_is_playing()? {
        true => {
            crate::media_remote::pause()?;
            Ok(Some(PausedPlaybackState::Global))
        }
        false => Ok(None),
    }
}

#[cfg(target_os = "macos")]
fn platform_resume_playback(paused_playback: PausedPlaybackState) -> Result<(), String> {
    match paused_playback {
        PausedPlaybackState::Global => {}
        #[cfg(test)]
        PausedPlaybackState::Session(_) => return Ok(()),
    }

    crate::media_remote::play()
}

#[cfg(target_os = "windows")]
fn platform_pause_playback() -> Result<Option<PausedPlaybackState>, String> {
    platform_windows::pause_active_session()
        .map(|session| session.map(PausedPlaybackState::Session))
}

#[cfg(target_os = "windows")]
fn platform_resume_playback(paused_playback: PausedPlaybackState) -> Result<(), String> {
    let PausedPlaybackState::Session(source_app_user_model_id) = paused_playback;
    platform_windows::resume_session(&source_app_user_model_id)
}

#[cfg(target_os = "linux")]
fn platform_pause_playback() -> Result<Option<PausedPlaybackState>, String> {
    platform_linux::pause_active_session().map(|session| session.map(PausedPlaybackState::Session))
}

#[cfg(target_os = "linux")]
fn platform_resume_playback(paused_playback: PausedPlaybackState) -> Result<(), String> {
    let PausedPlaybackState::Session(service_name) = paused_playback;
    platform_linux::resume_session(&service_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct TestBackend {
        pause_calls: AtomicUsize,
        resume_calls: AtomicUsize,
        pause_result: Mutex<Result<Option<PausedPlaybackState>, String>>,
        resume_result: Mutex<Result<(), String>>,
        pause_delay: Mutex<Option<Duration>>,
    }

    impl Default for TestBackend {
        fn default() -> Self {
            Self {
                pause_calls: AtomicUsize::new(0),
                resume_calls: AtomicUsize::new(0),
                pause_result: Mutex::new(Ok(None)),
                resume_result: Mutex::new(Ok(())),
                pause_delay: Mutex::new(None),
            }
        }
    }

    impl TestBackend {
        fn with_pause_result(result: Result<Option<PausedPlaybackState>, String>) -> Arc<Self> {
            Arc::new(Self {
                pause_result: Mutex::new(result),
                ..Default::default()
            })
        }

        fn pause_calls(&self) -> usize {
            self.pause_calls.load(Ordering::SeqCst)
        }

        fn resume_calls(&self) -> usize {
            self.resume_calls.load(Ordering::SeqCst)
        }

        fn install_pause_delay(&self, delay: Duration) {
            *self.pause_delay.lock().unwrap() = Some(delay);
        }
    }

    impl MediaControlBackend for TestBackend {
        fn pause_playback(&self) -> Result<Option<PausedPlaybackState>, String> {
            self.pause_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(delay) = *self.pause_delay.lock().unwrap() {
                std::thread::sleep(delay);
            }
            self.pause_result.lock().unwrap().clone()
        }

        fn resume_playback(&self, _paused_playback: PausedPlaybackState) -> Result<(), String> {
            self.resume_calls.fetch_add(1, Ordering::SeqCst);
            self.resume_result.lock().unwrap().clone()
        }
    }

    fn paused_session() -> PausedPlaybackState {
        PausedPlaybackState::Session("test-session".to_string())
    }

    #[test]
    fn pause_disabled_skips_backend() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(false, true);

        assert_eq!(backend.pause_calls(), 0);
    }

    #[test]
    fn play_after_recording_disabled_clears_state_without_resuming() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, true);
        manager.resume_after_recording_inner(false);
        manager.resume_after_recording_inner(true);

        assert_eq!(backend.pause_calls(), 1);
        assert_eq!(backend.resume_calls(), 0);
    }

    #[test]
    fn pause_called_while_not_recording_is_a_noop() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, false);

        assert_eq!(backend.pause_calls(), 0);
    }

    #[test]
    fn repeated_pause_during_one_recording_session_only_pauses_once() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, true);
        manager.pause_for_recording_inner(true, true);

        assert_eq!(backend.pause_calls(), 1);
    }

    #[test]
    fn stop_or_cancel_resumes_only_once() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, true);
        manager.resume_after_recording_inner(true);
        manager.resume_after_recording_inner(true);

        assert_eq!(backend.resume_calls(), 1);
    }

    #[test]
    fn resume_is_skipped_when_nothing_was_paused() {
        let backend = TestBackend::with_pause_result(Ok(None));
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, true);
        manager.resume_after_recording_inner(true);

        assert_eq!(backend.pause_calls(), 1);
        assert_eq!(backend.resume_calls(), 0);
    }

    #[test]
    fn paused_state_is_cleared_after_resume_attempt() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        *backend.resume_result.lock().unwrap() = Err("resume failed".to_string());
        let manager = MediaControlManager::with_backend(backend.clone());

        manager.pause_for_recording_inner(true, true);
        manager.resume_after_recording_inner(true);
        manager.resume_after_recording_inner(true);

        assert_eq!(backend.resume_calls(), 1);
    }

    #[test]
    fn stale_pause_is_resumed_when_resume_happens_first() {
        // The pause backend call is slow enough that resume_after_recording is called
        // before pause_for_recording stores its result. The pause thread should still
        // resume playback once the backend call returns, so media is never left stuck.
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        backend.install_pause_delay(Duration::from_millis(100));
        let manager = Arc::new(MediaControlManager::with_backend(backend.clone()));
        let manager_for_thread = manager.clone();

        let handle = std::thread::spawn(move || {
            manager_for_thread.pause_for_recording_inner(true, true);
        });

        while backend.pause_calls() == 0 {
            std::thread::yield_now();
        }

        manager.resume_after_recording_inner(true);
        handle.join().unwrap();
        manager.resume_after_recording_inner(true);

        assert_eq!(backend.pause_calls(), 1);
        assert_eq!(backend.resume_calls(), 1);
    }

    #[test]
    fn stale_pause_is_not_resumed_when_play_after_recording_is_disabled() {
        let backend = TestBackend::with_pause_result(Ok(Some(paused_session())));
        backend.install_pause_delay(Duration::from_millis(100));
        let manager = Arc::new(MediaControlManager::with_backend(backend.clone()));
        let manager_for_thread = manager.clone();

        let handle = std::thread::spawn(move || {
            manager_for_thread.pause_for_recording_inner(true, true);
        });

        while backend.pause_calls() == 0 {
            std::thread::yield_now();
        }

        manager.resume_after_recording_inner(false);
        handle.join().unwrap();

        assert_eq!(backend.pause_calls(), 1);
        assert_eq!(backend.resume_calls(), 0);
    }
}
