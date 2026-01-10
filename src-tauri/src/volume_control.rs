use log::{debug, info, warn};
use once_cell::sync::Lazy;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Stores the original volume before ducking was applied
static ORIGINAL_VOLUME: Lazy<Mutex<Option<f32>>> = Lazy::new(|| Mutex::new(None));

/// Get the path to the volume recovery file
/// Uses a temp directory location that persists across app restarts
fn get_recovery_file_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("handy_volume_recovery.txt");
    path
}

/// Persist the original volume to disk for crash recovery
fn persist_volume(volume: f32) {
    let path = get_recovery_file_path();
    if let Ok(mut file) = fs::File::create(&path) {
        let _ = writeln!(file, "{}", volume);
        debug!("Persisted original volume {} to {:?}", volume, path);
    }
}

/// Clear the persisted volume file (called after successful restore)
fn clear_persisted_volume() {
    let path = get_recovery_file_path();
    if path.exists() {
        let _ = fs::remove_file(&path);
        debug!("Cleared volume recovery file");
    }
}

/// Load persisted volume from disk (if exists)
fn load_persisted_volume() -> Option<f32> {
    let path = get_recovery_file_path();
    if let Ok(contents) = fs::read_to_string(&path) {
        contents.trim().parse().ok()
    } else {
        None
    }
}

/// Recover volume on app startup if a previous session crashed while ducking was active.
/// Call this once during app initialization.
pub fn recover_volume_on_startup() {
    if let Some(volume) = load_persisted_volume() {
        info!(
            "Found unrestored volume from previous session: {}%. Restoring...",
            (volume * 100.0) as i32
        );
        match set_volume(volume) {
            Ok(()) => {
                info!(
                    "Successfully restored volume to {}%",
                    (volume * 100.0) as i32
                );
                clear_persisted_volume();
            }
            Err(e) => {
                warn!(
                    "Failed to restore volume: {}. You may need to manually set your volume to {}%",
                    e,
                    (volume * 100.0) as i32
                );
            }
        }
    }
}

/// Get the current system volume
/// macOS/Windows: returns 0.0 - 1.0 range
/// Linux: may return values above 1.0 (PipeWire/PulseAudio boosted volumes)
pub fn get_volume() -> Result<f32, String> {
    #[cfg(target_os = "macos")]
    {
        macos::get_volume()
    }

    #[cfg(target_os = "windows")]
    {
        windows::get_volume()
    }

    #[cfg(target_os = "linux")]
    {
        linux::get_volume()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Unsupported platform".into())
    }
}

/// Set the system volume
/// macOS/Windows: 0.0 - 1.0 range
/// Linux: 0.0 - 1.5+ range (PipeWire/PulseAudio support boosted volumes)
pub fn set_volume(level: f32) -> Result<(), String> {
    // Linux supports volumes above 1.0, others don't
    #[cfg(target_os = "linux")]
    let level = level.max(0.0); // Only clamp minimum on Linux

    #[cfg(not(target_os = "linux"))]
    let level = level.clamp(0.0, 1.0);

    #[cfg(target_os = "macos")]
    {
        macos::set_volume(level)
    }

    #[cfg(target_os = "windows")]
    {
        windows::set_volume(level)
    }

    #[cfg(target_os = "linux")]
    {
        linux::set_volume(level)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("Unsupported platform".into())
    }
}

/// Apply audio ducking - stores original volume and reduces to target level
/// ducking_amount: 0.0 = no change, 1.0 = full mute
pub fn apply_ducking(ducking_amount: f32) -> Result<(), String> {
    let ducking_amount = ducking_amount.clamp(0.0, 1.0);

    // No ducking needed
    if ducking_amount == 0.0 {
        return Ok(());
    }

    // Get current volume
    let current_volume = get_volume()?;

    // Store original volume if not already stored
    let mut original = ORIGINAL_VOLUME.lock().map_err(|e| e.to_string())?;
    let is_first_duck = original.is_none();
    if is_first_duck {
        *original = Some(current_volume);
        debug!("Stored original volume: {}", current_volume);
    }

    // Calculate target volume: original * (1 - ducking_amount)
    let original_vol = original.unwrap_or(current_volume);
    let target_volume = original_vol * (1.0 - ducking_amount);

    debug!(
        "Applying ducking: {} -> {} ({}% reduction)",
        original_vol,
        target_volume,
        ducking_amount * 100.0
    );

    let result = set_volume(target_volume);

    // Only persist after successful volume change to avoid false recovery
    if result.is_ok() && is_first_duck {
        persist_volume(current_volume);
    }

    result
}

/// Restore the original volume after ducking
pub fn restore_volume() -> Result<(), String> {
    let mut original = ORIGINAL_VOLUME.lock().map_err(|e| e.to_string())?;

    if let Some(&vol) = original.as_ref() {
        debug!("Restoring original volume: {}", vol);
        let result = set_volume(vol);

        // Only clear state on successful restore - allows retry on failure
        if result.is_ok() {
            *original = None;
            clear_persisted_volume();
        }
        result
    } else {
        debug!("No original volume to restore");
        Ok(())
    }
}

/// Check if ducking is currently active
#[allow(dead_code)]
pub fn is_ducking_active() -> bool {
    ORIGINAL_VOLUME
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// macOS implementation using CoreAudio
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use coreaudio_sys::*;
    use std::mem;
    use std::ptr;

    fn get_default_output_device() -> Result<AudioDeviceID, String> {
        unsafe {
            let mut device_id: AudioDeviceID = 0;
            let mut size = mem::size_of::<AudioDeviceID>() as u32;

            let address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };

            let status = AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut _,
            );

            if status == 0 {
                Ok(device_id)
            } else {
                Err(format!(
                    "Failed to get default output device (status: {})",
                    status
                ))
            }
        }
    }

    pub fn get_volume() -> Result<f32, String> {
        let device_id = get_default_output_device()?;

        unsafe {
            let mut volume: f32 = 0.0;
            let mut size = mem::size_of::<f32>() as u32;

            // Try VirtualMainVolume first (newer API)
            let address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwareServiceDeviceProperty_VirtualMainVolume,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMain,
            };

            let status = AudioObjectGetPropertyData(
                device_id,
                &address,
                0,
                ptr::null(),
                &mut size,
                &mut volume as *mut _ as *mut _,
            );

            if status == 0 {
                Ok(volume)
            } else {
                // Fallback to older API
                let address = AudioObjectPropertyAddress {
                    mSelector: kAudioDevicePropertyVolumeScalar,
                    mScope: kAudioDevicePropertyScopeOutput,
                    mElement: 1, // Master channel
                };

                let status = AudioObjectGetPropertyData(
                    device_id,
                    &address,
                    0,
                    ptr::null(),
                    &mut size,
                    &mut volume as *mut _ as *mut _,
                );

                if status == 0 {
                    Ok(volume)
                } else {
                    Err(format!("Failed to get volume (status: {})", status))
                }
            }
        }
    }

    pub fn set_volume(level: f32) -> Result<(), String> {
        let device_id = get_default_output_device()?;

        unsafe {
            // Try VirtualMainVolume first (newer API)
            let address = AudioObjectPropertyAddress {
                mSelector: kAudioHardwareServiceDeviceProperty_VirtualMainVolume,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMain,
            };

            let status = AudioObjectSetPropertyData(
                device_id,
                &address,
                0,
                ptr::null(),
                mem::size_of::<f32>() as u32,
                &level as *const _ as *const _,
            );

            if status == 0 {
                Ok(())
            } else {
                // Fallback to older API
                let address = AudioObjectPropertyAddress {
                    mSelector: kAudioDevicePropertyVolumeScalar,
                    mScope: kAudioDevicePropertyScopeOutput,
                    mElement: 1, // Master channel
                };

                let status = AudioObjectSetPropertyData(
                    device_id,
                    &address,
                    0,
                    ptr::null(),
                    mem::size_of::<f32>() as u32,
                    &level as *const _ as *const _,
                );

                if status == 0 {
                    Ok(())
                } else {
                    Err(format!("Failed to set volume (status: {})", status))
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows implementation using COM API
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    use windows::Win32::{
        Media::Audio::{
            eMultimedia, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
            MMDeviceEnumerator,
        },
        System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
    };

    fn get_volume_interface() -> Result<IAudioEndpointVolume, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .map_err(|e| format!("Failed to get default audio endpoint: {}", e))?;

            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .map_err(|e| format!("Failed to activate volume interface: {}", e))
        }
    }

    pub fn get_volume() -> Result<f32, String> {
        unsafe {
            let volume_interface = get_volume_interface()?;
            volume_interface
                .GetMasterVolumeLevelScalar()
                .map_err(|e| format!("Failed to get volume: {}", e))
        }
    }

    pub fn set_volume(level: f32) -> Result<(), String> {
        unsafe {
            let volume_interface = get_volume_interface()?;
            volume_interface
                .SetMasterVolumeLevelScalar(level, std::ptr::null())
                .map_err(|e| format!("Failed to set volume: {}", e))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Linux implementation using wpctl/pactl
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    pub fn get_volume() -> Result<f32, String> {
        // Try wpctl first (PipeWire)
        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse "Volume: 0.50" or "Volume: 0.50 [MUTED]" format
                if let Some(vol_str) = stdout.split_whitespace().nth(1) {
                    if let Ok(vol) = vol_str.parse::<f32>() {
                        // Don't clamp - PipeWire/PulseAudio support volumes above 1.0 (boosted)
                        return Ok(vol);
                    }
                }
            }
        }

        // Try pactl (PulseAudio)
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse percentage from output like "Volume: front-left: 65536 / 100% / 0.00 dB"
                for part in stdout.split_whitespace() {
                    if part.ends_with('%') {
                        if let Ok(pct) = part.trim_end_matches('%').parse::<f32>() {
                            // Don't clamp - PulseAudio supports volumes above 100%
                            return Ok(pct / 100.0);
                        }
                    }
                }
            }
        }

        Err("Could not get system volume (wpctl/pactl not available)".into())
    }

    pub fn set_volume(level: f32) -> Result<(), String> {
        let level_str = format!("{:.2}", level);
        let percentage = format!("{}%", (level * 100.0) as i32);

        // Try wpctl first (PipeWire)
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &level_str])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(());
        }

        // Try pactl (PulseAudio)
        if Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &percentage])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(());
        }

        // Try amixer (ALSA)
        if Command::new("amixer")
            .args(["set", "Master", &percentage])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(());
        }

        Err("Could not set system volume (wpctl/pactl/amixer not available)".into())
    }
}
