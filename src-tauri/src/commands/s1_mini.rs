use crate::managers::s1_mini::{S1MiniManager, S1MiniStatus};
use crate::settings::{self, S1Context, S1Structure, S1Styling};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub fn get_s1_mini_status(manager: State<'_, Arc<S1MiniManager>>) -> S1MiniStatus {
    manager.status()
}

#[tauri::command]
#[specta::specta]
pub async fn download_s1_mini(manager: State<'_, Arc<S1MiniManager>>) -> Result<(), String> {
    manager.download().await.map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn cancel_s1_mini_download(manager: State<'_, Arc<S1MiniManager>>) -> Result<(), String> {
    manager.cancel_download().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_s1_mini(manager: State<'_, Arc<S1MiniManager>>) -> Result<(), String> {
    manager.delete().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn unload_s1_mini(manager: State<'_, Arc<S1MiniManager>>) -> Result<(), String> {
    manager.unload().map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn change_s1_styling_setting(app: AppHandle, value: S1Styling) {
    let mut app_settings = settings::get_settings(&app);
    app_settings.s1_styling = value;
    settings::write_settings(&app, app_settings);
}

#[tauri::command]
#[specta::specta]
pub fn change_s1_structure_setting(app: AppHandle, value: S1Structure) {
    let mut app_settings = settings::get_settings(&app);
    app_settings.s1_structure = value;
    settings::write_settings(&app, app_settings);
}

#[tauri::command]
#[specta::specta]
pub fn change_s1_context_setting(app: AppHandle, value: S1Context) {
    let mut app_settings = settings::get_settings(&app);
    app_settings.s1_context = value;
    settings::write_settings(&app, app_settings);
}
