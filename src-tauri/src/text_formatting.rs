//! Deterministic formatting around optional AI post-processing.
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PunctuationReplacement {
    pub phrase: String,
    pub replacement: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InitialCapitalization {
    #[default]
    Keep,
    Lower,
    Upper,
    AfterPeriod,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PeriodHandling {
    #[default]
    Keep,
    RemoveFinal,
    RemoveSentence,
    SpokenOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct TextFormatting {
    pub enabled: bool,
    pub spoken_punctuation: bool,
    pub initial_capitalization: InitialCapitalization,
    pub periods: PeriodHandling,
    pub replacements: Vec<PunctuationReplacement>,
}

impl Default for TextFormatting {
    fn default() -> Self {
        Self {
            enabled: true,
            spoken_punctuation: true,
            initial_capitalization: InitialCapitalization::AfterPeriod,
            periods: PeriodHandling::SpokenOnly,
            replacements: default_replacements(),
        }
    }
}

pub fn default_replacements() -> Vec<PunctuationReplacement> {
    [
        ("exclamation point", "!"),
        ("exclamation mark", "!"),
        ("question mark", "?"),
        ("full stop", "."),
        ("period", "."),
        ("comma", ","),
        ("semicolon", ";"),
        ("colon", ":"),
        ("new paragraph", "\n\n"),
        ("new line", "\n"),
    ]
    .into_iter()
    .map(|(phrase, replacement)| PunctuationReplacement {
        phrase: phrase.into(),
        replacement: replacement.into(),
    })
    .collect()
}

pub fn validate(config: &TextFormatting) -> Result<(), String> {
    if config.replacements.len() > 100 {
        return Err("Use at most 100 punctuation replacements".into());
    }
    let mut phrases = std::collections::HashSet::new();
    for entry in &config.replacements {
        let phrase = entry.phrase.trim();
        if phrase.is_empty() || phrase.len() > 200 || entry.replacement.len() > 100 {
            return Err(
                "Replacement phrases must contain 1–200 bytes; replacements at most 100 bytes"
                    .into(),
            );
        }
        if !phrases.insert(phrase.to_lowercase()) {
            return Err("Replacement phrases must be unique (ignoring case)".into());
        }
    }
    Ok(())
}

/// Replace phrases in one pass, so replacements never trigger other rules.
/// A spoken period is kept distinguishable until final formatting, preserving explicit commands.
pub fn replace_spoken(text: &str, config: &TextFormatting) -> String {
    if !config.enabled || !config.spoken_punctuation {
        return text.into();
    }
    let mut entries: Vec<_> = config
        .replacements
        .iter()
        .filter(|e| !e.phrase.trim().is_empty())
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.phrase.len()));
    if entries.is_empty() {
        return text.into();
    }
    let pattern = format!(
        r"(?i)(?:{})",
        entries
            .iter()
            .map(|e| regex::escape(e.phrase.trim()))
            .collect::<Vec<_>>()
            .join("|")
    );
    let Ok(regex) = Regex::new(&pattern) else {
        return text.into();
    };
    let mut result = String::new();
    let mut cursor = 0;
    for found in regex.find_iter(text) {
        if found.start() < cursor {
            continue;
        }
        let word = |c: char| c.is_alphanumeric() || c == '_';
        if text[..found.start()].chars().next_back().is_some_and(word)
            || text[found.end()..].chars().next().is_some_and(word)
        {
            continue;
        }
        let Some(entry) = entries
            .iter()
            .find(|e| e.phrase.trim().to_lowercase() == found.as_str().to_lowercase())
        else {
            continue;
        };
        result.push_str(&text[cursor..found.start()]);
        let punctuation = matches!(
            entry.replacement.as_str(),
            "." | "," | ";" | ":" | "!" | "?"
        );
        if punctuation {
            while result.ends_with([' ', '\t', ',', '.', ';', ':']) {
                result.pop();
            }
        }
        if entry.replacement.contains('\n') {
            while result.ends_with([' ', '\t']) {
                result.pop();
            }
        }
        if entry.replacement == "." {
            result.push('\u{e000}');
        } else {
            result.push_str(&entry.replacement);
        }
        cursor = found.end();
        if entry.replacement.contains('\n') {
            while text[cursor..].starts_with([' ', '\t']) {
                cursor += 1;
            }
        }
        if punctuation {
            // Discard a recognizer-added period directly after a spoken command.
            if text[cursor..].starts_with('.') {
                cursor += 1;
            }
        }
    }
    result.push_str(&text[cursor..]);
    result
}

pub fn finish(text: &str, config: &TextFormatting) -> String {
    if !config.enabled {
        return text.into();
    }
    let mut result = replace_spoken(text, config);
    if config.initial_capitalization == InitialCapitalization::AfterPeriod {
        let mut pending = Some(false);
        result = result
            .chars()
            .flat_map(|c| {
                if matches!(c, '\u{e000}' | '\n' | '\r') {
                    pending = Some(true);
                } else if matches!(c, '.' | '?' | '!') && pending != Some(true) {
                    pending = Some(false);
                }
                if c.is_alphabetic() {
                    if let Some(upper) = pending.take() {
                        return if upper {
                            c.to_uppercase().collect::<Vec<_>>()
                        } else {
                            c.to_lowercase().collect::<Vec<_>>()
                        };
                    }
                }
                vec![c]
            })
            .collect();
    }
    if config.periods != PeriodHandling::Keep {
        let chars: Vec<_> = result.chars().collect();
        let final_index = chars
            .iter()
            .rposition(|c| !c.is_whitespace() && !matches!(c, '"' | '”' | '’' | ')'));
        result = chars
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| {
                let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
                let next = chars.get(i + 1).copied();
                let decimal_or_ellipsis = prev == Some('.')
                    || next == Some('.')
                    || (prev.is_some_and(|v| v.is_ascii_digit())
                        && next.is_some_and(|v| v.is_ascii_digit()));
                let remove = c == '.'
                    && (!decimal_or_ellipsis
                        || (config.periods == PeriodHandling::SpokenOnly
                            && !(prev.is_some_and(|v| v.is_ascii_digit())
                                && next.is_some_and(|v| v.is_ascii_digit()))))
                    && match config.periods {
                        PeriodHandling::RemoveFinal => Some(i) == final_index,
                        PeriodHandling::RemoveSentence => next.is_none_or(|v| {
                            v.is_whitespace() || matches!(v, '"' | '”' | '’' | ')')
                        }),
                        PeriodHandling::SpokenOnly => true,
                        PeriodHandling::Keep => false,
                    };
                (!remove).then_some(c)
            })
            .collect();
    }
    if !matches!(
        config.initial_capitalization,
        InitialCapitalization::Keep | InitialCapitalization::AfterPeriod
    ) {
        if let Some((index, first)) = result.char_indices().find(|(_, c)| c.is_alphabetic()) {
            let replacement: String = match config.initial_capitalization {
                InitialCapitalization::Lower => first.to_lowercase().collect(),
                InitialCapitalization::Upper => first.to_uppercase().collect(),
                InitialCapitalization::Keep | InitialCapitalization::AfterPeriod => {
                    first.to_string()
                }
            };
            result.replace_range(index..index + first.len_utf8(), &replacement);
        }
    }
    result.replace('\u{e000}', ".")
}

/// Carry sentence boundaries across consecutive dictations in this app session.
#[cfg(test)]
pub fn finish_dictation(text: &str, config: &TextFormatting) -> String {
    finish_dictation_with_context(text, config, None)
}

pub fn finish_dictation_with_context(
    text: &str,
    config: &TextFormatting,
    cursor_capitalization: Option<bool>,
) -> String {
    static AFTER_PERIOD: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    let Ok(mut after_period) = AFTER_PERIOD.lock() else {
        return finish(text, config);
    };
    let mut result = finish(text, config);
    if config.enabled
        && config.initial_capitalization == InitialCapitalization::AfterPeriod
        && cursor_capitalization.unwrap_or(*after_period)
    {
        if let Some((index, first)) = result.char_indices().find(|(_, c)| c.is_alphabetic()) {
            result.replace_range(
                index..index + first.len_utf8(),
                &first.to_uppercase().collect::<String>(),
            );
        }
    }
    if !result.trim().is_empty() {
        *after_period =
            config.enabled && (result.trim_end().ends_with('.') || result.ends_with('\n'));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> TextFormatting {
        TextFormatting {
            enabled: true,
            spoken_punctuation: true,
            replacements: default_replacements(),
            initial_capitalization: InitialCapitalization::Keep,
            periods: PeriodHandling::Keep,
            ..Default::default()
        }
    }
    #[test]
    fn defaults_use_only_spoken_periods_and_capitalize_after_them() {
        assert_eq!(
            finish("Hello exclamation point.", &TextFormatting::default()),
            "hello!"
        );
        let c = TextFormatting::default();
        assert_eq!(
            finish("Hello. World period. Next sentence.", &c),
            "hello world. Next sentence"
        );
        assert_eq!(finish("Hello? World! Again.", &c), "hello? world! again");
        assert_eq!(finish("Version 3.14 period Next", &c), "version 3.14. Next");
        assert_eq!(finish("Wait... Again.", &c), "wait again");
        assert_eq!(
            finish("Hello period . next sentence.", &c),
            "hello.  Next sentence"
        );
        assert_eq!(finish("Hello new line next line.", &c), "hello\nNext line");
        assert_eq!(
            finish_dictation_with_context("Hello.", &c, Some(true)),
            "Hello"
        );
        assert_eq!(
            finish_dictation_with_context("Hello period", &c, Some(true)),
            "Hello."
        );
        assert_eq!(
            finish_dictation_with_context("Next word.", &c, Some(false)),
            "next word"
        );
        assert_eq!(finish_dictation("Hello period", &c), "hello.");
        assert_eq!(finish_dictation("Next sentence.", &c), "Next sentence");
        assert_eq!(
            finish_dictation("Another sentence.", &c),
            "another sentence"
        );
    }
    #[test]
    fn spoken_commands_and_word_boundaries() {
        let c = config();
        assert_eq!(finish("Hello, EXCLAMATION POINT.", &c), "Hello!");
        assert_eq!(finish("One comma two question mark.", &c), "One, two?");
        assert_eq!(finish("Periodic table.", &c), "Periodic table.");
        assert_eq!(finish("One new paragraph two", &c), "One\n\ntwo");
    }
    #[test]
    fn independent_options_and_explicit_periods() {
        let mut c = config();
        c.initial_capitalization = InitialCapitalization::Lower;
        c.periods = PeriodHandling::RemoveFinal;
        assert_eq!(finish("Hello. World. ", &c), "hello. World ");
        assert_eq!(finish("Hello period.", &c), "hello.");
        assert_eq!(finish("Really?", &c), "really?");
        assert_eq!(finish("3.14...", &c), "3.14...");
        c.periods = PeriodHandling::RemoveSentence;
        assert_eq!(finish("Hello. World.", &c), "hello World");
    }
    #[test]
    fn unicode_and_empty_input() {
        let mut c = config();
        c.initial_capitalization = InitialCapitalization::Lower;
        assert_eq!(finish("\"Élan.\"", &c), "\"élan.\"");
        assert_eq!(finish("", &c), "");
    }
    #[test]
    fn rules_are_literal_non_recursive_and_validated() {
        let mut c = config();
        c.replacements = vec![PunctuationReplacement {
            phrase: "a+b".into(),
            replacement: "$1 comma".into(),
        }];
        assert_eq!(finish("a+b", &c), "$1 comma");
        c.replacements.push(PunctuationReplacement {
            phrase: "A+B".into(),
            replacement: "!".into(),
        });
        assert!(validate(&c).is_err());
    }
}
