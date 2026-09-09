//! macOS media control.
//!
//! There is no public API to pause another application's media session on
//! macOS: `MediaRemote` is private and gated, and synthetic
//! `NSEventTypeSystemDefined` media-key events are no longer delivered to the
//! now-playing application. What is left is AppleScript, so this backend drives
//! the players that expose a scripting dictionary. Browser playback (YouTube
//! and friends) is out of reach and is deliberately left alone rather than
//! half-handled.
//!
//! Two things keep that cheap and quiet:
//!
//! - CoreAudio is asked first whether the default output device is running at
//!   all. Silence is the common case, and answering it costs one property read
//!   instead of an `osascript` launch — and, the first time round, instead of
//!   an automation permission prompt.
//! - Only players whose process is actually running are scripted, so a player
//!   that is merely installed is never launched or asked for permission.
//!
//! Each player gets its own script. AppleScript resolves an application's
//! terminology when the script is *compiled*, so one player with an unexpected
//! dictionary would otherwise take the whole run down with it.

use std::ffi::c_void;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use log::{debug, warn};

/// Cap on a single `osascript` run. AppleScript's own `with timeout` covers a
/// player that stops answering Apple events; this covers everything else,
/// including an automation permission prompt nobody is there to answer.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(5);

/// A player we can both interrogate and control through AppleScript.
struct ScriptablePlayer {
    /// Application name, which is also its bundle and executable name.
    app: &'static str,
    /// Condition, evaluated inside the tell block, that is true while playing.
    playing_condition: &'static str,
    /// Statement that pauses playback, inside the same tell block.
    pause: &'static str,
    /// Statement that resumes playback, inside the same tell block.
    resume: &'static str,
}

/// The players worth scripting. Apple's Music and TV share their dictionary
/// with Spotify's; VLC has no `pause` command (its `play` toggles, which is
/// safe here because the state is known first), and QuickTime Player is
/// document-scoped. Podcasts is absent on purpose: it ships no scripting
/// dictionary at all.
const PLAYERS: &[ScriptablePlayer] = &[
    ScriptablePlayer {
        app: "Music",
        playing_condition: "player state is playing",
        pause: "pause",
        resume: "play",
    },
    ScriptablePlayer {
        app: "Spotify",
        playing_condition: "player state is playing",
        pause: "pause",
        resume: "play",
    },
    ScriptablePlayer {
        app: "TV",
        playing_condition: "player state is playing",
        pause: "pause",
        resume: "play",
    },
    ScriptablePlayer {
        app: "VLC",
        playing_condition: "playing",
        pause: "play",
        resume: "play",
    },
    ScriptablePlayer {
        app: "QuickTime Player",
        playing_condition: "(count of documents) > 0 and playing of document 1",
        pause: "pause document 1",
        resume: "play document 1",
    },
];

pub fn pause_playing_media() -> Vec<String> {
    if !is_output_audio_active() {
        return Vec::new();
    }

    let running = running_players();
    if running.is_empty() {
        debug!("Audio is playing but no scriptable player is running");
        return Vec::new();
    }

    running
        .into_iter()
        .filter(|player| {
            let script = format!(
                "with timeout of 3 seconds\n\
                 tell application \"{app}\"\n\
                 \tif {condition} then\n\
                 \t\t{pause}\n\
                 \t\treturn true\n\
                 \tend if\n\
                 end tell\n\
                 end timeout\n\
                 return false",
                app = player.app,
                condition = player.playing_condition,
                pause = player.pause,
            );
            run_osascript(&script).is_some_and(|out| out.trim() == "true")
        })
        .map(|player| player.app.to_string())
        .collect()
}

pub fn resume_media(players: &[String]) {
    for app in players {
        let Some(player) = PLAYERS.iter().find(|player| player.app == app) else {
            continue;
        };
        let script = format!(
            "with timeout of 3 seconds\n\
             tell application \"{app}\"\n\
             \t{resume}\n\
             end tell\n\
             end timeout",
            app = player.app,
            resume = player.resume,
        );
        run_osascript(&script);
    }
}

/// The players that are currently running, in [`PLAYERS`] order.
///
/// Matching on the executable path rather than `pgrep -x` keeps this to a
/// single process spawn on the latency-sensitive pause path.
fn running_players() -> Vec<&'static ScriptablePlayer> {
    let Some(processes) = command_output("/bin/ps", &["-Ao", "comm="]) else {
        warn!("Failed to list running processes; skipping media control");
        return Vec::new();
    };

    PLAYERS
        .iter()
        .filter(|player| {
            let executable = format!("/{app}.app/Contents/MacOS/{app}", app = player.app);
            processes.lines().any(|line| line.ends_with(&executable))
        })
        .collect()
}

/// Runs a script, killing it if it outstays [`SCRIPT_TIMEOUT`]. Returns its
/// stdout, or `None` if it failed or had to be killed.
fn run_osascript(script: &str) -> Option<String> {
    let mut child = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| warn!("Failed to run osascript: {e}"))
        .ok()?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    debug!("Media control script exited with {status}");
                    return None;
                }
                break;
            }
            Ok(None) => {
                if started.elapsed() >= SCRIPT_TIMEOUT {
                    warn!("Media control script timed out; killing it");
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                warn!("Failed to wait on osascript: {e}");
                return None;
            }
        }
    }

    // The scripts print a single boolean at most, so the pipe cannot fill up
    // while we wait above.
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    Some(stdout)
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/* ---------- CoreAudio: is anything coming out of the speakers? ---------- */

type AudioObjectID = u32;

const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectID = 1;
/// `kAudioHardwarePropertyDefaultOutputDevice` ('dOut').
const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = 0x644F_7574;
/// `kAudioDevicePropertyDeviceIsRunningSomewhere` ('gone') — true while any
/// process on the system has the device running.
const K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE: u32 = 0x676F_6E65;
/// `kAudioObjectPropertyScopeGlobal` ('glob').
const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = 0x676C_6F62;
/// `kAudioObjectPropertyElementMain`.
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

/// Reads a fixed-size property off an audio object.
fn get_property<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
    let address = AudioObjectPropertyAddress {
        selector,
        scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
        element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
    };
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    // Safety: `value` is a `T` and `size` says so, which is the contract
    // AudioObjectGetPropertyData is documented against.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut value as *mut T as *mut c_void,
        )
    };
    (status == 0).then_some(value)
}

/// Whether the default output device is currently running audio for anyone.
///
/// This is a "is there any point in looking" gate rather than a decision on its
/// own: a video call also makes it true, but the AppleScript pass that follows
/// only ever pauses a player that reports itself as playing.
fn is_output_audio_active() -> bool {
    let Some(device) = get_property::<AudioObjectID>(
        K_AUDIO_OBJECT_SYSTEM_OBJECT,
        K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
    )
    .filter(|device| *device != 0) else {
        return false;
    };

    get_property::<u32>(device, K_AUDIO_DEVICE_PROPERTY_DEVICE_IS_RUNNING_SOMEWHERE)
        .is_some_and(|running| running != 0)
}
