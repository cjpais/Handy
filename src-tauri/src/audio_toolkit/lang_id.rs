//! Confidence-gated text-based language identification.
//!
//! Last-resort evidence for filler-word removal when neither the user's
//! language selection nor the transcription model identifies the output
//! language. Detection is constrained to the languages the active model can
//! produce and fails closed: any doubt returns `None`, which callers treat as
//! an unknown output language.

use whatlang::{Detector, Lang};

/// Minimum whatlang confidence (0.0–1.0) to accept a detection, on top of
/// whatlang's own `is_reliable()` heuristic. A wrong accepted language can
/// reintroduce real-word deletion (e.g. Portuguese "um"), so the gate is
/// deliberately strict: calibrated on ~8k short Tatoeba sentences across the
/// 16 filler-profile languages, `is_reliable() && confidence >= 0.9` fires on
/// ~66% of sentences with 99.9% accuracy (script-distinct languages ~100%,
/// Latin-script languages 22–64%). Missed detections merely skip gated filler
/// removal; the universal tier still applies.
const MIN_CONFIDENCE: f64 = 0.9;

/// Whatlang's Mandarin code is `cmn`, which has no ISO 639-1 form; model
/// metadata uses `zh`.
fn whatlang_lang_for_iso639_1(code: &str) -> Option<Lang> {
    let three = match code {
        "zh" => "cmn",
        other => isolang::Language::from_639_1(other)?.to_639_3(),
    };
    Lang::from_code(three)
}

fn iso639_1_for_whatlang(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Cmn => Some("zh"),
        other => isolang::Language::from_639_3(other.code())?.to_639_1(),
    }
}

/// Detects the language of transcribed text, constrained to the languages the
/// model can output. Returns an ISO 639-1 code only for a reliable,
/// high-confidence detection; `None` otherwise.
pub fn detect_output_language(text: &str, supported_languages: &[String]) -> Option<String> {
    let allowlist: Vec<Lang> = supported_languages
        .iter()
        .filter_map(|code| whatlang_lang_for_iso639_1(code))
        .collect();

    // No usable metadata means no constraint, not no detection.
    let detector = if allowlist.is_empty() {
        Detector::new()
    } else {
        Detector::with_allowlist(allowlist)
    };

    let info = detector.detect(text)?;
    if !info.is_reliable() || info.confidence() < MIN_CONFIDENCE {
        return None;
    }

    iso639_1_for_whatlang(info.lang()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn detects_portuguese_sentence_containing_um() {
        let detected = detect_output_language(
            "eu vi um carro na rua ontem de manhã quando fui ao mercado",
            &langs(&["en", "pt", "es"]),
        );
        assert_eq!(detected.as_deref(), Some("pt"));
    }

    #[test]
    fn short_ambiguous_text_returns_none() {
        let detected = detect_output_language("um ok", &langs(&["en", "pt"]));
        assert_eq!(detected, None);
    }

    #[test]
    fn chinese_maps_between_zh_and_cmn() {
        assert_eq!(whatlang_lang_for_iso639_1("zh"), Some(Lang::Cmn));
        assert_eq!(iso639_1_for_whatlang(Lang::Cmn), Some("zh"));
    }
}
