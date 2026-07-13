//! FFI wrapper for Apple's SpeechAnalyzer API (Speech framework, macOS 26+).
//! The Swift side is compiled by build.rs (see swift/speech_analyzer.swift);
//! on unsupported platforms every call reports unavailable.

use crate::managers::model::DownloadProgress;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter};

pub const MODEL_ID: &str = "apple-speechanalyzer";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod ffi {
    use std::ffi::{c_char, c_double, c_float, c_int, c_void, CStr, CString};

    #[repr(C)]
    pub struct SpeechAnalyzerResponse {
        pub text: *mut c_char,
        pub success: c_int,
        pub error_message: *mut c_char,
    }

    type ProgressCallback = unsafe extern "C" fn(c_double, *mut c_void);

    struct ProgressContext<'a> {
        callback: &'a (dyn Fn(f64) + Sync),
    }

    unsafe extern "C" fn report_progress(fraction: c_double, context: *mut c_void) {
        if let Some(context) = (context as *const ProgressContext<'_>).as_ref() {
            (context.callback)(fraction);
        }
    }

    extern "C" {
        fn is_speech_analyzer_available() -> c_int;
        fn speech_analyzer_supported_locales() -> *mut SpeechAnalyzerResponse;
        fn speech_analyzer_prepare(
            locale_id: *const c_char,
            progress_callback: Option<ProgressCallback>,
            progress_context: *mut c_void,
        ) -> *mut SpeechAnalyzerResponse;
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

    pub fn prepare(locale: &str, progress_callback: &(dyn Fn(f64) + Sync)) -> Result<(), String> {
        let locale_cstr = CString::new(locale).map_err(|e| e.to_string())?;
        let context = ProgressContext {
            callback: progress_callback,
        };
        let ptr = unsafe {
            speech_analyzer_prepare(
                locale_cstr.as_ptr(),
                Some(report_progress),
                (&context as *const ProgressContext<'_>).cast_mut().cast(),
            )
        };
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

    pub fn prepare(_locale: &str, _progress_callback: &(dyn Fn(f64) + Sync)) -> Result<(), String> {
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
    app_handle: AppHandle,
    model_id: String,
}

impl SpeechAnalyzerEngine {
    fn prepare_assets(app_handle: &AppHandle, model_id: &str, locale: &str) -> Result<(), String> {
        let installation_started = AtomicBool::new(false);
        let report_progress = |fraction: f64| {
            installation_started.store(true, Ordering::Release);
            let total = 1_000_u64;
            let downloaded = (fraction.clamp(0.0, 1.0) * total as f64).round() as u64;
            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total,
                    percentage: fraction.clamp(0.0, 1.0) * 100.0,
                },
            );
        };

        let result = ffi::prepare(locale, &report_progress);
        if installation_started.load(Ordering::Acquire) {
            match &result {
                Ok(()) => {
                    let _ = app_handle.emit("model-preparation-complete", model_id);
                }
                Err(_) => {
                    let _ = app_handle.emit("model-preparation-failed", model_id);
                }
            }
        }
        result
    }

    /// Verify availability and trigger the OS-managed asset download for the
    /// locale so the first transcription doesn't stall on it.
    pub fn load(locale: &str, app_handle: AppHandle, model_id: &str) -> Result<Self, String> {
        if !ffi::available() {
            return Err(
                "SpeechTranscriber requires a compatible Apple Silicon Mac running macOS 26 or newer."
                    .to_string(),
            );
        }
        Self::prepare_assets(&app_handle, model_id, locale)?;
        Ok(Self {
            locale: Mutex::new(locale.to_string()),
            app_handle,
            model_id: model_id.to_string(),
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
        // asset is installed and gives progress when the OS needs to fetch it.
        Self::prepare_assets(&self.app_handle, &self.model_id, &locale)?;
        *self
            .locale
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = locale.clone();
        ffi::transcribe(samples, &locale)
    }
}
