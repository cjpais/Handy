//! Local IPC server: exposes Handy's transcription engine to other processes
//! on this machine over a per-user local socket (Unix domain socket on
//! macOS/Linux, named pipe on Windows) speaking newline-delimited JSON-RPC.
//!
//! Protocol contract and client examples live in docs/ipc.md. The trust
//! boundary is deliberately the OS user: this is the same tier as the existing
//! ungated remote controls (SIGUSR2, `--toggle-transcription`), and nothing
//! privacy-sensitive (history, dictated text events) is exposed.
//!
//! Set `HANDY_NO_IPC=1` to disable the server entirely.

pub mod protocol;
mod server;
mod service;

use std::thread;

use interprocess::local_socket::{Listener, ListenerOptions};
use log::{info, warn};
use tauri::AppHandle;

/// Start the IPC server on a background thread. Called once from
/// `initialize_core_logic`; never called in headless mode (a one-shot
/// `--transcribe-file` process must not steal the socket from a running app).
pub fn start(app: &AppHandle) {
    if std::env::var("HANDY_NO_IPC").is_ok_and(|v| v == "1") {
        info!("IPC server disabled (HANDY_NO_IPC=1)");
        return;
    }

    let listener = match bind(app) {
        Ok(l) => l,
        Err(e) => {
            warn!("IPC server not started: {e}");
            return;
        }
    };

    let app = app.clone();
    thread::Builder::new()
        .name("ipc-server".into())
        .spawn(move || server::run(app, listener))
        .ok();
}

#[cfg(unix)]
fn bind(app: &AppHandle) -> Result<Listener, String> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    use std::os::unix::fs::PermissionsExt;

    let path = socket_path(app)?;
    // macOS caps sun_path at 104 bytes; give up (with a clear log) rather than
    // bind a truncated or invalid path.
    if path.as_os_str().len() > 100 {
        return Err(format!("socket path too long: {}", path.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    // Remove a stale socket from a previous run. The single-instance plugin
    // guarantees no other live Handy owns it.
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("remove stale socket: {e}"))?;
    }

    let name = path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .map_err(|e| format!("socket name: {e}"))?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .map_err(|e| format!("bind {}: {e}", path.display()))?;

    // Owner-only: the socket is the API's entire access control.
    if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        warn!(
            "could not set socket permissions on {}: {e}",
            path.display()
        );
    }

    info!("IPC server listening on {}", path.display());
    Ok(listener)
}

#[cfg(windows)]
fn bind(_app: &AppHandle) -> Result<Listener, String> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};

    // Maps to \\.\pipe\com.pais.handy.ipc. The default pipe security descriptor
    // limits clients to the same user; on a multi-session machine a second
    // user's Handy will fail to bind and log a warning (IPC disabled for them).
    let name = "com.pais.handy.ipc"
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| format!("pipe name: {e}"))?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .map_err(|e| format!("bind named pipe: {e}"))?;
    info!("IPC server listening on \\\\.\\pipe\\com.pais.handy.ipc");
    Ok(listener)
}

/// The socket lives in Handy's data dir: portable-mode aware, per-user, and
/// already the discovery point clients know (`~/Library/Application
/// Support/com.pais.handy/handy.sock` on macOS).
#[cfg(unix)]
fn socket_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;

    let dir = match crate::portable::data_dir() {
        Some(dir) => dir.clone(),
        None => app
            .path()
            .app_data_dir()
            .map_err(|e| format!("resolve app data dir: {e}"))?,
    };
    Ok(dir.join("handy.sock"))
}
