pub mod audio;
pub mod history;
pub mod models;
pub mod transcription;

use crate::settings::{get_settings, write_settings, AppSettings, LogLevel};
use crate::utils::cancel_current_operation;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
#[specta::specta]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}

#[tauri::command]
#[specta::specta]
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
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
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    Ok(log_dir.to_string_lossy().to_string())
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
    let app_data_dir = app
        .path()
        .app_data_dir()
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
    let log_dir = app
        .path()
        .app_log_dir()
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
    let app_data_dir = app
        .path()
        .app_data_dir()
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

/// Check if running on Wayland (Linux only)
/// Returns true if the system session is Wayland (regardless of GDK_BACKEND)
/// This is used for UI decisions about global shortcuts which don't work on Wayland
#[specta::specta]
#[tauri::command]
pub fn is_wayland_session() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check the actual session type, ignoring GDK_BACKEND
        // Global shortcuts don't work on Wayland even if we run the app with GDK_BACKEND=x11
        std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Configure a GNOME keyboard shortcut to trigger Handy via SIGUSR2
/// This is needed on Wayland where global shortcuts don't work
#[specta::specta]
#[tauri::command]
pub fn configure_gnome_shortcut(shortcut: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        // Get existing custom keybindings
        let existing = Command::new("gsettings")
            .args([
                "get",
                "org.gnome.settings-daemon.plugins.media-keys",
                "custom-keybindings",
            ])
            .output()
            .map_err(|e| format!("Failed to get existing keybindings: {}", e))?;

        let existing_str = String::from_utf8_lossy(&existing.stdout).trim().to_string();

        // Check if handy keybinding already exists
        let handy_path = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/handy/";
        let new_bindings = if existing_str == "@as []" || existing_str.is_empty() {
            format!("['{}']", handy_path)
        } else if existing_str.contains(handy_path) {
            existing_str
        } else {
            // Add handy to existing list
            let trimmed = existing_str.trim_matches(|c| c == '[' || c == ']');
            format!("[{}, '{}']", trimmed, handy_path)
        };

        // Set the custom keybindings list
        Command::new("gsettings")
            .args([
                "set",
                "org.gnome.settings-daemon.plugins.media-keys",
                "custom-keybindings",
                &new_bindings,
            ])
            .output()
            .map_err(|e| format!("Failed to set keybindings list: {}", e))?;

        // Configure the handy shortcut
        let base_path =
            "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/handy/";

        Command::new("gsettings")
            .args(["set", base_path, "name", "Handy Transcribe"])
            .output()
            .map_err(|e| format!("Failed to set shortcut name: {}", e))?;

        Command::new("gsettings")
            .args(["set", base_path, "command", "pkill -SIGUSR2 -f handy"])
            .output()
            .map_err(|e| format!("Failed to set shortcut command: {}", e))?;

        Command::new("gsettings")
            .args(["set", base_path, "binding", &shortcut])
            .output()
            .map_err(|e| format!("Failed to set shortcut binding: {}", e))?;

        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = shortcut;
        Err("GNOME shortcuts are only available on Linux".to_string())
    }
}

/// Get current GNOME shortcut for Handy if configured
#[specta::specta]
#[tauri::command]
pub fn get_gnome_shortcut() -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let base_path =
            "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/handy/";

        let output = Command::new("gsettings")
            .args(["get", base_path, "binding"])
            .output()
            .map_err(|e| format!("Failed to get shortcut: {}", e))?;

        if output.status.success() {
            let binding = String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_matches('\'')
                .to_string();
            if binding.is_empty() || binding == "" {
                Ok(None)
            } else {
                Ok(Some(binding))
            }
        } else {
            Ok(None)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(None)
    }
}
