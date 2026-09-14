//! Deterministic final-output replacement rules (fork feature).
//!
//! Deliberately separate from the fuzzy custom-word matcher in
//! [`crate::audio_toolkit::text`]: custom words influence *recognition*, while
//! these rules are applied exactly once to the final transcript — matched
//! case-insensitively, on word boundaries, longest rule first, never cascading.
//!
//! The function is pure, deterministic and side-effect free so it can be unit
//! tested in isolation and can fail open (returning the input unchanged) if the
//! rule list is empty or contains nothing usable.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A single "heard ⇒ emitted" rule. `from` is matched case-insensitively on
/// word boundaries; `to` is emitted verbatim (casing, punctuation and Unicode
/// are preserved exactly as configured).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DictionaryReplacement {
    pub from: String,
    pub to: String,
}

/// Normalize the `from` side of a rule: trim the ends and collapse internal
/// whitespace runs (accidental doubled spaces / newlines) to single spaces.
/// Does not touch `to`.
pub fn normalize_replacement_from(from: &str) -> String {
    from.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Clean a rule list coming from the UI before it is persisted: trim and
/// collapse the `from` side, trim the `to` side, drop rules that are empty on
/// either side, and drop case-insensitive duplicate `from` values (the first
/// occurrence wins). The returned list is exactly what should be stored.
pub fn normalize_dictionary_replacements(
    replacements: Vec<DictionaryReplacement>,
) -> Vec<DictionaryReplacement> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::with_capacity(replacements.len());
    for rule in replacements {
        let from = normalize_replacement_from(&rule.from);
        let to = rule.to.trim().to_string();
        if from.is_empty() || to.is_empty() {
            continue;
        }
        if !seen.insert(from.to_lowercase()) {
            continue;
        }
        normalized.push(DictionaryReplacement { from, to });
    }
    normalized
}

struct CompiledRule<'a> {
    /// Whitespace-separated parts of the normalized source, as chars.
    parts: Vec<Vec<char>>,
    /// Exact replacement text.
    to: &'a str,
    /// Length of the normalized source in chars (longest-match ordering).
    len: usize,
}

/// Case-insensitive single-char comparison (approximate Unicode case folding;
/// conservative when a char lowercases to multiple chars).
fn char_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Characters that make up a "word" for boundary purposes. Mirrors the usual
/// regex `\w` notion (letters, digits, underscore, Unicode-aware).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn compile_rules<'a>(replacements: &'a [DictionaryReplacement]) -> Vec<CompiledRule<'a>> {
    let mut rules = Vec::new();
    for rule in replacements {
        let from = normalize_replacement_from(&rule.from);
        if from.is_empty() || rule.to.trim().is_empty() {
            continue;
        }
        let parts: Vec<Vec<char>> = from.split(' ').map(|part| part.chars().collect()).collect();
        rules.push(CompiledRule {
            len: from.chars().count(),
            parts,
            to: &rule.to,
        });
    }
    // Most specific rule first: longer sources win on overlapping matches.
    // sort_by is stable, so equally long rules keep their configured order.
    rules.sort_by(|a, b| b.len.cmp(&a.len));
    rules
}

/// Try to match `rule` in `text` starting exactly at byte index `start`
/// (which must be a char boundary and the start of a word). Returns the
/// exclusive end byte index of the matched span.
fn match_rule(rule: &CompiledRule<'_>, text: &str, start: usize) -> Option<usize> {
    // Word-boundary guard before the match.
    if start > 0 {
        if let Some(prev) = text[..start].chars().next_back() {
            if is_word_char(prev) {
                return None;
            }
        }
    }

    let mut pos = start;
    for (part_index, part) in rule.parts.iter().enumerate() {
        if part_index > 0 {
            // A whitespace run separates rule parts; any run length matches
            // (normalization collapsed accidental whitespace in `from`).
            let mut skipped = false;
            while let Some(c) = text[pos..].chars().next() {
                if c.is_whitespace() {
                    pos += c.len_utf8();
                    skipped = true;
                } else {
                    break;
                }
            }
            if !skipped {
                return None;
            }
        }
        for expected in part {
            match text[pos..].chars().next() {
                Some(actual) if char_eq_ci(actual, *expected) => pos += actual.len_utf8(),
                _ => return None,
            }
        }
    }

    // Word-boundary guard after the match.
    if let Some(next) = text[pos..].chars().next() {
        if is_word_char(next) {
            return None;
        }
    }

    Some(pos)
}

/// Apply replacement rules to `text` as a single conceptual pass.
///
/// Semantics:
/// - case-insensitive source matching; the destination is emitted verbatim;
/// - matches only on word boundaries (never inside a larger word);
/// - one rule per position, longest source first (most specific wins);
/// - no cascading: text emitted by a rule is never re-examined by any rule;
/// - everything outside matched spans (whitespace, punctuation, Unicode) is
///   copied through untouched.
pub fn apply_dictionary_replacements(text: &str, replacements: &[DictionaryReplacement]) -> String {
    if text.is_empty() || replacements.is_empty() {
        return text.to_string();
    }
    let rules = compile_rules(replacements);
    if rules.is_empty() {
        return text.to_string();
    }

    // Bucket rules by their first character (case-folded) so most scan
    // positions only test rules that can actually start there.
    let mut buckets: HashMap<char, Vec<usize>> = HashMap::new();
    for (index, rule) in rules.iter().enumerate() {
        if let Some(first) = rule.parts.first().and_then(|part| part.first()) {
            let key = first.to_lowercase().next().unwrap_or(*first);
            buckets.entry(key).or_default().push(index);
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut search = 0usize;
    while search < text.len() {
        // `search` is always on a char boundary.
        let c = text[search..].chars().next().unwrap();
        let key = c.to_lowercase().next().unwrap_or(c);

        let mut matched: Option<(usize, &str)> = None;
        if let Some(indices) = buckets.get(&key) {
            for &index in indices {
                if let Some(end) = match_rule(&rules[index], text, search) {
                    matched = Some((end, rules[index].to));
                    break;
                }
            }
        }

        match matched {
            Some((end, to)) => {
                out.push_str(&text[cursor..search]);
                out.push_str(to);
                cursor = end;
                search = end;
            }
            None => search += c.len_utf8(),
        }
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(from: &str, to: &str) -> DictionaryReplacement {
        DictionaryReplacement {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    fn apply(text: &str, rules: &[DictionaryReplacement]) -> String {
        apply_dictionary_replacements(text, rules)
    }

    #[test]
    fn basic_replacement() {
        assert_eq!(
            apply(
                "I use open router daily",
                &[rule("open router", "OpenRouter")]
            ),
            "I use OpenRouter daily"
        );
    }

    #[test]
    fn source_match_is_case_insensitive() {
        let rules = [rule("open router", "OpenRouter")];
        assert_eq!(apply("Open Router", &rules), "OpenRouter");
        assert_eq!(apply("OPEN ROUTER", &rules), "OpenRouter");
        assert_eq!(apply("open router", &rules), "OpenRouter");
    }

    #[test]
    fn multi_word_phrase() {
        assert_eq!(
            apply(
                "check dark room edits later",
                &[rule("dark room edits", "Darkroom Edits")]
            ),
            "check Darkroom Edits later"
        );
    }

    #[test]
    fn output_casing_is_exact() {
        let rules = [rule("think pad", "ThinkPad")];
        assert_eq!(apply("think pad", &rules), "ThinkPad");
        assert_eq!(apply("Think Pad", &rules), "ThinkPad");
        assert_eq!(apply("THINK PAD", &rules), "ThinkPad");
    }

    #[test]
    fn punctuation_around_match_is_preserved() {
        let rules = [rule("open router", "OpenRouter")];
        assert_eq!(
            apply("I use open router, sometimes.", &rules),
            "I use OpenRouter, sometimes."
        );
        assert_eq!(apply("(open router)", &rules), "(OpenRouter)");
        assert_eq!(apply("\"open router\"", &rules), "\"OpenRouter\"");
    }

    #[test]
    fn no_replacement_inside_larger_words() {
        let rules = [rule("cat", "CAT")];
        assert_eq!(apply("the cat sat down", &rules), "the CAT sat down");
        assert_eq!(apply("concatenate", &rules), "concatenate");
        assert_eq!(apply("bobcat and catfish", &rules), "bobcat and catfish");
    }

    #[test]
    fn multi_word_rule_does_not_match_glued_text() {
        let rules = [rule("open router", "OpenRouter")];
        assert_eq!(apply("openrouter", &rules), "openrouter");
        assert_eq!(apply("open routers", &rules), "open routers");
    }

    #[test]
    fn multiple_occurrences_are_all_replaced() {
        assert_eq!(
            apply(
                "open router and open router",
                &[rule("open router", "OpenRouter")]
            ),
            "OpenRouter and OpenRouter"
        );
    }

    #[test]
    fn longest_rule_wins_on_overlap() {
        let rules = [
            rule("open router", "OpenRouter"),
            rule("open router api", "OpenRouter API"),
        ];
        assert_eq!(apply("open router api key", &rules), "OpenRouter API key");
        // Reverse configuration order must behave identically.
        let reversed = [
            rule("open router api", "OpenRouter API"),
            rule("open router", "OpenRouter"),
        ];
        assert_eq!(
            apply("open router api key", &reversed),
            "OpenRouter API key"
        );
        assert_eq!(apply("open router key", &rules), "OpenRouter key");
    }

    #[test]
    fn replacements_do_not_cascade() {
        let rules = [rule("A", "B"), rule("B", "C")];
        assert_eq!(apply("A", &rules), "B");
        assert_eq!(apply("B", &rules), "C");
        assert_eq!(apply("A B", &rules), "B C");
    }

    #[test]
    fn unicode_is_safe() {
        let rules = [rule("שלום", "עולם"), rule("open router", "OpenRouter")];
        assert_eq!(apply("שלום לכולם", &rules), "עולם לכולם");
        assert_eq!(
            apply("emoji 🚀 open router ✨", &rules),
            "emoji 🚀 OpenRouter ✨"
        );
        // Grapheme-adjacent unicode must not panic or corrupt UTF-8.
        assert_eq!(apply("café", &rules), "café");
    }

    #[test]
    fn empty_rule_list_returns_input_unchanged() {
        assert_eq!(apply("open router", &[]), "open router");
        assert_eq!(apply("", &[rule("a", "b")]), "");
    }

    #[test]
    fn whitespace_around_text_is_not_normalized() {
        let rules = [rule("open router", "OpenRouter")];
        assert_eq!(apply("  open router  ", &rules), "  OpenRouter  ");
        assert_eq!(apply("\topen router\n", &rules), "\tOpenRouter\n");
    }

    #[test]
    fn whitespace_inside_rule_matches_any_run() {
        let rules = [rule("dark   room edits", "Darkroom Edits")];
        assert_eq!(apply("dark room edits", &rules), "Darkroom Edits");
        assert_eq!(apply("dark\nroom   edits", &rules), "Darkroom Edits");
    }

    #[test]
    fn rules_with_punctuation_match_literally() {
        let rules = [rule("make.com", "Make.com"), rule("C++", "C plus plus")];
        assert_eq!(apply("visit make.com now", &rules), "visit Make.com now");
        assert_eq!(apply("visit make com now", &rules), "visit make com now");
        assert_eq!(
            apply("I code in C++ daily", &rules),
            "I code in C plus plus daily"
        );
        // A lone "c" must not trigger the C++ rule.
        assert_eq!(apply("a b c d", &rules), "a b c d");
    }

    #[test]
    fn invalid_rules_are_ignored() {
        let rules = [
            rule("", "Something"),
            rule("   ", "Something"),
            rule("something", ""),
        ];
        assert_eq!(apply("nothing to see", &rules), "nothing to see");
    }

    #[test]
    fn numbers_and_underscores_are_word_chars() {
        let rules = [rule("n eight n", "n8n")];
        assert_eq!(apply("use n eight n please", &rules), "use n8n please");
        let digits = [rule("404", "not found")];
        assert_eq!(apply("error 404 page", &digits), "error not found page");
        assert_eq!(apply("error 4040 page", &digits), "error 4040 page");
    }

    #[test]
    fn replacement_span_uses_original_separators_from_the_rule_match() {
        // Internal separators are part of the matched span and are replaced by
        // the destination text (plus surrounding text stays verbatim).
        let rules = [rule("pup on tech", "PuponTech")];
        assert_eq!(
            apply("welcome to pup on tech, enjoy", &rules),
            "welcome to PuponTech, enjoy"
        );
    }

    // ---- normalize_dictionary_replacements ---------------------------------

    #[test]
    fn normalize_trims_and_collapses_replacement_fields() {
        let rules =
            normalize_dictionary_replacements(vec![rule("  open   router ", "  OpenRouter  ")]);
        assert_eq!(rules, vec![rule("open router", "OpenRouter")]);
    }

    #[test]
    fn normalize_drops_empty_and_duplicate_rules() {
        let rules = normalize_dictionary_replacements(vec![
            rule("", "x"),
            rule("open router", ""),
            rule(" open router ", "OpenRouter"),
            rule("OPEN ROUTER", "Other"),
            rule("think pad", "ThinkPad"),
        ]);
        // Empty sides are dropped; the first case-insensitive `from` wins.
        assert_eq!(
            rules,
            vec![
                rule("open router", "OpenRouter"),
                rule("think pad", "ThinkPad")
            ]
        );
    }

    #[test]
    fn normalize_keeps_destination_casing_and_punctuation() {
        let rules = normalize_dictionary_replacements(vec![rule("c j studio", "CJ's Studio")]);
        assert_eq!(rules, vec![rule("c j studio", "CJ's Studio")]);
    }

    // ---- persisted settings compatibility (see crate::settings) ------------

    #[test]
    fn stored_settings_without_replacements_load_as_empty() {
        // A store written by upstream Handy has `custom_words` but no
        // `dictionary_replacements` key; loading it must not fail or reset it.
        let mut value =
            serde_json::to_value(crate::settings::AppSettings::default()).expect("serialize");
        let map = value.as_object_mut().expect("settings object");
        map.remove("dictionary_replacements");
        map.insert(
            "custom_words".to_string(),
            serde_json::json!(["OpenRouter", "ThinkPad"]),
        );

        let loaded: crate::settings::AppSettings =
            serde_json::from_value(value).expect("old store loads");
        assert!(loaded.dictionary_replacements.is_empty());
        assert_eq!(
            loaded.custom_words,
            vec!["OpenRouter".to_string(), "ThinkPad".to_string()]
        );
    }

    #[test]
    fn populated_replacements_roundtrip_without_touching_other_fields() {
        let mut settings = crate::settings::AppSettings::default();
        settings.custom_words = vec!["OpenRouter".to_string()];
        settings.dictionary_replacements = vec![
            rule("open router", "OpenRouter"),
            rule("n eight n", "n8n"),
            rule("שלום", "שלומות"),
        ];

        let json = serde_json::to_string(&settings).expect("serialize");
        let loaded: crate::settings::AppSettings =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(
            loaded.dictionary_replacements,
            settings.dictionary_replacements
        );
        assert_eq!(loaded.custom_words, settings.custom_words);
        assert_eq!(loaded.selected_model, settings.selected_model);
    }

    #[test]
    fn default_settings_serialize_with_empty_replacements() {
        let json =
            serde_json::to_string(&crate::settings::AppSettings::default()).expect("serialize");
        let loaded: crate::settings::AppSettings =
            serde_json::from_str(&json).expect("deserialize");
        assert!(loaded.dictionary_replacements.is_empty());
    }
}
