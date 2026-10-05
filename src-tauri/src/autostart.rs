//! Launch-at-login (autostart) handling.
//!
//! All platforms apply the setting through tauri-plugin-autostart, except
//! macOS 13+ where the app registers itself as a login item via
//! `SMAppService`. The plugin's launch agent plist carries no app
//! association, so the System Settings Login Items pane attributes it to the
//! code-signing certificate's developer name instead of the app (#337).
//! `SMAppService` login items are attributed to the app bundle itself and
//! appear under "Open at Login" with the app's name and icon.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Apply the user's autostart preference using the best mechanism for the
/// current platform.
///
/// Errors are logged rather than returned: the preference is re-applied on
/// every launch, so a transient failure self-heals and must not block
/// startup. This mirrors the pre-existing behavior of ignoring
/// enable()/disable() results.
pub fn apply_autostart(app: &AppHandle, enabled: bool) {
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
