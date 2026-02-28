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
    let recordings_dir = crate::settings::resolve_recordings_dir(&app)?;
    let path = recordings_dir.to_string_lossy().into_owned();
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

#[specta::specta]
#[tauri::command]
pub fn get_models_dir_path(app: AppHandle) -> Result<String, String> {
    let models_dir = crate::settings::resolve_models_dir(&app)?;
    Ok(models_dir.to_string_lossy().to_string())
}

#[specta::specta]
#[tauri::command]
pub fn open_models_folder(app: AppHandle) -> Result<(), String> {
    let models_dir = crate::settings::resolve_models_dir(&app)?;
    let path = models_dir.to_string_lossy().into_owned();
    app.opener()
        .open_path(path, None::<String>)
        .map_err(|e| format!("Failed to open models folder: {}", e))?;

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, specta::Type, Debug)]
pub struct SetModelsDirResult {
    pub moved: usize,
    pub skipped: usize,
    pub failed: usize,
}

fn move_model_files_between_dirs(
    old_dir: &std::path::Path,
    new_dir: &std::path::Path,
) -> Result<SetModelsDirResult, String> {
    let mut result = SetModelsDirResult {
        moved: 0,
        skipped: 0,
        failed: 0,
    };

    if !old_dir.exists() {
        return Ok(result);
    }

    let entries = std::fs::read_dir(old_dir)
        .map_err(|e| format!("Failed to read models directory: {}", e))?;

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let source = old_dir.join(&file_name);
        let target = new_dir.join(&file_name);

        if target.exists() {
            result.skipped += 1;
            continue;
        }

        let move_result = if source.is_dir() {
            std::fs::rename(&source, &target)
        } else {
            std::fs::rename(&source, &target)
        };

        match move_result {
            Ok(_) => result.moved += 1,
            Err(_) => result.failed += 1,
        }
    }

    Ok(result)
}

/// Set (or clear) the custom models directory.
///
/// - `path = Some(...)` activates a custom folder.
/// - `path = None` reverts to the default `<app_data_dir>/models`.
/// - When `move_existing = true` the existing model files are moved from
///   old effective directory to the new one.
#[tauri::command]
#[specta::specta]
pub async fn set_models_directory(
    app: AppHandle,
    path: Option<String>,
    move_existing: bool,
) -> Result<SetModelsDirResult, String> {
    let old_dir = crate::settings::resolve_models_dir(&app)?;

    let new_dir: std::path::PathBuf = if let Some(ref p) = path {
        if p.trim().is_empty() {
            return Err("Models directory path must not be empty.".to_string());
        }

        let candidate = std::path::PathBuf::from(p);

        std::fs::create_dir_all(&candidate)
            .map_err(|e| format!("Failed to create directory: {}", e))?;

        let test = candidate.join(".handy_write_test");
        std::fs::write(&test, b"").map_err(|e| format!("Directory is not writable: {}", e))?;
        std::fs::remove_file(&test).ok();

        candidate
    } else {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to get app data directory: {}", e))?;
        let default_dir = app_data_dir.join("models");
        std::fs::create_dir_all(&default_dir)
            .map_err(|e| format!("Failed to create default models directory: {}", e))?;
        default_dir
    };

    let mut settings = crate::settings::get_settings(&app);
    settings.models_custom_dir = path;
    crate::settings::write_settings(&app, settings);

    let mut result = SetModelsDirResult {
        moved: 0,
        skipped: 0,
        failed: 0,
    };

    if move_existing && old_dir != new_dir && old_dir.exists() {
        result = move_model_files_between_dirs(&old_dir, &new_dir)?;
    }

    Ok(result)
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

    // Mark as initialized
    app.manage(ShortcutsInitialized);

    log::info!("Shortcuts initialized successfully");
    Ok(())
}
