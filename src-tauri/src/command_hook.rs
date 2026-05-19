//! Command mode: runs a user-provided script with the transcript on stdin.
//!
//! When the user triggers the `command` shortcut, Handy records and transcribes
//! as normal, then — instead of pasting — pipes the raw transcript to an
//! executable `hooks/command` script in the app data directory. The script runs
//! detached; Handy does not read its output.

use crate::settings::get_settings;
use log::{debug, error, warn};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Candidate hook file names inside the `hooks/` directory, in resolution order.
///
/// Windows cannot execute an extensionless shebang script, so platform-specific
/// extensions are tried; the first existing file wins.
#[cfg(windows)]
const HOOK_CANDIDATES: &[&str] = &["command.cmd", "command.bat", "command.ps1", "command.exe"];
#[cfg(not(windows))]
const HOOK_CANDIDATES: &[&str] = &["command"];

/// True while a command hook process is running. Guards command mode against
/// re-entry: pressing the command shortcut again while set is a no-op.
static COMMAND_RUNNING: AtomicBool = AtomicBool::new(false);

/// Returns true while a command hook is executing.
pub fn is_command_running() -> bool {
    COMMAND_RUNNING.load(Ordering::SeqCst)
}

/// Returns the first existing command-hook file inside `hooks_dir`, or `None`.
pub fn resolve_hook(hooks_dir: &Path) -> Option<PathBuf> {
    HOOK_CANDIDATES
        .iter()
        .map(|name| hooks_dir.join(name))
        .find(|path| path.is_file())
}

/// Builds the `Command` that executes `hook_path`.
///
/// On Unix the script is run directly (it needs the executable bit + a shebang).
/// On Windows, `.cmd`/`.bat` are run via `cmd.exe /C` and `.ps1` via PowerShell,
/// because the OS cannot execute those file types directly. `.exe` runs directly.
fn build_command(hook_path: &Path) -> Command {
    #[cfg(windows)]
    {
        let ext = hook_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "ps1" => {
                let mut cmd = Command::new("powershell.exe");
                cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                    .arg(hook_path);
                return cmd;
            }
            "cmd" | "bat" => {
                let mut cmd = Command::new("cmd.exe");
                cmd.arg("/C").arg(hook_path);
                return cmd;
            }
            _ => {}
        }
    }
    Command::new(hook_path)
}

/// Context handed to the command hook as environment variables.
///
/// v1 populates `clipboard` only; `active_app` and `selected_text` are kept so
/// v2 can fill them without changing this interface.
pub struct CommandContext {
    pub active_app: Option<String>,
    pub clipboard: Option<String>,
    pub selected_text: Option<String>,
}

/// Builds the `(name, value)` environment pairs for the hook. Only fields that
/// are `Some` are emitted, so a script can distinguish "absent" from "empty".
pub fn build_env(ctx: &CommandContext) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(v) = &ctx.active_app {
        env.push(("HANDY_ACTIVE_APP".to_string(), v.clone()));
    }
    if let Some(v) = &ctx.clipboard {
        env.push(("HANDY_CLIPBOARD".to_string(), v.clone()));
    }
    if let Some(v) = &ctx.selected_text {
        env.push(("HANDY_SELECTED_TEXT".to_string(), v.clone()));
    }
    env
}

/// Runs the user's command hook with `transcript` on stdin, detached.
///
/// Non-blocking: the hook runs on its own process and a watcher thread waits
/// for it. Handy does not read the hook's stdout/stderr (they go to the null
/// device, which also prevents a full-pipe deadlock). The watcher kills the
/// process if it exceeds `command_hook_timeout_secs` and always clears the
/// busy flag and overlay when the process ends.
///
/// If no hook file exists, this just hides the overlay and returns.
pub fn run_command_hook(app: &AppHandle, transcript: String) {
    let hooks_dir = match crate::portable::app_data_dir(app) {
        Ok(dir) => dir.join("hooks"),
        Err(e) => {
            error!("Command hook: failed to resolve app data dir: {}", e);
            crate::overlay::hide_recording_overlay(app);
            return;
        }
    };

    let Some(hook_path) = resolve_hook(&hooks_dir) else {
        debug!("Command hook: no hook file in {:?}", hooks_dir);
        crate::overlay::hide_recording_overlay(app);
        return;
    };

    let ctx = CommandContext {
        active_app: None,
        clipboard: app.clipboard().read_text().ok(),
        selected_text: None,
    };

    let timeout = Duration::from_secs(get_settings(app).command_hook_timeout_secs);

    let mut command = build_command(&hook_path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in build_env(&ctx) {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            error!("Command hook: failed to spawn {:?}: {}", hook_path, e);
            crate::overlay::hide_recording_overlay(app);
            return;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(transcript.as_bytes()) {
            error!("Command hook: failed to write transcript to stdin: {}", e);
        }
        // stdin is dropped here, closing the pipe so the script sees EOF.
    }

    COMMAND_RUNNING.store(true, Ordering::SeqCst);
    crate::overlay::show_command_running_overlay(app);
    debug!("Command hook: started {:?}", hook_path);

    let app = app.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    debug!("Command hook: exited with {}", status);
                    break;
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        warn!("Command hook: timed out after {:?}, killing", timeout);
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    error!("Command hook: error waiting for process: {}", e);
                    break;
                }
            }
        }
        COMMAND_RUNNING.store(false, Ordering::SeqCst);
        crate::overlay::hide_recording_overlay(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hook_returns_none_when_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_hook(dir.path()), None);
    }

    #[test]
    fn resolve_hook_finds_an_existing_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let name = HOOK_CANDIDATES[0];
        let hook = dir.path().join(name);
        std::fs::write(&hook, b"#!/bin/sh\ncat\n").unwrap();
        assert_eq!(resolve_hook(dir.path()), Some(hook));
    }

    #[test]
    fn build_command_runs_unix_script_directly() {
        let cmd = build_command(Path::new("/data/hooks/command"));
        assert_eq!(cmd.get_program(), "/data/hooks/command");
    }

    #[test]
    fn build_env_emits_only_present_fields() {
        let ctx = CommandContext {
            active_app: None,
            clipboard: Some("hello".to_string()),
            selected_text: None,
        };
        assert_eq!(
            build_env(&ctx),
            vec![("HANDY_CLIPBOARD".to_string(), "hello".to_string())]
        );
    }

    #[test]
    fn build_env_is_empty_when_no_context() {
        let ctx = CommandContext {
            active_app: None,
            clipboard: None,
            selected_text: None,
        };
        assert!(build_env(&ctx).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn build_command_wraps_windows_script_types() {
        assert_eq!(
            build_command(Path::new("C:\\h\\command.ps1")).get_program(),
            "powershell.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.cmd")).get_program(),
            "cmd.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.bat")).get_program(),
            "cmd.exe"
        );
        assert_eq!(
            build_command(Path::new("C:\\h\\command.exe")).get_program(),
            "C:\\h\\command.exe"
        );
    }
}
