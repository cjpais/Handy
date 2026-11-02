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

/// Spoken Chinese variety; selects the Simplified tables, which differ for Cantonese.
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
/// `zh` is Simplified unless tagged Hant or regioned to Taiwan, Hong Kong or
/// Macau; Cantonese (`yue`) is Traditional unless tagged Hans. Traditional
/// takes Hong Kong's forms for Hong Kong, Macau and Cantonese, else Taiwan's.
pub fn chinese_script_for_locale(locale: &str) -> Option<ChineseScript> {
    let normalized = locale.to_lowercase().replace('_', "-");
    let subtags: Vec<_> = normalized.split('-').collect();
    let has = |subtag: &str| subtags.contains(&subtag);
    let hong_kong = has("hk") || has("mo");

    let script = match subtags.first().copied() {
        Some("zh") if has("hans") => ChineseScript::Simplified,
        Some("zh") if hong_kong => ChineseScript::TraditionalHk,
        Some("zh") if has("hant") || has("tw") => ChineseScript::TraditionalTw,
        Some("zh") => ChineseScript::Simplified,
        Some("yue") if has("hans") => ChineseScript::Simplified,
        Some("yue") if has("tw") => ChineseScript::TraditionalTw,
        Some("yue") => ChineseScript::TraditionalHk,
        _ => return None,
    };
    Some(script)
}

/// The OpenCC conversion for a variety and script. Traditional converts directly to
/// the region's forms; going via Simplified would mangle correct text (颱風後 → 台風后).
fn conversion(variety: ChineseVariety, script: ChineseScript) -> Option<BuiltinConfig> {
    match (variety, script) {
        (_, ChineseScript::AsTranscribed) => None,
        (ChineseVariety::Mandarin, ChineseScript::Simplified) => Some(BuiltinConfig::Tw2sp),
        (ChineseVariety::Cantonese, ChineseScript::Simplified) => Some(BuiltinConfig::Hk2s),
        (_, ChineseScript::TraditionalTw) => Some(BuiltinConfig::S2tw),
        (_, ChineseScript::TraditionalHk) => Some(BuiltinConfig::S2hk),
    }
}

/// The OpenCC converter for a variety/script pair, built on first use and
/// reused after. Building one parses its dictionaries, which is too slow to
/// repeat on every live-preview update. A failed build is cached as `None`.
fn converter(variety: ChineseVariety, script: ChineseScript) -> Option<&'static OpenCC> {
    static TW2SP: OnceLock<Option<OpenCC>> = OnceLock::new();
    static HK2S: OnceLock<Option<OpenCC>> = OnceLock::new();
    static S2TW: OnceLock<Option<OpenCC>> = OnceLock::new();
    static S2HK: OnceLock<Option<OpenCC>> = OnceLock::new();

    let config = conversion(variety, script)?;
    let cell = match config {
        BuiltinConfig::Tw2sp => &TW2SP,
        BuiltinConfig::Hk2s => &HK2S,
        BuiltinConfig::S2tw => &S2TW,
        _ => &S2HK,
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
    fn cantonese_uses_hong_kong_tables() {
        // Hong Kong standard writes 裏 where Taiwan writes 裡.
        assert_eq!(
            convert_chinese_script(
                "里面",
                ChineseVariety::Cantonese,
                ChineseScript::TraditionalHk
            ),
            "裏面"
        );
        assert_eq!(
            convert_chinese_script("裏面", ChineseVariety::Cantonese, ChineseScript::Simplified),
            "里面"
        );
    }
}
