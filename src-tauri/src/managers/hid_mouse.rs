use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

#[cfg(windows)]
use rdev::{listen, Button, Event, EventType};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use aimouse_device_init::models::Mouser;
#[cfg(windows)]
use aimouse_device_init::ports::{ManufacturerResolver, UsbHidProvider};
#[cfg(windows)]
use aimouse_device_init::windows_impl::{WindowsManufacturerResolver, WindowsUsbHidProvider};

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

    fn has_matched_device(&self) -> bool {
        self.snapshot
            .lock()
            .map(|guard| !guard.matched_devices.is_empty())
            .unwrap_or(false)
    }
}

pub fn start_hid_mouse_monitor(app: &AppHandle) -> Arc<HidMouseMonitorState> {
    let state = Arc::new(HidMouseMonitorState::new());

    #[cfg(windows)]
    {
        spawn_windows_hid_mouse_monitor(app.clone(), Arc::clone(&state));
        spawn_windows_mouse_button_listener(app.clone(), Arc::clone(&state));
    }

    #[cfg(not(windows))]
    log::info!("HID mouse monitor is only enabled on Windows");

    state
}

#[cfg(windows)]
fn spawn_windows_hid_mouse_monitor(app: AppHandle, state: Arc<HidMouseMonitorState>) {
    thread::spawn(move || {
        let provider = WindowsUsbHidProvider::default();
        let resolver = WindowsManufacturerResolver::with_default_rules();
        let poll_interval = Duration::from_secs(5);
        let mut previous = HidMouseMonitorSnapshot::default();
        let mut has_emitted_snapshot = false;

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

#[cfg(windows)]
const HID_MOUSE_TRANSCRIBE_BUTTON_CODE: u8 = 1;
#[cfg(windows)]
const HID_MOUSE_POST_PROCESS_BUTTON_CODE: u8 = 2;
#[cfg(windows)]
const HID_MOUSE_SHOW_WINDOW_BUTTON_CODE: u8 = 3;

#[cfg(windows)]
fn spawn_windows_mouse_button_listener(app: AppHandle, state: Arc<HidMouseMonitorState>) {
    thread::spawn(move || {
        log::info!("Starting HID mouse button listener");

        loop {
            let app_clone = app.clone();
            let state_clone = Arc::clone(&state);

            let result = listen(move |event| {
                handle_windows_mouse_event(&app_clone, &state_clone, event);
            });

            match result {
                Ok(()) => {
                    log::warn!("HID mouse button listener stopped unexpectedly");
                    break;
                }
                Err(error) => {
                    log::error!("HID mouse button listener failed: {:?}", error);
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
}

#[cfg(windows)]
fn handle_windows_mouse_event(app: &AppHandle, state: &Arc<HidMouseMonitorState>, event: Event) {
    let (button, is_pressed) = match event.event_type {
        EventType::ButtonPress(button) => (button, true),
        EventType::ButtonRelease(button) => (button, false),
        _ => return,
    };

    if !state.has_matched_device() {
        return;
    }

    match button {
        Button::Unknown(HID_MOUSE_TRANSCRIBE_BUTTON_CODE) => {
            crate::shortcut::handle_shortcut_event(app, "transcribe", "hid-mouse-x1", is_pressed);
        }
        Button::Unknown(HID_MOUSE_POST_PROCESS_BUTTON_CODE) => {
            crate::shortcut::handle_shortcut_event(
                app,
                "transcribe_with_post_process",
                "hid-mouse-x2",
                is_pressed,
            );
        }
        Button::Unknown(HID_MOUSE_SHOW_WINDOW_BUTTON_CODE) if is_pressed => {
            show_main_window_from_mouse(app);
        }
        Button::Unknown(code) if is_pressed => {
            log::debug!("Ignoring unmatched HID mouse button code: {}", code);
        }
        _ => {}
    }
}

#[cfg(windows)]
fn show_main_window_from_mouse(app: &AppHandle) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(main_window) = app_handle.get_webview_window("main") {
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

#[cfg(windows)]
fn scan_matching_hid_mice<U, R>(provider: &U, resolver: &R) -> Result<Vec<DetectedHidMouse>, String>
where
    U: UsbHidProvider,
    R: ManufacturerResolver,
{
    let hid_ids = provider
        .get_hid_mouse_ids()
        .map_err(|error| error.to_string())?;

    let mut matched_devices = Vec::new();

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

        matched_devices.push(DetectedHidMouse {
            hid_id,
            vid,
            pid,
            device_type,
            manufacturer_id: mouser.manufacturer_id,
            type_name: if mouser.type_name.is_empty() {
                "unknown".to_string()
            } else {
                mouser.type_name
            },
        });
    }

    Ok(matched_devices)
}

#[cfg(windows)]
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
