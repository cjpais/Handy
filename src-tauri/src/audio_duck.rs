#[cfg(target_os = "macos")]
use log::{debug, error};
use std::os::raw::{c_float, c_int};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn audio_duck_start(duck_level: c_float) -> c_int;
    fn audio_duck_stop() -> c_int;
    fn audio_duck_is_active() -> c_int;
}

#[cfg(target_os = "macos")]
pub fn start(duck_level: f32) -> bool {
    let result = unsafe { audio_duck_start(duck_level) };
    if result >= 0 {
        debug!("Audio duck started: {} processes tapped at level {}", result, duck_level);
        true
    } else {
        error!("Audio duck start failed: {}", result);
        false
    }
}

#[cfg(target_os = "macos")]
pub fn stop() {
    let result = unsafe { audio_duck_stop() };
    if result == 0 {
        debug!("Audio duck stopped");
    } else {
        error!("Audio duck stop failed: {}", result);
    }
}

#[cfg(target_os = "macos")]
pub fn is_active() -> bool {
    unsafe { audio_duck_is_active() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn start(_duck_level: f32) -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn stop() {}

#[cfg(not(target_os = "macos"))]
pub fn is_active() -> bool {
    false
}
