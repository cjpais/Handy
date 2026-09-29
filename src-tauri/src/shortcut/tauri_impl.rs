//! Tauri global-shortcut implementation
//!
//! This module provides shortcut functionality using Tauri's built-in
//! global-shortcut plugin.

use log::{debug, error, warn};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[cfg(not(target_os = "linux"))]
use crate::settings::get_settings;
use crate::settings::{self, ShortcutBinding};

use super::handler::handle_shortcut_event;

/// Initialize shortcuts using Tauri's global-shortcut plugin
pub fn init_shortcuts(app: &AppHandle) {
    // The load path has already merged in any missing default bindings, so
    // this covers the defaults and every user-added shortcut.
    let user_settings = settings::load_or_create_app_settings(app);

    for (id, binding) in &user_settings.bindings {
        if !user_settings.should_register_binding(id, binding) {
            continue;
        }
        if let Err(e) = register_shortcut(app, binding.clone()) {
            error!("Failed to register shortcut {} during init: {}", id, e);
        }
    }
}

/// Validate a shortcut string for the Tauri global-shortcut implementation.
/// Tauri requires at least one non-modifier key and doesn't support the fn key.
pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }

    let modifiers = [
        "ctrl", "control", "shift", "alt", "option", "meta", "command", "cmd", "super", "win",
        "windows",
    ];

    // Check for fn key which Tauri doesn't support
    let parts: Vec<String> = raw.split('+').map(|p| p.trim().to_lowercase()).collect();
    for part in &parts {
        if part == "fn" || part == "function" {
            return Err("The 'fn' key is not supported by Tauri global shortcuts".into());
        }
    }

    // Check for at least one non-modifier key
    let has_non_modifier = parts.iter().any(|part| !modifiers.contains(&part.as_str()));

    if !has_non_modifier {
        return Err("Tauri shortcuts must include a main key (letter, number, F-key, etc.) in addition to modifiers".into());
    }

    // The name check above passes side-specific modifiers such as
    // `option_left`, which the handy-keys recorder saves but the accelerator
    // parser rejects. Parse here so a binding carried over from handy-keys is
    // reset to the default instead of failing to register.
    raw.parse::<Shortcut>()
        .map(|_| ())
        .map_err(|e| format!("Failed to parse shortcut '{}': {}", raw, e))
}

/// Register a shortcut using Tauri's global-shortcut plugin
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    // Validate for Tauri requirements
    if let Err(e) = validate_shortcut(&binding.current_binding) {
        warn!(
            "register_tauri_shortcut validation error for binding '{}': {}",
            binding.current_binding, e
        );
        return Err(e);
    }

    // Parse shortcut and return error if it fails
    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!(
                "Failed to parse shortcut '{}': {}",
                binding.current_binding, e
            );
            error!("register_tauri_shortcut parse error: {}", error_msg);
            return Err(error_msg);
        }
    };

    // Prevent duplicate registrations that would silently shadow one another
    if app.global_shortcut().is_registered(shortcut) {
        let error_msg = format!("Shortcut '{}' is already in use", binding.current_binding);
        warn!("register_tauri_shortcut duplicate error: {}", error_msg);
        return Err(error_msg);
    }

    // Clone binding.id for use in the closure
    let binding_id_for_closure = binding.id.clone();

    app.global_shortcut()
        .on_shortcut(shortcut, move |app_handle, scut, event| {
            if scut == &shortcut {
                let shortcut_string = scut.into_string();
                let is_pressed = event.state == ShortcutState::Pressed;
                // Mirrors the handy-keys event log line; the distinct prefix
                // makes it possible to tell which backend fired a shortcut
                // (e.g. when diagnosing the Secure Input fallback)
                debug!(
                    "tauri global-shortcut event: binding={}, shortcut={}, state={:?}",
                    binding_id_for_closure, shortcut_string, event.state
                );
                handle_shortcut_event(
                    app_handle,
                    &binding_id_for_closure,
                    &shortcut_string,
                    is_pressed,
                );
            }
        })
        .map_err(|e| {
            let error_msg = format!(
                "Couldn't register shortcut '{}': {}",
                binding.current_binding, e
            );
            error!("register_tauri_shortcut registration error: {}", error_msg);
            error_msg
        })?;

    Ok(())
}

/// Unregister a shortcut from Tauri's global-shortcut plugin
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let shortcut = match binding.current_binding.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!(
                "Failed to parse shortcut '{}' for unregistration: {}",
                binding.current_binding, e
            );
            error!("unregister_tauri_shortcut parse error: {}", error_msg);
            return Err(error_msg);
        }
    };

    app.global_shortcut().unregister(shortcut).map_err(|e| {
        let error_msg = format!(
            "Failed to unregister shortcut '{}': {}",
            binding.current_binding, e
        );
        error!("unregister_tauri_shortcut error: {}", error_msg);
        error_msg
    })?;

    Ok(())
}

/// Register the cancel shortcut (called when recording starts)
pub fn register_cancel_shortcut(app: &AppHandle) {
    // Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                if let Err(e) = register_shortcut(&app_clone, cancel_binding) {
                    error!("Failed to register cancel shortcut: {}", e);
                }
            }
        });
    }
}

/// Unregister the cancel shortcut (called when recording stops)
pub fn unregister_cancel_shortcut(app: &AppHandle) {
    // Cancel shortcut is disabled on Linux due to instability with dynamic shortcut registration
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(cancel_binding) = get_settings(&app_clone).bindings.get("cancel").cloned() {
                // We ignore errors here as it might already be unregistered
                let _ = unregister_shortcut(&app_clone, cancel_binding);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::validate_shortcut;

    #[test]
    fn rejects_side_specific_modifiers_the_parser_cannot_register() {
        for raw in ["option_left+space", "ctrl_right+space"] {
            assert!(validate_shortcut(raw).is_err(), "{raw} should be rejected");
        }
    }

    #[test]
    fn accepts_the_default_shortcuts() {
        for raw in [
            "option+space",
            "option+shift+space",
            "ctrl+space",
            "ctrl+shift+space",
            "alt+space",
            "escape",
        ] {
            assert_eq!(validate_shortcut(raw), Ok(()), "{raw} should be accepted");
        }
    }
}
