//! Engine-agnostic text clean-up for Singlish / Chinglish dictation.
//!
//! No Tauri dependency: the app calls [`apply`] after transcription and before
//! paste, and the `bakeoff` binary calls it to score engines.

pub mod audio;
pub mod mer;
pub mod report;

use serde::{Deserialize, Serialize};

/// Vocabulary shipped in the binary; copied to the app data dir on first run.
pub const DEFAULT_VOCABULARY_JSON: &str = include_str!("default_vocabulary.json");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Replacement {
    pub from: Vec<String>,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vocabulary {
    #[serde(default)]
    pub replacements: Vec<Replacement>,
    #[serde(default)]
    pub protected_terms: Vec<String>,
}

impl Vocabulary {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The bundled default vocabulary.
    pub fn bundled() -> Self {
        Self::from_json(DEFAULT_VOCABULARY_JSON).expect("bundled vocabulary is valid JSON")
    }
}

/// One rule compiled for matching: lowercase words to match, and the output.
struct Rule {
    words: Vec<String>,
    to: String,
    /// Protected terms always apply; replacements skip all-caps acronyms.
    protected: bool,
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x31F0..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF | 0x20000..=0x2FA1F)
}

/// Characters that make up a word. CJK characters are never word characters,
/// so Latin words next to Chinese still match but Chinese is never touched.
fn is_word_char(c: char) -> bool {
    !is_cjk(c) && (c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '_')
}

/// Byte ranges of word runs in `text`.
fn tokenize(text: &str) -> Vec<(usize, usize)> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if is_word_char(c) {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            tokens.push((s, i));
        }
    }
    if let Some(s) = start {
        tokens.push((s, text.len()));
    }
    tokens
}

fn compile(vocab: &Vocabulary) -> Vec<Rule> {
    let split = |phrase: &str| -> Vec<String> {
        phrase
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect()
    };
    let mut rules = Vec::new();
    for term in &vocab.protected_terms {
        let words = split(term);
        if !words.is_empty() {
            rules.push(Rule {
                words,
                to: term.trim().to_string(),
                protected: true,
            });
        }
    }
    for r in &vocab.replacements {
        for from in &r.from {
            // Hyphenated mishearings ("pie-seh") match as separate words.
            let words = split(&from.replace('-', " "));
            if !words.is_empty() && !r.to.is_empty() {
                rules.push(Rule {
                    words,
                    to: r.to.clone(),
                    protected: false,
                });
            }
        }
    }
    // Longest phrase first; protected terms win ties.
    rules.sort_by(|a, b| {
        b.words
            .len()
            .cmp(&a.words.len())
            .then(b.protected.cmp(&a.protected))
    });
    rules
}

fn is_acronym(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// Carries the matched text's capitalisation over to a lowercase replacement.
fn match_case(original: &str, to: &str) -> String {
    let first_upper = original.chars().next().is_some_and(|c| c.is_uppercase());
    if !first_upper {
        return to.to_string();
    }
    let mut chars = to.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Applies protected-term casing and replacements. Case-insensitive,
/// whole-word, never touches Chinese characters.
pub fn apply(text: &str, vocab: &Vocabulary) -> String {
    if vocab.replacements.is_empty() && vocab.protected_terms.is_empty() {
        return text.to_string();
    }
    let rules = compile(vocab);
    let tokens = tokenize(text);
    let lowered: Vec<String> = tokens
        .iter()
        .map(|&(s, e)| text[s..e].to_lowercase())
        .collect();

    let mut out = String::with_capacity(text.len());
    let mut copied = 0; // bytes of `text` already emitted
    let mut i = 0;
    while i < tokens.len() {
        let mut matched = None;
        for rule in &rules {
            let n = rule.words.len();
            if i + n > tokens.len() {
                continue;
            }
            if !(0..n).all(|k| lowered[i + k] == rule.words[k]) {
                continue;
            }
            // Words of a phrase must be separated by plain whitespace only.
            let spaced = (0..n - 1).all(|k| {
                text[tokens[i + k].1..tokens[i + k + 1].0]
                    .chars()
                    .all(|c| c.is_whitespace())
            });
            if !spaced {
                continue;
            }
            let (s, e) = (tokens[i].0, tokens[i + n - 1].1);
            if !rule.protected && is_acronym(&text[s..e]) {
                continue;
            }
            matched = Some((n, s, e, rule));
            break;
        }
        match matched {
            Some((n, s, e, rule)) => {
                out.push_str(&text[copied..s]);
                if rule.protected {
                    out.push_str(&rule.to);
                } else {
                    out.push_str(&match_case(&text[s..e], &rule.to));
                }
                copied = e;
                i += n;
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Removes SenseVoice-style control tags such as `<|zh|>`, `<|HAPPY|>`,
/// `<|Speech|>`, `<|withitn|>`. transcribe-cpp already drops these; this is a
/// cheap safety net so a tag can never be pasted.
pub fn strip_asr_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<|") {
        match rest[start + 2..].find("|>") {
            Some(len) if len <= 32 && !rest[start + 2..start + 2 + len].contains('\n') => {
                out.push_str(&rest[..start]);
                rest = &rest[start + 2 + len + 2..];
            }
            _ => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    // Collapse the double space a removed tag can leave behind.
    let mut cleaned = String::with_capacity(out.len());
    for c in out.chars() {
        if c == ' ' && cleaned.ends_with(' ') {
            continue;
        }
        cleaned.push(c);
    }
    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> Vocabulary {
        Vocabulary::bundled()
    }

    #[test]
    fn bundled_parses_and_has_all_seed_words() {
        let v = v();
        for to in [
            "lah",
            "leh",
            "lor",
            "meh",
            "sia",
            "hor",
            "liao",
            "shiok",
            "sian",
            "paiseh",
            "kiasu",
            "kiasi",
            "alamak",
            "jialat",
            "makan",
            "tapau",
            "chope",
            "bojio",
            "steady",
            "walao",
            "can or not",
        ] {
            assert!(v.replacements.iter().any(|r| r.to == to), "missing {to}");
        }
    }

    #[test]
    fn basic_particle() {
        assert_eq!(apply("ok la see you", &v()), "ok lah see you");
        assert_eq!(apply("ok lar", &v()), "ok lah");
    }

    #[test]
    fn case_insensitive_keeps_leading_capital() {
        assert_eq!(apply("La, I know", &v()), "Lah, I know");
        assert_eq!(apply("LAR", &v()), "LAR"); // acronym-looking: untouched
    }

    #[test]
    fn whole_word_only() {
        assert_eq!(apply("the lamp is large", &v()), "the lamp is large");
        assert_eq!(apply("Atlanta lark lager", &v()), "Atlanta lark lager");
        assert_eq!(apply("lahore plaza", &v()), "lahore plaza");
    }

    #[test]
    fn acronyms_untouched() {
        assert_eq!(apply("I live in LA now", &v()), "I live in LA now");
    }

    #[test]
    fn common_english_words_not_mapped() {
        for s in [
            "low prices",
            "may I go",
            "she shook it",
            "chop the onions",
            "sir yes sir",
            "lore of the land",
        ] {
            assert_eq!(apply(s, &v()), s);
        }
    }

    #[test]
    fn multiword_phrase() {
        assert_eq!(apply("wah lao so jia lat", &v()), "walao so jialat");
        assert_eq!(apply("Pai seh ah", &v()), "Paiseh ah");
        assert_eq!(apply("can ornot?", &v()), "can or not?");
    }

    #[test]
    fn phrase_needs_whitespace_between_words() {
        assert_eq!(apply("wah, lao", &v()), "wah, lao");
        assert_eq!(apply("wah  lao", &v()), "walao"); // extra spaces ok
    }

    #[test]
    fn mixed_chinese_english() {
        assert_eq!(apply("我们去makan la", &v()), "我们去makan lah");
        assert_eq!(apply("好好la我很shiock", &v()), "好好lah我很shiok");
    }

    #[test]
    fn chinese_untouched() {
        let s = "这个很好吃，我们去吃饭吧";
        assert_eq!(apply(s, &v()), s);
    }

    #[test]
    fn protected_terms_fix_casing() {
        assert_eq!(
            apply("funding societies is hiring", &v()),
            "Funding Societies is hiring"
        );
        assert_eq!(apply("WORKSPLICE rocks", &v()), "Worksplice rocks");
        assert_eq!(apply("take the mrt to hdb", &v()), "take the MRT to HDB");
    }

    #[test]
    fn protected_terms_whole_word_only() {
        assert_eq!(apply("cpfs and mrts", &v()), "cpfs and mrts");
    }

    #[test]
    fn protected_term_shields_inside_replacement() {
        let vocab = Vocabulary {
            replacements: vec![Replacement {
                from: vec!["sian".into()],
                to: "x".into(),
            }],
            protected_terms: vec!["Sian Lee".into()],
        };
        assert_eq!(apply("sian lee said sian", &vocab), "Sian Lee said x");
    }

    #[test]
    fn punctuation_and_surroundings_preserved() {
        assert_eq!(apply("\"la!\" (lar) ok…", &v()), "\"lah!\" (lah) ok…");
        assert_eq!(apply("line1\nla\nline3", &v()), "line1\nlah\nline3");
    }

    #[test]
    fn apostrophe_words_not_split() {
        assert_eq!(apply("don't la's", &v()), "don't la's");
    }

    #[test]
    fn empty_and_noop() {
        assert_eq!(apply("", &v()), "");
        assert_eq!(apply("hello world", &Vocabulary::default()), "hello world");
    }

    #[test]
    fn user_json_roundtrip() {
        let json = r#"{"replacements":[{"from":["foo"],"to":"bar"}]}"#;
        let vocab = Vocabulary::from_json(json).unwrap();
        assert_eq!(apply("Foo foo", &vocab), "Bar bar");
    }

    #[test]
    fn strips_sensevoice_tags() {
        assert_eq!(
            strip_asr_tags("<|zh|><|HAPPY|><|Speech|><|withitn|>你好 world"),
            "你好 world"
        );
        assert_eq!(strip_asr_tags("hello <|en|> world"), "hello world");
    }

    #[test]
    fn strip_leaves_non_tags() {
        assert_eq!(strip_asr_tags("a <| b"), "a <| b");
        assert_eq!(strip_asr_tags("x < y |> z"), "x < y |> z");
    }
}
