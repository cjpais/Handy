use crate::input;
use crate::settings;
use crate::settings::OverlayPosition;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use log::debug;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel};

#[cfg(target_os = "linux")]
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[cfg(target_os = "linux")]
use std::env;

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(RecordingOverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

const OVERLAY_WIDTH: f64 = 172.0;
const OVERLAY_HEIGHT: f64 = 36.0;
const MEETING_PROMPT_WIDTH: f64 = 320.0;
const MEETING_PROMPT_HEIGHT: f64 = 80.0;
const MEETING_STOPPED_AUTO_CLOSE_MS: u64 = 5000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MeetingOverlayMode {
    Suggestion,
    Recording,
    Stopped,
    Hidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct MeetingOverlayPrompt {
    pub provider: String,
    pub title: String,
    pub source: crate::managers::meeting_assistant::MeetingPromptSource,
    pub start_time: String,
    pub join_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct MeetingOverlaySnapshot {
    pub sequence: u64,
    pub mode: MeetingOverlayMode,
    pub prompt: Option<MeetingOverlayPrompt>,
    pub recording_started_at: Option<String>,
}

static MEETING_OVERLAY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static MEETING_OVERLAY_SNAPSHOT: Lazy<Mutex<MeetingOverlaySnapshot>> = Lazy::new(|| {
    Mutex::new(MeetingOverlaySnapshot {
        sequence: 0,
        mode: MeetingOverlayMode::Hidden,
        prompt: None,
        recording_started_at: None,
    })
});

#[cfg(target_os = "macos")]
const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;

#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

#[cfg(target_os = "linux")]
fn update_gtk_layer_shell_anchors(overlay_window: &tauri::webview::WebviewWindow) {
    let window_clone = overlay_window.clone();
    let _ = overlay_window.run_on_main_thread(move || {
        // Try to get the GTK window from the Tauri webview
        if let Ok(gtk_window) = window_clone.gtk_window() {
            let settings = settings::get_settings(window_clone.app_handle());
            match settings.overlay_position {
                OverlayPosition::Top => {
                    gtk_window.set_anchor(Edge::Top, true);
                    gtk_window.set_anchor(Edge::Bottom, false);
                }
                OverlayPosition::Bottom | OverlayPosition::None => {
                    gtk_window.set_anchor(Edge::Bottom, true);
                    gtk_window.set_anchor(Edge::Top, false);
                }
            }
        }
    });
}

/// Returns true when the environment variable is set to a truthy value
/// (e.g. "1", "true", "yes", "on").
/// "0", "false", "no", "off" and empty string are treated as falsy (case-insensitive).
/// Returns false when the variable is not set.
#[cfg(target_os = "linux")]
fn env_flag_enabled(name: &str) -> bool {
    match env::var(name) {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        ),
        Err(_) => false,
    }
}

/// Initializes GTK layer shell for Linux overlay window
/// Returns true if layer shell was successfully initialized, false otherwise
#[cfg(target_os = "linux")]
fn init_gtk_layer_shell(overlay_window: &tauri::webview::WebviewWindow) -> bool {
    if env_flag_enabled("THEGAI_NO_GTK_LAYER_SHELL") {
        debug!("Skipping GTK layer shell init (THEGAI_NO_GTK_LAYER_SHELL is enabled)");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    // Try to get the GTK window from the Tauri webview
    if let Ok(gtk_window) = overlay_window.gtk_window() {
        // Initialize layer shell
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);

        update_gtk_layer_shell_anchors(overlay_window);

        return true;
    }
    false
}

/// Forces a window to be topmost using Win32 API (Windows only)
/// This is more reliable than Tauri's set_always_on_top which can be overridden
#[cfg(target_os = "windows")]
fn force_overlay_topmost(overlay_window: &tauri::webview::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    // Clone because run_on_main_thread takes 'static
    let overlay_clone = overlay_window.clone();

    // Make sure the Win32 call happens on the UI thread
    let _ = overlay_clone.clone().run_on_main_thread(move || {
        if let Ok(hwnd) = overlay_clone.hwnd() {
            unsafe {
                // Force Z-order: make this window topmost without changing size/pos or stealing focus
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
    });
}

fn get_monitor_with_cursor(app_handle: &AppHandle) -> Option<tauri::Monitor> {
    if let Some(mouse_location) = input::get_cursor_position(app_handle) {
        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                // Tauri's monitor position/size are physical pixels, but enigo
                // may return logical coordinates (confirmed on macOS via
                // NSEvent::mouseLocation; on Windows, GetCursorPos behavior
                // depends on the process DPI-awareness context). Dividing by
                // scale_factor normalizes to logical, which is safe regardless:
                // if enigo returns logical it matches directly, and if it returns
                // physical on a scale=1 monitor the division is a no-op.
                let scale = monitor.scale_factor();
                let pos = PhysicalPosition::new(
                    (monitor.position().x as f64 / scale) as i32,
                    (monitor.position().y as f64 / scale) as i32,
                );
                let size = PhysicalSize::new(
                    (monitor.size().width as f64 / scale) as u32,
                    (monitor.size().height as f64 / scale) as u32,
                );
                if is_mouse_within_monitor(mouse_location, &pos, &size) {
                    return Some(monitor);
                }
            }
        }
    }

    app_handle.primary_monitor().ok().flatten()
}

fn is_mouse_within_monitor(
    mouse_pos: (i32, i32),
    monitor_pos: &PhysicalPosition<i32>,
    monitor_size: &PhysicalSize<u32>,
) -> bool {
    let (mouse_x, mouse_y) = mouse_pos;
    let PhysicalPosition {
        x: monitor_x,
        y: monitor_y,
    } = *monitor_pos;
    let PhysicalSize {
        width: monitor_width,
        height: monitor_height,
    } = *monitor_size;

    mouse_x >= monitor_x
        && mouse_x < (monitor_x + monitor_width as i32)
        && mouse_y >= monitor_y
        && mouse_y < (monitor_y + monitor_height as i32)
}

/// Returns overlay position in logical coordinates (points on macOS).
///
/// Uses monitor position/size directly rather than work_area(), which can
/// return incorrect coordinates on macOS for monitors with negative positions.
/// The per-platform OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET constants
/// already account for system chrome (menu bar, taskbar).
///
/// We must use LogicalPosition (not PhysicalPosition) because Tauri/tao
/// converts PhysicalPosition using the scale factor of the monitor the window
/// is *currently* on, which is wrong when moving cross-monitor.
fn calculate_overlay_position(app_handle: &AppHandle) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;
    let monitor_height = monitor.size().height as f64 / scale;

    let settings = settings::get_settings(app_handle);

    let x = monitor_x + (monitor_width - OVERLAY_WIDTH) / 2.0;
    let y = match settings.overlay_position {
        OverlayPosition::Top => monitor_y + OVERLAY_TOP_OFFSET,
        OverlayPosition::Bottom | OverlayPosition::None => {
            monitor_y + monitor_height - OVERLAY_HEIGHT - OVERLAY_BOTTOM_OFFSET
        }
    };

    Some((x, y))
}

fn calculate_meeting_prompt_position(app_handle: &AppHandle) -> Option<(f64, f64)> {
    let monitor = match get_monitor_with_cursor(app_handle) {
        Some(m) => m,
        None => {
            log::warn!("calculate_meeting_prompt_position: get_monitor_with_cursor returned None");
            return None;
        }
    };
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;

    let x = monitor_x + monitor_width - MEETING_PROMPT_WIDTH - 24.0;
    let y = monitor_y + OVERLAY_TOP_OFFSET + 24.0;
    log::info!("calculate_meeting_prompt_position: monitor={:?}, scale={}, pos=({}, {})", monitor.name(), scale, x, y);
    Some((x, y))
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(app_handle);
        if position.is_none() {
            debug!("Failed to determine overlay position, not creating overlay window");
            return;
        }
    }

    // Position starts unset — update_overlay_position() sets the correct
    // LogicalPosition before the overlay is shown.
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "recording_overlay",
        tauri::WebviewUrl::App("src/overlay/index.html".into()),
    )
    .title("Recording")
    .resizable(false)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
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
                // Try to initialize GTK layer shell, ignore errors if compositor doesn't support it
                if init_gtk_layer_shell(&window) {
                    debug!("GTK layer shell initialized for overlay window");
                } else {
                    debug!("GTK layer shell not available, falling back to regular window");
                }
            }

            debug!("Recording overlay window created successfully (hidden)");
        }
        Err(e) => {
            debug!("Failed to create recording overlay window: {}", e);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn create_meeting_prompt_window(app_handle: &AppHandle) {
    if app_handle.get_webview_window("meeting_prompt").is_some() {
        return;
    }

    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "meeting_prompt",
        tauri::WebviewUrl::App("src/meeting_prompt/index.html".into()),
    )
    .title("Meeting Prompt")
    .resizable(false)
    .inner_size(MEETING_PROMPT_WIDTH, MEETING_PROMPT_HEIGHT)
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

    match builder.build() {
        Ok(_) => {
            log::debug!("Meeting prompt window created successfully (hidden)");
        }
        Err(e) => {
            log::error!("Failed to create meeting prompt window: {}", e);
        }
    }
}

/// Creates the recording overlay panel and keeps it hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    if let Some((x, y)) = calculate_overlay_position(app_handle) {
        // PanelBuilder creates a Tauri window then converts it to NSPanel.
        // The window remains registered, so get_webview_window() still works.
        match PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "recording_overlay")
            .url(WebviewUrl::App("src/overlay/index.html".into()))
            .title("Recording")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize {
                width: OVERLAY_WIDTH,
                height: OVERLAY_HEIGHT,
            }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
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
            }
            Err(e) => {
                log::error!("Failed to create recording overlay panel: {}", e);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn create_meeting_prompt_window(app_handle: &AppHandle) {
    if app_handle.get_webview_window("meeting_prompt").is_some() {
        return;
    }

    if let Some((x, y)) = calculate_meeting_prompt_position(app_handle) {
        let _ = PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "meeting_prompt")
            .url(WebviewUrl::App("src/meeting_prompt/index.html".into()))
            .title("Meeting Prompt")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize {
                width: MEETING_PROMPT_WIDTH,
                height: MEETING_PROMPT_HEIGHT,
            }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(false)
            .corner_radius(14.0)
            .with_window(|w| w.decorations(false).transparent(true))
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()
            .map(|panel| {
                let _ = panel.hide();
            });
    }
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Check if overlay should be shown based on position setting
    let settings = settings::get_settings(app_handle);
    if settings.overlay_position == OverlayPosition::None {
        return;
    }

    update_overlay_position(app_handle);

    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.show();

        // On Windows, aggressively re-assert "topmost" in the native Z-order after showing
        #[cfg(target_os = "windows")]
        force_overlay_topmost(&overlay_window);

        let _ = overlay_window.emit("show-overlay", state);
    }
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "transcribing");
}

/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}

/// Updates the overlay window position based on current settings
pub fn update_overlay_position(app_handle: &AppHandle) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        {
            update_gtk_layer_shell_anchors(&overlay_window);
        }

        if let Some((x, y)) = calculate_overlay_position(app_handle) {
            let _ = overlay_window
                .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        }
    }
}

/// Hides the recording overlay window with fade-out animation
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    // Always hide the overlay regardless of settings - if setting was changed while recording,
    // we still want to hide it properly
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Emit event to trigger fade-out animation
        let _ = overlay_window.emit("hide-overlay", ());
        // Hide the window after a short delay to allow animation to complete
        let window_clone = overlay_window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = window_clone.hide();
        });
    }
}

fn update_meeting_overlay_snapshot(mut snapshot: MeetingOverlaySnapshot) -> MeetingOverlaySnapshot {
    snapshot.sequence = MEETING_OVERLAY_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let mut state = MEETING_OVERLAY_SNAPSHOT
        .lock()
        .expect("meeting overlay snapshot poisoned");
    *state = snapshot.clone();
    snapshot
}

pub fn get_meeting_overlay_snapshot() -> MeetingOverlaySnapshot {
    MEETING_OVERLAY_SNAPSHOT
        .lock()
        .expect("meeting overlay snapshot poisoned")
        .clone()
}

fn emit_meeting_overlay_snapshot(
    app_handle: &AppHandle,
    snapshot: MeetingOverlaySnapshot,
) -> MeetingOverlaySnapshot {
    log::info!("emit_meeting_overlay_snapshot: snapshot={:?}", snapshot);
    let snapshot = update_meeting_overlay_snapshot(snapshot);

    if app_handle.get_webview_window("meeting_prompt").is_none() {
        log::info!("emit_meeting_overlay_snapshot: meeting_prompt window is None, creating it.");
        create_meeting_prompt_window(app_handle);
    }

    if let Some(window) = app_handle.get_webview_window("meeting_prompt") {
        if let Some((x, y)) = calculate_meeting_prompt_position(app_handle) {
            log::info!("emit_meeting_overlay_snapshot: setting position to ({}, {})", x, y);
            let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        } else {
            log::warn!("emit_meeting_overlay_snapshot: calculate_meeting_prompt_position returned None");
        }
        log::info!("emit_meeting_overlay_snapshot: calling window.show()");
        let show_res = window.show();
        log::info!("emit_meeting_overlay_snapshot: window.show() returned {:?}", show_res);
        #[cfg(target_os = "windows")]
        force_overlay_topmost(&window);
        log::info!("emit_meeting_overlay_snapshot: emitting meeting-overlay-show event");
        let emit_res = window.emit("meeting-overlay-show", snapshot.clone());
        log::info!("emit_meeting_overlay_snapshot: window.emit() returned {:?}", emit_res);
    } else {
        log::error!("emit_meeting_overlay_snapshot: Failed to get/create meeting_prompt window!");
    }

    snapshot
}

pub fn show_meeting_suggestion_overlay(app_handle: &AppHandle, prompt: MeetingOverlayPrompt) {
    emit_meeting_overlay_snapshot(
        app_handle,
        MeetingOverlaySnapshot {
            sequence: 0,
            mode: MeetingOverlayMode::Suggestion,
            prompt: Some(prompt),
            recording_started_at: None,
        },
    );
}

pub fn show_meeting_recording_overlay(app_handle: &AppHandle) {
    let prompt = get_meeting_overlay_snapshot().prompt;
    emit_meeting_overlay_snapshot(
        app_handle,
        MeetingOverlaySnapshot {
            sequence: 0,
            mode: MeetingOverlayMode::Recording,
            prompt,
            recording_started_at: Some(chrono::Utc::now().to_rfc3339()),
        },
    );
}

pub fn show_meeting_stopped_overlay(app_handle: &AppHandle) {
    let prompt = get_meeting_overlay_snapshot().prompt;
    let stopped_snapshot = emit_meeting_overlay_snapshot(
        app_handle,
        MeetingOverlaySnapshot {
            sequence: 0,
            mode: MeetingOverlayMode::Stopped,
            prompt,
            recording_started_at: None,
        },
    );

    let app_clone = app_handle.clone();
    let stopped_sequence = stopped_snapshot.sequence;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(
            MEETING_STOPPED_AUTO_CLOSE_MS,
        ));

        let current = get_meeting_overlay_snapshot();
        if current.sequence == stopped_sequence && current.mode == MeetingOverlayMode::Stopped {
            hide_meeting_prompt_window(&app_clone);
        }
    });
}

pub fn hide_meeting_prompt_window(app_handle: &AppHandle) {
    let snapshot = update_meeting_overlay_snapshot(MeetingOverlaySnapshot {
        sequence: 0,
        mode: MeetingOverlayMode::Hidden,
        prompt: None,
        recording_started_at: None,
    });

    if let Some(window) = app_handle.get_webview_window("meeting_prompt") {
        let _ = window.emit("meeting-overlay-show", snapshot);

        let window_clone = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = window_clone.hide();
        });
    }
}

pub fn emit_levels(app_handle: &AppHandle, levels: &Vec<f32>) {
    // emit levels to main app
    let _ = app_handle.emit("mic-level", levels);

    // also emit to the recording overlay if it's open
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        let _ = overlay_window.emit("mic-level", levels);
    }
}
