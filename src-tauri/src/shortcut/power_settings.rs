//! Power-user settings loaded from a JSON file rather than the UI.
//!
//! Settings only ever store one physical binding per action (e.g. one
//! shortcut for "transcribe"). Some devices — a laptop trackpad, for
//! instance — can't produce the binding a user relies on elsewhere (middle
//! mouse click), so there is no way to have both work from the UI alone.
//!
//! This loads an optional `power_settings.json` from the app data directory
//! and, today, registers each entry under `shortcuts` as an additional
//! trigger for an existing action, on top of whatever is already configured
//! in settings. It is intentionally file-only: no UI, not persisted through
//! `AppSettings`, and only read once at startup.
//!
//! The file is a single object so other power-user config can live
//! alongside `shortcuts` as sibling keys later without a format change.
//!
//! Format:
//! ```json
//! {
//!   "shortcuts": [
//!     { "action": "transcribe", "binding": "mousemiddle" }
//!   ]
//! }
//! ```

use log::{debug, info, warn};
use serde::Deserialize;
use tauri::AppHandle;

use crate::actions::ACTION_MAP;
use crate::settings::ShortcutBinding;

const POWER_SETTINGS_FILE: &str = "power_settings.json";

#[derive(Debug, Default, Deserialize)]
struct PowerSettings {
    #[serde(default)]
    shortcuts: Vec<ShortcutEntry>,
}

#[derive(Debug, Deserialize)]
struct ShortcutEntry {
    action: String,
    binding: String,
}

/// Load `power_settings.json` from the app data directory (if present) and
/// register each `shortcuts` entry alongside the normal settings-driven
/// bindings. Missing or malformed input is logged and otherwise ignored —
/// this is a power-user config file, not something that should ever block
/// startup.
pub fn apply_power_settings(app: &AppHandle) {
    let path = match crate::portable::resolve_app_data(app, POWER_SETTINGS_FILE) {
        Ok(path) => path,
        Err(e) => {
            warn!("Failed to resolve power_settings.json path: {}", e);
            return;
        }
    };

    if !path.exists() {
        debug!("No power_settings.json found at {}", path.display());
        return;
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            return;
        }
    };

    let config: PowerSettings = match serde_json::from_str(&contents) {
        Ok(config) => config,
        Err(e) => {
            warn!("Failed to parse {}: {}", path.display(), e);
            return;
        }
    };

    register_extra_shortcuts(app, &path, config.shortcuts);
}

fn register_extra_shortcuts(app: &AppHandle, path: &std::path::Path, entries: Vec<ShortcutEntry>) {
    let mut registered = 0;
    for entry in entries {
        let shortcut = match to_shortcut_binding(&entry) {
            Ok(shortcut) => shortcut,
            Err(e) => {
                warn!("Skipping shortcuts entry in {}: {}", path.display(), e);
                continue;
            }
        };

        match super::register_shortcut(app, shortcut) {
            Ok(()) => registered += 1,
            Err(e) => warn!(
                "Failed to register extra shortcut '{}' -> '{}': {}",
                entry.action, entry.binding, e
            ),
        }
    }

    if registered > 0 {
        info!(
            "Registered {} extra shortcut binding(s) from {}",
            registered,
            path.display()
        );
    }
}

/// Validate a single `shortcuts` entry and turn it into a registerable
/// `ShortcutBinding`, or a human-readable reason it was rejected.
fn to_shortcut_binding(entry: &ShortcutEntry) -> Result<ShortcutBinding, String> {
    let action = entry.action.trim();
    let binding = entry.binding.trim();

    if action.is_empty() || binding.is_empty() {
        return Err(format!("empty action or binding ({entry:?})"));
    }

    if !ACTION_MAP.contains_key(action) {
        let known: Vec<&str> = ACTION_MAP.keys().map(String::as_str).collect();
        return Err(format!(
            "unknown action '{}' (known actions: {})",
            action,
            known.join(", ")
        ));
    }

    Ok(ShortcutBinding {
        id: action.to_string(),
        name: format!("Extra: {action}"),
        description: format!("Extra binding for '{action}' from power_settings.json"),
        default_binding: binding.to_string(),
        current_binding: binding.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let config: PowerSettings = serde_json::from_str(
            r#"{ "shortcuts": [{ "action": "transcribe", "binding": "mousemiddle" }] }"#,
        )
        .unwrap();
        assert_eq!(config.shortcuts.len(), 1);
        assert_eq!(config.shortcuts[0].action, "transcribe");
        assert_eq!(config.shortcuts[0].binding, "mousemiddle");

        let shortcut = to_shortcut_binding(&config.shortcuts[0]).unwrap();
        assert_eq!(shortcut.id, "transcribe");
        assert_eq!(shortcut.current_binding, "mousemiddle");
    }

    #[test]
    fn tolerates_missing_shortcuts_key_and_unknown_sibling_keys() {
        let config: PowerSettings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(config.shortcuts.is_empty());

        // Unknown sibling keys (future power-user config) must not break parsing.
        let config: PowerSettings =
            serde_json::from_str(r#"{ "some_future_feature": { "on": true } }"#).unwrap();
        assert!(config.shortcuts.is_empty());
    }

    #[test]
    fn rejects_unknown_action() {
        let entry = ShortcutEntry {
            action: "not_a_real_action".to_string(),
            binding: "mousemiddle".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());
    }

    #[test]
    fn rejects_empty_fields() {
        let entry = ShortcutEntry {
            action: "  ".to_string(),
            binding: "mousemiddle".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());

        let entry = ShortcutEntry {
            action: "transcribe".to_string(),
            binding: "  ".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());
    }

    #[test]
    fn trims_whitespace() {
        let entry = ShortcutEntry {
            action: " transcribe ".to_string(),
            binding: " mousemiddle ".to_string(),
        };
        let shortcut = to_shortcut_binding(&entry).unwrap();
        assert_eq!(shortcut.id, "transcribe");
        assert_eq!(shortcut.current_binding, "mousemiddle");
    }
}
