use crate::settings::{get_settings, write_settings};
use anyhow::Result;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use tar::Archive;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum EngineType {
    Whisper,
    Parakeet,
    Moonshine,
    Qwen3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub filename: String,
    pub url: Option<String>,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
    pub is_directory: bool,
    pub engine_type: EngineType,
    pub accuracy_score: f32, // 0.0 to 1.0, higher is more accurate
    pub speed_score: f32,    // 0.0 to 1.0, higher is faster
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

pub struct ModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    available_models: Mutex<HashMap<String, ModelInfo>>,
}

impl ModelManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create models directory in app data
        let models_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
            .join("models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let mut available_models = HashMap::new();

        // TODO this should be read from a JSON file or something..
        available_models.insert(
            "small".to_string(),
            ModelInfo {
                id: "small".to_string(),
                name: "Whisper Small".to_string(),
                description: "Fast and fairly accurate.".to_string(),
                filename: "ggml-small.bin".to_string(),
                url: Some("https://blob.handy.computer/ggml-small.bin".to_string()),
                size_mb: 487,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::Whisper,
                accuracy_score: 0.60,
                speed_score: 0.85,
            },
        );

        // Add downloadable models
        available_models.insert(
            "medium".to_string(),
            ModelInfo {
                id: "medium".to_string(),
                name: "Whisper Medium".to_string(),
                description: "Good accuracy, medium speed".to_string(),
                filename: "whisper-medium-q4_1.bin".to_string(),
                url: Some("https://blob.handy.computer/whisper-medium-q4_1.bin".to_string()),
                size_mb: 492, // Approximate size
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::Whisper,
                accuracy_score: 0.75,
                speed_score: 0.60,
            },
        );

        available_models.insert(
            "turbo".to_string(),
            ModelInfo {
                id: "turbo".to_string(),
                name: "Whisper Turbo".to_string(),
                description: "Balanced accuracy and speed.".to_string(),
                filename: "ggml-large-v3-turbo.bin".to_string(),
                url: Some("https://blob.handy.computer/ggml-large-v3-turbo.bin".to_string()),
                size_mb: 1600, // Approximate size
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::Whisper,
                accuracy_score: 0.80,
                speed_score: 0.40,
            },
        );

        available_models.insert(
            "large".to_string(),
            ModelInfo {
                id: "large".to_string(),
                name: "Whisper Large".to_string(),
                description: "Good accuracy, but slow.".to_string(),
                filename: "ggml-large-v3-q5_0.bin".to_string(),
                url: Some("https://blob.handy.computer/ggml-large-v3-q5_0.bin".to_string()),
                size_mb: 1100, // Approximate size
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::Whisper,
                accuracy_score: 0.85,
                speed_score: 0.30,
            },
        );

        // Add NVIDIA Parakeet models (directory-based)
        available_models.insert(
            "parakeet-tdt-0.6b-v2".to_string(),
            ModelInfo {
                id: "parakeet-tdt-0.6b-v2".to_string(),
                name: "Parakeet V2".to_string(),
                description: "English only. The best model for English speakers.".to_string(),
                filename: "parakeet-tdt-0.6b-v2-int8".to_string(), // Directory name
                url: Some("https://blob.handy.computer/parakeet-v2-int8.tar.gz".to_string()),
                size_mb: 473, // Approximate size for int8 quantized model
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Parakeet,
                accuracy_score: 0.85,
                speed_score: 0.85,
            },
        );

        available_models.insert(
            "parakeet-tdt-0.6b-v3".to_string(),
            ModelInfo {
                id: "parakeet-tdt-0.6b-v3".to_string(),
                name: "Parakeet V3".to_string(),
                description: "Fast and accurate".to_string(),
                filename: "parakeet-tdt-0.6b-v3-int8".to_string(), // Directory name
                url: Some("https://blob.handy.computer/parakeet-v3-int8.tar.gz".to_string()),
                size_mb: 478, // Approximate size for int8 quantized model
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Parakeet,
                accuracy_score: 0.80,
                speed_score: 0.85,
            },
        );

        available_models.insert(
            "moonshine-base".to_string(),
            ModelInfo {
                id: "moonshine-base".to_string(),
                name: "Moonshine Base".to_string(),
                description: "Very fast, English only. Handles accents well.".to_string(),
                filename: "moonshine-base".to_string(),
                url: Some("https://blob.handy.computer/moonshine-base.tar.gz".to_string()),
                size_mb: 58,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: true,
                engine_type: EngineType::Moonshine,
                accuracy_score: 0.70,
                speed_score: 0.90,
            },
        );

        // Qwen3 ASR model (macOS only, MLX-based)
        // Note: Model is downloaded via mlx-audio on demand
        available_models.insert(
            "qwen3-asr".to_string(),
            ModelInfo {
                id: "qwen3-asr".to_string(),
                name: "Qwen3 ASR (MLX)".to_string(),
                description: "Apple Silicon optimized ASR using MLX framework. Supports Chinese and multilingual.".to_string(),
                filename: "qwen3-asr".to_string(),
                url: Some("mlx://qwen3-asr".to_string()), // Special URL scheme for mlx-audio managed models
                size_mb: 600, // Approximate size for Qwen3-ASR-0.6B-8bit
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::Qwen3,
                accuracy_score: 0.90,
                speed_score: 0.85,
            },
        );

        let manager = Self {
            app_handle: app_handle.clone(),
            models_dir,
            available_models: Mutex::new(available_models),
        };

        // Migrate any bundled models to user directory
        manager.migrate_bundled_models()?;

        // Check which models are already downloaded
        manager.update_download_status()?;

        // Auto-select a model if none is currently selected
        manager.auto_select_model_if_needed()?;

        Ok(manager)
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.values().cloned().collect()
    }

    pub fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.get(model_id).cloned()
    }

    fn migrate_bundled_models(&self) -> Result<()> {
        // Check for bundled models and copy them to user directory
        let bundled_models = ["ggml-small.bin"]; // Add other bundled models here if any

        for filename in &bundled_models {
            let bundled_path = self.app_handle.path().resolve(
                &format!("resources/models/{}", filename),
                tauri::path::BaseDirectory::Resource,
            );

            if let Ok(bundled_path) = bundled_path {
                if bundled_path.exists() {
                    let user_path = self.models_dir.join(filename);

                    // Only copy if user doesn't already have the model
                    if !user_path.exists() {
                        info!("Migrating bundled model {} to user directory", filename);
                        fs::copy(&bundled_path, &user_path)?;
                        info!("Successfully migrated {}", filename);
                    }
                }
            }
        }

        Ok(())
    }

    fn update_download_status(&self) -> Result<()> {
        // Pre-compute mlx model cache status to avoid deadlock
        let mlx_models_status: std::collections::HashMap<String, bool> = {
            let models = self.available_models.lock().unwrap();
            models
                .values()
                .filter(|m| m.url.as_ref().map(|u| u.starts_with("mlx://")).unwrap_or(false))
                .map(|m| (m.id.clone(), self.check_mlx_model_cached(&m.id)))
                .collect()
        };

        let mut models = self.available_models.lock().unwrap();

        for model in models.values_mut() {
            // Handle mlx-audio managed models (Qwen3)
            if let Some(url) = &model.url {
                if url.starts_with("mlx://") {
                    // Use pre-computed cache status
                    model.is_downloaded = *mlx_models_status.get(&model.id).unwrap_or(&false);
                    model.is_downloading = false;
                    continue;
                }
            }

            // Skip download status check for models managed externally
            if model.url.is_none() {
                model.is_downloaded = true;
                model.is_downloading = false;
                continue;
            }

            if model.is_directory {
                // For directory-based models, check if the directory exists
                let model_path = self.models_dir.join(&model.filename);
                let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));
                let extracting_path = self
                    .models_dir
                    .join(format!("{}.extracting", &model.filename));

                // Clean up any leftover .extracting directories from interrupted extractions
                if extracting_path.exists() {
                    warn!("Cleaning up interrupted extraction for model: {}", model.id);
                    let _ = fs::remove_dir_all(&extracting_path);
                }

                model.is_downloaded = model_path.exists() && model_path.is_dir();
                model.is_downloading = false;

                // Get partial file size if it exists (for the .tar.gz being downloaded)
                if partial_path.exists() {
                    model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
                } else {
                    model.partial_size = 0;
                }
            } else {
                // For file-based models (existing logic)
                let model_path = self.models_dir.join(&model.filename);
                let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));

                model.is_downloaded = model_path.exists();
                model.is_downloading = false;

                // Get partial file size if it exists
                if partial_path.exists() {
                    model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
                } else {
                    model.partial_size = 0;
                }
            }
        }

        Ok(())
    }

    fn auto_select_model_if_needed(&self) -> Result<()> {
        // Check if we have a selected model in settings
        let settings = get_settings(&self.app_handle);

        // If no model is selected or selected model is empty
        if settings.selected_model.is_empty() {
            // Find the first available (downloaded) model
            let models = self.available_models.lock().unwrap();
            if let Some(available_model) = models.values().find(|model| model.is_downloaded) {
                info!(
                    "Auto-selecting model: {} ({})",
                    available_model.id, available_model.name
                );

                // Update settings with the selected model
                let mut updated_settings = settings;
                updated_settings.selected_model = available_model.id.clone();
                write_settings(&self.app_handle, updated_settings);

                info!("Successfully auto-selected model: {}", available_model.id);
            }
        }

        Ok(())
    }

    pub async fn download_model(&self, model_id: &str) -> Result<()> {
        info!("========================================");
        info!("Starting model download for: {}", model_id);
        info!("========================================");

        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info = match model_info {
            Some(info) => {
                info!("Found model info: id={}, name={}, size_mb={}", info.id, info.name, info.size_mb);
                info
            }
            None => {
                error!("Model not found: {}", model_id);
                return Err(anyhow::anyhow!("Model not found: {}", model_id));
            }
        };

        let url = match &model_info.url {
            Some(u) => {
                info!("Download URL: {}", u);
                u.clone()
            }
            None => {
                error!("No download URL for model: {}", model_id);
                return Err(anyhow::anyhow!("No download URL for model"));
            }
        };

        // Handle mlx-audio managed models (Qwen3)
        if url.starts_with("mlx://") {
            info!("Detected mlx-audio managed model, using mlx download path");
            return self.download_mlx_model(model_id).await;
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        info!("Model path: {:?}", model_path);
        info!("Partial path: {:?}", partial_path);
        info!("Models directory: {:?}", self.models_dir);

        // Don't download if complete version already exists
        if model_path.exists() {
            info!("Model file already exists at {:?}, skipping download", model_path);
            // Clean up any partial file that might exist
            if partial_path.exists() {
                info!("Cleaning up existing partial file");
                let _ = fs::remove_file(&partial_path);
            }
            self.update_download_status()?;
            return Ok(());
        }

        // Check if we have a partial download to resume
        let mut resume_from = if partial_path.exists() {
            let size = partial_path.metadata()?.len();
            info!("Found partial download, resuming from byte {}", size);
            size
        } else {
            info!("Starting fresh download from {}", url);
            0
        };

        // Mark as downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = true;
                info!("Marked model {} as downloading", model_id);
            }
        }

        // Create HTTP client with range request for resuming
        info!("Creating HTTP client for download...");
        let client = reqwest::Client::new();
        let mut request = client.get(&url);

        if resume_from > 0 {
            info!("Adding Range header: bytes={}-", resume_from);
            request = request.header("Range", format!("bytes={}-", resume_from));
        }

        info!("Sending HTTP request...");
        let mut response = request.send().await?;

        // If we tried to resume but server returned 200 (not 206 Partial Content),
        // the server doesn't support range requests. Delete partial file and restart
        // fresh to avoid file corruption (appending full file to partial).
        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            warn!(
                "Server doesn't support range requests for model {}, restarting download",
                model_id
            );
            info!("Server response status: {:?}", response.status());
            drop(response);
            let _ = fs::remove_file(&partial_path);
            info!("Removed partial file, restarting download");

            // Reset resume_from since we're starting fresh
            resume_from = 0;

            // Restart download without range header
            info!("Restarting download without range header...");
            response = client.get(&url).send().await?;
        }

        // Check for success or partial content status
        info!("Server response status: {:?}", response.status());
        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            error!("Download failed with HTTP status: {}", response.status());
            // Mark as not downloading on error
            {
                let mut models = self.available_models.lock().unwrap();
                if let Some(model) = models.get_mut(model_id) {
                    model.is_downloading = false;
                }
            }
            return Err(anyhow::anyhow!(
                "Failed to download model: HTTP {}",
                response.status()
            ));
        }

        let total_size = if resume_from > 0 {
            // For resumed downloads, add the resume point to content length
            resume_from + response.content_length().unwrap_or(0)
        } else {
            response.content_length().unwrap_or(0)
        };

        info!("Total download size: {} bytes ({} MB)", total_size, total_size / 1024 / 1024);

        let mut downloaded = resume_from;
        let mut stream = response.bytes_stream();

        // Open file for appending if resuming, or create new if starting fresh
        info!("Opening file for download (resume_from={})...", resume_from);
        let mut file = if resume_from > 0 {
            info!("Opening existing partial file for append: {:?}", partial_path);
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&partial_path)?
        } else {
            info!("Creating new partial file: {:?}", partial_path);
            std::fs::File::create(&partial_path)?
        };

        // Emit initial progress
        let initial_progress = DownloadProgress {
            model_id: model_id.to_string(),
            downloaded,
            total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            },
        };
        let _ = self
            .app_handle
            .emit("model-download-progress", &initial_progress);
        info!("Initial progress emitted: {} / {} bytes", downloaded, total_size);

        // Download with progress
        info!("Starting download loop...");
        let mut last_log_time = std::time::Instant::now();
        let mut chunk_count = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                error!("Error downloading chunk: {}", e);
                // Mark as not downloading on error
                {
                    let mut models = self.available_models.lock().unwrap();
                    if let Some(model) = models.get_mut(model_id) {
                        model.is_downloading = false;
                    }
                }
                e
            })?;

            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            chunk_count += 1;

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            // Log progress every 5 seconds
            if last_log_time.elapsed().as_secs() >= 5 {
                info!(
                    "Download progress: {} / {} bytes ({:.1}%)",
                    downloaded, total_size, percentage
                );
                last_log_time = std::time::Instant::now();
            }

            // Emit progress event
            let progress = DownloadProgress {
                model_id: model_id.to_string(),
                downloaded,
                total: total_size,
                percentage,
            };

            let _ = self.app_handle.emit("model-download-progress", &progress);
        }

        info!("Download loop completed, received {} chunks", chunk_count);

        file.flush()?;
        info!("File flushed successfully");
        drop(file); // Ensure file is closed before moving
        info!("File handle dropped");

        // Verify downloaded file size matches expected size
        if total_size > 0 {
            let actual_size = partial_path.metadata()?.len();
            info!("Verifying file size: expected {} bytes, got {} bytes", total_size, actual_size);
            if actual_size != total_size {
                error!("Download incomplete: expected {} bytes, got {} bytes", total_size, actual_size);
                // Download is incomplete/corrupted - delete partial and return error
                let _ = fs::remove_file(&partial_path);
                {
                    let mut models = self.available_models.lock().unwrap();
                    if let Some(model) = models.get_mut(model_id) {
                        model.is_downloading = false;
                    }
                }
                return Err(anyhow::anyhow!(
                    "Download incomplete: expected {} bytes, got {} bytes",
                    total_size,
                    actual_size
                ));
            }
            info!("File size verification passed");
        }

        // Handle directory-based models (extract tar.gz) vs file-based models
        if model_info.is_directory {
            // Emit extraction started event
            let _ = self.app_handle.emit("model-extraction-started", model_id);
            info!("Extracting archive for directory-based model: {}", model_id);

            // Use a temporary extraction directory to ensure atomic operations
            let temp_extract_dir = self
                .models_dir
                .join(format!("{}.extracting", &model_info.filename));
            let final_model_dir = self.models_dir.join(&model_info.filename);
            info!("Temp extract dir: {:?}", temp_extract_dir);
            info!("Final model dir: {:?}", final_model_dir);

            // Clean up any previous incomplete extraction
            if temp_extract_dir.exists() {
                info!("Cleaning up previous incomplete extraction");
                let _ = fs::remove_dir_all(&temp_extract_dir);
            }

            // Create temporary extraction directory
            info!("Creating temporary extraction directory...");
            fs::create_dir_all(&temp_extract_dir)?;

            // Open the downloaded tar.gz file
            info!("Opening tar.gz file: {:?}", partial_path);
            let tar_gz = File::open(&partial_path)?;
            let tar = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);

            // Extract to the temporary directory first
            info!("Starting archive extraction...");
            archive.unpack(&temp_extract_dir).map_err(|e| {
                let error_msg = format!("Failed to extract archive: {}", e);
                error!("{}", error_msg);
                // Clean up failed extraction
                let _ = fs::remove_dir_all(&temp_extract_dir);
                let _ = self.app_handle.emit(
                    "model-extraction-failed",
                    &serde_json::json!({
                        "model_id": model_id,
                        "error": error_msg
                    }),
                );
                anyhow::anyhow!(error_msg)
            })?;
            info!("Archive extracted successfully");

            // Find the actual extracted directory (archive might have a nested structure)
            let extracted_dirs: Vec<_> = fs::read_dir(&temp_extract_dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();
            info!("Found {} directories in extraction", extracted_dirs.len());

            if extracted_dirs.len() == 1 {
                // Single directory extracted, move it to the final location
                let source_dir = extracted_dirs[0].path();
                info!("Moving single directory from {:?} to {:?}", source_dir, final_model_dir);
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&source_dir, &final_model_dir)?;
                // Clean up temp directory
                let _ = fs::remove_dir_all(&temp_extract_dir);
            } else {
                // Multiple items or no directories, rename the temp directory itself
                info!("Moving temp directory to final location");
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&temp_extract_dir, &final_model_dir)?;
            }

            info!("Successfully extracted archive for model: {}", model_id);
            // Emit extraction completed event
            let _ = self.app_handle.emit("model-extraction-completed", model_id);

            // Remove the downloaded tar.gz file
            info!("Removing tar.gz file: {:?}", partial_path);
            let _ = fs::remove_file(&partial_path);
        } else {
            // Move partial file to final location for file-based models
            info!("Moving partial file to final location: {:?} -> {:?}", partial_path, model_path);
            fs::rename(&partial_path, &model_path)?;
        }

        // Update download status
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                model.partial_size = 0;
                info!("Updated model status: is_downloaded=true, is_downloading=false");
            }
        }

        // Emit completion event
        info!("Emitting model-download-complete event");
        let _ = self.app_handle.emit("model-download-complete", model_id);

        info!("========================================");
        info!("Successfully downloaded model {}", model_id);
        info!("Path: {:?}", model_path);
        info!("========================================");

        Ok(())
    }

    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: delete_model called for: {}", model_id);

        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        debug!("ModelManager: Found model info: {:?}", model_info);

        // Handle mlx-audio managed models (Qwen3)
        if let Some(url) = &model_info.url {
            if url.starts_with("mlx://") {
                return self.delete_mlx_model(model_id);
            }
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));
        debug!("ModelManager: Model path: {:?}", model_path);
        debug!("ModelManager: Partial path: {:?}", partial_path);

        let mut deleted_something = false;

        if model_info.is_directory {
            // Delete complete model directory if it exists
            if model_path.exists() && model_path.is_dir() {
                info!("Deleting model directory at: {:?}", model_path);
                fs::remove_dir_all(&model_path)?;
                info!("Model directory deleted successfully");
                deleted_something = true;
            }
        } else {
            // Delete complete model file if it exists
            if model_path.exists() {
                info!("Deleting model file at: {:?}", model_path);
                fs::remove_file(&model_path)?;
                info!("Model file deleted successfully");
                deleted_something = true;
            }
        }

        // Delete partial file if it exists (same for both types)
        if partial_path.exists() {
            info!("Deleting partial file at: {:?}", partial_path);
            fs::remove_file(&partial_path)?;
            info!("Partial file deleted successfully");
            deleted_something = true;
        }

        if !deleted_something {
            return Err(anyhow::anyhow!("No model files found to delete"));
        }

        // Update download status
        self.update_download_status()?;
        debug!("ModelManager: download status updated");

        Ok(())
    }

    /// Delete an mlx-audio managed model from cache
    fn delete_mlx_model(&self, model_id: &str) -> Result<()> {
        info!("Deleting mlx-audio managed model: {}", model_id);

        // Map model_id to mlx-audio model name
        let mlx_model_name = match model_id {
            "qwen3-asr" => "mlx-community/Qwen3-ASR-0.6B-8bit",
            _ => {
                return Err(anyhow::anyhow!("Unknown mlx-audio model: {}", model_id));
            }
        };

        // Get the MLX cache directory
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let model_cache_dir = PathBuf::from(&home_dir)
            .join(".cache/mlx_audio")
            .join(mlx_model_name.replace("/", "--"));

        info!("MLX model cache directory: {:?}", model_cache_dir);

        if model_cache_dir.exists() {
            info!("Removing mlx-audio model cache: {:?}", model_cache_dir);
            fs::remove_dir_all(&model_cache_dir)?;
            info!("mlx-audio model cache removed successfully");
        } else {
            info!("mlx-audio model cache not found, may already be deleted");
        }

        // Update download status
        self.update_download_status()?;
        info!("Model delete completed for: {}", model_id);

        Ok(())
    }

    pub fn get_model_path(&self, model_id: &str) -> Result<PathBuf> {
        let model_info = self
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            return Err(anyhow::anyhow!("Model not available: {}", model_id));
        }

        // Ensure we don't return partial files/directories
        if model_info.is_downloading {
            return Err(anyhow::anyhow!(
                "Model is currently downloading: {}",
                model_id
            ));
        }

        // Handle mlx-audio managed models (Qwen3)
        if let Some(url) = &model_info.url {
            if url.starts_with("mlx://") {
                // For mlx-audio models, return a virtual path
                // The actual model is managed by mlx-audio
                return Ok(PathBuf::from(format!("mlx://{}", model_id)));
            }
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        if model_info.is_directory {
            // For directory-based models, ensure the directory exists and is complete
            if model_path.exists() && model_path.is_dir() && !partial_path.exists() {
                Ok(model_path)
            } else {
                Err(anyhow::anyhow!(
                    "Complete model directory not found: {}",
                    model_id
                ))
            }
        } else {
            // For file-based models (existing logic)
            if model_path.exists() && !partial_path.exists() {
                Ok(model_path)
            } else {
                Err(anyhow::anyhow!(
                    "Complete model file not found: {}",
                    model_id
                ))
            }
        }
    }

    /// Check if an mlx-audio model is cached locally
    fn check_mlx_model_cached(&self, model_id: &str) -> bool {
        // Get the MLX cache directory
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let mlx_cache_dir = PathBuf::from(&home_dir).join(".cache/mlx_audio");

        info!("Checking MLX cache at: {:?}", mlx_cache_dir);

        if !mlx_cache_dir.exists() {
            info!("MLX cache directory does not exist");
            return false;
        }

        // Map model_id to mlx-audio model name
        let mlx_model_name = match model_id {
            "qwen3-asr" => "mlx-community/Qwen3-ASR-0.6B-8bit",
            _ => {
                info!("Unknown mlx-audio model_id: {}", model_id);
                return false;
            }
        };

        // Check if model exists in cache (look for model.safetensors or similar files)
        let model_cache_dir = mlx_cache_dir.join(mlx_model_name.replace("/", "--"));
        info!("Checking model cache directory: {:?}", model_cache_dir);

        if model_cache_dir.exists() {
            // Check for essential model files
            let essential_files = ["model.safetensors", "config.json"];
            let found = essential_files.iter().any(|file| {
                let file_path = model_cache_dir.join(file);
                let exists = file_path.exists();
                if exists {
                    info!("Found essential file: {}", file);
                }
                exists
            });
            info!("Model {} cached: {}", model_id, found);
            found
        } else {
            info!("Model cache directory does not exist: {:?}", model_cache_dir);
            false
        }
    }

    /// Download an mlx-audio managed model using Python mlx-audio
    async fn download_mlx_model(&self, model_id: &str) -> Result<()> {
        info!("========================================");
        info!("Starting mlx-audio model download: {}", model_id);
        info!("========================================");

        // Mark as downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = true;
                info!("Marked model {} as downloading", model_id);
            }
        }

        // Map model_id to mlx-audio model name
        let mlx_model_name = match model_id {
            "qwen3-asr" => "mlx-community/Qwen3-ASR-0.6B-8bit",
            _ => {
                error!("Unknown mlx-audio model: {}", model_id);
                return Err(anyhow::anyhow!("Unknown mlx-audio model: {}", model_id));
            }
        };
        info!("MLX model name: {}", mlx_model_name);

        // Check if model is already cached
        info!("Checking if model is already cached...");
        if self.check_mlx_model_cached(model_id) {
            info!("Model {} is already cached, skipping download", model_id);
            // Mark as downloaded
            {
                let mut models = self.available_models.lock().unwrap();
                if let Some(model) = models.get_mut(model_id) {
                    model.is_downloading = false;
                    model.is_downloaded = true;
                }
            }
            // Emit completion event
            let _ = self.app_handle.emit("model-download-complete", model_id);
            return Ok(());
        }

        // Emit progress event
        info!("Emitting initial progress event (0%)");
        let _ = self.app_handle.emit(
            "model-download-progress",
            DownloadProgress {
                model_id: model_id.to_string(),
                downloaded: 0,
                total: 600 * 1024 * 1024, // Approximate size: 600MB
                percentage: 0.0,
            },
        );

        // Clone needed data for the async task
        let model_id_owned = model_id.to_string();
        let app_handle = self.app_handle.clone();
        let mlx_model_name_owned = mlx_model_name.to_string();

        // Start a task to emit simulated progress (since mlx-audio doesn't provide progress callbacks)
        info!("Starting progress simulation task");
        let progress_handle = tokio::spawn(async move {
            let mut progress = 0.0;
            while progress < 95.0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                progress += 2.5;
                let _ = app_handle.emit(
                    "model-download-progress",
                    DownloadProgress {
                        model_id: model_id_owned.clone(),
                        downloaded: (progress * 6.0 * 1024.0 * 1024.0) as u64, // Approximate
                        total: 600 * 1024 * 1024,
                        percentage: progress,
                    },
                );
            }
        });

        // Run Python script to preload the model in a blocking task
        info!("Running Python mlx-audio download script...");
        let output = tokio::task::spawn_blocking(move || {
            let script = format!(
                r#"
import sys
import json
import os

# Set HuggingFace mirror for China region
os.environ['HF_ENDPOINT'] = 'https://hf-mirror.com'

try:
    from mlx_audio.stt import load as load_stt
    # Load the model (this will download it if not cached)
    model = load_stt("{}")
    print(json.dumps({{"success": true}}))
except Exception as e:
    print(json.dumps({{"success": false, "error": str(e)}}))
    sys.exit(1)
"#,
                mlx_model_name_owned
            );
            std::process::Command::new("python3")
                .arg("-c")
                .arg(script)
                .output()
        })
        .await
        .map_err(|e| {
            error!("Failed to spawn mlx-audio download task: {}", e);
            progress_handle.abort();
            anyhow::anyhow!("Failed to run mlx-audio download: {}", e)
        })?
        .map_err(|e| {
            error!("mlx-audio download command failed: {}", e);
            progress_handle.abort();
            anyhow::anyhow!("Failed to run mlx-audio download: {}", e)
        })?;

        // Log stdout and stderr for debugging
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        info!("mlx-audio download stdout: {}", stdout);
        if !stderr.is_empty() {
            warn!("mlx-audio download stderr: {}", stderr);
        }

        // Abort the progress task
        info!("Aborting progress task");
        progress_handle.abort();

        // Emit 100% progress
        info!("Emitting 100% progress");
        let _ = self.app_handle.emit(
            "model-download-progress",
            DownloadProgress {
                model_id: model_id.to_string(),
                downloaded: 600 * 1024 * 1024,
                total: 600 * 1024 * 1024,
                percentage: 100.0,
            },
        );

        if !output.status.success() {
            error!("mlx-audio download failed with exit code: {:?}", output.status.code());
            // Mark as not downloading on error
            {
                let mut models = self.available_models.lock().unwrap();
                if let Some(model) = models.get_mut(model_id) {
                    model.is_downloading = false;
                }
            }
            return Err(anyhow::anyhow!("mlx-audio download failed: {}", stderr));
        }
        info!("Python script completed successfully");

        // Verify the download by checking cache
        info!("Verifying model download by checking cache...");
        if !self.check_mlx_model_cached(model_id) {
            error!("Model download verification failed - model not found in cache");
            // Mark as not downloading on error
            {
                let mut models = self.available_models.lock().unwrap();
                if let Some(model) = models.get_mut(model_id) {
                    model.is_downloading = false;
                }
            }
            return Err(anyhow::anyhow!("Model download verification failed"));
        }
        info!("Model verified in cache");

        // Mark as downloaded
        info!("Marking model as downloaded");
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                info!("Updated model status: is_downloaded=true");
            }
        }

        // Emit completion event
        info!("Emitting model-download-complete event");
        let _ = self.app_handle.emit("model-download-complete", model_id);

        info!("========================================");
        info!("Successfully downloaded mlx-audio model: {}", model_id);
        info!("========================================");
        Ok(())
    }

    pub fn cancel_download(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: cancel_download called for: {}", model_id);

        let _model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let _model_info =
            _model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        // Mark as not downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
            }
        }

        // Note: The actual download cancellation would need to be handled
        // by the download task itself. This just updates the state.
        // The partial file is kept so the download can be resumed later.

        // Update download status to reflect current state
        self.update_download_status()?;

        info!("Download cancelled for: {}", model_id);
        Ok(())
    }
}
