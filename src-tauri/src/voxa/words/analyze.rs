//! Text to words: tokens (the tokenizer of the basic statistics), a normalized
//! form, a stem, and the segments n-grams must not cross.

use crate::voxa::text_stats::{self, Script};
use rust_stemmers::{Algorithm, Stemmer};
use std::sync::OnceLock;

/// Characters between two tokens that end a sentence: phrases do not cross them.
const SENTENCE_BREAKS: &[char] = &['.', '!', '?', '\u{2026}', '\n'];

struct Stemmers {
    english: Stemmer,
    russian: Stemmer,
}

fn stemmers() -> &'static Stemmers {
    static STEMMERS: OnceLock<Stemmers> = OnceLock::new();
    STEMMERS.get_or_init(|| Stemmers {
        english: Stemmer::create(Algorithm::English),
        russian: Stemmer::create(Algorithm::Russian),
    })
}

/// Lowercase, `ё` -> `е` (also decomposed), typographic apostrophe -> `'`, and
/// an inflection after an apostrophe cut off (`Claude'а` -> `claude`, `Claude's`
/// -> `claude`; `don't` stays).
pub fn normalize_form(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        match c {
            // е + COMBINING DIAERESIS is a decomposed ё
            '\u{0308}' if out.ends_with('е') => {}
            'ё' | 'Ё' => out.push('е'),
            '\u{2019}' => out.push('\''),
            _ => out.extend(c.to_lowercase()),
        }
    }
    if let Some(at) = out.find('\'') {
        let suffix = &out[at + 1..];
        if suffix == "s" || suffix.chars().any(text_stats::is_cyrillic) {
            out.truncate(at);
        }
    }
    out
}

/// Lowercase with `ё` folded to `е`, nothing else: for cheap "does this text
/// contain these letters" checks (unlike [`normalize_form`], which cuts a word at
/// an apostrophe).
pub fn fold_case(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ё' { 'е' } else { c })
        .collect()
}

/// Snowball stem: the Russian stemmer for Cyrillic words, the English one for
/// words with an ASCII letter, the word itself for everything else.
pub fn stem_of(form: &str) -> String {
    let stemmers = stemmers();
    match text_stats::classify(form) {
        Script::Latin => stemmers.english.stem(form).into_owned(),
        Script::Cyrillic => stemmers.russian.stem(form).into_owned(),
        Script::Other => form.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Normalized, lowercase form.
    pub form: String,
    pub stem: String,
    /// Byte span in the analyzed text (original case, for highlighting).
    pub start: usize,
    pub end: usize,
    /// Tokens of one segment follow each other without a sentence end or a
    /// number between them.
    pub segment: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// Word tokens in order. Purely numeric tokens are not words and are left out.
    pub tokens: Vec<Token>,
}

pub fn analyze(text: &str) -> Analysis {
    let mut tokens = Vec::new();
    let mut segment = 0u32;
    let mut prev_end: Option<usize> = None;

    for raw in text_stats::tokens(text) {
        let start = raw.as_ptr() as usize - text.as_ptr() as usize;
        let end = start + raw.len();
        let gap_breaks = prev_end
            .map(|p| text[p..start].contains(SENTENCE_BREAKS))
            .unwrap_or(false);
        if gap_breaks {
            segment += 1;
        }
        prev_end = Some(end);

        if !raw.chars().any(char::is_alphabetic) {
            // a number: not a word, and phrases do not run through it
            segment += 1;
            continue;
        }
        let form = normalize_form(raw);
        if form.is_empty() {
            continue;
        }
        let stem = stem_of(&form);
        tokens.push(Token {
            form,
            stem,
            start,
            end,
            segment,
        });
    }
    Analysis { tokens }
}

impl Analysis {
    /// Start indexes of the windows of `n` consecutive tokens that lie inside
    /// one segment.
    pub fn window_starts(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        self.tokens
            .windows(n)
            .enumerate()
            .filter(|(_, w)| w.first().map(|f| f.segment) == w.last().map(|l| l.segment))
            .map(|(i, _)| i)
    }

    /// Windows of `n` consecutive tokens inside one segment.
    #[cfg(test)]
    pub fn windows(&self, n: usize) -> impl Iterator<Item = &[Token]> {
        self.window_starts(n).map(move |i| &self.tokens[i..i + n])
    }

    /// The first one and the first two words of the text.
    pub fn openers(&self) -> (Option<String>, Option<String>) {
        let one = self.tokens.first().map(|t| t.form.clone());
        let two = match (self.tokens.first(), self.tokens.get(1)) {
            (Some(a), Some(b)) => Some(format!("{} {}", a.form, b.form)),
            _ => None,
        };
        (one, two)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(text: &str) -> Vec<String> {
        analyze(text).tokens.into_iter().map(|t| t.form).collect()
    }

    fn stems(text: &str) -> Vec<String> {
        analyze(text).tokens.into_iter().map(|t| t.stem).collect()
    }

    #[test]
    fn forms_are_lowercase_with_yo_folded() {
        assert_eq!(forms("Ёлка ЁЖИК Всё"), vec!["елка", "ежик", "все"]);
        // decomposed ё
        assert_eq!(forms("е\u{0308}лка"), vec!["елка"]);
    }

    #[test]
    fn numbers_are_not_words() {
        assert_eq!(forms("в 2026 году 3.11 и 3,14"), vec!["в", "году", "и"]);
        assert_eq!(forms("GPT-4o и e-mail"), vec!["gpt-4o", "и", "e-mail"]);
    }

    #[test]
    fn forms_of_one_word_share_a_stem() {
        let s = stems("промпт промпта промптом промпте");
        assert!(s.iter().all(|x| *x == s[0]), "{s:?}");
        // and different words do not
        assert_ne!(stems("промпт")[0], stems("пайплайн")[0]);
    }

    #[test]
    fn english_forms_share_a_stem() {
        let s = stems("prompt prompts prompted prompting");
        assert!(s.iter().all(|x| *x == s[0]), "{s:?}");
    }

    #[test]
    fn claude_with_a_russian_ending_is_claude() {
        assert_eq!(forms("Claude Claude'а Claude’ом"), vec!["claude"; 3]);
        let s = stems("Claude Claude'а");
        assert_eq!(s[0], s[1]);
        // English contractions stay whole, possessives fold
        assert_eq!(forms("don't"), vec!["don't"]);
        assert_eq!(forms("Claude's"), vec!["claude"]);
    }

    #[test]
    fn segments_break_at_sentence_ends_and_numbers() {
        let a = analyze("Сделай так, чтобы работало. Потом проверь 5 раз");
        let seg: Vec<u32> = a.tokens.iter().map(|t| t.segment).collect();
        // сделай так чтобы работало | потом проверь | раз (after the number)
        assert_eq!(seg, vec![0, 0, 0, 0, 1, 1, 2]);
        let trigrams: Vec<String> = a
            .windows(3)
            .map(|w| {
                w.iter()
                    .map(|t| t.form.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        assert_eq!(trigrams, vec!["сделай так чтобы", "так чтобы работало"]);
    }

    #[test]
    fn openers_skip_leading_punctuation() {
        let a = analyze("— Короче, делаем так");
        assert_eq!(
            a.openers(),
            (
                Some("короче".to_string()),
                Some("короче делаем".to_string())
            )
        );
        let one = analyze("...Да.");
        assert_eq!(one.openers(), (Some("да".to_string()), None));
        assert_eq!(analyze("— ...").openers(), (None, None));
    }

    #[test]
    fn token_spans_point_into_the_original_text() {
        let text = "Привет, Claude'а и «Obsidian»";
        for t in analyze(text).tokens {
            let original = &text[t.start..t.end];
            assert_eq!(normalize_form(original), t.form);
        }
    }
}
