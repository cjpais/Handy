use crate::managers::model::ModelManager;
use crate::managers::qwen_asr::QwenAsrManager;
use crate::settings::get_settings;
use anyhow::Result;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{App, AppHandle, Emitter, Manager};
use whisper_rs::install_whisper_log_trampoline;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub error: Option<String>,
}

pub struct TranscriptionManager {
    state: Mutex<Option<WhisperState>>,
    context: Mutex<Option<WhisperContext>>,
    model_manager: Arc<ModelManager>,
    qwen_asr_manager: Arc<QwenAsrManager>,
    app_handle: AppHandle,
    current_model_id: Mutex<Option<String>>,
    /// The backend of the currently loaded model: "whisper" or "qwen-asr"
    current_backend: Mutex<Option<String>>,
}

impl TranscriptionManager {
    pub fn new(
        app: &App,
        model_manager: Arc<ModelManager>,
        qwen_asr_manager: Arc<QwenAsrManager>,
    ) -> Result<Self> {
        let app_handle = app.app_handle().clone();

        let manager = Self {
            state: Mutex::new(None),
            context: Mutex::new(None),
            model_manager,
            qwen_asr_manager,
            app_handle: app_handle.clone(),
            current_model_id: Mutex::new(None),
            current_backend: Mutex::new(None),
        };

        // Try to load the default model from settings, but don't fail if no models are available
        let settings = get_settings(&app_handle);
        let _ = manager.load_model(&settings.selected_model);

        Ok(manager)
    }

    pub fn load_model(&self, model_id: &str) -> Result<()> {
        // Emit loading started event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_started".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: None,
                error: None,
            },
        );

        let model_info = self
            .model_manager
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            let error_msg = "Model not downloaded";
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    model_id: Some(model_id.to_string()),
                    model_name: Some(model_info.name.clone()),
                    error: Some(error_msg.to_string()),
                },
            );
            return Err(anyhow::anyhow!(error_msg));
        }

        if model_info.backend == "qwen-asr" {
            return self.load_qwen_asr_model(model_id, &model_info.name);
        }

        // Whisper backend
        let model_path = self.model_manager.get_model_path(model_id)?;

        let path_str = model_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path for model: {}", model_id))?;

        println!(
            "Loading transcription model {} from: {}",
            model_id, path_str
        );

        // Install log trampoline once per model load (safe to call multiple times)
        install_whisper_log_trampoline();

        // Create new context
        let context =
            WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
                .map_err(|e| {
                    let error_msg = format!("Failed to load whisper model {}: {}", model_id, e);
                    let _ = self.app_handle.emit(
                        "model-state-changed",
                        ModelStateEvent {
                            event_type: "loading_failed".to_string(),
                            model_id: Some(model_id.to_string()),
                            model_name: Some(model_info.name.clone()),
                            error: Some(error_msg.clone()),
                        },
                    );
                    anyhow::anyhow!(error_msg)
                })?;

        // Create new state
        let state = context.create_state().map_err(|e| {
            let error_msg = format!("Failed to create state for model {}: {}", model_id, e);
            let _ = self.app_handle.emit(
                "model-state-changed",
                ModelStateEvent {
                    event_type: "loading_failed".to_string(),
                    model_id: Some(model_id.to_string()),
                    model_name: Some(model_info.name.clone()),
                    error: Some(error_msg.clone()),
                },
            );
            anyhow::anyhow!(error_msg)
        })?;

        // Update the current context and state
        {
            let mut current_context = self.context.lock().unwrap();
            *current_context = Some(context);
        }
        {
            let mut current_state = self.state.lock().unwrap();
            *current_state = Some(state);
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = Some(model_id.to_string());
        }
        {
            let mut backend = self.current_backend.lock().unwrap();
            *backend = Some("whisper".to_string());
        }

        // Emit loading completed event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "loading_completed".to_string(),
                model_id: Some(model_id.to_string()),
                model_name: Some(model_info.name.clone()),
                error: None,
            },
        );

        println!("Successfully loaded transcription model: {}", model_id);
        Ok(())
    }

    fn load_qwen_asr_model(&self, model_id: &str, model_name: &str) -> Result<()> {
        println!("Loading Qwen3-ASR model via sidecar...");

        match self.qwen_asr_manager.load_model() {
            Ok(()) => {
                // Clear whisper state since we're using a different backend
                {
                    let mut current_context = self.context.lock().unwrap();
                    *current_context = None;
                }
                {
                    let mut current_state = self.state.lock().unwrap();
                    *current_state = None;
                }
                {
                    let mut current_model = self.current_model_id.lock().unwrap();
                    *current_model = Some(model_id.to_string());
                }
                {
                    let mut backend = self.current_backend.lock().unwrap();
                    *backend = Some("qwen-asr".to_string());
                }

                let _ = self.app_handle.emit(
                    "model-state-changed",
                    ModelStateEvent {
                        event_type: "loading_completed".to_string(),
                        model_id: Some(model_id.to_string()),
                        model_name: Some(model_name.to_string()),
                        error: None,
                    },
                );

                println!("Successfully loaded Qwen3-ASR model");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Failed to load Qwen3-ASR: {}", e);
                let _ = self.app_handle.emit(
                    "model-state-changed",
                    ModelStateEvent {
                        event_type: "loading_failed".to_string(),
                        model_id: Some(model_id.to_string()),
                        model_name: Some(model_name.to_string()),
                        error: Some(error_msg.clone()),
                    },
                );
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    pub fn get_current_model(&self) -> Option<String> {
        let current_model = self.current_model_id.lock().unwrap();
        current_model.clone()
    }

    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        let st = std::time::Instant::now();

        if audio.is_empty() {
            println!("Empty audio vector");
            return Ok(String::new());
        }

        println!("Audio vector length: {}", audio.len());

        let backend = {
            let b = self.current_backend.lock().unwrap();
            b.clone()
        };

        let settings = get_settings(&self.app_handle);

        match backend.as_deref() {
            Some("qwen-asr") => {
                let language = match settings.selected_language.as_str() {
                    "auto" | "auto-zh-TW" | "auto-zh-CN" => Some("English"),
                    "zh-TW" | "zh-CN" | "zh" => Some("Chinese"),
                    "en" => Some("English"),
                    "ja" => Some("Japanese"),
                    "ko" => Some("Korean"),
                    "fr" => Some("French"),
                    "de" => Some("German"),
                    "es" => Some("Spanish"),
                    "pt" => Some("Portuguese"),
                    "ru" => Some("Russian"),
                    "it" => Some("Italian"),
                    other => Some(other),
                };

                let result = self.qwen_asr_manager.transcribe(&audio, language)?;

                let et = std::time::Instant::now();
                println!("\nQwen3-ASR took {}ms", (et - st).as_millis());

                Ok(result)
            }
            _ => {
                // Whisper backend (default)
                self.transcribe_whisper(audio, &settings, st)
            }
        }
    }

    fn transcribe_whisper(
        &self,
        audio: Vec<f32>,
        settings: &crate::settings::AppSettings,
        st: std::time::Instant,
    ) -> Result<String> {
        let mut result = String::new();

        let mut state_guard = self.state.lock().unwrap();
        let state = state_guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "No model loaded. Please download and select a model from settings first."
            )
        })?;

        // Initialize parameters
        let mut params = FullParams::new(SamplingStrategy::default());

        // Handle Chinese language variants
        let (language, initial_prompt) = match settings.selected_language.as_str() {
            "auto-zh-TW" => {
                (None, Some("English. 繁體中文。"))
            }
            "zh-TW" => {
                (Some("zh"), Some("繁體中文。"))
            }
            "zh-CN" => {
                (Some("zh"), Some("简体中文。"))
            }
            lang => (Some(lang), None),
        };

        params.set_language(language);

        if let Some(prompt) = initial_prompt {
            params.set_initial_prompt(prompt);
        }

        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);

        if settings.translate_to_english {
            params.set_translate(true);
        }

        state
            .full(params, &audio)
            .expect("failed to convert samples");

        let num_segments = state
            .full_n_segments()
            .expect("failed to get number of segments");

        for i in 0..num_segments {
            let segment = state
                .full_get_segment_text(i)
                .expect("failed to get segment");
            result.push_str(&segment);
        }

        let et = std::time::Instant::now();
        let translation_note = if settings.translate_to_english {
            " (translated)"
        } else {
            ""
        };
        println!("\ntook {}ms{}", (et - st).as_millis(), translation_note);

        Ok(result.trim().to_string())
    }
}
