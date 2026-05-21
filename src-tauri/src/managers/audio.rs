use crate::audio_toolkit::{list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad};
use crate::helpers::clamshell;
use crate::settings::{get_settings, AppSettings, MediaWhileRecordingMode};
use crate::utils;
use log::{debug, error, info};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
enum MediaModification {
    None,
    Muted,
    Paused { apps: Vec<String> },
    Faded { original_volume: u8 },
}

fn set_mute(mute: bool) {
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

        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        if Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", mute_val])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

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

/// Pause only apps that are currently playing. Returns list of paused app names.
fn pause_playing_media() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // Check each known media app's playback state via AppleScript before
        // pausing. This avoids the media-key-toggle problem where paused apps
        // get started.
        let script = r#"
set pausedApps to ""
try
    tell application "System Events"
        if (name of processes) contains "Spotify" then
            tell application "Spotify"
                if player state is playing then
                    pause
                    set pausedApps to pausedApps & "spotify,"
                end if
            end tell
        end if
    end tell
end try
try
    tell application "System Events"
        if (name of processes) contains "Music" then
            tell application "Music"
                if player state is playing then
                    pause
                    set pausedApps to pausedApps & "music,"
                end if
            end tell
        end if
    end tell
end try
return pausedApps
"#;
        if let Ok(output) = Command::new("/usr/bin/osascript").args(["-e", script]).output() {
            if output.status.success() {
                let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let apps: Vec<String> = result
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if !apps.is_empty() {
                    return apps;
                }
            }
        }
        // Fallback: send media key for unknown players (best effort for browsers etc.)
        let swift_code = r#"
import Cocoa
let k: UInt32 = 16; let s = Int16(8)
if let e = NSEvent.otherEvent(with:.systemDefined,location:.zero,modifierFlags:NSEvent.ModifierFlags(rawValue:0xa00),timestamp:0,windowNumber:0,context:nil,subtype:s,data1:Int((k<<16)|(0xa<<8)),data2:-1),let c=e.cgEvent{c.post(tap:.cghidEventTap)}
if let e = NSEvent.otherEvent(with:.systemDefined,location:.zero,modifierFlags:NSEvent.ModifierFlags(rawValue:0xb00),timestamp:0,windowNumber:0,context:nil,subtype:s,data1:Int((k<<16)|(0xb<<8)),data2:-1),let c=e.cgEvent{c.post(tap:.cghidEventTap)}
"#;
        if Command::new("/usr/bin/swift").args(["-e", swift_code]).output()
            .map(|o| o.status.success()).unwrap_or(false) {
            return vec!["_mediakey".to_string()];
        }
        return Vec::new();
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        if Command::new("playerctl").args(["pause"]).output()
            .map(|o| o.status.success()).unwrap_or(false) {
            return vec!["playerctl".to_string()];
        }
        return Vec::new();
    }

    #[cfg(target_os = "windows")]
    {
        return Vec::new();
    }
}

/// Resume only the apps that were previously paused.
fn resume_paused_media(apps: &[String]) {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        for app in apps {
            match app.as_str() {
                "spotify" => {
                    let _ = Command::new("/usr/bin/osascript")
                        .args(["-e", "tell application \"Spotify\" to play"])
                        .output();
                }
                "music" => {
                    let _ = Command::new("/usr/bin/osascript")
                        .args(["-e", "tell application \"Music\" to play"])
                        .output();
                }
                "_mediakey" => {
                    // Send play/pause toggle back for unknown players
                    let swift_code = r#"
import Cocoa
let k: UInt32 = 16; let s = Int16(8)
if let e = NSEvent.otherEvent(with:.systemDefined,location:.zero,modifierFlags:NSEvent.ModifierFlags(rawValue:0xa00),timestamp:0,windowNumber:0,context:nil,subtype:s,data1:Int((k<<16)|(0xa<<8)),data2:-1),let c=e.cgEvent{c.post(tap:.cghidEventTap)}
if let e = NSEvent.otherEvent(with:.systemDefined,location:.zero,modifierFlags:NSEvent.ModifierFlags(rawValue:0xb00),timestamp:0,windowNumber:0,context:nil,subtype:s,data1:Int((k<<16)|(0xb<<8)),data2:-1),let c=e.cgEvent{c.post(tap:.cghidEventTap)}
"#;
                    let _ = Command::new("/usr/bin/swift").args(["-e", swift_code]).output();
                }
                _ => {}
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        for app in apps {
            if app == "playerctl" {
                let _ = Command::new("playerctl").args(["play"]).output();
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = apps;
    }
}

fn get_system_volume() -> Option<u8> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("osascript")
            .args(["-e", "output volume of (get volume settings)"])
            .output()
            .ok()?;
        if output.status.success() {
            let vol_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return vol_str.parse().ok();
        }
        return std::option::Option::None;
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                if let Some(vol_str) = s.split_whitespace().nth(1) {
                    if let Ok(vol) = vol_str.parse::<f32>() {
                        return Some((vol * 100.0).min(100.0) as u8);
                    }
                }
            }
        }
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                for part in s.split_whitespace() {
                    if let Some(pct) = part.strip_suffix('%') {
                        if let Ok(vol) = pct.parse::<u8>() {
                            return Some(vol);
                        }
                    }
                }
            }
        }
        return std::option::Option::None;
    }

    #[cfg(target_os = "windows")]
    {
        return std::option::Option::None;
    }
}

fn set_system_volume(volume: u8) {
    let vol = volume.min(100);

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let script = format!("set volume output volume {}", vol);
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let pct = format!("{}%", vol);
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &pct])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }
        if Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &pct])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }
        let _ = Command::new("amixer")
            .args(["set", "Master", &pct])
            .output();
    }

    #[cfg(target_os = "windows")]
    {
        debug!("Volume fade not yet implemented on Windows");
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
    media_mod: Arc<Mutex<MediaModification>>,
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
            media_mod: Arc::new(Mutex::new(MediaModification::None)),
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

    /// Applies the configured media behavior (mute / pause / fade) while recording
    pub fn apply_mute(&self) {
        let settings = get_settings(&self.app_handle);
        let mut mod_guard = self.media_mod.lock().unwrap();

        if !*self.is_open.lock().unwrap() {
            return;
        }

        let mode = settings.media_while_recording_mode;
        match mode {
            MediaWhileRecordingMode::None => {}
            MediaWhileRecordingMode::Mute => {
                set_mute(true);
                *mod_guard = MediaModification::Muted;
                debug!("Media mode: muted");
            }
            MediaWhileRecordingMode::Pause => {
                let apps = pause_playing_media();
                if !apps.is_empty() {
                    debug!("Media mode: paused {:?}", apps);
                    *mod_guard = MediaModification::Paused { apps };
                } else {
                    debug!("Media mode: nothing was playing");
                }
            }
            MediaWhileRecordingMode::Fade => {
                if let Some(vol) = get_system_volume() {
                    let faded = (vol as f32 * 0.3).round() as u8;
                    set_system_volume(faded);
                    *mod_guard = MediaModification::Faded { original_volume: vol };
                    debug!("Media mode: faded from {} to {}", vol, faded);
                } else {
                    set_mute(true);
                    *mod_guard = MediaModification::Muted;
                    debug!("Media mode: fade failed, fell back to mute");
                }
            }
        }
    }

    /// Reverses whatever media modification was applied
    pub fn remove_mute(&self) {
        let mut mod_guard = self.media_mod.lock().unwrap();
        match &*mod_guard {
            MediaModification::None => {}
            MediaModification::Muted => {
                set_mute(false);
                debug!("Media restore: unmuted");
            }
            MediaModification::Paused { apps } => {
                resume_paused_media(apps);
                debug!("Media restore: resumed {:?}", apps);
            }
            MediaModification::Faded { original_volume } => {
                set_system_volume(*original_volume);
                debug!("Media restore: volume back to {}", original_volume);
            }
        }
        *mod_guard = MediaModification::None;
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

        // Don't apply media behavior immediately - caller will handle it after audio feedback
        let mut mod_guard = self.media_mod.lock().unwrap();
        *mod_guard = MediaModification::None;

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

        let mut mod_guard = self.media_mod.lock().unwrap();
        match &*mod_guard {
            MediaModification::Muted => set_mute(false),
            MediaModification::Paused { apps } => resume_paused_media(apps),
            MediaModification::Faded { original_volume } => set_system_volume(*original_volume),
            MediaModification::None => {}
        }
        *mod_guard = MediaModification::None;

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
