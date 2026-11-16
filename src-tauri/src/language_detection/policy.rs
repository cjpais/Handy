//! Which language a transcription runs in. Ported from `autoLanguagePolicy`
//! in `@handy-computer/models`, plus Handy's single-language and Always rules.

use serde::Serialize;
use specta::Type;

use crate::managers::model::{canonical_language_code, effective_language, supported_code};
use crate::settings::LanguageDetectionMode;

use super::LanguageSource;

/// What a model does with languages, from its catalog or probed capabilities.
pub struct ModelLanguages<'a> {
    /// The model's own codes; empty when language-agnostic or unknown.
    pub languages: &'a [String],
    /// The model detects the language itself.
    pub detects: bool,
    pub streaming: bool,
}

/// How a transcription picks its language, decided before any audio.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// One language, no detection.
    Fixed {
        language: String,
        source: LanguageSource,
    },
    /// The model's own detection.
    Native,
    /// Handy's detector chooses between `languages` (the model's codes, in
    /// preference order); `allowed` holds the detector's label for each.
    Detect {
        languages: Vec<String>,
        allowed: Vec<String>,
        fallback: DetectFallback,
    },
}

/// What a detect plan uses when detection can't decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectFallback {
    /// The first preferred language.
    FirstLanguage,
    /// The model's own detection.
    Native,
}

/// How Auto works for a model and the languages the person speaks.
#[derive(Debug, Clone, PartialEq)]
pub enum AutoLanguage {
    /// The model detects the language itself.
    Native,
    /// The model has one language, or one of theirs fits it.
    Single(String),
    /// The detector chooses between these.
    Detect {
        languages: Vec<String>,
        allowed: Vec<String>,
    },
    /// The detector can't identify `undetectable`, which the model supports.
    /// `compatible` is every spoken language the model supports.
    Undetectable {
        compatible: Vec<String>,
        undetectable: Vec<String>,
    },
    /// None of the spoken languages fit the model.
    NoMatch,
}

/// The detector's label for `language` (`he` → `iw`), if it has one.
pub fn detector_label(labels: &[String], language: &str) -> Option<String> {
    let identity = canonical_language_code(&language.to_ascii_lowercase()).to_string();
    labels
        .iter()
        .find(|label| canonical_language_code(&label.to_ascii_lowercase()) == identity)
        .cloned()
}

/// Spoken languages this model supports, as its own codes, in preference
/// order, one per language.
fn compatible_preferences(languages: &[String], preferred: &[String]) -> Vec<String> {
    let mut compatible: Vec<String> = Vec::new();
    for preference in preferred {
        if let Some(code) = supported_code(preference, languages) {
            let identity = canonical_language_code(code);
            if !compatible
                .iter()
                .any(|c| canonical_language_code(c) == identity)
            {
                compatible.push(code.clone());
            }
        }
    }
    compatible
}

/// The model has more than one language.
fn is_multilingual(languages: &[String]) -> bool {
    languages
        .iter()
        .skip(1)
        .any(|l| canonical_language_code(l) != canonical_language_code(&languages[0]))
}

/// Whether Auto on this model needs Handy's detector, whatever the person
/// speaks (`requiresLanguageDetector`).
pub fn requires_language_detector(model: &ModelLanguages) -> bool {
    !model.detects && is_multilingual(model.languages)
}

pub fn auto_language(
    model: &ModelLanguages,
    preferred: &[String],
    labels: &[String],
) -> AutoLanguage {
    if model.detects {
        return AutoLanguage::Native;
    }
    if !is_multilingual(model.languages) {
        return AutoLanguage::Single(effective_language(
            "auto",
            preferred,
            model.languages,
            false,
        ));
    }
    let compatible = compatible_preferences(model.languages, preferred);
    let allowed: Vec<Option<String>> = compatible
        .iter()
        .map(|language| detector_label(labels, language))
        .collect();
    match compatible.len() {
        0 => AutoLanguage::NoMatch,
        1 => AutoLanguage::Single(compatible[0].clone()),
        _ if allowed.iter().any(Option::is_none) => AutoLanguage::Undetectable {
            undetectable: compatible
                .iter()
                .zip(&allowed)
                .filter(|(_, label)| label.is_none())
                .map(|(language, _)| language.clone())
                .collect(),
            compatible,
        },
        _ => AutoLanguage::Detect {
            languages: compatible,
            allowed: allowed.into_iter().flatten().collect(),
        },
    }
}

/// The plan for one transcription. `intent` is the persisted selection
/// (`"auto"` or a code); an explicit language the model supports always wins.
pub fn plan(
    intent: &str,
    preferred: &[String],
    model: &ModelLanguages,
    mode: LanguageDetectionMode,
    labels: &[String],
) -> Plan {
    if model.languages.is_empty() {
        // Language-agnostic or unknown: the engine decides, and a concrete
        // intent is passed on to be gated against the loaded model.
        return match intent {
            "auto" => Plan::Native,
            language => Plan::Fixed {
                language: language.to_string(),
                source: LanguageSource::Selected,
            },
        };
    }
    if intent != "auto" {
        if let Some(code) = supported_code(intent, model.languages) {
            return Plan::Fixed {
                language: code.clone(),
                source: LanguageSource::Selected,
            };
        }
    }

    match auto_language(model, preferred, labels) {
        AutoLanguage::Native => {
            if mode == LanguageDetectionMode::Always && !model.streaming {
                let unrestricted = ModelLanguages {
                    detects: false,
                    ..*model
                };
                match auto_language(&unrestricted, preferred, labels) {
                    AutoLanguage::Detect { languages, allowed } => {
                        return Plan::Detect {
                            languages,
                            allowed,
                            fallback: DetectFallback::Native,
                        };
                    }
                    // Limited to one language, Auto is that language.
                    AutoLanguage::Single(language) => {
                        return Plan::Fixed {
                            language,
                            source: LanguageSource::Selected,
                        };
                    }
                    _ => {}
                }
            }
            Plan::Native
        }
        AutoLanguage::Single(language) => Plan::Fixed {
            source: if is_multilingual(model.languages) {
                LanguageSource::Selected
            } else {
                LanguageSource::Model
            },
            language,
        },
        AutoLanguage::Detect { languages, allowed } => Plan::Detect {
            languages,
            allowed,
            fallback: DetectFallback::FirstLanguage,
        },
        AutoLanguage::Undetectable { compatible, .. } => Plan::Fixed {
            language: compatible[0].clone(),
            source: LanguageSource::Fallback,
        },
        AutoLanguage::NoMatch => Plan::Fixed {
            language: effective_language("auto", preferred, model.languages, false),
            source: LanguageSource::Fallback,
        },
    }
}

/// What the language picker shows: the language a transcription will run in,
/// or Auto when it's decided per transcription. Derived from [`plan`], so it
/// can't disagree with what transcription does.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LanguageStatus {
    /// The language they picked.
    Selected { language: String },
    /// Decided per transcription: the model's own detection, or Handy's.
    Auto,
    /// Auto needs Handy's detector, which isn't downloaded; `fallback` (the
    /// first preferred language) is used until it is.
    DetectorMissing { fallback: String },
    /// Auto can't work here; `language` is used instead.
    AutoUnavailable {
        language: String,
        reason: AutoUnavailableReason,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutoUnavailableReason {
    /// Only one of their languages (or only one at all) fits the model.
    OneLanguage,
    /// The detector can't identify these spoken languages.
    Undetectable { languages: Vec<String> },
    /// None of their languages fit the model.
    NoMatch,
}

pub fn status(
    intent: &str,
    preferred: &[String],
    model: &ModelLanguages,
    mode: LanguageDetectionMode,
    labels: &[String],
    detector_ready: bool,
) -> LanguageStatus {
    let explicit = intent != "auto"
        && (model.languages.is_empty() || supported_code(intent, model.languages).is_some());
    match plan(intent, preferred, model, mode, labels) {
        Plan::Native => LanguageStatus::Auto,
        Plan::Detect {
            languages,
            fallback: DetectFallback::FirstLanguage,
            ..
        } if !detector_ready => LanguageStatus::DetectorMissing {
            fallback: languages[0].clone(),
        },
        Plan::Detect { .. } => LanguageStatus::Auto,
        Plan::Fixed { language, .. } if explicit => LanguageStatus::Selected { language },
        Plan::Fixed { language, .. } => LanguageStatus::AutoUnavailable {
            reason: match auto_language(model, preferred, labels) {
                AutoLanguage::Undetectable { undetectable, .. } => {
                    AutoUnavailableReason::Undetectable {
                        languages: undetectable,
                    }
                }
                AutoLanguage::NoMatch => AutoUnavailableReason::NoMatch,
                _ => AutoUnavailableReason::OneLanguage,
            },
            language,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AutoUnavailableReason::{NoMatch, OneLanguage, Undetectable};
    use DetectFallback::{FirstLanguage, Native};
    use LanguageDetectionMode::{Always, WhenNeeded};
    use LanguageSource::{Fallback, Model, Selected};

    fn codes(list: &str) -> Vec<String> {
        list.split_whitespace().map(String::from).collect()
    }

    const LABELS: &str = "de en es fr iw ja zh";

    fn model(languages: &[String], detects: bool, streaming: bool) -> ModelLanguages<'_> {
        ModelLanguages {
            languages,
            detects,
            streaming,
        }
    }

    fn plan_on(
        model: &ModelLanguages,
        intent: &str,
        spoken: &str,
        mode: LanguageDetectionMode,
    ) -> Plan {
        plan(intent, &codes(spoken), model, mode, &codes(LABELS))
    }

    fn fixed(language: &str, source: LanguageSource) -> Plan {
        Plan::Fixed {
            language: language.into(),
            source,
        }
    }

    fn detect(languages: &str, allowed: &str, fallback: DetectFallback) -> Plan {
        Plan::Detect {
            languages: codes(languages),
            allowed: codes(allowed),
            fallback,
        }
    }

    #[test]
    fn needs_the_detector_only_without_native_multilingual_detection() {
        let (multi, one) = (codes("en de"), codes("en"));
        assert!(requires_language_detector(&model(&multi, false, false)));
        assert!(!requires_language_detector(&model(&multi, true, false)));
        assert!(!requires_language_detector(&model(&one, false, false)));
    }

    #[test]
    fn plans_the_language_without_native_detection() {
        let languages = codes("en he fr yue");
        let canary = model(&languages, false, false);
        // The spoken languages it supports, in order, as the detector's labels.
        let both = detect("he en", "iw en", FirstLanguage);
        assert_eq!(plan_on(&canary, "auto", "he ja en", WhenNeeded), both);
        assert_eq!(
            plan_on(&canary, "auto", "ja fr", WhenNeeded),
            fixed("fr", Selected)
        );
        assert_eq!(
            plan_on(&canary, "auto", "yue en", WhenNeeded),
            fixed("yue", Fallback)
        );
        assert_eq!(
            plan_on(&canary, "auto", "ja", WhenNeeded),
            fixed("en", Fallback)
        );
        assert_eq!(
            plan_on(&canary, "fr", "he en", Always),
            fixed("fr", Selected)
        );

        let english = codes("en");
        let parakeet = model(&english, false, false);
        assert_eq!(
            plan_on(&parakeet, "auto", "de en", Always),
            fixed("en", Model)
        );
    }

    #[test]
    fn native_detection_is_kept_unless_always_and_not_streaming() {
        let languages = codes("en de");
        let whisper = model(&languages, true, false);
        let always = detect("en de", "en de", Native);
        assert_eq!(plan_on(&whisper, "auto", "en de", WhenNeeded), Plan::Native);
        assert_eq!(plan_on(&whisper, "auto", "en de", Always), always);
        assert_eq!(plan_on(&whisper, "auto", "en", WhenNeeded), Plan::Native);
        assert_eq!(
            plan_on(&whisper, "auto", "en ja", Always),
            fixed("en", Selected)
        );
        let streaming = model(&languages, true, true);
        assert_eq!(plan_on(&streaming, "auto", "en de", Always), Plan::Native);
        assert_eq!(plan_on(&streaming, "auto", "en", Always), Plan::Native);
    }

    #[test]
    fn status_shows_what_transcription_does() {
        let languages = codes("en de fr yue");
        let canary = model(&languages, false, false);
        let labels = codes(LABELS);
        let auto = |spoken: &str, ready| {
            status("auto", &codes(spoken), &canary, WhenNeeded, &labels, ready)
        };
        let unavailable = |language: &str, reason| LanguageStatus::AutoUnavailable {
            language: language.into(),
            reason,
        };
        let missing = LanguageStatus::DetectorMissing {
            fallback: "de".into(),
        };
        let undetectable = Undetectable {
            languages: codes("yue"),
        };
        assert_eq!(auto("de en", true), LanguageStatus::Auto);
        assert_eq!(auto("de en", false), missing);
        assert_eq!(auto("fr ja", true), unavailable("fr", OneLanguage));
        assert_eq!(auto("yue en", true), unavailable("yue", undetectable));
        assert_eq!(auto("ja", true), unavailable("en", NoMatch));
    }
}
