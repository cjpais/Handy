/// Convert a sequence of SentencePiece tokens to readable text.
///
/// Handles the `▁` (U+2581) word boundary marker by replacing it with a space,
/// trims the result, and cleans up contraction spacing.
pub fn sentencepiece_to_text(tokens: &[&str]) -> String {
    let mut text = String::new();
    for &token in tokens {
        text.push_str(&token.replace('\u{2581}', " "));
    }
    let text = text.trim().to_string();
    // Clean up contraction spacing (e.g. "can 't" → "can't")
    text.replace(" '", "'")
}
