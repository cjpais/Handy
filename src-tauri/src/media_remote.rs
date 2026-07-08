#[cfg(target_os = "macos")]
use log::debug;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
fn send_command(cmd: &str) -> bool {
    Command::new("/opt/homebrew/bin/nowplaying-cli")
        .arg(cmd)
        .output()
        .map(|o| {
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                debug!("nowplaying-cli {} failed: {}", cmd, err.trim());
            }
            o.status.success()
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn pause() -> bool {
    send_command("pause")
}

#[cfg(not(target_os = "macos"))]
pub fn pause() -> bool {
    false
}
