//! Bound wl-copy's wait for clipboard access, including its focus-based fallback.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const COPY_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn run(args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("wl-copy");
    command.args(args);
    run_command(command, COPY_TIMEOUT)
}

fn run_command(mut command: Command, timeout: Duration) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::null())
        // wl-copy forks a selection server. It must not keep captured pipes open.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to execute wl-copy: {e}"))?;

    let status = wait_for_copy(&mut child, timeout)?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wl-copy failed: {status}"))
    }
}

fn wait_for_copy(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            result => {
                // Stop and reap before falling back: a late focus event must not
                // let the old process replace the fallback's clipboard contents.
                let kill_result = child.kill();
                let status = child
                    .wait()
                    .map_err(|e| format!("Failed to reap wl-copy: {e}"))?;
                // It may have exited normally between try_wait and kill.
                if kill_result.is_err() && status.success() {
                    return Ok(status);
                }
                return match result {
                    Err(e) => Err(format!("Failed to wait for wl-copy: {e}")),
                    _ => Err(format!(
                        "wl-copy did not finish within {} ms",
                        timeout.as_millis()
                    )),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn command(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn clipboard stand-in")
    }

    #[test]
    fn returns_the_childs_exit_status() {
        for code in [0, 42] {
            let mut child = command(&format!("exit {code}"));
            let status = wait_for_copy(&mut child, Duration::from_secs(2)).unwrap();
            assert_eq!(status.code(), Some(code));
        }
    }

    #[test]
    fn timeout_kills_and_reaps_the_blocked_child() {
        let mut child = command("exec sleep 30");
        let pid = child.id();
        let started = Instant::now();
        let error = wait_for_copy(&mut child, Duration::from_millis(50)).unwrap_err();

        assert!(error.contains("within 50 ms"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        let status = child.try_wait().unwrap().expect("child must be reaped");
        assert_eq!(status.signal(), Some(9));
    }

    #[test]
    fn does_not_wait_for_a_forked_selection_server() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 2 & exit 0"]);
        let started = Instant::now();
        run_command(command, Duration::from_secs(1)).unwrap();

        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
