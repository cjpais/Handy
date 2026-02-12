// Apple Intelligence bridge.
//
// When the FoundationModels SDK is available, build.rs compiles a Swift bridge and sets
// `cfg(handy_apple_intelligence_swift)`.
// Otherwise we build without Swift and provide a Rust stub implementation.

#[cfg(handy_apple_intelligence_swift)]
mod swift {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int};

    #[repr(C)]
    pub struct AppleLLMResponse {
        pub response: *mut c_char,
        pub success: c_int,
        pub error_message: *mut c_char,
    }

    extern "C" {
        fn is_apple_intelligence_available() -> c_int;
        fn process_text_with_apple_llm(prompt: *const c_char, max_tokens: i32) -> *mut AppleLLMResponse;
        fn free_apple_llm_response(response: *mut AppleLLMResponse);
    }

    pub fn check_apple_intelligence_availability() -> bool {
        unsafe { is_apple_intelligence_available() == 1 }
    }

    pub fn process_text(prompt: &str, max_tokens: i32) -> Result<String, String> {
        let prompt_cstr = CString::new(prompt).map_err(|e| e.to_string())?;

        let response_ptr = unsafe { process_text_with_apple_llm(prompt_cstr.as_ptr(), max_tokens) };
        if response_ptr.is_null() {
            return Err("Null response from Apple LLM".to_string());
        }

        let response = unsafe { &*response_ptr };
        let result = if response.success == 1 {
            if response.response.is_null() {
                Ok(String::new())
            } else {
                let c_str = unsafe { CStr::from_ptr(response.response) };
                Ok(c_str.to_string_lossy().into_owned())
            }
        } else {
            let error_msg = if !response.error_message.is_null() {
                let c_str = unsafe { CStr::from_ptr(response.error_message) };
                c_str.to_string_lossy().into_owned()
            } else {
                "Unknown error".to_string()
            };
            Err(error_msg)
        };

        unsafe { free_apple_llm_response(response_ptr) };
        result
    }
}

#[cfg(handy_apple_intelligence_swift)]
pub use swift::{check_apple_intelligence_availability, process_text};

#[cfg(not(handy_apple_intelligence_swift))]
pub fn check_apple_intelligence_availability() -> bool {
    false
}

#[cfg(not(handy_apple_intelligence_swift))]
pub fn process_text(_prompt: &str, _max_tokens: i32) -> Result<String, String> {
    Err("Apple Intelligence is not available in this build (SDK requirement not met).".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability() {
        let available = check_apple_intelligence_availability();
        println!("Apple Intelligence available: {}", available);
    }
}

