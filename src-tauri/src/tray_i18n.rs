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

/// Get localized tray menu strings based on the system locale.
///
/// Lookup order: full locale (e.g. "zh-TW") → script-aware fallback → language code ("zh") → English.
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let locale_str = locale.as_deref().unwrap_or("en");
    let normalized = locale_str.to_lowercase();
    let mut subtags = normalized.split(['-', '_']);
    let lang_code = subtags.next().unwrap_or("en");
    // Script-aware fallback: Traditional-script Chinese locales
    // (e.g. zh-Hant-TW, zh-Hant-HK) should resolve to zh-TW, not fall
    // through to the "zh" (Simplified) language fallback below.
    let script_fallback = if lang_code == "zh" && subtags.next() == Some("hant") {
        TRANSLATIONS.get("zh-TW")
    } else {
        None
    };

    TRANSLATIONS
        .get(locale_str)
        .or(script_fallback)
        .or_else(|| TRANSLATIONS.get(lang_code))
        .or_else(|| TRANSLATIONS.get("en"))
        .cloned()
        .expect("English translations must exist")
}
