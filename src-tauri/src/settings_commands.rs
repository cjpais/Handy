//! Changing a setting from the frontend: one command for every plain setting.
//!
//! [`set_setting`] puts the new value in, checks it and saves it. Then
//! [`apply_change`] does whatever else the change needs right away, comparing
//! the old settings with the new: re-registering a shortcut, hiding the tray,
//! cleaning up history. That's the one place to look for what a setting does
//! when it changes.
//!
//! A setting with its own command isn't here: one that has to be applied
//! before it's saved and can fail (a microphone or VAD switch), or one with a
//! richer shape (bindings, models, prompts, providers).

use crate::settings::{self, AppSettings, LogLevel, OverlayStyle, LANGUAGE_DETECTION_WINDOW_SECS};
use log::warn;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Settings with a command of their own; `set_setting` refuses them.
const OWN_COMMAND: &[&str] = &[
    // Applied first, and can be refused (e.g. mid-recording).
    "always_on_microphone",
    "selected_microphone",
    "selected_channel",
    "airpods_mode",
    "vad_backend",
    // Validated or re-registered as they change.
    "bindings",
    "keyboard_implementation",
    "selected_model",
    "post_process_providers",
    "post_process_provider_id",
    "post_process_api_keys",
    "post_process_models",
    "post_process_prompts",
    "post_process_selected_prompt_id",
    // Not the user's to set.
    "onboarding_completed",
    "settings_schema_version",
];

/// Sets one setting, `key` being its name in `AppSettings`. The value must have
/// the same shape the settings file stores.
#[tauri::command]
#[specta::specta]
pub fn set_setting(app: AppHandle, key: String, value: serde_json::Value) -> Result<(), String> {
    if OWN_COMMAND.contains(&key.as_str()) {
        return Err(format!("'{key}' has its own command"));
    }
    if key == "update_checks_enabled" && settings::update_checks_forced_disabled() {
        return Err(
            "Update checks are disabled by system configuration (HANDY_DISABLE_UPDATER)".into(),
        );
    }

    let (old, new) = settings::try_update_settings(&app, |settings| {
        let old = settings.clone();
        *settings = with_value(settings, &key, value)?;
        Ok::<_, String>((old, settings.clone()))
    })?;
    apply_change(&app, &old, &new);
    Ok(())
}

/// The settings with `key` set to `value`, cleaned up where a value has bounds.
fn with_value(
    settings: &AppSettings,
    key: &str,
    value: serde_json::Value,
) -> Result<AppSettings, String> {
    let mut json = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let fields = json
        .as_object_mut()
        .ok_or("settings aren't a JSON object")?;
    if !fields.contains_key(key) {
        return Err(format!("Unknown setting '{key}'"));
    }
    fields.insert(key.to_string(), value);

    let mut updated: AppSettings =
        serde_json::from_value(json).map_err(|e| format!("Invalid value for '{key}': {e}"))?;

    let (min, max) = LANGUAGE_DETECTION_WINDOW_SECS.into_inner();
    updated.language_detection_window_secs = updated.language_detection_window_secs.clamp(min, max);
    updated.whats_new_last_seen_version = updated.whats_new_last_seen_version.trim().to_string();
    Ok(updated)
}

/// Applies what a change means for the running app. Settings read when
/// they're used (most of them) need nothing here.
pub(crate) fn apply_change(app: &AppHandle, old: &AppSettings, new: &AppSettings) {
    if old.theme != new.theme {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        crate::shortcut::apply_window_theme(app, new.theme);
        // The overlay sets the palette on its own document.
        let _ = app.emit("theme-changed", new.theme);
    }

    if old.overlay_style != new.overlay_style {
        crate::overlay::update_overlay_enabled_cache(new.overlay_style != OverlayStyle::None);
    }
    if old.overlay_style != new.overlay_style || old.overlay_position != new.overlay_position {
        crate::utils::update_overlay_position(app);
    }

    if old.debug_mode != new.debug_mode {
        // The live log viewer, its only consumer, exists only in debug mode.
        crate::WEBVIEW_LOG_STREAMING.store(new.debug_mode, std::sync::atomic::Ordering::Relaxed);
    }

    if old.log_level != new.log_level {
        set_file_log_level(new.log_level);
    }

    if old.autostart_enabled != new.autostart_enabled {
        crate::autostart::apply_autostart(app, new.autostart_enabled);
    }

    if old.show_tray_icon != new.show_tray_icon {
        crate::tray::set_tray_visibility(app, new.show_tray_icon);
    }

    if old.selected_language != new.selected_language
        || old.spoken_languages != new.spoken_languages
        || old.language_detection != new.language_detection
        || old.app_language != new.app_language
    {
        crate::tray::update_tray_menu(app);
    }

    if old.post_process_enabled != new.post_process_enabled {
        if let Some(binding) = new.bindings.get("transcribe_with_post_process").cloned() {
            let result = if new.post_process_enabled {
                crate::shortcut::register_shortcut(app, binding)
            } else {
                crate::shortcut::unregister_shortcut(app, binding)
            };
            if let Err(e) = result {
                warn!("Failed to update the post-process shortcut: {e}");
            }
        }
        crate::secure_input::reconcile_fallback(app);
    }

    if old.transcribe_accelerator != new.transcribe_accelerator
        || old.transcribe_gpu_device != new.transcribe_gpu_device
    {
        // The model in use keeps its engine; the next use reloads it, and the
        // GPU gets another try.
        let tm = app.state::<Arc<crate::managers::transcription::TranscriptionManager>>();
        tm.reload_model_on_next_use();
        tm.retry_transcribe_gpu("the transcribe.cpp accelerator settings changed");
    }

    if old.history_limit != new.history_limit
        || old.recording_retention_period != new.recording_retention_period
    {
        // Deletes files, so off the main thread.
        let history = app
            .state::<Arc<crate::managers::history::HistoryManager>>()
            .inner()
            .clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = history.cleanup_old_entries() {
                warn!("Failed to clean up history after a retention change: {e}");
            }
        });
    }
}

fn set_file_log_level(level: LogLevel) {
    let level: tauri_plugin_log::LogLevel = level.into();
    let level: log::Level = level.into();
    crate::FILE_LOG_LEVEL.store(
        level.to_level_filter() as u8,
        std::sync::atomic::Ordering::Relaxed,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{get_default_settings, PasteMethod, Theme};

    #[test]
    fn sets_a_value_by_its_stored_name() {
        let settings = get_default_settings();

        let updated = with_value(&settings, "paste_method", serde_json::json!("direct")).unwrap();
        assert_eq!(updated.paste_method, PasteMethod::Direct);

        let updated = with_value(&settings, "theme", serde_json::json!("dark")).unwrap();
        assert_eq!(updated.theme, Theme::Dark);

        let updated = with_value(&settings, "custom_words", serde_json::json!(["Handy"])).unwrap();
        assert_eq!(updated.custom_words, vec!["Handy".to_string()]);

        let updated =
            with_value(&settings, "external_script_path", serde_json::Value::Null).unwrap();
        assert_eq!(updated.external_script_path, None);
    }

    #[test]
    fn refuses_unknown_settings_and_wrong_values() {
        let settings = get_default_settings();

        assert!(with_value(&settings, "no_such_setting", serde_json::json!(true)).is_err());
        assert!(with_value(&settings, "paste_method", serde_json::json!("telepathy")).is_err());
        assert!(with_value(&settings, "history_limit", serde_json::json!("lots")).is_err());
    }

    #[test]
    fn keeps_bounded_values_in_bounds() {
        let settings = get_default_settings();
        let (min, max) = LANGUAGE_DETECTION_WINDOW_SECS.into_inner();

        let updated = with_value(
            &settings,
            "language_detection_window_secs",
            serde_json::json!(999),
        )
        .unwrap();
        assert_eq!(updated.language_detection_window_secs, max);

        let updated = with_value(
            &settings,
            "language_detection_window_secs",
            serde_json::json!(0),
        )
        .unwrap();
        assert_eq!(updated.language_detection_window_secs, min);

        let updated = with_value(
            &settings,
            "whats_new_last_seen_version",
            serde_json::json!(" 1.0.0 \n"),
        )
        .unwrap();
        assert_eq!(updated.whats_new_last_seen_version, "1.0.0");
    }

    #[test]
    fn every_command_only_setting_exists() {
        let json = serde_json::to_value(get_default_settings()).unwrap();
        for key in OWN_COMMAND {
            assert!(json.get(key).is_some(), "'{key}' isn't a setting");
        }
    }
}
