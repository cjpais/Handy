mod status;
pub use status::SpeechMikeStatus;

// HID-based sub-modules: macOS and Windows only.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod buttons;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod dispatch;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod hid_reader;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod identify;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod windows_process;

// ── macOS / Windows ──────────────────────────────────────────────────────────

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct SpeechMikeManager {
    status: Arc<Mutex<SpeechMikeStatus>>,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SpeechMikeManager {
    pub fn new(app: &tauri::AppHandle) -> Self {
        let status = Arc::new(Mutex::new(SpeechMikeStatus::disconnected()));
        let status_clone = Arc::clone(&status);
        let app_clone = app.clone();
        std::thread::Builder::new()
            .name("speechmike-hid".to_string())
            .spawn(move || hid_reader::polling_loop(app_clone, status_clone))
            .expect("failed to spawn SpeechMike HID thread");
        Self { status }
    }

    pub fn get_status(&self) -> SpeechMikeStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

// ── Linux stub ───────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub struct SpeechMikeManager;

#[cfg(target_os = "linux")]
impl SpeechMikeManager {
    pub fn new(_app: &tauri::AppHandle) -> Self {
        Self
    }

    pub fn get_status(&self) -> SpeechMikeStatus {
        SpeechMikeStatus::unsupported()
    }
}
