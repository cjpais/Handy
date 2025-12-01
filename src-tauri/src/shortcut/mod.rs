use log::{debug, error, warn};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::actions::ACTION_MAP;
use crate::settings::ShortcutBinding;
use crate::settings::{
    self, get_settings, ClipboardHandling, LLMPrompt, OverlayPosition, PasteMethod, SoundTheme,
};
use crate::ManagedToggleState;

#[cfg(target_os = "macos")]
mod fn_monitor;

/// Check if a binding string represents an fn-key-only binding (macOS)
fn is_fn_binding(binding: &str) -> bool {
    binding.eq_ignore_ascii_case("fn")
}

/// Register a binding, routing to the appropriate handler based on binding type
fn register_binding(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    debug!(
        "register_binding: id='{}', current_binding='{}'",
        binding.id, binding.current_binding
    );

    #[cfg(target_os = "macos")]
    if is_fn_binding(&binding.current_binding) {
        return fn_monitor::register_fn_binding(app, binding);
    }

    _register_shortcut(app, binding)
}

/// Unregister a binding, routing to the appropriate handler based on binding type
fn unregister_binding(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if is_fn_binding(&binding.current_binding) {
        return fn_monitor::unregister_fn_binding(app, &binding.id);
    }

    unregister_shortcut(app, binding)
}

pub fn init_shortcuts(app: &AppHandle) {
    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    // Register all default shortcuts, applying user customizations
    // Skip dynamic bindings - they are registered at runtime when needed
    for (id, default_binding) in default_bindings {
        let binding = user_settings
            .bindings
            .get(&id)
            .cloned()
            .unwrap_or(default_binding);

        if binding.dynamic {
            debug!("Skipping dynamic binding '{id}' during init");
            continue;
        }

        if let Err(e) = register_binding(app, binding) {
            error!("Failed to register shortcut {id} during init: {e}");
        }
    }
}

#[derive(Serialize, Type)]
pub struct BindingResponse {
    success: bool,
    binding: Option<ShortcutBinding>,
    error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn change_binding(
    app: AppHandle,
    id: String,
    binding: String,
) -> Result<BindingResponse, String> {
    let mut settings = settings::get_settings(&app);

    // Get the binding to modify
    let binding_to_modify = match settings.bindings.get(&id) {
        Some(binding) => binding.clone(),
        None => {
            let error_msg = format!("Binding with id '{id}' not found");
            warn!("change_binding error: {error_msg}");
            return Ok(BindingResponse {
                success: false,
                binding: None,
                error: Some(error_msg),
            });
        }
    };
    // If this is the cancel binding, just update the settings and return
    // It's managed dynamically, so we don't register/unregister here
    if id == "cancel" {
        if let Some(mut b) = settings.bindings.get(&id).cloned() {
            b.current_binding = binding;
            settings.bindings.insert(id.clone(), b.clone());
            settings::write_settings(&app, settings);
            return Ok(BindingResponse {
                success: true,
                binding: Some(b.clone()),
                error: None,
            });
        }
    }

    // Unregister the existing binding (ignore errors - it may not be registered)
    if let Err(e) = unregister_binding(&app, binding_to_modify.clone()) {
        debug!("change_binding: could not unregister existing binding: {e}");
    }

    // Validate the new shortcut before we touch the current registration
    if let Err(e) = validate_shortcut_string(&binding) {
        warn!("change_binding validation error: {e}");
        return Err(e);
    }

    // Create an updated binding
    let mut updated_binding = binding_to_modify;
    updated_binding.current_binding = binding;

    // Register the new binding
    if let Err(e) = register_binding(&app, updated_binding.clone()) {
        let error_msg = format!("Failed to register shortcut: {e}");
        error!("change_binding error: {error_msg}");
        return Ok(BindingResponse {
            success: false,
            binding: None,
            error: Some(error_msg),
        });
    }

    // Update the binding in the settings
    settings.bindings.insert(id, updated_binding.clone());

    // Save the settings
    settings::write_settings(&app, settings);

    // Return the updated binding
    Ok(BindingResponse {
        success: true,
        binding: Some(updated_binding),
        error: None,
    })
}

#[tauri::command]
#[specta::specta]
pub fn reset_binding(app: AppHandle, id: String) -> Result<BindingResponse, String> {
    let binding = settings::get_stored_binding(&app, &id);

    change_binding(app, id, binding.default_binding)
}

#[tauri::command]
#[specta::specta]
pub fn change_ptt_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // TODO if the setting is currently false, we probably want to
    // cancel any ongoing recordings or actions
    settings.push_to_talk = enabled;

    settings::write_settings(&app, settings);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_audio_feedback_volume_setting(app: AppHandle, volume: f32) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback_volume = volume;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_sound_theme_setting(app: AppHandle, theme: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match theme.as_str() {
        "marimba" => SoundTheme::Marimba,
        "pop" => SoundTheme::Pop,
        "custom" => SoundTheme::Custom,
        other => {
            warn!("Invalid sound theme '{other}', defaulting to marimba");
            SoundTheme::Marimba
        }
    };
    settings.sound_theme = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_translate_to_english_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.translate_to_english = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_selected_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.selected_language = language;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_overlay_position_setting(app: AppHandle, position: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match position.as_str() {
        "none" => OverlayPosition::None,
        "top" => OverlayPosition::Top,
        "bottom" => OverlayPosition::Bottom,
        other => {
            warn!("Invalid overlay position '{other}', defaulting to bottom");
            OverlayPosition::Bottom
        }
    };
    settings.overlay_position = parsed;
    settings::write_settings(&app, settings);

    // Update overlay position without recreating window
    crate::utils::update_overlay_position(&app);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_debug_mode_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.debug_mode = enabled;
    settings::write_settings(&app, settings);

    // Emit event to notify frontend of debug mode change
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "debug_mode",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_start_hidden_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.start_hidden = enabled;
    settings::write_settings(&app, settings);

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "start_hidden",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_autostart_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.autostart_enabled = enabled;
    settings::write_settings(&app, settings);

    // Apply the autostart setting immediately
    let autostart_manager = app.autolaunch();
    if enabled {
        let _ = autostart_manager.enable();
    } else {
        let _ = autostart_manager.disable();
    }

    // Notify frontend
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "autostart_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_update_checks_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.update_checks_enabled = enabled;
    settings::write_settings(&app, settings);

    let _ = app.emit(
        "settings-changed",
        serde_json::json!({
            "setting": "update_checks_enabled",
            "value": enabled
        }),
    );

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn update_custom_words(app: AppHandle, words: Vec<String>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.custom_words = words;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_word_correction_threshold_setting(
    app: AppHandle,
    threshold: f64,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.word_correction_threshold = threshold;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_paste_method_setting(app: AppHandle, method: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match method.as_str() {
        "ctrl_v" => PasteMethod::CtrlV,
        "direct" => PasteMethod::Direct,
        "none" => PasteMethod::None,
        "shift_insert" => PasteMethod::ShiftInsert,
        other => {
            warn!("Invalid paste method '{other}', defaulting to ctrl_v");
            PasteMethod::CtrlV
        }
    };
    settings.paste_method = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_clipboard_handling_setting(app: AppHandle, handling: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match handling.as_str() {
        "dont_modify" => ClipboardHandling::DontModify,
        "copy_to_clipboard" => ClipboardHandling::CopyToClipboard,
        other => {
            warn!("Invalid clipboard handling '{other}', defaulting to dont_modify");
            ClipboardHandling::DontModify
        }
    };
    settings.clipboard_handling = parsed;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.post_process_enabled = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_base_url_setting(
    app: AppHandle,
    provider_id: String,
    base_url: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let label = settings
        .post_process_provider(&provider_id)
        .map(|provider| provider.label.clone())
        .ok_or_else(|| format!("Provider '{provider_id}' not found"))?;

    let provider = settings
        .post_process_provider_mut(&provider_id)
        .expect("Provider looked up above must exist");

    if !provider.allow_base_url_edit {
        return Err(format!(
            "Provider '{label}' does not allow editing the base URL"
        ));
    }

    provider.base_url = base_url;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Generic helper to validate provider exists
fn validate_provider_exists(
    settings: &settings::AppSettings,
    provider_id: &str,
) -> Result<(), String> {
    if !settings
        .post_process_providers
        .iter()
        .any(|provider| provider.id == provider_id)
    {
        return Err(format!("Provider '{provider_id}' not found"));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_api_key_setting(
    app: AppHandle,
    provider_id: String,
    api_key: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.post_process_api_keys.insert(provider_id, api_key);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_post_process_model_setting(
    app: AppHandle,
    provider_id: String,
    model: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.post_process_models.insert(provider_id, model);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_post_process_provider(app: AppHandle, provider_id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    validate_provider_exists(&settings, &provider_id)?;
    settings.post_process_provider_id = provider_id;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn add_post_process_prompt(
    app: AppHandle,
    name: String,
    prompt: String,
) -> Result<LLMPrompt, String> {
    let mut settings = settings::get_settings(&app);

    // Generate unique ID using timestamp and random component
    let id = format!("prompt_{}", chrono::Utc::now().timestamp_millis());

    let new_prompt = LLMPrompt {
        id: id.clone(),
        name,
        prompt,
    };

    settings.post_process_prompts.push(new_prompt.clone());
    settings::write_settings(&app, settings);

    Ok(new_prompt)
}

#[tauri::command]
#[specta::specta]
pub fn update_post_process_prompt(
    app: AppHandle,
    id: String,
    name: String,
    prompt: String,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    if let Some(existing_prompt) = settings
        .post_process_prompts
        .iter_mut()
        .find(|p| p.id == id)
    {
        existing_prompt.name = name;
        existing_prompt.prompt = prompt;
        settings::write_settings(&app, settings);
        Ok(())
    } else {
        Err(format!("Prompt with id '{id}' not found"))
    }
}

#[tauri::command]
#[specta::specta]
pub fn delete_post_process_prompt(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // Don't allow deleting the last prompt
    if settings.post_process_prompts.len() <= 1 {
        return Err("Cannot delete the last prompt".to_string());
    }

    // Find and remove the prompt
    let original_len = settings.post_process_prompts.len();
    settings.post_process_prompts.retain(|p| p.id != id);

    if settings.post_process_prompts.len() == original_len {
        return Err(format!("Prompt with id '{id}' not found"));
    }

    // If the deleted prompt was selected, select the first one or None
    if settings.post_process_selected_prompt_id.as_ref() == Some(&id) {
        settings.post_process_selected_prompt_id =
            settings.post_process_prompts.first().map(|p| p.id.clone());
    }

    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_post_process_models(
    app: AppHandle,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let settings = settings::get_settings(&app);

    // Find the provider
    let provider = settings
        .post_process_providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' not found"))?;

    // Get API key
    let api_key = settings
        .post_process_api_keys
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();

    // Skip fetching if no API key for providers that typically need one
    if api_key.trim().is_empty() && provider.id != "custom" {
        return Err(format!(
            "API key is required for {}. Please add an API key to list available models.",
            provider.label
        ));
    }

    // TODO: In the future, we can use async-openai's models API:
    // let client = crate::llm_client::create_client(provider, api_key)?;
    // let response = client.models().list().await?;
    // return Ok(response.data.iter().map(|m| m.id.clone()).collect());

    // For now, use manual HTTP request to have more control over the endpoint
    fetch_models_manual(provider, api_key).await
}

/// Fetch models using manual HTTP request
/// This gives us more control and avoids issues with non-standard endpoints
async fn fetch_models_manual(
    provider: &crate::settings::PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    // Build the endpoint URL
    let base_url = provider.base_url.trim_end_matches('/');
    let models_endpoint = provider
        .models_endpoint
        .as_ref()
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or("models");
    let endpoint = format!("{base_url}/{models_endpoint}");

    // Create HTTP client with headers
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "HTTP-Referer",
        reqwest::header::HeaderValue::from_static("https://github.com/cjpais/Handy"),
    );
    headers.insert(
        "X-Title",
        reqwest::header::HeaderValue::from_static("Handy"),
    );

    // Add provider-specific headers
    if provider.id == "anthropic" {
        if !api_key.is_empty() {
            headers.insert(
                "x-api-key",
                reqwest::header::HeaderValue::from_str(&api_key)
                    .map_err(|e| format!("Invalid API key: {e}"))?,
            );
        }
        headers.insert(
            "anthropic-version",
            reqwest::header::HeaderValue::from_static("2023-06-01"),
        );
    } else if !api_key.is_empty() {
        headers.insert(
            "Authorization",
            reqwest::header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|e| format!("Invalid API key: {e}"))?,
        );
    }

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    // Make the request
    let response = http_client
        .get(&endpoint)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch models: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!(
            "Model list request failed ({status}): {error_text}"
        ));
    }

    // Parse the response
    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let mut models = Vec::new();

    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

#[tauri::command]
#[specta::specta]
pub fn set_post_process_selected_prompt(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // Verify the prompt exists
    if !settings.post_process_prompts.iter().any(|p| p.id == id) {
        return Err(format!("Prompt with id '{id}' not found"));
    }

    settings.post_process_selected_prompt_id = Some(id);
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_mute_while_recording_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.mute_while_recording = enabled;
    settings::write_settings(&app, settings);

    Ok(())
}

/// Determine whether a shortcut string contains at least one non-modifier key.
/// We allow single non-modifier keys (e.g. "f5" or "space") but disallow
/// modifier-only combos (e.g. "ctrl" or "ctrl+shift").
/// Special case: "fn" is allowed as a macOS-specific modifier-only binding.
fn validate_shortcut_string(raw: &str) -> Result<(), String> {
    // "fn" is only valid on macOS
    if is_fn_binding(raw) {
        #[cfg(target_os = "macos")]
        return Ok(());
        #[cfg(not(target_os = "macos"))]
        return Err("The fn key shortcut is only supported on macOS".into());
    }

    let modifiers = [
        "ctrl", "control", "shift", "alt", "option", "meta", "command", "cmd", "super", "win",
        "windows",
    ];
    let has_non_modifier = raw
        .split('+')
        .any(|part| !modifiers.contains(&part.trim().to_lowercase().as_str()));
    if has_non_modifier {
        Ok(())
    } else {
        Err("Shortcut must contain at least one non-modifier key".into())
    }
}

/// Temporarily unregister a binding while the user is editing it in the UI.
/// This avoids firing the action while keys are being recorded.
#[tauri::command]
#[specta::specta]
pub fn suspend_binding(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(b) = settings::get_bindings(&app).get(&id).cloned() {
        if let Err(e) = unregister_binding(&app, b) {
            error!("suspend_binding error for id '{id}': {e}");
            return Err(e);
        }
    }
    Ok(())
}

/// Re-register the binding after the user has finished editing.
#[tauri::command]
#[specta::specta]
pub fn resume_binding(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(b) = settings::get_bindings(&app).get(&id).cloned() {
        if let Err(e) = register_binding(&app, b) {
            error!("resume_binding error for id '{id}': {e}");
            return Err(e);
        }
    }
    Ok(())
}

/// Register a dynamic binding at runtime.
/// Dynamic bindings are shortcuts that are only active during certain states
/// (e.g., cancel shortcut only active while recording).
///
/// This function is idempotent - it will first unregister any existing binding
/// before registering. This allows safe re-registration without needing to
/// explicitly unregister first (which can deadlock if called from inside a
/// shortcut callback).
///
/// Note: Dynamic shortcut registration is disabled on Linux due to instability
/// with the global shortcut plugin. See PR #392.
pub fn register_dynamic_binding(app: &AppHandle, binding_id: &str) -> Result<(), String> {
    // Dynamic shortcut registration is disabled on Linux due to instability
    #[cfg(target_os = "linux")]
    {
        let _ = (app, binding_id);
        debug!(
            "Skipping dynamic binding registration on Linux (disabled for stability): {binding_id}"
        );
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let settings = get_settings(app);

        let binding = settings
            .bindings
            .get(binding_id)
            .ok_or_else(|| format!("Dynamic binding '{binding_id}' not found in settings"))?;

        debug!(
            "register_dynamic_binding: id='{}', binding='{}'",
            binding.id, binding.current_binding
        );

        if !binding.dynamic {
            return Err(format!("Binding '{binding_id}' is not marked as dynamic"));
        }

        // Try to unregister first (ignore errors - might not be registered)
        // This makes registration idempotent and avoids "already in use" errors
        let _ = unregister_binding(app, binding.clone());

        register_binding(app, binding.clone())
    }
}

/// Unregister a dynamic binding at runtime.
///
/// Note: Dynamic shortcut registration is disabled on Linux due to instability.
/// See PR #392.
pub fn unregister_dynamic_binding(app: &AppHandle, binding_id: &str) -> Result<(), String> {
    // Dynamic shortcut registration is disabled on Linux due to instability
    #[cfg(target_os = "linux")]
    {
        let _ = (app, binding_id);
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let settings = get_settings(app);

        let binding = settings
            .bindings
            .get(binding_id)
            .ok_or_else(|| format!("Dynamic binding '{binding_id}' not found in settings"))?;

        debug!("Unregistering dynamic binding: {binding_id}");
        unregister_binding(app, binding.clone())
    }
}

/// Dispatch a binding event to the appropriate action.
/// Shared by both regular shortcuts and fn key monitor.
pub(crate) fn dispatch_binding_event(
    app: &AppHandle,
    binding_id: &str,
    shortcut_string: &str,
    state: ShortcutState,
) {
    debug!(
        "dispatch_binding_event: binding_id='{binding_id}', shortcut='{shortcut_string}', state={state:?}"
    );
    let settings = get_settings(app);

    if let Some(action) = ACTION_MAP.get(binding_id) {
        if settings.push_to_talk {
            // Push-to-talk mode: start on press, stop on release
            if state == ShortcutState::Pressed {
                action.start(app, binding_id, shortcut_string);
            } else if state == ShortcutState::Released {
                action.stop(app, binding_id, shortcut_string);
            }
        } else {
            // Toggle mode: toggle on press only
            if state == ShortcutState::Pressed {
                // Determine action and update state while holding the lock,
                // but RELEASE the lock before calling the action to avoid deadlocks.
                // (Actions may need to acquire the lock themselves, e.g., cancel_current_operation)
                let should_start: bool;
                {
                    let toggle_state_manager = app.state::<ManagedToggleState>();
                    let mut states = toggle_state_manager
                        .lock()
                        .expect("Failed to lock toggle state manager");

                    let is_currently_active = states
                        .active_toggles
                        .entry(binding_id.to_string())
                        .or_insert(false);

                    should_start = !*is_currently_active;
                    *is_currently_active = should_start;
                } // Lock released here

                // Now call the action without holding the lock
                if should_start {
                    action.start(app, binding_id, shortcut_string);
                } else {
                    action.stop(app, binding_id, shortcut_string);
                }
            }
        }
    } else {
        warn!(
            "No action defined in ACTION_MAP for binding ID '{binding_id}'. Shortcut: '{shortcut_string}', State: {state:?}"
        );
    }
}

fn _register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    // Validate human-level rules first
    validate_shortcut_string(&binding.current_binding)?;

    // Parse shortcut and return error if it fails
    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let binding_str = &binding.current_binding;
            return Err(format!("Failed to parse shortcut '{binding_str}': {e}"));
        }
    };

    // Prevent duplicate registrations that would silently shadow one another
    if app.global_shortcut().is_registered(shortcut) {
        let binding_str = &binding.current_binding;
        return Err(format!("Shortcut '{binding_str}' is already in use"));
    }

    // Clone binding info for use in the closure
    let binding_id = binding.id.clone();
    let shortcut_string = binding.current_binding.clone();

    app.global_shortcut()
        .on_shortcut(shortcut, move |ah, scut, event| {
            if scut == &shortcut {
                dispatch_binding_event(ah, &binding_id, &shortcut_string, event.state);
            }
        })
        .map_err(|e| {
            let binding_str = &binding.current_binding;
            format!("Couldn't register shortcut '{binding_str}': {e}")
        })?;

    Ok(())
}

pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let binding_str = &binding.current_binding;
    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let error_msg =
                format!("Failed to parse shortcut '{binding_str}' for unregistration: {e}");
            error!("_unregister_shortcut parse error: {error_msg}");
            return Err(error_msg);
        }
    };

    app.global_shortcut().unregister(shortcut).map_err(|e| {
        let error_msg = format!("Failed to unregister shortcut '{binding_str}': {e}");
        error!("_unregister_shortcut error: {error_msg}");
        error_msg
    })?;

    Ok(())
}
