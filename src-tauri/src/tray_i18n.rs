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

#[cfg(test)]
mod tests {
    use super::{get_tray_translations, TRANSLATIONS};

    /// Compare every field rather than a single one, so a partial regression
    /// can't slip through. TrayStrings is generated and derives only Debug.
    fn assert_resolves_to(locale: Option<&str>, expected_key: &str) {
        let expected = TRANSLATIONS
            .get(expected_key)
            .unwrap_or_else(|| panic!("no translations for {expected_key}"));
        assert_eq!(
            format!("{:?}", get_tray_translations(locale.map(str::to_string))),
            format!("{expected:?}"),
            "{locale:?} should resolve to {expected_key}"
        );
    }

    #[test]
    fn simplified_and_traditional_chinese_differ() {
        // Every Chinese assertion below is vacuous if these two are equal.
        assert_ne!(
            format!("{:?}", TRANSLATIONS["zh"]),
            format!("{:?}", TRANSLATIONS["zh-TW"])
        );
    }

    #[test]
    fn traditional_script_locales_resolve_to_zh_tw() {
        for locale in [
            "zh-Hant-TW",
            "zh-Hant-HK",
            "zh-Hant",
            "ZH-HANT-TW",
            "zh_Hant_TW",
        ] {
            assert_resolves_to(Some(locale), "zh-TW");
        }
    }

    #[test]
    fn saved_zh_tw_preference_matches_exactly() {
        assert_resolves_to(Some("zh-TW"), "zh-TW");
    }

    #[test]
    fn simplified_chinese_locales_resolve_to_zh() {
        for locale in ["zh", "zh-CN", "zh-Hans-CN", "zh-SG"] {
            assert_resolves_to(Some(locale), "zh");
        }
    }

    #[test]
    fn region_subtag_falls_back_to_the_language() {
        for (locale, expected) in [
            ("en-US", "en"),
            ("ja-JP", "ja"),
            ("pt-BR", "pt"),
            ("fr-FR", "fr"),
            // Lookup lowercases before the language fallback.
            ("FR-FR", "fr"),
        ] {
            assert_resolves_to(Some(locale), expected);
        }
    }

    #[test]
    fn unsupported_or_missing_locales_fall_back_to_english() {
        assert_resolves_to(Some("xx-YY"), "en");
        assert_resolves_to(Some(""), "en");
        assert_resolves_to(None, "en");
    }
}
