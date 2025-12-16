use anyhow::Result;
use futures_util::StreamExt;
use log::info;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const WHISPERFILE_URL: &str =
    "https://github.com/mozilla-ai/llamafile/releases/download/0.9.3/whisperfile-0.9.3";
const WHISPERFILE_FILENAME: &str = "whisperfile-0.9.3";

#[derive(Debug, Clone, Serialize)]
pub struct WhisperfileDownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

pub fn get_whisperfile_path(app_handle: &AppHandle) -> Result<PathBuf> {
    Ok(app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
        .join("binaries")
        .join(WHISPERFILE_FILENAME))
}

pub fn is_whisperfile_downloaded(app_handle: &AppHandle) -> bool {
    get_whisperfile_path(app_handle)
        .map(|p| p.exists())
        .unwrap_or(false)
}

pub async fn download_whisperfile(app_handle: &AppHandle) -> Result<PathBuf> {
    let path = get_whisperfile_path(app_handle)?;

    if path.exists() {
        info!("Whisperfile already downloaded at {:?}", path);
        return Ok(path);
    }

    info!("Downloading whisperfile from {}", WHISPERFILE_URL);

    // Create parent directory
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let response = reqwest::get(WHISPERFILE_URL).await?;
    let total = response.content_length().unwrap_or(0);
    let mut file = File::create(&path)?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;

        let progress = WhisperfileDownloadProgress {
            downloaded,
            total,
            percentage: if total > 0 {
                (downloaded as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        };

        let _ = app_handle.emit("whisperfile-download-progress", progress);
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms)?;
    }

    info!("Whisperfile downloaded successfully to {:?}", path);
    Ok(path)
}
