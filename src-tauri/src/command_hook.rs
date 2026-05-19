//! Command mode: runs a user-provided script with the transcript on stdin.
//!
//! When the user triggers the `command` shortcut, Handy records and transcribes
//! as normal, then — instead of pasting — pipes the raw transcript to an
//! executable `hooks/command` script in the app data directory. The script runs
//! detached; Handy does not read its output.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Candidate hook file names inside the `hooks/` directory, in resolution order.
///
/// Windows cannot execute an extensionless shebang script, so platform-specific
/// extensions are tried; the first existing file wins.
#[cfg(windows)]
const HOOK_CANDIDATES: &[&str] = &["command.cmd", "command.bat", "command.ps1", "command.exe"];
#[cfg(not(windows))]
const HOOK_CANDIDATES: &[&str] = &["command"];

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
