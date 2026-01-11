use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const DESKTOP_FILE_NAME: &str = "com.pais.handy.desktop";

fn is_flatpak() -> bool {
    env::var("FLATPAK_ID").is_ok()
}

fn get_autostart_dir() -> Option<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        Some(PathBuf::from(config_home).join("autostart"))
    } else {
        env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".config").join("autostart"))
    }
}

fn get_autostart_file_path() -> Option<PathBuf> {
    get_autostart_dir().map(|dir| dir.join(DESKTOP_FILE_NAME))
}

fn get_exec_command() -> String {
    env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "handy".to_string())
}

fn create_desktop_file_content() -> String {
    let exec = get_exec_command();

    format!(
        "[Desktop Entry]
Type=Application
Version=1.0
Name=Handy
Comment=Speech-to-text application
Exec={exec}
Icon=com.pais.handy
Categories=Audio;Utility;Accessibility;
StartupNotify=false
Terminal=false
"
    )
}

/// Request autostart via the XDG Background portal.
///
/// Returns Ok if the gdbus command executed successfully. Note that portal
/// success depends on the desktop environment - some (like Hyprland) don't
/// implement the Background portal and will silently ignore the request.
fn request_background_portal(enable: bool) -> Result<(), String> {
    let options = format!(
        "{{'reason': <'Start Handy automatically at login'>, 'autostart': <{}>, 'dbus-activatable': <false>}}",
        if enable { "true" } else { "false" },
    );

    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Background.RequestBackground",
            "",
            &options,
        ])
        .output()
        .map_err(|e| format!("Failed to execute gdbus: {}", e))?;

    if output.status.success() {
        log::info!(
            "Background portal request sent (autostart={})",
            if enable { "enable" } else { "disable" }
        );
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("Background portal request failed: {}", stderr);
        Err(format!("Background portal request failed: {}", stderr))
    }
}

/// Enable autostart using desktop file (for native Linux)
fn enable_desktop_file() -> Result<(), String> {
    let autostart_dir = get_autostart_dir().ok_or("Could not determine autostart directory")?;
    let autostart_file =
        get_autostart_file_path().ok_or("Could not determine autostart file path")?;

    fs::create_dir_all(&autostart_dir)
        .map_err(|e| format!("Failed to create autostart directory: {}", e))?;

    let content = create_desktop_file_content();
    let mut file = fs::File::create(&autostart_file)
        .map_err(|e| format!("Failed to create autostart file: {}", e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write autostart file: {}", e))?;

    log::info!("Autostart enabled via desktop file: {:?}", autostart_file);
    Ok(())
}

/// Disable autostart by removing desktop file (for native Linux)
fn disable_desktop_file() -> Result<(), String> {
    let autostart_file =
        get_autostart_file_path().ok_or("Could not determine autostart file path")?;

    if autostart_file.exists() {
        fs::remove_file(&autostart_file)
            .map_err(|e| format!("Failed to remove autostart file: {}", e))?;
        log::info!("Autostart disabled via desktop file: {:?}", autostart_file);
    }

    Ok(())
}

pub fn enable() -> Result<(), String> {
    if is_flatpak() {
        request_background_portal(true)
    } else {
        enable_desktop_file()
    }
}

pub fn disable() -> Result<(), String> {
    if is_flatpak() {
        request_background_portal(false)
    } else {
        disable_desktop_file()
    }
}
