pub mod audio;
pub mod history;
pub mod models;
pub mod polish_rules;
pub mod regex_filters;
pub mod transcription;

use crate::utils::cancel_current_operation;
use crate::settings::get_settings;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}

#[tauri::command]
pub async fn apply_polish_to_text(app: AppHandle, text: String) -> Result<String, String> {
    // Get polish rules from settings
    let settings = get_settings(&app);
    
    // Apply polish rules to the text
    let polished_text = crate::audio_toolkit::text::apply_polish_rules(&text, &settings.polish_rules).await;

    Ok(polished_text)
}

#[tauri::command]
pub fn get_app_dir_path(app: AppHandle) -> Result<String, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    Ok(app_data_dir.to_string_lossy().to_string())
}
