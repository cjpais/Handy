use anyhow::Result;
use futures_util::StreamExt;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Type of Onichan model
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum OnichanModelType {
    Llm,
    Tts,
}

/// Information about an Onichan model (LLM or TTS)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OnichanModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub filename: String,
    pub url: Option<String>,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
    pub model_type: OnichanModelType,
    /// For LLM models: context size
    pub context_size: Option<u32>,
    /// For TTS models: sample rate
    pub sample_rate: Option<u32>,
    /// For TTS models: voice name/style
    pub voice_name: Option<String>,
}

/// Download progress for Onichan models
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OnichanDownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

/// Manages LLM and TTS models for Onichan
pub struct OnichanModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    available_models: Mutex<HashMap<String, OnichanModelInfo>>,
}

impl OnichanModelManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let models_dir = app_handle
            .path()
            .app_data_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
            .join("onichan_models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let mut available_models = HashMap::new();

        // LLM Models
        available_models.insert(
            "llama-3.2-1b".to_string(),
            OnichanModelInfo {
                id: "llama-3.2-1b".to_string(),
                name: "Llama 3.2 1B".to_string(),
                description: "Fast and lightweight. Good for simple conversations.".to_string(),
                filename: "Llama-3.2-1B-Instruct-Q4_K_M.gguf".to_string(),
                url: Some("https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf".to_string()),
                size_mb: 775,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(8192),
                sample_rate: None,
                voice_name: None,
            },
        );

        available_models.insert(
            "llama-3.2-3b".to_string(),
            OnichanModelInfo {
                id: "llama-3.2-3b".to_string(),
                name: "Llama 3.2 3B".to_string(),
                description: "Balanced speed and quality. Recommended for most users.".to_string(),
                filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf".to_string(),
                url: Some("https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf".to_string()),
                size_mb: 2020,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(8192),
                sample_rate: None,
                voice_name: None,
            },
        );

        available_models.insert(
            "qwen-2.5-1.5b".to_string(),
            OnichanModelInfo {
                id: "qwen-2.5-1.5b".to_string(),
                name: "Qwen 2.5 1.5B".to_string(),
                description: "Excellent multilingual support. Fast responses.".to_string(),
                filename: "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf".to_string(),
                url: Some("https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf".to_string()),
                size_mb: 1050,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(32768),
                sample_rate: None,
                voice_name: None,
            },
        );

        // Uncensored/less-restricted models for more fun conversations
        available_models.insert(
            "mistral-7b-instruct".to_string(),
            OnichanModelInfo {
                id: "mistral-7b-instruct".to_string(),
                name: "Mistral 7B Instruct (Recommended)".to_string(),
                description: "Best quality and personality. Less censored, more fun. Recommended for Discord.".to_string(),
                filename: "mistral-7b-instruct-v0.2.Q4_K_M.gguf".to_string(),
                url: Some("https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF/resolve/main/mistral-7b-instruct-v0.2.Q4_K_M.gguf".to_string()),
                size_mb: 4370,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(32768),
                sample_rate: None,
                voice_name: None,
            },
        );

        available_models.insert(
            "dolphin-3.0-llama3.1-8b".to_string(),
            OnichanModelInfo {
                id: "dolphin-3.0-llama3.1-8b".to_string(),
                name: "Dolphin 3.0 Llama 3.1 8B (Recommended Uncensored)".to_string(),
                description: "Latest Dolphin. Uncensored, great personality, follows instructions well. Best for Discord.".to_string(),
                filename: "Dolphin3.0-Llama3.1-8B-Q4_K_M.gguf".to_string(),
                url: Some("https://huggingface.co/bartowski/Dolphin3.0-Llama3.1-8B-GGUF/resolve/main/Dolphin3.0-Llama3.1-8B-Q4_K_M.gguf".to_string()),
                size_mb: 4920,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(131072), // 128k context
                sample_rate: None,
                voice_name: None,
            },
        );

        available_models.insert(
            "neoplus-20b-uncensored".to_string(),
            OnichanModelInfo {
                id: "neoplus-20b-uncensored".to_string(),
                name: "NEOPlus 20B Uncensored (Best Quality)".to_string(),
                description: "Fully uncensored 20B model with DI-MATRIX optimization. Best quality responses. Requires 16GB+ RAM.".to_string(),
                filename: "OpenAI-20B-NEOPlus-Uncensored-IQ4_NL.gguf".to_string(),
                url: Some("https://huggingface.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf/resolve/main/OpenAI-20B-NEOPlus-Uncensored-IQ4_NL.gguf".to_string()),
                size_mb: 12600,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(4096),
                sample_rate: None,
                voice_name: None,
            },
        );

        available_models.insert(
            "brainstorm-36b-uncensored".to_string(),
            OnichanModelInfo {
                id: "brainstorm-36b-uncensored".to_string(),
                name: "BrainStorm 36B Uncensored Q8 (Best Quality)".to_string(),
                description: "Massive 36B uncensored model at Q8 quality. Best responses and creativity. Requires 40GB+ RAM.".to_string(),
                filename: "OpenAI-36B-Brains20x-Uncensored-Q8_0.gguf".to_string(),
                url: Some("https://huggingface.co/DavidAU/OpenAi-GPT-oss-36B-BrainStorm20x-uncensored-gguf/resolve/main/OpenAI-36B-Brains20x-Uncensored-Q8_0.gguf".to_string()),
                size_mb: 38900,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Llm,
                context_size: Some(131072), // 128k context
                sample_rate: None,
                voice_name: None,
            },
        );

        // TTS Models (Piper voices)
        available_models.insert(
            "piper-amy".to_string(),
            OnichanModelInfo {
                id: "piper-amy".to_string(),
                name: "Amy (English US)".to_string(),
                description: "Clear female voice. Natural sounding.".to_string(),
                filename: "en_US-amy-medium.onnx".to_string(),
                url: Some("https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx".to_string()),
                size_mb: 63,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Tts,
                context_size: None,
                sample_rate: Some(22050),
                voice_name: Some("Amy".to_string()),
            },
        );

        available_models.insert(
            "piper-lessac".to_string(),
            OnichanModelInfo {
                id: "piper-lessac".to_string(),
                name: "Lessac (English US)".to_string(),
                description: "Professional female voice. Balanced quality.".to_string(),
                filename: "en_US-lessac-medium.onnx".to_string(),
                url: Some("https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx".to_string()),
                size_mb: 63,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Tts,
                context_size: None,
                sample_rate: Some(22050),
                voice_name: Some("Lessac".to_string()),
            },
        );

        available_models.insert(
            "piper-jenny".to_string(),
            OnichanModelInfo {
                id: "piper-jenny".to_string(),
                name: "Jenny (English UK)".to_string(),
                description: "British female voice. Warm and friendly.".to_string(),
                filename: "en_GB-jenny_dioco-medium.onnx".to_string(),
                url: Some("https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_GB/jenny_dioco/medium/en_GB-jenny_dioco-medium.onnx".to_string()),
                size_mb: 63,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Tts,
                context_size: None,
                sample_rate: Some(22050),
                voice_name: Some("Jenny".to_string()),
            },
        );

        available_models.insert(
            "piper-lessac-high".to_string(),
            OnichanModelInfo {
                id: "piper-lessac-high".to_string(),
                name: "Lessac High (Anime Style)".to_string(),
                description: "Youthful female voice. Best for anime/VTuber style. Clear and energetic.".to_string(),
                filename: "en_US-lessac-high.onnx".to_string(),
                url: Some("https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/high/en_US-lessac-high.onnx".to_string()),
                size_mb: 105,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                model_type: OnichanModelType::Tts,
                context_size: None,
                sample_rate: Some(22050),
                voice_name: Some("Lessac (Anime)".to_string()),
            },
        );

        let manager = Self {
            app_handle: app_handle.clone(),
            models_dir,
            available_models: Mutex::new(available_models),
        };

        manager.update_download_status()?;

        Ok(manager)
    }

    pub fn get_models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    pub fn get_available_models(&self) -> Vec<OnichanModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.values().cloned().collect()
    }

    pub fn get_llm_models(&self) -> Vec<OnichanModelInfo> {
        let models = self.available_models.lock().unwrap();
        models
            .values()
            .filter(|m| m.model_type == OnichanModelType::Llm)
            .cloned()
            .collect()
    }

    pub fn get_tts_models(&self) -> Vec<OnichanModelInfo> {
        let models = self.available_models.lock().unwrap();
        models
            .values()
            .filter(|m| m.model_type == OnichanModelType::Tts)
            .cloned()
            .collect()
    }

    pub fn get_model_info(&self, model_id: &str) -> Option<OnichanModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.get(model_id).cloned()
    }

    fn update_download_status(&self) -> Result<()> {
        let mut models = self.available_models.lock().unwrap();

        for model in models.values_mut() {
            let model_path = self.models_dir.join(&model.filename);
            let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));

            model.is_downloaded = model_path.exists();
            model.is_downloading = false;

            if partial_path.exists() {
                model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
            } else {
                model.partial_size = 0;
            }
        }

        Ok(())
    }

    pub async fn download_model(&self, model_id: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        let url = model_info
            .url
            .ok_or_else(|| anyhow::anyhow!("No download URL for model"))?;
        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        // Also download the JSON config for TTS models
        let config_url = if model_info.model_type == OnichanModelType::Tts {
            Some(url.replace(".onnx", ".onnx.json"))
        } else {
            None
        };

        if model_path.exists() {
            if partial_path.exists() {
                let _ = fs::remove_file(&partial_path);
            }
            self.update_download_status()?;
            return Ok(());
        }

        let mut resume_from = if partial_path.exists() {
            let size = partial_path.metadata()?.len();
            info!(
                "Resuming download of onichan model {} from byte {}",
                model_id, size
            );
            size
        } else {
            info!(
                "Starting fresh download of onichan model {} from {}",
                model_id, url
            );
            0
        };

        // Mark as downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = true;
            }
        }

        // Emit initial progress event immediately so UI updates
        let initial_progress = OnichanDownloadProgress {
            model_id: model_id.to_string(),
            downloaded: resume_from,
            total: model_info.size_mb * 1024 * 1024, // Use expected size
            percentage: 0.0,
        };
        info!("Emitting initial download progress for {}", model_id);
        let _ = self
            .app_handle
            .emit("onichan-model-download-progress", &initial_progress);

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .redirect(reqwest::redirect::Policy::limited(10))
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(600)) // 10 min timeout per chunk
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        info!("Sending HTTP request to: {}", url);
        let mut request = client.get(&url);

        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }

        let mut response = match request.send().await {
            Ok(r) => {
                info!("HTTP response status: {}", r.status());
                r
            }
            Err(e) => {
                log::error!("HTTP request failed: {}", e);
                {
                    let mut models = self.available_models.lock().unwrap();
                    if let Some(model) = models.get_mut(model_id) {
                        model.is_downloading = false;
                    }
                }
                return Err(anyhow::anyhow!("HTTP request failed: {}", e));
            }
        };

        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            warn!(
                "Server doesn't support range requests for model {}, restarting download",
                model_id
            );
            drop(response);
            let _ = fs::remove_file(&partial_path);
            resume_from = 0;
            response = client.get(&url).send().await?;
        }

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
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
            resume_from + response.content_length().unwrap_or(0)
        } else {
            response.content_length().unwrap_or(0)
        };

        let mut downloaded = resume_from;
        let mut stream = response.bytes_stream();

        let mut file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&partial_path)?
        } else {
            std::fs::File::create(&partial_path)?
        };

        // Update progress with real total size now that we know it
        info!("Download started - total size: {} bytes", total_size);
        let updated_progress = OnichanDownloadProgress {
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
            .emit("onichan-model-download-progress", &updated_progress);

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
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

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            let progress = OnichanDownloadProgress {
                model_id: model_id.to_string(),
                downloaded,
                total: total_size,
                percentage,
            };

            let _ = self
                .app_handle
                .emit("onichan-model-download-progress", &progress);
        }

        file.flush()?;
        drop(file);

        if total_size > 0 {
            let actual_size = partial_path.metadata()?.len();
            if actual_size != total_size {
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
        }

        fs::rename(&partial_path, &model_path)?;

        // Download config file for TTS models
        if let Some(config_url) = config_url {
            let config_path = self
                .models_dir
                .join(format!("{}.json", &model_info.filename));
            if !config_path.exists() {
                info!("Downloading TTS config from {}", config_url);
                match client.get(&config_url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(bytes) = resp.bytes().await {
                            let _ = fs::write(&config_path, &bytes);
                        }
                    }
                    _ => {
                        warn!("Could not download TTS config, will use defaults");
                    }
                }
            }
        }

        // Update download status
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                model.partial_size = 0;
            }
        }

        let _ = self
            .app_handle
            .emit("onichan-model-download-complete", model_id);

        info!(
            "Successfully downloaded onichan model {} to {:?}",
            model_id, model_path
        );

        Ok(())
    }

    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));
        let config_path = self
            .models_dir
            .join(format!("{}.json", &model_info.filename));

        let mut deleted_something = false;

        if model_path.exists() {
            fs::remove_file(&model_path)?;
            deleted_something = true;
        }

        if partial_path.exists() {
            fs::remove_file(&partial_path)?;
            deleted_something = true;
        }

        if config_path.exists() {
            fs::remove_file(&config_path)?;
        }

        if !deleted_something {
            return Err(anyhow::anyhow!("No model files found to delete"));
        }

        self.update_download_status()?;

        Ok(())
    }

    pub fn get_model_path(&self, model_id: &str) -> Result<PathBuf> {
        let model_info = self
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            return Err(anyhow::anyhow!("Model not downloaded: {}", model_id));
        }

        let model_path = self.models_dir.join(&model_info.filename);
        if model_path.exists() {
            Ok(model_path)
        } else {
            Err(anyhow::anyhow!("Model file not found: {}", model_id))
        }
    }
}
