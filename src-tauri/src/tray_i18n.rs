//! Tray menu internationalization
//!
//! Everything is auto-generated at compile time by build.rs from the
//! frontend locale files (src/i18n/locales/*/translation.json).
//!
//! The English translation.json is the single source of truth:
//! - TrayStrings struct fields are derived from the English "tray" keys
//! - All languages are auto-discovered from the locales directory
//!
//! To add a new tray menu item:
//! 1. Add the key to en/translation.json under "tray"
//! 2. Add translations to other locale files
//! 3. Update tray.rs to use the new field (e.g., strings.new_field)

use once_cell::sync::Lazy;
use std::collections::HashMap;

// Include the auto-generated TrayStrings struct and TRANSLATIONS static
include!(concat!(env!("OUT_DIR"), "/tray_translations.rs"));

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
