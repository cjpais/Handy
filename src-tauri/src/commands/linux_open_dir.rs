//! Opens a directory through the desktop's file manager on Linux (#2294).

use std::path::Path;
use std::time::Duration;

/// How long to wait for the file manager's reply. D-Bus activation may have to start it
/// first, which took about 7 s for a cold Nautilus in testing. This runs off the main
/// thread, so a long wait only delays the log line.
const METHOD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum ShowFolderError {
    /// No file manager took the request (no session bus, no service, or it failed).
    NotDelivered(String),
    /// The request went out but no reply came in time. The file manager may still be
    /// starting and open the folder, so falling back could open it twice.
    TimedOut,
}

/// Asks the file manager to open `path` with `org.freedesktop.FileManager1.ShowFolders`,
/// which Nautilus, Dolphin, Nemo, Thunar, Caja and others implement. The bus starts the
/// file manager with the session's environment, not Handy's.
pub fn show_folder_via_filemanager1(path: &Path, timeout: Duration) -> Result<(), ShowFolderError> {
    let uri = folder_uri(path).map_err(ShowFolderError::NotDelivered)?;
    let connection = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.method_timeout(timeout).build())
        .map_err(|e| ShowFolderError::NotDelivered(format!("no session bus: {}", e)))?;
    match connection.call_method(
        Some("org.freedesktop.FileManager1"),
        "/org/freedesktop/FileManager1",
        Some("org.freedesktop.FileManager1"),
        "ShowFolders",
        &(vec![uri.as_str()], ""),
    ) {
        Ok(_) => Ok(()),
        Err(zbus::Error::InputOutput(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
            Err(ShowFolderError::TimedOut)
        }
        Err(e) => Err(ShowFolderError::NotDelivered(e.to_string())),
    }
}

/// Tries FileManager1 first and runs `fallback` (the opener plugin) if no file manager
/// took the request.
pub fn open_with_fallback<F>(path: &str, fallback: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    open_with_fallback_within(path, METHOD_TIMEOUT, fallback)
}

fn open_with_fallback_within<F>(path: &str, timeout: Duration, fallback: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    match show_folder_via_filemanager1(Path::new(path), timeout) {
        Ok(()) => {
            log::info!("Opened {} via org.freedesktop.FileManager1", path);
            Ok(())
        }
        Err(ShowFolderError::TimedOut) => {
            log::warn!(
                "org.freedesktop.FileManager1 did not answer within {:?} for {}; not falling back, \
                 so the folder does not open twice",
                timeout,
                path
            );
            Ok(())
        }
        Err(ShowFolderError::NotDelivered(e)) => {
            log::warn!(
                "org.freedesktop.FileManager1 could not open {} ({}); falling back to xdg-open",
                path,
                e
            );
            fallback()
        }
    }
}

fn folder_uri(path: &Path) -> Result<String, String> {
    url::Url::from_directory_path(path)
        .map(String::from)
        .map_err(|_| format!("not an absolute path: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn folder_uri_is_percent_encoded() {
        assert_eq!(
            folder_uri(Path::new("/home/a b/com.pais.handy/recordings")).unwrap(),
            "file:///home/a%20b/com.pais.handy/recordings/"
        );
        assert!(folder_uri(Path::new("relative/dir")).is_err());
    }

    // Needs a session bus with a FileManager1 service, e.g. a mock under dbus-run-session.
    // HANDY_TEST_OPEN_DIR picks an existing directory to open instead of a temporary one.
    #[test]
    #[ignore]
    fn shows_folder_via_filemanager1_service() {
        let dir = tempfile::Builder::new()
            .prefix("handy open ")
            .tempdir()
            .unwrap();
        let path = std::env::var("HANDY_TEST_OPEN_DIR")
            .unwrap_or_else(|_| dir.path().to_str().unwrap().to_string());
        let fell_back = Cell::new(false);
        let result = open_with_fallback(&path, || {
            fell_back.set(true);
            Ok(())
        });
        println!(
            "dir={} result={:?} fell_back={}",
            path,
            result,
            fell_back.get()
        );
        assert!(result.is_ok());
        assert!(!fell_back.get());
    }

    // Needs a session bus on which nothing owns org.freedesktop.FileManager1.
    #[test]
    #[ignore]
    fn falls_back_without_filemanager1_service() {
        let dir = tempfile::tempdir().unwrap();
        let direct = show_folder_via_filemanager1(dir.path(), METHOD_TIMEOUT);
        println!("direct call: {:?}", direct);
        assert!(matches!(direct, Err(ShowFolderError::NotDelivered(_))));
        let fell_back = Cell::new(false);
        let result = open_with_fallback(dir.path().to_str().unwrap(), || {
            fell_back.set(true);
            Ok(())
        });
        println!("result={:?} fell_back={}", result, fell_back.get());
        assert!(fell_back.get());
    }

    // Needs a FileManager1 service that never answers ShowFolders.
    #[test]
    #[ignore]
    fn does_not_fall_back_when_filemanager1_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let fell_back = Cell::new(false);
        let result =
            open_with_fallback_within(dir.path().to_str().unwrap(), Duration::from_secs(1), || {
                fell_back.set(true);
                Ok(())
            });
        println!("result={:?} fell_back={}", result, fell_back.get());
        assert!(result.is_ok());
        assert!(!fell_back.get());
    }
}
