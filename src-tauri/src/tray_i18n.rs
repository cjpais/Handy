//! Tray menu internationalization
//!
//! Loads translations from the shared frontend JSON files at compile time.
//!
//! NOTE: When adding a new language to the frontend (src/i18n/locales/),
//! remember to also add it to the load_translations! macro below.

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

/// Localized strings for the tray menu
#[derive(Debug, Clone, Deserialize)]
pub struct TrayStrings {
    pub settings: String,
    #[serde(rename = "checkUpdates")]
    pub check_updates: String,
    pub quit: String,
    pub cancel: String,
}

/// Wrapper for deserializing the translation file
#[derive(Deserialize)]
struct TranslationFile {
    tray: TrayStrings,
}

/// Macro to load and parse tray translations at compile time
macro_rules! load_translations {
    ($($code:literal => $path:literal),* $(,)?) => {{
        let mut map = HashMap::new();
        $(
            if let Ok(file) = serde_json::from_str::<TranslationFile>(include_str!($path)) {
                map.insert($code, file.tray);
            }
        )*
        map
    }};
}

// Embed translation JSON files at compile time
static TRANSLATIONS: Lazy<HashMap<&'static str, TrayStrings>> = Lazy::new(|| {
    load_translations! {
        "en" => "../../src/i18n/locales/en/translation.json",
        "es" => "../../src/i18n/locales/es/translation.json",
        "fr" => "../../src/i18n/locales/fr/translation.json",
        "vi" => "../../src/i18n/locales/vi/translation.json",
        "de" => "../../src/i18n/locales/de/translation.json",
        "ja" => "../../src/i18n/locales/ja/translation.json",
        "zh" => "../../src/i18n/locales/zh/translation.json",
    }
});

/// Get the language code from a locale string (e.g., "en-US" -> "en")
fn get_language_code(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or("en")
}

/// Get localized tray menu strings based on the system locale
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let lang = locale.as_deref().map(get_language_code).unwrap_or("en");

    // Try requested language, fall back to English
    TRANSLATIONS
        .get(lang)
        .or_else(|| TRANSLATIONS.get("en"))
        .cloned()
        .expect("English translations must exist")
}

/// Get the current system locale
pub fn get_system_locale() -> Option<String> {
    tauri_plugin_os::locale()
}
