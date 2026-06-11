use crate::audio_toolkit::{list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad};
use crate::helpers::clamshell;
use crate::settings::{get_settings, AppSettings};
use crate::utils;
use log::{debug, error, info};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// macOS "mute" implementation that ducks (lowers) audio instead of muting it,
/// so background music stays faintly audible but no longer drowns out the
/// microphone. Everything changed by `duck()` is restored by `restore()`.
///
/// Three strategies, tried in order:
/// 1. Core Audio process tap (macOS 14.2+, see swift/audio_duck.swift):
///    ducks every app at once, works even when the output device has no
///    software volume control. Needs the system audio recording permission;
///    without it the tap is inert, which `duck()` detects and falls through.
/// 2. System output volume via AppleScript.
/// 3. Per-app AppleScript: music players (Spotify/Music) and browser tabs.
///    Browsers additionally require their "Allow JavaScript from Apple
///    Events" developer setting; tabs are skipped silently otherwise.
#[cfg(target_os = "macos")]
mod macos_duck {
    use log::debug;
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::Duration;

    /// How long to wait for the process tap to see audio before concluding
    /// it is inert (permission missing) or there is nothing to duck — in
    /// both cases the AppleScript strategies take over.
    const TAP_PROBE_DURATION: Duration = Duration::from_millis(300);

    /// Music players we can duck individually when the output device itself
    /// has no software volume control (common for USB audio interfaces and
    /// HDMI/DisplayPort outputs).
    const SCRIPTABLE_PLAYERS: [&str; 2] = ["Spotify", "Music"];

    /// Browsers whose tabs we can reach with AppleScript. All of them ship
    /// with that capability disabled; the user must enable "Allow JavaScript
    /// from Apple Events" once (Chromium family: View > Developer menu,
    /// Safari: Develop menu).
    const SCRIPTABLE_BROWSERS: [&str; 5] = [
        "Google Chrome",
        "Chromium",
        "Brave Browser",
        "Microsoft Edge",
        "Safari",
    ];

    mod ffi {
        extern "C" {
            pub fn handy_audio_duck_supported() -> i32;
            pub fn handy_audio_duck_start(gain: f32) -> i32;
            pub fn handy_audio_duck_stop();
            pub fn handy_audio_duck_has_signal() -> i32;
            pub fn handy_audio_duck_request_permission();
        }
    }

    struct DuckState {
        /// True while the process-tap ducker is running.
        tap_active: bool,
        /// True if we set the system output mute flag (duck volume 0).
        system_muted: bool,
        /// System output volume before ducking, if we changed it.
        system_volume: Option<u8>,
        /// (player, volume before ducking) for each player we changed.
        player_volumes: Vec<(&'static str, u8)>,
        /// Browsers whose tabs we ran the duck script in.
        browsers_ducked: Vec<&'static str>,
    }

    impl DuckState {
        fn is_ducked(&self) -> bool {
            self.tap_active
                || self.system_muted
                || self.system_volume.is_some()
                || !self.player_volumes.is_empty()
                || !self.browsers_ducked.is_empty()
        }
    }

    static DUCK_STATE: Mutex<DuckState> = Mutex::new(DuckState {
        tap_active: false,
        system_muted: false,
        system_volume: None,
        player_volumes: Vec::new(),
        browsers_ducked: Vec::new(),
    });

    /// Triggers the system audio recording permission prompt (used by the
    /// process-tap ducker) at app startup instead of mid-dictation. No-op
    /// when the permission is already granted or the API is unavailable.
    pub fn request_tap_permission() {
        unsafe {
            if ffi::handy_audio_duck_supported() == 1 {
                ffi::handy_audio_duck_request_permission();
            }
        }
    }

    /// Starts the process-tap ducker and confirms it actually engaged.
    /// Returns false when unsupported, denied, or when no audio is flowing —
    /// the caller then ducks via AppleScript instead.
    fn tap_duck(volume_percent: u8) -> bool {
        unsafe {
            if ffi::handy_audio_duck_supported() != 1
                || ffi::handy_audio_duck_start(f32::from(volume_percent) / 100.0) != 1
            {
                return false;
            }
        }
        // A tap created without the audio recording permission reports
        // success but stays inert (silence, no muting). Only trust it once
        // it has carried real audio.
        std::thread::sleep(TAP_PROBE_DURATION);
        if unsafe { ffi::handy_audio_duck_has_signal() } == 1 {
            true
        } else {
            debug!("Process tap saw no audio (permission missing or nothing playing)");
            unsafe { ffi::handy_audio_duck_stop() };
            false
        }
    }

    fn run_osascript(script: &str) -> Option<String> {
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Extracts a field from `get volume settings` output, e.g.
    /// "output volume:64, input volume:75, alert volume:100, output muted:false".
    fn volume_field<'a>(settings: &'a str, key: &str) -> Option<&'a str> {
        let start = settings.find(key)? + key.len();
        let rest = &settings[start..];
        Some(rest[..rest.find(',').unwrap_or(rest.len())].trim())
    }

    /// Ducks one player if it is running and louder than the duck level.
    /// Returns the previous volume when something was changed. Players that
    /// are not running, already quiet, or denied automation permission are
    /// skipped (the script returns -1 or fails, which doesn't parse as u8).
    fn duck_player(player: &str, volume_percent: u8) -> Option<u8> {
        let script = format!(
            "if application \"{p}\" is running then\n\
             \ttell application \"{p}\"\n\
             \t\tset v to sound volume\n\
             \t\tif v > {duck} then\n\
             \t\t\tset sound volume to {duck}\n\
             \t\t\treturn v\n\
             \t\tend if\n\
             \tend tell\n\
             end if\n\
             return -1",
            p = player,
            duck = volume_percent
        );
        run_osascript(&script)?.parse::<u8>().ok()
    }

    fn is_process_running(name: &str) -> bool {
        Command::new("pgrep")
            .args(["-x", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// JavaScript run in every browser tab to duck playing media elements.
    /// Uses single quotes only so it can be embedded in an AppleScript string
    /// literal. The previous volume is parked on the element itself
    /// (dataset), so restoring needs no bookkeeping on our side.
    fn browser_duck_js(volume_percent: u8) -> String {
        format!(
            "(function(){{var els=document.querySelectorAll('video,audio');for(var i=0;i<els.length;i++){{var m=els[i];try{{if(!m.paused&&!m.muted&&m.volume>{lvl}){{m.dataset.handyPrevVol=String(m.volume);m.volume={lvl}}}}}catch(e){{}}}}return els.length}})()",
            lvl = f32::from(volume_percent) / 100.0
        )
    }

    const BROWSER_RESTORE_JS: &str = "(function(){var els=document.querySelectorAll('video,audio');for(var i=0;i<els.length;i++){var m=els[i];try{if(m.dataset.handyPrevVol){var v=parseFloat(m.dataset.handyPrevVol);delete m.dataset.handyPrevVol;if(isFinite(v)&&v>0&&v<=1){m.volume=v}}}catch(e){}}return els.length})()";

    /// AppleScript that runs `js` in every tab of `browser`. Tabs where
    /// JavaScript-from-Apple-Events is disabled fail inside the `try` and are
    /// skipped silently.
    fn browser_tab_script(browser: &str, js: &str) -> String {
        let execute_line = if browser == "Safari" {
            format!("do JavaScript \"{js}\" in t")
        } else {
            format!("execute t javascript \"{js}\"")
        };
        format!(
            "if application \"{b}\" is running then\n\
             \ttell application \"{b}\"\n\
             \t\trepeat with w in windows\n\
             \t\t\trepeat with t in tabs of w\n\
             \t\t\t\ttry\n\
             \t\t\t\t\t{exec}\n\
             \t\t\t\tend try\n\
             \t\t\tend repeat\n\
             \t\tend repeat\n\
             \tend tell\n\
             end if",
            b = browser,
            exec = execute_line
        )
    }

    /// Lowers other apps' audio to `volume_percent` so background music
    /// doesn't drown out the microphone; 0 mutes (the original behavior).
    /// Tries the process tap first (ducks everything at once), then the
    /// system output volume, then scripting players and browser tabs
    /// directly.
    pub fn duck(volume_percent: u8) {
        let mut state = DUCK_STATE.lock().unwrap();
        if state.is_ducked() {
            // Already ducked; don't overwrite the saved volumes.
            return;
        }

        if tap_duck(volume_percent) {
            state.tap_active = true;
            debug!("Audio ducked via process tap");
            return;
        }

        let settings = run_osascript("get volume settings").unwrap_or_default();
        match volume_field(&settings, "output volume:").and_then(|v| v.parse::<u8>().ok()) {
            Some(current) => {
                // Leave muted output alone: setting a volume would unmute it.
                if volume_field(&settings, "output muted:") != Some("false") {
                    return;
                }
                if volume_percent == 0 {
                    // Full mute via the mute flag, like the original behavior
                    if run_osascript("set volume output muted true").is_some() {
                        state.system_muted = true;
                        debug!("Audio muted via system output mute flag");
                    }
                    return;
                }
                // Already at or below the duck level: nothing to do.
                if current <= volume_percent {
                    return;
                }
                let script = format!("set volume output volume {}", volume_percent);
                if run_osascript(&script).is_some() {
                    state.system_volume = Some(current);
                    debug!("Audio ducked via system output volume");
                }
            }
            None => {
                // Output device has no software volume control; duck the
                // audio sources themselves instead.
                for player in SCRIPTABLE_PLAYERS {
                    if let Some(previous) = duck_player(player, volume_percent) {
                        state.player_volumes.push((player, previous));
                    }
                }
                let js = browser_duck_js(volume_percent);
                for browser in SCRIPTABLE_BROWSERS {
                    if is_process_running(browser)
                        && run_osascript(&browser_tab_script(browser, &js)).is_some()
                    {
                        state.browsers_ducked.push(browser);
                    }
                }
                debug!(
                    "Audio ducked via app scripting: {} players, {} browsers",
                    state.player_volumes.len(),
                    state.browsers_ducked.len()
                );
            }
        }
    }

    /// Restores everything `duck` changed.
    pub fn restore() {
        let mut state = DUCK_STATE.lock().unwrap();
        if state.tap_active {
            unsafe { ffi::handy_audio_duck_stop() };
            state.tap_active = false;
        }
        if state.system_muted {
            let _ = run_osascript("set volume output muted false");
            state.system_muted = false;
        }
        if let Some(previous) = state.system_volume.take() {
            let _ = run_osascript(&format!("set volume output volume {}", previous));
        }
        for (player, previous) in state.player_volumes.drain(..) {
            let script = format!(
                "if application \"{p}\" is running then tell application \"{p}\" to set sound volume to {v}",
                p = player,
                v = previous
            );
            let _ = run_osascript(&script);
        }
        for browser in state.browsers_ducked.drain(..) {
            let _ = run_osascript(&browser_tab_script(browser, BROWSER_RESTORE_JS));
        }
    }
}

/// Triggers the macOS system audio recording permission prompt used by the
/// process-tap ducker, so it appears at a predictable time (startup) rather
/// than during the first dictation. No-op when already granted, denied, or
/// on other platforms.
pub fn request_audio_duck_permission() {
    #[cfg(target_os = "macos")]
    macos_duck::request_tap_permission();
}

fn set_mute(mute: bool, duck_volume: u8) {
    // Expected behavior:
    // - Windows: mutes system audio; works on most systems using standard audio drivers.
    // - Linux: mutes system audio; works on many systems (PipeWire, PulseAudio, ALSA),
    //   but some distros may lack the tools used.
    // - macOS: lowers system audio to `duck_volume` percent (0 = full mute,
    //   the original behavior) and restores it on unmute — see `macos_duck`.
    // If unsupported, fails silently.
    #[cfg(not(target_os = "macos"))]
    let _ = duck_volume; // Windows/Linux currently mute regardless of level

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
        if mute {
            macos_duck::duck(duck_volume);
        } else {
            macos_duck::restore();
        }
    }
}

const WHISPER_SAMPLE_RATE: usize = 16000;

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone, Debug)]
pub enum RecordingState {
    Idle,
    Recording { binding_id: String },
}

#[derive(Clone, Debug)]
pub enum MicrophoneMode {
    AlwaysOn,
    OnDemand,
}

/// Tracks whether we changed system audio, plus a generation counter that ties
/// each (possibly delayed) `apply_mute` to the recording session that requested
/// it. `remove_mute` bumps the generation, so an apply that lands after the
/// session already ended (e.g. a very quick press/release racing the delayed
/// audio-feedback thread) becomes a no-op instead of leaving system audio
/// muted/ducked with nothing left to restore it.
#[derive(Debug, Default)]
struct MuteState {
    generation: u64,
    did_mute: bool,
}

/* ──────────────────────────────────────────────────────────────── */

fn create_audio_recorder(
    vad_path: &str,
    app_handle: &tauri::AppHandle,
) -> Result<AudioRecorder, anyhow::Error> {
    let silero = SileroVad::new(vad_path, 0.3)
        .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {}", e))?;
    let smoothed_vad = SmoothedVad::new(Box::new(silero), 15, 15, 2);

    // Recorder with VAD plus a spectrum-level callback that forwards updates to
    // the frontend.
    let recorder = AudioRecorder::new()
        .map_err(|e| anyhow::anyhow!("Failed to create AudioRecorder: {}", e))?
        .with_vad(Box::new(smoothed_vad))
        .with_level_callback({
            let app_handle = app_handle.clone();
            move |levels| {
                utils::emit_levels(&app_handle, &levels);
            }
        });

    Ok(recorder)
}

/* ──────────────────────────────────────────────────────────────── */

#[derive(Clone)]
pub struct AudioRecordingManager {
    state: Arc<Mutex<RecordingState>>,
    mode: Arc<Mutex<MicrophoneMode>>,
    app_handle: tauri::AppHandle,

    recorder: Arc<Mutex<Option<AudioRecorder>>>,
    is_open: Arc<Mutex<bool>>,
    is_recording: Arc<Mutex<bool>>,
    mute_state: Arc<Mutex<MuteState>>,
    close_generation: Arc<AtomicU64>,
}

impl AudioRecordingManager {
    /* ---------- construction ------------------------------------------------ */

    pub fn new(app: &tauri::AppHandle) -> Result<Self, anyhow::Error> {
        let settings = get_settings(app);
        let mode = if settings.always_on_microphone {
            MicrophoneMode::AlwaysOn
        } else {
            MicrophoneMode::OnDemand
        };

        let manager = Self {
            state: Arc::new(Mutex::new(RecordingState::Idle)),
            mode: Arc::new(Mutex::new(mode.clone())),
            app_handle: app.clone(),

            recorder: Arc::new(Mutex::new(None)),
            is_open: Arc::new(Mutex::new(false)),
            is_recording: Arc::new(Mutex::new(false)),
            mute_state: Arc::new(Mutex::new(MuteState::default())),
            close_generation: Arc::new(AtomicU64::new(0)),
        };

        // Always-on?  Open immediately.
        if matches!(mode, MicrophoneMode::AlwaysOn) {
            manager.start_microphone_stream()?;
        }

        Ok(manager)
    }

    /* ---------- helper methods --------------------------------------------- */

    fn get_effective_microphone_device(&self, settings: &AppSettings) -> Option<cpal::Device> {
        // Check if we're in clamshell mode and have a clamshell microphone configured
        let use_clamshell_mic = if let Ok(is_clamshell) = clamshell::is_clamshell() {
            is_clamshell && settings.clamshell_microphone.is_some()
        } else {
            false
        };

        let device_name = if use_clamshell_mic {
            settings.clamshell_microphone.as_ref().unwrap()
        } else {
            settings.selected_microphone.as_ref()?
        };

        // Find the device by name
        match list_input_devices() {
            Ok(devices) => devices
                .into_iter()
                .find(|d| d.name == *device_name)
                .map(|d| d.device),
            Err(e) => {
                debug!("Failed to list devices, using default: {}", e);
                None
            }
        }
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

    /* ---------- microphone life-cycle -------------------------------------- */

    /// Snapshot of the current mute generation. Callers that defer
    /// `apply_mute` (e.g. until after the start sound finished playing) must
    /// take this before spawning the delayed work, so the apply is skipped if
    /// the recording session ends first.
    pub fn mute_generation(&self) -> u64 {
        self.mute_state.lock().unwrap().generation
    }

    /// Applies mute if mute_while_recording is enabled, the stream is open and
    /// the session identified by `generation` is still the active one
    pub fn apply_mute(&self, generation: u64) {
        let settings = get_settings(&self.app_handle);
        if !settings.mute_while_recording {
            return;
        }

        // Lock order: is_open before mute_state (same as stop_microphone_stream)
        let is_open = self.is_open.lock().unwrap();
        let mut mute_state = self.mute_state.lock().unwrap();
        if !*is_open || mute_state.generation != generation {
            debug!("Skipping mute: stream closed or session already ended");
            return;
        }
        set_mute(true, settings.recording_duck_volume);
        mute_state.did_mute = true;
        debug!("Mute applied");
    }

    /// Removes mute if it was applied and invalidates any pending delayed apply
    pub fn remove_mute(&self) {
        let mut mute_state = self.mute_state.lock().unwrap();
        mute_state.generation += 1;
        if mute_state.did_mute {
            set_mute(false, 0);
            mute_state.did_mute = false;
            debug!("Mute removed");
        }
    }

    pub fn preload_vad(&self) -> Result<(), anyhow::Error> {
        let mut recorder_opt = self.recorder.lock().unwrap();
        if recorder_opt.is_none() {
            let vad_path = self
                .app_handle
                .path()
                .resolve(
                    "resources/models/silero_vad_v4.onnx",
                    tauri::path::BaseDirectory::Resource,
                )
                .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {}", e))?;
            *recorder_opt = Some(create_audio_recorder(
                vad_path.to_str().unwrap(),
                &self.app_handle,
            )?);
        }
        Ok(())
    }

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        let mut open_flag = self.is_open.lock().unwrap();
        if *open_flag {
            debug!("Microphone stream already active");
            return Ok(());
        }

        let start_time = Instant::now();

        // Don't mute immediately - caller will handle muting after audio feedback.
        // The previous stream restores audio on close, so did_mute should already
        // be false here; if it somehow isn't, restore instead of just clearing the
        // flag, which would strand system audio in the muted/ducked state.
        {
            let mut mute_state = self.mute_state.lock().unwrap();
            if mute_state.did_mute {
                set_mute(false, 0);
                mute_state.did_mute = false;
            }
        }

        // Get the selected device from settings, considering clamshell mode
        let settings = get_settings(&self.app_handle);
        let selected_device = self.get_effective_microphone_device(&settings);

        // Pre-flight check: if no device was selected/configured AND no devices
        // exist at all, fail early with a clear error instead of letting cpal
        // produce a cryptic backend-specific message.
        if selected_device.is_none() {
            let has_any_device = list_input_devices()
                .map(|devices| !devices.is_empty())
                .unwrap_or(false);
            if !has_any_device {
                return Err(anyhow::anyhow!("No input device found"));
            }
        }

        // Ensure VAD is loaded if it wasn't for whatever reason
        self.preload_vad()?;

        let mut recorder_opt = self.recorder.lock().unwrap();
        if let Some(rec) = recorder_opt.as_mut() {
            rec.open(selected_device)
                .map_err(|e| anyhow::anyhow!("Failed to open recorder: {}", e))?;
        }

        *open_flag = true;
        // This timing covers through cpal's stream.play() returning — i.e. the
        // point cpal surfaces as "stream running." It does NOT guarantee the
        // host audio device is producing samples yet; the first input callback
        // fires asynchronously one buffer period later (hardware dependent,
        // typically ~10–200ms on macOS, longer on Bluetooth/USB).
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
            let mut mute_state = self.mute_state.lock().unwrap();
            // Invalidate any pending delayed apply for the closing stream
            mute_state.generation += 1;
            if mute_state.did_mute {
                set_mute(false, 0);
                mute_state.did_mute = false;
            }
        }

        if let Some(rec) = self.recorder.lock().unwrap().as_mut() {
            // If still recording, stop first.
            if *self.is_recording.lock().unwrap() {
                let _ = rec.stop();
                *self.is_recording.lock().unwrap() = false;
            }
            let _ = rec.close();
        }

        *open_flag = false;
        debug!("Microphone stream stopped");
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

    pub fn try_start_recording(&self, binding_id: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Idle = *state {
            // Ensure microphone is open in on-demand mode
            if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                // Cancel any pending lazy close
                self.close_generation.fetch_add(1, Ordering::SeqCst);
                if let Err(e) = self.start_microphone_stream() {
                    let msg = format!("{e}");
                    error!("Failed to open microphone stream: {msg}");
                    return Err(msg);
                }
            }

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                if rec.start().is_ok() {
                    *self.is_recording.lock().unwrap() = true;
                    *state = RecordingState::Recording {
                        binding_id: binding_id.to_string(),
                    };
                    debug!("Recording started for binding {binding_id}");
                    return Ok(());
                }
            }
            Err("Recorder not available".to_string())
        } else {
            Err("Already recording".to_string())
        }
    }

    pub fn update_selected_device(&self) -> Result<(), anyhow::Error> {
        // If currently open, restart the microphone stream to use the new device
        if *self.is_open.lock().unwrap() {
            self.close_generation.fetch_add(1, Ordering::SeqCst);
            self.stop_microphone_stream();
            self.start_microphone_stream()?;
        }
        Ok(())
    }

    pub fn stop_recording(&self, binding_id: &str) -> Option<Vec<f32>> {
        let mut state = self.state.lock().unwrap();

        match *state {
            RecordingState::Recording {
                binding_id: ref active,
            } if active == binding_id => {
                *state = RecordingState::Idle;
                drop(state);

                // Optionally keep recording for a bit longer to capture trailing audio
                let settings = get_settings(&self.app_handle);
                if settings.extra_recording_buffer_ms > 0 {
                    debug!(
                        "Extra recording buffer: sleeping {}ms before stopping",
                        settings.extra_recording_buffer_ms
                    );
                    std::thread::sleep(Duration::from_millis(settings.extra_recording_buffer_ms));
                }

                let samples = if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                    match rec.stop() {
                        Ok(buf) => buf,
                        Err(e) => {
                            error!("stop() failed: {e}");
                            Vec::new()
                        }
                    }
                } else {
                    error!("Recorder not available");
                    Vec::new()
                };

                *self.is_recording.lock().unwrap() = false;

                // In on-demand mode, close the mic (lazily if the setting is enabled)
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    if get_settings(&self.app_handle).lazy_stream_close {
                        self.schedule_lazy_close();
                    } else {
                        self.stop_microphone_stream();
                    }
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
        matches!(
            *self.state.lock().unwrap(),
            RecordingState::Recording { .. }
        )
    }

    /// Cancel any ongoing recording without returning audio samples
    pub fn cancel_recording(&self) {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Recording { .. } = *state {
            *state = RecordingState::Idle;
            drop(state);

            // Restore system audio right away; in always-on mode (or with lazy
            // stream close) nothing else would unmute after a cancellation
            self.remove_mute();

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                let _ = rec.stop(); // Discard the result
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
    }
}
