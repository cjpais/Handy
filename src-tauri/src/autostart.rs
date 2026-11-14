//! Launch-at-login (autostart) handling.
//!
//! All platforms apply the setting through tauri-plugin-autostart, except
//! macOS 13+ where the app registers itself as a login item via
//! `SMAppService`. The plugin's launch agent plist carries no app
//! association, so the System Settings Login Items pane attributes it to the
//! code-signing certificate's developer name instead of the app (#337).
//! `SMAppService` login items are attributed to the app bundle itself and
//! appear under "Open at Login" with the app's name and icon.

use std::sync::{Mutex, PoisonError};

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Held while the login item is being changed. On macOS a change is
/// check-then-act (read the `SMAppService` status, then register or
/// unregister), so two changes that interleave can leave the OS registered
/// against the older request.
static APPLYING: Mutex<()> = Mutex::new(());

/// Apply the user's autostart preference using the best mechanism for the
/// current platform.
///
/// This blocks: on macOS 13+ the `SMAppService` status query is a synchronous
/// round trip to a system service. Startup uses [`reconcile_autostart`] on a
/// background thread instead.
///
/// Errors are logged rather than returned: the preference is re-applied on
/// every launch, so a transient failure self-heals and must not block
/// startup. This mirrors the pre-existing behavior of ignoring
/// enable()/disable() results.
pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    serialized(|| enabled, |enabled| apply_now(app, enabled));
}

/// Bring the login item in line with the persisted `autostart_enabled`
/// preference. The preference is read only once no other change is in
/// flight, so a settings toggle made while this waited is what gets applied.
pub fn reconcile_autostart(app: &AppHandle) {
    serialized(
        || crate::settings::get_settings(app).autostart_enabled,
        |enabled| apply_now(app, enabled),
    );
}

/// Read the preference and apply it with [`APPLYING`] held across both.
fn serialized(read: impl FnOnce() -> bool, apply: impl FnOnce(bool)) {
    let _applying = APPLYING.lock().unwrap_or_else(PoisonError::into_inner);
    apply(read());
}

fn apply_now(app: &AppHandle, enabled: bool) {
    #[cfg(target_os = "macos")]
    if macos::login_item_api_available() {
        macos::remove_plugin_launch_agent(app);
        macos::set_login_item(enabled);
        return;
    }

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(e) = result {
        log::warn!(
            "Failed to apply autostart setting (enabled={}): {}",
            enabled,
            e
        );
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    use objc2::runtime::AnyClass;
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use tauri::{AppHandle, Manager};

    /// `SMAppService` requires macOS 13. The ServiceManagement framework is
    /// linked unconditionally (it has existed since 10.6), so looking up the
    /// class doubles as the OS version check: present exactly when the API is
    /// usable.
    pub fn login_item_api_available() -> bool {
        AnyClass::get(c"SMAppService").is_some()
    }

    /// Register or unregister the app as a login item, skipping the call when
    /// the service is already in the requested state (unregistering a
    /// never-registered service returns an error on every launch otherwise).
    pub fn set_login_item(enabled: bool) {
        let service = unsafe { SMAppService::mainAppService() };
        let status = unsafe { service.status() };

        if enabled {
            if status == SMAppServiceStatus::Enabled {
                return;
            }
            match unsafe { service.registerAndReturnError() } {
                Ok(()) => log::info!("Registered login item via SMAppService"),
                // Fails in dev (no signed app bundle) and when the user has
                // switched the item off in System Settings, which apps are
                // not allowed to override.
                Err(e) => log::warn!("Failed to register login item: {}", e),
            }
        } else {
            if status == SMAppServiceStatus::NotRegistered || status == SMAppServiceStatus::NotFound
            {
                return;
            }
            match unsafe { service.unregisterAndReturnError() } {
                Ok(()) => log::info!("Unregistered login item via SMAppService"),
                Err(e) => log::warn!("Failed to unregister login item: {}", e),
            }
        }
    }

    /// Remove the launch agent plist that tauri-plugin-autostart (via the
    /// auto-launch crate) wrote on older versions, so login doesn't start the
    /// app twice after migrating to `SMAppService`. Runs on every launch;
    /// missing file is the normal case.
    pub fn remove_plugin_launch_agent(app: &AppHandle) {
        let Ok(home) = app.path().home_dir() else {
            return;
        };
        remove_launch_agent_file(&plugin_launch_agent_path(&home, &app.package_info().name));
    }

    /// Path of the plist the auto-launch crate writes:
    /// `~/Library/LaunchAgents/{app name}.plist`.
    fn plugin_launch_agent_path(home: &Path, app_name: &str) -> PathBuf {
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", app_name))
    }

    fn remove_launch_agent_file(path: &Path) {
        match std::fs::remove_file(path) {
            Ok(()) => log::info!("Removed legacy autostart launch agent {:?}", path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Failed to remove legacy launch agent {:?}: {}", path, e),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Validates the assumption `login_item_api_available` rests on: the
        /// ServiceManagement framework is linked into the binary, so the
        /// class lookup finds `SMAppService` whenever the host is macOS 13+
        /// (which anything able to build this crate is).
        #[test]
        fn sm_app_service_class_resolves() {
            assert!(login_item_api_available());
        }

        #[test]
        fn launch_agent_path_matches_auto_launch_crate() {
            let path = plugin_launch_agent_path(Path::new("/Users/someone"), "Handy");
            assert_eq!(
                path,
                Path::new("/Users/someone/Library/LaunchAgents/Handy.plist")
            );
        }

        #[test]
        fn removes_existing_launch_agent() {
            let dir = tempfile::tempdir().unwrap();
            let plist = dir.path().join("Handy.plist");
            std::fs::write(&plist, "<plist/>").unwrap();

            remove_launch_agent_file(&plist);
            assert!(!plist.exists());
        }

        #[test]
        fn missing_launch_agent_is_a_no_op() {
            let dir = tempfile::tempdir().unwrap();
            remove_launch_agent_file(&dir.path().join("Handy.plist"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::serialized;
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// Startup reconciles the login item on a background thread, so it can
    /// begin while a settings toggle is still changing it. It must wait for
    /// that change and then apply the preference as persisted at that point,
    /// never a value it read while the toggle was in flight.
    #[test]
    fn startup_reconcile_waits_for_an_in_flight_toggle() {
        let persisted = Arc::new(Mutex::new(true));
        let login_item = Arc::new(Mutex::new(None));

        // The user turns autostart off. The settings command persists the
        // choice, then applies it, and the OS call stalls part-way.
        *persisted.lock().unwrap() = false;
        let (stalled_tx, stalled_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel::<()>();
        let toggle = thread::spawn({
            let login_item = Arc::clone(&login_item);
            move || {
                serialized(
                    || false,
                    |enabled| {
                        stalled_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                        *login_item.lock().unwrap() = Some(enabled);
                    },
                )
            }
        });
        stalled_rx.recv().unwrap();

        let (read_tx, read_rx) = mpsc::channel();
        let reconcile = thread::spawn({
            let persisted = Arc::clone(&persisted);
            let login_item = Arc::clone(&login_item);
            move || {
                serialized(
                    || {
                        read_tx.send(()).unwrap();
                        *persisted.lock().unwrap()
                    },
                    |enabled| *login_item.lock().unwrap() = Some(enabled),
                )
            }
        });

        // A correct implementation cannot read while the toggle is in flight,
        // however long this waits; the window only gives a broken one the
        // chance to show itself.
        assert!(
            read_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "the reconcile read the preference while a toggle was changing the login item"
        );

        // The user turns autostart back on before the first change lands.
        *persisted.lock().unwrap() = true;
        resume_tx.send(()).unwrap();
        toggle.join().unwrap();
        reconcile.join().unwrap();

        assert_eq!(*login_item.lock().unwrap(), Some(true));
    }
}
