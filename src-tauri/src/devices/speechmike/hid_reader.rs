use hidapi::HidApi;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use super::buttons::parse_button_event;
use super::dispatch::dispatch_button_event;
use super::identify::{find_matching_audio_device, is_philips_speechmike};
use super::status::SpeechMikeStatus;
use crate::managers::audio::AudioRecordingManager;
use crate::settings::{get_settings, write_settings};

/// Payload for the `speechmike://raw-report` debug event.
#[derive(serde::Serialize, Clone)]
struct RawHidReport {
    hex: String,
    length: usize,
    vendor_id: u16,
    product_id: u16,
}

/// Entry point for the background HID polling thread.
/// Runs forever; panics are caught by the thread name for easier debugging.
pub fn polling_loop(app: AppHandle, status: Arc<Mutex<SpeechMikeStatus>>) {
    loop {
        poll_cycle(&app, &status);
    }
}

fn poll_cycle(app: &AppHandle, status: &Arc<Mutex<SpeechMikeStatus>>) {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => {
            log::error!("SpeechMike: HidApi::new() failed: {}", e);
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
    };

    // Search for any Philips device.
    let Some(info) = api
        .device_list()
        .find(|d| is_philips_speechmike(d.vendor_id()))
    else {
        // No device found – clear connected state if it was set.
        let was_connected = {
            let mut s = lock_status(status);
            let was = s.connected;
            if was {
                s.connected = false;
                s.blocked_by_other_app = false;
                s.device_name = None;
                s.vendor_id = None;
                s.product_id = None;
                s.buttons_enabled = false;
            }
            was
        };
        if was_connected {
            log::info!("SpeechMike disconnected");
            let _ = app.emit("speechmike://disconnected", ());
        }
        std::thread::sleep(Duration::from_millis(500));
        return;
    };

    let vendor_id = info.vendor_id();
    let product_id = info.product_id();
    let product_name = info
        .product_string()
        .unwrap_or("Philips SpeechMike")
        .to_string();
    let serial = info.serial_number().map(|s| s.to_string());

    match info.open_device(&api) {
        Ok(device) => {
            let audio_name = find_matching_audio_device(&product_name);

            {
                let mut s = lock_status(status);
                s.connected = true;
                s.blocked_by_other_app = false;
                s.device_name = Some(product_name.clone());
                s.vendor_id = Some(vendor_id);
                s.product_id = Some(product_id);
                s.serial_number = serial;
                s.audio_device_name = audio_name.clone();
                s.buttons_enabled = true;
                s.detected_blocking_processes = vec![];
                s.last_error = None;
            }

            log::info!(
                "SpeechMike connected: {} (VID={:#06x} PID={:#06x})",
                product_name,
                vendor_id,
                product_id
            );

            let snapshot = lock_status(status).clone();
            let _ = app.emit("speechmike://connected", snapshot);

            maybe_auto_select_microphone(app, &audio_name);

            // Inner read loop – exits on disconnect or read error.
            let mut buf = [0u8; 64];
            loop {
                match device.read_timeout(&mut buf, 50) {
                    Ok(0) => {} // timeout, keep polling
                    Ok(n) => {
                        let raw = buf[..n].to_vec();
                        let settings = get_settings(app);

                        if settings.livesttt_raw_hid_debug {
                            let hex = raw
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            let _ = app.emit(
                                "speechmike://raw-report",
                                RawHidReport {
                                    hex,
                                    length: n,
                                    vendor_id,
                                    product_id,
                                },
                            );
                        }

                        if settings.speechmike_button_mapping_enabled {
                            if let Some(event) = parse_button_event(&raw) {
                                dispatch_button_event(app, event);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("SpeechMike read error (device removed?): {}", e);
                        break;
                    }
                }
            }

            // Mark disconnected after the inner loop exits.
            {
                let mut s = lock_status(status);
                s.connected = false;
                s.device_name = None;
                s.vendor_id = None;
                s.product_id = None;
                s.serial_number = None;
                s.audio_device_name = None;
                s.buttons_enabled = false;
            }
            log::info!("SpeechMike removed");
            let _ = app.emit("speechmike://disconnected", ());
        }
        Err(open_err) => {
            let err_lower = open_err.to_string().to_lowercase();
            let is_blocked = err_lower.contains("access")
                || err_lower.contains("denied")
                || err_lower.contains("permission")
                || err_lower.contains("sharing")
                || err_lower.contains("busy");

            if is_blocked {
                log::warn!("SpeechMike HID channel blocked: {}", open_err);
                let processes = scan_blocking_processes();
                {
                    let mut s = lock_status(status);
                    s.blocked_by_other_app = true;
                    s.detected_blocking_processes = processes;
                    s.last_error = Some(open_err.to_string());
                }
                let snapshot = lock_status(status).clone();
                let _ = app.emit("speechmike://blocked-by-other-app", snapshot);
            } else {
                log::debug!("SpeechMike open attempt failed: {}", open_err);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Auto-select the SpeechMike audio device if the user hasn't manually chosen one.
fn maybe_auto_select_microphone(app: &AppHandle, audio_name: &Option<String>) {
    let Some(name) = audio_name else {
        return;
    };

    let mut settings = get_settings(app);
    if !settings.speechmike_auto_select || settings.selected_microphone_user_overridden {
        return;
    }

    settings.selected_microphone = Some(name.clone());
    settings.speechmike_last_seen_name = Some(name.clone());
    write_settings(app, settings);

    if let Some(rm) = app.try_state::<Arc<AudioRecordingManager>>() {
        if let Err(e) = rm.update_selected_device() {
            log::error!(
                "SpeechMike auto-select: failed to switch audio device: {}",
                e
            );
        }
    }

    log::info!("SpeechMike auto-selected audio device: {}", name);
}

#[cfg(target_os = "windows")]
fn scan_blocking_processes() -> Vec<String> {
    super::windows_process::scan()
}

#[cfg(not(target_os = "windows"))]
fn scan_blocking_processes() -> Vec<String> {
    vec![]
}

fn lock_status(
    status: &Arc<Mutex<SpeechMikeStatus>>,
) -> std::sync::MutexGuard<'_, SpeechMikeStatus> {
    status.lock().unwrap_or_else(|e| e.into_inner())
}
