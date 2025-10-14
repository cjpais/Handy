use natural::phonetics::soundex;
use strsim::levenshtein;
use regex::Regex;
use crate::settings::{RegexFilter, PolishRule};
use reqwest;
use serde_json::{json, Value};

/// Applies custom word corrections to transcribed text using fuzzy matching
///
/// This function corrects words in the input text by finding the best matches
/// from a list of custom words using a combination of:
/// - Levenshtein distance for string similarity
/// - Soundex phonetic matching for pronunciation similarity
///
/// # Arguments
/// * `text` - The input text to correct
/// * `custom_words` - List of custom words to match against
/// * `threshold` - Maximum similarity score to accept (0.0 = exact match, 1.0 = any match)
///
/// # Returns
/// The corrected text with custom words applied
pub fn apply_custom_words(text: &str, custom_words: &[String], threshold: f64) -> String {
    if custom_words.is_empty() {
        return text.to_string();
    }

    // Pre-compute lowercase versions to avoid repeated allocations
    let custom_words_lower: Vec<String> = custom_words.iter().map(|w| w.to_lowercase()).collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut corrected_words = Vec::new();

    for word in words {
        let cleaned_word = word
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_lowercase();

        if cleaned_word.is_empty() {
            corrected_words.push(word.to_string());
            continue;
        }

        // Skip extremely long words to avoid performance issues
        if cleaned_word.len() > 50 {
            corrected_words.push(word.to_string());
            continue;
        }

        let mut best_match: Option<&String> = None;
        let mut best_score = f64::MAX;

        for (i, custom_word_lower) in custom_words_lower.iter().enumerate() {
            // Skip if lengths are too different (optimization)
            let len_diff = (cleaned_word.len() as i32 - custom_word_lower.len() as i32).abs();
            if len_diff > 5 {
                continue;
            }

            // Calculate Levenshtein distance (normalized by length)
            let levenshtein_dist = levenshtein(&cleaned_word, custom_word_lower);
            let max_len = cleaned_word.len().max(custom_word_lower.len()) as f64;
            let levenshtein_score = if max_len > 0.0 {
                levenshtein_dist as f64 / max_len
            } else {
                1.0
            };

            // Calculate phonetic similarity using Soundex
            let phonetic_match = soundex(&cleaned_word, custom_word_lower);

            // Combine scores: favor phonetic matches, but also consider string similarity
            let combined_score = if phonetic_match {
                levenshtein_score * 0.3 // Give significant boost to phonetic matches
            } else {
                levenshtein_score
            };

            // Accept if the score is good enough (configurable threshold)
            if combined_score < threshold && combined_score < best_score {
                best_match = Some(&custom_words[i]);
                best_score = combined_score;
            }
        }

        if let Some(replacement) = best_match {
            // Preserve the original case pattern as much as possible
            let corrected = preserve_case_pattern(word, replacement);

            // Preserve punctuation from original word
            let (prefix, suffix) = extract_punctuation(word);
            corrected_words.push(format!("{}{}{}", prefix, corrected, suffix));
        } else {
            corrected_words.push(word.to_string());
        }
    }

    corrected_words.join(" ")
}

/// Preserves the case pattern of the original word when applying a replacement
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if original.chars().next().map_or(false, |c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first_char) = chars.get_mut(0) {
            *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

/// Extracts punctuation prefix and suffix from a word
fn extract_punctuation(word: &str) -> (&str, &str) {
    let prefix_end = word.chars().take_while(|c| !c.is_alphabetic()).count();
    let suffix_start = word
        .char_indices()
        .rev()
        .take_while(|(_, c)| !c.is_alphabetic())
        .count();

    let prefix = if prefix_end > 0 {
        &word[..prefix_end]
    } else {
        ""
    };

    let suffix = if suffix_start > 0 {
        &word[word.len() - suffix_start..]
    } else {
        ""
    };

    (prefix, suffix)
}

/// Applies regex filters to text based on enabled filters
///
/// This function processes text through a series of regex filters,
/// applying replacements for each enabled filter in order.
///
/// # Arguments
/// * `text` - The input text to filter
/// * `regex_filters` - List of regex filters to apply
///
/// # Returns
/// The filtered text with all enabled regex replacements applied
pub fn apply_regex_filters(text: &str, regex_filters: &[RegexFilter]) -> String {
    let mut result = text.to_string();
    
    for filter in regex_filters {
        if !filter.enabled {
            continue;
        }
        
        // Try to compile the regex pattern
        match Regex::new(&filter.pattern) {
            Ok(regex) => {
                result = regex.replace_all(&result, &filter.replacement).to_string();
            }
            Err(e) => {
                // Log the error but continue processing other filters
                log::warn!("Invalid regex pattern '{}' in filter '{}': {}", 
                          filter.pattern, filter.name, e);
            }
        }
    }
    
    result
}

/// Applies polish rules to text using OpenAI-compatible API
///
/// This function processes text through a series of polish rules,
/// sending requests to configured LLM APIs for text enhancement.
///
/// # Arguments
/// * `text` - The input text to polish
/// * `polish_rules` - List of polish rules to apply
///
/// # Returns
/// The polished text with all enabled rules applied
pub async fn apply_polish_rules(text: &str, polish_rules: &[PolishRule]) -> String {
    let mut result = text.to_string();
    
    for rule in polish_rules {
        if !rule.enabled {
            continue;
        }
        
        match polish_text_with_rule(&result, rule).await {
            Ok(polished_text) => {
                result = polished_text;
            }
            Err(e) => {
                // Log the error but continue processing other rules
                log::warn!("Failed to apply polish rule '{}': {}", rule.name, e);
            }
        }
    }
    
    result
}

/// Applies polish rules to text using OpenAI-compatible API with detailed error reporting
///
/// This function processes text through a series of polish rules,
/// sending requests to configured LLM APIs for text enhancement.
/// Unlike apply_polish_rules, this function returns detailed error information.
///
/// # Arguments
/// * `text` - The input text to polish
/// * `polish_rules` - List of polish rules to apply
///
/// # Returns
/// Result containing the polished text or detailed error information
pub async fn apply_polish_rules_with_error(text: &str, polish_rules: &[PolishRule]) -> Result<String, String> {
    let mut result = text.to_string();
    let mut errors = Vec::new();
    
    for rule in polish_rules {
        if !rule.enabled {
            continue;
        }
        
        match polish_text_with_rule(&result, rule).await {
            Ok(polished_text) => {
                result = polished_text;
            }
            Err(e) => {
                let error_msg = format!("Rule '{}': {}", rule.name, e);
                log::warn!("Failed to apply polish rule: {}", error_msg);
                errors.push(error_msg);
            }
        }
    }
    
    if !errors.is_empty() && result == text {
        // If no rules succeeded and we have errors, return the error
        Err(errors.join("; "))
    } else if !errors.is_empty() {
        // Some rules failed but we have partial results
        log::warn!("Some polish rules failed: {}", errors.join("; "));
        Ok(result)
    } else {
        Ok(result)
    }
}

/// Polishes text using a single polish rule via OpenAI-compatible API
async fn polish_text_with_rule(text: &str, rule: &PolishRule) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    
    let request_body = json!({
        "model": rule.model,
        "messages": [
            {
                "role": "system",
                "content": rule.prompt
            },
            {
                "role": "user",
                "content": text
            }
        ],
        "temperature": 0.3,
        "max_tokens": 2000
    });
    
    let response = client
        .post(&rule.api_url)
        .header("Authorization", format!("Bearer {}", rule.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("API request failed with status: {}", response.status()).into());
    }
    
    let response_json: Value = response.json().await?;
    
    // Extract the polished text from the response
    if let Some(choices) = response_json.get("choices") {
        if let Some(first_choice) = choices.get(0) {
            if let Some(message) = first_choice.get("message") {
                if let Some(content) = message.get("content") {
                    if let Some(polished_text) = content.as_str() {
                        return Ok(polished_text.trim().to_string());
                    }
                }
            }
        }
    }
    
    Err("Invalid response format from API".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_custom_words_exact_match() {
        let text = "hello world";
        let custom_words = vec!["Hello".to_string(), "World".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let text = "helo wrold";
        let custom_words = vec!["hello".to_string(), "world".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_preserve_case_pattern() {
        assert_eq!(preserve_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(preserve_case_pattern("Hello", "world"), "World");
        assert_eq!(preserve_case_pattern("hello", "WORLD"), "WORLD");
    }

    #[test]
    fn test_extract_punctuation() {
        assert_eq!(extract_punctuation("hello"), ("", ""));
        assert_eq!(extract_punctuation("!hello?"), ("!", "?"));
        assert_eq!(extract_punctuation("...hello..."), ("...", "..."));
    }

    #[test]
    fn test_empty_custom_words() {
        let text = "hello world";
        let custom_words = vec![];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }
}
