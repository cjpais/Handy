//! Optional user hook: `<app data dir>/hooks/transcription` receives the text
//! about to be pasted on stdin, and its stdout is pasted instead.

use anyhow::Context;
use log::error;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Hung hooks are killed so they can't leave the pipeline stuck in "transcribing".
const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Returns the hook's output, or `text` unchanged if there's no hook or it
/// fails. Dropping the future kills the hook, so callers can cancel it.
pub async fn apply(app: &AppHandle, text: String) -> String {
    let Ok(dir) = crate::portable::app_data_dir(app) else {
        return text;
    };
    let hook = dir.join("hooks").join("transcription");
    if text.is_empty() || !hook.is_file() {
        return text;
    }

    run(&hook, &text).await.unwrap_or_else(|e| {
        error!("Transcription hook failed, using original text: {:#}", e);
        text
    })
}

async fn run(hook: &Path, text: &str) -> anyhow::Result<String> {
    let mut child = Command::new(hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // Write input while reading output so neither side blocks on a full pipe.
    // Write errors are ignored: a hook may exit without reading its input.
    let mut stdin = child.stdin.take().context("stdin not piped")?;
    let write = async move {
        let _ = stdin.write_all(text.as_bytes()).await;
    };
    let (_, output) = tokio::time::timeout(HOOK_TIMEOUT, async {
        tokio::join!(write, child.wait_with_output())
    })
    .await
    .context("timed out")?;
    let output = output?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(
        output.status.success(),
        "exited with {}: {}",
        output.status,
        stderr.trim()
    );

    // Drop one trailing newline so `echo` output doesn't submit a terminal line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.strip_suffix('\n').unwrap_or(&stdout).to_string())
}
