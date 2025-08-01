pub mod audio;
pub mod models;

use crate::utils::cancel_current_operation;
use tauri::AppHandle;

#[tauri::command]
pub fn cancel_operation(app: AppHandle) {
    cancel_current_operation(&app);
}
