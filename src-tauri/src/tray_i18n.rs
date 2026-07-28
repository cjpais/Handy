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

const TRADITIONAL_CHINESE_REGIONS: [&str; 3] = ["tw", "hk", "mo"];
const SIMPLIFIED_CHINESE_REGIONS: [&str; 2] = ["cn", "sg"];

fn find_script_and_region<'a>(subtags: &[&'a str]) -> (Option<&'a str>, Option<&'a str>) {
    let mut script = None;
    let mut region = None;

    for subtag in subtags.iter().skip(1) {
        // A singleton begins a BCP-47 extension, so later values are not part
        // of the language/script/region identifier.
        if subtag.len() == 1 {
            break;
        }
        if script.is_none() && subtag.len() == 4 && subtag.chars().all(|c| c.is_ascii_alphabetic())
        {
            script = Some(*subtag);
        }
        if region.is_none()
            && ((subtag.len() == 2 && subtag.chars().all(|c| c.is_ascii_alphabetic()))
                || (subtag.len() == 3 && subtag.chars().all(|c| c.is_ascii_digit())))
        {
            region = Some(*subtag);
        }
    }

    (script, region)
}

/// Get localized tray menu strings based on the system locale.
///
/// Lookup order: exact locale → Chinese script/region fallback → language code → English.
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let normalized = locale
        .as_deref()
        .unwrap_or("en")
        .trim()
        .to_lowercase()
        .replace('_', "-");
    let subtags: Vec<_> = normalized.split('-').collect();
    let lang_code = subtags.first().copied().unwrap_or("en");

    // Locale tags are case-insensitive, while the generated map preserves the
    // locale directory's casing (for example, "zh-TW").
    let exact_match = TRANSLATIONS
        .iter()
        .find_map(|(code, strings)| code.eq_ignore_ascii_case(&normalized).then_some(strings));

    let (script, region) = find_script_and_region(&subtags);
    let chinese_fallback = match lang_code {
        "zh" if script == Some("hant")
            || (script != Some("hans")
                && region.is_some_and(|r| TRADITIONAL_CHINESE_REGIONS.contains(&r))) =>
        {
            TRANSLATIONS.get("zh-TW")
        }
        // We do not have a Cantonese translation. Use the matching Chinese
        // script, with Traditional as Cantonese's default writing system.
        "yue" => {
            if script == Some("hans")
                || (script != Some("hant")
                    && region.is_some_and(|r| SIMPLIFIED_CHINESE_REGIONS.contains(&r)))
            {
                TRANSLATIONS.get("zh")
            } else {
                TRANSLATIONS.get("zh-TW")
            }
        }
        _ => None,
    };

    exact_match
        .or(chinese_fallback)
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
    fn traditional_chinese_locales_resolve_to_zh_tw() {
        for locale in [
            "zh-Hant-TW",
            "zh-Hant-HK",
            "zh-Hant",
            "zh-yue-Hant-HK",
            "zh-HK",
            "zh-MO",
            "ZH-HANT-TW",
            "zh_Hant_TW",
        ] {
            assert_resolves_to(Some(locale), "zh-TW");
        }
    }

    #[test]
    fn exact_locale_matching_ignores_case_and_separator_style() {
        for locale in ["zh-TW", "ZH-TW", "zh-tw", "zh_TW"] {
            assert_resolves_to(Some(locale), "zh-TW");
        }
    }

    #[test]
    fn simplified_chinese_locales_resolve_to_zh() {
        for locale in ["zh", "zh-CN", "zh-Hans-CN", "zh-Hans-HK", "zh-SG"] {
            assert_resolves_to(Some(locale), "zh");
        }
    }

    #[test]
    fn cantonese_uses_the_matching_chinese_script_as_a_fallback() {
        for locale in ["yue", "yue-Hant", "yue-Hant-HK", "yue-HK", "yue-MO"] {
            assert_resolves_to(Some(locale), "zh-TW");
        }
        for locale in ["yue-Hans", "yue-Hans-CN", "yue-CN", "yue-SG"] {
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
