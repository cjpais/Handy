use log::debug;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandFilterResult {
    Applied(String),
    CancelledEmpty,
    Failed(String),
}

fn resolve_home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }

    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            if !profile.is_empty() {
                return Some(PathBuf::from(profile));
            }
        }

        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut full = PathBuf::from(drive);
            full.push(path);
            return Some(full);
        }
    }

    None
}

fn expand_tilde_path_arg_with_home(arg: &str, home_dir: Option<&Path>) -> String {
    let Some(home_dir) = home_dir else {
        return arg.to_string();
    };

    if arg == "~" {
        return home_dir.to_string_lossy().to_string();
    }

    if let Some(stripped) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
        return home_dir.join(stripped).to_string_lossy().to_string();
    }

    arg.to_string()
}

fn expand_tilde_path_arg(arg: &str) -> String {
    let home_dir = resolve_home_dir();
    expand_tilde_path_arg_with_home(arg, home_dir.as_deref())
}

pub async fn run_command_filter(
    executable: &str,
    args: &[String],
    input_text: &str,
    timeout_ms: u64,
) -> CommandFilterResult {
    let executable = executable.trim();
    if executable.is_empty() {
        return CommandFilterResult::Failed("command_filter executable is empty".to_string());
    }
    let executable = expand_tilde_path_arg(executable);
    let expanded_args: Vec<String> = args.iter().map(|arg| expand_tilde_path_arg(arg)).collect();

    let timeout_ms = timeout_ms.max(1);
    let mut child = match Command::new(&executable)
        .args(&expanded_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return CommandFilterResult::Failed(format!(
                "command_filter failed to spawn '{}': {}",
                executable, err
            ));
        }
    };

    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(mut child_stdin) = stdin.take() {
        if let Err(err) = child_stdin.write_all(input_text.as_bytes()).await {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return CommandFilterResult::Failed(format!(
                "command_filter failed to write stdin for '{}': {}",
                executable, err
            ));
        }
    }

    // Closing stdin explicitly signals EOF to the child.
    drop(stdin);

    let stdout_task = tauri::async_runtime::spawn(async move {
        let mut buffer = Vec::new();
        if let Some(mut out) = stdout {
            out.read_to_end(&mut buffer).await?;
        }
        Ok::<Vec<u8>, std::io::Error>(buffer)
    });

    let stderr_task = tauri::async_runtime::spawn(async move {
        let mut buffer = Vec::new();
        if let Some(mut err) = stderr {
            err.read_to_end(&mut buffer).await?;
        }
        Ok::<Vec<u8>, std::io::Error>(buffer)
    });

    let status = match tokio::time::timeout(Duration::from_millis(timeout_ms), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            return CommandFilterResult::Failed(format!(
                "command_filter failed while waiting for '{}': {}",
                executable, err
            ));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return CommandFilterResult::Failed(format!(
                "command_filter timed out after {}ms for '{}'",
                timeout_ms, executable
            ));
        }
    };

    let stdout_bytes = match stdout_task.await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            return CommandFilterResult::Failed(format!(
                "command_filter failed reading stdout for '{}': {}",
                executable, err
            ));
        }
        Err(err) => {
            return CommandFilterResult::Failed(format!(
                "command_filter stdout task failed for '{}': {}",
                executable, err
            ));
        }
    };

    let stderr_bytes = match stderr_task.await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            return CommandFilterResult::Failed(format!(
                "command_filter failed reading stderr for '{}': {}",
                executable, err
            ));
        }
        Err(err) => {
            return CommandFilterResult::Failed(format!(
                "command_filter stderr task failed for '{}': {}",
                executable, err
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

    debug!(
        "command_filter '{}' exited with status {} (stdout={} chars, stderr={} chars)",
        executable,
        status,
        stdout.len(),
        stderr.len()
    );

    if !status.success() {
        let stderr_snippet = stderr.trim();
        let reason = if stderr_snippet.is_empty() {
            format!(
                "command_filter '{}' exited with status {}",
                executable, status
            )
        } else {
            format!(
                "command_filter '{}' exited with status {}: {}",
                executable, status, stderr_snippet
            )
        };
        return CommandFilterResult::Failed(reason);
    }

    if stdout.trim().is_empty() {
        return CommandFilterResult::CancelledEmpty;
    }

    CommandFilterResult::Applied(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn expands_tilde_prefixed_args_with_home_dir() {
        let home = PathBuf::from("home-dir");
        let expanded = expand_tilde_path_arg_with_home("~/filters/rules.txt", Some(home.as_path()));
        assert_eq!(PathBuf::from(expanded), home.join("filters/rules.txt"));
    }

    #[test]
    fn expands_standalone_tilde_to_home_dir() {
        let home = PathBuf::from("home-dir");
        let expanded = expand_tilde_path_arg_with_home("~", Some(home.as_path()));
        assert_eq!(expanded, home.to_string_lossy());
    }

    #[test]
    fn keeps_tilde_arg_when_home_dir_unknown() {
        let original = "~/filters/rules.txt";
        let expanded = expand_tilde_path_arg_with_home(original, None);
        assert_eq!(expanded, original);
    }

    #[cfg(unix)]
    #[test]
    fn applies_successful_stdout_output() {
        let result = tauri::async_runtime::block_on(run_command_filter("cat", &[], "hello", 1000));
        assert_eq!(result, CommandFilterResult::Applied("hello".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn returns_failed_for_non_zero_exit() {
        let result =
            tauri::async_runtime::block_on(run_command_filter("false", &[], "hello", 1000));
        assert!(matches!(result, CommandFilterResult::Failed(_)));
    }

    #[cfg(unix)]
    #[test]
    fn returns_cancelled_when_stdout_is_trimmed_empty() {
        let result = tauri::async_runtime::block_on(run_command_filter("true", &[], "hello", 1000));
        assert_eq!(result, CommandFilterResult::CancelledEmpty);
    }

    #[cfg(unix)]
    #[test]
    fn returns_failed_on_timeout() {
        let args = vec!["2".to_string()];
        let result = tauri::async_runtime::block_on(run_command_filter("sleep", &args, "", 50));
        assert!(
            matches!(result, CommandFilterResult::Failed(reason) if reason.contains("timed out"))
        );
    }
}
