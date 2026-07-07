//! Routing the native log sink into the [`log`] crate facade.
//!
//! Call [`init_logging`] once at process startup (before loading any model) to
//! send every library and ggml diagnostic through `log` (and thus to whatever
//! `env_logger`/`tracing-log`/etc. the application installed). This matches the
//! C contract: `transcribe_log_set` is a once-at-startup global.

use std::ffi::CStr;

use transcribe_cpp_sys as sys;

/// The C log callback: map the level and forward the message to `log`.
extern "C" fn log_trampoline(
    level: sys::transcribe_log_level,
    msg: *const std::os::raw::c_char,
    _userdata: *mut std::os::raw::c_void,
) {
    if msg.is_null() {
        return;
    }
    // SAFETY: `msg` is a NUL-terminated string valid for this call.
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    let text = text.trim_end_matches('\n');
    use sys::transcribe_log_level as L;
    let level = match level {
        L::TRANSCRIBE_LOG_LEVEL_ERROR => log::Level::Error,
        L::TRANSCRIBE_LOG_LEVEL_WARN => log::Level::Warn,
        L::TRANSCRIBE_LOG_LEVEL_DEBUG => log::Level::Debug,
        // INFO and CONT (continuation fragments) both surface at info.
        L::TRANSCRIBE_LOG_LEVEL_INFO | L::TRANSCRIBE_LOG_LEVEL_CONT => log::Level::Info,
        // NONE or any unknown level: drop.
        _ => return,
    };
    log::log!(target: "transcribe_cpp", level, "{text}");
}

/// Route native + ggml diagnostics into the [`log`] crate. Call once at
/// startup, before loading models.
pub fn init_logging() {
    // SAFETY: install-once-at-startup is the supported usage model.
    unsafe { sys::transcribe_log_set(Some(log_trampoline), std::ptr::null_mut()) };
}

/// Silence the native library entirely (drops both library and ggml messages,
/// including the stderr default). Call once at startup.
pub fn disable_logging() {
    unsafe { sys::transcribe_log_set(None, std::ptr::null_mut()) };
}
