#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::audio_feedback::{play_feedback_sound, play_feedback_sound_blocking, SoundType};
use crate::audio_toolkit::VadPolicy;
use crate::managers::audio::{recording_error_type, AudioRecordingManager};
use crate::managers::history::HistoryManager;
use crate::managers::model::ModelManager;
use crate::managers::transcription::policy::{live_decode, LiveDecode, ModelTraits};
use crate::managers::transcription::StreamWorkKind;
use crate::managers::transcription::{Transcription, TranscriptionManager};
use crate::settings::{get_settings, AppSettings, OverlayStyle, APPLE_INTELLIGENCE_PROVIDER_ID};
use crate::shortcut;
use crate::tray::{set_tray_state, TrayIconState};
use crate::utils::{
    self, show_processing_overlay, show_recording_overlay, show_transcribing_overlay,
};
use crate::TranscriptionCoordinator;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri::{AppHandle, Emitter};
use tauri_specta::Event;

/// How a dictation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionOutcome {
    /// The text was typed.
    Typed,
    /// Nothing was heard, so nothing was typed.
    Empty,
    /// The microphone sent only digital silence (e.g. muted).
    Silent,
    /// Transcribing or pasting failed.
    Failed,
}

/// Emitted when a dictation finishes (not on cancel); onboarding uses it to measure speed.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptionCompleteEvent {
    pub outcome: TranscriptionOutcome,
    pub audio_seconds: f64,
    pub wait_seconds: f64,
}

fn emit_transcription_complete(
    app: &AppHandle,
    outcome: TranscriptionOutcome,
    sample_count: usize,
    stop_time: Instant,
) {
    let event = TranscriptionCompleteEvent {
        outcome,
        audio_seconds: sample_count as f64
            / crate::audio_toolkit::constants::WHISPER_SAMPLE_RATE as f64,
        wait_seconds: stop_time.elapsed().as_secs_f64(),
    };
    if let Err(e) = event.emit(app) {
        warn!("Failed to emit transcription complete: {e}");
    }
}

/// Nothing to type: `Silent` if the microphone sent only zeros, else `Empty`.
fn empty_outcome(input_silent: bool) -> TranscriptionOutcome {
    if input_silent {
        TranscriptionOutcome::Silent
    } else {
        TranscriptionOutcome::Empty
    }
}

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Bumped per recording session, so a late paste from an earlier session
/// doesn't reset the tray or hide the overlay of a newer one.
static RECORDING_SESSION: AtomicU64 = AtomicU64::new(0);

struct ContextSelection {
    session: u64,
    text: String,
}

/// Selection captured at the start of a context-scribe recording, keyed by
/// [`RECORDING_SESSION`] so a newer take cannot consume an older one.
static CONTEXT_SELECTION: Mutex<Option<ContextSelection>> = Mutex::new(None);

fn store_context_selection(session: u64, text: String) {
    *CONTEXT_SELECTION
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(ContextSelection { session, text });
}

fn take_context_selection(session: u64) -> Option<String> {
    let mut slot = CONTEXT_SELECTION
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match slot.as_ref() {
        Some(captured) if captured.session == session => slot.take().map(|captured| captured.text),
        _ => None,
    }
}

#[derive(Clone, serde::Serialize)]
struct RecordingErrorEvent {
    error_type: String,
    detail: Option<String>,
}

/// Drop guard that finishes the transcription pipeline, including immediate
/// model unloading on early exits.
struct FinishGuard(AppHandle, Arc<TranscriptionManager>);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.1.maybe_unload_immediately("transcription session");
        if let Some(c) = self.0.try_state::<TranscriptionCoordinator>() {
            c.notify_processing_finished();
        }
        // The pipeline just freed its large transient buffers (captured PCM,
        // WAV copy, engine scratch); hand the cached pages back to the OS so
        // they don't sit in malloc arenas until they get swapped out (#1792).
        crate::memory::trim_freed_memory();
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
    /// Read the focused OS selection and send it with the transcript.
    /// Implies post-processing. Only the context-scribe hotkey sets this.
    context_scribe: bool,
}

/// Field name for structured output JSON schema
const TRANSCRIPTION_FIELD: &str = "transcription";

/// Strip invisible Unicode characters that some LLMs may insert
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

/// Strip a leading `<think>...</think>` block. Some endpoints can't disable
/// reasoning, and some local servers put the reasoning text into `content`
/// instead of a separate field — without this the user would get the model's
/// chain of thought pasted along with the cleaned transcription.
fn strip_think_block(s: &str) -> &str {
    if let Some(rest) = s.trim_start().strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return rest[end + "</think>".len()..].trim_start();
        }
    }
    s
}

const OUTPUT_PLACEHOLDER: &str = "${output}";
const SELECTION_PLACEHOLDER: &str = "${selection}";

/// Substitute `${output}` and `${selection}` without scanning the inserted
/// values, so a selection that itself contains `${output}` is left alone.
fn expand_prompt_template(template: &str, output: &str, selection: &str) -> String {
    let mut expanded = String::with_capacity(template.len() + output.len() + selection.len());
    let mut rest = template;
    while !rest.is_empty() {
        let output_at = rest.find(OUTPUT_PLACEHOLDER);
        let selection_at = rest.find(SELECTION_PLACEHOLDER);
        let (index, placeholder, value) = match (output_at, selection_at) {
            (None, None) => {
                expanded.push_str(rest);
                break;
            }
            (Some(index), None) => (index, OUTPUT_PLACEHOLDER, output),
            (None, Some(index)) => (index, SELECTION_PLACEHOLDER, selection),
            (Some(output_index), Some(selection_index)) if output_index <= selection_index => {
                (output_index, OUTPUT_PLACEHOLDER, output)
            }
            (Some(_), Some(selection_index)) => (selection_index, SELECTION_PLACEHOLDER, selection),
        };
        expanded.push_str(&rest[..index]);
        expanded.push_str(value);
        rest = &rest[index + placeholder.len()..];
    }
    expanded
}

/// Post-processing only: replace `${output}` and leave every other token,
/// including `${selection}`, as the user wrote it.
fn expand_output_placeholder(template: &str, output: &str) -> String {
    template.replace(OUTPUT_PLACEHOLDER, output)
}

/// System prompt for regular post-processing. `${output}` is removed because
/// the transcript is the user message. `${selection}` is not filled.
fn build_post_process_system_prompt(prompt_template: &str) -> String {
    expand_output_placeholder(prompt_template, "")
        .trim()
        .to_string()
}

/// System prompt for context scribe. `${output}` is removed because the
/// transcript is the user message, and `${selection}` is the captured text.
fn build_system_prompt(prompt_template: &str, selection: &str) -> String {
    expand_prompt_template(prompt_template, "", selection)
        .trim()
        .to_string()
}

/// `limit_chars` counts Unicode scalar values. Equal to the limit is allowed.
fn selection_is_too_long(selection: &str, limit_chars: u32) -> bool {
    selection.chars().count() > usize::try_from(limit_chars).unwrap_or(usize::MAX)
}

/// What to type after transcription, and whether that typing must replace a
/// captured selection instead of following the user's usual paste settings.
#[derive(Debug, PartialEq, Eq)]
struct TranscriptionPaste {
    /// Empty means do not paste.
    text: String,
    replace_selection: bool,
}

fn resolve_transcription_paste(
    transcription: &str,
    post_process: bool,
    llm_output: Option<String>,
    selection: Option<&str>,
    selection_too_long: bool,
) -> TranscriptionPaste {
    // An over-long selection is refused whole. Do not paste the dictation over it.
    if selection_too_long {
        return TranscriptionPaste {
            text: String::new(),
            replace_selection: false,
        };
    }

    let replacing = selection.is_some_and(|text| !text.is_empty());
    if !post_process {
        return TranscriptionPaste {
            text: transcription.to_string(),
            replace_selection: false,
        };
    }

    match llm_output {
        // A blank model reply must not wipe the text the user had selected.
        Some(text) if replacing && text.trim().is_empty() => TranscriptionPaste {
            text: String::new(),
            replace_selection: false,
        },
        Some(text) => {
            let replace_selection = replacing && !text.is_empty();
            TranscriptionPaste {
                text,
                replace_selection,
            }
        }
        // The model produced nothing. Keep the selection; do not type the dictation over it.
        None if replacing => TranscriptionPaste {
            text: String::new(),
            replace_selection: false,
        },
        None => TranscriptionPaste {
            text: transcription.to_string(),
            replace_selection: false,
        },
    }
}

/// Returns `true` when a transcription has no meaningful content to
/// post-process (empty or whitespace-only). Used to skip the post-processing
/// LLM call when nothing was actually transcribed, which would otherwise make
/// the model reply with an error message such as "you need to provide the
/// transcription".
fn is_blank_transcription(transcription: &str) -> bool {
    transcription.trim().is_empty()
}

async fn complete_unless_cancelled<F, C>(operation: F, is_cancelled: C) -> Option<F::Output>
where
    F: Future,
    C: Fn() -> bool,
{
    tokio::pin!(operation);

    loop {
        if is_cancelled() {
            return None;
        }

        if let Ok(result) =
            tokio::time::timeout(CANCELLATION_POLL_INTERVAL, operation.as_mut()).await
        {
            return Some(result);
        }
    }
}

fn should_use_streaming_overlay(style: OverlayStyle, is_streaming: bool) -> bool {
    style == OverlayStyle::Live && is_streaming
}

async fn post_process_transcription(
    settings: &AppSettings,
    transcription: &str,
    selection: &str,
    context_scribe: bool,
) -> Option<String> {
    if is_blank_transcription(transcription) {
        debug!("Post-processing skipped because the transcription is empty");
        return None;
    }

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

    let (prompts, selected_prompt_id) = if context_scribe {
        (
            &settings.context_scribe_prompts,
            &settings.context_scribe_selected_prompt_id,
        )
    } else {
        (
            &settings.post_process_prompts,
            &settings.post_process_selected_prompt_id,
        )
    };

    let selected_prompt_id = match selected_prompt_id {
        Some(id) => id.clone(),
        None => {
            debug!("Post-processing skipped because no prompt is selected");
            return None;
        }
    };

    let prompt = match prompts
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

    // Ask these providers to skip reasoning/thinking — post-processing rarely
    // benefits from it and it adds seconds of latency. llm_client picks the
    // field the endpoint understands and retries without it if rejected.
    let disable_reasoning = matches!(provider.id.as_str(), "custom" | "openrouter");

    if provider.supports_structured_output {
        debug!("Using structured outputs for provider '{}'", provider.id);

        let system_prompt = if context_scribe {
            build_system_prompt(&prompt, selection)
        } else {
            build_post_process_system_prompt(&prompt)
        };
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
            disable_reasoning,
        )
        .await
        {
            Ok(Some(content)) => {
                // Parse the JSON response to extract the transcription field
                let content = strip_think_block(&content);
                match serde_json::from_str::<serde_json::Value>(content) {
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
                            return Some(strip_invisible_chars(content));
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to parse structured output JSON: {}. Returning raw content.",
                            e
                        );
                        return Some(strip_invisible_chars(content));
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

    // Legacy mode: the whole template is the user message.
    // Context scribe fills `${output}` and `${selection}`. Regular post-processing
    // fills only `${output}`.
    let processed_prompt = if context_scribe {
        expand_prompt_template(&prompt, transcription, selection)
    } else {
        expand_output_placeholder(&prompt, transcription)
    };
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    match crate::llm_client::send_chat_completion(
        &provider,
        api_key,
        &model,
        processed_prompt,
        disable_reasoning,
    )
    .await
    {
        Ok(Some(content)) => {
            let content = strip_invisible_chars(strip_think_block(&content));
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

pub(crate) struct ProcessedTranscription {
    pub final_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
    /// Paste with Ctrl/Cmd+V and without a trailing space or auto-submit.
    pub replace_selection: bool,
    /// The captured selection was over the limit, so the model was not called.
    pub selection_too_long: bool,
}

pub(crate) async fn process_transcription_output(
    app: &AppHandle,
    transcription: &str,
    post_process: bool,
    selection: Option<String>,
    context_scribe: bool,
) -> ProcessedTranscription {
    let settings = get_settings(app);
    let selection_text = selection.as_deref().unwrap_or("");
    let selection_too_long = context_scribe
        && selection_is_too_long(selection_text, settings.context_scribe_max_selection_chars);

    if selection_too_long {
        debug!(
            "Context scribe refused a selection of {} characters (limit {})",
            selection_text.chars().count(),
            settings.context_scribe_max_selection_chars
        );
        return ProcessedTranscription {
            final_text: String::new(),
            post_processed_text: None,
            post_process_prompt: None,
            replace_selection: false,
            selection_too_long: true,
        };
    }

    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;
    let mut llm_output: Option<String> = None;

    if post_process {
        if let Some(processed_text) =
            post_process_transcription(&settings, transcription, selection_text, context_scribe)
                .await
        {
            post_processed_text = Some(processed_text.clone());
            llm_output = Some(processed_text);

            let (prompts, selected_id) = if context_scribe {
                (
                    &settings.context_scribe_prompts,
                    &settings.context_scribe_selected_prompt_id,
                )
            } else {
                (
                    &settings.post_process_prompts,
                    &settings.post_process_selected_prompt_id,
                )
            };
            if let Some(prompt_id) = selected_id {
                if let Some(prompt) = prompts.iter().find(|prompt| &prompt.id == prompt_id) {
                    post_process_prompt = Some(prompt.prompt.clone());
                }
            }
        }
    }

    let resolved = resolve_transcription_paste(
        transcription,
        post_process,
        llm_output,
        selection.as_deref(),
        false,
    );

    ProcessedTranscription {
        final_text: resolved.text,
        post_processed_text,
        post_process_prompt,
        replace_selection: resolved.replace_selection,
        selection_too_long: false,
    }
}

/// Tell the user a context-scribe selection was over the limit, and leave it in place.
fn present_selection_too_long(app: &AppHandle, length: usize, limit: u32) {
    warn!(
        "Context scribe selection is {length} characters, over the limit of {limit}. The model was not called and the dictation was not pasted."
    );
    let _ = app.emit(
        "context-scribe-selection-too-long",
        serde_json::json!({ "length": length, "limit": limit }),
    );
    if get_settings(app).overlay_style == OverlayStyle::None {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
        }
    } else {
        crate::overlay::show_notice_overlay(app);
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

        // Don't open the mic if nothing can transcribe the recording; the load
        // kicked off above fails and reports why.
        if !tm.is_model_loaded() {
            let selected_model = get_settings(app).selected_model;
            if let Err(e) = app
                .state::<Arc<ModelManager>>()
                .get_model_path(&selected_model)
            {
                warn!("Not starting recording: no model can transcribe it ({})", e);
                return;
            }
        }

        let binding_id = binding_id.to_string();
        let session = RECORDING_SESSION.fetch_add(1, Ordering::SeqCst) + 1;
        if self.context_scribe {
            // Before the overlay and the microphone. The hotkey chord may still
            // be held; the accessibility/UI Automation path does not care, and
            // the clipboard fallback waits for those modifiers.
            let selected = crate::selection::capture_focused_selection(app);
            debug!(
                "Context scribe captured {} characters of selected text",
                selected.chars().count()
            );
            store_context_selection(session, selected);
        }
        let tray_started = Instant::now();
        set_tray_state(app, TrayIconState::Recording);
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
        // Before the model loads, whether it keeps its accuracy on long audio
        // is unknown, so assume not: the worker decides again with the loaded
        // model, and a wrong guess costs only an idle worker.
        let expected_model = ModelTraits {
            streaming: model_supports_streaming,
            long_form: false,
        };
        if live_decode(expected_model, settings.long_form_chunking) != LiveDecode::None {
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

        let mut recording_error: Option<anyhow::Error> = None;
        let recording_start_time = Instant::now();
        match rm.try_start_recording(&binding_id, vad_policy) {
            Ok(readiness) => {
                debug!(
                    "Recording request accepted in {:?}; waiting for first microphone samples",
                    recording_start_time.elapsed()
                );
                let generation = readiness.generation();
                let app_clone = app.clone();
                let rm_clone = Arc::clone(&rm);
                std::thread::spawn(move || {
                    if !readiness.wait() {
                        debug!("Microphone readiness wait ended without receiving samples");
                        return;
                    }

                    // Development-only preview hook for evaluating the brief
                    // arming animation on hardware that normally starts too fast
                    // to make it visible.
                    #[cfg(debug_assertions)]
                    if let Ok(delay_ms) = std::env::var("HANDY_DEBUG_MIC_READY_DELAY_MS")
                        .unwrap_or_default()
                        .parse::<u64>()
                    {
                        let delay_ms = delay_ms.min(10_000);
                        if delay_ms > 0 {
                            debug!("Delaying microphone-ready cue by {delay_ms}ms for UI preview");
                            std::thread::sleep(Duration::from_millis(delay_ms));
                        }
                    }

                    if !rm_clone.is_recording_readiness_current(generation) {
                        debug!("Microphone became ready for an inactive recording");
                        return;
                    }

                    debug!("Microphone is receiving samples; recording is ready");
                    utils::emit_recording_ready(&app_clone);

                    // The start chime is a readiness cue, so it must follow the
                    // first real input callback rather than Stream::play() or a
                    // fixed delay. The helper returns immediately when feedback
                    // is disabled; mute still follows the same readiness point.
                    if rm_clone.is_recording_readiness_current(generation) {
                        play_feedback_sound_blocking(&app_clone, SoundType::Start);
                    }
                    if rm_clone.is_recording_readiness_current(generation) {
                        rm_clone.apply_mute();
                    }
                });
            }
            Err(e) => {
                debug!("Failed to start recording: {}", e);
                recording_error = Some(e);
            }
        }

        if recording_error.is_none() {
            // Dynamically register the cancel shortcut in a separate task to avoid deadlock
            shortcut::register_cancel_shortcut(app);
        } else {
            if self.context_scribe {
                let _ = take_context_selection(session);
            }
            // Starting failed (for example due to blocked microphone permissions).
            // Revert UI state so we don't stay stuck in the recording overlay.
            tm.cancel_stream();
            utils::hide_recording_overlay(app);
            set_tray_state(app, TrayIconState::Idle);
            if let Some(err) = recording_error {
                let _ = app.emit(
                    "recording-error",
                    RecordingErrorEvent {
                        error_type: recording_error_type(&err).to_string(),
                        detail: Some(format!("{err:#}")),
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
        // Prevent a slow microphone from emitting a ready event or start chime
        // after the user has already requested stop.
        app.state::<Arc<AudioRecordingManager>>()
            .invalidate_recording_readiness();

        // Unregister the cancel shortcut when transcription stops
        shortcut::unregister_cancel_shortcut(app);

        let stop_time = Instant::now();
        debug!("TranscribeAction::stop called for binding: {}", binding_id);

        let ah = app.clone();
        let rm = Arc::clone(&app.state::<Arc<AudioRecordingManager>>());
        let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
        let hm = Arc::clone(&app.state::<Arc<HistoryManager>>());

        set_tray_state(app, TrayIconState::Transcribing);
        // Stop should give immediate visual feedback. Live streaming can keep
        // the larger panel, but it still switches from listening to a working
        // spinner while the stream finalizes. Non-streaming paths use the
        // compact transcribing pill (None no-ops in show_*).
        let style = get_settings(app).overlay_style;
        // Capture this before finalizing the stream so every later working state
        // targets the same overlay that was shown for this transcription.
        let use_streaming_overlay = should_use_streaming_overlay(style, tm.is_streaming());
        if use_streaming_overlay {
            tm.emit_stream_working(StreamWorkKind::Transcribing);
        } else {
            show_transcribing_overlay(app);
        }

        // Unmute before playing audio feedback so the stop sound is audible
        rm.remove_mute();

        // Play audio feedback for recording stop
        play_feedback_sound(app, SoundType::Stop);

        let binding_id = binding_id.to_string(); // Clone binding_id for the async task
        let post_process = self.post_process;
        let context_scribe = self.context_scribe;
        let session = RECORDING_SESSION.load(Ordering::SeqCst);
        let cancel_generation = rm.cancel_generation();

        tauri::async_runtime::spawn(async move {
            let _guard = FinishGuard(ah.clone(), Arc::clone(&tm));
            // Take this session's selection before any await so a newer
            // recording can store its own without us consuming it later.
            let selection = if context_scribe {
                take_context_selection(session)
            } else {
                None
            };
            let limit_settings = get_settings(&ah);
            let selection_limit = limit_settings.context_scribe_max_selection_chars;
            let selection_chars = selection.as_deref().unwrap_or("").chars().count();
            let selection_too_long = context_scribe
                && selection_is_too_long(selection.as_deref().unwrap_or(""), selection_limit);
            debug!(
                "Starting async transcription task for binding: {}",
                binding_id
            );

            let stop_recording_time = Instant::now();
            if let Some(samples) = rm.stop_recording(&binding_id, cancel_generation) {
                // Read now: by the time transcribing ends, another take may
                // have replaced it.
                let input_silent = rm.last_input_silent();
                debug!(
                    "Recording stopped and samples retrieved in {:?}, sample count: {}",
                    stop_recording_time.elapsed(),
                    samples.len()
                );

                if rm.was_cancelled_since(cancel_generation) {
                    debug!("Transcription operation cancelled after recording stop");
                    tm.cancel_stream();
                    utils::hide_recording_overlay(&ah);
                    set_tray_state(&ah, TrayIconState::Idle);
                    return;
                }

                if samples.is_empty() {
                    debug!("Recording produced no audio samples; skipping persistence");
                    // Tear down any streaming worker so its channel doesn't leak
                    // and block the next start_stream.
                    tm.cancel_stream();
                    if selection_too_long {
                        present_selection_too_long(&ah, selection_chars, selection_limit);
                    } else {
                        utils::hide_recording_overlay(&ah);
                    }
                    set_tray_state(&ah, TrayIconState::Idle);
                    emit_transcription_complete(&ah, empty_outcome(input_silent), 0, stop_time);
                } else {
                    // Save WAV concurrently with transcription
                    let sample_count = samples.len();
                    let file_name = format!("handy-{}.wav", chrono::Utc::now().timestamp());
                    let wav_path = hm.recordings_dir().join(&file_name);
                    let wav_path_for_verify = wav_path.clone();
                    let samples_for_wav = samples.clone();
                    let wav_handle = tauri::async_runtime::spawn_blocking(move || {
                        crate::audio_toolkit::save_wav_file(&wav_path, &samples_for_wav)
                    });

                    // Transcribe concurrently with WAV save: the live decode's
                    // result when it has one (all audio was already fed to it),
                    // otherwise a batch run of the samples. A cancel is surfaced
                    // as an error, so it never starts a batch run.
                    let transcription_time = Instant::now();
                    let transcription_result = tm.transcribe_recording(samples);

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
                        set_tray_state(&ah, TrayIconState::Idle);
                        return;
                    }

                    match transcription_result {
                        Ok(Transcription {
                            text: transcription,
                            language,
                        }) => {
                            debug!(
                                "Transcription completed in {:?}: '{}'",
                                transcription_time.elapsed(),
                                utils::redact_text(&transcription)
                            );

                            if post_process && !selection_too_long {
                                if use_streaming_overlay {
                                    tm.emit_stream_working(StreamWorkKind::Polishing);
                                } else {
                                    show_processing_overlay(&ah);
                                }
                            }
                            let selection_for_diff = selection.clone();
                            let Some(processed) = complete_unless_cancelled(
                                process_transcription_output(
                                    &ah,
                                    &transcription,
                                    post_process,
                                    selection,
                                    context_scribe,
                                ),
                                || rm.was_cancelled_since(cancel_generation),
                            )
                            .await
                            else {
                                debug!("Transcription operation cancelled during output handling");
                                utils::hide_recording_overlay(&ah);
                                set_tray_state(&ah, TrayIconState::Idle);
                                return;
                            };

                            if rm.was_cancelled_since(cancel_generation) {
                                debug!("Transcription operation cancelled before paste");
                                utils::hide_recording_overlay(&ah);
                                set_tray_state(&ah, TrayIconState::Idle);
                                return;
                            }

                            let diff = crate::text_diff::result_overlay(
                                context_scribe,
                                selection_for_diff.as_deref().unwrap_or(""),
                                &transcription,
                                processed.post_processed_text.as_deref(),
                            );
                            // Save to history if WAV was saved
                            if wav_saved {
                                if let Err(err) = hm.save_entry(
                                    file_name,
                                    transcription,
                                    post_process,
                                    processed.post_processed_text.clone(),
                                    processed.post_process_prompt.clone(),
                                    language,
                                ) {
                                    error!("Failed to save history entry: {}", err);
                                }
                            }

                            if processed.final_text.is_empty() {
                                if processed.selection_too_long {
                                    present_selection_too_long(
                                        &ah,
                                        selection_chars,
                                        selection_limit,
                                    );
                                } else {
                                    utils::hide_recording_overlay(&ah);
                                }
                                set_tray_state(&ah, TrayIconState::Idle);
                                emit_transcription_complete(
                                    &ah,
                                    empty_outcome(input_silent),
                                    sample_count,
                                    stop_time,
                                );
                            } else {
                                let showed_diff = match diff {
                                    Some(diff) => crate::overlay::show_diff_overlay(&ah, diff),
                                    None => false,
                                };
                                let ah_clone = ah.clone();
                                let paste_time = Instant::now();
                                let final_text = processed.final_text;
                                let replace_selection = processed.replace_selection;
                                let rm_for_paste = Arc::clone(&rm);
                                // Taken before the paste thread exists, and so
                                // before the next session can start.
                                let session = RECORDING_SESSION.load(Ordering::SeqCst);
                                // Off the main thread: paste sleeps between steps, and a
                                // paste into Handy's own windows needs the main thread free.
                                std::thread::Builder::new()
                                    .name("paste".into())
                                    .spawn(move || {
                                        if rm_for_paste.was_cancelled_since(cancel_generation) {
                                            debug!(
                                                "Transcription operation cancelled before paste"
                                            );
                                            utils::hide_recording_overlay(&ah_clone);
                                            set_tray_state(&ah_clone, TrayIconState::Idle);
                                            return;
                                        }

                                        let outcome = match if replace_selection {
                                            utils::paste_replacing_selection(
                                                final_text,
                                                ah_clone.clone(),
                                            )
                                        } else {
                                            utils::paste(final_text, ah_clone.clone())
                                        } {
                                            Ok(()) => {
                                                debug!(
                                                    "Text pasted successfully in {:?}",
                                                    paste_time.elapsed()
                                                );
                                                TranscriptionOutcome::Typed
                                            }
                                            Err(e) => {
                                                error!("Failed to paste transcription: {}", e);
                                                let _ = ah_clone.emit("paste-error", ());
                                                TranscriptionOutcome::Failed
                                            }
                                        };
                                        // A new session may have started while
                                        // this paste ran; the UI is now its.
                                        // A result diff hides itself on a timer
                                        // and must survive this paste finishing.
                                        if RECORDING_SESSION.load(Ordering::SeqCst) == session {
                                            if !showed_diff {
                                                utils::hide_recording_overlay(&ah_clone);
                                            }
                                            set_tray_state(&ah_clone, TrayIconState::Idle);
                                        }
                                        emit_transcription_complete(
                                            &ah_clone,
                                            outcome,
                                            sample_count,
                                            stop_time,
                                        );
                                    })
                                    .map(|_| ())
                                    .unwrap_or_else(|e| {
                                        error!("Failed to start the paste thread: {:?}", e);
                                        if !showed_diff {
                                            utils::hide_recording_overlay(&ah);
                                        }
                                        set_tray_state(&ah, TrayIconState::Idle);
                                        emit_transcription_complete(
                                            &ah,
                                            TranscriptionOutcome::Failed,
                                            sample_count,
                                            stop_time,
                                        );
                                    });
                            }
                        }
                        Err(err) => {
                            if rm.was_cancelled_since(cancel_generation) {
                                debug!(
                                    "Transcription operation cancelled after transcription error"
                                );
                                utils::hide_recording_overlay(&ah);
                                set_tray_state(&ah, TrayIconState::Idle);
                                return;
                            }

                            error!("Transcription failed: {}", err);
                            // Surface the failure to the UI (toast). The full
                            // message is also in handy.log via the line above.
                            let _ = ah.emit("transcription-error", err.to_string());
                            // Save entry with empty text so user can retry
                            if wav_saved {
                                if let Err(save_err) = hm.save_entry(
                                    file_name,
                                    String::new(),
                                    post_process,
                                    None,
                                    None,
                                    None,
                                ) {
                                    error!("Failed to save failed history entry: {}", save_err);
                                }
                            }
                            if selection_too_long {
                                present_selection_too_long(&ah, selection_chars, selection_limit);
                            } else {
                                utils::hide_recording_overlay(&ah);
                            }
                            set_tray_state(&ah, TrayIconState::Idle);
                            emit_transcription_complete(
                                &ah,
                                TranscriptionOutcome::Failed,
                                sample_count,
                                stop_time,
                            );
                        }
                    }
                }
            } else {
                debug!("No samples retrieved from recording stop");
                // Tear down any streaming worker so its channel doesn't leak.
                tm.cancel_stream();
                if selection_too_long && !rm.was_cancelled_since(cancel_generation) {
                    present_selection_too_long(&ah, selection_chars, selection_limit);
                } else {
                    utils::hide_recording_overlay(&ah);
                }
                set_tray_state(&ah, TrayIconState::Idle);
                if !rm.was_cancelled_since(cancel_generation) {
                    emit_transcription_complete(&ah, TranscriptionOutcome::Empty, 0, stop_time);
                }
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
            context_scribe: false,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_post_process".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            context_scribe: false,
        }) as Arc<dyn ShortcutAction>,
    );
    map.insert(
        "transcribe_with_context_scribe".to_string(),
        Arc::new(TranscribeAction {
            post_process: true,
            context_scribe: true,
        }) as Arc<dyn ShortcutAction>,
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
    use super::{
        build_post_process_system_prompt, build_system_prompt, complete_unless_cancelled,
        expand_output_placeholder, expand_prompt_template, is_blank_transcription,
        resolve_transcription_paste, selection_is_too_long, should_use_streaming_overlay,
        strip_think_block, TranscriptionPaste,
    };
    use crate::settings::OverlayStyle;
    use std::future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

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
    fn completed_operation_returns_its_output() {
        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::ready("done"),
            || false,
        ));

        assert_eq!(result, Some("done"));
    }

    #[test]
    fn pending_operation_stops_after_cancellation() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_thread = Arc::clone(&cancelled);
        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            cancelled_for_thread.store(true, Ordering::Release);
        });

        let result = tauri::async_runtime::block_on(complete_unless_cancelled(
            future::pending::<()>(),
            || cancelled.load(Ordering::Acquire),
        ));

        cancel_thread.join().unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn leading_think_block_is_stripped() {
        assert_eq!(
            strip_think_block("<think>pondering...</think>Cleaned text."),
            "Cleaned text."
        );
        assert_eq!(
            strip_think_block("  \n<think>multi\nline</think>\n  Cleaned text."),
            "Cleaned text."
        );
    }

    #[test]
    fn content_without_think_block_is_unchanged() {
        assert_eq!(strip_think_block("Cleaned text."), "Cleaned text.");
        assert_eq!(
            strip_think_block("Mentions <think> mid-sentence."),
            "Mentions <think> mid-sentence."
        );
        // Unclosed block: leave untouched rather than guess
        assert_eq!(
            strip_think_block("<think>never closed"),
            "<think>never closed"
        );
    }

    #[test]
    fn live_overlay_uses_streaming_states_only_for_streaming_models() {
        assert!(should_use_streaming_overlay(OverlayStyle::Live, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::Live, false));
        assert!(!should_use_streaming_overlay(OverlayStyle::Minimal, true));
        assert!(!should_use_streaming_overlay(OverlayStyle::None, true));
    }

    #[test]
    fn prompt_expansion_fills_output_and_selection_without_rescanning() {
        let template = "Instruction.\n<selection>\n${selection}\n</selection>\n<transcript>\n${output}\n</transcript>";
        let expanded = expand_prompt_template(
            template,
            "say ${selection} please",
            "keep ${output} literal",
        );
        assert_eq!(
            expanded,
            "Instruction.\n<selection>\nkeep ${output} literal\n</selection>\n<transcript>\nsay ${selection} please\n</transcript>"
        );

        let system = build_system_prompt(template, "keep ${output} literal");
        assert!(!system.contains("${selection}"));
        assert!(!system.contains("${output}\n"));
        assert!(system.contains("keep ${output} literal"));
        assert!(system.contains("<transcript>"));
    }

    #[test]
    fn legacy_prompt_without_selection_placeholder_is_unchanged() {
        let template = "Clean this:\n${output}";
        assert_eq!(
            expand_prompt_template(template, "hello", "selected paragraph"),
            "Clean this:\nhello"
        );
    }

    #[test]
    fn llm_none_with_selection_does_not_paste_raw_dictation() {
        let paste = resolve_transcription_paste(
            "make it shorter",
            true,
            None,
            Some("the selected paragraph"),
            false,
        );
        assert_eq!(
            paste,
            TranscriptionPaste {
                text: String::new(),
                replace_selection: false,
            }
        );
    }

    #[test]
    fn blank_llm_reply_does_not_replace_a_selection() {
        let paste = resolve_transcription_paste(
            "make it shorter",
            true,
            Some("  \n".to_string()),
            Some("the selected paragraph"),
            false,
        );
        assert_eq!(paste.text, "");
        assert!(!paste.replace_selection);
    }

    #[test]
    fn llm_text_replaces_a_non_empty_selection() {
        let paste = resolve_transcription_paste(
            "make it shorter",
            true,
            Some("shorter paragraph".to_string()),
            Some("the selected paragraph"),
            false,
        );
        assert_eq!(
            paste,
            TranscriptionPaste {
                text: "shorter paragraph".to_string(),
                replace_selection: true,
            }
        );
    }

    #[test]
    fn llm_none_without_selection_pastes_the_dictation() {
        let paste = resolve_transcription_paste("make it shorter", true, None, Some(""), false);
        assert_eq!(paste.text, "make it shorter");
        assert!(!paste.replace_selection);

        let missing = resolve_transcription_paste("make it shorter", true, None, None, false);
        assert_eq!(missing.text, "make it shorter");
        assert!(!missing.replace_selection);
    }

    #[test]
    fn post_process_expansion_fills_only_the_transcript() {
        let template = "Clean ${output}. Keep ${selection} literal.";
        assert_eq!(
            expand_output_placeholder(template, "hello"),
            "Clean hello. Keep ${selection} literal."
        );
        let system = build_post_process_system_prompt(template);
        assert!(system.contains("${selection}"));
        assert!(!system.contains("${output}"));
    }

    #[test]
    fn overlong_selection_does_not_paste_raw_dictation() {
        assert!(!selection_is_too_long(&"a".repeat(2000), 2000));
        assert!(selection_is_too_long(&"a".repeat(2001), 2000));
        // One Unicode scalar, even though it is two UTF-8 bytes.
        assert!(!selection_is_too_long("é", 1));
        assert!(selection_is_too_long("éé", 1));

        let paste = resolve_transcription_paste(
            "make it shorter",
            true,
            Some("should not be used".to_string()),
            Some(&"a".repeat(2001)),
            true,
        );
        assert_eq!(
            paste,
            TranscriptionPaste {
                text: String::new(),
                replace_selection: false,
            }
        );
    }
}
