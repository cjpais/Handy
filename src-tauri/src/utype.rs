//! Handy's bundled keyboard input for Linux.
//!
//! Since Wayland, every desktop has grown its own protocol for faking keyboard
//! input, each with its own CLI tool to install. [utype](https://github.com/vanviegen/utype)
//! speaks all of them — the Wayland virtual-keyboard and KDE fake-input
//! protocols, Mutter's and Muffin's remote-desktop D-Bus APIs, uinput through
//! `ydotoold`, and XTEST on X11 — and picks one from the session it finds.
//! `build.rs` compiles it from `vendor/utype/` into `resources/utype`, so it is
//! always there and users need install nothing.
//!
//! It runs as a child process on purpose: utype reports fatal errors by exiting,
//! and its X11 backend restores the keymap it borrowed on the way out.

use crate::settings::PasteMethod;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::{path::BaseDirectory, AppHandle, Manager};

static BINARY: OnceLock<Option<PathBuf>> = OnceLock::new();
static AUTO_OFF: AtomicBool = AtomicBool::new(false);

/// The bundled utype, or `None` for a build that somehow shipped without it.
fn binary(app: &AppHandle) -> Option<&'static Path> {
    BINARY
        .get_or_init(|| {
            let path = app
                .path()
                .resolve("resources/utype", BaseDirectory::Resource)
                .ok()
                .filter(|path| path.is_file())
                .map(|resource| stage(app, &resource).unwrap_or(resource));
            match &path {
                Some(path) => log::info!("Bundled utype: {}", path.display()),
                None => log::warn!("This build has no bundled utype"),
            }
            path
        })
        .as_deref()
}

/// Copies utype in among the rest of our data and returns that copy.
///
/// KWin grants fake_input by matching the client's executable path against a
/// .desktop file utype installs for itself, so that path has to be the same on
/// every launch -- and an AppImage mounts itself somewhere new every time.
fn stage(app: &AppHandle, resource: &Path) -> Option<PathBuf> {
    let dir = crate::portable::app_data_dir(app).ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let (staged, temp) = (dir.join("utype"), dir.join("utype.new"));
    // Rename instead of overwriting in place, which fails with ETXTBSY while an
    // earlier run's utype is still executing the old copy.
    std::fs::copy(resource, &temp).ok()?;
    std::fs::rename(&temp, &staged).ok()?;
    Some(staged)
}

/// `dirs` with the entries an AppImage's AppRun prepended taken back out: its
/// own `$APPDIR/usr/share` (twice, once with a trailing slash) and a duplicate
/// of `/usr/share`.
fn session_data_dirs(dirs: &str, appdir: &str) -> String {
    let mut dirs: Vec<&str> = dirs
        .split(':')
        .skip_while(|dir| dir.starts_with(appdir))
        .collect();
    if dirs.first() == Some(&"/usr/share") && dirs[1..].contains(&"/usr/share") {
        dirs.remove(0);
    }
    dirs.join(":")
}

pub fn is_available(app: &AppHandle) -> bool {
    binary(app).is_some()
}

/// Runs `action` on behalf of the automatic tool chain, reporting whether it
/// handled the request. A failure drops utype from that chain for the rest of
/// the session, so a desktop it cannot reach does not pay for a doomed spawn on
/// every paste. Picking utype explicitly always tries it.
pub fn try_auto(
    app: &AppHandle,
    what: &str,
    action: impl FnOnce(&AppHandle) -> Result<(), String>,
) -> bool {
    if AUTO_OFF.load(Ordering::Relaxed) {
        return false;
    }
    match action(app) {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "Bundled utype could not {what}: {error}. Falling back to the installed \
                 typing tools for the rest of this session."
            );
            AUTO_OFF.store(true, Ordering::Relaxed);
            false
        }
    }
}

pub fn type_text(app: &AppHandle, text: &str) -> Result<(), String> {
    run(app, &["--", text])
}

pub fn send_key_combo(app: &AppHandle, paste_method: &PasteMethod) -> Result<(), String> {
    run(app, key_combo_args(paste_method)?)
}

/// utype takes wtype's command line: -M holds a modifier, -k taps a key, -m
/// releases the modifier again.
fn key_combo_args(paste_method: &PasteMethod) -> Result<&'static [&'static str], String> {
    Ok(match paste_method {
        PasteMethod::CtrlV => &["-M", "ctrl", "-k", "v", "-m", "ctrl"],
        PasteMethod::ShiftInsert => &["-M", "shift", "-k", "Insert", "-m", "shift"],
        PasteMethod::CtrlShiftV => &[
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ],
        _ => return Err("Unsupported paste method".into()),
    })
}

fn run(app: &AppHandle, args: &[&str]) -> Result<(), String> {
    let binary = binary(app).ok_or("utype is not bundled with this build")?;

    let mut command = Command::new(binary);
    // -v narrates which protocol utype picked and why, never the text itself —
    // exactly what a paste bug report needs.
    if log::log_enabled!(log::Level::Debug) {
        command.arg("-v");
    }
    if let Ok(appdir) = std::env::var("APPDIR") {
        // utype's KDE authorization rebuilds KService's cache, and KService
        // derives that cache's filename from the data dirs it scanned -- so with
        // the AppImage's own tree left in the list the rebuild lands in a file
        // KWin never reads, and fake_input is never granted.
        if let Ok(dirs) = std::env::var("XDG_DATA_DIRS") {
            command.env("XDG_DATA_DIRS", session_data_dirs(&dirs, &appdir));
        }
        command.env_remove("LD_LIBRARY_PATH");
    }

    let output = command
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute utype: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !output.status.success() {
        return Err(format!("utype failed: {}", stderr));
    }
    if !stderr.is_empty() {
        log::debug!("utype: {}", stderr);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An AppImage's AppRun prepends its own tree to XDG_DATA_DIRS, which would
    /// send utype's KService cache rebuild to a file KWin does not read.
    #[test]
    fn session_data_dirs_undoes_an_appimage() {
        let session = "/home/u/.local/share/flatpak/exports/share:/usr/local/share:/usr/share";
        let appdir = "/tmp/.mount_Handy_DGEpEI";
        let polluted = format!("{appdir}/usr/share/:{appdir}/usr/share:/usr/share:{session}");
        assert_eq!(session_data_dirs(&polluted, appdir), session);
    }

    /// build.rs must produce a runnable utype that understands every argument
    /// vector Handy builds. Pinning a protocol that cannot work without DISPLAY
    /// makes utype parse the arguments and then bail out, so the test never
    /// types into whatever window happens to be focused.
    #[test]
    fn bundled_utype_accepts_our_arguments() {
        let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/utype");
        let combos = [
            PasteMethod::CtrlV,
            PasteMethod::CtrlShiftV,
            PasteMethod::ShiftInsert,
        ];

        for args in std::iter::once(&["--", "hello"][..])
            .chain(combos.iter().map(|m| key_combo_args(m).unwrap()))
        {
            let output = Command::new(&binary)
                .env("UTYPE_PROTOCOL", "xtest")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .args(args)
                .output()
                .expect("run the bundled utype");

            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("DISPLAY is not set"),
                "utype did not accept {args:?}: {stderr}"
            );
        }
    }
}
