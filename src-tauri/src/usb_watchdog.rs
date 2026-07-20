//! USB hub power-cycle watchdog for recovering dead USB audio devices
//!
//! When Handy fails to open the microphone stream (device not found, zombie
//! device, etc.), this module can automatically power-cycle the USB hub port
//! via `uhubctl` and then retry the stream open.
//!
//! The user selects a device by *name* (e.g. "RØDE Microphones RØDE VideoMic NTG").
//! At cycle time, we re-run `uhubctl` to resolve the device name to a
//! specific hub location and port number, then cycle that port.
//!
//! Ported from the Hammerspoon Rode watchdog script at:
//!   ~/.hammerspoon/init.lua
//!
//! Robustness fix: Names are normalized (alphanumeric only) before comparison
//! to handle encoding issues (e.g. RØDE vs R?DE).

use crate::managers::audio::AudioRecordingManager;
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;

/// How long to poll for the device to re-appear after cycling power.
/// The RØDE VideoMic NTG typically comes back in 2-3s over USB.
/// We poll every 250ms and bail out early once the device is seen.
const POWER_CYCLE_SETTLE_SECS: u64 = 5;

/// How often to poll for the device to re-appear (in ms)
const POWER_CYCLE_POLL_INTERVAL_MS: u64 = 250;

/// Minimum seconds between two automatic power cycles (cooldown)
const RESET_COOLDOWN_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct UsbCycleStage {
    pub stage: String,
    pub message: String,
}

/// A USB device discovered by `uhubctl`.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct UsbDevice {
    /// Human-readable device name (e.g. "RØDE Microphones RØDE VideoMic NTG 762210B9")
    pub name: String,
    /// Hub location ID (e.g. "8-3")
    pub hub: String,
    /// Port number on the hub (e.g. "1")
    pub port: String,
}

/// Internal state for the watchdog (shared behind Arc)
pub struct UsbWatchdog {
    /// Whether the watchdog is enabled
    enabled: AtomicBool,
    /// Device name to watch for (set by user, resolved to hub/port at cycle time)
    device_name: Mutex<String>,
    /// A cycle is currently in progress (shared Arc so spawned threads can clear it)
    cycling: Arc<AtomicBool>,
    /// Epoch seconds of last completed cycle (for cooldown)
    last_cycle_epoch: AtomicU64,
    /// Number of consecutive mic-open failures since last successful open
    consecutive_failures: AtomicU64,
    /// After how many consecutive failures to trigger a cycle (default 2)
    fail_threshold: AtomicU64,
    /// Whether we're in a grace period after a successful USB cycle
    /// During this time, we're more lenient about counting failures
    /// (shared Arc so spawned threads can set it)
    post_cycle_grace: Arc<AtomicBool>,
    /// AppHandle for emitting events to the frontend during power cycling
    app_handle: Option<tauri::AppHandle>,
}

impl UsbWatchdog {
    pub fn new(enabled: bool, device_name: &str, app_handle: Option<tauri::AppHandle>) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            device_name: Mutex::new(device_name.to_string()),
            cycling: Arc::new(AtomicBool::new(false)),
            last_cycle_epoch: AtomicU64::new(0),
            consecutive_failures: AtomicU64::new(0),
            fail_threshold: AtomicU64::new(2), // Require 2 consecutive failures before cycling
            post_cycle_grace: Arc::new(AtomicBool::new(false)),
            app_handle,
        }
    }

    /// Update configuration at runtime
    pub fn update_config(&self, enabled: bool, device_name: String) {
        self.enabled.store(enabled, Ordering::SeqCst);
        *self.device_name.lock() = device_name;
        debug!("USB watchdog config updated: enabled={}", enabled);
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Returns `true` if a power cycle is currently in progress.
    #[allow(dead_code)]
    pub fn is_cycling(&self) -> bool {
        self.cycling.load(Ordering::SeqCst)
    }

    /// Called when the mic stream fails to open. Returns `true` if a
    /// power cycle was completed (caller should retry).
    pub fn on_mic_open_failed(&self) -> bool {
        if !self.enabled.load(Ordering::SeqCst) {
            debug!("USB watchdog disabled, skipping auto-cycle");
            return false;
        }

        if self.cycling.load(Ordering::SeqCst) {
            debug!("USB cycle already in progress, skipping");
            return false;
        }

        // During grace period after a USB cycle, be more lenient
        // Don't count failures for a short time after cycling
        if self.post_cycle_grace.load(Ordering::SeqCst) {
            debug!("USB watchdog: in post-cycle grace period, not counting failure");
            return false;
        }

        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        let threshold = self.fail_threshold.load(Ordering::SeqCst);
        debug!(
            "USB watchdog: mic open failure #{} (threshold: {})",
            failures, threshold
        );

        if failures < threshold {
            debug!("Failure count below threshold, not cycling yet");
            return false;
        }

        self.power_cycle_blocking()
    }

    /// Called when the mic stream opens successfully (resets failure counter)
    pub fn on_mic_open_succeeded(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::SeqCst);
        if prev > 0 {
            debug!(
                "USB watchdog: mic opened successfully, reset failures (was {})",
                prev
            );
        }
        // Clear the grace period - we have a working mic now
        self.post_cycle_grace.store(false, Ordering::SeqCst);
    }

    /// Called to report whether the microphone stream is currently alive.
    pub fn on_stream_alive_check(&self, alive: bool) {
        if alive {
            self.on_mic_open_succeeded();
        } else {
            debug!("USB watchdog: stream reported dead during liveness check");
        }
    }

    /// Called when a recording finishes. If the sample count is zero, it
    /// counts as a failure and may trigger a power cycle.
    ///
    /// Parameters:
    /// - `sample_count`: Number of audio samples captured
    /// - `duration_secs`: Duration of the recording in seconds
    ///
    /// Returns true if a USB power cycle was triggered.
    pub fn on_recording_finished(&self, sample_count: usize, _duration_secs: f32) -> bool {
        if sample_count > 0 {
            self.on_mic_open_succeeded();
            return false;
        }

        // Zero samples always indicates a problem, regardless of duration
        warn!("USB watchdog: recording finished with 0 samples - treating as dead stream");
        self.on_mic_open_failed()
    }

    /// Called when a transcription returns empty text despite having audio samples.
    /// This may indicate the microphone is capturing silence/noise instead of actual audio.
    ///
    /// Parameters:
    /// - `duration_secs`: Duration of the recording in seconds. Short recordings (< 10s)
    ///   are less likely to indicate a dead mic (user may have stopped early).
    ///
    /// Returns true if a USB power cycle was triggered.
    pub fn on_silent_transcription(&self, duration_secs: f32) -> bool {
        if !self.enabled.load(Ordering::SeqCst) {
            debug!("USB watchdog disabled, skipping silent transcription handler");
            return false;
        }

        if self.cycling.load(Ordering::SeqCst) {
            debug!("USB cycle already in progress, skipping silent transcription handler");
            return false;
        }

        // During grace period after a USB cycle, be more lenient
        if self.post_cycle_grace.load(Ordering::SeqCst) {
            debug!("USB watchdog: in post-cycle grace period, not counting silent transcription as failure");
            return false;
        }

        // Short recordings (< 10 seconds) are less likely to indicate a dead mic.
        // The user may have started recording and stopped quickly without speaking.
        // Only count as a failure if the recording was at least 10 seconds.
        const MIN_DURATION_SECS: f32 = 10.0;
        if duration_secs < MIN_DURATION_SECS {
            debug!(
                "USB watchdog: silent transcription ignored (duration {:.1}s < {:.1}s threshold)",
                duration_secs, MIN_DURATION_SECS
            );
            return false;
        }

        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        let threshold = self.fail_threshold.load(Ordering::SeqCst);
        info!(
            "USB watchdog: silent transcription detected (failure #{}, threshold: {}, duration: {:.1}s)",
            failures, threshold, duration_secs
        );

        if failures < threshold {
            debug!("Failure count below threshold, not cycling yet");
            return false;
        }

        warn!("USB watchdog: {} consecutive failures (silent transcription detected), triggering USB power cycle", failures);
        self.power_cycle_blocking()
    }

    /// Called when a recording had very low audio levels (near silence).
    /// This indicates the microphone may be dead or muted.
    ///
    /// Parameters:
    /// - `duration_secs`: Duration of the recording in seconds. Short recordings (< 10s)
    ///   are less likely to indicate a dead mic (user may have stopped early).
    ///
    /// Returns true if a USB power cycle was triggered.
    pub fn on_low_audio_level(&self, duration_secs: f32) -> bool {
        if !self.enabled.load(Ordering::SeqCst) {
            debug!("USB watchdog disabled, skipping low audio level handler");
            return false;
        }

        if self.cycling.load(Ordering::SeqCst) {
            debug!("USB cycle already in progress, skipping low audio level handler");
            return false;
        }

        // During grace period after a USB cycle, be more lenient
        if self.post_cycle_grace.load(Ordering::SeqCst) {
            debug!(
                "USB watchdog: in post-cycle grace period, not counting low audio level as failure"
            );
            return false;
        }

        // Short recordings (< 10 seconds) are less likely to indicate a dead mic.
        // The user may have started recording and stopped quickly without speaking.
        // Only count as a failure if the recording was at least 10 seconds.
        const MIN_DURATION_SECS: f32 = 10.0;
        if duration_secs < MIN_DURATION_SECS {
            debug!(
                "USB watchdog: low audio level ignored (duration {:.1}s < {:.1}s threshold)",
                duration_secs, MIN_DURATION_SECS
            );
            return false;
        }

        warn!(
            "USB watchdog: recording had very low audio level (duration: {:.1}s) - treating as potential dead mic",
            duration_secs
        );
        self.on_mic_open_failed()
    }

    /// Attempt a USB hub port power cycle **synchronously** (blocking).
    fn power_cycle_blocking(&self) -> bool {
        // Check cooldown
        let now_epoch = epoch_secs();
        let last = self.last_cycle_epoch.load(Ordering::SeqCst);
        if now_epoch > last && (now_epoch - last) < RESET_COOLDOWN_SECS {
            let remaining = RESET_COOLDOWN_SECS - (now_epoch - last);
            debug!("USB watchdog: cooldown active, {}s remaining", remaining);
            return false;
        }

        if self.cycling.swap(true, Ordering::SeqCst) {
            debug!("USB watchdog: cycle already in progress");
            return false;
        }

        let device_name = self.device_name.lock().clone();
        if device_name.is_empty() {
            warn!("USB watchdog: device name not configured, skipping");
            self.cycling.store(false, Ordering::SeqCst);
            return false;
        }

        self.emit_cycle_event("usb-power-cycle-started", &device_name);
        self.emit_stage_event("resolving", "Locating USB device...");

        let device = match resolve_device(&device_name) {
            Some(d) => d,
            None => {
                warn!(
                    "USB watchdog: device '{}' not found in USB tree, cannot cycle",
                    device_name
                );
                self.cycling.store(false, Ordering::SeqCst);
                self.emit_cycle_event("usb-power-cycle-failed", "device not found in USB tree");
                return false;
            }
        };

        self.last_cycle_epoch.store(now_epoch, Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);

        info!(
            "USB watchdog: power cycling device '{}' (hub {} port {})",
            device_name, device.hub, device.port
        );

        self.emit_stage_event("cycling", "Power cycling port...");

        let app_handle = self.app_handle.clone();
        let mut cycle_succeeded = false;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let start = Instant::now();
            match run_uhubctl_cycle(&device.hub, &device.port) {
                Ok(()) => {
                    info!(
                        "USB watchdog: uhubctl cycle completed for '{}' in {:?}",
                        device_name,
                        start.elapsed()
                    );
                    emit_stage_event_with_handle(&app_handle, "waiting", "Waiting for device...");
                    let settle_start = Instant::now();
                    let settle_max = Duration::from_secs(POWER_CYCLE_SETTLE_SECS);
                    let poll_interval = Duration::from_millis(POWER_CYCLE_POLL_INTERVAL_MS);
                    loop {
                        if resolve_device(&device_name).is_some() {
                            info!(
                                "USB watchdog: device '{}' re-appeared after {:?}",
                                device_name,
                                settle_start.elapsed()
                            );
                            std::thread::sleep(Duration::from_millis(300));
                            emit_stage_event_with_handle(
                                &app_handle,
                                "recovered",
                                "Device recovered!",
                            );
                            cycle_succeeded = true;
                            break;
                        }
                        if settle_start.elapsed() >= settle_max {
                            warn!(
                                "USB watchdog: device '{}' did not re-appear within {}s",
                                device_name, POWER_CYCLE_SETTLE_SECS
                            );
                            break;
                        }
                        std::thread::sleep(poll_interval);
                    }
                }
                Err(e) => {
                    error!("USB watchdog: uhubctl failed: {}", e);
                    emit_cycle_event_with_handle(
                        &app_handle,
                        "usb-power-cycle-failed",
                        &format!("uhubctl failed: {}", e),
                    );
                }
            }
        }));

        if let Err(_) = result {
            error!("USB watchdog: power_cycle_blocking panicked");
            self.emit_cycle_event("usb-power-cycle-failed", "power cycle panicked");
        }

        self.cycling.store(false, Ordering::SeqCst);

        if cycle_succeeded {
            // Set a grace period after successful USB cycle to prevent
            // false positives from the mic still recovering
            self.post_cycle_grace.store(true, Ordering::SeqCst);
            self.emit_cycle_event("usb-power-cycle-finished", &device_name);
            info!("USB watchdog: grace period started after successful cycle");
            // Grace period will be cleared when mic opens successfully (on_mic_open_succeeded)
            true
        } else {
            false
        }
    }

    pub fn force_power_cycle(&self) -> bool {
        if self.cycling.swap(true, Ordering::SeqCst) {
            debug!("USB watchdog: cycle already in progress");
            return false;
        }

        let device_name = self.device_name.lock().clone();
        if device_name.is_empty() {
            warn!("USB watchdog: device name not configured");
            self.cycling.store(false, Ordering::SeqCst);
            return false;
        }

        self.emit_cycle_event("usb-power-cycle-started", &device_name);
        self.emit_stage_event("resolving", "Locating USB device...");

        let device = match resolve_device(&device_name) {
            Some(d) => d,
            None => {
                warn!(
                    "USB watchdog: device '{}' not found for forced cycle",
                    device_name
                );
                self.cycling.store(false, Ordering::SeqCst);
                self.emit_cycle_event(
                    "usb-power-cycle-failed",
                    "device not found for forced cycle",
                );
                return false;
            }
        };

        self.last_cycle_epoch.store(epoch_secs(), Ordering::SeqCst);
        self.consecutive_failures.store(0, Ordering::SeqCst);

        info!(
            "USB watchdog: FORCE power cycling device '{}' (hub {} port {})",
            device_name, device.hub, device.port
        );

        self.emit_stage_event("cycling", "Power cycling port...");

        let hub = device.hub.clone();
        let port = device.port.clone();
        let name = device_name.clone();
        let cycling = self.cycling.clone();
        let post_cycle_grace = self.post_cycle_grace.clone();
        let app_handle = self.app_handle.clone();

        std::thread::spawn(move || {
            let mut cycle_succeeded = false;

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let start = Instant::now();
                match run_uhubctl_cycle(&hub, &port) {
                    Ok(()) => {
                        info!(
                            "USB watchdog: forced uhubctl cycle completed for '{}' in {:?}",
                            name,
                            start.elapsed()
                        );
                        emit_stage_event_with_handle(
                            &app_handle,
                            "waiting",
                            "Waiting for device...",
                        );
                        let settle_start = Instant::now();
                        let settle_max = Duration::from_secs(POWER_CYCLE_SETTLE_SECS);
                        let poll_interval = Duration::from_millis(POWER_CYCLE_POLL_INTERVAL_MS);
                        loop {
                            if resolve_device(&name).is_some() {
                                info!(
                                    "USB watchdog: device '{}' re-appeared after {:?} (forced cycle)",
                                    name,
                                    settle_start.elapsed()
                                );
                                std::thread::sleep(Duration::from_millis(300));
                                emit_stage_event_with_handle(
                                    &app_handle,
                                    "recovered",
                                    "Device recovered!",
                                );
                                cycle_succeeded = true;
                                break;
                            }
                            if settle_start.elapsed() >= settle_max {
                                warn!(
                                    "USB watchdog: device '{}' did not re-appear within {}s",
                                    name, POWER_CYCLE_SETTLE_SECS
                                );
                                break;
                            }
                            std::thread::sleep(poll_interval);
                        }
                    }
                    Err(e) => {
                        error!("USB watchdog: forced uhubctl failed: {}", e);
                        emit_cycle_event_with_handle(
                            &app_handle,
                            "usb-power-cycle-failed",
                            &format!("uhubctl failed: {}", e),
                        );
                    }
                }
            }));

            if let Err(_) = result {
                error!("USB watchdog: force_power_cycle thread panicked");
                emit_cycle_event_with_handle(
                    &app_handle,
                    "usb-power-cycle-failed",
                    "forced power cycle panicked",
                );
            }

            cycling.store(false, Ordering::SeqCst);

            if cycle_succeeded {
                // Set grace period after successful forced cycle
                post_cycle_grace.store(true, Ordering::SeqCst);
                emit_cycle_event_with_handle(&app_handle, "usb-power-cycle-finished", &name);
                info!("USB watchdog: grace period started after successful forced cycle");

                if let Some(ah) = &app_handle {
                    if let Some(rm) = ah.try_state::<Arc<AudioRecordingManager>>() {
                        if let Err(e) = rm.restart_microphone_if_needed() {
                            error!("Failed to restart microphone after forced USB cycle: {}", e);
                        }
                    }
                }
            }
        });

        true
    }

    fn emit_stage_event(&self, stage: &str, message: &str) {
        emit_stage_event_with_handle(&self.app_handle, stage, message);
    }

    fn emit_cycle_event(&self, event_name: &str, message: &str) {
        emit_cycle_event_with_handle(&self.app_handle, event_name, message);
    }
}

pub fn emit_cycle_event_with_handle(
    app_handle: &Option<tauri::AppHandle>,
    event_name: &str,
    message: &str,
) {
    if let Some(ah) = app_handle {
        use tauri::Emitter;
        let _ = ah.emit(event_name, message.to_string());
        if let Some(overlay) = ah.get_webview_window("recording_overlay") {
            let _ = overlay.emit(event_name, message.to_string());
        }
    }
}

pub fn emit_stage_event_with_handle(
    app_handle: &Option<tauri::AppHandle>,
    stage: &str,
    message: &str,
) {
    if let Some(ah) = app_handle {
        use tauri::Emitter;
        let payload = UsbCycleStage {
            stage: stage.to_string(),
            message: message.to_string(),
        };
        let _ = ah.emit("usb-power-cycle-stage", payload.clone());
        if let Some(overlay) = ah.get_webview_window("recording_overlay") {
            let _ = overlay.emit("usb-power-cycle-stage", payload);
        }
    }
}

/// Resolve a device name substring to a UsbDevice by running `uhubctl`.
fn resolve_device(name: &str) -> Option<UsbDevice> {
    let devices = list_usb_devices_inner();

    // Helper to normalize strings for comparison (case-insensitive, alphanumeric only)
    let to_fuzzy = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    let fuzzy_target = to_fuzzy(name);
    if fuzzy_target.is_empty() {
        return None;
    }

    let found = devices.into_iter().find(|d| {
        let fuzzy_device = to_fuzzy(&d.name);
        !fuzzy_device.is_empty()
            && (fuzzy_device.contains(&fuzzy_target) || fuzzy_target.contains(&fuzzy_device))
    });

    if let Some(ref d) = found {
        debug!(
            "USB watchdog: resolved '{}' fuzzy matching to '{}' (hub {} port {})",
            name, d.name, d.hub, d.port
        );
    }

    found
}

pub fn list_usb_devices() -> Vec<UsbDevice> {
    list_usb_devices_inner()
}

fn list_usb_devices_inner() -> Vec<UsbDevice> {
    let bin = match uhubctl_bin() {
        Some(b) => b,
        None => return Vec::new(),
    };

    let output = match std::process::Command::new(&bin)
        .env(
            "PATH",
            "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return Vec::new(),
    };

    parse_uhubctl_output(&output)
}

fn parse_uhubctl_output(output: &str) -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let mut current_hub: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Current status for hub ") {
            if let Some(hub_id) = rest.split_whitespace().next() {
                current_hub = Some(hub_id.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Port ") {
            if let Some(colon_pos) = rest.find(':') {
                let port_str = rest[..colon_pos].trim();
                if !rest.contains("connect") {
                    continue;
                }
                if let Some(desc) = extract_device_description(rest) {
                    if let Some(ref hub) = current_hub {
                        devices.push(UsbDevice {
                            name: desc,
                            hub: hub.clone(),
                            port: port_str.to_string(),
                        });
                    }
                }
            }
        }
    }
    devices
}

fn extract_device_description(rest: &str) -> Option<String> {
    let start = rest.find('[')?;
    let end = rest.rfind(']')?;
    let bracket_content = &rest[start + 1..end];
    let mut parts = bracket_content.splitn(2, ' ');
    parts.next(); // skip vid:pid
    parts.next().map(|s| s.to_string())
}

const UHUBCTL_PATHS: &[&str] = &["/usr/local/bin/uhubctl", "/opt/homebrew/bin/uhubctl"];

fn uhubctl_bin() -> Option<std::path::PathBuf> {
    // First check common Homebrew paths
    for path in UHUBCTL_PATHS {
        if std::path::Path::new(path).exists() {
            return Some(std::path::PathBuf::from(*path));
        }
    }
    // Fall back to `which uhubctl` which finds it wherever it's installed
    which_uhubctl()
}

fn which_uhubctl() -> Option<std::path::PathBuf> {
    std::process::Command::new("which")
        .arg("uhubctl")
        .env(
            "PATH",
            "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

const UHUBCTL_TIMEOUT_SECS: u64 = 5;

fn run_uhubctl_cycle(hub_id: &str, port: &str) -> Result<(), String> {
    let bin = uhubctl_bin().ok_or_else(|| "uhubctl not found on system".to_string())?;
    let mut child = std::process::Command::new(&bin)
        .args(["-l", hub_id, "-p", port, "-a", "cycle", "-d", "3"])
        .env(
            "PATH",
            "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn uhubctl: {}", e))?;

    let timeout = Duration::from_secs(UHUBCTL_TIMEOUT_SECS);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    let _ = child.wait();
                    return Ok(());
                }
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "killed by signal".to_string());
                return Err(format!("uhubctl exited with {}", code));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("uhubctl timed out after {}s", UHUBCTL_TIMEOUT_SECS));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(format!("uhubctl wait error: {}", e)),
        }
    }
}

pub fn is_uhubctl_available() -> bool {
    uhubctl_bin().is_some()
}

pub fn ensure_uhubctl_installed() -> bool {
    if is_uhubctl_available() {
        info!("uhubctl found — USB watchdog ready");
        return true;
    }
    info!("uhubctl not found, attempting to install via Homebrew…");
    let brew_check = std::process::Command::new("which").arg("brew").output();
    match brew_check {
        Ok(output) if output.status.success() => {
            match std::process::Command::new("brew")
                .args(["install", "uhubctl"])
                .env(
                    "PATH",
                    "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                )
                .output()
            {
                Ok(output) => {
                    if output.status.success() {
                        info!("uhubctl installed successfully via Homebrew");
                        is_uhubctl_available()
                    } else {
                        warn!("brew install uhubctl failed");
                        false
                    }
                }
                Err(_) => false,
            }
        }
        _ => false,
    }
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
