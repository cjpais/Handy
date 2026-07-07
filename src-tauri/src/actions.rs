#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::{is_microphone_access_denied, is_no_input_device_error};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, AppSettings, OutputLanguage, APPLE_INTELLIGENCE_PROVIDER_ID, PostProcessProvider};
use crate::shortcut;
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
use crate::TranscriptionCoordinator;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tauri::Manager;
use tauri::{AppHandle, Emitter};

#[derive(Clone, serde::Serialize)]
struct RecordingErrorEvent {
    error_type: String,
    detail: Option<String>,
}

/// Drop guard that notifies the [`TranscriptionCoordinator`] when the
/// transcription pipeline finishes — whether it completes normally or panics.
struct FinishGuard(AppHandle);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        let _ = self.0.emit(
            "recording-state-changed",
            RecordingStatePayload {
                mode: "idle".to_string(),
            },
        );
        if let Some(c) = self.0.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
    }
}

// Shortcut Action Trait
pub trait ShortcutAction: Send + Sync {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str);
}

// Transcribe Action
struct TranscribeAction;

/// Field name for structured output JSON schema
const TRANSCRIPTION_FIELD: &str = "transcription";

/// Strip invisible Unicode characters that some LLMs may insert
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

#[derive(Clone, serde::Serialize)]
struct MeetingSummaryPayload {
    summary: String,
    transcript: String,
}

#[derive(Clone, serde::Serialize)]
struct RecordingStatePayload {
    mode: String, // "transcribe" | "meeting" | "idle"
}

/// Build a system prompt from the user's prompt template.
/// Removes `${output}` placeholder since the transcription is sent as the user message.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

async fn post_process_transcription(settings: &AppSettings, transcription: &str) -> Option<String> {
    let provider = match settings.active_post_process_provider().cloned() {
        Some(provider) => provider,
        None => {
            debug!("Post-processing enabled but no provider is selected");
            return None;
        }
    };

    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    if model.trim().is_empty() {
        debug!(
            "Post-processing skipped because provider '{}' has no model configured",
            provider.id
        );
        return None;
    }

    let selected_prompt_id = match &settings.post_process_selected_prompt_id {
        Some(id) => id.clone(),
        None => {
            debug!("Post-processing skipped because no prompt is selected");
            return None;
        }
    };

    let prompt = match settings
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == selected_prompt_id)
    {
        Some(prompt) => prompt.prompt.clone(),
        None => {
            debug!(
                "Post-processing skipped because prompt '{}' was not found",
                selected_prompt_id
            );
            return None;
        }
    };

    if prompt.trim().is_empty() {
        debug!("Post-processing skipped because the selected prompt is empty");
        return None;
    }

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    // Disable reasoning for providers where post-processing rarely benefits from it.
    // - custom: top-level reasoning_effort (works for local OpenAI-compat servers)
    // - openrouter: nested reasoning object; exclude:true also keeps reasoning text
    //   out of the response so it can't pollute structured-output JSON parsing
    let (reasoning_effort, reasoning) = match provider.id.as_str() {
        "custom" | "google" | "ollama" => (Some("none".to_string()), None),
        "openrouter" => (
            None,
            Some(crate::llm_client::ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
        ),
        _ => (None, None),
    };

    if provider.supports_structured_output {
        debug!("Using structured outputs for provider '{}'", provider.id);

        let system_prompt = build_system_prompt(&prompt);
        let user_content = transcription.to_string();

        // Handle Apple Intelligence separately since it uses native Swift APIs
        if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                if !apple_intelligence::check_apple_intelligence_availability() {
                    debug!(
                        "Apple Intelligence selected but not currently available on this device"
                    );
                    return None;
                }

                let token_limit = model.trim().parse::<i32>().unwrap_or(0);
                return match apple_intelligence::process_text_with_system_prompt(
                    &system_prompt,
                    &user_content,
                    token_limit,
                ) {
                    Ok(result) => {
                        if result.trim().is_empty() {
                            debug!("Apple Intelligence returned an empty response");
                            None
                        } else {
                            let result = strip_invisible_chars(&result);
                            debug!(
                                "Apple Intelligence post-processing succeeded. Output length: {} chars",
                                result.len()
                            );
                            Some(result)
                        }
                    }
                    Err(err) => {
                        error!("Apple Intelligence post-processing failed: {}", err);
                        None
                    }
                };
            }

            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            {
                debug!("Apple Intelligence provider selected on unsupported platform");
                return None;
            }
        }

        // Define JSON schema for transcription output
        let description = if selected_prompt_id == "default_meeting_summary" {
            "The meeting summary and action items in English"
        } else if selected_prompt_id == "default_translate_to_english" {
            "The translated text in English"
        } else if selected_prompt_id == "default_manglish_transliteration" {
            "The transliterated text in Manglish"
        } else {
            "The cleaned and processed transcription text"
        };

        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                (TRANSCRIPTION_FIELD): {
                    "type": "string",
                    "description": description
                }
            },
            "required": [TRANSCRIPTION_FIELD],
            "additionalProperties": false
        });

        match crate::llm_client::send_chat_completion_with_schema(
            &provider,
            api_key.clone(),
            &model,
            user_content,
            Some(system_prompt),
            Some(json_schema),
            reasoning_effort.clone(),
            reasoning.clone(),
        )
        .await
        {
            Ok(Some(content)) => {
                // Parse the JSON response to extract the transcription field
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => {
                        if let Some(transcription_value) =
                            json.get(TRANSCRIPTION_FIELD).and_then(|t| t.as_str())
                        {
                            let result = strip_invisible_chars(transcription_value);
                            debug!(
                                "Structured output post-processing succeeded for provider '{}'. Output length: {} chars",
                                provider.id,
                                result.len()
                            );
                            return Some(result);
                        } else {
                            error!("Structured output response missing 'transcription' field");
                            return Some(strip_invisible_chars(&content));
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to parse structured output JSON: {}. Returning raw content.",
                            e
                        );
                        return Some(strip_invisible_chars(&content));
                    }
                }
            }
            Ok(None) => {
                error!("LLM API response has no content");
                return None;
            }
            Err(e) => {
                warn!(
                    "Structured output failed for provider '{}': {}. Falling back to legacy mode.",
                    provider.id, e
                );
                // Fall through to legacy mode below
            }
        }
    }

    // Legacy mode: Replace ${output} variable in the prompt with the actual text
    let processed_prompt = prompt.replace("${output}", transcription);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    match crate::llm_client::send_chat_completion(
        &provider,
        api_key,
        &model,
        processed_prompt,
        reasoning_effort,
        reasoning,
    )
    .await
    {
        Ok(Some(content)) => {
            let content = strip_invisible_chars(&content);
            debug!(
                "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                provider.id,
                content.len()
            );
            Some(content)
        }
        Ok(None) => {
            error!("LLM API response has no content");
            None
        }
        Err(e) => {
            error!(
                "LLM post-processing failed for provider '{}': {}. Falling back to original transcription.",
                provider.id,
                e
            );
            None
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/fallback_api_keys.rs"));

#[derive(Clone, serde::Serialize)]
struct FallbackEventPayload {
    failed_model: String,
    failed_provider: String,
    error: String,
    next_model: Option<String>,
    next_provider: Option<String>,
}

struct FallbackModel {
    provider_id: &'static str,
    model_name: &'static str,
}

const FALLBACK_CHAIN: &[FallbackModel] = &[
    // Gemini/Google
    FallbackModel { provider_id: "google", model_name: "gemini-3.5-flash" },
    FallbackModel { provider_id: "google", model_name: "gemini-3.1-flash-lite" },
    FallbackModel { provider_id: "google", model_name: "gemini-2.5-flash" },
    FallbackModel { provider_id: "google", model_name: "gemini-2.5-flash-lite" },
    FallbackModel { provider_id: "google", model_name: "gemma-4-31b-it" },
    FallbackModel { provider_id: "google", model_name: "gemma-4-26b-a4b-it" },
    
    // OpenRouter
    FallbackModel { provider_id: "openrouter", model_name: "nvidia/nemotron-3-ultra-550b-a55b:free" },
    FallbackModel { provider_id: "openrouter", model_name: "google/gemma-4-31b-it:free" },
    FallbackModel { provider_id: "openrouter", model_name: "google/gemma-4-26b-a4b-it:free" },
    
    // Groq
    FallbackModel { provider_id: "groq", model_name: "llama-3.3-70b-versatile" },
    FallbackModel { provider_id: "groq", model_name: "llama-3.1-8b-instant" },
    FallbackModel { provider_id: "groq", model_name: "mixtral-8x7b-32768" },
];

const DEFAULT_MEETING_NOTES_WITH_ACTIONS_PROMPT: &str = r#"You are a helpful assistant. Write a high-level, concise summary of the meeting transcript in English. Focus on the main topics discussed, key arguments, and decisions made. Return a JSON object with a "summary" field containing the summary text and an "action_items" field containing a list of action items.

Transcript:
${output}"#;

fn sanitize_error_msg(mut err: String) -> String {
    let keys = [
        GOOGLE_API_KEY,
        GROQ_API_KEY,
        OPENROUTER_API_KEY,
        GEMINI_API_KEY_1,
        GEMINI_API_KEY_2,
    ];
    for key in &keys {
        if !key.is_empty() {
            err = err.replace(key, "[REDACTED]");
        }
    }
    err
}

fn get_candidate_keys(settings: &AppSettings, provider_id: &str) -> Vec<String> {
    let mut keys = Vec::new();
    
    if let Some(key) = settings.post_process_api_keys.get(provider_id) {
        let key_trimmed = key.trim().to_string();
        if !key_trimmed.is_empty() {
            keys.push(key_trimmed);
        }
    }
    
    match provider_id {
        "google" => {
            for key in &[GOOGLE_API_KEY, GEMINI_API_KEY_1, GEMINI_API_KEY_2] {
                let key_trimmed = key.trim().to_string();
                if !key_trimmed.is_empty() && !keys.contains(&key_trimmed) {
                    keys.push(key_trimmed);
                }
            }
        }
        "openrouter" => {
            let key_trimmed = OPENROUTER_API_KEY.trim().to_string();
            if !key_trimmed.is_empty() && !keys.contains(&key_trimmed) {
                keys.push(key_trimmed);
            }
        }
        "groq" => {
            let key_trimmed = GROQ_API_KEY.trim().to_string();
            if !key_trimmed.is_empty() && !keys.contains(&key_trimmed) {
                keys.push(key_trimmed);
            }
        }
        _ => {}
    }
    
    keys
}

fn get_fallback_provider(settings: &AppSettings, provider_id: &str) -> PostProcessProvider {
    if let Some(provider) = settings.post_process_provider(provider_id) {
        provider.clone()
    } else {
        match provider_id {
            "google" => PostProcessProvider {
                id: "google".to_string(),
                label: "Google (Gemini)".to_string(),
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
                allow_base_url_edit: false,
                models_endpoint: Some("/models".to_string()),
                supports_structured_output: true,
            },
            "openrouter" => PostProcessProvider {
                id: "openrouter".to_string(),
                label: "OpenRouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                allow_base_url_edit: false,
                models_endpoint: Some("/models".to_string()),
                supports_structured_output: true,
            },
            "groq" => PostProcessProvider {
                id: "groq".to_string(),
                label: "Groq".to_string(),
                base_url: "https://api.groq.com/openai/v1".to_string(),
                allow_base_url_edit: false,
                models_endpoint: Some("/models".to_string()),
                supports_structured_output: false,
            },
            _ => panic!("Unknown provider"),
        }
    }
}

async fn attempt_chat_completion(
    provider: &PostProcessProvider,
    api_key: &str,
    model: &str,
    prompt_id: &str,
    prompt: &str,
    text: &str,
) -> Result<String, String> {
    let (reasoning_effort, reasoning) = match provider.id.as_str() {
        "custom" | "google" | "ollama" => (Some("none".to_string()), None),
        "openrouter" => (
            None,
            Some(crate::llm_client::ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
        ),
        _ => (None, None),
    };

    if provider.supports_structured_output {
        let system_prompt = build_system_prompt(prompt);
        let user_content = text.to_string();

        let description = if prompt_id == "default_meeting_summary" {
            "The meeting summary and action items in English"
        } else if prompt_id == "default_translate_to_english" {
            "The translated text in English"
        } else if prompt_id == "default_manglish_transliteration" {
            "The transliterated text in Manglish"
        } else {
            "The cleaned and processed transcription text"
        };

        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                (TRANSCRIPTION_FIELD): {
                    "type": "string",
                    "description": description
                }
            },
            "required": [TRANSCRIPTION_FIELD],
            "additionalProperties": false
        });

        match crate::llm_client::send_chat_completion_with_schema(
            provider,
            api_key.to_string(),
            model,
            user_content,
            Some(system_prompt),
            Some(json_schema),
            reasoning_effort.clone(),
            reasoning.clone(),
        )
        .await
        {
            Ok(Some(content)) => {
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(json) => {
                        if let Some(transcription_value) =
                            json.get(TRANSCRIPTION_FIELD).and_then(|t| t.as_str())
                        {
                            return Ok(strip_invisible_chars(transcription_value));
                        } else {
                            return Ok(strip_invisible_chars(&content));
                        }
                    }
                    Err(_) => {
                        return Ok(strip_invisible_chars(&content));
                    }
                }
            }
            Ok(None) => return Err("LLM API response has no content".to_string()),
            Err(e) => return Err(e),
        }
    }

    let processed_prompt = prompt.replace("${output}", text);
    match crate::llm_client::send_chat_completion(
        provider,
        api_key.to_string(),
        model,
        processed_prompt,
        reasoning_effort,
        reasoning,
    )
    .await
    {
        Ok(Some(content)) => Ok(strip_invisible_chars(&content)),
        Ok(None) => Err("LLM API response has no content".to_string()),
        Err(e) => Err(e),
    }
}

pub async fn run_specific_llm_prompt(
    app: &AppHandle,
    settings: &AppSettings,
    prompt_id: &str,
    text: &str,
) -> Option<String> {
    let is_meeting_summary = prompt_id == "default_meeting_summary" || prompt_id == "default_meeting_notes_with_actions";

    let prompt = match settings
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == prompt_id)
    {
        Some(prompt) => prompt.prompt.clone(),
        None => {
            if prompt_id == "default_meeting_notes_with_actions" {
                DEFAULT_MEETING_NOTES_WITH_ACTIONS_PROMPT.to_string()
            } else if prompt_id == "default_meeting_summary" {
                // Return default meeting summary prompt if not found
                "You are a helpful assistant. Write a high-level, concise summary of the meeting transcript in English.\n\nTranscript:\n${output}".to_string()
            } else {
                debug!(
                    "run_specific_llm_prompt: prompt '{}' was not found",
                    prompt_id
                );
                return None;
            }
        }
    };

    if prompt.trim().is_empty() {
        debug!("run_specific_llm_prompt: the prompt is empty");
        return None;
    }

    let mut result: Option<String> = None;

    if is_meeting_summary {
        // 1. Try configured provider/model
        let primary_provider = settings.active_post_process_provider().cloned();
        let primary_model = settings
            .post_process_models
            .get(&primary_provider.as_ref().map(|p| p.id.clone()).unwrap_or_default())
            .cloned()
            .unwrap_or_default();

        if let Some(ref provider) = primary_provider {
            if !primary_model.trim().is_empty() {
                let api_key = settings
                    .post_process_api_keys
                    .get(&provider.id)
                    .cloned()
                    .unwrap_or_default();

                // Try up to 2 times (initial + 1 retry)
                for attempt in 1..=2 {
                    debug!("Attempt {} for primary model {} (provider: {})", attempt, primary_model, provider.id);
                    match attempt_chat_completion(
                        provider,
                        &api_key,
                        &primary_model,
                        prompt_id,
                        &prompt,
                        text,
                    )
                    .await
                    {
                        Ok(res) => {
                            result = Some(res);
                            break;
                        }
                        Err(e) => {
                            warn!(
                                "Primary model call failed (attempt {}): {}",
                                attempt,
                                e
                            );
                            // Emit fallback event
                            let next_fallback = FALLBACK_CHAIN.first();
                            let sanitized_err = sanitize_error_msg(e);
                            let _ = app.emit(
                                "meeting-summary-fallback",
                                FallbackEventPayload {
                                    failed_model: primary_model.clone(),
                                    failed_provider: provider.id.clone(),
                                    error: sanitized_err,
                                    next_model: next_fallback.map(|f| f.model_name.to_string()),
                                    next_provider: next_fallback.map(|f| f.provider_id.to_string()),
                                },
                            );
                        }
                    }
                }
            }
        }

        if result.is_none() {
            warn!("Primary model failed or was not configured. Starting fallback chain.");

            // 2. Iterate through fallback chain
            for (idx, fallback) in FALLBACK_CHAIN.iter().enumerate() {
                let provider = get_fallback_provider(settings, fallback.provider_id);
                let candidate_keys = get_candidate_keys(settings, fallback.provider_id);

                if candidate_keys.is_empty() {
                    debug!(
                        "Skipping fallback model {} because no API key is available for provider {}",
                        fallback.model_name,
                        fallback.provider_id
                    );
                    continue;
                }

                let mut model_success = false;
                // Try each candidate key
                for key in &candidate_keys {
                    // Try up to 2 times for each key
                    for attempt in 1..=2 {
                        debug!(
                            "Fallback Attempt {} for model {} using provider {} (key length: {})",
                            attempt,
                            fallback.model_name,
                            fallback.provider_id,
                            key.len()
                        );
                        match attempt_chat_completion(
                            &provider,
                            key,
                            fallback.model_name,
                            prompt_id,
                            &prompt,
                            text,
                        )
                        .await
                        {
                            Ok(res) => {
                                result = Some(res);
                                model_success = true;
                                break;
                            }
                            Err(e) => {
                                warn!(
                                    "Fallback model {} failed (attempt {}): {}",
                                    fallback.model_name,
                                    attempt,
                                    e
                                );

                                // Determine next model in the chain for the event payload
                                let next_fallback = FALLBACK_CHAIN.get(idx + 1);
                                let sanitized_err = sanitize_error_msg(e);
                                let _ = app.emit(
                                    "meeting-summary-fallback",
                                    FallbackEventPayload {
                                        failed_model: fallback.model_name.to_string(),
                                        failed_provider: fallback.provider_id.to_string(),
                                        error: sanitized_err,
                                        next_model: next_fallback.map(|f| f.model_name.to_string()),
                                        next_provider: next_fallback.map(|f| f.provider_id.to_string()),
                                    },
                                );
                            }
                        }
                    }
                    if model_success {
                        break;
                    }
                }

                if model_success {
                    break;
                }
            }
        }
    } else {
        // Fallback-free path for non-meeting-summary prompts
        let provider = match settings.active_post_process_provider().cloned() {
            Some(provider) => provider,
            None => {
                debug!("run_specific_llm_prompt: no provider is selected");
                return None;
            }
        };

        let model = settings
            .post_process_models
            .get(&provider.id)
            .cloned()
            .unwrap_or_default();

        if model.trim().is_empty() {
            debug!(
                "run_specific_llm_prompt: provider '{}' has no model configured",
                provider.id
            );
            return None;
        }

        let api_key = settings
            .post_process_api_keys
            .get(&provider.id)
            .cloned()
            .unwrap_or_default();

        match attempt_chat_completion(
            &provider,
            &api_key,
            &model,
            prompt_id,
            &prompt,
            text,
        )
        .await
        {
            Ok(res) => result = Some(res),
            Err(_) => {}
        }
    }

    result
}

async fn maybe_convert_chinese_variant(
    settings: &AppSettings,
    transcription: &str,
) -> Option<String> {
    // Check if language is set to Simplified or Traditional Chinese
    let is_simplified = settings.selected_language == "zh-Hans";
    let is_traditional = settings.selected_language == "zh-Hant";

    if !is_simplified && !is_traditional {
        debug!("selected_language is not Simplified or Traditional Chinese; skipping translation");
        return None;
    }

    debug!(
        "Starting Chinese translation using OpenCC for language: {}",
        settings.selected_language
    );

    // Use OpenCC to convert based on selected language
    let config = if is_simplified {
        // Convert Traditional Chinese to Simplified Chinese
        BuiltinConfig::Tw2sp
    } else {
        // Convert Simplified Chinese to Traditional Chinese
        BuiltinConfig::S2tw
    };

    match OpenCC::from_config(config) {
        Ok(converter) => {
            let converted = converter.convert(transcription);
            debug!(
                "OpenCC translation completed. Input length: {}, Output length: {}",
                transcription.len(),
                converted.len()
            );
            Some(converted)
        }
        Err(e) => {
            error!("Failed to initialize OpenCC converter: {}. Falling back to original transcription.", e);
            None
        }
    }
}

pub(crate) struct ProcessedTranscription {
    pub final_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
}

pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    let mut final_text = transcription.to_string();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;

    if let Some(converted_text) = maybe_convert_chinese_variant(&settings, transcription).await {
        final_text = converted_text;
    }

    if post_process {
        if let Some(processed_text) = post_process_transcription(&settings, &final_text).await {
            post_processed_text = Some(processed_text.clone());
            final_text = processed_text;

            if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
                if let Some(prompt) = settings
                    .post_process_prompts
                    .iter()
                    .find(|prompt| &prompt.id == prompt_id)
                {
                    post_process_prompt = Some(prompt.prompt.clone());
                }
            }
        }
    } else if final_text != transcription {
        post_processed_text = Some(final_text.clone());
    }

    match settings.output_language {
        OutputLanguage::Malayalam => {}
        OutputLanguage::Manglish => {
            if let Some(transliterated) = run_manglish_transliteration(app, &settings, &final_text).await
            {
                post_processed_text = Some(transliterated.clone());
                final_text = transliterated;
                if post_process_prompt.is_none() {
                    post_process_prompt = settings
                        .post_process_prompts
                        .iter()
                        .find(|p| p.id == "default_manglish_transliteration")
                        .map(|p| p.prompt.clone());
                }
            }
        }
        OutputLanguage::English => {
            if let Some(translated) = run_english_translation(app, &settings, &final_text).await {
                post_processed_text = Some(translated.clone());
                final_text = translated;
                if post_process_prompt.is_none() {
                    post_process_prompt = settings
                        .post_process_prompts
                        .iter()
                        .find(|p| p.id == "default_translate_to_english")
                        .map(|p| p.prompt.clone());
                }
            }
        }
    }

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
    }
}

/// Run Manglish transliteration using the Google/Gemini provider with gemma-4-26b-a4b-it.
/// Falls back to the active post-processing provider if Google API key is not set.
async fn run_manglish_transliteration(app: &AppHandle, settings: &AppSettings, text: &str) -> Option<String> {
    let google_provider = settings.post_process_provider("google").cloned();
    let google_key = settings
        .post_process_api_keys
        .get("google")
        .cloned()
        .unwrap_or_default();

    if let Some(provider) = google_provider {
        if !google_key.trim().is_empty() {
            let prompt_text = settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == "default_manglish_transliteration")
                .map(|p| p.prompt.clone())
                .unwrap_or_else(|| {
                    "Transliterate the following Malayalam text into Manglish:\n\n${output}"
                        .to_string()
                });

            let processed_prompt = prompt_text.replace("${output}", text);
            debug!("Running Manglish transliteration with Google/gemma-4-26b-a4b-it");
            match crate::llm_client::send_chat_completion(
                &provider,
                google_key,
                "gemma-4-26b-a4b-it",
                processed_prompt,
                Some("none".to_string()),
                None,
            )
            .await
            {
                Ok(Some(result)) => return Some(strip_invisible_chars(&result)),
                Ok(None) => debug!("Manglish: Google returned empty response"),
                Err(e) => debug!(
                    "Manglish: Google failed: {}; falling back to active provider",
                    e
                ),
            }
        }
    }
    // Fallback: use active post-process provider
    run_specific_llm_prompt(app, settings, "default_manglish_transliteration", text).await
}

/// Run English translation using the Google/Gemini provider with gemma-4-26b-a4b-it.
/// Falls back to the active post-processing provider if Google API key is not set.
async fn run_english_translation(app: &AppHandle, settings: &AppSettings, text: &str) -> Option<String> {
    let google_provider = settings.post_process_provider("google").cloned();
    let google_key = settings
        .post_process_api_keys
        .get("google")
        .cloned()
        .unwrap_or_default();

    if let Some(provider) = google_provider {
        if !google_key.trim().is_empty() {
            let prompt_text = settings
                .post_process_prompts
                .iter()
                .find(|p| p.id == "default_translate_to_english")
                .map(|p| p.prompt.clone())
                .unwrap_or_else(|| {
                    "Translate the following Malayalam text into English:\n\n${output}".to_string()
                });

            let processed_prompt = prompt_text.replace("${output}", text);
            debug!("Running English translation with Google/gemma-4-26b-a4b-it");
            match crate::llm_client::send_chat_completion(
                &provider,
                google_key,
                "gemma-4-26b-a4b-it",
                processed_prompt,
                Some("none".to_string()),
                None,
            )
            .await
            {
                Ok(Some(result)) => return Some(strip_invisible_chars(&result)),
                Ok(None) => debug!("English: Google returned empty response"),
                Err(e) => debug!(
                    "English: Google failed: {}; falling back to active provider",
                    e
                ),
            }
        }
    }
    // Fallback: use active post-process provider
    run_specific_llm_prompt(app, settings, "default_translate_to_english", text).await
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        let rm = app.state::<Arc<AudioRecordingManager>>();

        // Load ASR model and VAD model in parallel
        tm.initiate_model_load();
        let rm_clone = Arc::clone(&rm);
        std::thread::spawn(move || {
            if let Err(e) = rm_clone.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });

        let binding_id = binding_id.to_string();
        change_tray_icon(app, TrayIconState::Recording);
        show_recording_overlay(app);

        // Get the microphone mode to determine audio feedback timing
        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;
        debug!("Microphone mode - always_on: {}", is_always_on);

        // Emit recording state
        let _ = app.emit(
            "recording-state-changed",
            RecordingStatePayload {
                mode: "transcribe".to_string(),
            },
        );

        let mut recording_error: Option<String> = None;
        if is_always_on {
            // Always-on mode: Play audio feedback immediately, then apply mute after sound finishes
            debug!("Always-on mode: Playing audio feedback immediately");
            let rm_clone = Arc::clone(&rm);
            let app_clone = app.clone();
            // The blocking helper exits immediately if audio feedback is disabled,
            // so we can always reuse this thread to ensure mute happens right after playback.
            std::thread::spawn(move || {
                play_feedback_sound_blocking(&app_clone, SoundType::Start);
                rm_clone.apply_mute();
            });

            if let Err(e) = rm.try_start_recording(&binding_id) {
                debug!("Recording failed: {}", e);
                recording_error = Some(e);
            }
        } else {
            // On-demand mode: Start recording first, then play audio feedback, then apply mute
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            match rm.try_start_recording(&binding_id) {
                Ok(()) => {
                    debug!("Recording started in {:?}", recording_start_time.elapsed());
                    // Small delay to ensure microphone stream is active
                    let app_clone = app.clone();
                    let rm_clone = Arc::clone(&rm);
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        debug!("Handling delayed audio feedback/mute sequence");
                        // Helper handles disabled audio feedback by returning early, so we reuse it
                        // to keep mute sequencing consistent in every mode.
                        play_feedback_sound_blocking(&app_clone, SoundType::Start);
                        rm_clone.apply_mute();
                    });
                }
                Err(e) => {
                    debug!("Failed to start recording: {}", e);
                    recording_error = Some(e);
                }
            }
        }

        if recording_error.is_none() {
            // Dynamically register the cancel shortcut in a separate task to avoid deadlock
            shortcut::register_cancel_shortcut(app);
        } else {
            // Starting failed (for example due to blocked microphone permissions).
            // Revert UI state so we don't stay stuck in the recording overlay.
            let _ = app.emit(
                "recording-state-changed",
                RecordingStatePayload {
                    mode: "idle".to_string(),
                },
            );
            utils::hide_recording_overlay(app);
            change_tray_icon(app, TrayIconState::Idle);
            if let Some(err) = recording_error {
                let error_type = if is_microphone_access_denied(&err) {
                    "microphone_permission_denied"
                } else if is_no_input_device_error(&err) {
                    "no_input_device"
                } else {
                    "unknown"
                };
                let _ = app.emit(
                    "recording-error",
                    RecordingErrorEvent {
                        error_type: error_type.to_string(),
                        detail: Some(err),
                    },
                );
            }
        }

        debug!(
            "TranscribeAction::start completed in {:?}",
            start_time.elapsed()
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        // Unregister the cancel shortcut when transcription stops
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);
        show_transcribing_overlay(app);

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let settings = get_settings(app);
        let post_process = binding_id == "transcribe_with_post_process";
        let has_llm_post_process = post_process
            || settings.output_language == OutputLanguage::Manglish
            || settings.output_language == OutputLanguage::English;

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if samples.is_empty() {
                    debug!("Recording produced no audio samples; skipping persistence");
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                } else {
                    // Save WAV concurrently with transcription
                    let sample_count = samples.len();
                    let file_name = format!("thegai-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    // Transcribe concurrently with WAV save
                    let transcription_time = Instant::now();
                    let transcription_result = tm.transcribe(samples);

                    // Await WAV save and verify
                    let wav_saved = match wav_handle.await {
                        Ok(Ok(())) => {
                            match crate::audio_toolkit::verify_wav_file(
                                &wav_path_for_verify,
                                sample_count,
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    error!("WAV verification failed: {}", e);
                                    false
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Failed to save WAV file: {}", e);
                            false
                        }
                        Err(e) => {
                            error!("WAV save task panicked: {}", e);
                            false
                        }
                    };

                    match transcription_result {
                        Ok(result) => {
                            let transcription = result.text;
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                transcription
                            );

                            if has_llm_post_process {
                                show_processing_overlay(&ah);
                            }
                            let processed =
                                process_transcription_output(&ah, &transcription, post_process)
                                    .await;

                            // Save to history if WAV was saved
                            if wav_saved {
                                if let Err(err) = hm.save_entry(
                                    file_name,
                                    transcription,
                                    has_llm_post_process,
                                    processed.post_processed_text.clone(),
                                    processed.post_process_prompt.clone(),
                                ) {
                                    error!("Failed to save history entry: {}", err);
                                }
                            }

                            if processed.final_text.is_empty() {
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                            } else {
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let final_text = processed.final_text;
                                ah.run_on_main_thread(move || {
                                    match utils::paste(final_text, ah_clone.clone()) {
                                        Ok(()) => debug!(
                                            "Text pasted successfully in {:?}",
                                            paste_time.elapsed()
                                        ),
                                        Err(e) => {
                                            error!("Failed to paste transcription: {}", e);
                                            let _ = ah_clone.emit("paste-error", ());
                                        }
                                    }
                                    utils::hide_recording_overlay(&ah_clone);
                                    change_tray_icon(&ah_clone, TrayIconState::Idle);
                                })
                                .unwrap_or_else(|e| {
                                    error!("Failed to run paste on main thread: {:?}", e);
                                    utils::hide_recording_overlay(&ah);
                                    change_tray_icon(&ah, TrayIconState::Idle);
                                });
                            }
                        }
                        Err(err) => {
                            debug!("Global Shortcut Transcription error: {}", err);
                            // Save entry with empty text so user can retry
                            if wav_saved {
                                if let Err(save_err) = hm.save_entry(
                                    file_name,
                                    String::new(),
                                    post_process,
                                    None,
                                    None,
                                ) {
                                    error!("Failed to save failed history entry: {}", save_err);
                                }
                            }
                            utils::hide_recording_overlay(&ah);
                            change_tray_icon(&ah, TrayIconState::Idle);
                        }
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                utils::hide_recording_overlay(&ah);
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!(
            "TranscribeAction::stop completed in {:?}",
            stop_time.elapsed()
        );
    }
}

// Cancel Action
struct CancelAction;

impl ShortcutAction for CancelAction {
    fn start(&self, app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        utils::cancel_current_operation(app);
    }

    fn stop(&self, _app: &AppHandle, _binding_id: &str, _shortcut_str: &str) {
        // Nothing to do on stop for cancel
    }
}

// Meeting Action
struct MeetingAction;

const MEETING_MIN_RECORDING_SECONDS: usize = 30;
const MEETING_SAMPLE_RATE: usize = 16_000;
const MEETING_MIN_SAMPLE_COUNT: usize = MEETING_MIN_RECORDING_SECONDS * MEETING_SAMPLE_RATE;

impl ShortcutAction for MeetingAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("MeetingAction::start called for binding: {}", binding_id);

        let rm = app.state::<Arc<AudioRecordingManager>>();

        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;

        let mut recording_error: Option<String> = None;
        if is_always_on {
            let rm_clone = Arc::clone(&rm);
            let app_clone = app.clone();
            std::thread::spawn(move || {
                play_feedback_sound_blocking(&app_clone, SoundType::Start);
                rm_clone.apply_mute();
            });

            if let Err(e) = rm.try_start_recording("meeting") {
                debug!("Recording failed: {}", e);
                recording_error = Some(e);
            }
        } else {
            let recording_start_time = Instant::now();
            match rm.try_start_recording("meeting") {
                Ok(()) => {
                    debug!("Recording started in {:?}", recording_start_time.elapsed());
                    let app_clone = app.clone();
                    let rm_clone = Arc::clone(&rm);
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        play_feedback_sound_blocking(&app_clone, SoundType::Start);
                        rm_clone.apply_mute();
                    });
                }
                Err(e) => {
                    debug!("Failed to start recording: {}", e);
                    recording_error = Some(e);
                }
            }
        }

        if recording_error.is_none() {
            change_tray_icon(app, TrayIconState::Recording);
            crate::overlay::show_meeting_recording_overlay(app);

            // Emit recording state
            let _ = app.emit(
                "recording-state-changed",
                RecordingStatePayload {
                    mode: "meeting".to_string(),
                },
            );

            shortcut::register_cancel_shortcut(app);
        } else {
            let _ = app.emit(
                "recording-state-changed",
                RecordingStatePayload {
                    mode: "idle".to_string(),
                },
            );
            crate::overlay::hide_meeting_prompt_window(app);
            change_tray_icon(app, TrayIconState::Idle);
            if let Some(err) = recording_error {
                let error_type = if is_microphone_access_denied(&err) {
                    "microphone_permission_denied"
                } else if is_no_input_device_error(&err) {
                    "no_input_device"
                } else {
                    "unknown"
                };
                let _ = app.emit(
                    "recording-error",
                    RecordingErrorEvent {
                        error_type: error_type.to_string(),
                        detail: Some(err),
                    },
                );
            }
        }

        debug!(
            "MeetingAction::start completed in {:?}",
            start_time.elapsed()
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("MeetingAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        change_tray_icon(app, TrayIconState::Transcribing);

        rm.remove_mute();
        play_feedback_sound(app, SoundType::Stop);

        let binding_id_str = binding_id.to_string();
        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            debug!(
                "Starting async meeting transcription task for binding: {}",
                binding_id_str
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording("meeting") {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if samples.len() < MEETING_MIN_SAMPLE_COUNT {
                    debug!(
                        "Meeting recording shorter than {} seconds ({} samples); discarding",
                        MEETING_MIN_RECORDING_SECONDS,
                        samples.len()
                    );
                    change_tray_icon(&ah, TrayIconState::Idle);
                    crate::overlay::show_meeting_discarded_overlay(&ah);
                } else {
                    crate::overlay::show_meeting_stopped_overlay(&ah);
                    
                    // Save WAV concurrently with transcription
                    let sample_count = samples.len();
                    let file_name = format!("thegai-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    let settings = get_settings(&ah);
                    let prompt_id = if settings.google_oauth_token.is_some() {
                        "default_meeting_notes_with_actions"
                    } else {
                        "default_meeting_summary"
                    };

                    // Transcribe concurrently with WAV save
                    let transcription_time = Instant::now();

                    let tm_clone = tm.clone();
                    let samples_for_transcribe = samples.clone();
                    let transcribe_handle = tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = tm_clone.load_model_if_different("thegav1") {
                            error!("Failed to load ThegaV1 model for meeting transcription: {}", e);
                            return Err(anyhow::anyhow!("Failed to load ThegaV1 model: {}", e));
                        }
                        let res = tm_clone.transcribe(samples_for_transcribe);
                        if let Err(e) = tm_clone.unload_model() {
                            warn!("Failed to unload ThegaV1 model after meeting transcription: {}", e);
                        }
                        res
                    });

                    // Await WAV save and verify
                    let wav_saved = match wav_handle.await {
                        Ok(Ok(())) => {
                            match crate::audio_toolkit::verify_wav_file(
                                &wav_path_for_verify,
                                sample_count,
                            ) {
                                Ok(()) => true,
                                Err(e) => {
                                    error!("WAV verification failed: {}", e);
                                    false
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Failed to save WAV file: {}", e);
                            false
                        }
                        Err(e) => {
                            error!("WAV save task panicked: {}", e);
                            false
                        }
                    };

                    let history_entry_id = if wav_saved {
                        match hm.save_entry(
                            file_name.clone(),
                            String::new(),
                            true,
                            None,
                            Some(prompt_id.to_string()),
                        ) {
                            Ok(entry) => Some(entry.id),
                            Err(err) => {
                                error!("Failed to save pending meeting history entry: {}", err);
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let transcription_result = match transcribe_handle.await {
                        Ok(res) => res,
                        Err(e) => Err(anyhow::anyhow!("Transcription task panicked: {}", e)),
                    };

                    match transcription_result {
                        Ok(result) => {
                            let transcription = result.text.clone();
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                transcription
                            );

                            let summary_opt =
                                run_specific_llm_prompt(&ah, &settings, prompt_id, &transcription).await;

                            let display_summary = if prompt_id
                                == "default_meeting_notes_with_actions"
                            {
                                summary_opt
                                    .as_ref()
                                    .and_then(|json_str| {
                                        serde_json::from_str::<serde_json::Value>(json_str)
                                            .ok()
                                            .and_then(|v| {
                                                v.get("summary")
                                                    .and_then(|s| s.as_str())
                                                    .map(|s| s.to_string())
                                            })
                                    })
                                    .unwrap_or_else(|| {
                                        summary_opt.clone().unwrap_or_else(|| transcription.clone())
                                    })
                            } else {
                                summary_opt.clone().unwrap_or_else(|| transcription.clone())
                            };

                            if let Some(entry_id) = history_entry_id {
                                if let Err(err) = hm.update_transcription(
                                    entry_id,
                                    transcription.clone(),
                                    summary_opt.clone(),
                                    Some(prompt_id.to_string()),
                                ) {
                                    error!("Failed to update meeting history entry: {}", err);
                                }
                            }

                            if display_summary.is_empty() {
                                change_tray_icon(&ah, TrayIconState::Idle);
                            } else {
                                let _ = ah.emit(
                                    "meeting-summary",
                                    MeetingSummaryPayload {
                                        summary: display_summary,
                                        transcript: transcription,
                                    },
                                );
                                change_tray_icon(&ah, TrayIconState::Idle);
                            }
                        }
                        Err(err) => {
                            debug!("Global Shortcut Transcription error: {}", err);
                            if history_entry_id.is_none() && wav_saved {
                                error!("Meeting WAV was saved but no history placeholder exists");
                            }
                            change_tray_icon(&ah, TrayIconState::Idle);
                        }
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                change_tray_icon(&ah, TrayIconState::Idle);
            }
        });

        debug!("MeetingAction::stop completed in {:?}", stop_time.elapsed());
    }
}
// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Started - {} (App: {})",
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Stopped - {} (App: {})",
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }
}

// Static Action Map
pub static ACTION_MAP: Lazy<HashMap<String, Arc<dyn ShortcutAction>>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "transcribe".to_string(),
        Arc::new(TranscribeAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cancel".to_string(),
        Arc::new(CancelAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "meeting".to_string(),
        Arc::new(MeetingAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});
