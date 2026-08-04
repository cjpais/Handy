//! Power-user extra shortcut bindings.
//!
//! Settings only ever store one physical binding per action (e.g. one
//! shortcut for "transcribe"). Some devices — a laptop trackpad, for
//! instance — can't produce the binding a user relies on elsewhere (middle
//! mouse click), so there is no way to have both work from the UI alone.
//!
//! This loads an optional `extra_bindings.json` from the app data directory
//! and registers each entry as an additional trigger for an existing
//! action, on top of whatever is already configured in settings. It is
//! intentionally file-only: no UI, not persisted through `AppSettings`, and
//! only read once at startup.
//!
//! Format:
//! ```json
//! [
//!   { "action": "transcribe", "binding": "mousemiddle" }
//! ]
//! ```

use log::{debug, info, warn};
use serde::Deserialize;
use tauri::AppHandle;

use crate::actions::ACTION_MAP;
use crate::settings::ShortcutBinding;

const EXTRA_BINDINGS_FILE: &str = "extra_bindings.json";

#[derive(Debug, Deserialize)]
struct ExtraBindingEntry {
    action: String,
    binding: String,
}

/// Load `extra_bindings.json` from the app data directory (if present) and
/// register each entry alongside the normal settings-driven bindings.
/// Missing or malformed input is logged and otherwise ignored — this is a
/// power-user config file, not something that should ever block startup.
pub fn register_extra_bindings(app: &AppHandle) {
    let path = match crate::portable::resolve_app_data(app, EXTRA_BINDINGS_FILE) {
        Ok(path) => path,
        Err(e) => {
            warn!("Failed to resolve extra_bindings.json path: {}", e);
            return;
        }
    };

    if !path.exists() {
        debug!("No extra_bindings.json found at {}", path.display());
        return;
    }

    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            return;
        }
    };

    let entries: Vec<ExtraBindingEntry> = match serde_json::from_str(&contents) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to parse {}: {}", path.display(), e);
            return;
        }
    };

    let mut registered = 0;
    for entry in entries {
        let shortcut = match to_shortcut_binding(&entry) {
            Ok(shortcut) => shortcut,
            Err(e) => {
                warn!("Skipping entry in {}: {}", path.display(), e);
                continue;
            }
        };

        match super::register_shortcut(app, shortcut) {
            Ok(()) => registered += 1,
            Err(e) => warn!(
                "Failed to register extra binding '{}' -> '{}': {}",
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

/// Validate a single JSON entry and turn it into a registerable
/// `ShortcutBinding`, or a human-readable reason it was rejected.
fn to_shortcut_binding(entry: &ExtraBindingEntry) -> Result<ShortcutBinding, String> {
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
        description: format!("Extra binding for '{action}' from extra_bindings.json"),
        default_binding: binding.to_string(),
        current_binding: binding.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let entries: Vec<ExtraBindingEntry> =
            serde_json::from_str(r#"[{ "action": "transcribe", "binding": "mousemiddle" }]"#)
                .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].action, "transcribe");
        assert_eq!(entries[0].binding, "mousemiddle");

        let shortcut = to_shortcut_binding(&entries[0]).unwrap();
        assert_eq!(shortcut.id, "transcribe");
        assert_eq!(shortcut.current_binding, "mousemiddle");
    }

    #[test]
    fn rejects_unknown_action() {
        let entry = ExtraBindingEntry {
            action: "not_a_real_action".to_string(),
            binding: "mousemiddle".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());
    }

    #[test]
    fn rejects_empty_fields() {
        let entry = ExtraBindingEntry {
            action: "  ".to_string(),
            binding: "mousemiddle".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());

        let entry = ExtraBindingEntry {
            action: "transcribe".to_string(),
            binding: "  ".to_string(),
        };
        assert!(to_shortcut_binding(&entry).is_err());
    }

    #[test]
    fn trims_whitespace() {
        let entry = ExtraBindingEntry {
            action: " transcribe ".to_string(),
            binding: " mousemiddle ".to_string(),
        };
        let shortcut = to_shortcut_binding(&entry).unwrap();
        assert_eq!(shortcut.id, "transcribe");
        assert_eq!(shortcut.current_binding, "mousemiddle");
    }
}
