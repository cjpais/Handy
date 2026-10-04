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
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PeriodHandling {
    #[default]
    Keep,
    RemoveFinal,
    RemoveSentence,
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
            enabled: false,
            spoken_punctuation: true,
            initial_capitalization: InitialCapitalization::Keep,
            periods: PeriodHandling::Keep,
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
                    && !decimal_or_ellipsis
                    && match config.periods {
                        PeriodHandling::RemoveFinal => Some(i) == final_index,
                        PeriodHandling::RemoveSentence => next.is_none_or(|v| {
                            v.is_whitespace() || matches!(v, '"' | '”' | '’' | ')')
                        }),
                        PeriodHandling::Keep => false,
                    };
                (!remove).then_some(c)
            })
            .collect();
    }
    if config.initial_capitalization != InitialCapitalization::Keep {
        if let Some((index, first)) = result.char_indices().find(|(_, c)| c.is_alphabetic()) {
            let replacement: String = match config.initial_capitalization {
                InitialCapitalization::Lower => first.to_lowercase().collect(),
                InitialCapitalization::Upper => first.to_uppercase().collect(),
                InitialCapitalization::Keep => first.to_string(),
            };
            result.replace_range(index..index + first.len_utf8(), &replacement);
        }
    }
    result.replace('\u{e000}', ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> TextFormatting {
        TextFormatting {
            enabled: true,
            spoken_punctuation: true,
            replacements: default_replacements(),
            ..Default::default()
        }
    }
    #[test]
    fn defaults_preserve_existing_transcriptions() {
        assert_eq!(
            finish("Hello exclamation point.", &TextFormatting::default()),
            "Hello exclamation point."
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
