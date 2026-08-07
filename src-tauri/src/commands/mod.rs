pub mod audio;
pub mod history;
pub mod models;
pub mod transcription;

use crate::settings::{get_settings, write_settings, AppSettings, LogLevel};
use crate::utils::cancel_current_operation;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

#[derive(serde::Serialize, Clone, specta::Type)]
pub struct SystemDetails {
    pub os_version: String,
    pub cpu_model: String,
    pub gpu_model: String,
}

#[tauri::command]
#[specta::specta]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}

#[tauri::command]
#[specta::specta]
pub fn is_portable() -> bool {
    crate::portable::is_portable()
}

#[tauri::command]
#[specta::specta]
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_app_settings(app: AppHandle) -> Result<AppSettings, String> {
    Ok(get_settings(&app))
}

#[tauri::command]
#[specta::specta]
pub fn get_default_settings() -> Result<AppSettings, String> {
    Ok(crate::settings::get_default_settings())
}

#[tauri::command]
#[specta::specta]
pub fn get_log_dir_path(app: AppHandle) -> Result<String, String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    Ok(log_dir.to_string_lossy().to_string())
}

#[cfg(target_os = "windows")]
fn get_windows_os_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let key = match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
    {
        Ok(key) => key,
        Err(_) => return "Unknown OS".to_string(),
    };
    let product_name: String = key.get_value("ProductName").unwrap_or_default();
    let display_version: String = key.get_value("DisplayVersion").unwrap_or_default();

    match (product_name.is_empty(), display_version.is_empty()) {
        (false, false) => format!("{} (Version {})", product_name, display_version),
        (false, true) => product_name,
        _ => "Unknown OS".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn get_windows_cpu_model() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0")
        .ok()
        .and_then(|key| key.get_value::<String, _>("ProcessorNameString").ok())
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

#[specta::specta]
#[tauri::command]
pub fn get_system_details() -> SystemDetails {
    #[cfg(target_os = "windows")]
    let os_version = get_windows_os_version();
    #[cfg(target_os = "macos")]
    let os_version = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| format!("macOS {}", String::from_utf8_lossy(&output.stdout).trim()))
        .unwrap_or_else(|| "Unknown OS".to_string());
    #[cfg(target_os = "linux")]
    let os_version = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| content.lines().find_map(|line| line.strip_prefix("PRETTY_NAME=")))
        .map(|name| name.trim_matches('"').to_string())
        .unwrap_or_else(|| "Unknown OS".to_string());
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let os_version = "Unknown OS".to_string();

    #[cfg(target_os = "windows")]
    let cpu_model = get_windows_cpu_model();
    #[cfg(target_os = "macos")]
    let cpu_model = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    #[cfg(target_os = "linux")]
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|content| content.lines().find_map(|line| {
            (line.starts_with("model name") || line.starts_with("Model"))
                .then(|| line.split_once(':').map(|(_, name)| name.trim().to_string()))
                .flatten()
        }))
        .unwrap_or_else(|| "Unknown CPU".to_string());
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let cpu_model = "Unknown CPU".to_string();

    let gpu_model = crate::managers::transcription::get_available_accelerators()
        .gpu_devices
        .iter()
        .map(|device| device.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    SystemDetails {
        os_version,
        cpu_model,
        gpu_model: if gpu_model.is_empty() { "Unknown GPU".to_string() } else { gpu_model },
    }
}

#[specta::specta]
#[tauri::command]
pub fn read_recent_logs(app: AppHandle) -> Result<String, String> {
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    const MAX_LINES: usize = 100;
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|error| format!("Failed to get log directory: {}", error))?;
    let file = File::open(log_dir.join("handy.log"))
        .map_err(|error| format!("Failed to read Handy logs: {}", error))?;
    let mut recent_lines = VecDeque::with_capacity(MAX_LINES);

    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| format!("Failed to read Handy logs: {}", error))?;
        if recent_lines.len() == MAX_LINES {
            recent_lines.pop_front();
        }
        recent_lines.push_back(line);
    }

    Ok(recent_lines.into_iter().collect::<Vec<_>>().join("\n"))
}

#[specta::specta]
#[tauri::command]
pub fn set_log_level(app: AppHandle, level: LogLevel) -> Result<(), String> {
    let tauri_log_level: tauri_plugin_log::LogLevel = level.into();
    let log_level: log::Level = tauri_log_level.into();
    // Update the file log level atomic so the filter picks up the new level
    crate::FILE_LOG_LEVEL.store(
        log_level.to_level_filter() as u8,
        std::sync::atomic::Ordering::Relaxed,
    );

    let mut settings = get_settings(&app);
    settings.log_level = level;
    write_settings(&app, settings);

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_recordings_folder(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let recordings_dir = app_data_dir.join("recordings");

    let path = recordings_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open recordings folder: {}", e))?;

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let log_dir = crate::portable::app_log_dir(&app)
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    let path = log_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open log directory: {}", e))?;

    Ok(())
}

#[specta::specta]
#[tauri::command]
pub fn open_app_data_dir(app: AppHandle) -> Result<(), String> {
    let app_data_dir = crate::portable::app_data_dir(&app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let path = app_data_dir.to_string_lossy().as_ref().to_string();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open app data directory: {}", e))?;

    Ok(())
}

/// Check if Apple Intelligence is available on this device.
/// Called by the frontend when the user selects Apple Intelligence provider.
#[specta::specta]
#[tauri::command]
pub fn check_apple_intelligence_available() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        crate::apple_intelligence::check_apple_intelligence_availability()
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

/// Try to initialize Enigo (keyboard/mouse simulation).
/// On macOS, this will return an error if accessibility permissions are not granted.
#[specta::specta]
#[tauri::command]
pub fn initialize_enigo(app: AppHandle) -> Result<(), String> {
    use crate::input::EnigoState;

    // Check if already initialized
    if app.try_state::<EnigoState>().is_some() {
        log::debug!("Enigo already initialized");
        return Ok(());
    }

    // Try to initialize
    match EnigoState::new() {
        Ok(enigo_state) => {
            app.manage(enigo_state);
            log::info!("Enigo initialized successfully after permission grant");
            Ok(())
        }
        Err(e) => {
            if cfg!(target_os = "macos") {
                log::warn!(
                    "Failed to initialize Enigo: {} (accessibility permissions may not be granted)",
                    e
                );
            } else {
                log::warn!("Failed to initialize Enigo: {}", e);
            }
            Err(format!("Failed to initialize input system: {}", e))
        }
    }
}

/// Marker state to track if shortcuts have been initialized.
pub struct ShortcutsInitialized;

/// Initialize keyboard shortcuts.
/// On macOS, this should be called after accessibility permissions are granted.
/// This is idempotent - calling it multiple times is safe.
#[specta::specta]
#[tauri::command]
pub fn initialize_shortcuts(app: AppHandle) -> Result<(), String> {
    // Check if already initialized
    if app.try_state::<ShortcutsInitialized>().is_some() {
        log::debug!("Shortcuts already initialized");
        return Ok(());
    }

    // Initialize shortcuts
    crate::shortcut::init_shortcuts(&app);

    // Mark as initialized before reconciling the macOS Secure Input fallback.
    app.manage(ShortcutsInitialized);
    crate::secure_input::reconcile_fallback(&app);

    log::info!("Shortcuts initialized successfully");
    Ok(())
}
