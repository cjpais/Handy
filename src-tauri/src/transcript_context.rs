/// Manager for tracking recent transcription context per application.
/// This allows the LLM post-processing to have context about previous
/// transcriptions in the same application.
use log::debug;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Maximum number of words to keep in the short previous transcript
const MAX_PREV_WORDS: usize = 200;

/// How long before previous transcript expires (5 minutes)
const EXPIRY_DURATION: Duration = Duration::from_secs(5 * 60);

/// Entry for tracking transcript history per application
#[derive(Clone, Debug)]
struct TranscriptEntry {
    /// The transcript text (trimmed to last MAX_PREV_WORDS words)
    text: String,
    /// When this entry was last updated
    last_updated: Instant,
}

/// Global state for tracking transcripts per application
static TRANSCRIPT_CONTEXT: Lazy<Mutex<HashMap<String, TranscriptEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Get the short previous transcript for an application.
/// Returns up to the last 200 words of the most recent transcript
/// for this application, if it was within the last 5 minutes.
/// Returns an empty string if no recent transcript exists or if it has expired.
pub fn get_short_prev_transcript(app_name: &str) -> String {
    let context = match TRANSCRIPT_CONTEXT.lock() {
        Ok(guard) => guard,
        Err(e) => {
            debug!("Failed to lock transcript context: {}", e);
            return String::new();
        }
    };

    if let Some(entry) = context.get(app_name) {
        // Check if the entry has expired
        if entry.last_updated.elapsed() < EXPIRY_DURATION {
            debug!(
                "Found previous transcript for '{}': {} chars",
                app_name,
                entry.text.len()
            );
            return entry.text.clone();
        } else {
            debug!(
                "Previous transcript for '{}' has expired ({:?} ago)",
                app_name,
                entry.last_updated.elapsed()
            );
        }
    }

    String::new()
}

/// Update the transcript context for an application.
/// The text is trimmed to the last MAX_PREV_WORDS words.
pub fn update_transcript_context(app_name: &str, transcript: &str) {
    if app_name.is_empty() {
        debug!("Skipping transcript context update: empty app name");
        return;
    }

    let trimmed_text = trim_to_last_words(transcript, MAX_PREV_WORDS);

    let mut context = match TRANSCRIPT_CONTEXT.lock() {
        Ok(guard) => guard,
        Err(e) => {
            debug!("Failed to lock transcript context for update: {}", e);
            return;
        }
    };

    // Update or insert the entry
    let entry = context
        .entry(app_name.to_string())
        .or_insert_with(|| TranscriptEntry {
            text: String::new(),
            last_updated: Instant::now(),
        });

    // Append the new transcript to existing text, then trim
    if !entry.text.is_empty() && entry.last_updated.elapsed() < EXPIRY_DURATION {
        // Combine with previous text if not expired
        let combined = format!("{} {}", entry.text, trimmed_text);
        entry.text = trim_to_last_words(&combined, MAX_PREV_WORDS);
    } else {
        // Start fresh if expired or empty
        entry.text = trimmed_text;
    }
    entry.last_updated = Instant::now();

    debug!(
        "Updated transcript context for '{}': {} chars",
        app_name,
        entry.text.len()
    );

    // Clean up expired entries periodically
    cleanup_expired_entries(&mut context);
}

/// Trim text to the last N words
fn trim_to_last_words(text: &str, max_words: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        words.join(" ")
    } else {
        words[words.len() - max_words..].join(" ")
    }
}

/// Remove expired entries from the context map
fn cleanup_expired_entries(context: &mut HashMap<String, TranscriptEntry>) {
    let expired_keys: Vec<String> = context
        .iter()
        .filter(|(_, entry)| entry.last_updated.elapsed() >= EXPIRY_DURATION)
        .map(|(key, _)| key.clone())
        .collect();

    for key in expired_keys {
        context.remove(&key);
        debug!("Removed expired transcript context for '{}'", key);
    }
}

/// Clear all transcript context (useful for testing or reset)
#[allow(dead_code)]
pub fn clear_all_context() {
    if let Ok(mut context) = TRANSCRIPT_CONTEXT.lock() {
        context.clear();
        debug!("Cleared all transcript context");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_to_last_words() {
        assert_eq!(trim_to_last_words("hello world", 10), "hello world");
        assert_eq!(trim_to_last_words("a b c d e", 3), "c d e");
        assert_eq!(trim_to_last_words("one", 5), "one");
        assert_eq!(trim_to_last_words("", 5), "");
    }

    #[test]
    fn test_get_and_update_context() {
        clear_all_context();

        // Initially empty
        assert_eq!(get_short_prev_transcript("TestApp"), "");

        // Update with some text
        update_transcript_context("TestApp", "Hello world this is a test");

        // Should get the text back
        let result = get_short_prev_transcript("TestApp");
        assert_eq!(result, "Hello world this is a test");

        // Update with more text - should combine
        update_transcript_context("TestApp", "Another sentence here");

        let result = get_short_prev_transcript("TestApp");
        assert!(result.contains("Another sentence here"));

        clear_all_context();
    }

    #[test]
    fn test_empty_app_name_ignored() {
        clear_all_context();
        update_transcript_context("", "Some text");
        assert_eq!(get_short_prev_transcript(""), "");
        clear_all_context();
    }
}
