use crate::audio_toolkit::{
    find_ai_mouse_microphone_name, list_input_devices, vad::SmoothedVad, AudioRecorder, SileroVad,
};
use crate::helpers::clamshell;
use crate::settings::{get_settings, AppSettings};
use crate::utils;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Manager;

#[cfg(windows)]
use crate::managers::hid_mouse::HidMouseMonitorState;

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
    /// Lock-free gate: set to false immediately when stop is requested so
    /// inject_hid_audio() stops flooding the channel before rec.stop() returns.
    hid_inject_active: Arc<AtomicBool>,
    did_mute: Arc<Mutex<bool>>,
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
            hid_inject_active: Arc::new(AtomicBool::new(false)),
            did_mute: Arc::new(Mutex::new(false)),
        };

        // Always-on?  Open immediately.
        if matches!(mode, MicrophoneMode::AlwaysOn) {
            manager.start_microphone_stream()?;
        }

        Ok(manager)
    }

    /* ---------- helper methods --------------------------------------------- */

    /// Returns `true` when an AI mouse is connected via HID and its audio
    /// arrives over the private HID protocol rather than as a WASAPI endpoint.
    fn hid_mouse_is_active(&self) -> bool {
        #[cfg(windows)]
        {
            self.app_handle
                .try_state::<Arc<HidMouseMonitorState>>()
                .map(|s| !s.snapshot().matched_devices.is_empty())
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        false
    }

    /// Resolve the microphone cpal device this app is allowed to use.
    ///
    /// When an AI mouse is active and sends audio over HID, we skip WASAPI
    /// enumeration entirely and return `None`; the recorder will be opened in
    /// HID injection mode.  When no HID mouse is detected we fall back to the
    /// WASAPI endpoint exposed by the receiver dongle.
    fn get_effective_microphone_device(
        &self,
        _settings: &AppSettings,
    ) -> Result<Option<cpal::Device>, String> {
        if self.hid_mouse_is_active() {
            info!("HID mouse active — using HID audio injection mode (no WASAPI device)");
            return Ok(None);
        }

        // Legacy path: look for the USB Audio Class endpoint via WASAPI.
        let mouse_mic_name = find_ai_mouse_microphone_name().ok_or_else(|| {
            "未检测到鼠标麦克风：请确认 AI 鼠标接收器已正确插入 USB 端口。".to_string()
        })?;

        let devices = list_input_devices()
            .map_err(|e| format!("枚举音频输入设备失败：{e}"))?;

        let chosen = devices
            .into_iter()
            .find(|d| d.name == mouse_mic_name)
            .map(|d| d.device);

        match chosen {
            Some(device) => {
                info!("Using AI mouse microphone: {mouse_mic_name}");
                Ok(Some(device))
            }
            None => {
                warn!(
                    "AI mouse mic '{mouse_mic_name}' is registered with WASAPI but cpal didn't list it; \
                     the receiver may have just been plugged in — please retry."
                );
                Err(format!(
                    "未能打开鼠标麦克风（系统已识别为 \"{mouse_mic_name}\" 但 cpal 暂未枚举到）。请稍候重试，或重新插拔 AI 鼠标接收器。"
                ))
            }
        }
    }

    /// Public probe used by the frontend (e.g. settings UI) to learn the
    /// current AI mouse mic friendly name, or `None` when the receiver is
    /// unplugged.
    pub fn ai_mouse_microphone_name() -> Option<String> {
        find_ai_mouse_microphone_name()
    }

    /// Inject f32 mono 16 kHz samples produced by the HID ADPCM decoder into
    /// the active recorder. No-op when not in HID injection mode or not recording.
    pub fn inject_hid_audio(&self, samples: Vec<f32>) {
        // Lock-free gate: cleared atomically before rec.stop() is called so
        // HID frames stop entering the channel immediately, without any lock.
        if !self.hid_inject_active.load(Ordering::Relaxed) {
            return;
        }
        if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
            rec.inject_samples(samples);
        }
    }

    /* ---------- microphone life-cycle -------------------------------------- */

    /// Applies mute if mute_while_recording is enabled and stream is open
    pub fn apply_mute(&self) {
        let settings = get_settings(&self.app_handle);
        let mut did_mute_guard = self.did_mute.lock().unwrap();

        if settings.mute_while_recording && *self.is_open.lock().unwrap() {
            set_mute(true);
            *did_mute_guard = true;
            debug!("Mute applied");
        }
    }

    /// Removes mute if it was applied
    pub fn remove_mute(&self) {
        let mut did_mute_guard = self.did_mute.lock().unwrap();
        if *did_mute_guard {
            set_mute(false);
            *did_mute_guard = false;
            debug!("Mute removed");
        }
    }

    pub fn start_microphone_stream(&self) -> Result<(), anyhow::Error> {
        let mut open_flag = self.is_open.lock().unwrap();
        if *open_flag {
            debug!("Microphone stream already active");
            return Ok(());
        }

        let start_time = Instant::now();

        // Don't mute immediately - caller will handle muting after audio feedback
        let mut did_mute_guard = self.did_mute.lock().unwrap();
        *did_mute_guard = false;

        let vad_path = self
            .app_handle
            .path()
            .resolve(
                "resources/models/silero_vad_v4.onnx",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve VAD path: {}", e))?;
        let mut recorder_opt = self.recorder.lock().unwrap();

        let settings = get_settings(&self.app_handle);
        let mic_device = self
            .get_effective_microphone_device(&settings)
            .map_err(|e| anyhow::anyhow!(e))?;

        if recorder_opt.is_none() {
            // In HID injection mode skip VAD — ADPCM audio is pre-filtered by
            // the mouse firmware; VAD may misclassify compressed artifacts as
            // noise and swallow all samples.
            let rec = if mic_device.is_none() {
                AudioRecorder::new()
                    .map_err(|e| anyhow::anyhow!("Failed to create AudioRecorder: {}", e))?
                    .with_level_callback({
                        let app_handle = self.app_handle.clone();
                        move |levels| {
                            crate::utils::emit_levels(&app_handle, &levels);
                        }
                    })
            } else {
                create_audio_recorder(vad_path.to_str().unwrap(), &self.app_handle)?
            };
            *recorder_opt = Some(rec);
        }

        if let Some(rec) = recorder_opt.as_mut() {
            rec.open(mic_device)
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

        let mut did_mute_guard = self.did_mute.lock().unwrap();
        if *did_mute_guard {
            set_mute(false);
        }
        *did_mute_guard = false;

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

    pub fn try_start_recording(&self, binding_id: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();

        if let RecordingState::Idle = *state {
            // Ensure microphone is open in on-demand mode
            if matches!(*self.mode.lock().unwrap(), MicrophoneMode::OnDemand) {
                if let Err(e) = self.start_microphone_stream() {
                    let msg = format!("{e}");
                    error!("Failed to open microphone stream: {msg}");
                    return Err(msg);
                }
            }

            if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                if rec.start().is_ok() {
                    *self.is_recording.lock().unwrap() = true;
                    self.hid_inject_active.store(true, Ordering::Relaxed);
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

                // Stop HID injection atomically before calling rec.stop().
                self.hid_inject_active.store(false, Ordering::Relaxed);
                *self.is_recording.lock().unwrap() = false;

                // Wake the consumer thread: it blocks on sample_rx.recv() and
                // won't see Cmd::Stop until at least one chunk arrives.
                // EndOfStream is handled as `continue` in the top-level loop,
                // so after processing it the consumer checks cmd_rx and finds Stop.
                {
                    let rec_guard = self.recorder.lock().unwrap();
                    if let Some(rec) = rec_guard.as_ref() {
                        rec.send_end_of_stream();
                    }
                }
                info!("stop_recording: hid gate closed + EOS sent, calling rec.stop()…");

                let t0 = Instant::now();
                let samples = if let Some(rec) = self.recorder.lock().unwrap().as_ref() {
                    info!("stop_recording: recorder lock acquired, sending Cmd::Stop");
                    match rec.stop() {
                        Ok(buf) => {
                            info!("stop_recording: rec.stop() returned {} samples in {:?}", buf.len(), t0.elapsed());
                            buf
                        }
                        Err(e) => {
                            error!("stop() failed: {e}");
                            Vec::new()
                        }
                    }
                } else {
                    error!("Recorder not available");
                    Vec::new()
                };

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
