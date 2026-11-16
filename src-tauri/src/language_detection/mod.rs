//! Spoken-language detection for Auto: Handy's detector picks between the
//! person's languages for models that can't detect the language themselves
//! (and, in Always mode, for those that can). It never blocks a transcription.

mod detector;
pub mod policy;
pub mod speech;

use log::{info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::Duration;

pub use detector::LanguageDetector;
pub use policy::{DetectFallback, LanguageStatus, Plan};

use crate::managers::model::ModelInfo;
use crate::settings::AppSettings;
use policy::ModelLanguages;

/// The detector's labels, from the catalog.
fn detector_labels() -> &'static [String] {
    crate::catalog::LANGUAGE_DETECTOR
        .as_ref()
        .map(|detector| detector.labels.as_slice())
        .unwrap_or_default()
}

fn model_languages(model: Option<&ModelInfo>, streaming: bool) -> ModelLanguages<'_> {
    ModelLanguages {
        languages: model
            .map(|m| m.supported_languages.as_slice())
            .unwrap_or_default(),
        detects: model.is_some_and(|m| m.supports_language_detection),
        streaming,
    }
}

/// How a transcription on `model` picks its language (`None`: a model the
/// registry doesn't know). Computed fresh each time and never written back
/// to settings, so the persisted intent survives switching models and back.
pub fn plan_for(settings: &AppSettings, model: Option<&ModelInfo>, streaming: bool) -> Plan {
    policy::plan(
        &settings.selected_language,
        &settings.spoken_languages,
        &model_languages(model, streaming),
        settings.language_detection,
        detector_labels(),
    )
}

/// What the language picker shows for `model`.
pub fn status_for(
    settings: &AppSettings,
    model: Option<&ModelInfo>,
    detector_ready: bool,
) -> LanguageStatus {
    policy::status(
        &settings.selected_language,
        &settings.spoken_languages,
        &model_languages(model, model.is_some_and(|m| m.supports_streaming)),
        settings.language_detection,
        detector_labels(),
        detector_ready,
    )
}

/// The detector rejects less speech than this (`InputTooShort`).
const MIN_SPEECH_SAMPLES: usize = 16_000 / 2;

/// A pick less than this many times as likely as the runner-up is logged as
/// uncertain (with two languages: p < 0.6). It is used either way.
const UNSURE_RATIO: f32 = 1.5;

/// How a transcription's language was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LanguageSource {
    /// The person picked it, or it is the only language of theirs the model
    /// supports.
    Selected,
    /// The model decides: its own detection, or its only language.
    Model,
    /// Handy's detector picked it.
    Detected,
    /// Handy had to pick without detecting (too little speech, detection
    /// failed or unavailable, or Auto can't work for these languages).
    Fallback,
}

impl LanguageSource {
    /// How it is stored in history.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Model => "model",
            Self::Detected => "detected",
            Self::Fallback => "fallback",
        }
    }

    /// Reads [`as_str`](Self::as_str); `None` for anything else (a newer
    /// Handy's value).
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Selected, Self::Model, Self::Detected, Self::Fallback]
            .into_iter()
            .find(|source| source.as_str() == value)
    }
}

/// The language one transcription runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDecision {
    /// The model's code, or `None` for the model's own detection.
    pub language: Option<String>,
    pub source: LanguageSource,
}

impl LanguageDecision {
    /// As a language intent: the code, or `"auto"`.
    pub fn intent(&self) -> &str {
        self.language.as_deref().unwrap_or("auto")
    }
}

/// A detector run: `(label, p)` ranked by `p`, over the allowed labels.
pub struct Identified {
    pub candidates: Vec<(String, f32)>,
    pub allowed_mass: f32,
    pub elapsed: Duration,
}

pub enum IdentifyError {
    /// The detector isn't on disk.
    Missing,
    Failed(anyhow::Error),
}

impl Plan {
    /// Whether [`decide`](Self::decide) needs the audio's speech.
    pub fn detects(&self) -> bool {
        matches!(self, Plan::Detect { .. })
    }

    /// The language, given the transcription's `speech` (VAD'd, at most the
    /// detection window), or `None` if the detector isn't on disk. `identify`
    /// runs the detector over `allowed`.
    pub fn decide(
        &self,
        speech: Option<&[f32]>,
        identify: impl FnOnce(&[f32], &[String]) -> Result<Identified, IdentifyError>,
    ) -> LanguageDecision {
        let (languages, allowed, fallback) = match self {
            Plan::Fixed { language, source } => {
                return LanguageDecision {
                    language: Some(language.clone()),
                    source: *source,
                }
            }
            Plan::Native => {
                return LanguageDecision {
                    language: None,
                    source: LanguageSource::Model,
                }
            }
            Plan::Detect {
                languages,
                allowed,
                fallback,
            } => (languages, allowed, *fallback),
        };

        let fallback_decision = LanguageDecision {
            language: match fallback {
                DetectFallback::FirstLanguage => languages.first().cloned(),
                DetectFallback::Native => None,
            },
            source: LanguageSource::Fallback,
        };
        let transcribing_as = match &fallback_decision.language {
            Some(language) => format!("transcribing as {language}, the first preferred language"),
            None => "using the model's own detection".to_string(),
        };
        let Some(speech) = speech else {
            info!("Language detection: the detector isn't downloaded; {transcribing_as}");
            return fallback_decision;
        };
        let seconds = speech.len() as f32 / 16_000.0;

        if speech.len() < MIN_SPEECH_SAMPLES {
            info!(
                "Language detection: skipped, {seconds:.2} s of speech is under the 0.5 s minimum; {transcribing_as}"
            );
            return fallback_decision;
        }
        let identified = match identify(speech, allowed) {
            Ok(identified) => identified,
            Err(IdentifyError::Missing) => {
                info!("Language detection: the detector isn't downloaded; {transcribing_as}");
                return fallback_decision;
            }
            Err(IdentifyError::Failed(e)) => {
                warn!("Language detection failed: {e:#}; {transcribing_as}");
                return fallback_decision;
            }
        };

        let top = identified.candidates.first();
        let language = top
            .filter(|(_, p)| p.is_finite())
            .and_then(|(label, _)| allowed.iter().position(|a| a == label))
            .and_then(|i| languages.get(i))
            .cloned();
        let scores = identified
            .candidates
            .iter()
            .map(|(label, p)| format!("{label} {:.1}%", p * 100.0))
            .collect::<Vec<_>>()
            .join(", ");
        let uncertain = match (top, identified.candidates.get(1)) {
            (Some((_, first)), Some((_, second))) if *first < UNSURE_RATIO * second => {
                ", uncertain"
            }
            _ => "",
        };
        info!(
            "Language detection: {} ({scores}; {seconds:.2} s of speech, allowed mass {:.1}%{uncertain}, {} ms)",
            language.as_deref().unwrap_or("none"),
            identified.allowed_mass * 100.0,
            identified.elapsed.as_millis()
        );
        match language {
            Some(language) => LanguageDecision {
                language: Some(language),
                source: LanguageSource::Detected,
            },
            None => {
                warn!("Language detection returned no usable language; {transcribing_as}");
                fallback_decision
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|c| c.to_string()).collect()
    }

    fn detect(fallback: DetectFallback) -> Plan {
        Plan::Detect {
            languages: codes(&["he", "en"]),
            allowed: codes(&["iw", "en"]),
            fallback,
        }
    }

    fn ranked(candidates: &[(&str, f32)]) -> Result<Identified, IdentifyError> {
        Ok(Identified {
            candidates: candidates
                .iter()
                .map(|(label, p)| (label.to_string(), *p))
                .collect(),
            allowed_mass: 0.9,
            elapsed: Duration::ZERO,
        })
    }

    static SPEECH: [f32; 16_000] = [0.1; 16_000];

    #[test]
    fn detects_in_the_models_own_code() {
        let decision = detect(DetectFallback::FirstLanguage).decide(Some(&SPEECH), |_, allowed| {
            assert_eq!(allowed, codes(&["iw", "en"]));
            ranked(&[("iw", 0.9), ("en", 0.1)])
        });
        assert_eq!(decision.language.as_deref(), Some("he"));
        assert_eq!(decision.source, LanguageSource::Detected);
    }

    #[test]
    fn falls_back_when_detection_cant_decide() {
        let first = detect(DetectFallback::FirstLanguage);
        let fallback = |language: Option<&str>| LanguageDecision {
            language: language.map(String::from),
            source: LanguageSource::Fallback,
        };
        let short = &SPEECH[..MIN_SPEECH_SAMPLES - 1];
        assert_eq!(
            first.decide(Some(short), |_, _| panic!("too little speech to run")),
            fallback(Some("he"))
        );
        assert_eq!(
            first.decide(None, |_, _| panic!("no detector to run")),
            fallback(Some("he"))
        );
        let missing = first.decide(Some(&SPEECH), |_, _| Err(IdentifyError::Missing));
        assert_eq!(missing, fallback(Some("he")));
        // Always mode falls back to the model's own detection.
        let failed = detect(DetectFallback::Native).decide(Some(&SPEECH), |_, _| {
            Err(IdentifyError::Failed(anyhow::anyhow!("boom")))
        });
        assert_eq!(failed, fallback(None));
    }
}
