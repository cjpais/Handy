use crate::managers::whisperfile;
use tauri::AppHandle;

#[tauri::command]
#[specta::specta]
pub async fn download_whisperfile_binary(app: AppHandle) -> Result<String, String> {
    let path = whisperfile::download_whisperfile(&app)
        .await
        .map_err(|e| format!("Failed to download whisperfile: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
#[specta::specta]
pub fn is_whisperfile_binary_downloaded(app: AppHandle) -> bool {
    whisperfile::is_whisperfile_downloaded(&app)
}
