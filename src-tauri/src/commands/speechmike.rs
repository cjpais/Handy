use crate::devices::speechmike::{SpeechMikeManager, SpeechMikeStatus};
use crate::settings::{get_settings, write_settings};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub fn get_speechmike_status(state: State<'_, Arc<SpeechMikeManager>>) -> SpeechMikeStatus {
    state.get_status()
}

#[tauri::command]
#[specta::specta]
pub fn set_speechmike_auto_select(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    if enabled {
        // Toggling on: reset the user-override flag so auto-select resumes.
        settings.selected_microphone_user_overridden = false;
    }
    settings.speechmike_auto_select = enabled;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_speechmike_button_mapping_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.speechmike_button_mapping_enabled = enabled;
    write_settings(&app, settings);
    Ok(())
}
