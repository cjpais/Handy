use serde::{Deserialize, Serialize};
use specta::Type;

/// Default English filler words and phrases
pub const DEFAULT_FILLER_WORDS: &[&str] = &[
    // Hesitation sounds
    "um",
    "umm",
    "uh",
    "uhh",
    "er",
    "err",
    "ah",
    "ahh",
    "hmm",
    "hm",
    "mm",
    "mmm",
    // Common filler words
    "like",
    "basically",
    "actually",
    "literally",
    "honestly",
    "obviously",
    // Filler phrases (order matters - longer phrases first)
    "you know what I mean",
    "you know",
    "I mean",
    "kind of",
    "sort of",
    "I guess",
    "I think",
    // Discourse markers (when used as fillers)
    "so yeah",
    "okay so",
    "and then",
    "anyway",
    "right",
    "well",
    "so",
];

/// Represents a single filler word match in the text
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FillerWordMatch {
    /// The filler word that was matched
    pub word: String,
    /// Start index in the original text
    pub start_index: usize,
    /// End index in the original text
    pub end_index: usize,
}

/// Analysis result for filler word detection
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FillerWordAnalysis {
    /// List of all filler word matches found
    pub matches: Vec<FillerWordMatch>,
    /// Text with filler words removed (cleaned text)
    pub cleaned_text: String,
    /// Total word count in original text
    pub total_words: usize,
    /// Number of filler words detected
    pub filler_count: usize,
    /// Percentage of words that were fillers (0.0 - 100.0)
    pub filler_percentage: f32,
    /// Breakdown of filler word usage by word
    pub filler_breakdown: Vec<FillerWordCount>,
}

/// Count of how many times a specific filler word was used
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FillerWordCount {
    pub word: String,
    pub count: usize,
}

/// Detect filler words in the given text
///
/// # Arguments
/// * `text` - The transcribed text to analyze
/// * `custom_fillers` - Optional custom filler words to detect in addition to defaults
///
/// # Returns
/// A `FillerWordAnalysis` containing all detection results
pub fn detect_filler_words(text: &str, custom_fillers: Option<&[String]>) -> FillerWordAnalysis {
    let text_lower = text.to_lowercase();

    // Build the list of filler words to check (custom + defaults)
    let mut filler_patterns: Vec<&str> = DEFAULT_FILLER_WORDS.to_vec();
    let custom_refs: Vec<&str>;
    if let Some(custom) = custom_fillers {
        custom_refs = custom.iter().map(|s| s.as_str()).collect();
        filler_patterns.extend(custom_refs.iter());
    }

    // Sort by length descending so longer phrases match first
    filler_patterns.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut matches: Vec<FillerWordMatch> = Vec::new();
    let mut matched_ranges: Vec<(usize, usize)> = Vec::new();

    // Find all filler word matches
    for pattern in &filler_patterns {
        let pattern_lower = pattern.to_lowercase();
        let mut search_start = 0;

        while let Some(pos) = text_lower[search_start..].find(&pattern_lower) {
            let start = search_start + pos;
            let end = start + pattern.len();

            // Check if this position overlaps with already matched ranges
            let overlaps = matched_ranges.iter().any(|(s, e)| start < *e && end > *s);

            if !overlaps {
                // Check word boundaries - ensure we're matching whole words/phrases
                let is_word_start = start == 0
                    || !text_lower
                        .chars()
                        .nth(start - 1)
                        .unwrap_or(' ')
                        .is_alphabetic();
                let is_word_end = end >= text_lower.len()
                    || !text_lower.chars().nth(end).unwrap_or(' ').is_alphabetic();

                if is_word_start && is_word_end {
                    matches.push(FillerWordMatch {
                        word: text[start..end].to_string(),
                        start_index: start,
                        end_index: end,
                    });
                    matched_ranges.push((start, end));
                }
            }

            search_start = start + 1;
        }
    }

    // Sort matches by position
    matches.sort_by(|a, b| a.start_index.cmp(&b.start_index));

    // Build cleaned text by removing filler words
    let cleaned_text = build_cleaned_text(text, &matches);

    // Count total words in original text
    let total_words = count_words(text);

    // Count filler words
    let filler_count = matches.len();

    // Calculate percentage
    let filler_percentage = if total_words > 0 {
        (filler_count as f32 / total_words as f32) * 100.0
    } else {
        0.0
    };

    // Build breakdown
    let filler_breakdown = build_breakdown(&matches);

    FillerWordAnalysis {
        matches,
        cleaned_text,
        total_words,
        filler_count,
        filler_percentage,
        filler_breakdown,
    }
}

/// Build cleaned text by removing filler words
fn build_cleaned_text(original: &str, matches: &[FillerWordMatch]) -> String {
    if matches.is_empty() {
        return original.to_string();
    }

    let mut result = String::new();
    let mut last_end = 0;

    for m in matches {
        // Add text before this match
        if m.start_index > last_end {
            result.push_str(&original[last_end..m.start_index]);
        }
        last_end = m.end_index;
    }

    // Add remaining text after last match
    if last_end < original.len() {
        result.push_str(&original[last_end..]);
    }

    // Clean up extra whitespace
    let cleaned: String = result.split_whitespace().collect::<Vec<&str>>().join(" ");

    // Trim and ensure proper capitalization if needed
    cleaned.trim().to_string()
}

/// Count words in text
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Build breakdown of filler word usage
fn build_breakdown(matches: &[FillerWordMatch]) -> Vec<FillerWordCount> {
    use std::collections::HashMap;

    let mut counts: HashMap<String, usize> = HashMap::new();

    for m in matches {
        let word_lower = m.word.to_lowercase();
        *counts.entry(word_lower).or_insert(0) += 1;
    }

    let mut breakdown: Vec<FillerWordCount> = counts
        .into_iter()
        .map(|(word, count)| FillerWordCount { word, count })
        .collect();

    // Sort by count descending
    breakdown.sort_by(|a, b| b.count.cmp(&a.count));

    breakdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_filler_detection() {
        let text = "So um I was like thinking about, you know, the project";
        let analysis = detect_filler_words(text, None);

        assert!(analysis.filler_count > 0);
        assert!(analysis
            .matches
            .iter()
            .any(|m| m.word.to_lowercase() == "um"));
        assert!(analysis
            .matches
            .iter()
            .any(|m| m.word.to_lowercase() == "like"));
        assert!(analysis
            .matches
            .iter()
            .any(|m| m.word.to_lowercase() == "you know"));
    }

    #[test]
    fn test_cleaned_text() {
        let text = "I um think we should uh do this";
        let analysis = detect_filler_words(text, None);

        assert_eq!(analysis.cleaned_text, "I think we should do this");
    }

    #[test]
    fn test_custom_fillers() {
        let text = "So basically dude I was thinking";
        let custom = vec!["dude".to_string()];
        let analysis = detect_filler_words(text, Some(&custom));

        assert!(analysis
            .matches
            .iter()
            .any(|m| m.word.to_lowercase() == "dude"));
    }

    #[test]
    fn test_no_fillers() {
        let text = "The quick brown fox jumps over the lazy dog";
        let analysis = detect_filler_words(text, None);

        assert_eq!(analysis.filler_count, 0);
        assert_eq!(analysis.filler_percentage, 0.0);
        assert_eq!(analysis.cleaned_text, text);
    }

    #[test]
    fn test_percentage_calculation() {
        let text = "um um um um um"; // 5 filler words out of 5 total
        let analysis = detect_filler_words(text, None);

        assert_eq!(analysis.total_words, 5);
        assert_eq!(analysis.filler_count, 5);
        assert!((analysis.filler_percentage - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_breakdown() {
        let text = "um like um like um";
        let analysis = detect_filler_words(text, None);

        assert_eq!(analysis.filler_breakdown.len(), 2);
        let um_count = analysis
            .filler_breakdown
            .iter()
            .find(|b| b.word == "um")
            .map(|b| b.count)
            .unwrap_or(0);
        assert_eq!(um_count, 3);
    }
}
