use std::process::Command;

use crate::device_initializer::InitDeviceError;

pub fn run_powershell_lines(script: &str) -> Result<Vec<String>, InitDeviceError> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|ex| InitDeviceError::Provider(format!("failed to start PowerShell: {ex}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(InitDeviceError::Provider(format!(
            "PowerShell command failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}
