//! Floating record button: a small always-on-top button docked to the right
//! edge of the primary monitor. Tapping it toggles transcription exactly like
//! `handy --toggle-transcription`, so Handy stays usable on touch devices
//! (tablets, 2-in-1s with the keyboard detached) where no shortcut can be
//! pressed.
//!
//! The window is never focusable — like the recording overlay — so tapping it
//! leaves keyboard focus in the text field the user was typing in, and the
//! transcript is pasted there.

use crate::settings;
use crate::tray::TrayIconState;
use log::{debug, error};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

const WINDOW_LABEL: &str = "floating_button";

// Logical size of the button window. Large enough to be an easy touch target.
const BUTTON_WIDTH: f64 = 44.0;
const BUTTON_HEIGHT: f64 = 72.0;

/// When the last drag step was applied (ms since the epoch); the display
/// watcher stays out of the way while a drag is in progress.
static LAST_DRAG_MS: AtomicU64 = AtomicU64::new(0);
const DRAG_QUIET_MS: u64 = 3000;

/// Last placement, used to skip redundant moves in the display watcher.
static LAST_PLACEMENT: Mutex<Option<(i32, i32, u32, u32)>> = Mutex::new(None);

static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(FloatingButtonPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Creates the floating button window (hidden) and shows it if enabled.
pub fn create_floating_button(app_handle: &AppHandle) {
    if build_window(app_handle) {
        debug!("Floating button window created (hidden)");
        if settings::get_settings(app_handle).show_floating_button {
            set_visible(app_handle, true);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn build_window(app_handle: &AppHandle) -> bool {
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        WINDOW_LABEL,
        tauri::WebviewUrl::App("src/floating-button/index.html".into()),
    )
    .title("Handy Record Button")
    .resizable(false)
    .inner_size(BUTTON_WIDTH, BUTTON_HEIGHT)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focusable(false)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    match builder.build() {
        Ok(_) => true,
        Err(e) => {
            error!("Failed to create floating button window: {e}");
            false
        }
    }
}

#[cfg(target_os = "macos")]
fn build_window(app_handle: &AppHandle) -> bool {
    match PanelBuilder::<_, FloatingButtonPanel>::new(app_handle, WINDOW_LABEL)
        .url(WebviewUrl::App("src/floating-button/index.html".into()))
        .title("Handy Record Button")
        .level(PanelLevel::Status)
        .size(tauri::Size::Logical(tauri::LogicalSize {
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
        }))
        .has_shadow(false)
        .transparent(true)
        .no_activate(true)
        .corner_radius(0.0)
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .with_window(|w| w.decorations(false).transparent(true).focusable(false))
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary(),
        )
        .build()
    {
        Ok(panel) => {
            panel.hide();
            true
        }
        Err(e) => {
            error!("Failed to create floating button panel: {e}");
            false
        }
    }
}

/// Shows or hides the button. Positioning touches monitors and window
/// geometry, so it always runs on the main thread (see overlay.rs).
pub fn set_visible(app_handle: &AppHandle, visible: bool) {
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        if visible {
            place(&handle, &window, None);
            let _ = window.show();
            #[cfg(target_os = "windows")]
            force_topmost(&window);
            start_display_watcher(&handle);
        } else {
            let _ = window.hide();
        }
    });
}

/// Forwards the recording state so the button can show it.
pub fn emit_state(app_handle: &AppHandle, state: TrayIconState) {
    let state = match state {
        TrayIconState::Idle => "idle",
        TrayIconState::Recording => "recording",
        TrayIconState::Transcribing => "transcribing",
    };
    let _ = app_handle.emit_to(WINDOW_LABEL, "floating-button-state", state);
}

/// Pixel rectangle `(x, y, width, height)` for the button, docked to the
/// right edge of the work area at `offset` (0.0 top – 1.0 bottom).
fn docked_bounds(
    work_position: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    scale: f64,
    offset: f64,
) -> (i32, i32, u32, u32) {
    let width = (BUTTON_WIDTH * scale).round().max(1.0) as u32;
    let height = (BUTTON_HEIGHT * scale).round().max(1.0) as u32;
    let x = work_position.x + work_size.width as i32 - width as i32;
    let free = (work_size.height as i32 - height as i32).max(0);
    let y = work_position.y + (offset.clamp(0.0, 1.0) * free as f64).round() as i32;
    (x, y, width, height)
}

/// Docks the window to the right edge. `y_override` (physical) is used while
/// dragging; otherwise the saved offset decides the height.
fn place(app_handle: &AppHandle, window: &tauri::WebviewWindow, y_override: Option<i32>) {
    let Ok(Some(monitor)) = app_handle.primary_monitor() else {
        return;
    };
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let offset = match y_override {
        Some(y) => {
            let height = (BUTTON_HEIGHT * scale).round();
            let free = (work.size.height as f64 - height).max(1.0);
            (y - work.position.y) as f64 / free
        }
        None => settings::get_settings(app_handle).floating_button_offset,
    };
    let bounds = docked_bounds(work.position, work.size, scale, offset);
    let (x, y, width, height) = bounds;
    // Resizing a transparent webview window repaints it from scratch, which
    // shows as a flicker when it happens on every drag step — so only resize
    // when the size actually changes (first show, DPI change).
    let target_size = PhysicalSize::new(width, height);
    if window.outer_size().ok() != Some(target_size) {
        let _ = window.set_size(target_size);
    }
    let _ = window.set_position(PhysicalPosition::new(x, y));
    if let Ok(mut last) = LAST_PLACEMENT.lock() {
        *last = Some(bounds);
    }
}

/// Re-docks the button when the display changes — e.g. a tablet rotating, or
/// the taskbar resizing — since no window event reports that.
fn start_display_watcher(app_handle: &AppHandle) {
    if WATCHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app_handle.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if now_ms().saturating_sub(LAST_DRAG_MS.load(Ordering::Relaxed)) < DRAG_QUIET_MS {
            continue;
        }
        let inner = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let Some(window) = inner.get_webview_window(WINDOW_LABEL) else {
                return;
            };
            if !window.is_visible().unwrap_or(false) {
                return;
            }
            // "Show desktop" (Win+D) minimizes the button along with
            // everything else; it has no taskbar entry to bring it back.
            if window.is_minimized().unwrap_or(false) {
                let _ = window.unminimize();
                place(&inner, &window, None);
                return;
            }
            let Ok(Some(monitor)) = inner.primary_monitor() else {
                return;
            };
            let target = docked_bounds(
                monitor.work_area().position,
                monitor.work_area().size,
                monitor.scale_factor(),
                settings::get_settings(&inner).floating_button_offset,
            );
            let changed = LAST_PLACEMENT
                .lock()
                .map(|last| *last != Some(target))
                .unwrap_or(false);
            if changed {
                debug!("Display changed, re-docking floating button");
                place(&inner, &window, None);
            }
        });
    });
}

#[cfg(target_os = "windows")]
fn force_topmost(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

/// Tap on the floating button: toggle transcription.
#[tauri::command]
#[specta::specta]
pub fn floating_button_pressed(app: AppHandle) {
    crate::signal_handle::send_transcription_input(&app, "transcribe", "floating_button");
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// One step of dragging the floating button: moves it vertically by `delta`
/// logical pixels (clamped to the work area). `finished` saves the position.
///
/// Deliberately synchronous, so the window has already moved when the call
/// returns: the frontend sends one step at a time and discards pointer events
/// that were measured against the window's previous position.
#[tauri::command]
#[specta::specta]
pub fn floating_button_drag_by(app: AppHandle, delta: f64, finished: bool) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    LAST_DRAG_MS.store(now_ms(), Ordering::Relaxed);
    if delta != 0.0 {
        place(
            &app,
            &window,
            Some(position.y + (delta * scale).round() as i32),
        );
    }

    if finished {
        LAST_DRAG_MS.store(0, Ordering::Relaxed);
        if let (Ok(Some(monitor)), Ok(placed)) = (app.primary_monitor(), window.outer_position()) {
            let work = monitor.work_area();
            let height = (BUTTON_HEIGHT * monitor.scale_factor()).round();
            let free = (work.size.height as f64 - height).max(1.0);
            let mut s = settings::get_settings(&app);
            s.floating_button_offset = ((placed.y - work.position.y) as f64 / free).clamp(0.0, 1.0);
            settings::write_settings(&app, s);
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn change_show_floating_button_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.show_floating_button = enabled;
    settings::write_settings(&app, s);
    set_visible(&app, enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docks_to_right_edge_of_work_area() {
        // 2880x1920 Surface at 200%, taskbar takes 48 px at the bottom.
        let (x, y, w, h) = docked_bounds(
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(2880, 1872),
            2.0,
            0.0,
        );
        assert_eq!((x, y, w, h), (2792, 0, 88, 144));

        let (_, bottom_y, _, _) = docked_bounds(
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(2880, 1872),
            2.0,
            1.0,
        );
        assert_eq!(bottom_y + 144, 1872);
    }

    #[test]
    fn clamps_offset_and_supports_offset_monitors() {
        let (x, y, _, _) = docked_bounds(
            PhysicalPosition::new(-1920, 100),
            PhysicalSize::new(1920, 1040),
            1.0,
            7.0,
        );
        assert_eq!(x, -44);
        assert_eq!(y, 100 + 1040 - 72);
    }
}
