//! Defaults for a fresh Voxa install (the owner's setup).

/// Registry id of Whisper large-v3-turbo, Q8_0 quantization (0.9 GB). The
/// onboarding screen puts it first; the model is never downloaded without a click.
#[cfg(test)]
pub const TURBO_Q8_MODEL_ID: &str =
    "handy-computer/whisper-large-v3-turbo-gguf/whisper-large-v3-turbo-Q8_0.gguf";

/// Language the owner dictates in.
pub const DEFAULT_LANGUAGE: &str = "ru";

/// Context sentence that shows Whisper how Russian speech with English
/// technical terms is written.
pub const DEFAULT_INITIAL_PROMPT: &str =
    "Настроил RAG-пайплайн на LangChain, промпт для Claude, заметка в Obsidian, деплой в Docker.";

/// Terms pre-filled into the dictionary.
pub const DEFAULT_CUSTOM_WORDS: &[&str] = &[
    "Claude",
    "Obsidian",
    "LangChain",
    "RAG",
    "GigaChat",
    "FastAPI",
    "Docker",
    "GitHub",
    "Python",
    "pandas",
    "Streamlit",
    "embeddings",
    "PostgreSQL",
    "Sber",
];

/// Filler words the Words tab counts out of the box. "как бы" is a phrase.
pub const DEFAULT_FILLER_WORDS: &[&str] = &[
    "типо",
    "типа",
    "короче",
    "ну",
    "как бы",
    "вот",
    "значит",
    "блин",
    "э",
    "ээ",
    "эээ",
    "м",
    "мм",
];

pub fn default_filler_words() -> Vec<String> {
    DEFAULT_FILLER_WORDS.iter().map(|w| w.to_string()).collect()
}

/// How many dictations stay in Handy's history by default. Statistics live in
/// their own store, so this only bounds the History screen and the saved audio.
pub const DEFAULT_HISTORY_LIMIT: usize = 200;

pub fn default_custom_words() -> Vec<String> {
    DEFAULT_CUSTOM_WORDS.iter().map(|w| w.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_has_the_owners_terms() {
        let words = default_custom_words();
        assert_eq!(words.len(), 14);
        for expected in [
            "Claude",
            "Obsidian",
            "LangChain",
            "RAG",
            "Sber",
            "PostgreSQL",
        ] {
            assert!(words.iter().any(|w| w == expected), "{expected}");
        }
    }

    #[test]
    fn turbo_q8_id_is_in_the_bundled_catalog() {
        let in_catalog = crate::catalog::CATALOG
            .iter()
            .any(|d| d.id == TURBO_Q8_MODEL_ID);
        assert!(in_catalog, "{TURBO_Q8_MODEL_ID} must be a catalog model id");
    }

    #[test]
    fn turbo_q8_is_the_first_recommended_model() {
        let first = crate::catalog::CATALOG
            .iter()
            .filter(|d| d.recommended)
            .min_by_key(|d| d.recommended_rank.unwrap_or(u32::MAX))
            .expect("catalog has recommended models");
        assert_eq!(first.id, TURBO_Q8_MODEL_ID);
    }

    #[test]
    fn default_prompt_mentions_the_dictionary_terms() {
        for term in ["RAG", "LangChain", "Claude", "Obsidian", "Docker"] {
            assert!(DEFAULT_INITIAL_PROMPT.contains(term), "{term}");
        }
    }
}
