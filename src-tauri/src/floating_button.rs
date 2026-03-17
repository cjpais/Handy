use crate::overlay::get_monitor_with_cursor;
use crate::settings;
use crate::settings::FloatingButtonPosition;
use log::debug;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "linux")]
use gtk_layer_shell::{KeyboardMode, Layer};
#[cfg(target_os = "linux")]
use std::env;

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(FloatingButtonPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

const BUTTON_WINDOW_SIZE: f64 = 64.0;
const EDGE_OFFSET: f64 = 20.0;

fn calculate_floating_button_position(app_handle: &AppHandle) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;
    let monitor_height = monitor.size().height as f64 / scale;

    let settings = settings::get_settings(app_handle);
    let pos = settings.floating_button_position;

    let x = match pos {
        FloatingButtonPosition::BottomCenter => {
            monitor_x + (monitor_width - BUTTON_WINDOW_SIZE) / 2.0
        }
        FloatingButtonPosition::TopLeft
        | FloatingButtonPosition::BottomLeft
        | FloatingButtonPosition::CenterLeft => monitor_x + EDGE_OFFSET,
        FloatingButtonPosition::TopRight
        | FloatingButtonPosition::BottomRight
        | FloatingButtonPosition::CenterRight => {
            monitor_x + monitor_width - BUTTON_WINDOW_SIZE - EDGE_OFFSET
        }
    };

    let y = match pos {
        FloatingButtonPosition::BottomCenter => {
            // Extra offset to clear the macOS Dock
            monitor_y + monitor_height - BUTTON_WINDOW_SIZE - 90.0
        }
        FloatingButtonPosition::TopLeft | FloatingButtonPosition::TopRight => {
            monitor_y + EDGE_OFFSET + 46.0 // account for menu bar on macOS
        }
        FloatingButtonPosition::BottomLeft | FloatingButtonPosition::BottomRight => {
            monitor_y + monitor_height - BUTTON_WINDOW_SIZE - EDGE_OFFSET
        }
        FloatingButtonPosition::CenterLeft | FloatingButtonPosition::CenterRight => {
            monitor_y + (monitor_height - BUTTON_WINDOW_SIZE) / 2.0
        }
    };

    Some((x, y))
}

#[cfg(target_os = "linux")]
fn init_floating_button_layer_shell(window: &tauri::webview::WebviewWindow) -> bool {
    let is_wayland = env::var("WAYLAND_DISPLAY").is_ok()
        || env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false);
    let is_kde = env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_uppercase().contains("KDE"))
        .unwrap_or(false)
        || env::var("KDE_SESSION_VERSION").is_ok();
    if is_wayland && is_kde {
        debug!("Skipping GTK layer shell init for floating button on KDE Wayland");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    if let Ok(gtk_window) = window.gtk_window() {
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);
        return true;
    }
    false
}

/// Creates the floating record button window, hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_floating_button(app_handle: &AppHandle) {
    #[cfg(not(target_os = "linux"))]
    {
        if calculate_floating_button_position(app_handle).is_none() {
            debug!("Failed to determine floating button position, not creating window");
            return;
        }
    }

    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "floating_record_button",
        tauri::WebviewUrl::App("src/floating-button/index.html".into()),
    )
    .title("Record")
    .resizable(false)
    .inner_size(BUTTON_WINDOW_SIZE, BUTTON_WINDOW_SIZE)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    #[allow(unused_variables)]
    match builder.build() {
        Ok(window) => {
            #[cfg(target_os = "linux")]
            {
                if init_floating_button_layer_shell(&window) {
                    debug!("GTK layer shell initialized for floating button");
                } else {
                    debug!("GTK layer shell not available for floating button, using regular window");
                }
            }
            debug!("Floating record button window created (hidden)");
        }
        Err(e) => {
            debug!("Failed to create floating record button window: {}", e);
        }
    }
}

/// Creates the floating record button panel, hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_floating_button(app_handle: &AppHandle) {
    if let Some((x, y)) = calculate_floating_button_position(app_handle) {
        match PanelBuilder::<_, FloatingButtonPanel>::new(app_handle, "floating_record_button")
            .url(WebviewUrl::App("src/floating-button/index.html".into()))
            .title("Record")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Floating)
            .size(tauri::Size::Logical(tauri::LogicalSize {
                width: BUTTON_WINDOW_SIZE,
                height: BUTTON_WINDOW_SIZE,
            }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().nonactivating_panel())
            .with_window(|w| w.decorations(false).transparent(true))
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()
        {
            Ok(panel) => {
                let _ = panel.hide();
                debug!("Floating record button panel created (hidden)");
            }
            Err(e) => {
                log::error!("Failed to create floating record button panel: {}", e);
            }
        }
    }
}

/// Shows the floating record button
pub fn show_floating_button(app_handle: &AppHandle) {
    update_floating_button_position(app_handle);

    if let Some(window) = app_handle.get_webview_window("floating_record_button") {
        let _ = window.show();

        #[cfg(target_os = "windows")]
        crate::overlay::force_overlay_topmost(&window);

        let _ = app_handle.emit("floating-button-state", "idle");
    }
}

/// Hides the floating record button
pub fn hide_floating_button(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("floating_record_button") {
        let _ = window.hide();
    }
}

/// Updates the floating button position based on current settings
pub fn update_floating_button_position(app_handle: &AppHandle) {
    if let Some(window) = app_handle.get_webview_window("floating_record_button") {
        if let Some((x, y)) = calculate_floating_button_position(app_handle) {
            let _ =
                window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        }
    }
}

/// Emits a state update to the floating button window
pub fn update_floating_button_state(app_handle: &AppHandle, state: &str) {
    let _ = app_handle.emit("floating-button-state", state);
}

/// Returns whether the floating button is currently visible
pub fn is_floating_button_visible(app_handle: &AppHandle) -> bool {
    app_handle
        .get_webview_window("floating_record_button")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}
