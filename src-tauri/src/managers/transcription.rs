use crate::audio_toolkit::apply_custom_words;
use chrono::Local;
use crate::managers::model::{EngineType, ModelManager};
use crate::settings::{get_settings, ModelUnloadTimeout};
use anyhow::Result;
use log::{debug, error, info, warn};
use regex::Regex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter};
use transcribe_rs::{
    engines::{
        parakeet::{
            ParakeetEngine, ParakeetInferenceParams, ParakeetModelParams, TimestampGranularity,
        },
        whisper::{WhisperEngine, WhisperInferenceParams},
    },
    TranscriptionEngine,
};

enum MagicTransformation {
    Lowercase,
    Uppercase,
    NoSpace,
    Capitalize,
}

fn parse_magic_transformations(s: &str) -> (Vec<MagicTransformation>, String) {
    let mut transformations = Vec::new();
    let mut clean_s = s.to_string();

    // Handle dynamic content tags
    if clean_s.contains("[date]") {
        let now = Local::now();
        clean_s = clean_s.replace("[date]", &now.format("%Y-%m-%d").to_string());
    }
    if clean_s.contains("[time]") {
        let now = Local::now();
        clean_s = clean_s.replace("[time]", &now.format("%H:%M").to_string());
    }
    
    // Regex to find tags like [lowercase]
    if let Ok(re) = regex::Regex::new(r"\[([a-zA-Z]+)\]") {
        let mut tags_to_remove = Vec::new();
        for cap in re.captures_iter(&clean_s.clone()) {
            match &cap[1] {
                "lowercase" | "lowcase" => {
                    transformations.push(MagicTransformation::Lowercase);
                    tags_to_remove.push(cap[0].to_string());
                },
                "uppercase" => {
                    transformations.push(MagicTransformation::Uppercase);
                    tags_to_remove.push(cap[0].to_string());
                },
                "nospace" => {
                    transformations.push(MagicTransformation::NoSpace);
                    tags_to_remove.push(cap[0].to_string());
                },
                "capitalize" => {
                    transformations.push(MagicTransformation::Capitalize);
                    tags_to_remove.push(cap[0].to_string());
                },
                _ => {}
            }
        }
        for tag in tags_to_remove {
            clean_s = clean_s.replace(&tag, "");
        }
    }
    (transformations, clean_s)
}

fn apply_transformations(text: &mut String, transformations: &[MagicTransformation]) {
    for t in transformations {
        match t {
            MagicTransformation::Lowercase => *text = text.to_lowercase(),
            MagicTransformation::Uppercase => *text = text.to_uppercase(),
            MagicTransformation::NoSpace => *text = text.replace(" ", ""),
            MagicTransformation::Capitalize => {
                let mut result = String::new();
                let mut capitalize_next = true;
                for c in text.chars() {
                    if c.is_whitespace() || c.is_ascii_punctuation() {
                        capitalize_next = true;
                        result.push(c);
                    } else if capitalize_next {
                        result.push(c.to_uppercase().next().unwrap_or(c));
                        capitalize_next = false;
                    } else {
                        result.push(c);
                    }
                }
                *text = result;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub error: Option<String>,
}

enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
}

#[derive(Clone)]
pub struct TranscriptionManager {
    engine: Arc<Mutex<Option<LoadedEngine>>>,
    model_manager: Arc<ModelManager>,
    app_handle: AppHandle,
    current_model_id: Arc<Mutex<Option<String>>>,
    last_activity: Arc<AtomicU64>,
    shutdown_signal: Arc<AtomicBool>,
    watcher_handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    is_loading: Arc<Mutex<bool>>,
    loading_condvar: Arc<Condvar>,
}

impl TranscriptionManager {
    pub fn new(app_handle: &AppHandle, model_manager: Arc<ModelManager>) -> Result<Self> {
        let manager = Self {
            engine: Arc::new(Mutex::new(None)),
            model_manager,
            app_handle: app_handle.clone(),
            current_model_id: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(AtomicU64::new(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            )),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            watcher_handle: Arc::new(Mutex::new(None)),
            is_loading: Arc::new(Mutex::new(false)),
            loading_condvar: Arc::new(Condvar::new()),
        };

        // Start the idle watcher
        {
            let app_handle_cloned = app_handle.clone();
            let manager_cloned = manager.clone();
            let shutdown_signal = manager.shutdown_signal.clone();
            let handle = thread::spawn(move || {
                while !shutdown_signal.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(10)); // Check every 10 seconds

                    // Check shutdown signal again after sleep
                    if shutdown_signal.load(Ordering::Relaxed) {
                        break;
                    }

                    let settings = get_settings(&app_handle_cloned);
                    let timeout_seconds = settings.model_unload_timeout.to_seconds();

                    if let Some(limit_seconds) = timeout_seconds {
                        // Skip polling-based unloading for immediate timeout since it's handled directly in transcribe()
                        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately {
                            continue;
                        }

                        let last = manager_cloned.last_activity.load(Ordering::Relaxed);
                        let now_ms = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        if now_ms.saturating_sub(last) > limit_seconds * 1000 {
                            // idle -> unload
                            if manager_cloned.is_model_loaded() {
                                let unload_start = std::time::Instant::now();
                                debug!("Starting to unload model due to inactivity");

                                if let Ok(()) = manager_cloned.unload_model() {
                                    let _ = app_handle_cloned.emit(
                                        "model-state-changed",
                                        ModelStateEvent {
                                            event_type: "unloaded".to_string(),
                                            model_id: None,
                                            model_name: None,
                                            error: None,
                                        },
                                    );
                                    let unload_duration = unload_start.elapsed();
                                    debug!(
                                        "Model unloaded due to inactivity (took {}ms)",
                                        unload_duration.as_millis()
                                    );
                                }
                            }
                        }
                    }
                }
                debug!("Idle watcher thread shutting down gracefully");
            });
            *manager.watcher_handle.lock().unwrap() = Some(handle);
        }

        Ok(manager)
    }

    pub fn is_model_loaded(&self) -> bool {
        let engine = self.engine.lock().unwrap();
        engine.is_some()
    }

    pub fn unload_model(&self) -> Result<()> {
        let unload_start = std::time::Instant::now();
        debug!("Starting to unload model");

        {
            let mut engine = self.engine.lock().unwrap();
            if let Some(ref mut loaded_engine) = *engine {
                match loaded_engine {
                    LoadedEngine::Whisper(ref mut whisper) => whisper.unload_model(),
                    LoadedEngine::Parakeet(ref mut parakeet) => parakeet.unload_model(),
                }
            }
            *engine = None; // Drop the engine to free memory
        }
        {
            let mut current_model = self.current_model_id.lock().unwrap();
            *current_model = None;
        }

        // Emit unloaded event
        let _ = self.app_handle.emit(
            "model-state-changed",
            ModelStateEvent {
                event_type: "unloaded".to_string(),
                model_id: None,
                model_name: None,
                error: None,
            },
        );

        let unload_duration = unload_start.elapsed();
        debug!(
            "Model unloaded manually (took {}ms)",
            unload_duration.as_millis()
        );
        Ok(())
    }

    pub fn load_model(&self, model_id: &str) -> Result<()> {
        let load_start = std::time::Instant::now();
        debug!("Starting to load model: {}", model_id);

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

        // Create appropriate engine based on model type
        let loaded_engine = match model_info.engine_type {
            EngineType::Whisper => {
                let mut engine = WhisperEngine::new();
                engine.load_model(&model_path).map_err(|e| {
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
                LoadedEngine::Whisper(engine)
            }
            EngineType::Parakeet => {
                let mut engine = ParakeetEngine::new();
                engine
                    .load_model_with_params(&model_path, ParakeetModelParams::int8())
                    .map_err(|e| {
                        let error_msg =
                            format!("Failed to load parakeet model {}: {}", model_id, e);
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
                LoadedEngine::Parakeet(engine)
            }
        };

        // Update the current engine and model ID
        {
            let mut engine = self.engine.lock().unwrap();
            *engine = Some(loaded_engine);
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

        let load_duration = load_start.elapsed();
        debug!(
            "Successfully loaded transcription model: {} (took {}ms)",
            model_id,
            load_duration.as_millis()
        );
        Ok(())
    }

    /// Kicks off the model loading in a background thread if it's not already loaded
    pub fn initiate_model_load(&self) {
        let mut is_loading = self.is_loading.lock().unwrap();
        if *is_loading || self.is_model_loaded() {
            return;
        }

        *is_loading = true;
        let self_clone = self.clone();
        thread::spawn(move || {
            let settings = get_settings(&self_clone.app_handle);
            if let Err(e) = self_clone.load_model(&settings.selected_model) {
                error!("Failed to load model: {}", e);
            }
            let mut is_loading = self_clone.is_loading.lock().unwrap();
            *is_loading = false;
            self_clone.loading_condvar.notify_all();
        });
    }

    pub fn get_current_model(&self) -> Option<String> {
        let current_model = self.current_model_id.lock().unwrap();
        current_model.clone()
    }

    pub fn transcribe(&self, audio: Vec<f32>) -> Result<String> {
        // Update last activity timestamp
        self.last_activity.store(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::Relaxed,
        );

        let st = std::time::Instant::now();

        debug!("Audio vector length: {}", audio.len());

        if audio.len() == 0 {
            debug!("Empty audio vector");
            return Ok(String::new());
        }

        // Check if model is loaded, if not try to load it
        {
            // If the model is loading, wait for it to complete.
            let mut is_loading = self.is_loading.lock().unwrap();
            while *is_loading {
                is_loading = self.loading_condvar.wait(is_loading).unwrap();
            }

            let engine_guard = self.engine.lock().unwrap();
            if engine_guard.is_none() {
                return Err(anyhow::anyhow!("Model is not loaded for transcription."));
            }
        }

        // Get current settings for configuration
        let settings = get_settings(&self.app_handle);

        // Perform transcription with the appropriate engine
        let result = {
            let mut engine_guard = self.engine.lock().unwrap();
            let engine = engine_guard.as_mut().ok_or_else(|| {
                anyhow::anyhow!(
                    "Model failed to load after auto-load attempt. Please check your model settings."
                )
            })?;

            match engine {
                LoadedEngine::Whisper(whisper_engine) => {
                    // Normalize language code for Whisper
                    // Convert zh-Hans and zh-Hant to zh since Whisper uses ISO 639-1 codes
                    let whisper_language = if settings.selected_language == "auto" {
                        None
                    } else {
                        let normalized = if settings.selected_language == "zh-Hans"
                            || settings.selected_language == "zh-Hant"
                        {
                            "zh".to_string()
                        } else {
                            settings.selected_language.clone()
                        };
                        Some(normalized)
                    };

                    let params = WhisperInferenceParams {
                        language: whisper_language,
                        translate: settings.translate_to_english,
                        ..Default::default()
                    };

                    whisper_engine
                        .transcribe_samples(audio, Some(params))
                        .map_err(|e| anyhow::anyhow!("Whisper transcription failed: {}", e))?
                }
                LoadedEngine::Parakeet(parakeet_engine) => {
                    let params = ParakeetInferenceParams {
                        timestamp_granularity: TimestampGranularity::Segment,
                        ..Default::default()
                    };

                    parakeet_engine
                        .transcribe_samples(audio, Some(params))
                        .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {}", e))?
                }
            }
        };

        // Apply word correction if custom words are configured
        let corrected_result = if !settings.custom_words.is_empty() {
            apply_custom_words(
                &result.text,
                &settings.custom_words,
                settings.word_correction_threshold,
            )
        } else {
            result.text
        };

        // Apply replacements
        let mut replaced_result = corrected_result;
        let mut global_transformations = Vec::new();

        for replacement in &settings.replacements {
            let search_pattern = if replacement.is_regex {
                replacement.search.clone()
            } else {
                // Build accent-insensitive regex pattern
                let mut pattern = String::from("(?i)");
                for c in replacement.search.chars() {
                    match c {
                        'a' | 'A' | 'à' | 'À' | 'á' | 'Á' | 'â' | 'Â' | 'ã' | 'Ã' | 'ä' | 'Ä' | 'å' | 'Å' => pattern.push_str("[aàáâãäå]"),
                        'e' | 'E' | 'é' | 'É' | 'è' | 'È' | 'ê' | 'Ê' | 'ë' | 'Ë' => pattern.push_str("[eéèêë]"),
                        'i' | 'I' | 'ì' | 'Ì' | 'í' | 'Í' | 'î' | 'Î' | 'ï' | 'Ï' => pattern.push_str("[iìíîï]"),
                        'o' | 'O' | 'ò' | 'Ò' | 'ó' | 'Ó' | 'ô' | 'Ô' | 'õ' | 'Õ' | 'ö' | 'Ö' => pattern.push_str("[oòóôõö]"),
                        'u' | 'U' | 'ù' | 'Ù' | 'ú' | 'Ú' | 'û' | 'Û' | 'ü' | 'Ü' => pattern.push_str("[uùúûü]"),
                        'y' | 'Y' | 'ý' | 'Ý' | 'ÿ' | 'Ÿ' => pattern.push_str("[yýÿ]"),
                        'c' | 'C' | 'ç' | 'Ç' => pattern.push_str("[cç]"),
                        'n' | 'N' | 'ñ' | 'Ñ' => pattern.push_str("[nñ]"),
                        _ => pattern.push_str(&regex::escape(&c.to_string())),
                    }
                }
                pattern
            };

            let re = match regex::Regex::new(&search_pattern) {
                Ok(re) => re,
                Err(_) => continue, // Skip invalid regex
            };

            // Check for magic tags
            let (magic_transformations, clean_replace_with) = parse_magic_transformations(&replacement.replace);
            let has_magic = !magic_transformations.is_empty();

            // Handle \n in replacement string
            let replace_with = clean_replace_with.replace("\\n", "\n");

            if replacement.remove_surrounding_punctuation 
                || replacement.capitalization_rule != crate::settings::CapitalizationRule::None 
                || has_magic
            {
                let mut new_text = String::new();
                let mut last_end = 0;
                
                // Find all matches
                let matches: Vec<_> = re.find_iter(&replaced_result).collect();
                
                if matches.is_empty() {
                    continue;
                }

                // If we have matches and magic transformations, add them to global list
                if has_magic {
                    global_transformations.extend(magic_transformations);
                }

                for mat in matches {
                    let start = mat.start();
                    let end = mat.end();

                    // If we've already processed past this match (due to lookahead), skip it
                    if start < last_end {
                        continue;
                    }

                    // 1. Handle text BEFORE the match
                    let prefix = &replaced_result[last_end..start];
                    
                    if replacement.remove_surrounding_punctuation {
                        // Trim trailing punctuation and whitespace
                        // Also handles French punctuation and common symbols
                        let trimmed_prefix = prefix.trim_end_matches(|c: char| 
                            c.is_ascii_punctuation() || 
                            c.is_whitespace() || 
                            ['«', '»', '—', '…', '’', '“', '”'].contains(&c)
                        );
                        new_text.push_str(trimmed_prefix);
                        
                        // Add space if needed
                        if !trimmed_prefix.is_empty() {
                            // Don't add space if replacement starts with punctuation that shouldn't have one
                            let no_space_before = replace_with.chars().next().map_or(false, |c| 
                                ['.', ',', ')', ']', '}', '…', '’'].contains(&c)
                            );
                            
                            if !no_space_before {
                                new_text.push(' ');
                            }
                        }
                    } else {
                        new_text.push_str(prefix);
                    }

                    // 2. Append REPLACEMENT
                    new_text.push_str(&replace_with);

                    // 3. Handle text AFTER the match
                    let mut current_pos = end;
                    
                    if replacement.remove_surrounding_punctuation {
                        // Skip immediate punctuation/whitespace
                        let remainder = &replaced_result[current_pos..];
                        let skipped_len = remainder.chars()
                            .take_while(|c| 
                                c.is_ascii_punctuation() || 
                                c.is_whitespace() || 
                                ['«', '»', '—', '…', '’', '“', '”'].contains(&c)
                            )
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        current_pos += skipped_len;
                        
                        // Add space if replacement is a word
                        if replace_with.chars().last().map_or(false, |c| c.is_alphanumeric()) {
                             new_text.push(' ');
                        }
                    }

                    // 4. Handle CAPITALIZATION
                    let remainder = &replaced_result[current_pos..];
                    let mut chars = remainder.char_indices();
                    
                    if let Some((_, c)) = chars.next() {
                        if c.is_whitespace() {
                            new_text.push(c);
                            // Look at next char for capitalization
                            if let Some((_, c2)) = chars.next() {
                                let char_to_append = match replacement.capitalization_rule {
                                    crate::settings::CapitalizationRule::ForceUppercase => c2.to_uppercase().to_string(),
                                    crate::settings::CapitalizationRule::ForceLowercase => c2.to_lowercase().to_string(),
                                    _ => c2.to_string(),
                                };
                                new_text.push_str(&char_to_append);
                                current_pos += c.len_utf8() + c2.len_utf8();
                            } else {
                                current_pos += c.len_utf8();
                            }
                        } else {
                             let char_to_append = match replacement.capitalization_rule {
                                crate::settings::CapitalizationRule::ForceUppercase => c.to_uppercase().to_string(),
                                crate::settings::CapitalizationRule::ForceLowercase => c.to_lowercase().to_string(),
                                _ => c.to_string(),
                            };
                            new_text.push_str(&char_to_append);
                            current_pos += c.len_utf8();
                        }
                    }
                    
                    last_end = current_pos;
                }
                
                new_text.push_str(&replaced_result[last_end..]);
                replaced_result = new_text;

            } else {
                // Simple replacement but case-insensitive
                // We can use regex replace_all
                
                // If we have magic tags, we need to check if a match occurs to trigger them
                if has_magic {
                    if re.is_match(&replaced_result) {
                        global_transformations.extend(magic_transformations);
                        replaced_result = re.replace_all(&replaced_result, &replace_with).to_string();
                    }
                } else {
                    replaced_result = re.replace_all(&replaced_result, &replace_with).to_string();
                }
            }
        }

        // Apply global transformations
        apply_transformations(&mut replaced_result, &global_transformations);


        let et = std::time::Instant::now();
        let translation_note = if settings.translate_to_english {
            " (translated)"
        } else {
            ""
        };
        info!(
            "Transcription completed in {}ms{}",
            (et - st).as_millis(),
            translation_note
        );

        let final_result = replaced_result.trim().to_string();

        if final_result.is_empty() {
            info!("Transcription result is empty");
        } else {
            info!("Transcription result: {}", final_result);
        }

        // Check if we should immediately unload the model after transcription
        if settings.model_unload_timeout == ModelUnloadTimeout::Immediately {
            info!("Immediately unloading model after transcription");
            if let Err(e) = self.unload_model() {
                error!("Failed to immediately unload model: {}", e);
            }
        }

        Ok(final_result)
    }
}

impl Drop for TranscriptionManager {
    fn drop(&mut self) {
        debug!("Shutting down TranscriptionManager");

        // Signal the watcher thread to shutdown
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // Wait for the thread to finish gracefully
        if let Some(handle) = self.watcher_handle.lock().unwrap().take() {
            if let Err(e) = handle.join() {
                warn!("Failed to join idle watcher thread: {:?}", e);
            } else {
                debug!("Idle watcher thread joined successfully");
            }
        }
    }
}
