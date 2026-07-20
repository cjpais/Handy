//! macOS sleep/wake detection via wall-clock monitoring
//!
//! Detects when macOS wakes from sleep by monitoring how long a thread-sleep
//! actually takes in wall-clock time. If a 3-second sleep takes more than
//! 10 seconds of wall-clock time, the system was likely suspended.
//!
//! This is the most reliable cross-platform-friendly way to detect wake
//! without fragile Objective-C runtime FFI.

#[cfg(target_os = "macos")]
use log::{debug, error, info, warn};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use tauri::Manager;
#[cfg(target_os = "macos")]

/// Start listening for macOS wake-from-sleep events.
#[cfg(target_os = "macos")]
pub fn start_sleep_wake_listener(app_handle: tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};

    static LISTENER_STARTED: AtomicBool = AtomicBool::new(false);

    if LISTENER_STARTED.swap(true, Ordering::SeqCst) {
        debug!("Sleep/wake listener already started, skipping");
        return;
    }

    info!("Starting robust sleep/wake detector (polling every 3s)");

    let app_handle = Arc::new(app_handle);

    std::thread::spawn(move || {
        use std::time::{Duration, Instant, SystemTime};

        let sleep_duration = Duration::from_secs(3);
        let wake_threshold = Duration::from_secs(10);

        loop {
            let before_wall = SystemTime::now();
            let before_mono = Instant::now();

            std::thread::sleep(sleep_duration);

            let after_wall = SystemTime::now();
            let after_mono = Instant::now();

            let wall_elapsed = after_wall
                .duration_since(before_wall)
                .unwrap_or(sleep_duration);
            let mono_elapsed = after_mono.duration_since(before_mono);

            // If the 3s sleep took more than 10s of wall-clock time,
            // the system was almost certainly sleeping.
            if wall_elapsed > wake_threshold {
                info!(
                    "Detected macOS wake from sleep: 3s sleep took {}s wall-clock time (monotonic: {}s)",
                    wall_elapsed.as_secs(),
                    mono_elapsed.as_secs()
                );
                on_system_wake(&app_handle);
            } else {
                // Heartbeat to logs at DEBUG level
                debug!(
                    "Sleep/wake heartbeat: 3s sleep took {}s wall-clock (mono: {}s)",
                    wall_elapsed.as_secs(),
                    mono_elapsed.as_secs()
                );
            }
        }
    });
}

/// Handle macOS wake-from-sleep event.
#[cfg(target_os = "macos")]
fn on_system_wake(app_handle: &Arc<tauri::AppHandle>) {
    info!("Handling system wake event...");

    // Reset hotkey state first — this is independent of USB watchdog settings
    // and should happen immediately so shortcuts work right after wake.
    crate::shortcut::handy_keys::reset_hotkey_state_after_wake(app_handle);

    // USB watchdog recovery
    let settings = crate::settings::get_settings_safe(app_handle);
    let cycle_on_wake = settings.usb_watchdog_enabled && settings.usb_watchdog_cycle_on_wake;

    if !cycle_on_wake {
        // Log clearly why recovery is skipped
        if !settings.usb_watchdog_enabled {
            info!("Wake recovery skipped: USB watchdog is disabled (usb_watchdog_enabled=false)");
        } else {
            info!("Wake recovery skipped: cycle-on-wake is disabled (usb_watchdog_cycle_on_wake=false)");
        }
        return;
    }

    let device_name = settings.usb_watchdog_device_name.clone();
    if device_name.is_empty() {
        warn!("Wake recovery skipped: USB watchdog device name not configured (empty setting)");
        return;
    }

    info!(
        "Triggering post-wake recovery for device '{}' (will check stream health then cycle USB if needed)",
        device_name
    );

    let ah = app_handle.clone();
    std::thread::spawn(move || {
        // Reduced from 5s to 2s - macOS USB re-enumeration is fast on most systems.
        // If the device isn't ready in 2s, the watchdog power cycle will handle it.
        std::thread::sleep(std::time::Duration::from_secs(2));

        let recording_manager =
            ah.try_state::<Arc<crate::managers::audio::AudioRecordingManager>>();

        match recording_manager {
            Some(rm) => {
                // Check stream health using the public method
                let (is_open, is_alive) = rm.check_stream_health();

                if !is_open {
                    info!("Wake recovery: stream not open, will open on next recording");
                    // Stream not open (OnDemand mode or closed), skip cycle
                    // The stream will be opened fresh on next recording attempt
                    return;
                }

                if is_alive {
                    info!("Wake recovery: stream is alive, no USB cycle needed");
                    return;
                }

                warn!("Wake recovery: stream is dead (zombie), triggering USB power cycle");
                let wd = &rm.usb_watchdog;
                info!("Starting post-wake USB power cycle sequence");
                // force_power_cycle already handles restart_microphone_if_needed
                if !wd.force_power_cycle() {
                    warn!("Post-wake USB power cycle was skipped (already cycling or cooldown active)");
                }
            }
            None => {
                error!("Audio recording manager state not found on wake!");
            }
        }
    });
}

/// No-op on non-macOS platforms.
#[cfg(not(target_os = "macos"))]
pub fn start_sleep_wake_listener(_app_handle: tauri::AppHandle) {
    // Sleep/wake detection is macOS-only
}
