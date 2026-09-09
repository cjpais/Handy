//! Linux media control over MPRIS.
//!
//! Everything that shows up in the desktop's media controls — browsers,
//! Spotify, VLC, mpv, Rhythmbox — implements `org.mpris.MediaPlayer2.Player`,
//! so one mechanism covers the lot. `playerctl` is the pleasant way to reach
//! it and is used when installed; otherwise the same three calls are made with
//! `dbus-send`, which ships with D-Bus itself. This mirrors how
//! `managers::audio` tries wpctl, then pactl, then amixer for muting.
//!
//! Paused players are remembered with the backend that paused them, so the
//! resume takes the same route it came in on.

use std::process::{Command, Stdio};

use log::debug;

const PLAYERCTL_PREFIX: &str = "playerctl:";
const DBUS_PREFIX: &str = "dbus:";
const MPRIS_BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

pub fn pause_playing_media() -> Vec<String> {
    if let Some(players) = playerctl_pause_playing() {
        return players;
    }
    dbus_pause_playing()
}

pub fn resume_media(players: &[String]) {
    for player in players {
        if let Some(name) = player.strip_prefix(PLAYERCTL_PREFIX) {
            run("playerctl", &["--player", name, "play"]);
        } else if let Some(bus) = player.strip_prefix(DBUS_PREFIX) {
            dbus_call(bus, "Play");
        }
    }
}

/* ---------- playerctl ---------- */

/// Returns `None` when `playerctl` is not usable at all, which is the signal to
/// fall back to `dbus-send`. An empty vector means "it worked, nothing was
/// playing".
fn playerctl_pause_playing() -> Option<Vec<String>> {
    let listed = output("playerctl", &["--list-all"])?;

    let mut paused = Vec::new();
    for name in parse_playerctl_players(&listed) {
        let Some(status) = output("playerctl", &["--player", &name, "status"]) else {
            continue;
        };
        if status.trim() != "Playing" {
            continue;
        }
        if run("playerctl", &["--player", &name, "pause"]) {
            paused.push(format!("{PLAYERCTL_PREFIX}{name}"));
        }
    }
    Some(paused)
}

/// `playerctl --list-all` prints one player name per line, or the literal
/// "No players found" when there is nothing to list.
fn parse_playerctl_players(listed: &str) -> Vec<String> {
    listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "No players found")
        .map(str::to_string)
        .collect()
}

/* ---------- dbus-send fallback ---------- */

fn dbus_pause_playing() -> Vec<String> {
    let Some(names) = output(
        "dbus-send",
        &[
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.ListNames",
        ],
    ) else {
        debug!("Neither playerctl nor dbus-send is available; media control is unavailable");
        return Vec::new();
    };

    let mut paused = Vec::new();
    for bus in parse_dbus_mpris_names(&names) {
        if !dbus_is_playing(&bus) {
            continue;
        }
        if dbus_call(&bus, "Pause") {
            paused.push(format!("{DBUS_PREFIX}{bus}"));
        }
    }
    paused
}

/// Pulls the MPRIS bus names out of a `ListNames` reply, whose body is a list
/// of `string "…"` lines.
fn parse_dbus_mpris_names(reply: &str) -> Vec<String> {
    let mut names: Vec<String> = reply
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("string \"")?;
            let name = rest.strip_suffix('"')?;
            name.starts_with(MPRIS_BUS_PREFIX).then(|| name.to_string())
        })
        .collect();
    // ListNames also reports the unique names (":1.42") aliasing the same
    // players, but those never carry the MPRIS prefix. Duplicates would only
    // come from a malformed reply; drop them anyway so a player is never
    // paused twice.
    names.sort();
    names.dedup();
    names
}

fn dbus_is_playing(bus: &str) -> bool {
    let Some(reply) = output(
        "dbus-send",
        &[
            "--session",
            "--print-reply",
            &format!("--dest={bus}"),
            MPRIS_PATH,
            "org.freedesktop.DBus.Properties.Get",
            &format!("string:{MPRIS_PLAYER_INTERFACE}"),
            "string:PlaybackStatus",
        ],
    ) else {
        return false;
    };
    parse_dbus_playback_status(&reply).as_deref() == Some("Playing")
}

/// The reply body is `variant       string "Playing"`.
fn parse_dbus_playback_status(reply: &str) -> Option<String> {
    reply.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("variant")?.trim_start();
        let rest = rest.strip_prefix("string \"")?;
        rest.strip_suffix('"').map(str::to_string)
    })
}

fn dbus_call(bus: &str, method: &str) -> bool {
    run(
        "dbus-send",
        &[
            "--session",
            &format!("--dest={bus}"),
            MPRIS_PATH,
            &format!("{MPRIS_PLAYER_INTERFACE}.{method}"),
        ],
    )
}

/* ---------- process helpers ---------- */

/// Runs a command, returning its stdout when it succeeds. `None` covers both
/// "the tool is not installed" and "it failed", which callers treat the same.
fn output<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> Option<String> {
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

fn run<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_playerctl_player_list() {
        assert_eq!(
            parse_playerctl_players("spotify\nfirefox\n"),
            vec!["spotify".to_string(), "firefox".to_string()]
        );
    }

    #[test]
    fn treats_empty_playerctl_list_as_no_players() {
        assert!(parse_playerctl_players("No players found\n").is_empty());
        assert!(parse_playerctl_players("\n").is_empty());
    }

    #[test]
    fn parses_mpris_names_out_of_list_names_reply() {
        let reply = r#"method return time=1700000000.000000 sender=org.freedesktop.DBus -> destination=:1.99 serial=3 reply_serial=2
   array [
      string "org.freedesktop.DBus"
      string ":1.42"
      string "org.mpris.MediaPlayer2.spotify"
      string "org.mpris.MediaPlayer2.firefox.instance_1_23"
   ]
"#;
        assert_eq!(
            parse_dbus_mpris_names(reply),
            vec![
                "org.mpris.MediaPlayer2.firefox.instance_1_23".to_string(),
                "org.mpris.MediaPlayer2.spotify".to_string(),
            ]
        );
    }

    #[test]
    fn parses_playback_status_variant() {
        let reply = r#"method return time=1700000000.000000 sender=:1.42 -> destination=:1.99 serial=7 reply_serial=2
   variant       string "Playing"
"#;
        assert_eq!(
            parse_dbus_playback_status(reply).as_deref(),
            Some("Playing")
        );
    }

    #[test]
    fn ignores_a_reply_without_a_status() {
        assert!(parse_dbus_playback_status("method return time=1 sender=:1.4\n").is_none());
    }
}
