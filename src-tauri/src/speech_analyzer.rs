//! FFI wrapper for Apple's SpeechAnalyzer API (Speech framework, macOS 26+).
//! The Swift side is compiled by build.rs (see swift/speech_analyzer.swift);
//! on unsupported platforms every call reports unavailable.

use std::sync::Mutex;

pub const MODEL_ID: &str = "apple-speechanalyzer";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod ffi {
    use std::ffi::{c_char, c_float, c_int, CStr, CString};

    #[repr(C)]
    pub struct SpeechAnalyzerResponse {
        pub text: *mut c_char,
        pub success: c_int,
        pub error_message: *mut c_char,
    }

    extern "C" {
        fn is_speech_analyzer_available() -> c_int;
        fn speech_analyzer_supported_locales() -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_prepare(locale_id: *const c_char) -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_transcribe(
            samples: *const c_float,
            sample_count: c_int,
            locale_id: *const c_char,
        ) -> *mut SpeechAnalyzerResponse;
        fn free_speech_analyzer_response(response: *mut SpeechAnalyzerResponse);
    }

    pub fn available() -> bool {
        unsafe { is_speech_analyzer_available() == 1 }
    }

    pub fn supported_locales() -> Result<Vec<String>, String> {
        let response = consume_response(unsafe { speech_analyzer_supported_locales() })?;
        Ok(response
            .lines()
            .map(str::trim)
            .filter(|locale| !locale.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Consume a Swift-allocated response into a Result, always freeing it.
    fn consume_response(ptr: *mut SpeechAnalyzerResponse) -> Result<String, String> {
        if ptr.is_null() {
            return Err("Null response from SpeechAnalyzer".to_string());
        }
        let response = unsafe { &*ptr };
        let result = if response.success == 1 {
            if response.text.is_null() {
                Ok(String::new())
            } else {
                Ok(unsafe { CStr::from_ptr(response.text) }
                    .to_string_lossy()
                    .into_owned())
            }
        } else {
            let error = if response.error_message.is_null() {
                "Unknown SpeechAnalyzer error".to_string()
            } else {
                unsafe { CStr::from_ptr(response.error_message) }
                    .to_string_lossy()
                    .into_owned()
            };
            Err(error)
        };
        unsafe { free_speech_analyzer_response(ptr) };
        result
    }

    pub fn prepare(locale: &str) -> Result<(), String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let ptr = unsafe { speech_analyzer_prepare(locale_cstr.as_ptr()) };
        consume_response(ptr).map(|_| ())
    }

    pub fn transcribe(samples: &[f32], locale: &str) -> Result<String, String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let count = c_int::try_from(samples.len())
            .map_err(|_| "Audio too long for SpeechAnalyzer bridge".to_string())?;
        let ptr =
            unsafe { speech_analyzer_transcribe(samples.as_ptr(), count, locale_cstr.as_ptr()) };
        consume_response(ptr)
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod ffi {
    pub fn available() -> bool {
        false
    }

    pub fn supported_locales() -> Result<Vec<String>, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn prepare(_locale: &str) -> Result<(), String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn transcribe(_samples: &[f32], _locale: &str) -> Result<String, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }
}

pub fn is_available() -> bool {
    ffi::available()
}

pub fn supported_locales() -> Result<Vec<String>, String> {
    ffi::supported_locales()
}

/// Handle held by the transcription manager while the SpeechAnalyzer "model"
/// is loaded. The Swift side is stateless per call, so this only carries the
/// resolved locale.
pub struct SpeechAnalyzerEngine {
    locale: Mutex<String>,
}

impl SpeechAnalyzerEngine {
    /// Verify availability and trigger the OS-managed asset download for the
    /// locale (blocking; the model card shows its loading state meanwhile) so
    /// the first transcription doesn't stall on it.
    pub fn load(locale: &str) -> Result<Self, String> {
        if !ffi::available() {
            return Err(
                "Apple speech recognition requires a compatible Apple Silicon Mac running macOS 26 or newer."
                    .to_string(),
            );
        }
        ffi::prepare(locale)?;
        Ok(Self {
            locale: Mutex::new(locale.to_string()),
        })
    }

    /// Transcribe 16 kHz mono f32 PCM. `language` overrides the load-time
    /// locale when the user changed it between load and dictation.
    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let locale = language.map(str::to_string).unwrap_or_else(|| {
            self.locale
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        });

        // AssetInventory may retire an unused asset between runs, and the user
        // may have changed languages since load. Rechecking is cheap when the
        // asset is installed and re-downloads it when the OS evicted it.
        ffi::prepare(&locale)?;
        *self
            .locale
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = locale.clone();
        ffi::transcribe(samples, &locale).map(|text| capitalize_sentence_starts(&text))
    }
}

/// SpeechTranscriber punctuates well but leaves sentence starts lowercase
/// (its `TranscriptionOption` set has no casing knob), so uppercase the first
/// letter of the text and of each sentence after `.`, `!`, `?`, or `…`.
/// A no-op for uncased scripts and for already-capitalized letters.
fn capitalize_sentence_starts(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut at_sentence_start = true;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if at_sentence_start && c.is_alphabetic() {
            result.extend(c.to_uppercase());
            at_sentence_start = false;
            continue;
        }
        // Punctuation counts as a sentence end only when followed by
        // whitespace (or end of text) so decimals like "3.5" stay intact.
        // Quotes and brackets pass through without consuming the sentence
        // start; anything else (digits, mid-sentence letters) consumes it.
        if matches!(c, '.' | '!' | '?' | '…')
            && chars.peek().is_none_or(|next| next.is_whitespace())
        {
            at_sentence_start = true;
        } else if !c.is_whitespace() && !matches!(c, '"' | '\'' | '“' | '”' | '(' | '[') {
            at_sentence_start = false;
        }
        result.push(c);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::capitalize_sentence_starts;

    #[test]
    fn capitalizes_first_letter_and_after_sentence_punctuation() {
        assert_eq!(
            capitalize_sentence_starts("hello there. how are you? great! okay"),
            "Hello there. How are you? Great! Okay"
        );
    }

    #[test]
    fn ignores_decimals_and_leaves_numbers_alone() {
        assert_eq!(
            capitalize_sentence_starts("it costs 3.5 dollars. 6 people paid"),
            "It costs 3.5 dollars. 6 people paid"
        );
    }

    #[test]
    fn capitalizes_through_opening_quotes() {
        assert_eq!(
            capitalize_sentence_starts("she said. \"hello world\""),
            "She said. \"Hello world\""
        );
    }
}
