//! Tray menu internationalization
//!
//! Loads translations from the shared frontend JSON files at compile time.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

/// Localized strings for the tray menu
#[derive(Debug, Clone)]
pub struct TrayStrings {
    pub settings: String,
    pub check_updates: String,
    pub quit: String,
    pub cancel: String,
}

impl Default for TrayStrings {
    fn default() -> Self {
        Self {
            settings: "Settings...".to_string(),
            check_updates: "Check for Updates...".to_string(),
            quit: "Quit".to_string(),
            cancel: "Cancel".to_string(),
        }
    }
}

// Embed translation JSON files at compile time
static TRANSLATIONS: Lazy<HashMap<&'static str, Value>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // English
    if let Ok(json) =
        serde_json::from_str(include_str!("../../src/i18n/locales/en/translation.json"))
    {
        map.insert("en", json);
    }

    // Spanish
    if let Ok(json) =
        serde_json::from_str(include_str!("../../src/i18n/locales/es/translation.json"))
    {
        map.insert("es", json);
    }

    // French
    if let Ok(json) =
        serde_json::from_str(include_str!("../../src/i18n/locales/fr/translation.json"))
    {
        map.insert("fr", json);
    }

    // Vietnamese
    if let Ok(json) =
        serde_json::from_str(include_str!("../../src/i18n/locales/vi/translation.json"))
    {
        map.insert("vi", json);
    }

    map
});

/// Get the language code from a locale string (e.g., "en-US" -> "en")
fn get_language_code(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or("en")
}

/// Extract tray strings from a JSON value
fn extract_tray_strings(json: &Value) -> Option<TrayStrings> {
    let tray = json.get("tray")?;

    Some(TrayStrings {
        settings: tray.get("settings")?.as_str()?.to_string(),
        check_updates: tray.get("checkUpdates")?.as_str()?.to_string(),
        quit: tray.get("quit")?.as_str()?.to_string(),
        cancel: tray.get("cancel")?.as_str()?.to_string(),
    })
}

/// Get localized tray menu strings based on the system locale
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let lang = locale.as_deref().map(get_language_code).unwrap_or("en");

    // Try to get translations for the requested language
    if let Some(json) = TRANSLATIONS.get(lang) {
        if let Some(strings) = extract_tray_strings(json) {
            return strings;
        }
    }

    // Fall back to English
    if let Some(json) = TRANSLATIONS.get("en") {
        if let Some(strings) = extract_tray_strings(json) {
            return strings;
        }
    }

    // Ultimate fallback to hardcoded defaults
    TrayStrings::default()
}

/// Get the current system locale
pub fn get_system_locale() -> Option<String> {
    sys_locale::get_locale()
}
