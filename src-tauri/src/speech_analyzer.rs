//! FFI wrapper for Apple's SpeechAnalyzer API (Speech framework, macOS 26+).
//! The Swift side is compiled by build.rs (see swift/speech_analyzer.swift);
//! on unsupported platforms every call reports unavailable.

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

/// Handle held by the transcription manager while the SpeechAnalyzer "model"
/// is loaded. The Swift side is stateless per call, so this only carries the
/// resolved locale.
pub struct SpeechAnalyzerEngine {
    locale: String,
}

impl SpeechAnalyzerEngine {
    /// Verify availability and trigger the OS-managed asset download for the
    /// locale so the first transcription doesn't stall on it.
    pub fn load(locale: &str) -> Result<Self, String> {
        if !ffi::available() {
            return Err("SpeechAnalyzer requires macOS 26 or newer.".to_string());
        }
        ffi::prepare(locale)?;
        Ok(Self {
            locale: locale.to_string(),
        })
    }

    /// Transcribe 16 kHz mono f32 PCM. `language` overrides the load-time
    /// locale when the user changed it between load and dictation.
    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        ffi::transcribe(samples, language.unwrap_or(&self.locale))
    }
}
