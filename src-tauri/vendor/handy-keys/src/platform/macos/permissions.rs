use std::process::Command;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// Check if the current process has accessibility permissions
pub fn check_accessibility() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Open System Settings to the Accessibility privacy panel
pub fn open_accessibility_settings() -> std::io::Result<()> {
    Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()?
        .wait()?;
    Ok(())
}
