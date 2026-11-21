use super::*;
use crate::settings::SETTINGS_STORE_PATH;
use handy_recorder::testing::FakeMic;
use std::cell::RefCell;
use std::sync::{MutexGuard, OnceLock};
use tauri::test::MockRuntime;
use tauri_plugin_store::StoreExt;

type TestManager = AudioRecordingManager<MockRuntime>;

const WAIT: Duration = Duration::from_secs(5);
const BINDING: &str = "transcribe";

struct Hardware {
    mic: FakeMic,
    muted: AtomicBool,
}

thread_local! {
    static HARDWARE: RefCell<Option<Arc<Hardware>>> = const { RefCell::new(None) };
}

pub(super) fn set_mute(muted: bool) -> bool {
    HARDWARE.with_borrow(|hardware| {
        if let Some(hardware) = hardware {
            hardware.muted.store(muted, Ordering::SeqCst);
        }
        hardware.is_some()
    })
}

pub(super) fn get_mute() -> Option<bool> {
    HARDWARE.with_borrow(|hardware| hardware.as_ref().map(|h| h.muted.load(Ordering::SeqCst)))
}

pub(super) fn open_microphone(
    config: &RecorderConfig,
) -> Option<Result<OpenMicrophone, handy_recorder::Error>> {
    HARDWARE.with_borrow(|hardware| {
        hardware.as_ref().map(|hardware| {
            let failed = Arc::new(AtomicBool::new(false));
            let recorder = hardware.mic.open(
                config.clone(),
                Some(Box::new({
                    let failed = failed.clone();
                    move |_| failed.store(true, Ordering::Release)
                })),
                WAIT,
            )?;
            Ok(OpenMicrophone { recorder, failed })
        })
    })
}

fn app() -> &'static tauri::AppHandle<MockRuntime> {
    static APP: OnceLock<tauri::AppHandle<MockRuntime>> = OnceLock::new();
    APP.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().identifier = "computer.handy.audio-lifecycle-tests".into();
            let app = tauri::test::mock_builder()
                .plugin(tauri_plugin_store::Builder::default().build())
                .build(context)
                .expect("build headless audio lifecycle test app");
            tx.send(app.handle().clone()).unwrap();
            loop {
                std::thread::park();
            }
        });
        rx.recv_timeout(WAIT).expect("initialize test app")
    })
}

struct Fixture {
    hardware: Arc<Hardware>,
    _exclusive: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(lazy_stream_close: bool) -> Self {
        static EXCLUSIVE: Mutex<()> = Mutex::new(());
        let exclusive = EXCLUSIVE.lock().unwrap_or_else(|e| e.into_inner());
        let settings = AppSettings {
            always_on_microphone: false,
            mute_while_recording: true,
            extra_recording_buffer_ms: 0,
            lazy_stream_close,
            vad_backend: VadBackend::Earshot,
            ..AppSettings::default()
        };
        // Skip disk load and autosave; get_settings reuses this cached store.
        app()
            .store_builder(crate::portable::store_path(SETTINGS_STORE_PATH))
            .create_new()
            .disable_auto_save()
            .default("settings", serde_json::to_value(settings).unwrap())
            .build()
            .unwrap();
        let hardware = Arc::new(Hardware {
            mic: FakeMic::new(16000, 1),
            muted: AtomicBool::new(false),
        });
        HARDWARE.with_borrow_mut(|slot| *slot = Some(hardware.clone()));
        Self {
            hardware,
            _exclusive: exclusive,
        }
    }

    fn manager(&self) -> TestManager {
        let mut manager =
            AudioRecordingManager::new(app(), Arc::new(StreamRouter::for_audio_tests())).unwrap();
        manager.on_levels = Arc::new(|_| {});
        manager.on_audio = Arc::new(|_| {});
        manager
    }

    fn muted(&self) -> bool {
        self.hardware.muted.load(Ordering::SeqCst)
    }

    fn capture(&self, manager: &mut TestManager) -> Vec<f32> {
        let (tx, rx) = mpsc::channel();
        manager.on_audio = Arc::new(move |frame| {
            let _ = tx.send(frame.to_vec());
        });
        let ready = manager
            .try_start_recording(BINDING, VadPolicy::Disabled)
            .unwrap();
        let frame = vec![0.25; 256];
        let deadline = Instant::now() + WAIT;
        let first = loop {
            assert!(self.hardware.mic.push(&frame));
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(frame) => break frame,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    assert!(Instant::now() < deadline, "fake audio never reached sink");
                }
                Err(error) => panic!("capture callback disconnected: {error}"),
            }
        };
        ready
            .receiver
            .recv_timeout(WAIT)
            .expect("first-sample readiness");
        let mut samples = first;
        for _ in 0..64 {
            assert!(self.hardware.mic.push(&frame));
            samples.extend(rx.recv_timeout(WAIT).expect("sink accepted audio"));
        }
        assert!(samples.len() > WHISPER_SAMPLE_RATE);
        samples
    }

    fn with_callbacks<T>(&self, action: impl FnOnce() -> T) -> T {
        // Recorder stop needs a callback to acknowledge its pause request.
        std::thread::scope(|scope| {
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let mic = self.hardware.mic.clone();
            scope.spawn(move || {
                while matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    mic.push(&[0.0; 256]);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            let result = action();
            drop(stop_tx);
            result
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        HARDWARE.with_borrow_mut(|slot| *slot = None);
    }
}

#[test]
fn ordinary_recording_preserves_audio_and_mute_until_stop() {
    let fixture = Fixture::new(false);
    let mut manager = fixture.manager();
    let expected = fixture.capture(&mut manager);
    manager.apply_mute();
    assert!(fixture.muted());
    let samples = fixture
        .with_callbacks(|| manager.stop_recording(BINDING, 0))
        .expect("recording completes");
    assert!(
        samples.starts_with(&expected),
        "ordinary take lost captured audio"
    );
    assert!(!fixture.muted(), "stop must restore prior output mute");
}

#[test]
fn device_change_mid_take_preserves_captured_audio() {
    let fixture = Fixture::new(false);
    let mut manager = fixture.manager();
    let expected = fixture.capture(&mut manager);
    let _change = fixture.with_callbacks(|| manager.update_selected_device());
    let samples = fixture
        .with_callbacks(|| manager.stop_recording(BINDING, 0))
        .expect("device change must not cancel the take");
    assert!(
        samples.starts_with(&expected),
        "device change discarded captured audio: expected prefix of {} samples, got {} samples",
        expected.len(),
        samples.len()
    );
}

#[test]
fn device_change_mid_take_keeps_output_muted() {
    let fixture = Fixture::new(false);
    let mut manager = fixture.manager();
    fixture.capture(&mut manager);
    manager.apply_mute();
    assert!(fixture.muted(), "recording must apply output mute first");
    let _change = fixture.with_callbacks(|| manager.update_selected_device());
    let muted_during_take = fixture.muted();
    fixture.with_callbacks(|| manager.stop_recording(BINDING, 0));
    assert!(
        muted_during_take,
        "device change restored output mute during an active take"
    );
}
