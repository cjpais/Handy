//! Install the running `handy` binary as a symlink in the user's PATH
//! (`~/.local/bin/handy`) so the headless CLI (`handy --transcribe-file`,
//! `--list-models`, ...) can be invoked without the full .app/absolute path.
//!
//! Unix only (macOS/Linux). Windows PATH handling is a separate effort.

use serde::Serialize;

#[cfg(unix)]
pub const RELATIVE_BIN_DIR: &str = ".local/bin";
#[cfg(unix)]
pub const BINARY_NAME: &str = "handy";

#[derive(Debug, Serialize, specta::Type)]
pub struct CliInstallStatus {
    pub installed: bool,
    pub link_path: String,
    pub target_path: String,
    pub dir_on_path: bool,
}

/// Resolve the binary that the symlink should point at.
///
/// Returns the canonicalized path of the current executable. When running
/// from an AppImage, the resolved path lives under a transient
/// `/tmp/.mount_*` mount that disappears with the session, so linking into
/// it is useless; refuse and let the user install via deb/AUR instead.
#[cfg(unix)]
pub fn resolve_bundled_binary() -> Result<std::path::PathBuf, String> {
    if std::env::var_os("APPIMAGE").is_some() {
        return Err(
            "Running from an AppImage: the binary lives on a temporary mount that \
             disappears when the image is closed, so no stable symlink is possible. \
             Install Handy via the .deb, AUR package, or point the symlink at the \
             real binary manually."
                .to_string(),
        );
    }
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("Failed to resolve the current executable: {e}"))
}

#[cfg(unix)]
fn home_dir() -> Result<std::path::PathBuf, String> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty())
        .ok_or_else(|| "Could not determine the home directory ($HOME)".to_string())
}

/// Where the symlink should live: `<home>/.local/bin/handy`
#[cfg(unix)]
pub fn link_path(home: &std::path::Path) -> std::path::PathBuf {
    home.join(RELATIVE_BIN_DIR).join(BINARY_NAME)
}

/// Whether `<home>/.local/bin` is present in the current `PATH`.
#[cfg(unix)]
pub fn dir_on_path(home: &std::path::Path) -> bool {
    let bin_dir = home.join(RELATIVE_BIN_DIR);
    let Some(bin_dir) = bin_dir.to_str() else {
        return false;
    };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.to_str() == Some(bin_dir)))
        .unwrap_or(false)
}

/// Create (or repair) the symlink at `<home>/.local/bin/<name>` pointing at
/// `target`.
///
/// Idempotent: if the link already points at `target`, this is a no-op. If it
/// points elsewhere it is replaced (stale-link repair). A regular file or
/// directory at the link path is refused rather than clobbered.
#[cfg(unix)]
pub fn install_to(home: &std::path::Path, target: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let link = link_path(home);

    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }

    match std::fs::symlink_metadata(&link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Failed to stat {}: {e}", link.display())),
        // Existing symlink: no-op if it points at target, repair otherwise.
        Ok(meta) if meta.file_type().is_symlink() => {
            let current = std::fs::read_link(&link)
                .map_err(|e| format!("Failed to read existing symlink: {e}"))?;
            if current == target {
                return Ok(link);
            }
            std::fs::remove_file(&link)
                .map_err(|e| format!("Failed to remove stale symlink: {e}"))?;
        }
        // A real file or directory: refuse to clobber.
        Ok(meta) if meta.is_dir() => {
            return Err(format!(
                "{} already exists and is a directory; refusing to overwrite",
                link.display()
            ));
        }
        Ok(_) => {
            return Err(format!(
                "{} already exists and is a regular file; refusing to overwrite",
                link.display()
            ));
        }
    }

    std::os::unix::fs::symlink(target, &link)
        .map_err(|e| format!("Failed to create symlink at {}: {e}", link.display()))?;
    Ok(link)
}

/// Remove the symlink at `<home>/.local/bin/<name>` if it points at `target`.
/// Returns Ok(true) if removed, Ok(false) if nothing was there. A foreign
/// symlink, regular file, or directory is left alone and reported as an error.
#[cfg(unix)]
pub fn uninstall_from(
    home: &std::path::Path,
    target: &std::path::Path,
) -> Result<bool, String> {
    let link = link_path(home);

    let meta = match std::fs::symlink_metadata(&link) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Failed to stat {}: {e}", link.display())),
    };

    if !meta.file_type().is_symlink() {
        return Err(format!(
            "{} exists but is not a symlink; refusing to remove",
            link.display()
        ));
    }

    let current = std::fs::read_link(&link)
        .map_err(|e| format!("Failed to read existing symlink: {e}"))?;
    if current != target {
        return Err(format!(
            "{} points at {}, not at {}; refusing to remove",
            link.display(),
            current.display(),
            target.display()
        ));
    }

    std::fs::remove_file(&link)
        .map_err(|e| format!("Failed to remove symlink: {e}"))?;
    Ok(true)
}

#[cfg(unix)]
pub fn status_for(home: &std::path::Path, target: &std::path::Path) -> CliInstallStatus {
    let link = link_path(home);
    let installed = std::fs::symlink_metadata(&link)
        .map(|m| {
            m.file_type().is_symlink()
                && std::fs::read_link(&link).map(|l| l == target).unwrap_or(false)
        })
        .unwrap_or(false);

    CliInstallStatus {
        installed,
        link_path: link.display().to_string(),
        target_path: target.display().to_string(),
        dir_on_path: dir_on_path(home),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_cli_install_status() -> CliInstallStatus {
    #[cfg(not(unix))]
    return CliInstallStatus {
        installed: false,
        link_path: String::new(),
        target_path: String::new(),
        dir_on_path: false,
    };

    #[cfg(unix)]
    {
        let target = resolve_bundled_binary().unwrap_or_default();
        match home_dir() {
            Ok(home) => status_for(&home, &target),
            Err(_) => CliInstallStatus {
                installed: false,
                link_path: String::new(),
                target_path: target.display().to_string(),
                dir_on_path: false,
            },
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn install_cli() -> Result<String, String> {
    #[cfg(not(unix))]
    return Err("CLI install is not supported on this platform yet".to_string());

    #[cfg(unix)]
    {
        let target = resolve_bundled_binary()?;
        let home = home_dir()?;
        let link = install_to(&home, &target)?;
        Ok(link.display().to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub fn uninstall_cli() -> Result<(), String> {
    #[cfg(not(unix))]
    return Err("CLI install is not supported on this platform yet".to_string());

    #[cfg(unix)]
    {
        let target = resolve_bundled_binary()?;
        let home = home_dir()?;
        uninstall_from(&home, &target)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup() -> (TempDir, std::path::PathBuf) {
        let home = TempDir::new().expect("temp home");
        let target = home.path().join("fake-bundle").join("handy");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "#!/bin/sh\necho handy\n").unwrap();
        (home, target)
    }

    #[test]
    fn install_creates_link() {
        let (home, target) = setup();
        let link = install_to(home.path(), &target).unwrap();
        assert_eq!(link, link_path(home.path()));
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn install_is_idempotent() {
        let (home, target) = setup();
        install_to(home.path(), &target).unwrap();
        let link = install_to(home.path(), &target).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn install_repairs_stale_link() {
        let (home, target) = setup();
        let other = home.path().join("other-binary");
        fs::write(&other, "old").unwrap();

        let bin_dir = home.path().join(RELATIVE_BIN_DIR);
        fs::create_dir_all(&bin_dir).unwrap();
        std::os::unix::fs::symlink(&other, link_path(home.path())).unwrap();

        install_to(home.path(), &target).unwrap();
        assert_eq!(fs::read_link(link_path(home.path())).unwrap(), target);
    }

    #[test]
    fn install_refuses_regular_file() {
        let (home, target) = setup();
        let bin_dir = home.path().join(RELATIVE_BIN_DIR);
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(link_path(home.path()), "a real file").unwrap();

        let err = install_to(home.path(), &target).unwrap_err();
        assert!(err.contains("regular file"), "got: {err}");
        assert!(link_path(home.path()).is_file());
    }

    #[test]
    fn install_refuses_directory() {
        let (home, target) = setup();
        let bin_dir = home.path().join(RELATIVE_BIN_DIR);
        fs::create_dir_all(bin_dir.join(BINARY_NAME)).unwrap();

        let err = install_to(home.path(), &target).unwrap_err();
        assert!(err.contains("directory"), "got: {err}");
        assert!(link_path(home.path()).is_dir());
    }

    #[test]
    fn uninstall_removes_own_link() {
        let (home, target) = setup();
        install_to(home.path(), &target).unwrap();
        assert!(uninstall_from(home.path(), &target).unwrap());
        assert!(!link_path(home.path()).symlink_metadata().is_ok());
    }

    #[test]
    fn uninstall_missing_link_is_noop() {
        let (home, target) = setup();
        assert!(!uninstall_from(home.path(), &target).unwrap());
    }

    #[test]
    fn uninstall_refuses_foreign_link() {
        let (home, target) = setup();
        let other = home.path().join("other");
        fs::write(&other, "x").unwrap();
        let bin_dir = home.path().join(RELATIVE_BIN_DIR);
        fs::create_dir_all(&bin_dir).unwrap();
        std::os::unix::fs::symlink(&other, link_path(home.path())).unwrap();

        let err = uninstall_from(home.path(), &target).unwrap_err();
        assert!(err.contains("refusing to remove"), "got: {err}");
        assert!(link_path(home.path()).symlink_metadata().is_ok());
    }

    #[test]
    fn status_reports_installed_and_missing() {
        let (home, target) = setup();
        let st = status_for(home.path(), &target);
        assert!(!st.installed);
        assert_eq!(st.link_path, link_path(home.path()).display().to_string());
        assert_eq!(st.target_path, target.display().to_string());

        install_to(home.path(), &target).unwrap();
        assert!(status_for(home.path(), &target).installed);
    }

    #[test]
    fn status_ignores_wrong_target() {
        let (home, target) = setup();
        let other = home.path().join("other");
        fs::write(&other, "x").unwrap();
        let bin_dir = home.path().join(RELATIVE_BIN_DIR);
        fs::create_dir_all(&bin_dir).unwrap();
        std::os::unix::fs::symlink(&other, link_path(home.path())).unwrap();
        assert!(!status_for(home.path(), &target).installed);
    }

    #[test]
    fn link_path_is_under_local_bin() {
        let home = Path::new("/home/tester");
        assert_eq!(link_path(home), Path::new("/home/tester/.local/bin/handy"));
    }
}
