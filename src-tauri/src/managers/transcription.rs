use crate::managers::model::ModelManager;
use crate::settings::get_settings;
use anyhow::Result;
use natural::phonetics::soundex;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use strsim::levenshtein;
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
    app_handle: AppHandle,
    current_model_id: Mutex<Option<String>>,
}

fn correct_words(text: &str, correct_words: &[String]) -> String {
    if correct_words.is_empty() {
        return text.to_string();
    }

    // Pre-compute lowercase versions to avoid repeated allocations
    let correct_words_lower: Vec<String> = correct_words.iter()
        .map(|w| w.to_lowercase())
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut corrected_words = Vec::new();

    for word in words {
        let cleaned_word = word.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
        
        if cleaned_word.is_empty() {
            corrected_words.push(word.to_string());
            continue;
        }

        // Skip extremely long words to avoid performance issues
        if cleaned_word.len() > 50 {
            corrected_words.push(word.to_string());
            continue;
        }

        let mut best_match: Option<&String> = None;
        let mut best_score = f64::MAX;

        for (i, correct_word_lower) in correct_words_lower.iter().enumerate() {
            // Skip if lengths are too different (optimization)
            let len_diff = (cleaned_word.len() as i32 - correct_word_lower.len() as i32).abs();
            if len_diff > 5 {
                continue;
            }
            
            // Calculate Levenshtein distance (normalized by length)
            let levenshtein_dist = levenshtein(&cleaned_word, correct_word_lower);
            let max_len = cleaned_word.len().max(correct_word_lower.len()) as f64;
            let levenshtein_score = if max_len > 0.0 { levenshtein_dist as f64 / max_len } else { 1.0 };
            
            // Calculate phonetic similarity using Soundex
            let phonetic_match = soundex(&cleaned_word, correct_word_lower);
            
            // Combine scores: favor phonetic matches, but also consider string similarity
            let combined_score = if phonetic_match {
                levenshtein_score * 0.3  // Give significant boost to phonetic matches
            } else {
                levenshtein_score
            };
            
            // Accept if the score is good enough (threshold: 0.4 for good matches)
            if combined_score < 0.4 && combined_score < best_score {
                best_match = Some(&correct_words[i]);
                best_score = combined_score;
            }
        }

        if let Some(replacement) = best_match {
            // Preserve the original case pattern as much as possible
            let corrected = if word.chars().all(|c| c.is_uppercase()) {
                replacement.to_uppercase()
            } else if word.chars().next().map_or(false, |c| c.is_uppercase()) {
                let mut chars: Vec<char> = replacement.chars().collect();
                if let Some(first_char) = chars.get_mut(0) {
                    *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
                }
                chars.into_iter().collect()
            } else {
                replacement.clone()
            };
            
            // Preserve punctuation from original word - optimized version
            let prefix_end = word.chars().take_while(|c| !c.is_alphabetic()).count();
            let suffix_start = word.char_indices().rev().take_while(|(_, c)| !c.is_alphabetic()).count();
            
            let original_prefix = if prefix_end > 0 { &word[..prefix_end] } else { "" };
            let original_suffix = if suffix_start > 0 { &word[word.len() - suffix_start..] } else { "" };
                
            corrected_words.push(format!("{}{}{}", original_prefix, corrected, original_suffix));
        } else {
            corrected_words.push(word.to_string());
        }
    }

    corrected_words.join(" ")
}

impl TranscriptionManager {
    pub fn new(app: &App, model_manager: Arc<ModelManager>) -> Result<Self> {
        let app_handle = app.app_handle().clone();

        let manager = Self {
            state: Mutex::new(None),
            context: Mutex::new(None),
            model_manager,
            app_handle: app_handle.clone(),
            current_model_id: Mutex::new(None),
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

    pub fn get_current_model(&self) -> Option<String> {
        let current_model = self.current_model_id.lock().unwrap();
        current_model.clone()
    }

    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        let st = std::time::Instant::now();

        let mut result = String::new();
        println!("Audio vector length: {}", audio.len());

        if audio.len() == 0 {
            println!("Empty audio vector");
            return Ok(result);
        }

        let mut state_guard = self.state.lock().unwrap();
        let state = state_guard.as_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "No model loaded. Please download and select a model from settings first."
            )
        })?;

        // Get current settings to check translation preference
        let settings = get_settings(&self.app_handle);

        // Initialize parameters
        let mut params = FullParams::new(SamplingStrategy::default());
        let language = Some(settings.selected_language.as_str());
        params.set_language(language);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);

        // Enable translation to English if requested
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

        // Apply word correction if correct words are configured
        let corrected_result = if !settings.correct_words.is_empty() {
            correct_words(&result, &settings.correct_words)
        } else {
            result
        };

        let et = std::time::Instant::now();
        let translation_note = if settings.translate_to_english {
            " (translated)"
        } else {
            ""
        };
        println!("\ntook {}ms{}", (et - st).as_millis(), translation_note);

        Ok(corrected_result.trim().to_string())
    }
}
