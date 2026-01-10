use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const DESKTOP_FILE_NAME: &str = "com.pais.handy.desktop";

fn get_autostart_dir() -> Option<PathBuf> {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        Some(PathBuf::from(config_home).join("autostart"))
    } else if let Ok(home) = env::var("HOME") {
        Some(PathBuf::from(home).join(".config").join("autostart"))
    } else {
        None
    }
}

fn get_autostart_file_path() -> Option<PathBuf> {
    get_autostart_dir().map(|dir| dir.join(DESKTOP_FILE_NAME))
}

fn get_flatpak_id() -> Option<String> {
    env::var("FLATPAK_ID").ok()
}

fn get_exec_command() -> String {
    if let Some(flatpak_id) = get_flatpak_id() {
        return format!("flatpak run {}", flatpak_id);
    }

    // Fall back to current executable for non-Flatpak
    env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "handy".to_string())
}

fn create_desktop_file_content() -> String {
    let exec = get_exec_command();

    let mut content = format!(
        "[Desktop Entry]
Type=Application
Version=1.0
Name=Handy
Comment=Speech-to-text application
Exec={exec}
Icon=com.pais.handy
Categories=Audio;Utility;Accessibility;
StartupNotify=false
Terminal=false"
    );

    if let Some(flatpak_id) = get_flatpak_id() {
        content.push_str(&format!("\nX-Flatpak={flatpak_id}"));
    }

    content.push('\n');
    content
}

pub fn enable() -> Result<(), String> {
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

    log::info!("Autostart enabled: {:?}", autostart_file);
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let autostart_file =
        get_autostart_file_path().ok_or("Could not determine autostart file path")?;

    if autostart_file.exists() {
        fs::remove_file(&autostart_file)
            .map_err(|e| format!("Failed to remove autostart file: {}", e))?;
        log::info!("Autostart disabled: {:?}", autostart_file);
    }

    Ok(())
}
