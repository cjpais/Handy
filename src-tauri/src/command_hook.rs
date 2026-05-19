//! Command mode: runs a user-provided script with the transcript on stdin.
//!
//! When the user triggers the `command` shortcut, Handy records and transcribes
//! as normal, then — instead of pasting — pipes the raw transcript to an
//! executable `hooks/command` script in the app data directory. The script runs
//! detached; Handy does not read its output.

use std::path::{Path, PathBuf};

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
}
