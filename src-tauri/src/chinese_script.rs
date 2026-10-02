//! Simplified / Traditional script handling for Chinese transcription output.
//!
//! Recognition language (Mandarin vs Cantonese) and output script are separate
//! choices: the language picks what the model listens for, and the
//! [`ChineseScript`] setting picks how the resulting text is written. Conversion
//! is keyed off the language the output is known to be in, so mixed-language
//! users on auto-detect only get Chinese output rewritten.

use crate::settings::ChineseScript;
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::error;
use std::sync::OnceLock;

/// The Chinese language a transcription was produced in. Picks the regional
/// conversion tables: Taiwan for Mandarin, Hong Kong for Cantonese.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChineseVariety {
    Mandarin,
    Cantonese,
}

impl ChineseVariety {
    /// Maps an output language code (`zh`, `zh-CN`, `yue`, …) to a Chinese
    /// variety. Any other language returns `None` and is never converted.
    pub fn from_language(language: &str) -> Option<Self> {
        let base = language.split(&['-', '_'][..]).next()?.to_ascii_lowercase();
        match base.as_str() {
            "zh" => Some(Self::Mandarin),
            "yue" => Some(Self::Cantonese),
            _ => None,
        }
    }
}

/// The script a locale writes Chinese in, or `None` for non-Chinese locales.
///
/// `zh` defaults to Simplified unless tagged Hant or regioned to Taiwan, Hong
/// Kong or Macau; Cantonese (`yue`) defaults to Traditional unless tagged Hans.
pub fn chinese_script_for_locale(locale: &str) -> Option<ChineseScript> {
    let normalized = locale.to_lowercase().replace('_', "-");
    let subtags: Vec<_> = normalized.split('-').collect();
    let is_hant = subtags.contains(&"hant");
    let is_hans = subtags.contains(&"hans");
    let is_traditional_region = ["tw", "hk", "mo"]
        .iter()
        .any(|region| subtags.contains(region));

    let traditional = match subtags.first().copied() {
        Some("zh") => is_hant || (!is_hans && is_traditional_region),
        Some("yue") => !is_hans,
        _ => return None,
    };
    Some(if traditional {
        ChineseScript::Traditional
    } else {
        ChineseScript::Simplified
    })
}

/// The OpenCC converter for a variety/script pair, built on first use and
/// reused after. Building one parses its dictionaries, which is too slow to
/// repeat on every live-preview update. A failed build is cached as `None`.
fn converter(variety: ChineseVariety, script: ChineseScript) -> Option<&'static OpenCC> {
    static MANDARIN_SIMPLIFIED: OnceLock<Option<OpenCC>> = OnceLock::new();
    static MANDARIN_TRADITIONAL: OnceLock<Option<OpenCC>> = OnceLock::new();
    static CANTONESE_SIMPLIFIED: OnceLock<Option<OpenCC>> = OnceLock::new();
    static CANTONESE_TRADITIONAL: OnceLock<Option<OpenCC>> = OnceLock::new();

    let (cell, config) = match (variety, script) {
        (_, ChineseScript::AsTranscribed) => return None,
        (ChineseVariety::Mandarin, ChineseScript::Simplified) => {
            (&MANDARIN_SIMPLIFIED, BuiltinConfig::Tw2sp)
        }
        (ChineseVariety::Mandarin, ChineseScript::Traditional) => {
            (&MANDARIN_TRADITIONAL, BuiltinConfig::S2tw)
        }
        (ChineseVariety::Cantonese, ChineseScript::Simplified) => {
            (&CANTONESE_SIMPLIFIED, BuiltinConfig::Hk2s)
        }
        (ChineseVariety::Cantonese, ChineseScript::Traditional) => {
            (&CANTONESE_TRADITIONAL, BuiltinConfig::S2hk)
        }
    };

    cell.get_or_init(|| match OpenCC::from_config(config) {
        Ok(converter) => Some(converter),
        Err(e) => {
            error!(
                "Failed to initialize OpenCC converter: {}. Keeping the original script.",
                e
            );
            None
        }
    })
    .as_ref()
}

/// Rewrites `text` into `script`. Fails open: if OpenCC can't be initialized
/// the text is returned unchanged.
pub fn convert_chinese_script(
    text: &str,
    variety: ChineseVariety,
    script: ChineseScript,
) -> String {
    match converter(variety, script) {
        Some(converter) => converter.convert(text),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variety_from_language_matches_chinese_codes_only() {
        assert_eq!(
            ChineseVariety::from_language("zh"),
            Some(ChineseVariety::Mandarin)
        );
        assert_eq!(
            ChineseVariety::from_language("zh-CN"),
            Some(ChineseVariety::Mandarin)
        );
        assert_eq!(
            ChineseVariety::from_language("yue"),
            Some(ChineseVariety::Cantonese)
        );
        assert_eq!(ChineseVariety::from_language("ja"), None);
        assert_eq!(ChineseVariety::from_language("en"), None);
        assert_eq!(ChineseVariety::from_language(""), None);
    }

    #[test]
    fn locale_maps_to_chinese_script() {
        let cases = [
            ("zh-CN", Some(ChineseScript::Simplified)),
            ("zh_SG", Some(ChineseScript::Simplified)),
            ("zh", Some(ChineseScript::Simplified)),
            ("zh-Hans-HK", Some(ChineseScript::Simplified)),
            ("zh-TW", Some(ChineseScript::Traditional)),
            ("zh-HK", Some(ChineseScript::Traditional)),
            ("zh-MO", Some(ChineseScript::Traditional)),
            ("zh-Hant", Some(ChineseScript::Traditional)),
            ("yue", Some(ChineseScript::Traditional)),
            ("yue-Hans", Some(ChineseScript::Simplified)),
            ("en-US", None),
            ("ja-JP", None),
        ];
        for (locale, expected) in cases {
            assert_eq!(chinese_script_for_locale(locale), expected, "{locale}");
        }
    }

    #[test]
    fn mandarin_uses_taiwan_tables() {
        assert_eq!(
            convert_chinese_script("软件", ChineseVariety::Mandarin, ChineseScript::Traditional),
            "軟件"
        );
        assert_eq!(
            convert_chinese_script("軟體", ChineseVariety::Mandarin, ChineseScript::Simplified),
            "软体"
        );
    }

    #[test]
    fn cantonese_uses_hong_kong_tables() {
        // Hong Kong standard writes 裏 where Taiwan writes 裡.
        assert_eq!(
            convert_chinese_script(
                "里面",
                ChineseVariety::Cantonese,
                ChineseScript::Traditional
            ),
            "裏面"
        );
        assert_eq!(
            convert_chinese_script("裏面", ChineseVariety::Cantonese, ChineseScript::Simplified),
            "里面"
        );
    }

    #[test]
    fn as_transcribed_leaves_text_alone() {
        let text = "软件 軟體";
        assert_eq!(
            convert_chinese_script(text, ChineseVariety::Mandarin, ChineseScript::AsTranscribed),
            text
        );
    }
}
