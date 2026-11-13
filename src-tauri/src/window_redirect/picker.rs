//! Picking the redirect target by clicking it, as a screen recorder picks a
//! window: a transparent panel covers every screen, the window under the
//! pointer is highlighted, a click takes it and Esc gives up.
//!
//! Each panel is a webview (`src/picker/`) that reports pointer positions in
//! its own coordinates; the commands here turn them into screen coordinates
//! and ask the platform what's there.

use super::{platform, WindowInfo};
use log::{error, warn};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow};

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

/// The panels' labels start with this; one panel per screen.
const LABEL_PREFIX: &str = "window-picker-";
const PAGE: &str = "src/picker/index.html";

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(WindowPickerPanel {
        config: {
            // Key so Esc reaches it; a non-activating panel, so Handy
            // doesn't become the active application and the user's focus
            // is where it was once the panel closes.
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

/// Covers every screen with a picker panel. The panels are made once and
/// hidden between uses: destroying a panel from inside its own click
/// handler threw an Objective-C exception, which aborts a Rust process.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Err(e) = open_on_main(&app) {
            error!("Could not open the window picker: {e}");
            close(&app);
        }
    });
}

fn open_on_main(app: &AppHandle) -> tauri::Result<()> {
    platform::remember_front();
    let existing: Vec<WebviewWindow> = panels(app).collect();
    if !existing.is_empty() {
        for panel in existing {
            show_panel(app, &panel)?;
        }
        return Ok(());
    }
    for (index, monitor) in app.available_monitors()?.iter().enumerate() {
        let scale = monitor.scale_factor();
        let position = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);
        create_panel(app, &format!("{LABEL_PREFIX}{index}"), position, size)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn show_panel(app: &AppHandle, panel: &WebviewWindow) -> tauri::Result<()> {
    match app.get_webview_panel(panel.label()) {
        Ok(panel) => panel.show_and_make_key(),
        Err(e) => error!("Picker panel {} has no panel handle: {e:?}", panel.label()),
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn show_panel(_app: &AppHandle, panel: &WebviewWindow) -> tauri::Result<()> {
    panel.show()?;
    panel.set_focus()
}

#[cfg(target_os = "macos")]
fn hide_panel(app: &AppHandle, panel: &WebviewWindow) -> tauri::Result<()> {
    match app.get_webview_panel(panel.label()) {
        Ok(panel) => panel.hide(),
        Err(e) => error!("Picker panel {} has no panel handle: {e:?}", panel.label()),
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn hide_panel(_app: &AppHandle, panel: &WebviewWindow) -> tauri::Result<()> {
    panel.hide()
}

#[cfg(target_os = "macos")]
fn create_panel(
    app: &AppHandle,
    label: &str,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> tauri::Result<()> {
    let panel = PanelBuilder::<_, WindowPickerPanel>::new(app, label)
        .url(WebviewUrl::App(PAGE.into()))
        .title("Pick a Window")
        .position(tauri::Position::Logical(position))
        .size(tauri::Size::Logical(size))
        .level(PanelLevel::Status)
        .has_shadow(false)
        .transparent(true)
        .accepts_mouse_moved_events(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| w.decorations(false).transparent(true))
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build()?;
    panel.show_and_make_key();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn create_panel(
    app: &AppHandle,
    label: &str,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, label, WebviewUrl::App(PAGE.into()))
        .title("Pick a Window")
        .position(position.x, position.y)
        .inner_size(size.width, size.height)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .build()?;
    Ok(())
}

fn panels(app: &AppHandle) -> impl Iterator<Item = WebviewWindow> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(LABEL_PREFIX))
        .map(|(_, window)| window)
}

/// Takes the picker off the screen; the panels stay for the next opening.
pub fn close(app: &AppHandle) {
    let app = app.clone();
    // Window ordering belongs to the main thread; a command handler isn't.
    let _ = app.clone().run_on_main_thread(move || {
        // A click on the panel made Handy the active application. Give that
        // back to the application the user was in before the panels go: a
        // key window going away while Handy is active makes AppKit bring
        // Handy's next window, the settings window, to the front.
        platform::restore_front();
        for panel in panels(&app) {
            if let Err(e) = hide_panel(&app, &panel) {
                warn!("Could not hide picker panel {}: {e}", panel.label());
            }
        }
    });
}

/// Where a panel's own point is on screen.
fn on_screen(panel: &WebviewWindow, x: f64, y: f64) -> Option<LogicalPosition<f64>> {
    let scale = panel.scale_factor().ok()?;
    let origin = panel.outer_position().ok()?.to_logical::<f64>(scale);
    Some(LogicalPosition::new(origin.x + x, origin.y + y))
}

/// A window under the pointer, in the panel's coordinates.
#[derive(Clone, Debug, Serialize, Type)]
pub struct PickerHover {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub app_name: String,
}

/// The windows under the pointer, front to back, so the user can reach one
/// that another covers.
#[tauri::command]
#[specta::specta]
pub fn picker_hover(window: WebviewWindow, x: f64, y: f64) -> Vec<PickerHover> {
    let Some(point) = on_screen(&window, x, y) else {
        return Vec::new();
    };
    platform::windows_at(point.x, point.y)
        .into_iter()
        .map(|found| PickerHover {
            x: found.x - (point.x - x),
            y: found.y - (point.y - y),
            width: found.width,
            height: found.height,
            app_name: found.app_name,
        })
        .collect()
}

/// Picks the window at `depth` under the pointer (0 is the front one) as the
/// redirect target and takes the picker down. `false` when there's no window
/// there to pick; the picker stays up for another try.
#[tauri::command]
#[specta::specta]
pub fn picker_pick(app: AppHandle, window: WebviewWindow, x: f64, y: f64, depth: u32) -> bool {
    let picked: Option<WindowInfo> =
        on_screen(&window, x, y).and_then(|point| platform::pick(point.x, point.y, depth as usize));
    let Some(picked) = picked else {
        return false;
    };
    close(&app);
    super::start(&app, picked);
    true
}

#[tauri::command]
#[specta::specta]
pub fn picker_cancel(app: AppHandle) {
    close(&app);
}
