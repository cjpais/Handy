use std::os::raw::c_int;

const MEDIA_REMOTE_COMMAND_PLAY: c_int = 0;
const MEDIA_REMOTE_COMMAND_PAUSE: c_int = 1;

unsafe extern "C" {
    fn media_remote_any_application_is_playing() -> c_int;
    fn media_remote_send_command(command: c_int) -> c_int;
}

pub fn any_application_is_playing() -> Result<bool, String> {
    match unsafe { media_remote_any_application_is_playing() } {
        1 => Ok(true),
        0 => Ok(false),
        -1 => Err("Failed to load MediaRemote framework".to_string()),
        -2 => Err("MediaRemote any-playing symbol was not found".to_string()),
        -3 => Err("MediaRemote any-playing query timed out".to_string()),
        status => Err(format!(
            "MediaRemote any-playing query returned unexpected status {status}"
        )),
    }
}

/// Resume media playback via Apple's global MediaRemote API.
///
/// This is intentionally global rather than app- or session-specific: macOS does not expose a
/// stable public API here for resuming the exact player Handy previously paused.
pub fn play() -> Result<(), String> {
    send_command(MEDIA_REMOTE_COMMAND_PLAY)
}

/// Pause media playback via Apple's global MediaRemote API.
///
/// On macOS this behaves more like a system-wide play/pause transport command than a targeted
/// pause for one specific application.
pub fn pause() -> Result<(), String> {
    send_command(MEDIA_REMOTE_COMMAND_PAUSE)
}

fn send_command(command: c_int) -> Result<(), String> {
    let status = unsafe { media_remote_send_command(command) };

    match status {
        0 => Ok(()),
        -1 => Err("Failed to load MediaRemote framework".to_string()),
        -2 => Err("MediaRemote command symbol was not found".to_string()),
        status => Err(format!(
            "MediaRemote command returned unexpected status {status}"
        )),
    }
}
