use std::os::raw::c_int;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

const MEDIA_REMOTE_COMMAND_PLAY: c_int = 0;
const MEDIA_REMOTE_COMMAND_PAUSE: c_int = 1;

unsafe extern "C" {
    fn media_remote_send_command(command: c_int) -> c_int;
    fn media_remote_send_play_pause_key() -> c_int;
}

#[cfg(target_os = "macos")]
const MEDIA_STATE_ADAPTER: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/libhandy_media_state_adapter.dylib"
));

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

/// Toggle media playback by posting the same system-defined HID event as the play/pause media key.
///
/// This is still global, but in live testing it controlled the active macOS media session more
/// reliably than `MRMediaRemoteSendCommand(Pause)`.
pub fn play_pause_key() -> Result<(), String> {
    let status = unsafe { media_remote_send_play_pause_key() };

    match status {
        0 => Ok(()),
        -3 => Err("Failed to create CoreGraphics event source for media key".to_string()),
        -4 => Err("Failed to create CoreGraphics media key events".to_string()),
        status => Err(format!(
            "Media key command returned unexpected status {status}"
        )),
    }
}

/// Query active playback through MediaRemote while hosted inside `/usr/bin/perl`.
///
/// macOS 15.4+ gates direct MediaRemote state access by process entitlement. Loading this tiny
/// adapter into Apple's Perl process keeps the state query native while avoiding `osascript`.
#[cfg(target_os = "macos")]
pub fn private_is_playing() -> Result<bool, String> {
    static ADAPTER_PATH: OnceLock<Result<PathBuf, String>> = OnceLock::new();

    let adapter_path = ADAPTER_PATH
        .get_or_init(write_media_state_adapter)
        .as_ref()
        .map_err(|err| err.clone())?;

    let perl = r#"
use strict;
use warnings;
use DynaLoader;
my $path = shift @ARGV or die "missing adapter path\n";
my $handle = DynaLoader::dl_load_file($path, 0) or die "failed to load adapter: $path\n";
my $symbol = DynaLoader::dl_find_symbol($handle, "handy_media_is_playing") or die "symbol not found\n";
DynaLoader::dl_install_xsub("main::handy_media_is_playing", $symbol);
handy_media_is_playing();
"#;

    let output = Command::new("/usr/bin/perl")
        .args(["-e", perl, "--"])
        .arg(adapter_path)
        .output()
        .map_err(|err| format!("Failed to run MediaRemote state adapter: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "MediaRemote state adapter failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    match String::from_utf8_lossy(&output.stdout).trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!(
            "Unexpected MediaRemote state adapter output: {other}"
        )),
    }
}

#[cfg(target_os = "macos")]
fn write_media_state_adapter() -> Result<PathBuf, String> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "handy-media-state-adapter-{}.dylib",
        env!("CARGO_PKG_VERSION")
    ));

    let should_write = std::fs::read(&path)
        .map(|existing| existing != MEDIA_STATE_ADAPTER)
        .unwrap_or(true);

    if should_write {
        std::fs::write(&path, MEDIA_STATE_ADAPTER)
            .map_err(|err| format!("Failed to write MediaRemote state adapter: {err}"))?;
    }

    Ok(path)
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
