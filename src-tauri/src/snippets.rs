//! Literal spoken-phrase expansion. Replacement text is never scanned again.

/// The same normalization is used for validation and matching.
pub fn normalize_phrase(phrase: &str) -> String {
    phrase
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold_sigma)
        .collect()
}

// Lowercasing a whole string can choose final sigma contextually, while the
// offset-preserving matcher processes scalar values. Treat both forms alike.
fn fold_sigma(c: char) -> char {
    if c == 'ς' {
        'σ'
    } else {
        c
    }
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric()
        || c == '_'
        || matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

/// The runtime switch is checked before doing any matching work.
pub fn apply_snippets<'a>(
    text: &str,
    enabled: bool,
    rules: impl IntoIterator<Item = (&'a str, &'a str, bool)>,
) -> String {
    if !enabled {
        return text.to_owned();
    }
    expand_snippets(
        text,
        rules
            .into_iter()
            .filter(|(_, _, enabled)| *enabled)
            .map(|(phrase, replacement, _)| (phrase, replacement)),
    )
}

/// Expand whole phrases, ignoring case and whitespace differences. At each
/// position the longest matching phrase wins; punctuation outside it survives.
pub fn expand_snippets<'a>(
    text: &str,
    rules: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut rules: Vec<_> = rules
        .into_iter()
        .map(|(phrase, replacement)| (normalize_phrase(phrase), replacement))
        .filter(|(phrase, _)| !phrase.is_empty())
        .collect();
    rules.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    if rules.is_empty() {
        return text.to_owned();
    }

    // A standalone trigger inserts the saved value exactly. Speech models add
    // sentence punctuation even for an email field; do not append it to the
    // saved address or duplicate punctuation already in a signature.
    let utterance = normalize_phrase(text);
    for (phrase, replacement) in &rules {
        if utterance.strip_prefix(phrase.as_str()).is_some_and(|tail| {
            tail.chars().all(|c| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '.' | ',' | '!' | '?' | ':' | ';' | '…' | '。' | '！' | '？'
                    )
            })
        }) {
            return (*replacement).to_owned();
        }
    }

    let chars: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let mut matched = None;
        for (phrase, replacement) in &rules {
            if i > 0 && word_char(chars[i - 1]) {
                continue;
            }
            let mut cursor = i;
            let mut valid = true;
            let mut expected_chars = phrase.chars();
            while let Some(expected) = expected_chars.next() {
                if expected == ' ' {
                    if cursor >= chars.len() || !chars[cursor].is_whitespace() {
                        valid = false;
                        break;
                    }
                    while cursor < chars.len() && chars[cursor].is_whitespace() {
                        cursor += 1;
                    }
                } else {
                    // Compare complete lowercase expansions so Unicode case mappings
                    // never invalidate the original UTF-8 byte offsets.
                    if cursor >= chars.len() {
                        valid = false;
                        break;
                    }
                    let mut lowered = chars[cursor].to_lowercase().map(fold_sigma);
                    if lowered.next() != Some(expected) {
                        valid = false;
                        break;
                    }
                    for extra in lowered {
                        if expected_chars.next() != Some(extra) {
                            valid = false;
                            break;
                        }
                    }
                    if !valid {
                        break;
                    }
                    cursor += 1;
                }
            }
            if valid && cursor < chars.len() && word_char(chars[cursor]) {
                valid = false;
            }
            if valid {
                matched = Some((cursor, *replacement));
                break;
            }
        }
        if let Some((end, replacement)) = matched {
            output.push_str(replacement);
            i = end;
        } else {
            output.push(chars[i]);
            i += 1;
        }
    }
    output
}

/// Validate the full list before saving, including disabled entries.
pub fn validate_snippets<'a>(
    rules: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut phrases = std::collections::HashSet::new();
    for (index, (id, phrase, replacement)) in rules.into_iter().enumerate() {
        if index >= 500 {
            return Err("At most 500 snippets are supported".into());
        }
        if id.trim().is_empty() || id.chars().count() > 100 || !ids.insert(id) {
            return Err("Snippet IDs must be nonempty and unique".into());
        }
        let normalized = normalize_phrase(phrase);
        if normalized.is_empty()
            || phrase.chars().count() > 100
            || !phrase.chars().any(char::is_alphanumeric)
        {
            return Err(
                "Spoken phrases must contain a letter or number and be at most 100 characters"
                    .into(),
            );
        }
        if !phrases.insert(normalized) {
            return Err("Spoken phrases must be unique, ignoring case and whitespace".into());
        }
        if replacement.trim().is_empty() || replacement.chars().count() > 4000 {
            return Err("Replacement text must contain 1 to 4000 characters".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_in_sentence_preserves_punctuation_and_exact_replacement() {
        assert_eq!(
            expand_snippets(
                "Send it to MY work email, please.",
                [("my work email", "Alan@example.com")]
            ),
            "Send it to Alan@example.com, please."
        );
    }

    #[test]
    fn matches_whole_phrases_and_flexible_whitespace() {
        assert_eq!(
            expand_snippets(
                "email emails preemail email_address caféemail my\t work  email",
                [("email", "X"), ("my work email", "Y")]
            ),
            "X emails preemail email_address caféemail Y"
        );
    }

    #[test]
    fn longest_wins_and_expansions_are_not_recursive() {
        assert_eq!(
            expand_snippets(
                "my work email and my email",
                [
                    ("my email", "my work email"),
                    ("my work email", "contact@example.com")
                ]
            ),
            "contact@example.com and my work email"
        );
    }

    #[test]
    fn preserves_multiline_and_literal_replacement_text() {
        assert_eq!(
            expand_snippets(
                "Use my signature.",
                [("my signature", "Alan\n$1 \\ team@example.com")]
            ),
            "Use Alan\n$1 \\ team@example.com."
        );
    }

    #[test]
    fn standalone_email_ignores_automatic_sentence_punctuation() {
        let rules = [("my work email", "Alan@example.com")];
        assert_eq!(expand_snippets("My work email.", rules), "Alan@example.com");
        assert_eq!(
            expand_snippets(" MY  work email?! ", rules),
            "Alan@example.com"
        );
        assert_eq!(expand_snippets("my work email", rules), "Alan@example.com");
        assert_eq!(
            expand_snippets("Send it to my work email.", rules),
            "Send it to Alan@example.com."
        );
    }

    #[test]
    fn no_rules_or_no_match_leaves_text_unchanged() {
        assert_eq!(expand_snippets("Hello 世界!", []), "Hello 世界!");
        assert_eq!(
            expand_snippets("Hello 世界!", [("signature", "Alan")]),
            "Hello 世界!"
        );
        assert_eq!(expand_snippets("anything", [("  ", "wrong")]), "anything");
    }

    #[test]
    fn unicode_case_and_boundaries() {
        assert_eq!(
            expand_snippets(
                "ÜBER İ e\u{301}mail mail",
                [("über", "address"), ("İ", "Y"), ("mail", "X")]
            ),
            "address Y e\u{301}mail X"
        );
    }

    #[test]
    fn greek_final_sigma_matches_uppercase_and_lowercase() {
        assert_eq!(expand_snippets("ΟΣ ος οσ", [("ΟΣ", "X")]), "X X X");
        assert_eq!(normalize_phrase("ΟΣ"), normalize_phrase("ος"));
    }

    #[test]
    fn rejects_ambiguous_or_empty_rules() {
        assert!(validate_snippets([
            ("a", "my email", "a@b.com"),
            ("b", " MY  EMAIL ", "b@c.com")
        ])
        .is_err());
        assert!(validate_snippets([("a", "my email", " ")]).is_err());
        assert!(validate_snippets([("a", ".", "text")]).is_err());
        assert!(validate_snippets([("a", "my email", "A@example.com\nThanks")]).is_ok());
    }

    #[test]
    fn global_switch_and_individual_disable_preserve_original_text() {
        let rules = [
            ("my email", "a@example.com", true),
            ("signature", "Alan", false),
        ];
        assert_eq!(
            apply_snippets("my email signature", false, rules),
            "my email signature"
        );
        assert_eq!(
            apply_snippets("my email signature", true, rules),
            "a@example.com signature"
        );
    }
}
