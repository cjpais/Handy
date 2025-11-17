#[cfg(target_os = "macos")]
use std::process::Command;

/// Checks if the MacBook is in clamshell mode (lid closed with external display)
///
/// This queries the macOS IORegistry for the AppleClamshellState key.
/// Returns true if the lid is closed, false if open.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn is_clamshell() -> Result<bool, String> {
    let output = Command::new("ioreg")
        .args(["-r", "-k", "AppleClamshellState", "-d", "4"])
        .output()
        .map_err(|e| format!("Failed to execute ioreg: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ioreg command failed with status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Look for "AppleClamshellState" = Yes in the output
    Ok(stdout.contains("\"AppleClamshellState\" = Yes"))
}

/// Checks if the Mac has a built-in display (i.e., is a laptop)
///
/// This queries the macOS IORegistry for built-in displays.
/// Returns true if a built-in display is found (MacBook), false otherwise (Mac Mini, Mac Studio, etc.)
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn has_builtin_display() -> Result<bool, String> {
    let output = Command::new("ioreg")
        .args(["-l", "-w", "0", "-r", "-c", "IODisplayConnect"])
        .output()
        .map_err(|e| format!("Failed to execute ioreg: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ioreg command failed with status: {}",
            output.status
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Look for built-in display indicators
    // Built-in displays typically have AppleDisplay or AppleBacklightDisplay
    Ok(stdout.contains("AppleBacklightDisplay")
        || (stdout.contains("built-in") && stdout.contains("IODisplayConnect")))
}

/// Stub implementation for non-macOS platforms
/// Always returns false since clamshell mode is macOS-specific
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn is_clamshell() -> Result<bool, String> {
    Ok(false)
}

/// Stub implementation for non-macOS platforms
/// Always returns false since built-in display detection is macOS-specific
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn has_builtin_display() -> Result<bool, String> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn test_clamshell_check() {
        // This will run on macOS and should not panic
        let result = is_clamshell();
        assert!(result.is_ok());
        let _ = result.unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_has_builtin_display() {
        let result = has_builtin_display();
        assert!(result.is_ok());
        let _ = result.unwrap();
    }
}
