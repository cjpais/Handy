use crate::audio_toolkit::{list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad};
use crate::helpers::clamshell;
use crate::settings::{get_settings, AppSettings};
use crate::utils;
use log::{debug, error, info};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Manager;

#[derive(Clone, Copy, Debug)]
struct VolumeSnapshot {
    level: f32,
    muted: bool,
}

fn clamp_volume(level: f32) -> f32 {
    level.clamp(0.0, 1.0)
}

fn get_volume_snapshot() -> Option<VolumeSnapshot> {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            use windows::Win32::{
                Foundation::BOOL,
                Media::Audio::{
                    eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                    MMDeviceEnumerator,
                },
                System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
            };

            macro_rules! unwrap_or_none {
                ($expr:expr) => {
                    match $expr {
                        Ok(val) => val,
                        Err(_) => return None,
                    }
                };
            }

            // Initialize the COM library for this thread.
            // If already initialized (e.g., by another library like Tauri), this does nothing.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let all_devices: IMMDeviceEnumerator =
                unwrap_or_none!(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL));
            let default_device =
                unwrap_or_none!(all_devices.GetDefaultAudioEndpoint(eRender, eMultimedia));
            let volume_interface =
                unwrap_or_none!(default_device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None));

            let mut level = 0.0f32;
            if volume_interface
                .GetMasterVolumeLevelScalar(&mut level)
                .is_err()
            {
                return None;
            }

            let mut muted = BOOL(0);
            if volume_interface.GetMute(&mut muted).is_err() {
                return None;
            }

            return Some(VolumeSnapshot {
                level: clamp_volume(level),
                muted: muted.as_bool(),
            });
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let muted = text.contains("[MUTED]");
                let tokens: Vec<&str> = text.split_whitespace().collect();
                let mut level = None;

                for (index, token) in tokens.iter().enumerate() {
                    if token.starts_with("Volume:") {
                        let value = token.trim_start_matches("Volume:");
                        let candidate = if value.is_empty() {
                            tokens.get(index + 1).copied()
                        } else {
                            Some(*token)
                        };

                        if let Some(candidate) = candidate {
                            if let Ok(parsed) =
                                candidate.trim_start_matches("Volume:").parse::<f32>()
                            {
                                level = Some(parsed);
                            }
                        }
                        break;
                    }
                }

                if let Some(level) = level {
                    return Some(VolumeSnapshot {
                        level: clamp_volume(level),
                        muted,
                    });
                }
            }
        }

        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let percent = text
                    .split_whitespace()
                    .find(|token| token.ends_with('%'))
                    .and_then(|token| token.trim_end_matches('%').parse::<f32>().ok());

                if let Some(percent) = percent {
                    let muted = Command::new("pactl")
                        .args(["get-sink-mute", "@DEFAULT_SINK@"])
                        .output()
                        .ok()
                        .and_then(|output| String::from_utf8(output.stdout).ok())
                        .map(|output| output.to_lowercase().contains("yes"))
                        .unwrap_or(false);

                    return Some(VolumeSnapshot {
                        level: clamp_volume(percent / 100.0),
                        muted,
                    });
                }
            }
        }

        if let Ok(output) = Command::new("amixer").args(["get", "Master"]).output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let percent = text
                    .split_whitespace()
                    .find(|token| token.starts_with('[') && token.ends_with("%]"))
                    .and_then(|token| {
                        token
                            .trim_start_matches('[')
                            .trim_end_matches("]")
                            .trim_end_matches('%')
                            .parse::<f32>()
                            .ok()
                    });

                if let Some(percent) = percent {
                    let muted = text.contains("[off]");
                    return Some(VolumeSnapshot {
                        level: clamp_volume(percent / 100.0),
                        muted,
                    });
                }
            }
        }

        return None;
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let script =
            "output volume of (get volume settings) & \",\" & output muted of (get volume settings)";
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut parts = text.trim().split(',');
        let volume = parts.next()?.trim().parse::<f32>().ok()?;
        let muted = parts
            .next()
            .map(|value| value.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        return Some(VolumeSnapshot {
            level: clamp_volume(volume / 100.0),
            muted,
        });
    }

    #[allow(unreachable_code)]
    None
}

fn set_volume(level: f32) {
    // Expected behavior:
    // - Windows: works on most systems using standard audio drivers.
    // - Linux: works on many systems (PipeWire, PulseAudio, ALSA),
    //   but some distros may lack the tools used.
    // - macOS: works on most standard setups via AppleScript.
    // If unsupported, fails silently.
    let level = clamp_volume(level);

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

            let _ = volume_interface.SetMasterVolumeLevelScalar(level, std::ptr::null());
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let level_str = format!("{level:.2}");
        let percent = format!("{}%", (level * 100.0).round());

        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &level_str])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        if Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &percent])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return;
        }

        let _ = Command::new("amixer")
            .args(["set", "Master", &percent])
            .output();
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let volume = (level * 100.0).round() as i32;
        let script = format!("set volume output volume {}", volume);
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
}

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
    ducking_snapshot: Arc<Mutex<Option<VolumeSnapshot>>>,
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
            ducking_snapshot: Arc::new(Mutex::new(None)),
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

    /* ---------- microphone life-cycle -------------------------------------- */

    /// Applies audio ducking if enabled and stream is open
    pub fn apply_ducking(&self) {
        let settings = get_settings(&self.app_handle);
        if !settings.audio_ducking_enabled || !*self.is_open.lock().unwrap() {
            return;
        }

        let mut snapshot_guard = self.ducking_snapshot.lock().unwrap();
        if snapshot_guard.is_some() {
            return;
        }

        let Some(snapshot) = get_volume_snapshot() else {
            debug!("Unable to read system volume for ducking");
            return;
        };

        if snapshot.muted {
            debug!("System output muted; skipping ducking");
            return;
        }

        let target_level = clamp_volume(settings.audio_ducking_level);
        if target_level >= snapshot.level {
            debug!("Ducking target above current volume; skipping");
            return;
        }

        set_volume(target_level);
        *snapshot_guard = Some(snapshot);
        debug!("Audio ducking applied");
    }

    /// Removes audio ducking if it was applied
    pub fn remove_ducking(&self) {
        let mut snapshot_guard = self.ducking_snapshot.lock().unwrap();
        if let Some(snapshot) = snapshot_guard.take() {
            let current_muted = get_volume_snapshot()
                .map(|current| current.muted)
                .unwrap_or(false);
            set_volume(snapshot.level);
            if snapshot.muted || current_muted {
                set_mute(true);
            }
            debug!("Audio ducking removed");
        }
    }

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        let mut open_flag = self.is_open.lock().unwrap();
        if *open_flag {
            debug!("Microphone stream already active");
            return Ok(());
        }

        let start_time = Instant::now();

        // Don't duck immediately - caller will handle ducking after audio feedback
        let mut ducking_guard = self.ducking_snapshot.lock().unwrap();
        *ducking_guard = None;

        let vad_path = self
            .app_handle
            .path()
            .resolve(
                "resources/models/silero_vad_v4.onnx",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {}", e))?;
        let mut recorder_opt = self.recorder.lock().unwrap();

        if recorder_opt.is_none() {
            *recorder_opt = Some(create_audio_recorder(
                vad_path.to_str().unwrap(),
                &self.app_handle,
            )?);
        }

        // Get the selected device from settings, considering clamshell mode
        let settings = get_settings(&self.app_handle);
        let selected_device = self.get_effective_microphone_device(&settings);

        if let Some(rec) = recorder_opt.as_mut() {
            rec.open(selected_device)
                .map_err(|e| anyhow::anyhow!("Failed to open recorder: {}", e))?;
        }

        *open_flag = true;
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

        self.remove_ducking();

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
        let mode_guard = self.mode.lock().unwrap();
        let cur_mode = mode_guard.clone();

        match (cur_mode, &new_mode) {
            (MicrophoneMode::AlwaysOn, MicrophoneMode::OnDemand) => {
                if matches!(*self.state.lock().unwrap(), RecordingState::Idle) {
                    drop(mode_guard);
                    self.stop_microphone_stream();
                }
            }
            (MicrophoneMode::OnDemand, MicrophoneMode::AlwaysOn) => {
                drop(mode_guard);
                self.start_microphone_stream()?;
            }
            _ => {}
        }

        *self.mode.lock().unwrap() = new_mode;
        Ok(())
    }

    /* ---------- recording --------------------------------------------------- */

    pub fn try_start_recording(&self, binding_id: &str) -> bool {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Idle = *state {
            // Ensure microphone is open in on-demand mode
            if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                if let Err(e) = self.start_microphone_stream() {
                    error!("Failed to open microphone stream: {e}");
                    return false;
                }
            }

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                if rec.start().is_ok() {
                    *self.is_recording.lock().unwrap() = true;
                    *state = RecordingState::Recording {
                        binding_id: binding_id.to_string(),
                    };
                    debug!("Recording started for binding {binding_id}");
                    return true;
                }
            }
            error!("Recorder not available");
            false
        } else {
            false
        }
    }

    pub fn update_selected_device(&self) -> Result<(), anyhow::Error> {
        // If currently open, restart the microphone stream to use the new device
        if *self.is_open.lock().unwrap() {
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

                // In on-demand mode turn the mic off again
                if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                    self.stop_microphone_stream();
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
            self.remove_ducking();
            *state = RecordingState::Idle;
            drop(state);

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                let _ = rec.stop(); // Discard the result
            }

            *self.is_recording.lock().unwrap() = false;

            // In on-demand mode turn the mic off again
            if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                self.stop_microphone_stream();
            }
        }
    }
}
