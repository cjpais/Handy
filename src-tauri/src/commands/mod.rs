pub mod audio;
pub mod history;
pub mod models;
pub mod transcription;

use crate::{settings, utils::cancel_current_operation};
use tauri::{AppHandle, Manager};
use tauri_plugin_log::LogLevel;

#[tauri::command]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}

#[tauri::command]
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_log_dir_path(app: AppHandle) -> Result<String, String> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("Failed to get log directory: {}", e))?;

    Ok(log_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub fn set_log_level(app: AppHandle, level: LogLevel) -> Result<(), String> {
    let log_level: log::Level = level.clone().into();
    log::set_max_level(log_level.to_level_filter());

    let mut settings = settings::get_settings(&app);
    settings.log_level = level;
    settings::write_settings(&app, settings);

    Ok(())
}
