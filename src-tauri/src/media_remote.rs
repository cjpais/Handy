#[cfg(target_os = "macos")]
use log::debug;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
fn send_command(command: u32) -> bool {
    // Pre-compiled Swift binary that calls MRMediaRemoteSendCommand via dlopen.
    // Command 0 = Play, 1 = Pause (dedicated, NOT toggle).
    // Binary location: ~/bin/media-remote-cmd (compiled once from Swift source).
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users".to_string());
    let binary = format!("{}/bin/media-remote-cmd", home);

    Command::new(&binary)
        .arg(command.to_string())
        .output()
        .map(|o| {
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                debug!("media-remote-cmd failed: {}", err.trim());
            }
            o.status.success()
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn pause() -> bool {
    send_command(1)
}

#[cfg(target_os = "macos")]
pub fn play() -> bool {
    send_command(0)
}

#[cfg(not(target_os = "macos"))]
pub fn pause() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn play() -> bool {
    false
}
