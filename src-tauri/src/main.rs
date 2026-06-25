// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use handy_app_lib::CliArgs;

#[cfg(target_os = "linux")]
use std::ffi::CString;

#[cfg(target_os = "linux")]
fn env_value_is_truthy(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| {
        !value.is_empty()
            && !matches!(
                value.to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
    })
}

#[cfg(target_os = "linux")]
fn x11_display_is_usable_with(
    display: Option<&str>,
    open_display: impl FnOnce(&str) -> bool,
) -> bool {
    let Some(display) = display.map(str::trim).filter(|display| !display.is_empty()) else {
        return false;
    };

    open_display(display)
}

#[cfg(target_os = "linux")]
fn x11_display_is_usable(display: Option<&str>) -> bool {
    x11_display_is_usable_with(display, |display| {
        let Ok(display) = CString::new(display) else {
            return false;
        };
        let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
            return false;
        };

        unsafe {
            // XOpenDisplay performs the X11 setup and authorization handshake.
            // A live socket alone is insufficient because GTK would still fail
            // to start when XAUTHORITY is stale or mismatched.
            let connection = (xlib.XOpenDisplay)(display.as_ptr());
            if connection.is_null() {
                return false;
            }
            (xlib.XCloseDisplay)(connection);
        }

        true
    })
}

#[cfg(target_os = "linux")]
fn should_probe_gnome_xwayland(
    current_desktop: Option<&str>,
    wayland_display: Option<&str>,
    session_type: Option<&str>,
    gtk_backend: Option<&str>,
    disabled: Option<&str>,
) -> bool {
    let is_gnome =
        current_desktop.is_some_and(|desktop| desktop.to_ascii_lowercase().contains("gnome"));
    let is_wayland = wayland_display.is_some_and(|display| !display.trim().is_empty())
        || session_type.is_some_and(|kind| kind.eq_ignore_ascii_case("wayland"));
    let backend_is_unset = gtk_backend.is_none_or(|backend| backend.trim().is_empty());

    is_gnome && is_wayland && backend_is_unset && !env_value_is_truthy(disabled)
}

#[cfg(target_os = "linux")]
fn should_use_gnome_xwayland_with(
    current_desktop: Option<&str>,
    wayland_display: Option<&str>,
    session_type: Option<&str>,
    x11_display: Option<&str>,
    gtk_backend: Option<&str>,
    disabled: Option<&str>,
    probe_x11: impl FnOnce(Option<&str>) -> bool,
) -> bool {
    should_probe_gnome_xwayland(
        current_desktop,
        wayland_display,
        session_type,
        gtk_backend,
        disabled,
    ) && probe_x11(x11_display)
}

fn main() {
    let cli_args = CliArgs::parse();

    #[cfg(target_os = "linux")]
    {
        // DMABUF renderer causes crashes on various GPU/display server configurations
        // See: https://github.com/tauri-apps/tauri/issues/9394
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

        // Mutter does not let regular Wayland windows choose their position.
        // Using GTK's X11 backend restores overlay placement, at the cost of
        // running Handy through XWayland. Set HANDY_NO_GNOME_XWAYLAND=1 to
        // retain the native Wayland backend and its compositor-selected position.
        let current_desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        let session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let x11_display = std::env::var("DISPLAY").ok();
        let gtk_backend = std::env::var("GDK_BACKEND").ok();
        let disabled = std::env::var("HANDY_NO_GNOME_XWAYLAND").ok();
        if should_use_gnome_xwayland_with(
            current_desktop.as_deref(),
            wayland_display.as_deref(),
            session_type.as_deref(),
            x11_display.as_deref(),
            gtk_backend.as_deref(),
            disabled.as_deref(),
            x11_display_is_usable,
        ) {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    handy_app_lib::run(cli_args)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn gnome_wayland_decision(x11_display_usable: bool) -> bool {
        should_use_gnome_xwayland_with(
            Some("ubuntu:GNOME"),
            Some("wayland-0"),
            Some("wayland"),
            Some(":0"),
            None,
            None,
            |_| x11_display_usable,
        )
    }

    #[test]
    fn enables_xwayland_for_gnome_wayland_when_available() {
        assert!(gnome_wayland_decision(true));
    }

    #[test]
    fn accepts_session_type_when_wayland_display_is_missing() {
        assert!(should_use_gnome_xwayland_with(
            Some("GNOME"),
            None,
            Some("WAYLAND"),
            Some(":0"),
            None,
            None,
            |_| true,
        ));
    }

    #[test]
    fn preserves_native_wayland_when_xwayland_is_unavailable() {
        assert!(!gnome_wayland_decision(false));
    }

    #[test]
    fn preserves_user_selected_gtk_backend() {
        assert!(!should_use_gnome_xwayland_with(
            Some("GNOME"),
            Some("wayland-0"),
            Some("wayland"),
            Some(":0"),
            Some("wayland"),
            None,
            |_| panic!("X11 probe should be skipped for an explicit GTK backend"),
        ));
    }

    #[test]
    fn treats_empty_gtk_backend_as_unset() {
        assert!(should_use_gnome_xwayland_with(
            Some("GNOME"),
            Some("wayland-0"),
            Some("wayland"),
            Some(":0"),
            Some(""),
            None,
            |_| true,
        ));
    }

    #[test]
    fn supports_truthy_opt_out_values() {
        for value in ["1", "true", "YES", "on"] {
            assert!(!should_use_gnome_xwayland_with(
                Some("GNOME"),
                Some("wayland-0"),
                Some("wayland"),
                Some(":0"),
                None,
                Some(value),
                |_| panic!("X11 probe should be skipped when the workaround is disabled"),
            ));
        }
    }

    #[test]
    fn ignores_false_opt_out_values() {
        for value in ["", "0", "false", "NO", "off"] {
            assert!(should_use_gnome_xwayland_with(
                Some("GNOME"),
                Some("wayland-0"),
                Some("wayland"),
                Some(":0"),
                None,
                Some(value),
                |_| true,
            ));
        }
    }

    #[test]
    fn does_not_affect_other_desktops_or_x11_sessions() {
        assert!(!should_use_gnome_xwayland_with(
            Some("KDE"),
            Some("wayland-0"),
            Some("wayland"),
            Some("remote.invalid:0"),
            None,
            None,
            |_| panic!("X11 probe should be skipped outside GNOME"),
        ));
        assert!(!should_use_gnome_xwayland_with(
            Some("GNOME"),
            None,
            Some("x11"),
            Some("remote.invalid:0"),
            None,
            None,
            |_| panic!("X11 probe should be skipped outside Wayland"),
        ));
    }

    #[test]
    fn accepts_authenticated_x11_display() {
        assert!(x11_display_is_usable_with(Some(":0"), |display| display == ":0"));
    }

    #[test]
    fn rejects_x11_display_when_authenticated_open_fails() {
        assert!(!x11_display_is_usable_with(Some(":0"), |_| false));
    }

    #[test]
    fn rejects_missing_or_empty_x11_display() {
        assert!(!x11_display_is_usable_with(None, |_| true));
        assert!(!x11_display_is_usable_with(Some("  "), |_| true));
    }

    #[test]
    fn passes_protocol_qualified_local_display_to_xlib() {
        assert!(x11_display_is_usable_with(
            Some("hostname/unix:0"),
            |display| display == "hostname/unix:0",
        ));
    }
}
