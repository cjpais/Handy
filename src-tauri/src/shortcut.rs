use serde::Serialize;
use tauri::{App, AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState};

use crate::actions::ACTION_MAP;
use crate::settings::ShortcutBinding;
use crate::settings::{self, get_settings, OverlayPosition};
use crate::ManagedToggleState;

#[cfg(target_os = "macos")]
use crate::fn_monitor;

pub fn init_shortcuts(app: &App) {
    let settings = settings::load_or_create_app_settings(app);

    // Register shortcuts with the bindings from settings
    for (_id, binding) in settings.bindings {
        // Pass app.handle() which is &AppHandle
        if let Err(e) = _register_shortcut(app.handle(), binding) {
            eprintln!("Failed to register shortcut {} during init: {}", _id, e);
        }
    }
}

#[derive(Serialize)]
pub struct BindingResponse {
    success: bool,
    binding: Option<ShortcutBinding>,
    error: Option<String>,
}

#[tauri::command]
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
            let error_msg = format!("Binding with id '{}' not found", id);
            eprintln!("change_binding error: {}", error_msg);
            return Ok(BindingResponse {
                success: false,
                binding: None,
                error: Some(error_msg),
            });
        }
    };

    // Unregister the existing binding
    if let Err(e) = _unregister_shortcut(&app, binding_to_modify.clone()) {
        let error_msg = format!("Failed to unregister shortcut: {}", e);
        eprintln!("change_binding error: {}", error_msg);
    }

    // Validate the new shortcut before we touch the current registration
    if let Err(e) = validate_shortcut_string(&binding) {
        eprintln!("change_binding validation error: {}", e);
        return Err(e);
    }

    // Create an updated binding
    let mut updated_binding = binding_to_modify;
    updated_binding.current_binding = binding;

    // Register the new binding
    if let Err(e) = _register_shortcut(&app, updated_binding.clone()) {
        let error_msg = format!("Failed to register shortcut: {}", e);
        eprintln!("change_binding error: {}", error_msg);
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
pub fn reset_binding(app: AppHandle, id: String) -> Result<BindingResponse, String> {
    let binding = settings::get_stored_binding(&app, &id);

    return change_binding(app, id, binding.default_binding);
}

#[tauri::command]
pub fn change_ptt_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);

    // TODO if the setting is currently false, we probably want to
    // cancel any ongoing recordings or actions
    settings.push_to_talk = enabled;

    settings::write_settings(&app, settings);

    Ok(())
}

#[tauri::command]
pub fn change_audio_feedback_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.audio_feedback = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
pub fn change_translate_to_english_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.translate_to_english = enabled;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
pub fn change_selected_language_setting(app: AppHandle, language: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.selected_language = language;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
pub fn change_overlay_position_setting(app: AppHandle, position: String) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    let parsed = match position.as_str() {
        "none" => OverlayPosition::None,
        "top" => OverlayPosition::Top,
        "bottom" => OverlayPosition::Bottom,
        other => {
            eprintln!("Invalid overlay position '{}', defaulting to bottom", other);
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
pub fn update_custom_words(app: AppHandle, words: Vec<String>) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.custom_words = words;
    settings::write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
pub fn change_word_correction_threshold_setting(
    app: AppHandle,
    threshold: f64,
) -> Result<(), String> {
    let mut settings = settings::get_settings(&app);
    settings.word_correction_threshold = threshold;
    settings::write_settings(&app, settings);
    Ok(())
}

/// Determine whether a shortcut string contains at least one non-modifier key.
/// We allow single non-modifier keys (e.g. "f5" or "space") but disallow
/// modifier-only combos (e.g. "ctrl" or "ctrl+shift").
fn validate_shortcut_string(raw: &str) -> Result<(), String> {
    if cfg!(target_os = "macos") && raw.trim().eq_ignore_ascii_case("fn") {
        return Ok(());
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
pub fn suspend_binding(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(b) = settings::get_bindings(&app).get(&id).cloned() {
        if let Err(e) = _unregister_shortcut(&app, b) {
            eprintln!("suspend_binding error for id '{}': {}", id, e);
            return Err(e);
        }
    }
    Ok(())
}

/// Re-register the binding after the user has finished editing.
#[tauri::command]
pub fn resume_binding(app: AppHandle, id: String) -> Result<(), String> {
    if let Some(b) = settings::get_bindings(&app).get(&id).cloned() {
        #[cfg(target_os = "macos")]
        if b.current_binding.eq_ignore_ascii_case("fn") {
            if let Err(e) = fn_monitor::register_fn_binding(&app, b) {
                eprintln!("resume_binding error for id '{}': {}", id, e);
                return Err(e);
            }
            return Ok(());
        }

        #[cfg(not(target_os = "macos"))]
        if b.current_binding.eq_ignore_ascii_case("fn") {
            return Ok(());
        }

        let already_registered = b
            .current_binding
            .parse::<Shortcut>()
            .map(|shortcut| app.global_shortcut().is_registered(shortcut))
            .unwrap_or(false);

        if already_registered {
            return Ok(());
        }

        if let Err(e) = _register_shortcut(&app, b) {
            eprintln!("resume_binding error for id '{}': {}", id, e);
            return Err(e);
        }
    }
    Ok(())
}

fn _register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    // Validate human-level rules first
    if let Err(e) = validate_shortcut_string(&binding.current_binding) {
        eprintln!(
            "_register_shortcut validation error for binding '{}': {}",
            binding.current_binding, e
        );
        return Err(e);
    }

    #[cfg(target_os = "macos")]
    {
        if binding.current_binding.eq_ignore_ascii_case("fn") {
            return fn_monitor::register_fn_binding(app, binding);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if binding.current_binding.eq_ignore_ascii_case("fn") {
            let error_msg = "Fn bindings are only supported on macOS".to_string();
            eprintln!("_register_shortcut platform error: {}", error_msg);
            return Err(error_msg);
        }
    }

    // Parse shortcut and return error if it fails
    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!(
                "Failed to parse shortcut '{}': {}",
                binding.current_binding, e
            );
            eprintln!("_register_shortcut parse error: {}", error_msg);
            return Err(error_msg);
        }
    };

    // Prevent duplicate registrations that would silently shadow one another
    if app.global_shortcut().is_registered(shortcut) {
        let error_msg = format!("Shortcut '{}' is already in use", binding.current_binding);
        eprintln!("_register_shortcut duplicate error: {}", error_msg);
        return Err(error_msg);
    }

    // Clone binding.id for use in the closure
    let binding_id_for_closure = binding.id.clone();

    app.global_shortcut()
        .on_shortcut(shortcut, move |ah, scut, event| {
            if scut == &shortcut {
                let shortcut_string = scut.into_string();
                dispatch_binding_event(
                    ah,
                    &binding_id_for_closure,
                    &shortcut_string,
                    event.state,
                );
            }
        })
        .map_err(|e| {
            let error_msg = format!("Couldn't register shortcut '{}': {}", binding.current_binding, e);
            eprintln!("_register_shortcut registration error: {}", error_msg);
            error_msg
        })?;

    Ok(())
}

pub(crate) fn dispatch_binding_event(
    app: &AppHandle,
    binding_id: &str,
    shortcut_string: &str,
    state: ShortcutState,
) {
    let settings = get_settings(app);

    if let Some(action) = ACTION_MAP.get(binding_id) {
        if settings.push_to_talk {
            match state {
                ShortcutState::Pressed => {
                    action.start(app, binding_id, shortcut_string);
                }
                ShortcutState::Released => {
                    action.stop(app, binding_id, shortcut_string);
                }
            }
        } else if state == ShortcutState::Pressed {
            let toggle_state_manager = app.state::<ManagedToggleState>();
            let mut states = toggle_state_manager
                .lock()
                .expect("Failed to lock toggle state manager");

            let is_currently_active = states
                .active_toggles
                .entry(binding_id.to_string())
                .or_insert(false);

            if *is_currently_active {
                action.stop(app, binding_id, shortcut_string);
                *is_currently_active = false;
            } else {
                action.start(app, binding_id, shortcut_string);
                *is_currently_active = true;
            }
        }
    } else {
        println!(
            "Warning: No action defined in ACTION_MAP for shortcut ID '{}'. Shortcut: '{}', State: {:?}",
            binding_id, shortcut_string, state
        );
    }
}

fn _unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if binding.current_binding.eq_ignore_ascii_case("fn") {
            return fn_monitor::unregister_fn_binding(app, &binding.id);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if binding.current_binding.eq_ignore_ascii_case("fn") {
            // No-op on unsupported platforms; binding shouldn't exist but avoid crashing.
            return Ok(());
        }
    }

    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!(
                "Failed to parse shortcut '{}' for unregistration: {}",
                binding.current_binding, e
            );
            eprintln!("_unregister_shortcut parse error: {}", error_msg);
            return Err(error_msg);
        }
    };

    app.global_shortcut().unregister(shortcut).map_err(|e| {
        let error_msg = format!(
            "Failed to unregister shortcut '{}': {}",
            binding.current_binding, e
        );
        eprintln!("_unregister_shortcut error: {}", error_msg);
        error_msg
    })?;

    Ok(())
}
