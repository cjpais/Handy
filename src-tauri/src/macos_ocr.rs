use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

#[repr(C)]
pub struct OCRTextResponse {
    pub text: *mut c_char,
    pub success: c_int,
    pub error_message: *mut c_char,
}

extern "C" {
    fn macos_ocr_preflight_screen_capture_access() -> c_int;
    fn macos_ocr_request_screen_capture_access() -> c_int;
    fn macos_ocr_capture_frontmost_window_text() -> *mut OCRTextResponse;
    fn macos_ocr_free_response(response: *mut OCRTextResponse);
}

pub fn has_screen_capture_access() -> bool {
    unsafe { macos_ocr_preflight_screen_capture_access() == 1 }
}

pub fn request_screen_capture_access() -> bool {
    unsafe { macos_ocr_request_screen_capture_access() == 1 }
}

pub fn capture_frontmost_window_ocr_text() -> Result<String, String> {
    let response_ptr = unsafe { macos_ocr_capture_frontmost_window_text() };

    if response_ptr.is_null() {
        return Err("Null response from macOS OCR bridge".to_string());
    }

    let response = unsafe { &*response_ptr };

    let result = if response.success == 1 {
        if response.text.is_null() {
            Ok(String::new())
        } else {
            let text = unsafe { CStr::from_ptr(response.text) };
            Ok(text.to_string_lossy().into_owned())
        }
    } else {
        let error = if response.error_message.is_null() {
            "Unknown macOS OCR error".to_string()
        } else {
            let c_error = unsafe { CStr::from_ptr(response.error_message) };
            c_error.to_string_lossy().into_owned()
        };
        Err(error)
    };

    unsafe { macos_ocr_free_response(response_ptr) };
    result
}
