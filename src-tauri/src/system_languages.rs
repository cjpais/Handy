//! OS preferred languages and the spoken-language / Chinese-script guesses made
//! from them, for onboarding and the settings migration.

use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Serialize;

use crate::settings::ChineseScript;

/// How many languages a guess offers, before English rides along.
const MAX_GUESSED: usize = 3;

/// What reading the OS's languages found.
struct Read {
    languages: Vec<String>,
    /// Why the platform's own list couldn't be read, if it couldn't.
    problem: Option<String>,
}

/// Read once: they rarely change while Handy runs.
static READ: Lazy<Read> = Lazy::new(read);

/// The OS's preferred languages, most preferred first, as BCP-47 tags
/// (`en-US`, `zh-Hant-TW`).
pub fn system_languages() -> &'static [String] {
    &READ.languages
}

/// Logs what was read. Called from setup, since the first read precedes logging.
pub fn log_system_languages() {
    if let Some(problem) = &READ.problem {
        warn!("{problem}");
    }
    info!("System languages: {:?}", READ.languages);
}

/// Time & Language's list, which includes keyboard-only languages that
/// `sys-locale`'s display languages miss.
#[cfg(windows)]
fn read() -> Read {
    use windows::System::UserProfile::GlobalizationPreferences;
    let languages = GlobalizationPreferences::Languages()
        .map(|list| list.into_iter().map(|l| l.to_string()).collect::<Vec<_>>());
    match languages {
        Ok(languages) if !languages.is_empty() => Read {
            languages,
            problem: None,
        },
        result => Read {
            languages: sys_locale::get_locales().collect(),
            problem: Some(match result {
                Err(e) => format!(
                    "Couldn't read Windows' preferred languages ({e}); using the display languages"
                ),
                Ok(_) => {
                    "Windows listed no preferred languages; using the display languages".to_string()
                }
            }),
        },
    }
}

/// macOS: System Settings → Language & Region's Preferred Languages.
/// Linux: `LANGUAGE`, else the locale.
#[cfg(not(windows))]
fn read() -> Read {
    Read {
        languages: sys_locale::get_locales().collect(),
        problem: None,
    }
}

/// Up to [`MAX_GUESSED`] picker languages from `tags`, deduplicated, plus English.
pub fn suggested_languages<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut languages: Vec<String> = Vec::new();
    for language in tags.into_iter().map(crate::catalog::language_identity) {
        if languages.len() == MAX_GUESSED {
            break;
        }
        if crate::catalog::offers_language(&language) && !languages.contains(&language) {
            languages.push(language);
        }
    }
    if !languages.iter().any(|l| l == "en") {
        languages.push("en".to_string());
    }
    languages
}

/// The Chinese script the first Chinese tag names (`zh-Hant-US` →
/// Traditional), if any tag is Chinese.
pub fn suggested_chinese_script<'a>(
    tags: impl IntoIterator<Item = &'a str>,
) -> Option<ChineseScript> {
    tags.into_iter()
        .find_map(crate::chinese_script::chinese_script_for_locale)
}

/// What onboarding suggests, from the OS's languages.
#[derive(Serialize)]
struct Suggestion {
    languages: Vec<String>,
    chinese_script: Option<ChineseScript>,
}

/// Injects the suggestion as `window.__HANDY_SYSTEM_LANGUAGES__`, since onboarding
/// needs it synchronously before a command could answer.
pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let tags = system_languages().iter().map(String::as_str);
    let suggestion = Suggestion {
        languages: suggested_languages(tags.clone()),
        chinese_script: suggested_chinese_script(tags),
    };
    let json = serde_json::to_string(&suggestion).expect("suggestion serializes");
    tauri::plugin::Builder::new("system-languages")
        .js_init_script(format!("window.__HANDY_SYSTEM_LANGUAGES__ = {json};"))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_offered_languages_in_order_with_english() {
        for (tags, languages) in [
            (vec!["en-US", "zh-Hant-US"], vec!["en", "zh"]),
            (vec!["zh-Hans-CN", "en-GB"], vec!["zh", "en"]),
            (vec!["yue-Hant-HK"], vec!["yue", "en"]),
            (vec!["en-US", "en-GB"], vec!["en"]),
            (
                vec!["de-DE", "nb-NO", "no", "fr-FR", "es-ES"],
                vec!["de", "nb", "fr", "en"],
            ),
            (vec!["xx-YY"], vec!["en"]),
            (vec![], vec!["en"]),
        ] {
            assert_eq!(suggested_languages(tags.clone()), languages, "{tags:?}");
        }
    }

    #[test]
    fn suggests_the_first_chinese_tags_script() {
        for (tags, script) in [
            (
                vec!["en-US", "zh-Hant-US"],
                Some(ChineseScript::TraditionalTw),
            ),
            (
                vec!["zh-Hans-CN", "zh-Hant-TW"],
                Some(ChineseScript::Simplified),
            ),
            (
                vec!["en-GB", "zh-Hant-HK"],
                Some(ChineseScript::TraditionalHk),
            ),
            (vec!["yue-Hant-HK"], Some(ChineseScript::TraditionalHk)),
            (vec!["en-US", "ja-JP"], None),
        ] {
            assert_eq!(suggested_chinese_script(tags.clone()), script, "{tags:?}");
        }
    }
}
