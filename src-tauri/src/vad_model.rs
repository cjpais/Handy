//! VAD (Voice Activity Detection) model management
//!
//! This module handles downloading and verifying the Silero VAD model
//! which is required for voice activity detection during recording.

use anyhow::Result;
use futures_util::StreamExt;
use log::{debug, info, warn};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// URL for downloading the Silero VAD model
const VAD_MODEL_URL: &str = "https://blob.handy.computer/silero_vad_v4.onnx";

/// Expected file size in bytes (approximately 1.8 MB)
const VAD_MODEL_EXPECTED_SIZE: u64 = 1_807_522;

/// Filename for the VAD model
const VAD_MODEL_FILENAME: &str = "silero_vad_v4.onnx";

/// Progress event for VAD model download
#[derive(Clone, serde::Serialize)]
pub struct VadDownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

/// Get the path where the VAD model should be stored
pub fn get_vad_model_path(app_handle: &AppHandle) -> Result<PathBuf> {
    let resource_path = app_handle
        .path()
        .resolve(
            format!("resources/models/{}", VAD_MODEL_FILENAME),
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| anyhow::anyhow!("Failed to resolve VAD model path: {}", e))?;

    Ok(resource_path)
}

/// Check if the VAD model exists and is valid
pub fn is_vad_model_available(app_handle: &AppHandle) -> bool {
    match get_vad_model_path(app_handle) {
        Ok(path) => {
            if path.exists() {
                // Optionally verify file size
                if let Ok(metadata) = fs::metadata(&path) {
                    let size = metadata.len();
                    if size > 0 {
                        debug!("VAD model found at {:?} ({} bytes)", path, size);
                        return true;
                    }
                }
            }
            debug!("VAD model not found at {:?}", path);
            false
        }
        Err(e) => {
            warn!("Failed to check VAD model path: {}", e);
            false
        }
    }
}

/// Get the fallback path in app data directory for VAD model
fn get_vad_model_fallback_path(app_handle: &AppHandle) -> Result<PathBuf> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?;

    let models_dir = app_data.join("models");
    if !models_dir.exists() {
        fs::create_dir_all(&models_dir)?;
    }

    Ok(models_dir.join(VAD_MODEL_FILENAME))
}

/// Check if VAD model exists in fallback location
pub fn is_vad_model_in_fallback(app_handle: &AppHandle) -> bool {
    match get_vad_model_fallback_path(app_handle) {
        Ok(path) => path.exists() && fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false),
        Err(_) => false,
    }
}

/// Get the best available VAD model path (resource or fallback)
pub fn get_best_vad_model_path(app_handle: &AppHandle) -> Result<PathBuf> {
    // First try the bundled resource path
    if let Ok(resource_path) = get_vad_model_path(app_handle) {
        if resource_path.exists() {
            return Ok(resource_path);
        }
    }

    // Fall back to app data directory
    let fallback_path = get_vad_model_fallback_path(app_handle)?;
    if fallback_path.exists() {
        return Ok(fallback_path);
    }

    Err(anyhow::anyhow!(
        "VAD model not found. Please download it first."
    ))
}

/// Download the VAD model if it's not available
pub async fn ensure_vad_model(app_handle: &AppHandle) -> Result<PathBuf> {
    // Check if already available in resource path
    if let Ok(resource_path) = get_vad_model_path(app_handle) {
        if resource_path.exists() {
            info!("VAD model already available at {:?}", resource_path);
            return Ok(resource_path);
        }
    }

    // Check if available in fallback path
    let fallback_path = get_vad_model_fallback_path(app_handle)?;
    if fallback_path.exists() {
        info!("VAD model found in fallback location: {:?}", fallback_path);
        return Ok(fallback_path);
    }

    // Need to download
    info!("VAD model not found, downloading from {}", VAD_MODEL_URL);
    download_vad_model(app_handle, &fallback_path).await?;

    Ok(fallback_path)
}

/// Download the VAD model to the specified path
async fn download_vad_model(app_handle: &AppHandle, target_path: &PathBuf) -> Result<()> {
    let partial_path = target_path.with_extension("onnx.partial");

    // Emit download start event
    let _ = app_handle.emit("vad-download-started", ());

    let client = reqwest::Client::new();
    let response = client.get(VAD_MODEL_URL).send().await?;

    if !response.status().is_success() {
        let _ = app_handle.emit("vad-download-failed", "HTTP error");
        return Err(anyhow::anyhow!(
            "Failed to download VAD model: HTTP {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(VAD_MODEL_EXPECTED_SIZE);
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    let mut file = fs::File::create(&partial_path)?;

    // Emit initial progress
    let _ = app_handle.emit(
        "vad-download-progress",
        VadDownloadProgress {
            downloaded: 0,
            total: total_size,
            percentage: 0.0,
        },
    );

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;

        let progress = VadDownloadProgress {
            downloaded,
            total: total_size,
            percentage: (downloaded as f64 / total_size as f64) * 100.0,
        };
        let _ = app_handle.emit("vad-download-progress", &progress);
    }

    file.flush()?;
    drop(file);

    // Rename partial to final
    fs::rename(&partial_path, target_path)?;

    info!("VAD model downloaded successfully to {:?}", target_path);
    let _ = app_handle.emit("vad-download-complete", ());

    Ok(())
}

/// Tauri command to check if VAD model is available
#[tauri::command]
#[specta::specta]
pub fn is_vad_model_ready(app: AppHandle) -> bool {
    is_vad_model_available(&app) || is_vad_model_in_fallback(&app)
}

/// Tauri command to download VAD model if needed
#[tauri::command]
#[specta::specta]
pub async fn download_vad_model_if_needed(app: AppHandle) -> Result<String, String> {
    match ensure_vad_model(&app).await {
        Ok(path) => Ok(path.to_string_lossy().to_string()),
        Err(e) => Err(format!("Failed to download VAD model: {}", e)),
    }
}
