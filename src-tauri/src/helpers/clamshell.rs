use std::process::Command;

/// Checks if the MacBook is in clamshell mode (lid closed with external display)
///
/// This queries the macOS IORegistry for the AppleClamshellState key.
/// Returns true if the lid is closed, false if open.
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

/// Returns detailed clamshell state information
#[tauri::command]
pub fn get_clamshell_info() -> Result<ClamshellInfo, String> {
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
    let is_closed = stdout.contains("\"AppleClamshellState\" = Yes");

    Ok(ClamshellInfo {
        lid_closed: is_closed,
        mode: if is_closed { "clamshell" } else { "open" }.to_string(),
        raw_output: stdout.to_string(),
    })
}

#[derive(serde::Serialize)]
pub struct ClamshellInfo {
    pub lid_closed: bool,
    pub mode: String,
    pub raw_output: String,
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
        println!("Clamshell state: {:?}", result.unwrap());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_clamshell_info() {
        let result = get_clamshell_info();
        assert!(result.is_ok());
        if let Ok(info) = result {
            println!("Lid closed: {}", info.lid_closed);
            println!("Mode: {}", info.mode);
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_has_builtin_display() {
        let result = has_builtin_display();
        assert!(result.is_ok());
        if let Ok(has_builtin) = result {
            println!("Has built-in display (is laptop): {}", has_builtin);
        }
    }
}