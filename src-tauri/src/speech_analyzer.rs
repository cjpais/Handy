//! FFI wrapper for Apple's SpeechAnalyzer API (Speech framework, macOS 26+).
//! The Swift side is compiled by build.rs (see swift/speech_analyzer.swift);
//! on unsupported platforms every call reports unavailable.

use serde::Deserialize;
use std::ffi::c_void;
use std::sync::Mutex;

pub const MODEL_ID: &str = "apple-speechanalyzer";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod ffi {
    use std::ffi::{c_char, c_float, c_int, c_void, CStr, CString};

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
        fn speech_analyzer_stream_start(
            locale_id: *const c_char,
            stream_out: *mut *mut c_void,
        ) -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_stream_feed(
            stream: *mut c_void,
            samples: *const c_float,
            sample_count: c_int,
        ) -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_stream_snapshot(stream: *mut c_void) -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_stream_finish(stream: *mut c_void) -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_stream_cancel(stream: *mut c_void) -> *mut SpeechAnalyzerResponse;
        fn free_speech_analyzer_stream(stream: *mut c_void);
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

    /// On success the response text is an optional diagnostic notice from the
    /// bridge (empty when there is nothing to report).
    pub fn prepare(locale: &str) -> Result<String, String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let ptr = unsafe { speech_analyzer_prepare(locale_cstr.as_ptr()) };
        consume_response(ptr)
    }

    pub fn transcribe(samples: &[f32], locale: &str) -> Result<String, String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let count = c_int::try_from(samples.len())
            .map_err(|_| "Audio too long for SpeechAnalyzer bridge".to_string())?;
        let ptr =
            unsafe { speech_analyzer_transcribe(samples.as_ptr(), count, locale_cstr.as_ptr()) };
        consume_response(ptr)
    }

    pub fn stream_start(locale: &str) -> Result<*mut c_void, String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let mut stream = std::ptr::null_mut();
        let response = unsafe { speech_analyzer_stream_start(locale_cstr.as_ptr(), &mut stream) };
        consume_response(response)?;
        if stream.is_null() {
            Err("SpeechAnalyzer returned a null stream".to_string())
        } else {
            Ok(stream)
        }
    }

    pub fn stream_feed(stream: *mut c_void, samples: &[f32]) -> Result<(), String> {
        let count = c_int::try_from(samples.len())
            .map_err(|_| "Audio chunk too long for SpeechAnalyzer bridge".to_string())?;
        let response = unsafe { speech_analyzer_stream_feed(stream, samples.as_ptr(), count) };
        consume_response(response).map(|_| ())
    }

    pub fn stream_snapshot(stream: *mut c_void) -> Result<String, String> {
        consume_response(unsafe { speech_analyzer_stream_snapshot(stream) })
    }

    pub fn stream_finish(stream: *mut c_void) -> Result<String, String> {
        consume_response(unsafe { speech_analyzer_stream_finish(stream) })
    }

    pub fn stream_cancel(stream: *mut c_void) -> Result<(), String> {
        consume_response(unsafe { speech_analyzer_stream_cancel(stream) }).map(|_| ())
    }

    pub unsafe fn stream_free(stream: *mut c_void) {
        unsafe { free_speech_analyzer_stream(stream) };
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod ffi {
    use std::ffi::c_void;

    pub fn available() -> bool {
        false
    }

    pub fn supported_locales() -> Result<Vec<String>, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn prepare(_locale: &str) -> Result<String, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn transcribe(_samples: &[f32], _locale: &str) -> Result<String, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn stream_start(_locale: &str) -> Result<*mut c_void, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn stream_feed(_stream: *mut c_void, _samples: &[f32]) -> Result<(), String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn stream_snapshot(_stream: *mut c_void) -> Result<String, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn stream_finish(_stream: *mut c_void) -> Result<String, String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub fn stream_cancel(_stream: *mut c_void) -> Result<(), String> {
        Err("SpeechAnalyzer is only available on Apple Silicon macOS".to_string())
    }

    pub unsafe fn stream_free(_stream: *mut c_void) {}
}

pub fn is_available() -> bool {
    ffi::available()
}

pub fn supported_locales() -> Result<Vec<String>, String> {
    ffi::supported_locales()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAnalyzerStreamResultEvent {
    pub revision: u64,
    pub elapsed_ms: u64,
    pub is_final: bool,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct SpeechAnalyzerStreamSnapshot {
    pub revision: u64,
    pub committed: String,
    pub tentative: String,
    pub events: Vec<SpeechAnalyzerStreamResultEvent>,
}

pub struct SpeechAnalyzerStream {
    handle: *mut c_void,
    finished: bool,
}

impl SpeechAnalyzerStream {
    pub fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        ffi::stream_feed(self.handle, samples)
    }

    pub fn snapshot(&self) -> Result<SpeechAnalyzerStreamSnapshot, String> {
        let json = ffi::stream_snapshot(self.handle)?;
        serde_json::from_str(&json)
            .map_err(|error| format!("Invalid SpeechAnalyzer stream snapshot: {error}"))
    }

    pub fn finish(&mut self) -> Result<String, String> {
        let result = ffi::stream_finish(self.handle);
        self.finished = true;
        result
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        let result = ffi::stream_cancel(self.handle);
        self.finished = true;
        result
    }
}

impl Drop for SpeechAnalyzerStream {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        if !self.finished {
            let _ = ffi::stream_cancel(self.handle);
        }
        unsafe { ffi::stream_free(self.handle) };
        self.handle = std::ptr::null_mut();
    }
}

/// Prepare assets for the locale and surface any diagnostic notice the bridge
/// returns — currently the analyzer preferring a different audio format than
/// Handy's 16 kHz mono Float32 feed, which engages per-chunk conversion on the
/// Swift side and should never happen in practice.
fn prepare_with_notice(locale: &str) -> Result<(), String> {
    let notice = ffi::prepare(locale)?;
    if !notice.is_empty() {
        log::warn!("SpeechAnalyzer: {notice}");
    }
    Ok(())
}

/// Handle held by the transcription manager while the SpeechAnalyzer "model"
/// is loaded. Batch calls create short-lived analyzers; live transcription
/// creates a separate [`SpeechAnalyzerStream`] while retaining this locale.
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
        prepare_with_notice(locale)?;
        Ok(Self {
            locale: Mutex::new(locale.to_string()),
        })
    }

    fn resolve_locale(&self, language: Option<&str>) -> String {
        language.map(str::to_string).unwrap_or_else(|| {
            self.locale
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        })
    }

    fn prepare_locale(&self, locale: &str) -> Result<(), String> {
        // AssetInventory may retire an unused asset between runs, and the user
        // may have changed languages since load. Rechecking is cheap when the
        // asset is installed and re-downloads it when the OS evicted it.
        prepare_with_notice(locale)?;
        *self
            .locale
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = locale.to_string();
        Ok(())
    }

    /// Transcribe 16 kHz mono f32 PCM. `language` overrides the load-time
    /// locale when the user changed it between load and dictation.
    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let locale = self.resolve_locale(language);
        self.prepare_locale(&locale)?;
        ffi::transcribe(samples, &locale)
    }

    /// Start a long-lived analyzer session that accepts incremental audio and
    /// exposes final plus volatile transcription snapshots.
    pub fn start_stream(&self, language: Option<&str>) -> Result<SpeechAnalyzerStream, String> {
        let locale = self.resolve_locale(language);
        self.prepare_locale(&locale)?;
        let handle = ffi::stream_start(&locale)?;
        Ok(SpeechAnalyzerStream {
            handle,
            finished: false,
        })
    }
}
