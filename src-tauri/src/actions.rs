#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::{is_microphone_access_denied, is_no_input_device_error, VadPolicy};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID};
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
struct TranscribeAction {
    post_process: bool,
}

/// Field name for structured output JSON schema
const TRANSCRIPTION_FIELD: &str = "transcription";

/// Strip invisible Unicode characters that some LLMs may insert
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

/// Build a system prompt from the user's prompt template.
/// Removes `${output}` placeholder since the transcription is sent as the user message.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

/// Returns `true` when a transcription has no meaningful content to
/// post-process (empty or whitespace-only). Used to skip the post-processing
/// LLM call when nothing was actually transcribed, which would otherwise make
/// the model reply with an error message such as "you need to provide the
/// transcription".
fn is_blank_transcription(transcription: &str) -> bool {
    transcription.trim().is_empty()
}

async fn post_process_transcription(settings: &AppSettings, transcription: &str) -> Option<String> {
    if is_blank_transcription(transcription) {
        debug!("Post-processing skipped because the transcription is empty");
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

    // ponytail: frontmost app read at post-process time, not recording start;
    // good enough since the target app keeps focus during transcription
    let prompt = prompt.replace(
        "${app}",
        &crate::utils::frontmost_app_name().unwrap_or_else(|| "unknown".to_string()),
    );

    let system_prompt = build_system_prompt(&prompt);
    let legacy_prompt = prompt.replace("${output}", transcription);
    run_llm(
        settings,
        system_prompt,
        transcription.to_string(),
        legacy_prompt,
    )
    .await
}

/// Send a system+user request through the configured post-processing provider
/// and extract the `transcription` field from the structured response.
/// Non-JSON content (legacy fallback, Apple Intelligence) passes through raw.
async fn run_llm(
    settings: &AppSettings,
    system_prompt: String,
    user_content: String,
    legacy_prompt: String,
) -> Option<String> {
    let json_schema = serde_json::json!({
        "type": "object",
        "properties": {
            (TRANSCRIPTION_FIELD): {
                "type": "string",
                "description": "The cleaned and processed transcription text"
            }
        },
        "required": [TRANSCRIPTION_FIELD],
        "additionalProperties": false
    });
    let content = run_llm_raw(
        settings,
        system_prompt,
        user_content,
        legacy_prompt,
        json_schema,
    )
    .await?;
    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(json) => match json.get(TRANSCRIPTION_FIELD).and_then(|t| t.as_str()) {
            Some(text) => Some(text.to_string()),
            None => Some(content),
        },
        Err(_) => Some(content),
    }
}

/// Send a system+user request through the configured post-processing provider,
/// returning the raw response content. `json_schema` shapes structured output
/// where the provider supports it; `legacy_prompt` is the single-message
/// fallback used for providers without structured output support (and when
/// structured output fails).
async fn run_llm_raw(
    settings: &AppSettings,
    system_prompt: String,
    user_content: String,
    legacy_prompt: String,
    json_schema: serde_json::Value,
) -> Option<String> {
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
        "custom" => (Some("none".to_string()), None),
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
                debug!(
                    "Structured output succeeded for provider '{}'. Output length: {} chars",
                    provider.id,
                    content.len()
                );
                return Some(strip_invisible_chars(&content));
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

    // Legacy mode: send the fully-rendered prompt as a single message
    let processed_prompt = legacy_prompt;
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

async fn maybe_convert_chinese_variant(
    effective_language: &str,
    transcription: &str,
) -> Option<String> {
    // Gate on the language the model actually transcribed in (the effective
    // language), not the persisted intent. A leftover zh-Hans/zh-Hant intent
    // from a previously selected model must not run OpenCC S2T/T2S over output a
    // non-Chinese model produced — that would silently rewrite any shared CJK
    // characters (e.g. Japanese kanji) in the result.
    let is_simplified = effective_language == "zh-Hans";
    let is_traditional = effective_language == "zh-Hant";

    if !is_simplified && !is_traditional {
        debug!("effective language is not Simplified or Traditional Chinese; skipping conversion");
        return None;
    }

    debug!(
        "Starting Chinese variant conversion using OpenCC for language: {}",
        effective_language
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
    /// Select-all before pasting so the text replaces the whole focused field
    /// (whole-field rewrites in Command Mode).
    pub select_all_before_paste: bool,
}

/// Resolve the persisted language *intent* into the language the currently-loaded
/// model will actually use — the same capability-aware coercion the transcription
/// paths apply (see [`crate::managers::model::effective_language`]). Post-processing
/// resolves it independently so it agrees with the language the transcription ran
/// in, without threading a value through the pipeline.
fn resolve_effective_language(app: &AppHandle, settings: &AppSettings) -> String {
    let tm = app.state::<Arc<TranscriptionManager>>();
    let model_manager = app.state::<Arc<ModelManager>>();
    let active_model = tm
        .get_current_model()
        .unwrap_or_else(|| settings.selected_model.clone());
    match model_manager.get_model_info(&active_model) {
        Some(info) => crate::managers::model::effective_language(
            &settings.selected_language,
            &info.supported_languages,
            info.supports_language_detection,
        ),
        None => settings.selected_language.clone(),
    }
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

    // Resolve the language the transcription actually ran in (the persisted
    // intent coerced against the loaded model's capabilities) so OpenCC keys off
    // the effective language rather than a possibly-stale intent.
    let effective_language = resolve_effective_language(app, &settings);
    if let Some(converted_text) =
        maybe_convert_chinese_variant(&effective_language, transcription).await
    {
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

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
        select_all_before_paste: false,
    }
}

/// Hotwords that route a transcript to Command Mode, normalized to lowercase
/// alphanumerics. Longest first so "hey poptart" isn't short-matched by "poptart".
const HOTWORDS: [&str; 2] = ["heypoptart", "poptart"];

/// Returns the spoken instruction iff the transcript starts with a hotword.
/// Matching skips separator characters (spaces, commas, hyphens, quotes) and
/// compares alphanumerics case-insensitively, so Whisper renderings like
/// "Hey, Pop-Tart:" all match. Must run on the raw transcript, before LLM
/// cleanup (which could rewrite the hotword). A hotword with nothing after it
/// returns None — that's plain dictation.
fn strip_hotword(transcript: &str) -> Option<String> {
    'hotword: for hotword in HOTWORDS {
        let mut expected = hotword.chars();
        let mut next = expected.next();
        let mut end = 0;
        for (i, c) in transcript.char_indices() {
            let Some(e) = next else { break };
            if c.is_alphanumeric() {
                if c.to_ascii_lowercase() != e {
                    continue 'hotword;
                }
                next = expected.next();
                end = i + c.len_utf8();
            }
        }
        if next.is_some() {
            continue; // transcript ended mid-hotword
        }
        let rest = &transcript[end..];
        // Word boundary: reject "poptarts" and possessives like "Poptart's"
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '\'' || c == '\u{2019}')
        {
            continue;
        }
        let instruction =
            rest.trim_start_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());
        if instruction.is_empty() {
            return None;
        }
        return Some(instruction.to_string());
    }
    None
}

/// System prompt for Command Mode: apply a spoken instruction to the selected text.
const COMMAND_MODE_SYSTEM_PROMPT: &str = "You are a text editing engine. The user selected text and spoke an instruction. Apply the instruction to the text and output only the resulting text — no explanations, no markdown fences. If no text is provided, output only the text the instruction asks for.";

/// System prompt for Command Mode with field context (no manual selection):
/// the LLM decides between rewriting the whole field and inserting at the cursor.
const COMMAND_FIELD_SYSTEM_PROMPT: &str = "You are a text editing engine inside a text field. You are given the field's current content and a spoken instruction. If the instruction edits, rewrites, fixes, or transforms the existing content, return the complete new field content with action \"replace_field\". If the instruction asks for new text to add (compose, continue, answer), return only the new text with action \"insert\"; it will be typed at the cursor. Respond with only JSON: {\"action\": \"replace_field\" or \"insert\", \"text\": \"...\"} — no explanations, no markdown fences.";

/// What a spoken command operates on, in fallback order.
pub(crate) enum CommandContext {
    /// Text the user selected (AX read or clipboard capture) — output replaces it.
    Selection(String),
    /// No selection; the focused field's full text, read via AX — the LLM
    /// decides between a whole-field rewrite and an insertion.
    Field(String),
    /// Nothing readable — instruction-only generation, typed at the cursor.
    Empty,
}

/// Gather the text the spoken command should operate on. Must run on the main
/// thread (AX reads + possible synthesized Cmd+C).
fn capture_command_context(app: &AppHandle) -> CommandContext {
    let (ax_selected, ax_value) = crate::utils::ax_focused_texts();
    if let Some(sel) = ax_selected {
        return CommandContext::Selection(sel);
    }
    // Some apps (terminals, some Electron views) don't expose AX text; fall
    // back to the synthesized-copy clipboard dance.
    match crate::clipboard::copy_selected_text(app) {
        Ok(sel) if !sel.trim().is_empty() => return CommandContext::Selection(sel),
        Ok(_) => {}
        Err(e) => warn!("Command Mode selection capture failed: {}", e),
    }
    match ax_value {
        Some(field) => CommandContext::Field(field),
        None => CommandContext::Empty,
    }
}

/// Parse the LLM's field-context decision. Any parse failure degrades to
/// insert-at-cursor with the raw content — never destroys the field.
fn parse_command_decision(content: &str) -> (bool, String) {
    #[derive(serde::Deserialize)]
    struct Decision {
        action: String,
        text: String,
    }
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    match serde_json::from_str::<Decision>(trimmed) {
        Ok(d) => (d.action == "replace_field", d.text),
        Err(_) => (false, content.to_string()),
    }
}

pub(crate) async fn process_command_output(
    app: &AppHandle,
    instruction: &str,
    context: CommandContext,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    if let CommandContext::Field(field) = &context {
        let user_content = format!("Field content:\n{}\n\nInstruction:\n{}", field, instruction);
        let legacy_prompt = format!("{}\n\n{}", COMMAND_FIELD_SYSTEM_PROMPT, user_content);
        let json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["replace_field", "insert"],
                    "description": "replace_field: text replaces the entire field content. insert: text is typed at the cursor."
                },
                "text": {"type": "string", "description": "The resulting text."}
            },
            "required": ["action", "text"],
            "additionalProperties": false
        });
        let response = run_llm_raw(
            &settings,
            COMMAND_FIELD_SYSTEM_PROMPT.to_string(),
            user_content,
            legacy_prompt,
            json_schema,
        )
        .await;
        // On LLM failure paste nothing rather than typing the raw instruction.
        let (replace_field, text) = match response {
            Some(content) => parse_command_decision(&content),
            None => (false, String::new()),
        };
        return ProcessedTranscription {
            final_text: text.clone(),
            post_processed_text: if text.is_empty() { None } else { Some(text) },
            post_process_prompt: Some(COMMAND_FIELD_SYSTEM_PROMPT.to_string()),
            select_all_before_paste: replace_field,
        };
    }

    let user_content = match &context {
        CommandContext::Selection(sel) => {
            format!("Text:\n{}\n\nInstruction:\n{}", sel, instruction)
        }
        _ => format!("Instruction:\n{}", instruction),
    };
    let legacy_prompt = format!("{}\n\n{}", COMMAND_MODE_SYSTEM_PROMPT, user_content);
    let edited = run_llm(
        &settings,
        COMMAND_MODE_SYSTEM_PROMPT.to_string(),
        user_content,
        legacy_prompt,
    )
    .await;

    // On LLM failure paste nothing rather than replacing the user's selection
    // with the raw spoken instruction.
    ProcessedTranscription {
        final_text: edited.clone().unwrap_or_default(),
        post_processed_text: edited,
        post_process_prompt: Some(COMMAND_MODE_SYSTEM_PROMPT.to_string()),
        select_all_before_paste: false,
    }
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let tm = app.state::<Arc<TranscriptionManager>>();
        let rm = app.state::<Arc<AudioRecordingManager>>();

        // Load ASR model and VAD model in parallel
        let kickoff_started = Instant::now();
        tm.initiate_model_load();
        let rm_clone = Arc::clone(&rm);
        std::thread::spawn(move || {
            if let Err(e) = rm_clone.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });
        let kickoff_elapsed = kickoff_started.elapsed();

        let binding_id = binding_id.to_string();
        let tray_started = Instant::now();
        change_tray_icon(app, TrayIconState::Recording);
        let tray_elapsed = tray_started.elapsed();

        // Get the microphone mode to determine audio feedback timing
        let plan_started = Instant::now();
        let settings = get_settings(app);
        let is_always_on = settings.always_on_microphone;

        let selected_model_info = app
            .state::<Arc<ModelManager>>()
            .get_model_info(&settings.selected_model);

        // Use the app-facing model capability as the single pre-recording source
        // for live streaming decisions. Unknown support is represented as false
        // until the model registry is updated by discovery or runtime load.
        let model_supports_streaming = selected_model_info
            .as_ref()
            .map(|m| m.supports_streaming)
            .unwrap_or(false);
        let vad_policy = if !settings.vad_enabled {
            VadPolicy::Disabled
        } else if model_supports_streaming {
            VadPolicy::Streaming
        } else {
            VadPolicy::Offline
        };
        if model_supports_streaming {
            tm.start_stream();
        }
        let plan_elapsed = plan_started.elapsed();

        // Sizing the overlay follows the same advertised capability. A model that
        // doesn't stream (or whose capability is not known yet) gets the compact
        // pill instead of an oversized transparent live window.
        let overlay_started = Instant::now();
        match settings.overlay_style {
            OverlayStyle::Live if model_supports_streaming => utils::show_streaming_overlay(app),
            OverlayStyle::Live | OverlayStyle::Minimal => show_recording_overlay(app),
            OverlayStyle::None => {} // show_overlay_state no-ops on None anyway
        }
        // Everything above runs before capture can begin, so each span here is
        // added keypress->capture latency.
        debug!(
            "start-path pre-recording steps: model_kickoff={:?} tray={:?} settings+stream_plan={:?} overlay={:?}",
            kickoff_elapsed,
            tray_elapsed,
            plan_elapsed,
            overlay_started.elapsed()
        );
        debug!("Microphone mode - always_on: {}", is_always_on);

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

            if let Err(e) = rm.try_start_recording(&binding_id, vad_policy) {
                debug!("Recording failed: {}", e);
                recording_error = Some(e);
            }
        } else {
            // On-demand mode: Start recording first, then play audio feedback, then apply mute
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            match rm.try_start_recording(&binding_id, vad_policy) {
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
            tm.cancel_stream();
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
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        let style = get_settings(app).overlay_style;
        match (style, tm.is_streaming()) {
            (OverlayStyle::Live, true) => {
                tm.emit_stream_working(StreamWorkKind::Transcribing);
            }
            _ => show_transcribing_overlay(app),
        }

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        let post_process = self.post_process;
        let cancel_generation = rm.cancel_generation();

        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone());
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id, cancel_generation) {
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if rm.was_cancelled_since(cancel_generation) {
                    debug!("Transcription operation cancelled after recording stop");
                    tm.cancel_stream();
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                    return;
                }

                if samples.is_empty() {
                    debug!("Recording produced no audio samples; skipping persistence");
                    // Tear down any streaming worker so its channel doesn't leak
                    // and block the next start_stream.
                    tm.cancel_stream();
                    utils::hide_recording_overlay(&ah);
                    change_tray_icon(&ah, TrayIconState::Idle);
                } else {
                    // Save WAV concurrently with transcription
                    let sample_count = samples.len();
                    let file_name = format!("poptart-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    // Transcribe concurrently with WAV save. If a live stream was
                    // running, finalize it and use its text (all audio was already
                    // fed to the stream); otherwise batch-transcribe the samples.
                    let transcription_time = Instant::now();
                    let transcription_result = match tm.finalize_stream() {
                        // A finalized stream with usable text wins. An empty result
                        // (no active stream, produced nothing, or a finalize error
                        // after the engine was returned) falls back to a full batch
                        // transcription of the same audio. A finalize timeout is
                        // surfaced instead — the worker may still hold the engine,
                        // so a batch fallback would contend with it.
                        Ok(Some(text)) if !text.trim().is_empty() => Ok(text),
                        Ok(_) => tm.transcribe(samples),
                        Err(err) => Err(err),
                    };

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

                    if rm.was_cancelled_since(cancel_generation) {
                        debug!("Transcription operation cancelled before output handling");
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
                        return;
                    }

                    match transcription_result {
                        Ok(transcription) => {
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                transcription
                            );

                            // Hotword routing: "hey poptart …" turns the rest of
                            // the transcript into a command. Detected on the raw
                            // transcript (LLM cleanup could rewrite the hotword);
                            // needs the post-process LLM, so without it everything
                            // is dictation.
                            let command_instruction = if get_settings(&ah).post_process_enabled {
                                strip_hotword(&transcription)
                            } else {
                                None
                            };

                            if post_process || command_instruction.is_some() {
                                if style == OverlayStyle::Live {
                                    tm.emit_stream_working(StreamWorkKind::Polishing);
                                } else {
                                    show_processing_overlay(&ah);
                                }
                            }
                            let processed = if let Some(instruction) = &command_instruction {
                                // Capture focus context on the main thread (AX
                                // reads + possible synthesized Cmd+C); any
                                // failure degrades to instruction-only mode.
                                let (tx, rx) = std::sync::mpsc::channel::<CommandContext>();
                                let ah_ctx = ah.clone();
                                let context = match ah.run_on_main_thread(move || {
                                    let _ = tx.send(capture_command_context(&ah_ctx));
                                }) {
                                    Ok(()) => tauri::async_runtime::spawn_blocking(move || {
                                        rx.recv_timeout(std::time::Duration::from_secs(2))
                                            .unwrap_or(CommandContext::Empty)
                                    })
                                    .await
                                    .unwrap_or(CommandContext::Empty),
                                    Err(e) => {
                                        error!("Failed to schedule context capture: {:?}", e);
                                        CommandContext::Empty
                                    }
                                };
                                process_command_output(&ah, instruction, context).await
                            } else {
                                process_transcription_output(&ah, &transcription, post_process)
                                    .await
                            };

                            if rm.was_cancelled_since(cancel_generation) {
                                debug!("Transcription operation cancelled before paste");
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }

                            // Save to history if WAV was saved
                            if wav_saved {
                                if let Err(err) = hm.save_entry(
                                    file_name,
                                    transcription,
                                    post_process || command_instruction.is_some(),
                                    processed.post_processed_text.clone(),
                                    processed.post_process_prompt.clone(),
                                ) {
                                    error!("Failed to save history entry: {}", err);
                                }
                            }

                            if processed.final_text.is_empty() {
                                if command_instruction.is_some() {
                                    // A routed command produced nothing (LLM
                                    // unreachable or empty response): flash the
                                    // error pill so the failure isn't silent.
                                    utils::show_error_overlay(&ah);
                                    let ah_err = ah.clone();
                                    let rm_err = Arc::clone(&rm);
                                    std::thread::spawn(move || {
                                        std::thread::sleep(std::time::Duration::from_millis(1800));
                                        // Skip the hide if a new recording
                                        // already took over the overlay.
                                        if !rm_err.is_recording() {
                                            utils::hide_recording_overlay(&ah_err);
                                        }
                                    });
                                } else {
                                    utils::hide_recording_overlay(&ah);
                                }
                                change_tray_icon(&ah, TrayIconState::Idle);
                            } else {
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let final_text = processed.final_text;
                                let select_all_before_paste = processed.select_all_before_paste;
                                let rm_for_paste = Arc::clone(&rm);
                                ah.run_on_main_thread(move || {
                                    if rm_for_paste.was_cancelled_since(cancel_generation) {
                                        debug!("Transcription operation cancelled before paste");
                                        utils::hide_recording_overlay(&ah_clone);
                                        change_tray_icon(&ah_clone, TrayIconState::Idle);
                                        return;
                                    }

                                    // Whole-field rewrite: select everything so the
                                    // paste replaces the field. Lock released before
                                    // utils::paste re-acquires it.
                                    if select_all_before_paste {
                                        match ah_clone.try_state::<crate::input::EnigoState>() {
                                            Some(state) => {
                                                let result = state
                                                    .0
                                                    .lock()
                                                    .map_err(|e| e.to_string())
                                                    .and_then(|mut enigo| {
                                                        crate::input::send_select_all(&mut enigo)
                                                    });
                                                if let Err(e) = result {
                                                    warn!("Select-all before paste failed: {}", e);
                                                }
                                                // Give the app a beat to apply the selection
                                                std::thread::sleep(
                                                    std::time::Duration::from_millis(50),
                                                );
                                            }
                                            None => warn!(
                                                "EnigoState unavailable; pasting without select-all"
                                            ),
                                        }
                                    }

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
                            if rm.was_cancelled_since(cancel_generation) {
                                debug!(
                                    "Transcription operation cancelled after transcription error"
                                );
                                utils::hide_recording_overlay(&ah);
                                change_tray_icon(&ah, TrayIconState::Idle);
                                return;
                            }

                            error!("Transcription failed: {}", err);
                            // Surface the failure to the UI (toast). The full
                            // message is also in poptart.log via the line above.
                            let _ = ah.emit("transcription-error", err.to_string());
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
                // Tear down any streaming worker so its channel doesn't leak.
                tm.cancel_stream();
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

// Test Action
struct TestAction;

impl ShortcutAction for TestAction {
    fn start(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Started - {} (App: {})", // Changed "Pressed" to "Started" for consistency
            binding_id,
            shortcut_str,
            app.package_info().name
        );
    }

    fn stop(&self, app: &AppHandle, binding_id: &str, shortcut_str: &str) {
        log::info!(
            "Shortcut ID '{}': Stopped - {} (App: {})", // Changed "Released" to "Stopped" for consistency
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
        Arc::new(TranscribeAction {
            post_process: false,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction { post_process: true }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "cancel".to_string(),
        Arc::new(CancelAction) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "test".to_string(),
        Arc::new(TestAction) as Arc<dyn ShortcutAction>,
    );
    map
});

#[cfg(test)]
mod tests {
    use super::is_blank_transcription;

    #[test]
    fn blank_transcription_is_detected() {
        assert!(is_blank_transcription(""));
        assert!(is_blank_transcription("   "));
        assert!(is_blank_transcription("\t\n  \r\n"));
    }

    #[test]
    fn non_blank_transcription_is_kept() {
        assert!(!is_blank_transcription("hello"));
        assert!(!is_blank_transcription("  hello  "));
    }

    #[test]
    fn hotword_variants_route_to_command() {
        let cases = [
            ("Hey Poptart, make this shorter", "make this shorter"),
            ("hey pop tart make it formal", "make it formal"),
            (
                "Poptart. delete the last sentence.",
                "delete the last sentence.",
            ),
            ("Pop-Tart: fix grammar", "fix grammar"),
            ("POPTART do it", "do it"),
            ("\"Hey Poptart\" summarize", "summarize"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                super::strip_hotword(input).as_deref(),
                Some(expected),
                "{input}"
            );
        }
    }

    #[test]
    fn non_hotword_transcripts_stay_dictation() {
        let cases = [
            "poptarts are great",
            "The pop tart was tasty",
            "Poptart's my favorite snack",
            "Poptart.",    // empty instruction
            "hey poptart", // empty instruction
            "hello world",
            "",
            "pop start the music",
        ];
        for input in cases {
            assert_eq!(super::strip_hotword(input), None, "{input}");
        }
    }

    #[test]
    fn command_decision_parses_replace_and_insert() {
        let (replace, text) =
            super::parse_command_decision(r#"{"action":"replace_field","text":"fixed text"}"#);
        assert!(replace);
        assert_eq!(text, "fixed text");

        let (replace, text) =
            super::parse_command_decision(r#"{"action":"insert","text":"new sentence"}"#);
        assert!(!replace);
        assert_eq!(text, "new sentence");
    }

    #[test]
    fn command_decision_strips_fences() {
        let (replace, text) = super::parse_command_decision(
            "```json\n{\"action\":\"replace_field\",\"text\":\"hi\"}\n```",
        );
        assert!(replace);
        assert_eq!(text, "hi");
    }

    #[test]
    fn command_decision_garbage_degrades_to_insert() {
        let (replace, text) =
            super::parse_command_decision("Sure! Here is the text you asked for.");
        assert!(!replace);
        assert_eq!(text, "Sure! Here is the text you asked for.");
    }
}
