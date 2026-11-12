// The manager's surroundings, written once and compiled twice. `lib.rs`
// includes this file into `on_std` and `on_loom`, each with its own `Arc`,
// `Mutex`, `MutexGuard`, `AtomicU64`, `Ordering`, `mpsc`, `thread`, `PauseSlot`,
// `pause` and `report_exit` in scope.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

type Result<T, E = crate::ReproError> = std::result::Result<T, E>;

#[derive(Clone, Debug)]
struct ModelStateEvent {
    event_type: String,
    model_id: Option<String>,
    model_name: Option<String>,
    error: Option<String>,
}

struct App {
    events: Mutex<Vec<ModelStateEvent>>,
    /// Held by whoever is running on the main thread.
    main_thread: Mutex<()>,
    main: OnceLock<thread::ThreadId>,
}

#[derive(Clone)]
struct AppHandle(Arc<App>);

impl AppHandle {
    /// Production listeners run inside `emit`. The tray listener asks the main
    /// thread for the window theme and waits for the answer, so an emit from
    /// any other thread can't finish while the main thread is blocked.
    fn emit(&self, name: &str, event: ModelStateEvent) -> Result<()> {
        assert_eq!(name, "model-state-changed");
        if self.0.main.get() != Some(&thread::current().id()) {
            drop(self.0.main_thread.lock().unwrap());
        }
        self.0.events.lock().unwrap().push(event);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Backend {
    Auto,
}

#[derive(Debug)]
enum DeviceSelector {
    Auto,
    Index(usize),
    Key(String),
}

struct LoadSpec {
    path: PathBuf,
    backend: Backend,
    device: DeviceSelector,
}

#[derive(Clone, Default)]
struct Capabilities {
    supports_streaming: bool,
    supports_translate: bool,
    supports_language_detect: bool,
    languages: Vec<String>,
}

#[derive(Clone)]
struct LoadedInfo {
    capabilities: Capabilities,
    backend: String,
    device: String,
    /// Not in production: lets a check name the model the engine holds.
    model: String,
}

#[derive(Debug)]
enum EngineError {
    Failed(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self::Failed(message) = self;
        message.fmt(formatter)
    }
}

fn engine_stopped() -> EngineError {
    EngineError::Failed("the transcription engine has stopped".to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

type Reply<T> = mpsc::Sender<T>;

enum Command {
    Load(LoadSpec, Reply<Result<LoadedInfo, EngineError>>),
    Unload(Reply<()>),
    /// Not a production command: the worker is lost and can't be restarted,
    /// which production reaches inside a transcription.
    Lose(Reply<()>),
    /// Loom's channels don't report a hung-up sender, so the owner is told.
    Stop,
}

struct Shared {
    loaded: Mutex<Option<LoadedInfo>>,
    unloads_requested: AtomicU64,
    load_pause: PauseSlot,
}

struct Unloading(Option<mpsc::Receiver<()>>);

struct EngineSupervisor {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}

impl EngineSupervisor {
    fn lose(&self) {
        self.ask(Command::Lose);
    }
}

/// The owner thread's side of a load and an unload, in the order
/// `Owner::run`, `Owner::load` and `Owner::start` take them in production.
/// The worker process is left out. `publish_loaded` is the production method.
struct Owner {
    shared: Arc<Shared>,
    unloads_done: u64,
    /// Every model `publish_loaded` reported loaded.
    published: Vec<String>,
}

impl Owner {
    fn run(mut self, queue: mpsc::Receiver<Command>) -> Vec<String> {
        while let Ok(command) = queue.recv() {
            match command {
                Command::Load(spec, reply) => {
                    let _ = reply.send(self.load(spec));
                }
                Command::Unload(reply) => {
                    self.unload();
                    self.unloads_done += 1;
                    report_exit(reply);
                }
                Command::Lose(reply) => {
                    self.unload();
                    let _ = reply.send(());
                }
                Command::Stop => break,
            }
        }
        self.published
    }

    fn load(&mut self, spec: LoadSpec) -> Result<LoadedInfo, EngineError> {
        self.unload();
        let info = LoadedInfo {
            capabilities: Capabilities::default(),
            backend: "fake".into(),
            device: "fake".into(),
            model: spec.path.display().to_string(),
        };
        pause(&self.shared.load_pause);
        self.publish_loaded(&info);
        if lock(&self.shared.loaded).is_some() {
            self.published.push(info.model.clone());
        }
        Ok(info)
    }

    fn unload(&mut self) {
        *lock(&self.shared.loaded) = None;
    }
}

include!(concat!(env!("OUT_DIR"), "/engine_methods.rs"));

#[derive(Default)]
struct LanguageDetector;

impl LanguageDetector {
    fn unload(&self) {}
}

struct ModelInfo {
    name: String,
    is_downloaded: bool,
}

#[derive(Default)]
struct ModelManager {
    pause: PauseSlot,
}

impl ModelManager {
    fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        matches!(model_id, "model-a" | "model-b").then(|| ModelInfo {
            name: model_id.into(),
            is_downloaded: true,
        })
    }

    fn get_model_path(&self, model_id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(model_id))
    }

    fn set_runtime_capabilities(
        &self,
        _model_id: &str,
        _streaming: bool,
        _translate: bool,
        _detect: bool,
        _languages: Vec<String>,
    ) {
        pause(&self.pause);
    }
}

#[derive(Clone, Copy)]
enum Accelerator {
    Auto,
}

struct AppSettings {
    transcribe_accelerator: Accelerator,
    transcribe_gpu_device: Option<String>,
}

fn get_settings(_app: &AppHandle) -> AppSettings {
    AppSettings {
        transcribe_accelerator: Accelerator::Auto,
        transcribe_gpu_device: None,
    }
}

fn resolve_gpu_device(_accelerator: Accelerator, _device: Option<&str>) -> Option<String> {
    None
}

fn select_transcribe_backend(_accelerator: Accelerator) -> Backend {
    Backend::Auto
}

#[derive(Clone, Copy)]
enum NoticeSource {
    Dictation,
    Model,
}

#[derive(Clone, Copy)]
enum NoticeCode {
    ModelLoadFailed,
    NoModelSelected,
}

mod notices {
    use super::{AppHandle, NoticeCode, NoticeSource};

    pub(super) fn clear_matching(
        _app: &AppHandle,
        _source: NoticeSource,
        _code: Option<NoticeCode>,
    ) {
    }
}

include!(concat!(env!("OUT_DIR"), "/manager_types.rs"));

struct TranscriptionManager {
    engine: EngineSupervisor,
    model_manager: Arc<ModelManager>,
    app_handle: AppHandle,
    model_state: Arc<Mutex<ModelState>>,
    last_activity: AtomicU64,
    language_detector: LanguageDetector,
}

include!(concat!(env!("OUT_DIR"), "/manager_methods.rs"));

/// A manager with its engine owner thread running.
struct World {
    manager: Arc<TranscriptionManager>,
    owner: thread::JoinHandle<Vec<String>>,
}

impl World {
    /// The calling thread plays the main thread.
    fn new() -> Self {
        let (commands, queue) = mpsc::channel();
        let shared = Arc::new(Shared {
            loaded: Mutex::new(None),
            unloads_requested: AtomicU64::new(0),
            load_pause: PauseSlot::default(),
        });
        let owner = Owner {
            shared: Arc::clone(&shared),
            unloads_done: 0,
            published: Vec::new(),
        };
        let app = App {
            events: Mutex::new(Vec::new()),
            main_thread: Mutex::new(()),
            main: OnceLock::new(),
        };
        app.main.set(thread::current().id()).unwrap();
        let manager = Arc::new(TranscriptionManager {
            engine: EngineSupervisor { commands, shared },
            model_manager: Arc::new(ModelManager::default()),
            app_handle: AppHandle(Arc::new(app)),
            model_state: Arc::new(Mutex::new(ModelState::default())),
            last_activity: AtomicU64::new(0),
            language_detector: LanguageDetector,
        });
        Self {
            manager,
            owner: thread::spawn(move || owner.run(queue)),
        }
    }

    /// Run `on_main` as main-thread work: while it runs, no other thread's
    /// emit can finish.
    fn on_main<T>(&self, on_main: impl FnOnce(&TranscriptionManager) -> T) -> T {
        let _busy = self.manager.app_handle.0.main_thread.lock().unwrap();
        on_main(&self.manager)
    }

    fn events(&self) -> Vec<String> {
        let events = self.manager.app_handle.0.events.lock().unwrap();
        events.iter().map(|e| e.event_type.clone()).collect()
    }

    /// Let the engine finish its queue, then read every side of the state.
    fn settle(self) -> Settled {
        self.manager.engine.commands.send(Command::Stop).unwrap();
        let published = self.owner.join().unwrap();
        let events = self.manager.app_handle.0.events.lock().unwrap().clone();
        Settled {
            current_model: self.manager.get_current_model(),
            held: self.manager.engine.loaded().map(|info| info.model),
            events,
            published,
        }
    }
}

/// The state once every load and unload has returned and the engine is idle.
#[derive(Debug)]
struct Settled {
    /// The manager's `get_current_model`.
    current_model: Option<String>,
    /// The model the engine holds.
    held: Option<String>,
    events: Vec<ModelStateEvent>,
    /// Every model the engine reported loaded, in order.
    published: Vec<String>,
}

/// The model selector's status, as `ModelSelector.tsx` derives it from the
/// event stream.
#[derive(Clone, Debug, PartialEq)]
enum Selector {
    Initial,
    Loading,
    Ready(String),
    Error,
    Unloaded,
}

impl Settled {
    fn event_types(&self) -> Vec<&str> {
        self.events.iter().map(|e| e.event_type.as_str()).collect()
    }

    /// The selector's final status and whether the overlay still shows a load,
    /// replaying the handlers in `ModelSelector.tsx` and `RecordingOverlay.tsx`.
    fn frontend(&self) -> (Selector, bool) {
        let mut selector = Selector::Initial;
        let mut overlay_loading = false;
        for event in &self.events {
            match event.event_type.as_str() {
                "loading_started" => {
                    selector = Selector::Loading;
                    overlay_loading = true;
                }
                "loading_completed" => {
                    selector = Selector::Ready(event.model_id.clone().unwrap());
                    overlay_loading = false;
                }
                "loading_failed" => {
                    selector = Selector::Error;
                    overlay_loading = false;
                }
                "loading_cancelled" => {
                    selector = Selector::Unloaded;
                    overlay_loading = false;
                }
                "unloaded" => selector = Selector::Unloaded,
                other => panic!("unknown event {other}"),
            }
        }
        (selector, overlay_loading)
    }

    /// The properties every schedule must end with. `unloads` is how many
    /// `unloaded` events the schedule's unloads account for, when known.
    fn check(&self, unloads: Option<usize>) {
        let types = self.event_types();
        assert_eq!(
            self.current_model, self.held,
            "the manager's model id must name what the engine holds; events {types:?}"
        );
        let (selector, overlay_loading) = self.frontend();
        let expected = match &self.held {
            Some(model) => Selector::Ready(model.clone()),
            None => Selector::Unloaded,
        };
        assert_eq!(
            selector, expected,
            "the model selector must end on what the engine holds; events {types:?}"
        );
        assert!(
            !overlay_loading,
            "every load must end the overlay's loading state; events {types:?}"
        );
        let mut unreported = self.published.clone();
        for event in &self.events {
            if event.event_type == "loading_completed" {
                let model = event.model_id.as_deref().unwrap();
                let held_once = unreported.iter().position(|published| published == model);
                assert!(
                    held_once.is_some(),
                    "`loading_completed` for a load the engine never reported loaded; \
                     events {types:?}, engine reported {:?}",
                    self.published
                );
                unreported.remove(held_once.unwrap());
            }
        }
        if let Some(unloads) = unloads {
            let unloaded = types.iter().filter(|t| **t == "unloaded").count();
            assert_eq!(
                unloaded, unloads,
                "one `unloaded` per unload, none from a load; events {types:?}"
            );
        }
    }
}
