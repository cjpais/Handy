//! Word tokenization and script classification for dictation statistics.
//!
//! A token is a run of letters and digits. A few characters join two
//! alphanumerics into one token instead of splitting them, so that `GPT-4o`,
//! `e-mail`, `3.11`, `3,14` and `Claude'а` each count as one word. Everything
//! else (spaces, punctuation, dashes, symbols) separates tokens; a chunk with
//! no letter or digit at all (`—`, `...`) is not a word.

/// Script class of a token. Exactly one applies, in this order of priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// Contains at least one ASCII letter (`RAG-пайплайн` is Latin).
    Latin,
    /// No ASCII letter, at least one Cyrillic letter.
    Cyrillic,
    /// Numbers and every other script.
    Other,
}

/// Word totals of one text. `latin + cyrillic + other == words`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WordCounts {
    pub words: u32,
    pub latin: u32,
    pub cyrillic: u32,
}

impl WordCounts {
    /// Tokens that are neither Latin nor Cyrillic (numbers, other scripts).
    #[cfg(test)]
    pub fn other(&self) -> u32 {
        self.words - self.latin - self.cyrillic
    }
}

fn is_combining_mark(c: char) -> bool {
    // Combining Diacritical Marks: a decomposed `ё` is `е` + U+0308.
    ('\u{0300}'..='\u{036F}').contains(&c)
}

fn is_cyrillic(c: char) -> bool {
    // Cyrillic + Cyrillic Supplement
    ('\u{0400}'..='\u{052F}').contains(&c)
}

/// Whether `joiner`, sitting between `prev` and `next`, glues them together.
fn joins(joiner: char, prev: char, next: char) -> bool {
    let prev_alnum = prev.is_alphanumeric() || is_combining_mark(prev);
    let next_alnum = next.is_alphanumeric();
    if !prev_alnum || !next_alnum {
        return false;
    }
    match joiner {
        // Hyphens, apostrophes and underscores inside a word.
        '-' | '\u{2010}' | '\u{2011}' | '\'' | '\u{2019}' | '_' => true,
        // Dots join letters or digits (`node.js`, `3.11`, `v1.2.3`).
        '.' => true,
        // A comma only joins digits (`3,14`); between letters it separates.
        ',' => prev.is_numeric() && next.is_numeric(),
        _ => false,
    }
}

/// Splits `text` into word tokens.
pub fn tokens(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut end = 0usize;

    for (i, &(byte_idx, c)) in chars.iter().enumerate() {
        // Letters and digits are always part of a token. Inside one, a combining
        // mark continues it and a joiner continues it when both neighbours are
        // alphanumeric.
        let in_token = c.is_alphanumeric()
            || (start.is_some()
                && (is_combining_mark(c)
                    || chars
                        .get(i + 1)
                        .is_some_and(|&(_, next)| joins(c, chars[i - 1].1, next))));

        if in_token {
            if start.is_none() {
                start = Some(byte_idx);
            }
            end = byte_idx + c.len_utf8();
        } else if let Some(s) = start.take() {
            out.push(&text[s..end]);
        }
    }
    if let Some(s) = start {
        out.push(&text[s..end]);
    }
    out
}

/// Script class of a single token.
pub fn classify(token: &str) -> Script {
    if token.chars().any(|c| c.is_ascii_alphabetic()) {
        Script::Latin
    } else if token.chars().any(is_cyrillic) {
        Script::Cyrillic
    } else {
        Script::Other
    }
}

/// Word, Latin-word and Cyrillic-word totals of `text`.
pub fn count_words(text: &str) -> WordCounts {
    let mut counts = WordCounts::default();
    for token in tokens(text) {
        counts.words += 1;
        match classify(token) {
            Script::Latin => counts.latin += 1,
            Script::Cyrillic => counts.cyrillic += 1,
            Script::Other => {}
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<&str> {
        tokens(text)
    }

    #[test]
    fn model_names_with_digits_and_hyphen_are_one_latin_word() {
        assert_eq!(toks("GPT-4o"), vec!["GPT-4o"]);
        assert_eq!(classify("GPT-4o"), Script::Latin);
        assert_eq!(count_words("Я использую GPT-4o каждый день").words, 5);
    }

    #[test]
    fn yo_is_cyrillic_in_composed_and_decomposed_form() {
        assert_eq!(toks("ёлка"), vec!["ёлка"]);
        assert_eq!(classify("ёлка"), Script::Cyrillic);
        // е + U+0308 COMBINING DIAERESIS
        let decomposed = "е\u{0308}лка";
        assert_eq!(toks(decomposed), vec![decomposed]);
        assert_eq!(classify(decomposed), Script::Cyrillic);
    }

    #[test]
    fn hyphenated_latin_word_is_one_token() {
        assert_eq!(toks("e-mail"), vec!["e-mail"]);
        assert_eq!(classify("e-mail"), Script::Latin);
    }

    #[test]
    fn version_numbers_are_one_numeric_token_in_neither_script() {
        assert_eq!(toks("Python 3.11"), vec!["Python", "3.11"]);
        assert_eq!(classify("3.11"), Script::Other);
        let counts = count_words("Python 3.11");
        assert_eq!((counts.words, counts.latin, counts.cyrillic), (2, 1, 0));
        assert_eq!(counts.other(), 1);
        // Decimal comma
        assert_eq!(toks("число 3,14"), vec!["число", "3,14"]);
    }

    #[test]
    fn comma_between_letters_separates_but_between_digits_joins() {
        assert_eq!(toks("кошка,собака"), vec!["кошка", "собака"]);
        assert_eq!(toks("1,5 и 2,5"), vec!["1,5", "и", "2,5"]);
    }

    #[test]
    fn latin_stem_with_cyrillic_ending_is_one_latin_token() {
        assert_eq!(toks("Claude'а"), vec!["Claude'а"]);
        assert_eq!(classify("Claude'а"), Script::Latin);
        // typographic apostrophe
        assert_eq!(toks("Claude’а"), vec!["Claude’а"]);
        assert_eq!(classify("Claude’а"), Script::Latin);
    }

    #[test]
    fn mixed_alphabet_words_are_latin_when_they_contain_any_ascii_letter() {
        assert_eq!(classify("RAG-пайплайн"), Script::Latin);
        assert_eq!(classify("пайплайн"), Script::Cyrillic);
        assert_eq!(classify("Pythonовский"), Script::Latin);
        let counts = count_words("Настроил RAG-пайплайн на LangChain");
        assert_eq!((counts.words, counts.latin, counts.cyrillic), (4, 2, 2));
        assert_eq!(
            counts.latin + counts.cyrillic + counts.other(),
            counts.words
        );
    }

    #[test]
    fn punctuation_only_chunks_are_not_words() {
        assert_eq!(toks("— ... ?!"), Vec::<&str>::new());
        assert_eq!(toks(""), Vec::<&str>::new());
        assert_eq!(toks("   \n\t "), Vec::<&str>::new());
        assert_eq!(
            count_words("Привет, мир! — сказал он..."),
            WordCounts {
                words: 4,
                latin: 0,
                cyrillic: 4
            }
        );
    }

    #[test]
    fn surrounding_punctuation_is_trimmed_but_dots_between_alphanumerics_join() {
        assert_eq!(toks("(Docker)"), vec!["Docker"]);
        assert_eq!(toks("«Obsidian»,"), vec!["Obsidian"]);
        assert_eq!(
            toks("node.js и example.com."),
            vec!["node.js", "и", "example.com"]
        );
        assert_eq!(toks("C++ и #тег @user"), vec!["C", "и", "тег", "user"]);
    }

    #[test]
    fn spaced_groups_and_dashes_split() {
        assert_eq!(toks("5 000"), vec!["5", "000"]);
        assert_eq!(toks("слово — слово"), vec!["слово", "слово"]);
        assert_eq!(toks("1-2-3"), vec!["1-2-3"]);
    }

    #[test]
    fn counts_partition_the_words() {
        let counts = count_words("Тест 42 test тест-драйв GPT-4o 3.11 ёлка");
        assert_eq!(counts.words, 7);
        assert_eq!(counts.latin, 2);
        assert_eq!(counts.cyrillic, 3);
        assert_eq!(counts.other(), 2);
    }
}
