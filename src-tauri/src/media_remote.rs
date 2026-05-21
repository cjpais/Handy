#[cfg(target_os = "macos")]
use log::debug;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
fn send_command(cmd: &str) -> bool {
    // nowplaying-cli uses MediaRemote via Perl adapter (com.apple.perl5 bundle ID)
    // to bypass macOS 15.4+ entitlement restrictions. Dedicated pause/play commands.
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
pub fn is_playing() -> bool {
    Command::new("/opt/homebrew/bin/nowplaying-cli")
        .args(["get", "playbackRate"])
        .output()
        .map(|o| {
            let rate = String::from_utf8_lossy(&o.stdout).trim().to_string();
            rate == "1" || rate.starts_with("1.")
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn pause() -> bool {
    send_command("pause")
}

#[cfg(target_os = "macos")]
pub fn play() -> bool {
    send_command("play")
}

#[cfg(not(target_os = "macos"))]
pub fn pause() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn play() -> bool {
    false
}
