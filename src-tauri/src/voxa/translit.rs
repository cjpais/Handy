//! Built-in Cyrillic spellings of technical terms.
//!
//! When the model writes `питон` instead of `Python`, that term is a good
//! candidate for the custom-words dictionary. Tokens are matched by stem plus a
//! known case ending, so `питоне`, `питона` and `питоном` all hit the same term
//! while `питонить` does not.

use super::text_stats::{classify, tokens, Script};

/// One term and the Cyrillic spellings (stems) it is commonly written as.
pub struct Term {
    /// Canonical spelling to put into the dictionary.
    pub term: &'static str,
    /// Lowercase stems, `е` instead of `ё`.
    pub stems: &'static [&'static str],
    /// Short or ambiguous stems must match the whole token (`раг`, not `рагу`).
    pub exact_only: bool,
}

const fn term(term: &'static str, stems: &'static [&'static str]) -> Term {
    Term {
        term,
        stems,
        exact_only: false,
    }
}

const fn exact(term: &'static str, stems: &'static [&'static str]) -> Term {
    Term {
        term,
        stems,
        exact_only: true,
    }
}

/// The terms from the project plan first, then more tech vocabulary.
pub static TERMS: &[Term] = &[
    term("Python", &["питон", "пайтон"]),
    term("Claude", &["клод", "клауд"]),
    term("Docker", &["докер"]),
    term("GitHub", &["гитхаб", "гитхуб"]),
    term("Obsidian", &["обсидиан"]),
    term(
        "LangChain",
        &["лэнгчейн", "ленгчейн", "лангчейн", "лэнчейн"],
    ),
    term("GigaChat", &["гигачат"]),
    term("FastAPI", &["фастапи", "фастапай"]),
    exact("RAG", &["раг"]),
    term("embeddings", &["эмбеддинг", "эмбединг"]),
    term("pandas", &["пандас"]),
    term("Streamlit", &["стримлит"]),
    term("GitLab", &["гитлаб"]),
    term("TypeScript", &["тайпскрипт"]),
    term("JavaScript", &["джаваскрипт", "джавоскрипт"]),
    term("Kubernetes", &["кубернетес", "кубернетис"]),
    term("PostgreSQL", &["постгрес", "постгрескюэл"]),
    term("Redis", &["редис"]),
    term("Kafka", &["кафка"]),
    term("React", &["реакт"]),
    term("Whisper", &["виспер", "уиспер"]),
    term("Tauri", &["таури"]),
    exact("Rust", &["раст"]),
    term("Ollama", &["олама", "оллама"]),
    term("PyTorch", &["пайторч", "питорч"]),
    term("TensorFlow", &["тензорфлоу"]),
    term("Jupyter", &["джупитер"]),
    term("Linux", &["линукс"]),
    exact("API", &["апи"]),
    term("Anthropic", &["антропик"]),
    term("Notion", &["ноушен"]),
    term("Figma", &["фигма"]),
    term("Slack", &["слэк", "слак"]),
    term("ChatGPT", &["чатгпт", "чатжпт"]),
    term("Gemini", &["джемини"]),
    term("Jira", &["джира"]),
    term("Confluence", &["конфлюенс"]),
    term("Grafana", &["графана"]),
    term("Pydantic", &["пайдантик"]),
];

/// Noun case endings accepted after a stem.
const ENDINGS: &[&str] = &[
    "", "а", "у", "ом", "ем", "е", "ы", "и", "ов", "ев", "ам", "ами", "ах", "ям", "ями", "ях",
    "ой", "ю", "я", "ию", "ии", "ием",
];

/// Lowercases and folds `ё` into `е` so spellings compare equal.
fn fold(token: &str) -> String {
    token
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| if c == 'ё' { 'е' } else { c })
        .collect()
}

/// The built-in term a Cyrillic token is a spelling of, if any.
pub fn match_token(token: &str) -> Option<&'static Term> {
    if classify(token) != Script::Cyrillic {
        return None;
    }
    let folded = fold(token);
    // Longest stem first, so a long spelling wins over a shorter overlapping one.
    let mut best: Option<(&'static Term, usize)> = None;
    for entry in TERMS {
        for stem in entry.stems {
            let stem_len = stem.chars().count();
            let Some(rest) = folded.strip_prefix(stem) else {
                continue;
            };
            let accepted = if entry.exact_only {
                rest.is_empty()
            } else {
                ENDINGS.contains(&rest)
            };
            if accepted && best.is_none_or(|(_, len)| stem_len > len) {
                best = Some((entry, stem_len));
            }
        }
    }
    best.map(|(entry, _)| entry)
}

/// Every built-in term spelled in Cyrillic in `text`, as `(term, spelling)`
/// with the spelling lowercased.
pub fn find_in_text(text: &str) -> Vec<(&'static Term, String)> {
    tokens(text)
        .into_iter()
        .filter_map(|token| match_token(token).map(|t| (t, fold(token))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term_of(token: &str) -> Option<&'static str> {
        match_token(token).map(|t| t.term)
    }

    #[test]
    fn plan_terms_match_in_the_nominative() {
        for (token, expected) in [
            ("питон", "Python"),
            ("клод", "Claude"),
            ("докер", "Docker"),
            ("гитхаб", "GitHub"),
            ("обсидиан", "Obsidian"),
            ("лэнгчейн", "LangChain"),
            ("гигачат", "GigaChat"),
            ("фастапи", "FastAPI"),
            ("раг", "RAG"),
            ("эмбеддинги", "embeddings"),
            ("пандас", "pandas"),
            ("стримлит", "Streamlit"),
        ] {
            assert_eq!(term_of(token), Some(expected), "{token}");
        }
    }

    #[test]
    fn case_endings_match_by_stem() {
        assert_eq!(term_of("питоне"), Some("Python"));
        assert_eq!(term_of("питона"), Some("Python"));
        assert_eq!(term_of("питоном"), Some("Python"));
        assert_eq!(term_of("Обсидиане"), Some("Obsidian"));
        assert_eq!(term_of("докером"), Some("Docker"));
        assert_eq!(term_of("гитхабе"), Some("GitHub"));
        assert_eq!(term_of("эмбеддингов"), Some("embeddings"));
        assert_eq!(term_of("эмбеддингами"), Some("embeddings"));
        assert_eq!(term_of("клоду"), Some("Claude"));
    }

    #[test]
    fn matching_ignores_case_and_yo() {
        assert_eq!(term_of("ПИТОН"), Some("Python"));
        assert_eq!(term_of("Питон"), Some("Python"));
        assert_eq!(term_of("пайтон"), Some("Python"));
    }

    #[test]
    fn unrelated_words_with_the_same_prefix_do_not_match() {
        assert_eq!(term_of("питонить"), None);
        assert_eq!(term_of("докерство"), None);
        assert_eq!(term_of("клодт"), None);
        assert_eq!(term_of("реактор"), None);
    }

    #[test]
    fn short_stems_match_whole_tokens_only() {
        assert_eq!(term_of("раг"), Some("RAG"));
        // `рагу` is a stew, not RAG
        assert_eq!(term_of("рагу"), None);
        assert_eq!(term_of("рага"), None);
        assert_eq!(term_of("раст"), Some("Rust"));
        assert_eq!(term_of("растение"), None);
    }

    #[test]
    fn latin_and_mixed_tokens_never_match() {
        assert_eq!(term_of("Python"), None);
        assert_eq!(term_of("питон3"), None);
        assert_eq!(term_of("RAG-пайплайн"), None);
    }

    #[test]
    fn longer_stem_wins() {
        // `постгрескюэл` also starts with the shorter stem `постгрес`
        assert_eq!(term_of("постгрескюэл"), Some("PostgreSQL"));
        assert_eq!(term_of("постгресе"), Some("PostgreSQL"));
    }

    #[test]
    fn finds_every_spelling_in_a_sentence() {
        let found = find_in_text("Написал на питоне скрипт для докера, потом в Obsidian.");
        let terms: Vec<_> = found.iter().map(|(t, s)| (t.term, s.as_str())).collect();
        assert_eq!(terms, vec![("Python", "питоне"), ("Docker", "докера")]);
    }

    #[test]
    fn every_stem_is_lowercase_cyrillic_without_yo() {
        for entry in TERMS {
            assert!(!entry.stems.is_empty(), "{}", entry.term);
            for stem in entry.stems {
                assert_eq!(*stem, fold(stem), "{stem} must be folded");
                assert!(
                    stem.chars().all(|c| ('\u{0430}'..='\u{044F}').contains(&c)),
                    "{stem} must be plain lowercase Cyrillic"
                );
            }
        }
    }
}
