#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::{is_microphone_access_denied, is_no_input_device_error};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::history::{HistoryEntryMetadata, HistoryManager};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{
    get_settings, AppSettings, TranscriptionBackend, APPLE_INTELLIGENCE_PROVIDER_ID,
};
use crate::shortcut;
use crate::transcription_finalizer::{
    finalize_transcription_outcome, save_recording_wav, strip_invisible_chars,
    TranscriptionFinalizeOptions, TranscriptionOutcome,
};
use crate::tray::{change_tray_icon, TrayIconState};
use crate::utils::{self, show_recording_overlay, show_transcribing_overlay};
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

fn show_livestt_error(app: &AppHandle, error_code: &str, error_message: &str) {
    show_recording_overlay(app);
    crate::livestt::events::emit_livestt_error(app, None, error_code, error_message);

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let recording_active = app_clone
            .try_state::<Arc<AudioRecordingManager>>()
            .map(|manager| manager.is_recording())
            .unwrap_or(false);
        let livestt_active = app_clone
            .try_state::<Arc<crate::livestt::session::LiveSttSessionManager>>()
            .map(|manager| manager.is_active())
            .unwrap_or(false);
        if !recording_active && !livestt_active {
            utils::hide_recording_overlay(&app_clone);
        }
    });
}

fn notify_livestt_start_aborted(app: &AppHandle, binding_id: &str) {
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_start_failed(binding_id);
    }
}

fn notify_livestt_start_failed(app: &AppHandle, binding_id: &str) {
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_start_failed(binding_id);
    }
}

fn notify_livestt_recording_started(app: &AppHandle, binding_id: &str) {
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_recording_started(binding_id);
    }
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

// ── Hardware-button action dispatch ──────────────────────────────────────────

/// What the caller wants to happen (independent of how it was triggered).
#[allow(dead_code)]
pub enum ActionIntent {
    Transcribe,
    TranscribeWithPostProcess,
    Cancel,
}

/// Who triggered the action (for future telemetry / routing).
#[allow(dead_code)]
pub enum ActionTriggerSource {
    Keyboard,
    SpeechMike,
    Tray,
    Cli,
}

/// Unified entry point for triggering recording actions from any source.
///
/// Delegates to `TranscriptionCoordinator` for transcribe intents and to
/// `ACTION_MAP` for cancel, mirroring `shortcut::handler::handle_shortcut_event`.
pub fn fire_action(
    app: &AppHandle,
    intent: ActionIntent,
    pressed: bool,
    _source: ActionTriggerSource,
) {
    use crate::transcription_coordinator::is_transcribe_binding;
    use crate::TranscriptionCoordinator;

    let binding_id = match intent {
        ActionIntent::Transcribe => "transcribe",
        ActionIntent::TranscribeWithPostProcess => "transcribe_with_post_process",
        ActionIntent::Cancel => "cancel",
    };

    if is_transcribe_binding(binding_id) {
        let push_to_talk = get_settings(app).push_to_talk;
        if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
            coordinator.send_input(binding_id, "speechmike", pressed, push_to_talk);
        } else {
            warn!("fire_action: TranscriptionCoordinator not initialized");
        }
        return;
    }

    // Cancel action
    let Some(action) = ACTION_MAP.get(binding_id) else {
        return;
    };

    if binding_id == "cancel" {
        let audio_manager = app.state::<Arc<AudioRecordingManager>>();
        let livestt_active = app
            .try_state::<Arc<crate::livestt::session::LiveSttSessionManager>>()
            .map(|m| m.is_active())
            .unwrap_or(false);
        if (audio_manager.is_recording() || livestt_active) && pressed {
            action.start(app, binding_id, "speechmike");
        }
        return;
    }

    if pressed {
        action.start(app, binding_id, "speechmike");
    } else {
        action.stop(app, binding_id, "speechmike");
    }
}

// ── Shortcut Action Trait ─────────────────────────────────────────────────────

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

fn parse_livestt_consultation_id_for_history(value: Option<&str>) -> Option<i64> {
    value.and_then(|value| value.trim().parse::<i64>().ok())
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

    ProcessedTranscription {
        final_text,
        post_processed_text,
        post_process_prompt,
    }
}

impl ShortcutAction for TranscribeAction {
    fn start(&self, app: &AppHandle, binding_id: &str, _shortcut_str: &str) {
        let start_time = Instant::now();
        debug!("TranscribeAction::start called for binding: {}", binding_id);

        // Load model in the background
        let rm = app.state::<Arc<AudioRecordingManager>>();
        let settings = get_settings(app);
        let backend = settings.transcription_backend;
        debug!("Selected transcription backend: {:?}", backend);

        // Load ASR model only for local transcription. LiveSTT uses the remote backend.
        if backend == TranscriptionBackend::Local {
            let tm = app.state::<Arc<TranscriptionManager>>();
            tm.initiate_model_load();
        }

        // Load VAD model for both backends. LiveSTT streams the same processed samples
        // that the local backend records.
        let rm_clone = Arc::clone(&rm);
        std::thread::spawn(move || {
            if let Err(e) = rm_clone.preload_vad() {
                debug!("VAD pre-load failed: {}", e);
            }
        });

        // Get the microphone mode to determine audio feedback timing
        let is_always_on = settings.always_on_microphone;
        debug!("Microphone mode - always_on: {}", is_always_on);

        let binding_id = binding_id.to_string();

        if backend == TranscriptionBackend::LiveStt {
            debug!("Reserving async LiveSTT session for binding {}", binding_id);

            let livestt_state = app.state::<Arc<crate::livestt::session::LiveSttSessionManager>>();
            let livestt_manager = Arc::clone(&*livestt_state);
            let reservation = match livestt_manager.reserve_start() {
                Ok(reservation) => reservation,
                Err(err) => {
                    warn!("LiveSTT session reserve failed: {}", err);
                    change_tray_icon(app, TrayIconState::Idle);
                    let error_code = crate::livestt::session::classify_livestt_error(&err);
                    if error_code != crate::livestt::events::LIVESTT_ERROR_CANCELED {
                        show_livestt_error(app, error_code, &err);
                    } else {
                        utils::hide_recording_overlay(app);
                    }
                    notify_livestt_start_failed(app, &binding_id);
                    return;
                }
            };

            // Pre-allocate the audio pipe so the microphone can capture while
            // the WebSocket is still connecting. Mic chunks accumulate in the
            // bounded mpsc channel; the writer task drains them in order once
            // the socket is live, preserving every word from the hotkey press.
            let pending_audio = livestt_manager.create_pending_audio();
            let mic_sink = pending_audio.sink();

            change_tray_icon(app, TrayIconState::Recording);
            show_recording_overlay(app);

            let recording_start_time = Instant::now();
            if let Err(e) = rm.try_start_recording_with_chunk_sender(&binding_id, mic_sink) {
                debug!("Failed to start recording before LiveSTT connect: {}", e);
                let _ = livestt_manager.cancel_session();
                change_tray_icon(app, TrayIconState::Idle);
                show_livestt_error(
                    app,
                    crate::livestt::events::LIVESTT_ERROR_AUDIO_WRITER_FAILED,
                    &e,
                );
                let error_type = if is_microphone_access_denied(&e) {
                    "microphone_permission_denied"
                } else if is_no_input_device_error(&e) {
                    "no_input_device"
                } else {
                    "unknown"
                };
                let _ = app.emit(
                    "recording-error",
                    RecordingErrorEvent {
                        error_type: error_type.to_string(),
                        detail: Some(e),
                    },
                );
                notify_livestt_start_aborted(app, &binding_id);
                return;
            }
            debug!(
                "LiveSTT recording started in {:?} (buffering until socket ready)",
                recording_start_time.elapsed()
            );

            // Schedule audio feedback / mute matching the original mode-specific
            // timing. Always-on plays immediately; on-demand waits 100 ms so the
            // mic stream is settled before the cue.
            let app_for_sound = app.clone();
            let rm_for_mute = Arc::clone(&rm);
            if is_always_on {
                debug!("Always-on mode: Playing audio feedback immediately");
                std::thread::spawn(move || {
                    play_feedback_sound_blocking(&app_for_sound, SoundType::Start);
                    rm_for_mute.apply_mute();
                });
            } else {
                debug!("On-demand mode: Delayed audio feedback/mute sequence");
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    play_feedback_sound_blocking(&app_for_sound, SoundType::Start);
                    rm_for_mute.apply_mute();
                });
            }

            let app_clone = app.clone();
            let rm_clone = Arc::clone(&rm);
            let binding_id_for_start = binding_id.clone();
            let livestt_manager_for_start = Arc::clone(&livestt_manager);

            tauri::async_runtime::spawn(async move {
                let start_result = livestt_manager_for_start
                    .start_reserved_session(
                        app_clone.clone(),
                        binding_id_for_start.clone(),
                        reservation,
                        pending_audio,
                    )
                    .await;

                if let Err(err) = start_result {
                    warn!("LiveSTT session start failed: {}", err);
                    rm_clone.cancel_recording();
                    change_tray_icon(&app_clone, TrayIconState::Idle);
                    let error_code = crate::livestt::session::classify_livestt_error(&err);
                    if error_code != crate::livestt::events::LIVESTT_ERROR_CANCELED {
                        show_livestt_error(&app_clone, error_code, &err);
                    } else {
                        utils::hide_recording_overlay(&app_clone);
                    }
                    notify_livestt_start_aborted(&app_clone, &binding_id_for_start);
                    debug!(
                        "TranscribeAction::start completed in {:?}",
                        start_time.elapsed()
                    );
                    return;
                }

                debug!(
                    "LiveSTT session started for binding {}",
                    binding_id_for_start
                );
                shortcut::register_cancel_shortcut(&app_clone);
                notify_livestt_recording_started(&app_clone, &binding_id_for_start);
                debug!(
                    "TranscribeAction::start completed in {:?}",
                    start_time.elapsed()
                );
            });

            debug!(
                "TranscribeAction::start returned before LiveSTT connect completed in {:?}",
                start_time.elapsed()
            );
            return;
        }

        change_tray_icon(app, TrayIconState::Recording);
        show_recording_overlay(app);

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

            let start_result = rm.try_start_recording(&binding_id);
            if let Err(e) = start_result {
                debug!("Recording failed: {}", e);
                recording_error = Some(e);
            }
        } else {
            // On-demand mode: Start recording first, then play audio feedback, then apply mute
            // This allows the microphone to be activated before playing the sound
            debug!("On-demand mode: Starting recording first, then audio feedback");
            let recording_start_time = Instant::now();
            let start_result = rm.try_start_recording(&binding_id);
            match start_result {
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
            if backend == TranscriptionBackend::LiveStt {
                let _ = app
                    .state::<Arc<crate::livestt::session::LiveSttSessionManager>>()
                    .cancel_session();
            }
            // Starting failed (for example due to blocked microphone permissions).
            // Revert UI state so we don't stay stuck in the recording overlay.
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
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());
        let backend = get_settings(app).transcription_backend;
        debug!("Stopping transcription backend: {:?}", backend);

        if backend == TranscriptionBackend::LiveStt {
            let livestt_state = app.state::<Arc<crate::livestt::session::LiveSttSessionManager>>();
            let livestt_manager = Arc::clone(&*livestt_state);
            if livestt_manager.is_starting() || !rm.is_recording() {
                debug!(
                    "Canceling LiveSTT start/session for binding {} before recording began",
                    binding_id
                );
                let _ = livestt_manager.cancel_session();
                // Mic is started synchronously at hotkey press now (before the
                // socket connects), so it can be active even when the session
                // is still in the Starting state — make sure to stop it.
                rm.cancel_recording();
                utils::hide_recording_overlay(app);
                change_tray_icon(app, TrayIconState::Idle);
                if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
                    coordinator.notify_start_failed(binding_id);
                }
                debug!(
                    "TranscribeAction::stop completed in {:?}",
                    stop_time.elapsed()
                );
                return;
            }
        }

        change_tray_icon(app, TrayIconState::Transcribing);
        show_transcribing_overlay(app);

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        let post_process = self.post_process;

        if backend == TranscriptionBackend::LiveStt {
            tauri::async_runtime::spawn(async move {
                let _guard = FinishGuard(ah.clone());
                debug!(
                    "Starting async LiveSTT stop task for binding: {}",
                    binding_id
                );

                let stop_recording_time = Instant::now();
                let samples = rm.stop_recording(&binding_id);
                if let Some(samples) = &samples {
                    debug!(
                        "LiveSTT microphone stopped in {:?}, local sample count: {}",
                        stop_recording_time.elapsed(),
                        samples.len()
                    );
                } else {
                    warn!("No samples retrieved from LiveSTT recording stop");
                }

                let livestt_state =
                    ah.state::<Arc<crate::livestt::session::LiveSttSessionManager>>();
                let livestt_manager = Arc::clone(&*livestt_state);
                let session_result = livestt_manager.stop_session(&binding_id).await;

                let session_result = match session_result {
                    Ok(result) => {
                        debug!(
                            "LiveSTT finalization complete for session {:?}. Final length: {} chars",
                            result.session_id,
                            result.final_text.len()
                        );
                        result
                    }
                    Err(err) => {
                        warn!("LiveSTT transcription failed: {}", err);
                        change_tray_icon(&ah, TrayIconState::Idle);
                        let error_code = crate::livestt::session::classify_livestt_error(&err);
                        if error_code != crate::livestt::events::LIVESTT_ERROR_CANCELED {
                            show_livestt_error(&ah, error_code, &err);
                        } else {
                            utils::hide_recording_overlay(&ah);
                        }
                        return;
                    }
                };
                let app_settings = get_settings(&ah);
                let consultation_id = parse_livestt_consultation_id_for_history(
                    app_settings.livestt_consultation_id.as_deref(),
                );
                let outcome = TranscriptionOutcome {
                    raw_text: session_result.final_text,
                    samples,
                    metadata: HistoryEntryMetadata::livestt(
                        session_result.session_id,
                        consultation_id,
                    ),
                };

                if let Err(err) = finalize_transcription_outcome(
                    ah.clone(),
                    Arc::clone(&hm),
                    outcome,
                    TranscriptionFinalizeOptions {
                        post_process,
                        provider_label_for_logs: "LiveSTT",
                    },
                )
                .await
                {
                    error!("LiveSTT finalizer failed: {}", err);
                }
            });

            debug!(
                "TranscribeAction::stop completed in {:?}",
                stop_time.elapsed()
            );
            return;
        }

        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());

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
                    return;
                }

                let transcription_time = Instant::now();
                match tm.transcribe(samples.clone()) {
                    Ok(transcription) => {
                        debug!(
                            "Transcription completed in {:?}: '{}'",
                            transcription_time.elapsed(),
                            transcription
                        );

                        let outcome = TranscriptionOutcome {
                            raw_text: transcription,
                            samples: Some(samples),
                            metadata: HistoryEntryMetadata::local(),
                        };

                        if let Err(err) = finalize_transcription_outcome(
                            ah.clone(),
                            Arc::clone(&hm),
                            outcome,
                            TranscriptionFinalizeOptions {
                                post_process,
                                provider_label_for_logs: "Local",
                            },
                        )
                        .await
                        {
                            error!("Local finalizer failed: {}", err);
                        }
                    }
                    Err(err) => {
                        debug!("Global Shortcut Transcription error: {}", err);
                        if let Some(file_name) =
                            save_recording_wav(Arc::clone(&hm), samples, "Local").await
                        {
                            if let Err(save_err) =
                                hm.save_entry(file_name, String::new(), post_process, None, None)
                            {
                                error!("Failed to save failed history entry: {}", save_err);
                            }
                        }
                        utils::hide_recording_overlay(&ah);
                        change_tray_icon(&ah, TrayIconState::Idle);
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
