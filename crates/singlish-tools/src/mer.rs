//! Mixed error rate (MER) for code-switched Chinese + English text.
//!
//! English is tokenised by whitespace into words (lower-cased, edge
//! punctuation stripped); every Chinese character counts as one token. MER is
//! the token-level Levenshtein distance divided by the reference length.

/// Ideographs, kana and hangul; punctuation blocks are deliberately excluded.
fn is_cjk_token_char(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

fn push_word(word: &mut String, tokens: &mut Vec<String>) {
    let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
    if !trimmed.is_empty() {
        tokens.push(trimmed.to_lowercase());
    }
    word.clear();
}

/// Splits mixed text into MER tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if is_cjk_token_char(c) {
            push_word(&mut word, &mut tokens);
            tokens.push(c.to_string());
        } else if c.is_whitespace() {
            push_word(&mut word, &mut tokens);
        } else {
            word.push(c);
        }
    }
    push_word(&mut word, &mut tokens);
    tokens
}

/// Token-level Levenshtein distance (substitutions, deletions, insertions).
pub fn edit_distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(x != y);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Error count and reference length for one utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub errors: usize,
    pub ref_tokens: usize,
}

impl Score {
    /// Errors / reference tokens. An empty reference is 0.0 when the
    /// hypothesis is also empty, otherwise 1.0.
    pub fn rate(&self) -> f64 {
        if self.ref_tokens == 0 {
            return if self.errors == 0 { 0.0 } else { 1.0 };
        }
        self.errors as f64 / self.ref_tokens as f64
    }
}

pub fn score(reference: &str, hypothesis: &str) -> Score {
    let r = tokenize(reference);
    let h = tokenize(hypothesis);
    Score {
        errors: edit_distance(&r, &h),
        ref_tokens: r.len(),
    }
}

/// Mixed error rate of one utterance.
pub fn mixed_error_rate(reference: &str, hypothesis: &str) -> f64 {
    score(reference, hypothesis).rate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_english_by_whitespace() {
        assert_eq!(tokenize("Hello,  World!"), vec!["hello", "world"]);
    }

    #[test]
    fn tokenizes_each_chinese_character() {
        assert_eq!(tokenize("你好吗"), vec!["你", "好", "吗"]);
    }

    #[test]
    fn tokenizes_mixed_without_spaces() {
        assert_eq!(
            tokenize("我们去makan吧"),
            vec!["我", "们", "去", "makan", "吧"]
        );
    }

    #[test]
    fn punctuation_is_ignored() {
        assert_eq!(
            tokenize("你好，世界。 OK?"),
            vec!["你", "好", "世", "界", "ok"]
        );
        assert!(tokenize("，。!?").is_empty());
    }

    #[test]
    fn keeps_inner_apostrophes() {
        assert_eq!(tokenize("don't go"), vec!["don't", "go"]);
    }

    #[test]
    fn identical_is_zero() {
        assert_eq!(mixed_error_rate("我们去makan", "我们去Makan!"), 0.0);
    }

    #[test]
    fn one_english_substitution() {
        // 3 ref tokens (去, makan, la), 1 substitution
        let s = score("去 makan la", "去 makan lah");
        assert_eq!(
            s,
            Score {
                errors: 1,
                ref_tokens: 3
            }
        );
        assert!((s.rate() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn one_chinese_character_wrong_counts_one() {
        // 4 ref tokens, 1 substitution
        assert_eq!(
            score("我很高兴", "我很高心"),
            Score {
                errors: 1,
                ref_tokens: 4
            }
        );
    }

    #[test]
    fn deletion_and_insertion() {
        assert_eq!(score("a b c", "a c").errors, 1);
        assert_eq!(score("a c", "a b c").errors, 1);
    }

    #[test]
    fn can_exceed_one() {
        assert!(mixed_error_rate("hi", "a b c d") > 1.0);
    }

    #[test]
    fn empty_reference() {
        assert_eq!(mixed_error_rate("", ""), 0.0);
        assert_eq!(mixed_error_rate("", "x"), 1.0);
    }

    #[test]
    fn completely_wrong_is_one() {
        assert_eq!(mixed_error_rate("你好", "再见"), 1.0);
    }

    #[test]
    fn edit_distance_generic() {
        assert_eq!(edit_distance(&['k', 'i', 't'], &['s', 'i', 't', 's']), 2);
    }

    #[test]
    fn singlish_layer_lowers_error() {
        // Reference uses the spelling "lah"; raw ASR heard "la".
        let reference = "this one shiok lah";
        let raw = "this one shiock la";
        let fixed = crate::apply(raw, &crate::Vocabulary::bundled());
        assert!(mixed_error_rate(reference, &fixed) < mixed_error_rate(reference, raw));
        assert_eq!(mixed_error_rate(reference, &fixed), 0.0);
    }
}
