//! User-provided transcription hook: an executable at
//! `<app data dir>/hooks/transcription` that receives the text about to be
//! pasted on stdin and whose stdout is pasted instead.

use anyhow::Context;
use log::{debug, error, info};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// How long a hook may run before it's killed and the original text is used,
/// so a hung script can't leave the pipeline stuck in "transcribing".
const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs the transcription hook if one exists and returns its output. Returns
/// `text` unchanged when there's no hook, the text is empty, or the hook fails.
///
/// The hook process is killed if this future is dropped, so callers can
/// abandon it on cancellation.
pub async fn apply(app: &AppHandle, text: String) -> String {
    if text.is_empty() {
        return text;
    }

    let hook = match crate::portable::app_data_dir(app) {
        Ok(dir) => dir.join("hooks").join("transcription"),
        Err(e) => {
            error!(
                "Failed to resolve app data directory for transcription hook: {}",
                e
            );
            return text;
        }
    };

    if !hook.is_file() {
        return text;
    }

    match tokio::time::timeout(HOOK_TIMEOUT, run(&hook, &text)).await {
        Ok(Ok(output)) => {
            if output.is_empty() {
                info!("Transcription hook produced no output; nothing will be pasted");
            }
            output
        }
        Ok(Err(e)) => {
            error!("Transcription hook failed, using original text: {:#}", e);
            text
        }
        Err(_) => {
            error!(
                "Transcription hook timed out after {:?}, using original text",
                HOOK_TIMEOUT
            );
            text
        }
    }
}

async fn run(hook: &Path, text: &str) -> anyhow::Result<String> {
    let mut child = Command::new(hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to spawn")?;

    let mut stdin = child.stdin.take().context("stdin was not piped")?;
    // Feed stdin while collecting output, so a hook that writes before it has
    // read all of its input can't deadlock on a full pipe.
    let write_input = async move {
        // A hook is free to exit without reading its input.
        if let Err(e) = stdin.write_all(text.as_bytes()).await {
            debug!("Transcription hook did not read all of its input: {}", e);
        }
        // Dropping stdin closes the pipe so the hook sees EOF.
    };
    let (_, output) = tokio::join!(write_input, child.wait_with_output());
    let output = output.context("failed to wait for exit")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !output.status.success() {
        anyhow::bail!("exited with {}, stderr: {:?}", output.status, stderr);
    }
    if !stderr.is_empty() {
        debug!("Transcription hook stderr: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(strip_trailing_newline(&stdout).to_string())
}

/// Drops a single trailing newline, as `echo` and `print` add one and pasting
/// it into a terminal would submit the line.
fn strip_trailing_newline(s: &str) -> &str {
    s.strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_one_trailing_newline() {
        assert_eq!(strip_trailing_newline("hello\n"), "hello");
        assert_eq!(strip_trailing_newline("hello\r\n"), "hello");
        assert_eq!(strip_trailing_newline("hello\n\n"), "hello\n");
        assert_eq!(strip_trailing_newline("hello"), "hello");
        assert_eq!(strip_trailing_newline(""), "");
    }

    #[cfg(unix)]
    fn write_hook(dir: &Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("transcription");
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_pipes_text_through_hook() {
        let dir = tempfile::tempdir().unwrap();
        let hook = write_hook(dir.path(), "#!/bin/sh\necho \"> $(cat)\"\n");
        assert_eq!(run(&hook, "hi there").await.unwrap(), "> hi there");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_accepts_hook_that_ignores_input() {
        let dir = tempfile::tempdir().unwrap();
        let hook = write_hook(dir.path(), "#!/bin/sh\nprintf replaced\n");
        let input = "x".repeat(1 << 20);
        assert_eq!(run(&hook, &input).await.unwrap(), "replaced");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_reports_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let hook = write_hook(dir.path(), "#!/bin/sh\necho oops >&2\nexit 3\n");
        let err = run(&hook, "hi").await.unwrap_err().to_string();
        assert!(err.contains("oops"), "{}", err);
    }
}
