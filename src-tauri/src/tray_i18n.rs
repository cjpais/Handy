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
/// Lookup order: exact locale → Chinese script/region fallback → language code → English.
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let normalized = locale
        .as_deref()
        .unwrap_or("en")
        .to_lowercase()
        .replace('_', "-");
    let subtags: Vec<_> = normalized.split('-').collect();
    let language = subtags.first().copied().unwrap_or("en");
    let is_hant = subtags.contains(&"hant");
    let is_hans = subtags.contains(&"hans");
    let is_traditional_region = ["tw", "hk", "mo"]
        .iter()
        .any(|region| subtags.contains(region));

    let exact_match = TRANSLATIONS
        .iter()
        .find_map(|(code, strings)| code.eq_ignore_ascii_case(&normalized).then_some(strings));
    let fallback = match language {
        "zh" if is_hant || (!is_hans && is_traditional_region) => "zh-TW",
        // Cantonese uses Traditional Chinese unless explicitly tagged as Hans.
        "yue" if is_hans => "zh",
        "yue" => "zh-TW",
        _ => language,
    };

    exact_match
        .or_else(|| TRANSLATIONS.get(fallback))
        .or_else(|| TRANSLATIONS.get("en"))
        .cloned()
        .expect("English translations must exist")
}

#[cfg(test)]
mod tests {
    use super::{get_tray_translations, TRANSLATIONS};

    #[test]
    fn resolves_locale_fallbacks() {
        for (locale, expected) in [
            ("zh-Hant-TW", "zh-TW"),
            ("zh-Hant-HK", "zh-TW"),
            ("zh-HK", "zh-TW"),
            ("zh-MO", "zh-TW"),
            ("ZH-TW", "zh-TW"),
            ("zh_Hant_TW", "zh-TW"),
            ("zh-Hans-CN", "zh"),
            ("yue-Hant-HK", "zh-TW"),
            ("yue-Hans-CN", "zh"),
            ("fr-FR", "fr"),
            ("xx-YY", "en"),
        ] {
            assert_eq!(
                format!("{:?}", get_tray_translations(Some(locale.into()))),
                format!("{:?}", TRANSLATIONS[expected]),
                "{locale} should resolve to {expected}"
            );
        }
    }
}
