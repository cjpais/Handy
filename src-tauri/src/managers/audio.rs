use crate::audio_toolkit::{
    audio::{AudioFrameCallback, LevelCallback},
    vad::{
        frames_for_duration_ms, EarshotVad, SmoothedVad, VAD_OFFLINE_HANGOVER_MS, VAD_ONSET_MS,
        VAD_PREFILL_MS, VAD_STREAMING_HANGOVER_MS,
    },
    CaptureSink, CaptureVad, SileroVad, VadPolicy, VoiceActivityDetector,
};
use crate::helpers::clamshell;
use crate::managers::transcription::StreamRouter;
use crate::settings::{get_settings, write_settings, AppSettings, VadBackend};
use crate::utils;
use handy_recorder::{Channels, EndReason, ErrorKind, Recorder, RecorderConfig, StartError};
use log::{debug, error, info, trace, warn};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const SILERO_VAD_THRESHOLD: f32 = 0.3;
const EARSHOT_VAD_THRESHOLD: f32 = 0.5;

fn set_mute(mute: bool) {
    // Expected behavior:
    // - Windows: works on most systems using standard audio drivers.
    // - Linux: works on many systems (PipeWire, PulseAudio, ALSA),
    //   but some distros may lack the tools used.
    // - macOS: works on most standard setups via AppleScript.
    // If unsupported, fails silently.

    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows::Win32::{
                Media::Audio::{
                    eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                    MMDeviceEnumerator,
                },
                System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
            };

            macro_rules! unwrap_or_return {
                ($expr:expr) => {
                    match $expr {
                        Ok(val) => val,
                        Err(_) => return,
                    }
                };
            }

            // Initialize the COM library for this thread.
            // If already initialized (e.g., by another library like Tauri), this does nothing.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let all_devices: IMMDeviceEnumerator =
                unwrap_or_return!(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL));
            let default_device =
                unwrap_or_return!(all_devices.GetDefaultAudioEndpoint(eRender, eMultimedia));
            let volume_interface = unwrap_or_return!(
                default_device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            );

            let _ = volume_interface.SetMute(mute, std::ptr::null());
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let mute_val = if mute { "1" } else { "0" };
        let amixer_state = if mute { "mute" } else { "unmute" };

        // Try multiple backends to increase compatibility
        // 1. PipeWire (wpctl)
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        // 2. PulseAudio (pactl)
        if Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        // 3. ALSA (amixer)
        let _ = Command::new("amixer")
            .args(["set", "Master", amixer_state])
            .output();
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let script = format!(
            "set volume output muted {}",
            if mute { "true" } else { "false" }
        );
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
}

/// Reads the current system output mute state, mirroring `set_mute`'s backends.
///
/// Returns `Some(true)`/`Some(false)` when the state could be determined, or
/// `None` when it couldn't (unsupported platform, missing CLI tools, or an
/// error). Callers treat `None` as "unknown" and fall back to unmuting on stop,
/// so we never strand the user's audio muted.
#[cfg(target_os = "windows")]
fn get_mute() -> Option<bool> {
    unsafe {
        use windows::Win32::{
            Media::Audio::{
                eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                MMDeviceEnumerator,
            },
            System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
        };

        // Matches set_mute: no-op if COM is already initialized on this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let all_devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let default_device = all_devices
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .ok()?;
        let volume_interface = default_device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .ok()?;

        Some(volume_interface.GetMute().ok()?.as_bool())
    }
}

#[cfg(target_os = "linux")]
fn get_mute() -> Option<bool> {
    use std::process::Command;

    // 1. PipeWire (wpctl): prints "[MUTED]" in the volume line when muted.
    if let Ok(out) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
    {
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).contains("[MUTED]"));
        }
    }

    // 2. PulseAudio (pactl): prints "Mute: yes" / "Mute: no".
    // Force LC_ALL=C so a localized system still emits the parseable English
    // "yes"/"no" instead of e.g. "ja"/"nein".
    if let Ok(out) = Command::new("pactl")
        .env("LC_ALL", "C")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if s.contains("yes") {
                return Some(true);
            }
            if s.contains("no") {
                return Some(false);
            }
        }
    }

    // 3. ALSA (amixer): prints "[off]" for muted channels, "[on]" otherwise.
    // LC_ALL=C keeps the "[on]"/"[off]" tokens stable across locales.
    if let Ok(out) = Command::new("amixer")
        .env("LC_ALL", "C")
        .args(["get", "Master"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.contains("[off]") {
                return Some(true);
            }
            if s.contains("[on]") {
                return Some(false);
            }
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn get_mute() -> Option<bool> {
    use std::process::Command;

    let out = Command::new("osascript")
        .args(["-e", "output muted of (get volume settings)"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn get_mute() -> Option<bool> {
    None
}

/// Restores the system mute state after our forced mute, given the state
/// captured just before we muted. We only ever need to unmute — and only when
/// the system was NOT already muted beforehand. If the prior state was muted,
/// we leave it muted (the user's own state). If it's unknown (`None`), we
/// default to unmuting so audio is never left stranded muted by us.
fn restore_mute(prev_muted: Option<bool>) {
    if prev_muted != Some(true) {
        set_mute(false);
    }
}

const WHISPER_SAMPLE_RATE: usize = 16000;

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone, Debug)]
pub enum RecordingState {
    Idle,
    Recording { binding_id: String },
    Stopping,
}

#[derive(Clone, Debug)]
pub enum MicrophoneMode {
    AlwaysOn,
    OnDemand,
}

/// Tracks our forced "mute while recording" so we can restore the user's audio
/// exactly as it was. `did_mute` is true while our mute is active; `prev_muted`
/// is the system mute state captured just before we muted, used to decide
/// whether to unmute on stop (so a system that was already muted stays muted).
#[derive(Debug, Default, Clone, Copy)]
struct MuteState {
    did_mute: bool,
    prev_muted: Option<bool>,
}

/// The persisted microphone preference currently in effect. Clamshell and
/// regular selections are kept distinct so losing a clamshell-only device does
/// not erase the user's normal microphone preference.
enum DesiredMicrophone {
    Default,
    Selected(String),
    Clamshell(String),
}

/// Result of resolving the persisted preference to a recorder device ID.
/// `device: None` opens the system default. The unavailable name is populated
/// only when enumeration succeeded and confirmed that the user's regular
/// selected microphone is missing.
struct MicrophoneResolution {
    device: Option<String>,
    /// The ID came from `cached_device`, so it may be stale.
    from_cache: bool,
    unavailable_selected_microphone: Option<String>,
}

/// The VAD detector. A recording borrows it for its sink and `stop` hands it
/// back; a recording whose sink is lost loses it, and the next one reloads it.
enum VadSlot {
    Unloaded,
    Idle(CaptureVad),
    /// Lent to the active recording. Keeps the frame size, so a microphone
    /// opened meanwhile still gets VAD-sized chunks.
    Lent {
        frame_samples: usize,
    },
}

/// An open microphone: the stream is running whether or not it is recording.
struct OpenMicrophone {
    recorder: Recorder<CaptureSink>,
    /// Set when this recorder fails. Its own flag, so a failure handler that
    /// runs late can't mark a replacement recorder failed.
    failed: Arc<AtomicBool>,
}

impl OpenMicrophone {
    fn needs_reopen(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

/// The `recording-error` event's `error_type` for a recording that couldn't
/// start.
pub fn recording_error_type(error: &anyhow::Error) -> &'static str {
    match error
        .downcast_ref::<handy_recorder::Error>()
        .map(handy_recorder::Error::kind)
    {
        Some(ErrorKind::PermissionDenied) => "microphone_permission_denied",
        // A missing selected microphone falls back to the default, so this
        // means there is no input device at all.
        Some(ErrorKind::DeviceUnavailable) => "no_input_device",
        _ => "unknown",
    }
}

/* ──────────────────────────────────────────────────────────────── */

fn create_capture_vad(
    backend: VadBackend,
    app_handle: &tauri::AppHandle,
) -> Result<CaptureVad, anyhow::Error> {
    let detector: Box<dyn VoiceActivityDetector> = match backend {
        VadBackend::Silero => {
            let vad_path = app_handle
                .path()
                .resolve(
                    "resources/models/silero_vad_v4.onnx",
                    tauri::path::BaseDirectory::Resource,
                )
                .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {e}"))?;
            Box::new(
                SileroVad::new(vad_path, SILERO_VAD_THRESHOLD)
                    .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {e}"))?,
            )
        }
        VadBackend::Earshot => Box::new(
            EarshotVad::new(EARSHOT_VAD_THRESHOLD)
                .map_err(|e| anyhow::anyhow!("Failed to create EarshotVad: {e}"))?,
        ),
    };

    // Earshot uses 16 ms frames while Silero uses 30 ms. Convert the existing
    // time-based capture profile to each detector's frame size so selecting a
    // backend does not shorten pre-roll, onset, or post-speech audio.
    let frame_samples = detector.frame_samples();
    let prefill_frames = frames_for_duration_ms(VAD_PREFILL_MS, frame_samples);
    let offline_hangover_frames = frames_for_duration_ms(VAD_OFFLINE_HANGOVER_MS, frame_samples);
    let streaming_hangover_frames =
        frames_for_duration_ms(VAD_STREAMING_HANGOVER_MS, frame_samples);
    let onset_frames = frames_for_duration_ms(VAD_ONSET_MS, frame_samples);
    let smoothed_vad = SmoothedVad::new(
        detector,
        prefill_frames,
        offline_hangover_frames,
        onset_frames,
    );

    info!(
        "Initialized {:?} VAD backend ({} samples/frame)",
        backend, frame_samples
    );

    Ok(CaptureVad::new(
        Box::new(smoothed_vad),
        offline_hangover_frames,
        streaming_hangover_frames,
    ))
}

/// 16 kHz mono in VAD-frame chunks, from `device` (`None`: the system
/// default). `channel` picks one zero-based input channel; `None` averages
/// them all.
fn microphone_config(
    device: Option<String>,
    channel: Option<u16>,
    frames_per_chunk: usize,
) -> RecorderConfig {
    RecorderConfig {
        device,
        channels: channel.map_or(Channels::MixToMono, Channels::Only),
        frames_per_chunk: Some(frames_per_chunk),
        ..RecorderConfig::speech()
    }
}

fn open_microphone(config: RecorderConfig) -> Result<OpenMicrophone, handy_recorder::Error> {
    let failed = Arc::new(AtomicBool::new(false));
    let recorder = Recorder::open_with_failure_handler(config, {
        let failed = Arc::clone(&failed);
        // handy-recorder logs the failure. The stream is rebuilt on the next
        // start; a recording in progress keeps the audio captured before it.
        move |_| failed.store(true, Ordering::Release)
    })?;
    Ok(OpenMicrophone { recorder, failed })
}

/// Opens with the selected channel, or averages all channels if the device
/// doesn't have it (a different microphone than the one it was chosen on).
fn open_microphone_on_channel(
    device: Option<String>,
    channel: Option<u16>,
    frames_per_chunk: usize,
) -> Result<OpenMicrophone, handy_recorder::Error> {
    match open_microphone(microphone_config(device.clone(), channel, frames_per_chunk)) {
        Err(error) if error.kind() == ErrorKind::InvalidChannel => {
            warn!("{error}; averaging all input channels instead");
            open_microphone(microphone_config(device, None, frames_per_chunk))
        }
        result => result,
    }
}

/* ──────────────────────────────────────────────────────────────── */

/// One recording session's first-sample notification. Waiting on this never
/// blocks the shortcut coordinator: callers hand it to a dedicated worker.
pub struct RecordingReadiness {
    receiver: mpsc::Receiver<()>,
    generation: u64,
}

impl RecordingReadiness {
    pub fn wait(self) -> bool {
        self.receiver.recv().is_ok()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone)]
pub struct AudioRecordingManager {
    /// Never assign through this directly — route every write through
    /// `set_state()`, which keeps `recording_active` in sync.
    state: Arc<Mutex<RecordingState>>,
    mode: Arc<Mutex<MicrophoneMode>>,
    app_handle: tauri::AppHandle,

    recorder: Arc<Mutex<Option<OpenMicrophone>>>,
    vad: Arc<Mutex<VadSlot>>,
    /// Zero-based input channel to record, or `None` to average them all.
    /// Held here rather than read from settings, which are persisted only
    /// after a channel change reopens successfully.
    selected_channel: Arc<Mutex<Option<u16>>>,
    /// Spectrum levels for the overlay.
    on_levels: LevelCallback,
    /// VAD-passed frames for live streaming, via a shared `StreamRouter`
    /// (captured directly, not via Tauri state — see its docs).
    on_audio: AudioFrameCallback,
    is_open: Arc<Mutex<bool>>,
    is_recording: Arc<Mutex<bool>>,
    mute_state: Arc<Mutex<MuteState>>,
    close_generation: Arc<AtomicU64>,
    cancel_generation: Arc<AtomicU64>,
    /// Lock-free mirror of "is the state in {Recording, Stopping}",
    /// maintained by `set_state()`. The hot-path `is_recording()` reads THIS
    /// instead of the std `state` mutex, so a UI poll can no longer deadlock
    /// the main/webview thread when a worker holds `state` across a slow
    /// CoreAudio open/close.
    recording_active: Arc<AtomicBool>,
    /// Invalidates asynchronous first-sample UI/chime work when a recording is
    /// stopped or cancelled. This prevents a slow device from producing a late
    /// "ready" indication for a session the user already ended.
    capture_generation: Arc<AtomicU64>,
    /// Resolution of a *named* microphone (selected or clamshell) to its
    /// recorder device ID, cached so on-demand recording starts skip an extra
    /// device enumeration. Keyed by the resolved name, so a settings change
    /// misses naturally; cleared when an open fails (device unplugged) so the
    /// retry re-enumerates. The system-default case is never cached — the
    /// recorder resolves the current default itself.
    cached_device: Arc<Mutex<Option<(String, String)>>>,
}

impl AudioRecordingManager {
    /* ---------- construction ------------------------------------------------ */

    pub fn new(
        app: &tauri::AppHandle,
        stream_router: Arc<StreamRouter>,
    ) -> Result<Self, anyhow::Error> {
        let settings = get_settings(app);
        let mode = if settings.always_on_microphone {
            MicrophoneMode::AlwaysOn
        } else {
            MicrophoneMode::OnDemand
        };

        let on_levels: LevelCallback = {
            let app_handle = app.clone();
            Arc::new(move |levels| utils::emit_levels(&app_handle, &levels))
        };
        let on_audio: AudioFrameCallback = Arc::new(move |frame| stream_router.feed(frame));

        let manager = Self {
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            mode: Arc::new(Mutex::new(mode.clone())),
            app_handle: app.clone(),

            recorder: Arc::new(Mutex::new(None)),
            vad: Arc::new(Mutex::new(VadSlot::Unloaded)),
            selected_channel: Arc::new(Mutex::new(settings.selected_channel)),
            on_levels,
            on_audio,
            is_open: Arc::new(Mutex::new(false)),
            is_recording: Arc::new(Mutex::new(false)),
            mute_state: Arc::new(Mutex::new(MuteState::default())),
            close_generation: Arc::new(AtomicU64::new(0)),
            cancel_generation: Arc::new(AtomicU64::new(0)),
            recording_active: Arc::new(AtomicBool::new(false)),
            capture_generation: Arc::new(AtomicU64::new(0)),
            cached_device: Arc::new(Mutex::new(None)),
        };

        // Always-on?  Open immediately.
        if matches!(mode, MicrophoneMode::AlwaysOn) {
            manager.start_microphone_stream()?;
        }

        Ok(manager)
    }

    /* ---------- helper methods --------------------------------------------- */

    /// The persisted microphone preference currently in effect. Only runs the
    /// clamshell probe (an `ioreg` subprocess, ~10-20ms) when a clamshell
    /// microphone is actually configured.
    fn desired_microphone(&self, settings: &AppSettings) -> DesiredMicrophone {
        if let Some(clamshell_microphone) = &settings.clamshell_microphone {
            let clamshell_started = Instant::now();
            let is_clamshell = clamshell::is_clamshell().unwrap_or(false);
            debug!(
                "device resolve: clamshell_check={:?} (clamshell={})",
                clamshell_started.elapsed(),
                is_clamshell
            );
            if is_clamshell {
                return DesiredMicrophone::Clamshell(clamshell_microphone.clone());
            }
        }
        match &settings.selected_microphone {
            Some(name) => DesiredMicrophone::Selected(name.clone()),
            None => DesiredMicrophone::Default,
        }
    }

    pub fn invalidate_device_cache(&self) {
        *self.cached_device.lock().unwrap() = None;
    }

    fn resolve_microphone_device(&self, settings: &AppSettings) -> MicrophoneResolution {
        let desired = self.desired_microphone(settings);
        let (device_name, selected_microphone) = match desired {
            DesiredMicrophone::Default => {
                debug!("device resolve: no mic configured -> system default");
                return MicrophoneResolution {
                    device: None,
                    from_cache: false,
                    unavailable_selected_microphone: None,
                };
            }
            DesiredMicrophone::Selected(name) => (name.clone(), Some(name)),
            DesiredMicrophone::Clamshell(name) => (name, None),
        };

        // Cache hit: skip the enumeration. A stale ID (device unplugged)
        // fails at open, where the caller invalidates and retries fresh.
        if let Some((cached_name, id)) = self.cached_device.lock().unwrap().as_ref() {
            if *cached_name == device_name {
                debug!("device resolve: cache hit for '{}'", device_name);
                return MicrophoneResolution {
                    device: Some(id.clone()),
                    from_cache: true,
                    unavailable_selected_microphone: None,
                };
            }
        }

        // Only report a selected microphone as unavailable when enumeration
        // itself succeeded. A backend enumeration error may be transient and
        // must not erase the user's persisted preference.
        let enumerate_started = Instant::now();
        let (device, enumeration_succeeded) = match handy_recorder::list_input_devices() {
            Ok(devices) => (
                devices
                    .into_iter()
                    .find(|d| d.name == device_name)
                    .map(|d| d.id),
                true,
            ),
            Err(e) => {
                debug!("Failed to list devices, using default: {}", e);
                (None, false)
            }
        };
        debug!(
            "device resolve: enumerate={:?} (found={})",
            enumerate_started.elapsed(),
            device.is_some()
        );
        if let Some(id) = &device {
            *self.cached_device.lock().unwrap() = Some((device_name, id.clone()));
        }

        let unavailable_selected_microphone = if enumeration_succeeded && device.is_none() {
            selected_microphone
        } else {
            None
        };
        MicrophoneResolution {
            device,
            from_cache: false,
            unavailable_selected_microphone,
        }
    }

    /// Keep persisted settings and the UI aligned with a successful runtime
    /// fallback. Re-read first so recovery cannot clear a microphone the user
    /// selected concurrently while the stream was being rebuilt.
    fn persist_default_microphone_after_fallback(&self, unavailable_name: &str) {
        let mut settings = get_settings(&self.app_handle);
        if settings.selected_microphone.as_deref() != Some(unavailable_name) {
            return;
        }

        settings.selected_microphone = None;
        write_settings(&self.app_handle, settings);
        let _ = self.app_handle.emit(
            "settings-changed",
            serde_json::json!({
                "setting": "selected_microphone",
                "value": "Default"
            }),
        );
    }

    fn schedule_lazy_close(&self) {
        let gen = self.close_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let app = self.app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(STREAM_IDLE_TIMEOUT);
            let rm = app.state::<Arc<AudioRecordingManager>>();
            // Hold state lock across the check AND close to serialize against
            // try_start_recording, preventing a race where the stream is closed
            // under an active recording.
            let state = rm.state.lock().unwrap();
            if rm.close_generation.load(Ordering::SeqCst) == gen
                && matches!(*state, RecordingState::Idle)
            {
                // stop_microphone_stream does not acquire the state lock,
                // so holding it here is safe (no deadlock).
                info!(
                    "Closing idle microphone stream after {:?}",
                    STREAM_IDLE_TIMEOUT
                );
                rm.stop_microphone_stream();
            }
        });
    }

    /* ---------- VAD --------------------------------------------------------- */

    pub fn preload_vad(&self) -> Result<(), anyhow::Error> {
        let mut vad = self.vad.lock().unwrap();
        if matches!(*vad, VadSlot::Unloaded) {
            let settings = get_settings(&self.app_handle);
            *vad = VadSlot::Idle(create_capture_vad(settings.vad_backend, &self.app_handle)?);
        }
        Ok(())
    }

    /// Samples per VAD frame: the chunk size the microphone is opened with.
    fn vad_frame_samples(&self) -> Result<usize, anyhow::Error> {
        self.preload_vad()?;
        match &*self.vad.lock().unwrap() {
            VadSlot::Idle(vad) => Ok(vad.frame_samples()),
            VadSlot::Lent { frame_samples } => Ok(*frame_samples),
            VadSlot::Unloaded => unreachable!("preload_vad just loaded it"),
        }
    }

    /// Lends the detector to a recording.
    /// Lends the detector to a new recording. Called only while idle.
    fn lend_vad(&self) -> Result<CaptureVad, anyhow::Error> {
        {
            let mut slot = self.vad.lock().unwrap();
            // No recording is active, so a lent detector went with a lost sink.
            if matches!(*slot, VadSlot::Lent { .. }) {
                warn!("The last recording did not return its VAD detector; reloading it");
                *slot = VadSlot::Unloaded;
            }
        }
        self.preload_vad()?;
        let mut slot = self.vad.lock().unwrap();
        match std::mem::replace(&mut *slot, VadSlot::Unloaded) {
            VadSlot::Idle(vad) => {
                *slot = VadSlot::Lent {
                    frame_samples: vad.frame_samples(),
                };
                Ok(vad)
            }
            other => {
                *slot = other;
                Err(anyhow::anyhow!("VAD is not available"))
            }
        }
    }

    fn return_vad(&self, vad: CaptureVad) {
        *self.vad.lock().unwrap() = VadSlot::Idle(vad);
    }

    /* ---------- microphone life-cycle -------------------------------------- */

    /// Applies mute if mute_while_recording is enabled and stream is open.
    /// Snapshots the system's prior mute state first so `remove_mute` can
    /// restore it instead of unconditionally unmuting.
    pub fn apply_mute(&self) {
        let settings = get_settings(&self.app_handle);
        if !settings.mute_while_recording {
            return;
        }

        // Lock order: is_open before mute_state (matches stop_microphone_stream).
        let is_open = self.is_open.lock().unwrap();
        let mut mute_guard = self.mute_state.lock().unwrap();
        // Already muted this session — don't re-snapshot, or a duplicate/late
        // apply would overwrite prev_muted with our own forced-muted state and
        // strand audio muted on stop.
        if mute_guard.did_mute {
            return;
        }
        if *is_open {
            mute_guard.prev_muted = get_mute();
            set_mute(true);
            mute_guard.did_mute = true;
            debug!("Mute applied (prev_muted={:?})", mute_guard.prev_muted);
        }
    }

    /// Removes mute if it was applied, restoring the system's prior mute state
    /// (a system already muted before recording stays muted).
    pub fn remove_mute(&self) {
        let mut mute_guard = self.mute_state.lock().unwrap();
        if mute_guard.did_mute {
            restore_mute(mute_guard.prev_muted);
            mute_guard.did_mute = false;
            debug!(
                "Mute removed (restored prev_muted={:?})",
                mute_guard.prev_muted
            );
        }
    }

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        let mut open_flag = self.is_open.lock().unwrap();
        if *open_flag {
            // `is_open` only records that we opened a stream, not that it is
            // still running. If the recorder has since failed (mic unplugged,
            // stalled, permission revoked), rebuild it before the next
            // recording instead of handing the caller a failed recorder.
            let needs_reopen = self
                .recorder
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(OpenMicrophone::needs_reopen);

            if !needs_reopen {
                // trace, not debug: with the aliveness check in
                // try_start_recording this now fires on every keypress in
                // always-on mode.
                trace!("Microphone stream already active");
                return Ok(());
            }

            warn!("Microphone stream failed; reopening");

            // Torn down inline rather than via stop_microphone_stream(), which
            // takes the `is_open` lock we are already holding.
            {
                let mut mute_guard = self.mute_state.lock().unwrap();
                if mute_guard.did_mute {
                    restore_mute(mute_guard.prev_muted);
                    mute_guard.did_mute = false;
                }
            }
            let failed = self.recorder.lock().unwrap().take();
            if let Some(microphone) = failed {
                self.close_microphone(microphone);
            }
            *open_flag = false;
            self.invalidate_device_cache();
            // Fall through to the same fresh resolution and fallback path used
            // when an on-demand stream opens after its device was unplugged.
        }

        let start_time = Instant::now();

        // Don't mute immediately - caller will handle muting after audio feedback.
        // The previous stream restored audio on close, so did_mute should already
        // be false here; if it somehow isn't, restore rather than just clearing the
        // flag, which would strand system audio muted.
        {
            let mut mute_guard = self.mute_state.lock().unwrap();
            if mute_guard.did_mute {
                restore_mute(mute_guard.prev_muted);
                mute_guard.did_mute = false;
            }
        }

        // Get the selected device from settings, considering clamshell mode.
        // No pre-flight enumeration here: when nothing is configured the
        // recorder resolves the system default itself, and a machine with no
        // input devices at all fails inside open() with `DeviceUnavailable`.
        let settings = get_settings(&self.app_handle);
        let resolve_started = Instant::now();
        let mut resolution = self.resolve_microphone_device(&settings);
        let resolve_elapsed = resolve_started.elapsed();

        // Ensure VAD is loaded if it wasn't for whatever reason; its frame
        // size is the chunk size.
        let vad_started = Instant::now();
        let frames_per_chunk = self.vad_frame_samples()?;
        let vad_elapsed = vad_started.elapsed();

        let channel = *self.selected_channel.lock().unwrap();
        let open_started = Instant::now();
        let microphone = match open_microphone_on_channel(
            resolution.device.clone(),
            channel,
            frames_per_chunk,
        ) {
            Err(first_err) if resolution.from_cache => {
                // The cached device ID may have gone stale (unplugged).
                // Re-resolve from a fresh enumeration and retry once before
                // surfacing the error.
                warn!("Recorder open failed ({first_err}); re-resolving device and retrying once");
                self.invalidate_device_cache();
                resolution = self.resolve_microphone_device(&settings);
                open_microphone_on_channel(resolution.device.clone(), channel, frames_per_chunk)
            }
            result => result,
        }?;
        debug!(
            "mic stream breakdown: device_resolve={:?} vad_ensure={:?} open={:?}",
            resolve_elapsed,
            vad_elapsed,
            open_started.elapsed()
        );
        *self.recorder.lock().unwrap() = Some(microphone);

        *open_flag = true;
        if let Some(unavailable_name) = resolution.unavailable_selected_microphone {
            // Do this only after the default stream opened successfully. A
            // failed fallback must not erase the user's microphone preference.
            self.persist_default_microphone_after_fallback(&unavailable_name);
        }
        // The stream is running, but the device may not be delivering audio
        // yet (Bluetooth can take seconds); a recording's readiness waits for
        // its first chunk.
        info!(
            "Microphone stream initialized in {:?}",
            start_time.elapsed()
        );
        Ok(())
    }

    pub fn stop_microphone_stream(&self) {
        let mut open_flag = self.is_open.lock().unwrap();
        if !*open_flag {
            return;
        }

        {
            let mut mute_guard = self.mute_state.lock().unwrap();
            if mute_guard.did_mute {
                restore_mute(mute_guard.prev_muted);
            }
            mute_guard.did_mute = false;
        }

        let microphone = self.recorder.lock().unwrap().take();
        if let Some(microphone) = microphone {
            self.close_microphone(microphone);
        }

        *open_flag = false;
        debug!("Microphone stream stopped");
    }

    /// Closes a microphone, first discarding any recording on it (its
    /// detector comes back).
    fn close_microphone(&self, microphone: OpenMicrophone) {
        let mut is_recording = self.is_recording.lock().unwrap();
        if *is_recording {
            let _ = self.collect_recording(&microphone);
            *is_recording = false;
        }
        if let Err(e) = microphone.recorder.close() {
            warn!("Closing the microphone failed: {e}");
        }
    }

    /// Stops the recording on `microphone`, returning its audio and taking
    /// back the detector it borrowed.
    fn collect_recording(&self, microphone: &OpenMicrophone) -> Vec<f32> {
        match microphone.recorder.stop() {
            Ok(stopped) => {
                match &stopped.end_reason {
                    EndReason::StopCalled => {}
                    EndReason::RecorderFailed(error) => {
                        microphone.failed.store(true, Ordering::Release);
                        warn!("Recording cut short by a microphone failure; keeping the audio before it: {error}");
                    }
                    EndReason::SinkPanicked(message) => {
                        error!("Recording cut short; audio processing panicked: {message}");
                    }
                }
                if stopped.dropped_frames > 0 {
                    warn!(
                        "Recording completed after dropping {} microphone frames",
                        stopped.dropped_frames
                    );
                }
                let captured = stopped.sink.finish();
                self.return_vad(captured.vad);
                captured.samples
            }
            Err(e) => {
                // The recording and its detector are lost with the sink.
                error!("Failed to stop recording: {e}");
                if e.kind() != ErrorKind::NotRecording {
                    microphone.failed.store(true, Ordering::Release);
                }
                *self.vad.lock().unwrap() = VadSlot::Unloaded;
                Vec::new()
            }
        }
    }

    /* ---------- mode switching --------------------------------------------- */

    pub fn update_mode(&self, new_mode: MicrophoneMode) -> Result<(), anyhow::Error> {
        let cur_mode = self.mode.lock().unwrap().clone();

        match (cur_mode, &new_mode) {
            (MicrophoneMode::AlwaysOn, MicrophoneMode::OnDemand) => {
                if matches!(*self.state.lock().unwrap(), RecordingState::Idle) {
                    self.close_generation.fetch_add(1, Ordering::SeqCst);
                    self.stop_microphone_stream();
                }
            }
            (MicrophoneMode::OnDemand, MicrophoneMode::AlwaysOn) => {
                self.close_generation.fetch_add(1, Ordering::SeqCst);
                self.start_microphone_stream()?;
            }
            _ => {}
        }

        *self.mode.lock().unwrap() = new_mode;
        Ok(())
    }

    /* ---------- recording --------------------------------------------------- */

    /// The one place `state` is written. Derives `recording_active` (the
    /// lock-free mirror read by `is_recording()`) from the new value itself,
    /// so the two can never drift: a new `RecordingState` variant only needs
    /// its active-set membership decided here, once.
    fn set_state(&self, guard: &mut RecordingState, new_state: RecordingState) {
        *guard = new_state;
        self.recording_active.store(
            matches!(
                *guard,
                RecordingState::Recording { .. } | RecordingState::Stopping
            ),
            Ordering::SeqCst,
        );
    }

    pub fn try_start_recording(
        &self,
        binding_id: &str,
        vad_policy: VadPolicy,
    ) -> Result<RecordingReadiness, anyhow::Error> {
        let mut state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!("Already recording"));
        }

        // Cancel any pending lazy close (no-op in always-on mode, where
        // closes are never scheduled).
        self.close_generation.fetch_add(1, Ordering::SeqCst);

        // A recorder can fail just before this start, ahead of its failure
        // handler; the start then reports it. Reopen and try once more.
        let mut reopened = false;
        loop {
            // Opens the stream in on-demand mode. In always-on mode the stream
            // is normally already open and this is a cheap aliveness check —
            // but if the recorder failed (device disconnect), it rebuilds the
            // stream instead of leaving every subsequent start wedged.
            if let Err(e) = self.start_microphone_stream() {
                error!("Failed to open microphone stream: {e}");
                return Err(e);
            }

            let (ready_tx, receiver) = mpsc::channel();
            let sink = CaptureSink::new(
                self.lend_vad()?,
                vad_policy,
                Arc::clone(&self.on_levels),
                Arc::clone(&self.on_audio),
                ready_tx,
            );

            let recorder = self.recorder.lock().unwrap();
            let Some(microphone) = recorder.as_ref() else {
                self.return_vad(sink.into_vad());
                return Err(anyhow::anyhow!("Recorder not available"));
            };
            match microphone.recorder.start(sink) {
                Ok(()) => {
                    let generation = self.capture_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    *self.is_recording.lock().unwrap() = true;
                    self.set_state(
                        &mut state,
                        RecordingState::Recording {
                            binding_id: binding_id.to_string(),
                        },
                    );
                    debug!("Recording requested for binding {binding_id}");
                    return Ok(RecordingReadiness {
                        receiver,
                        generation,
                    });
                }
                Err(StartError { error, sink }) => {
                    self.return_vad(sink.into_vad());
                    if reopened || error.kind() == ErrorKind::AlreadyRecording {
                        return Err(anyhow::Error::new(error).context("Failed to start recorder"));
                    }
                    warn!("Microphone failed before this recording ({error}); reopening");
                    microphone.failed.store(true, Ordering::Release);
                    reopened = true;
                }
            }
        }
    }

    /// Replace the VAD implementation while idle. If the microphone stream is
    /// currently warm (always-on or lazy-close mode), reopen it with the new
    /// detector's chunk size before reporting success. A failed reopen restores
    /// the previous detector so the persisted setting can remain unchanged.
    pub fn update_vad_backend(&self, backend: VadBackend) -> Result<(), anyhow::Error> {
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change the VAD backend while recording"
            ));
        }

        let replacement = create_capture_vad(backend, &self.app_handle)?;
        let was_open = *self.is_open.lock().unwrap();

        // Invalidate any delayed close before swapping the detector.
        self.close_generation.fetch_add(1, Ordering::SeqCst);
        if was_open {
            self.stop_microphone_stream();
        }

        let previous =
            std::mem::replace(&mut *self.vad.lock().unwrap(), VadSlot::Idle(replacement));
        if was_open {
            if let Err(change_error) = self.start_microphone_stream() {
                *self.vad.lock().unwrap() = previous;

                if let Err(rollback_error) = self.start_microphone_stream() {
                    error!(
                        "Failed to restore microphone stream after VAD backend change failed: {rollback_error}"
                    );
                }
                return Err(anyhow::anyhow!(
                    "Failed to reopen microphone with {:?} VAD: {change_error}",
                    backend
                ));
            }
        }

        info!("VAD backend changed to {:?}", backend);
        drop(state);
        Ok(())
    }

    pub fn update_selected_device(&self) -> Result<(), anyhow::Error> {
        // Device settings changed; re-enumerate the device and restart capture.
        self.invalidate_device_cache();
        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
            self.start_microphone_stream()?;
        }
        Ok(())
    }

    pub fn update_selected_channel(
        &self,
        selected_channel: Option<u16>,
    ) -> Result<(), anyhow::Error> {
        // Serialize against recording start/stop. Restarting an active capture
        // would discard its samples and leave the manager's recording state out
        // of sync with the new recorder.
        let state = self.state.lock().unwrap();
        if !matches!(*state, RecordingState::Idle) {
            return Err(anyhow::anyhow!(
                "Cannot change the input channel while recording"
            ));
        }

        let was_open = *self.is_open.lock().unwrap();
        if was_open {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
        }
        let previous_channel = std::mem::replace(
            &mut *self.selected_channel.lock().unwrap(),
            selected_channel,
        );
        if was_open {
            if let Err(error) = self.start_microphone_stream() {
                *self.selected_channel.lock().unwrap() = previous_channel;
                return Err(error);
            }
        }
        drop(state);
        Ok(())
    }

    /// Invalidate pending first-sample UI and audio-feedback work immediately.
    /// Called at the beginning of stop, before the slower capture drain starts.
    pub fn invalidate_recording_readiness(&self) {
        self.capture_generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn is_recording_readiness_current(&self, generation: u64) -> bool {
        self.capture_generation.load(Ordering::Acquire) == generation
    }

    pub fn cancel_generation(&self) -> u64 {
        self.cancel_generation.load(Ordering::Acquire)
    }

    pub fn was_cancelled_since(&self, generation: u64) -> bool {
        self.cancel_generation.load(Ordering::Acquire) != generation
    }

    pub fn stop_recording(&self, binding_id: &str, cancel_generation: u64) -> Option<Vec<f32>> {
        self.invalidate_recording_readiness();
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording {
                binding_id: ref active,
            } if active == binding_id => {
                self.set_state(&mut state, RecordingState::Stopping);
                drop(state);

                // Optionally keep recording for a bit longer to capture trailing audio.
                // This is only the explicit user setting; streaming VAD must not add
                // hidden post-release capture time.
                let settings = get_settings(&self.app_handle);
                let buffer_ms = settings.extra_recording_buffer_ms;
                if buffer_ms > 0 {
                    debug!(
                        "Extra recording buffer: sleeping {}ms before stopping",
                        buffer_ms
                    );
                    let started = Instant::now();
                    let buffer = Duration::from_millis(buffer_ms);
                    while started.elapsed() < buffer {
                        if self.was_cancelled_since(cancel_generation) {
                            debug!("Recording stop cancelled during extra buffer");
                            break;
                        }
                        let remaining = buffer.saturating_sub(started.elapsed());
                        std::thread::sleep(remaining.min(Duration::from_millis(25)));
                    }
                }

                let samples = match self.recorder.lock().unwrap().as_ref() {
                    Some(microphone) => self.collect_recording(microphone),
                    None => {
                        error!("Recorder not available");
                        Vec::new()
                    }
                };

                *self.is_recording.lock().unwrap() = false;
                self.set_state(&mut self.state.lock().unwrap(), RecordingState::Idle);

                // In on-demand mode, close the mic (lazily if the setting is enabled)
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    if get_settings(&self.app_handle).lazy_stream_close {
                        self.schedule_lazy_close();
                    } else {
                        self.stop_microphone_stream();
                    }
                }

                if self.was_cancelled_since(cancel_generation) {
                    debug!("Recording stop cancelled; discarding captured samples");
                    return None;
                }

                // Pad if very short
                let s_len = samples.len();
                // debug!("Got {} samples", s_len);
                if s_len < WHISPER_SAMPLE_RATE && s_len > 0 {
                    let mut padded = samples;
                    padded.resize(WHISPER_SAMPLE_RATE * 5 / 4, 0.0);
                    Some(padded)
                } else {
                    Some(samples)
                }
            }
            _ => None,
        }
    }
    pub fn is_recording(&self) -> bool {
        // Lock-free: mirrors the `state` {Recording, Stopping} membership via
        // an atomic maintained by `set_state()`. Polled from the webview/main
        // thread, so it MUST NOT take the `state` mutex (a worker can hold it
        // across a slow CoreAudio open/close → main-thread deadlock / UI
        // freeze).
        self.recording_active.load(Ordering::SeqCst)
    }

    /// Cancel any ongoing recording without returning audio samples
    pub fn cancel_recording(&self) {
        self.invalidate_recording_readiness();
        self.cancel_generation.fetch_add(1, Ordering::AcqRel);
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording { .. } => {
                self.set_state(&mut state, RecordingState::Idle);
                drop(state);

                if let Some(microphone) = self.recorder.lock().unwrap().as_ref() {
                    let _ = self.collect_recording(microphone); // Discard the result
                }

                *self.is_recording.lock().unwrap() = false;

                // In on-demand mode, close the mic (lazily if the setting is enabled)
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    if get_settings(&self.app_handle).lazy_stream_close {
                        self.schedule_lazy_close();
                    } else {
                        self.stop_microphone_stream();
                    }
                }
            }
            RecordingState::Stopping => {
                debug!("Cancellation requested while recording is stopping");
            }
            RecordingState::Idle => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorder error, without touching a device: the request is rejected
    /// before one is opened.
    fn recorder_error() -> handy_recorder::Error {
        let config = RecorderConfig {
            sample_rate: Some(0),
            ..RecorderConfig::speech()
        };
        match Recorder::<CaptureSink>::open(config) {
            Err(error) => error,
            Ok(_) => panic!("a 0 Hz output rate should be rejected"),
        }
    }

    #[test]
    fn recorder_errors_are_classified_through_added_context() {
        let error = recorder_error();
        assert_eq!(error.kind(), ErrorKind::UnsupportedFormat);

        let wrapped = anyhow::Error::new(error).context("Failed to start recorder");
        assert_eq!(
            wrapped
                .downcast_ref::<handy_recorder::Error>()
                .map(handy_recorder::Error::kind),
            Some(ErrorKind::UnsupportedFormat)
        );
        assert_eq!(recording_error_type(&wrapped), "unknown");
    }

    #[test]
    fn errors_from_outside_the_recorder_are_unknown() {
        assert_eq!(
            recording_error_type(&anyhow::anyhow!("Already recording")),
            "unknown"
        );
    }

    #[test]
    fn microphone_config_maps_the_selected_channel() {
        let all = microphone_config(None, None, 480);
        assert_eq!(all.channels, Channels::MixToMono);
        assert_eq!(all.sample_rate, Some(16_000));
        assert_eq!(all.frames_per_chunk, Some(480));

        let second = microphone_config(Some("id".into()), Some(1), 256);
        assert_eq!(second.channels, Channels::Only(1));
        assert_eq!(second.device.as_deref(), Some("id"));
        assert_eq!(second.frames_per_chunk, Some(256));
    }
}
