//! Context-aware text processing using macOS Accessibility APIs.
//!
//! This module reads the character before the cursor in the currently focused
//! text field to determine appropriate capitalization for inserted text.

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

/// Result of context analysis including both capitalization and spacing.
#[derive(Debug, Clone)]
pub struct ContextResult {
    /// Whether to capitalize, lowercase, or leave unchanged
    pub hint: CapitalizationHint,
    /// Whether to prepend a space before the text
    pub needs_leading_space: bool,
}

/// Analyze the context and return capitalization hint plus spacing info.
///
/// This function:
/// 1. Reads text before the cursor
/// 2. Scans back through whitespace (up to MAX_WHITESPACE_LOOKBACK chars)
/// 3. Determines capitalization based on the punctuation found
/// 4. Determines if a leading space is needed
///
/// ## Scenarios handled:
/// - `"Hello."` → Capitalize, add leading space
/// - `"Hello. "` → Capitalize, no leading space needed
/// - `"Hey,"` → Lowercase, add leading space
/// - `"Hey, "` → Lowercase, no leading space needed
/// - `"Hello"` → Unknown (mid-word), no change
/// - Empty/beginning → Capitalize (start of text)
fn analyze_context() -> ContextResult {
    let text = match get_text_before_cursor() {
        Some(t) => t,
        None => {
            // Empty field, can't read, or at beginning - capitalize by default
            debug!("No text before cursor, defaulting to Capitalize");
            return ContextResult {
                hint: CapitalizationHint::Capitalize,
                needs_leading_space: false,
            };
        }
    };

    // Check if there's already a space at the end
    let has_trailing_space = text.ends_with(' ') || text.ends_with('\t');

    // Find the relevant punctuation by looking back through whitespace
    let relevant_char = find_relevant_punctuation(&text);

    match relevant_char {
        None => {
            // Reached beginning of text or exceeded lookback - capitalize
            debug!("No relevant punctuation found, defaulting to Capitalize");
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                needs_leading_space: false,
            }
        }
        Some(c) if CAPITALIZE_AFTER.contains(&c) => {
            debug!(
                "Found sentence-ending '{}', Capitalize, needs_space={}",
                c, !has_trailing_space
            );
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                needs_leading_space: !has_trailing_space,
            }
        }
        Some(c) if c == '\n' || c == '\r' => {
            // Newline - capitalize, no space needed (newline acts as separator)
            debug!("Found newline, Capitalize, no space needed");
            ContextResult {
                hint: CapitalizationHint::Capitalize,
                needs_leading_space: false,
            }
        }
        Some(c) if LOWERCASE_AFTER.contains(&c) => {
            debug!(
                "Found continuation '{}', Lowercase, needs_space={}",
                c, !has_trailing_space
            );
            ContextResult {
                hint: CapitalizationHint::Lowercase,
                needs_leading_space: !has_trailing_space,
            }
        }
        Some(c) => {
            // Some other character (letter, number, etc.)
            // Don't change capitalization, but might need space
            debug!(
                "Found other char '{}', Unknown, needs_space={}",
                c, !has_trailing_space
            );
            ContextResult {
                hint: CapitalizationHint::Unknown,
                needs_leading_space: !has_trailing_space,
            }
        }
    }
}

/// Legacy function for getting just the hint (used by tests).
pub fn get_capitalization_hint() -> CapitalizationHint {
    analyze_context().hint
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
/// On non-macOS platforms or if context cannot be read, returns text unchanged.
///
/// This function:
/// 1. Analyzes the text before the cursor
/// 2. Determines appropriate capitalization (capitalize/lowercase/unchanged)
/// 3. Adds a leading space if needed (no space after punctuation)
///
/// ## Examples:
/// - After `"Hello."` with input `"world"` → `" World"` (space + capitalize)
/// - After `"Hello. "` with input `"world"` → `"World"` (capitalize, space exists)
/// - After `"Hey,"` with input `"What"` → `" what"` (space + lowercase)
/// - After `"Hey, "` with input `"What"` → `"what"` (lowercase, space exists)
pub fn apply_context_aware_capitalization(text: &str) -> String {
    let context = analyze_context();
    let capitalized = apply_capitalization(text, context.hint);

    let result = if context.needs_leading_space {
        format!(" {}", capitalized)
    } else {
        capitalized
    };

    info!(
        "Context-aware capitalization: hint={:?}, needs_space={}, input='{}', output='{}'",
        context.hint, context.needs_leading_space, text, result
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
        assert_eq!(
            apply_capitalization("(note: Hello)", CapitalizationHint::Lowercase),
            "(note: hello)"
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
}
