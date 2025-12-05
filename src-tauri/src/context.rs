//! Context-aware text processing using macOS Accessibility APIs.
//!
//! This module reads the character before the cursor in the currently focused
//! text field to determine appropriate capitalization for inserted text.
//!
//! Features:
//! - Capitalizes after sentence-ending punctuation (. ! ?)
//! - Lowercases after continuation punctuation (, ; : -)
//! - Adds trailing space after sentence-ending punctuation in output
//! - When context cannot be read (terminal apps), assumes consecutive sentences

use log::{debug, info};

/// Characters that indicate the next word should be capitalized.
/// These are sentence-ending punctuation marks.
const CAPITALIZE_AFTER: &[char] = &['.', '!', '?'];

/// Characters that indicate the next word should be lowercase.
/// These typically appear mid-sentence and signal continuation.
const LOWERCASE_AFTER: &[char] = &[',', ';', ':', '-', '–', '—'];

/// Maximum number of whitespace characters to look back through.
/// Allows for "Hello. " (1 space) or "Hello.  " (2 spaces) scenarios.
const MAX_WHITESPACE_LOOKBACK: usize = 2;

/// Result of analyzing the text context before the cursor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapitalizationHint {
    /// Should capitalize (after sentence-ending punctuation, newline, or empty field)
    Capitalize,
    /// Should use lowercase (after comma, semicolon, etc.)
    Lowercase,
    /// Could not determine context - use default behavior
    Unknown,
}

/// Get the text before the cursor in the focused text field.
///
/// Returns `None` if:
/// - No text field is focused
/// - The cursor is at the beginning of the field
/// - The accessibility API call fails
/// - Running on a non-macOS platform
#[cfg(target_os = "macos")]
fn get_text_before_cursor() -> Option<String> {
    use accessibility::{AXAttribute, AXUIElement, AXUIElementAttributes};
    use accessibility_sys::{kAXValueTypeCFRange, AXValueGetValue, AXValueRef};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::string::CFString;
    use std::mem::MaybeUninit;

    // CFRange struct for extracting the selection range
    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    struct CFRange {
        location: i64,
        length: i64,
    }

    // Get system-wide accessibility element
    let system_wide = AXUIElement::system_wide();

    // Get the focused UI element
    let focused_attr = AXAttribute::<CFType>::new(&CFString::new("AXFocusedUIElement"));
    let focused_cftype: CFType = match system_wide.attribute(&focused_attr) {
        Ok(elem) => elem,
        Err(e) => {
            debug!("Failed to get focused element: {:?}", e);
            return None;
        }
    };

    // Cast CFType to AXUIElement
    let focused = unsafe { AXUIElement::wrap_under_get_rule(focused_cftype.as_CFTypeRef() as _) };

    // Get the text value of the focused element using the predefined value() attribute
    let value_cftype: CFType = match focused.value() {
        Ok(val) => val,
        Err(e) => {
            debug!("Failed to get text value: {:?}", e);
            return None;
        }
    };

    // Try to convert the CFType to a CFString
    let text = if value_cftype.instance_of::<CFString>() {
        unsafe { CFString::wrap_under_get_rule(value_cftype.as_CFTypeRef() as _) }.to_string()
    } else {
        debug!("Value is not a string type");
        return None;
    };

    if text.is_empty() {
        debug!("Text field is empty");
        return None;
    }

    // Get the selected text range (cursor position)
    let range_attr = AXAttribute::<CFType>::new(&CFString::new("AXSelectedTextRange"));
    let range_cftype: CFType = match focused.attribute(&range_attr) {
        Ok(val) => val,
        Err(e) => {
            debug!("Failed to get selected text range: {:?}", e);
            return None;
        }
    };

    // Extract the CFRange from the AXValue
    let mut range = MaybeUninit::<CFRange>::uninit();
    let success = unsafe {
        AXValueGetValue(
            range_cftype.as_CFTypeRef() as AXValueRef,
            kAXValueTypeCFRange,
            range.as_mut_ptr() as *mut _,
        )
    };

    if !success {
        debug!("Failed to extract range from AXValue");
        return None;
    }

    let range = unsafe { range.assume_init() };
    let cursor_pos = range.location as usize;

    debug!(
        "Cursor position: {}, text length: {}",
        cursor_pos,
        text.len()
    );

    if cursor_pos == 0 {
        debug!("Cursor at beginning of field");
        return None;
    }

    // Get the text before the cursor
    // Handle UTF-8 properly by using char indices
    let chars: Vec<char> = text.chars().collect();
    if cursor_pos <= chars.len() {
        let text_before: String = chars[..cursor_pos].iter().collect();
        debug!("Text before cursor: {:?}", text_before);
        Some(text_before)
    } else {
        debug!(
            "Cursor position {} out of bounds for {} chars",
            cursor_pos,
            chars.len()
        );
        None
    }
}

#[cfg(not(target_os = "macos"))]
fn get_text_before_cursor() -> Option<String> {
    // Not implemented for other platforms
    None
}

/// Find the relevant punctuation character by looking back through the text.
///
/// This handles common scenarios:
/// - "Hello." (cursor right after period)
/// - "Hello. " (one space after period)
/// - "Hello.  " (two spaces after period)
/// - "Hey," (cursor right after comma)
/// - "Hey, " (one space after comma)
///
/// Returns the first non-whitespace character found within MAX_WHITESPACE_LOOKBACK spaces,
/// or None if we hit the beginning of text or exceed the lookback limit.
fn find_relevant_punctuation(text: &str) -> Option<char> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut whitespace_count = 0;

    // Scan backwards from the end of the text
    for &c in chars.iter().rev() {
        if c == ' ' || c == '\t' {
            whitespace_count += 1;
            if whitespace_count > MAX_WHITESPACE_LOOKBACK {
                // Too much whitespace - can't determine context reliably
                debug!(
                    "Exceeded max whitespace lookback ({}), returning None",
                    MAX_WHITESPACE_LOOKBACK
                );
                return None;
            }
        } else if c == '\n' || c == '\r' {
            // Newline always means capitalize
            debug!("Found newline, returning it as relevant punctuation");
            return Some(c);
        } else {
            // Found a non-whitespace character
            debug!(
                "Found relevant character '{}' after {} whitespace chars",
                c, whitespace_count
            );
            return Some(c);
        }
    }

    // Reached beginning of text (all whitespace or empty)
    debug!("Reached beginning of text while scanning");
    None
}

/// Result of context analysis.
#[derive(Debug, Clone)]
struct ContextResult {
    /// Whether to capitalize, lowercase, or leave unchanged
    hint: CapitalizationHint,
    /// Whether context was successfully read (false = fallback mode)
    context_readable: bool,
}

/// Analyze the context and return capitalization hint plus context readability.
///
/// This function:
/// 1. Reads text before the cursor
/// 2. Scans back through whitespace (up to MAX_WHITESPACE_LOOKBACK chars)
/// 3. Determines capitalization based on the punctuation found
/// 4. Reports whether context was successfully read
///
/// ## Scenarios handled:
/// - `"Hello."` → Capitalize (context readable)
/// - `"Hello. "` → Capitalize (context readable)
/// - `"Hey,"` → Lowercase (context readable)
/// - `"Hey, "` → Lowercase (context readable)
/// - `"Hello"` → Unknown/mid-word (context readable)
/// - Empty/beginning → Capitalize (context readable)
/// - Terminal/API failure → Capitalize (context NOT readable - fallback mode)
fn analyze_context() -> ContextResult {
    let text = match get_text_before_cursor() {
        Some(t) => t,
        None => {
            // Can't read context (unsupported app like terminal, or API failure)
            // In fallback mode, assume consecutive sentences
            debug!("No text before cursor (fallback mode), defaulting to Capitalize");
            return ContextResult {
                hint: CapitalizationHint::Capitalize,
                context_readable: false,
            };
        }
    };

    // If we got an empty string, treat as start of text (but context IS readable)
    if text.is_empty() {
        debug!("Empty text field, Capitalize (context readable)");
        return ContextResult {
            hint: CapitalizationHint::Capitalize,
            context_readable: true,
        };
    }

    // Find the relevant punctuation by looking back through whitespace
    let relevant_char = find_relevant_punctuation(&text);

    match relevant_char {
        None => {
            // Reached beginning of text or exceeded lookback - capitalize
            debug!("No relevant punctuation found, defaulting to Capitalize");
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                context_readable: true,
            }
        }
        Some(c) if CAPITALIZE_AFTER.contains(&c) => {
            debug!("Found sentence-ending '{}', Capitalize", c);
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                context_readable: true,
            }
        }
        Some(c) if c == '\n' || c == '\r' => {
            debug!("Found newline, Capitalize");
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                context_readable: true,
            }
        }
        Some(c) if LOWERCASE_AFTER.contains(&c) => {
            debug!("Found continuation '{}', Lowercase", c);
            ContextResult {
                hint: CapitalizationHint::Lowercase,
                context_readable: true,
            }
        }
        Some(c) => {
            // Some other character (letter, number, etc.)
            debug!("Found other char '{}', Unknown", c);
            ContextResult {
                hint: CapitalizationHint::Unknown,
                context_readable: true,
            }
        }
    }
}

/// Apply capitalization hint to the given text.
///
/// - If hint is `Capitalize`, ensures first alphabetic char is uppercase
/// - If hint is `Lowercase`, ensures first alphabetic char is lowercase
/// - If hint is `Unknown`, returns text unchanged
pub fn apply_capitalization(text: &str, hint: CapitalizationHint) -> String {
    if text.is_empty() {
        return text.to_string();
    }

    match hint {
        CapitalizationHint::Unknown => text.to_string(),
        CapitalizationHint::Capitalize => {
            let mut chars: Vec<char> = text.chars().collect();
            // Find first alphabetic character and uppercase it
            for c in chars.iter_mut() {
                if c.is_alphabetic() {
                    if c.is_lowercase() {
                        *c = c.to_uppercase().next().unwrap_or(*c);
                    }
                    break;
                }
            }
            chars.into_iter().collect()
        }
        CapitalizationHint::Lowercase => {
            let mut chars: Vec<char> = text.chars().collect();
            // Find first alphabetic character and lowercase it
            for c in chars.iter_mut() {
                if c.is_alphabetic() {
                    if c.is_uppercase() {
                        *c = c.to_lowercase().next().unwrap_or(*c);
                    }
                    break;
                }
            }
            chars.into_iter().collect()
        }
    }
}

/// Convenience function that reads context and applies capitalization in one step.
///
/// This is the main entry point for context-aware capitalization.
/// On non-macOS platforms or if context cannot be read, uses fallback behavior.
///
/// This function:
/// 1. Analyzes the text before the cursor
/// 2. Determines appropriate capitalization (capitalize/lowercase/unchanged)
/// 3. Adds a trailing space based on smart logic:
///    - If output ends with sentence-ending punctuation (. ! ?), add space
///    - If context was NOT readable (terminal apps, etc.), add space (assume consecutive sentences)
///
/// ## Examples:
/// - After `"Hello."` with input `"world"` → `"World"` (capitalize, context readable)
/// - After `"Hey,"` with input `"What"` → `"what"` (lowercase, context readable)
/// - Input `"Hello world."` → `"Hello world. "` (trailing space for punctuation)
/// - In terminal with input `"hello"` → `"Hello "` (trailing space for fallback mode)
pub fn apply_context_aware_capitalization(text: &str) -> String {
    let context = analyze_context();
    let capitalized = apply_capitalization(text, context.hint);

    // Determine if we should add a trailing space:
    // 1. Always add space after sentence-ending punctuation
    // 2. Add space in fallback mode (context not readable) - assume consecutive sentences
    let ends_with_sentence_punctuation = capitalized.ends_with('.')
        || capitalized.ends_with('!')
        || capitalized.ends_with('?');

    let should_add_trailing_space = ends_with_sentence_punctuation || !context.context_readable;

    let result = if should_add_trailing_space {
        format!("{} ", capitalized)
    } else {
        capitalized
    };

    info!(
        "Context-aware capitalization: hint={:?}, context_readable={}, input='{}', output='{}'",
        context.hint, context.context_readable, text, result
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_capitalization_capitalize() {
        assert_eq!(
            apply_capitalization("hello world", CapitalizationHint::Capitalize),
            "Hello world"
        );
        assert_eq!(
            apply_capitalization("Hello world", CapitalizationHint::Capitalize),
            "Hello world"
        );
        assert_eq!(
            apply_capitalization("123 hello", CapitalizationHint::Capitalize),
            "123 Hello"
        );
    }

    #[test]
    fn test_apply_capitalization_lowercase() {
        assert_eq!(
            apply_capitalization("Hello world", CapitalizationHint::Lowercase),
            "hello world"
        );
        assert_eq!(
            apply_capitalization("hello world", CapitalizationHint::Lowercase),
            "hello world"
        );
        assert_eq!(
            apply_capitalization("123 Hello", CapitalizationHint::Lowercase),
            "123 hello"
        );
    }

    #[test]
    fn test_apply_capitalization_unknown() {
        assert_eq!(
            apply_capitalization("Hello world", CapitalizationHint::Unknown),
            "Hello world"
        );
        assert_eq!(
            apply_capitalization("hello world", CapitalizationHint::Unknown),
            "hello world"
        );
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(
            apply_capitalization("", CapitalizationHint::Capitalize),
            ""
        );
        assert_eq!(
            apply_capitalization("", CapitalizationHint::Lowercase),
            ""
        );
    }

    #[test]
    fn test_unicode_characters() {
        // Test with accented characters
        assert_eq!(
            apply_capitalization("éllo world", CapitalizationHint::Capitalize),
            "Éllo world"
        );
        assert_eq!(
            apply_capitalization("Éllo world", CapitalizationHint::Lowercase),
            "éllo world"
        );
        // Test with non-Latin scripts (should handle gracefully)
        assert_eq!(
            apply_capitalization("日本語", CapitalizationHint::Capitalize),
            "日本語" // No change - no alphabetic uppercase/lowercase distinction
        );
    }

    #[test]
    fn test_punctuation_only() {
        // Strings with only punctuation should be unchanged
        assert_eq!(
            apply_capitalization("...", CapitalizationHint::Capitalize),
            "..."
        );
        assert_eq!(
            apply_capitalization("123", CapitalizationHint::Lowercase),
            "123"
        );
    }

    #[test]
    fn test_leading_whitespace() {
        // Whitespace before the first letter
        assert_eq!(
            apply_capitalization("  hello", CapitalizationHint::Capitalize),
            "  Hello"
        );
        assert_eq!(
            apply_capitalization("  Hello", CapitalizationHint::Lowercase),
            "  hello"
        );
    }

    #[test]
    fn test_mixed_content() {
        // Numbers and punctuation before letters
        assert_eq!(
            apply_capitalization("42 - hello", CapitalizationHint::Capitalize),
            "42 - Hello"
        );
        // Lowercase hint only affects the first alphabetic char
        // Here 'n' in 'note' is already lowercase, so no change
        assert_eq!(
            apply_capitalization("(note: Hello)", CapitalizationHint::Lowercase),
            "(note: Hello)"
        );
        // Test with first letter being uppercase
        assert_eq!(
            apply_capitalization("(Note: Hello)", CapitalizationHint::Lowercase),
            "(note: Hello)"
        );
    }

    #[test]
    fn test_single_character() {
        assert_eq!(
            apply_capitalization("a", CapitalizationHint::Capitalize),
            "A"
        );
        assert_eq!(
            apply_capitalization("A", CapitalizationHint::Lowercase),
            "a"
        );
    }

    #[test]
    fn test_find_relevant_punctuation_sentence_end() {
        // Cursor right after punctuation
        assert_eq!(find_relevant_punctuation("Hello."), Some('.'));
        assert_eq!(find_relevant_punctuation("Hello!"), Some('!'));
        assert_eq!(find_relevant_punctuation("Hello?"), Some('?'));
    }

    #[test]
    fn test_find_relevant_punctuation_with_spaces() {
        // One space after punctuation
        assert_eq!(find_relevant_punctuation("Hello. "), Some('.'));
        assert_eq!(find_relevant_punctuation("Hello! "), Some('!'));
        // Two spaces after punctuation
        assert_eq!(find_relevant_punctuation("Hello.  "), Some('.'));
        // Three spaces exceeds MAX_WHITESPACE_LOOKBACK
        assert_eq!(find_relevant_punctuation("Hello.   "), None);
    }

    #[test]
    fn test_find_relevant_punctuation_continuation() {
        assert_eq!(find_relevant_punctuation("Hey,"), Some(','));
        assert_eq!(find_relevant_punctuation("Hey, "), Some(','));
        assert_eq!(find_relevant_punctuation("Note:"), Some(':'));
        assert_eq!(find_relevant_punctuation("well-"), Some('-'));
    }

    #[test]
    fn test_find_relevant_punctuation_newline() {
        assert_eq!(find_relevant_punctuation("Hello\n"), Some('\n'));
        assert_eq!(find_relevant_punctuation("Hello\r"), Some('\r'));
    }

    #[test]
    fn test_find_relevant_punctuation_edge_cases() {
        // Empty string
        assert_eq!(find_relevant_punctuation(""), None);
        // Only whitespace (within lookback)
        assert_eq!(find_relevant_punctuation("  "), None);
        // Regular word ending
        assert_eq!(find_relevant_punctuation("Hello"), Some('o'));
        assert_eq!(find_relevant_punctuation("Hello "), Some('o'));
    }
}
