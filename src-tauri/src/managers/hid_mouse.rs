use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "macos")]
use aimouse_device_init::macos_impl::{
    MacosHidStarter as PlatformHidStarter,
    MacosManufacturerResolver as PlatformManufacturerResolver,
    MacosUsbHidProvider as PlatformUsbHidProvider, MoserDispatcher,
};
#[cfg(any(windows, target_os = "macos"))]
use aimouse_device_init::models::Mouser;
#[cfg(any(windows, target_os = "macos"))]
use aimouse_device_init::moser_hid_startup::{
    ButtonFunctionDefinition, ConnectionMode, HandlerConfig, MoserHost,
};
#[cfg(any(windows, target_os = "macos"))]
use aimouse_device_init::ports::{HidStarter, ManufacturerResolver, UsbHidProvider};
#[cfg(windows)]
use aimouse_device_init::windows_impl::{
    MoserDispatcher, WindowsHidStarter as PlatformHidStarter,
    WindowsManufacturerResolver as PlatformManufacturerResolver,
    WindowsUsbHidProvider as PlatformUsbHidProvider,
};

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, Default)]
pub struct DetectedHidMouse {
    pub hid_id: String,
    pub vid: i32,
    pub pid: i32,
    pub device_type: i32,
    pub manufacturer_id: i32,
    pub type_name: String,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq, Default)]
pub struct HidMouseMonitorSnapshot {
    pub matched_devices: Vec<DetectedHidMouse>,
    pub last_error: Option<String>,
    pub updated_at_unix_ms: Option<u128>,
}

pub struct HidMouseMonitorState {
    snapshot: Mutex<HidMouseMonitorSnapshot>,
}

impl HidMouseMonitorState {
    fn new() -> Self {
        Self {
            snapshot: Mutex::new(HidMouseMonitorSnapshot::default()),
        }
    }

    fn replace_snapshot(&self, snapshot: HidMouseMonitorSnapshot) {
        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = snapshot;
        }
    }

    pub fn snapshot(&self) -> HidMouseMonitorSnapshot {
        self.snapshot
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    fn has_matched_device(&self) -> bool {
        self.snapshot
            .lock()
            .map(|guard| !guard.matched_devices.is_empty())
            .unwrap_or(false)
    }
}

pub fn start_hid_mouse_monitor(app: &AppHandle) -> Arc<HidMouseMonitorState> {
    let state = Arc::new(HidMouseMonitorState::new());

    #[cfg(any(windows, target_os = "macos"))]
    {
        let starter = Arc::new(PlatformHidStarter::new());
        install_hid_dispatcher(app.clone(), Arc::clone(&starter));
        spawn_hid_mouse_monitor(app.clone(), Arc::clone(&state), Arc::clone(&starter));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    log::info!("HID mouse monitor is only enabled on Windows and macOS");

    state
}

#[cfg(any(windows, target_os = "macos"))]
fn spawn_hid_mouse_monitor(
    app: AppHandle,
    state: Arc<HidMouseMonitorState>,
    starter: Arc<PlatformHidStarter>,
) {
    thread::spawn(move || {
        let provider = PlatformUsbHidProvider::default();
        let resolver = PlatformManufacturerResolver::with_default_rules();
        let poll_interval = Duration::from_secs(5);
        let mut previous = HidMouseMonitorSnapshot::default();
        let mut has_emitted_snapshot = false;
        let mut started_keys: HashSet<(i32, i32)> = HashSet::new();

        log::info!("Starting HID mouse monitor thread");

        loop {
            let next = match scan_matching_hid_mice(&provider, &resolver) {
                Ok(devices) => HidMouseMonitorSnapshot {
                    matched_devices: devices,
                    last_error: None,
                    updated_at_unix_ms: now_unix_ms(),
                },
                Err(error) => HidMouseMonitorSnapshot {
                    matched_devices: Vec::new(),
                    last_error: Some(error.clone()),
                    updated_at_unix_ms: now_unix_ms(),
                },
            };

            // Open HID readers for newly-appeared devices.
            for dev in &next.matched_devices {
                let key = (dev.vid, dev.pid);
                if started_keys.contains(&key) {
                    continue;
                }
                match starter.hid_startup(dev.vid as u16, dev.pid as u16, dev.manufacturer_id) {
                    Ok(()) => {
                        started_keys.insert(key);
                        log::info!(
                            "HID reader online for VID_{:04X} PID_{:04X} ({})",
                            dev.vid,
                            dev.pid,
                            dev.type_name
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "HID reader failed to open VID_{:04X} PID_{:04X}: {:?}",
                            dev.vid,
                            dev.pid,
                            e
                        );
                    }
                }
            }

            // If all devices vanished, tear down readers so we re-open cleanly later.
            if next.matched_devices.is_empty() && !started_keys.is_empty() {
                starter.stop_all();
                started_keys.clear();
            }

            if !has_emitted_snapshot || next != previous {
                log_snapshot_change(&previous, &next);
                state.replace_snapshot(next.clone());
                let _ = app.emit("hid-mouse-detection-changed", &next);
                previous = next;
                has_emitted_snapshot = true;
            }

            thread::sleep(poll_interval);
        }
    });
}

/// Installs the parser/dispatcher callback on the HID starter. Every HID
/// input report is forwarded into a [`MoserHidStartupHandler`] which, when it
/// recognises a voice-key press/release, triggers Handy's transcribe shortcut.
#[cfg(any(windows, target_os = "macos"))]
fn install_hid_dispatcher(app: AppHandle, starter: Arc<PlatformHidStarter>) {
    // Manufacturer == 1 is the only USB-receiver branch the parser actually
    // executes (the C# code's `Manufacturer == 0` branch is intentionally
    // skipped in the Rust port). Match the rule we declared for the receiver.
    let mut config = HandlerConfig::default();
    config.manufacturer = 1;
    config.mouse_connection_mode = ConnectionMode::Receiver;
    config.button_function_definition = ButtonFunctionDefinition::VoiceTyping;

    let host = HandyMoserHost::new(app);
    let dispatcher = Arc::new(MoserDispatcher::new(config, host));

    let dispatcher_for_cb = Arc::clone(&dispatcher);
    starter.set_data_callback(Arc::new(move |data: &[u8]| {
        dispatcher_for_cb.dispatch(data);
    }));
}

/// Bridges [`MoserHost`] callbacks decoded from HID reports to Handy's
/// transcription coordinator.
///
/// The C# parser fires `mouse_recording_start` / `mouse_recording_stop` from
/// three distinct gestures: the voice-key short toggle `(32,1)`, the voice-key
/// long-press pair `(32,3)/(32,4)`, and the AI-key long-press pair
/// `(35,3)/(35,4)`. They all funnel into the same trait methods, so this
/// adapter treats `start` / `stop` as a desired-state signal and emits a
/// single press pulse against `transcribe` (with `push_to_talk = false`) only
/// when the requested state differs from what the audio recorder is doing.
/// Each press pulse is interpreted by the coordinator as a toggle: idle →
/// recording, recording → idle.
// ── IMA ADPCM step table ─────────────────────────────────────────────────── //
#[cfg(any(windows, target_os = "macos"))]
static IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
#[cfg(any(windows, target_os = "macos"))]
static IMA_INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Decode one IMA ADPCM nibble given the running predictor and step_index.
/// Returns the new (predicted_sample, step_index).
#[cfg(any(windows, target_os = "macos"))]
fn ima_decode_nibble(nibble: u8, predicted: i32, step_index: i32) -> (i32, i32) {
    let step = IMA_STEP_TABLE[step_index as usize];
    let sign = nibble & 0x8;
    let delta = nibble & 0x7;
    let mut diff = step >> 3;
    if delta & 4 != 0 {
        diff += step;
    }
    if delta & 2 != 0 {
        diff += step >> 1;
    }
    if delta & 1 != 0 {
        diff += step >> 2;
    }
    let new_predicted = if sign != 0 {
        (predicted - diff).max(-32768)
    } else {
        (predicted + diff).min(32767)
    };
    let new_index = (step_index + IMA_INDEX_TABLE[nibble as usize]).clamp(0, 88);
    (new_predicted, new_index)
}

#[cfg(any(windows, target_os = "macos"))]
struct HandyMoserHost {
    app: AppHandle,
    /// Tracks the recording intent we last asserted, so a duplicated `start`
    /// (e.g. (32,1) toggle followed by an immediate (32,3) long-press from
    /// some firmware) doesn't accidentally toggle off.
    intended_recording: bool,
    /// Timestamp of the last accepted state change. Used to swallow a
    /// transition that arrives before the audio pipeline has had time to
    /// settle — e.g. some firmwares emit both (32,1) and (32,4) on a release,
    /// which would otherwise stop, then immediately restart the recorder.
    last_transition: Option<Instant>,
    /// Running IMA ADPCM decoder state across HID frames.
    adpcm_predicted: i32,
    adpcm_step_index: i32,
}

#[cfg(any(windows, target_os = "macos"))]
const HANDY_HID_TRANSITION_DEBOUNCE: Duration = Duration::from_millis(300);

/// The AI mouse sends audio at 8 000 Hz mono; Whisper needs 16 000 Hz.
#[cfg(any(windows, target_os = "macos"))]
const HID_AUDIO_SAMPLE_RATE: u32 = 8000;

#[cfg(any(windows, target_os = "macos"))]
impl HandyMoserHost {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            intended_recording: false,
            last_transition: None,
            adpcm_predicted: 0,
            adpcm_step_index: 0,
        }
    }

    fn reset_adpcm(&mut self) {
        self.adpcm_predicted = 0;
        self.adpcm_step_index = 0;
    }

    fn within_debounce(&self) -> bool {
        self.last_transition
            .map(|t| t.elapsed() < HANDY_HID_TRANSITION_DEBOUNCE)
            .unwrap_or(false)
    }

    fn audio_is_recording(&self) -> bool {
        self.app
            .try_state::<std::sync::Arc<crate::managers::audio::AudioRecordingManager>>()
            .map(|m| m.is_recording())
            .unwrap_or(false)
    }

    /// Send a single press pulse on the `transcribe` binding. Force
    /// `push_to_talk = false` so the coordinator treats it as a toggle
    /// regardless of the user's setting — releases coming from the HID
    /// parser are intentionally swallowed.
    fn pulse_transcribe(&self) {
        if let Some(coordinator) = self.app.try_state::<crate::TranscriptionCoordinator>() {
            coordinator.send_input("transcribe", "hid-mouse-voice", true, false);
        } else {
            log::warn!("TranscriptionCoordinator missing — HID voice key dropped");
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl MoserHost for HandyMoserHost {
    type Error = ();

    fn log_debug(&mut self, msg: &str) {
        log::debug!("[moser] {}", msg);
    }

    fn log_error(&mut self, msg: &str) {
        log::error!("[moser] {}", msg);
    }

    fn send_bytes_mouse_recording_start(&mut self) -> Result<(), Self::Error> {
        // Receiver-side "start recording" command bytes are device-specific
        // (the C# version uses them to wake the device's MCU). Handy captures
        // audio through the standard USB mic enumerated by the receiver, so
        // we don't need to send anything back over HID.
        Ok(())
    }

    fn send_bytes_mouse_recording_stop(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn mouse_recording_start(&mut self) -> Result<(), Self::Error> {
        let already_on = self.audio_is_recording() || self.intended_recording;
        if already_on {
            log::debug!("HID voice key: start ignored (already recording)");
            return Ok(());
        }
        if self.within_debounce() {
            log::debug!("HID voice key: start ignored (transition debounce)");
            return Ok(());
        }
        log::info!("HID voice key: starting recording");
        self.intended_recording = true;
        self.last_transition = Some(Instant::now());
        self.reset_adpcm();
        self.pulse_transcribe();
        Ok(())
    }

    fn mouse_recording_stop(&mut self) -> Result<(), Self::Error> {
        let on = self.audio_is_recording() || self.intended_recording;
        if !on {
            log::debug!("HID voice key: stop ignored (not recording)");
            return Ok(());
        }
        if self.within_debounce() {
            log::debug!("HID voice key: stop ignored (transition debounce)");
            return Ok(());
        }
        log::info!("HID voice key: stopping recording");
        self.intended_recording = false;
        self.last_transition = Some(Instant::now());
        self.pulse_transcribe();
        Ok(())
    }

    fn m_key_execute(&mut self) -> Result<(), Self::Error> {
        // No Handy equivalent for the M-key custom actions (browser launch,
        // search, etc. in the original WPF app). Logged for visibility.
        log::debug!("HID m-key long-press (no Handy mapping)");
        Ok(())
    }

    fn m_key_execute_on_click(&mut self) -> Result<(), Self::Error> {
        log::debug!("HID m-key single-click (no Handy mapping)");
        Ok(())
    }

    fn open_main_window(&mut self) -> Result<(), Self::Error> {
        show_main_window_from_mouse(&self.app);
        Ok(())
    }

    fn close_main_window(&mut self) -> Result<(), Self::Error> {
        let app_handle = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            if let Some(main_window) = app_handle.get_webview_window("main") {
                let _ = main_window.hide();
            }

            #[cfg(target_os = "macos")]
            {
                let settings = crate::settings::get_settings(&app_handle);
                let tray_visible =
                    settings.show_tray_icon && !app_handle.state::<crate::CliArgs>().no_tray;
                if tray_visible {
                    let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
        });
        Ok(())
    }

    fn decode_adpcm_to_pcm(&mut self, adpcm_60: &[u8]) -> Result<Vec<u8>, Self::Error> {
        // C# reference: IntelAdpcmDecoder.Decode — headerless streaming ADPCM.
        // 60 bytes × 2 nibbles = 120 i16 samples. State is persistent across
        // frames (adpcm_predicted / adpcm_step_index carry over between calls).
        // Low nibble first, then high nibble — matching the C# implementation.
        let mut out: Vec<u8> = Vec::with_capacity(adpcm_60.len() * 4);

        for &byte in adpcm_60 {
            let lo = byte & 0x0F;
            let hi = (byte >> 4) & 0x0F;
            for nibble in [lo, hi] {
                let (p, idx) =
                    ima_decode_nibble(nibble, self.adpcm_predicted, self.adpcm_step_index);
                self.adpcm_predicted = p;
                self.adpcm_step_index = idx;
                out.extend_from_slice(&(p as i16).to_le_bytes());
            }
        }
        Ok(out)
    }

    fn append_pcm(&mut self, pcm: &[u8]) -> Result<(), Self::Error> {
        if pcm.is_empty() {
            return Ok(());
        }
        // Convert i16 LE → f32. The mouse records at 16 000 Hz mono so no
        // resampling is needed — inject directly at Whisper's native rate.
        let f32_samples: Vec<f32> = pcm
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect();

        if let Some(mgr) = self
            .app
            .try_state::<std::sync::Arc<crate::managers::audio::AudioRecordingManager>>()
        {
            mgr.inject_hid_audio(f32_samples);
        }
        Ok(())
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn show_main_window_from_mouse(app: &AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(main_window) = app_handle.get_webview_window("main") {
            #[cfg(target_os = "macos")]
            {
                let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Regular);
            }
            if let Err(error) = main_window.unminimize() {
                log::error!("Failed to unminimize main window from HID mouse: {}", error);
            }
            if let Err(error) = main_window.show() {
                log::error!("Failed to show main window from HID mouse: {}", error);
            }
            if let Err(error) = main_window.set_focus() {
                log::error!("Failed to focus main window from HID mouse: {}", error);
            }
        }
    });
}

#[cfg(any(windows, target_os = "macos"))]
fn scan_matching_hid_mice<U, R>(provider: &U, resolver: &R) -> Result<Vec<DetectedHidMouse>, String>
where
    U: UsbHidProvider,
    R: ManufacturerResolver,
{
    let hid_ids = provider
        .get_hid_mouse_ids()
        .map_err(|error| error.to_string())?;

    let mut matched_devices = Vec::new();
    let mut seen_device_keys = HashSet::new();

    for hid_id in hid_ids {
        let [vid, pid] = match resolver.get_pid_vid(&hid_id) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let device_type = resolver
            .get_device_type(&hid_id)
            .map_err(|error| error.to_string())?;

        if device_type < 0 {
            continue;
        }

        let mut mouser = Mouser::default();
        resolver
            .populate_usb_manufacturer(pid, vid, &mut mouser)
            .map_err(|error| error.to_string())?;

        let type_name = if mouser.type_name.is_empty() {
            "unknown".to_string()
        } else {
            mouser.type_name
        };

        let device_key = format!(
            "{:04X}:{:04X}:{}:{}:{}",
            vid, pid, device_type, mouser.manufacturer_id, type_name
        );
        if !seen_device_keys.insert(device_key) {
            continue;
        }

        matched_devices.push(DetectedHidMouse {
            hid_id,
            vid,
            pid,
            device_type,
            manufacturer_id: mouser.manufacturer_id,
            type_name,
        });
    }

    Ok(matched_devices)
}

#[cfg(any(windows, target_os = "macos"))]
fn log_snapshot_change(previous: &HidMouseMonitorSnapshot, next: &HidMouseMonitorSnapshot) {
    match (&previous.last_error, &next.last_error) {
        (None, Some(error)) => log::error!("HID mouse monitor failed: {}", error),
        (Some(old), Some(new)) if old != new => log::error!("HID mouse monitor failed: {}", new),
        (Some(_), None) => log::info!("HID mouse monitor recovered"),
        _ => {}
    }

    if previous.matched_devices != next.matched_devices {
        if next.matched_devices.is_empty() {
            log::info!("No matching HID mouse detected");
        } else {
            let summary = next
                .matched_devices
                .iter()
                .map(|device| {
                    format!(
                        "{} (VID_{:04X}, PID_{:04X}, type {})",
                        device.type_name, device.vid, device.pid, device.device_type
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");

            log::info!("Matching HID mouse detected: {}", summary);
        }
    }
}

fn now_unix_ms() -> Option<u128> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}
