use crate::input;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use log::debug;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

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

// Native overlay window sizes (logical points). One window is reused for every
// state and resized in `show_overlay_state`; each size need only be at least as
// large as the card it hosts (the `--ov-*` vars in RecordingOverlay.css). The
// card is CSS-anchored flush to the screen edge, so window height doesn't move
// where the card sits — only OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET do. Keep
// these in sync with the CSS card geometry.
//
// Compact overlay (Minimal / transcribing / processing): the 40h pill animates
// width from 172 (--ov-rest-w) to 216 (--ov-work-w) and expands from center, so
// the window must fit the widest state plus a little slack.
const OVERLAY_WIDTH: f64 = 256.0;
const OVERLAY_HEIGHT: f64 = 46.0;

// Actual is 394x118, just a little extra
const OVERLAY_STREAM_WIDTH: f64 = 400.0;
const OVERLAY_STREAM_HEIGHT: f64 = 120.0;

/// Overlay window size (logical) for a given UI state.
fn overlay_dimensions(state: &str) -> (f64, f64) {
    if state == "streaming" {
        (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT)
    } else {
        (OVERLAY_WIDTH, OVERLAY_HEIGHT)
    }
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

#[cfg(target_os = "macos")]
const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;

#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

/// Configures the edge and offset of a GTK layer surface. gtk-layer-shell
/// commits anchor and margin changes itself, including while the surface is
/// mapped, so changing position does not require a manual hide/show cycle.
#[cfg(target_os = "linux")]
fn configure_layer_shell_position(gtk_window: &gtk::ApplicationWindow, position: OverlayPosition) {
    let (edge, opposite_edge, margin) = match position {
        OverlayPosition::Top => (Edge::Top, Edge::Bottom, OVERLAY_TOP_OFFSET),
        OverlayPosition::Bottom => (Edge::Bottom, Edge::Top, OVERLAY_BOTTOM_OFFSET),
    };

    gtk_window.set_anchor(edge, true);
    gtk_window.set_anchor(opposite_edge, false);
    gtk_window.set_layer_shell_margin(edge, margin.round() as i32);
    gtk_window.set_layer_shell_margin(opposite_edge, 0);
}

/// Configures a GTK layer surface's position and size.
///
/// Tauri's normal `set_size` path calls `gtk_window_resize`, but layer surfaces
/// derive their dimensions from GTK's size request. gtk-layer-shell documents
/// the `set_size_request` + `resize(1, 1)` sequence for forcing a new size.
///
/// Also used to "park" the surface back at the compact size before hiding —
/// see `hide_recording_overlay`.
#[cfg(target_os = "linux")]
fn configure_layer_shell_surface(
    gtk_window: &gtk::ApplicationWindow,
    position: OverlayPosition,
    width: f64,
    height: f64,
) {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    configure_layer_shell_position(gtk_window, position);

    gtk_window.set_size_request(
        width.round().max(1.0) as i32,
        height.round().max(1.0) as i32,
    );
    gtk_window.resize(1, 1);
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
    if env_flag_enabled("HANDY_NO_GTK_LAYER_SHELL") {
        debug!("Skipping GTK layer shell init (HANDY_NO_GTK_LAYER_SHELL is enabled)");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    // Try to get the GTK window from the Tauri webview
    if let Ok(gtk_window) = overlay_window.gtk_window() {
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);

        let overlay_position = settings::get_settings(overlay_window.app_handle()).overlay_position;
        configure_layer_shell_surface(&gtk_window, overlay_position, OVERLAY_WIDTH, OVERLAY_HEIGHT);

        let initialized = gtk_window.is_layer_window();
        LAYER_SHELL_ACTIVE.store(initialized, Ordering::SeqCst);
        return initialized;
    }
    false
}

/// Forces a window to be topmost using Win32 API (Windows only)
/// This is more reliable than Tauri's set_always_on_top which can be overridden
#[cfg(target_os = "windows")]
fn force_overlay_topmost(overlay_window: &tauri::webview::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
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
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    });
}

fn get_monitor_with_cursor(app_handle: &AppHandle) -> Option<tauri::Monitor> {
    if let Some(mouse_location) = input::get_cursor_position(app_handle) {
        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                // On Windows both the cursor (enigo -> GetCursorPos) and the
                // monitor bounds are physical pixels, so compare them directly.
                #[cfg(target_os = "windows")]
                if is_mouse_within_monitor(mouse_location, monitor.position(), monitor.size()) {
                    return Some(monitor);
                }

                // macOS/Linux: enigo returns logical coords, so scale the bounds down.
                #[cfg(not(target_os = "windows"))]
                {
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
/// The Bottom anchor uses the macOS work area (visibleFrame) so the overlay
/// tracks the Dock — above it when shown, at the screen edge when hidden.
/// This relies on tauri 2.11's work_area.position.y fix (#14655), the same
/// bug that led PR #969 to abandon work_area for full monitor bounds. Top and
/// the other platforms keep full monitor bounds plus the fixed offsets
/// (work_area is unreliable on Wayland; Windows' offset clears the taskbar).
///
/// We must use LogicalPosition (not PhysicalPosition) because Tauri/tao
/// converts PhysicalPosition using the scale factor of the monitor the window
/// is *currently* on, which is wrong when moving cross-monitor. Windows uses
/// `place_windows_overlay` instead (no single logical space across mixed DPI).
fn calculate_overlay_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;

    let settings = settings::get_settings(app_handle);

    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = match settings.overlay_position {
        OverlayPosition::Top => monitor_y + OVERLAY_TOP_OFFSET,
        OverlayPosition::Bottom => {
            // work_area.position shares monitor.position's global coordinate
            // space, so no monitor offset is added.
            #[cfg(target_os = "macos")]
            let bottom = {
                let wa = monitor.work_area();
                (wa.position.y as f64 + wa.size.height as f64) / scale
            };
            #[cfg(not(target_os = "macos"))]
            let bottom = monitor_y + monitor.size().height as f64 / scale;

            bottom - height - OVERLAY_BOTTOM_OFFSET
        }
    };

    Some((x, y))
}

/// Current overlay window size in logical units (points), for repositioning
/// without assuming a fixed size (compact vs. streaming).
#[cfg(not(target_os = "windows"))]
fn current_overlay_logical_size(window: &tauri::webview::WebviewWindow) -> Option<(f64, f64)> {
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some((size.width as f64 / scale, size.height as f64 / scale))
}

#[cfg(target_os = "windows")]
static WINDOWS_OVERLAY_IS_STREAMING: AtomicBool = AtomicBool::new(false);

/// Overlay rectangle in the destination monitor's physical pixels, so nothing
/// is converted through the window's previous-monitor DPI.
#[cfg(target_os = "windows")]
fn windows_overlay_bounds(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    scale: f64,
    logical_width: f64,
    logical_height: f64,
    overlay_position: OverlayPosition,
) -> (i32, i32, i32, i32) {
    let width = (logical_width * scale).round().max(1.0) as i32;
    let height = (logical_height * scale).round().max(1.0) as i32;
    let x = (monitor_position.x as f64 + (monitor_size.width as f64 - width as f64) / 2.0).round()
        as i32;
    let y = match overlay_position {
        OverlayPosition::Top => {
            (monitor_position.y as f64 + OVERLAY_TOP_OFFSET * scale).round() as i32
        }
        OverlayPosition::Bottom => (monitor_position.y as f64 + monitor_size.height as f64
            - height as f64
            - OVERLAY_BOTTOM_OFFSET * scale)
            .round() as i32,
    };

    (x, y, width, height)
}

/// Moves and sizes the overlay in one native SetWindowPos, bypassing tao's
/// current-DPI logical conversion that mislands cross-monitor moves.
#[cfg(target_os = "windows")]
fn place_windows_overlay(
    app_handle: &AppHandle,
    overlay_window: &tauri::webview::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};

    let monitor = get_monitor_with_cursor(app_handle)
        .ok_or_else(|| "failed to determine the monitor containing the cursor".to_string())?;
    let (x, y, width, height) = windows_overlay_bounds(
        *monitor.position(),
        *monitor.size(),
        monitor.scale_factor(),
        logical_width,
        logical_height,
        settings::get_settings(app_handle).overlay_position,
    );
    let hwnd = overlay_window
        .hwnd()
        .map_err(|error| format!("failed to get overlay window handle: {error}"))?;

    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| format!("failed to set overlay bounds: {error}"))?;
    }

    log::debug!(
        "windows overlay bounds: x={} y={} width={} height={} scale={}",
        x,
        y,
        width,
        height,
        monitor.scale_factor()
    );
    Ok(())
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT);
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
    .focusable(false)
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

/// Creates the recording overlay panel and keeps it hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    if let Some((x, y)) = calculate_overlay_position(app_handle, OVERLAY_WIDTH, OVERLAY_HEIGHT) {
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
            }
            Err(e) => {
                log::error!("Failed to create recording overlay panel: {}", e);
            }
        }
    }
}

/// Logical visibility of the overlay surface. All transitions are driven
/// through the controller below instead of fire-and-forget show/hide calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OverlayPhase {
    Hidden,
    Visible,
    /// A hide chain (park + commit, then unmap) is in flight.
    Hiding,
}

/// What the overlay should settle into. A show arriving mid-hide rewrites
/// `want` instead of racing the chain; the chain applies it at the end.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Desired {
    Hidden,
    Visible(String),
}

struct OverlayController {
    phase: OverlayPhase,
    /// Id of the operation that currently owns the overlay (a transcription
    /// start). Doubles as the id seed: `begin_operation` increments it in
    /// place. Hides carrying an older id are ignored, so a stale hide from a
    /// finished operation can never unmap a newer session's overlay.
    operation_id: u64,
    want: Desired,
}

static OVERLAY: Mutex<OverlayController> = Mutex::new(OverlayController {
    phase: OverlayPhase::Hidden,
    operation_id: 0,
    want: Desired::Hidden,
});

impl OverlayController {
    /// A new transcription takes over overlay ownership; hides from older
    /// operations become no-ops.
    fn begin_operation(&mut self) -> u64 {
        self.operation_id += 1;
        self.operation_id
    }

    /// Mutation half of `show_overlay_state`: rewrite `want`, then apply
    /// immediately unless a hide chain is in flight (a deferred show is
    /// applied by the chain end instead).
    fn request_show(&mut self, state: &str) -> ShowEffect {
        self.want = Desired::Visible(state.to_string());
        let effect = decide_show(self.phase);
        if effect == ShowEffect::Immediate {
            self.phase = OverlayPhase::Visible;
        }
        effect
    }

    /// Mutation half of `hide_recording_overlay`: consult `decide_hide` and
    /// mutate accordingly (chain start / retarget / ignore).
    fn request_hide(&mut self, operation_id: u64) -> HideDecision {
        let decision = decide_hide(self.phase, operation_id, self.operation_id);
        match decision {
            HideDecision::Ignore => {}
            HideDecision::RetargetChain => {
                // A chain is already running (fast stop after a re-trigger):
                // re-target it, its end will unmap.
                self.want = Desired::Hidden;
            }
            HideDecision::StartChain => {
                self.phase = OverlayPhase::Hiding;
                self.want = Desired::Hidden;
            }
        }
        decision
    }

    /// The hide-chain end: apply `want`. Visible re-shows without unmapping.
    /// On Hidden the phase flips while the caller still holds the lock across
    /// the unmap, so a concurrent show cannot slip between the unmap and the
    /// phase transition.
    fn settle_chain_end(&mut self) -> Desired {
        match self.want.clone() {
            want @ Desired::Visible(_) => {
                self.phase = OverlayPhase::Visible;
                want
            }
            Desired::Hidden => {
                self.phase = OverlayPhase::Hidden;
                Desired::Hidden
            }
        }
    }

    /// The overlay window is gone — settle so future shows apply immediately
    /// instead of deferring to a chain that will never end.
    fn reset_to_hidden(&mut self) {
        self.phase = OverlayPhase::Hidden;
        self.want = Desired::Hidden;
    }
}

/// What a hide request should do, given the controller state.
#[derive(Debug, PartialEq, Eq)]
enum HideDecision {
    /// Stale operation id, duplicate hide, or already hidden.
    Ignore,
    /// Overlay is up: start the hide chain.
    StartChain,
    /// A chain is already running for this op: just re-target it to Hidden.
    RetargetChain,
}

fn decide_hide(phase: OverlayPhase, operation_id: u64, current_operation_id: u64) -> HideDecision {
    if operation_id != current_operation_id {
        return HideDecision::Ignore;
    }
    match phase {
        OverlayPhase::Visible => HideDecision::StartChain,
        OverlayPhase::Hiding => HideDecision::RetargetChain,
        OverlayPhase::Hidden => HideDecision::Ignore,
    }
}

/// Whether a show applies its geometry immediately or is deferred to the
/// running hide chain's end (which re-shows instead of unmapping).
#[derive(Debug, PartialEq, Eq)]
enum ShowEffect {
    Immediate,
    Deferred,
}

fn decide_show(phase: OverlayPhase) -> ShowEffect {
    match phase {
        OverlayPhase::Hiding => ShowEffect::Deferred,
        _ => ShowEffect::Immediate,
    }
}

/// Starts a new overlay-owning operation (a transcription start) and returns
/// its id. Hides from older operations become no-ops.
pub fn begin_operation() -> u64 {
    OVERLAY.lock().unwrap().begin_operation()
}

/// The id of the operation that currently owns the overlay.
pub fn current_op() -> u64 {
    OVERLAY.lock().unwrap().operation_id
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Whether the overlay shows at all is governed by overlay_style; position
    // only chooses Top vs Bottom placement. Checked here (off the main thread)
    // so the common overlay-disabled case never pays for a main-thread hop.
    let settings = settings::get_settings(app_handle);
    if settings.overlay_style == OverlayStyle::None {
        return;
    }

    {
        let mut controller = OVERLAY.lock().unwrap();
        if controller.request_show(state) == ShowEffect::Deferred {
            // A hide chain is in flight: it finishes the park repaint, then
            // re-shows this state at the end without ever unmapping. Emitting
            // or resizing now would paint the new card into the compact
            // parked frame; deferring keeps the parked scene transparent.
            return;
        }
    }

    // The rest queries monitors and the cursor and mutates window geometry. On
    // Linux the monitor/cursor lookups hit GDK/Xlib on the process's shared X11
    // connection, which is only safe from the GTK main thread — running them on
    // a background thread corrupts the connection and hard-crashes the app
    // (issue #227). Hop to the main thread on every platform to keep the
    // geometry path uniform (a no-op cost on Windows, and it also keeps macOS's
    // NSScreen access main-thread-correct). run_on_main_thread runs the closure
    // inline when already on the main thread, so this never deadlocks.
    let handle = app_handle.clone();
    let state = state.to_string();
    let _ = app_handle.run_on_main_thread(move || show_overlay_state_on_main(&handle, &state));
}

fn show_overlay_state_on_main(app_handle: &AppHandle, state: &str) {
    // Size the overlay for this state (compact vs. streaming), then position it.
    let (width, height) = overlay_dimensions(state);
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        let shown_with_layer_shell = if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                Ok(gtk_window) => {
                    configure_layer_shell_surface(&gtk_window, position, width, height)
                }
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            let _ = overlay_window.show();
            true
        } else {
            false
        };
        #[cfg(not(target_os = "linux"))]
        let shown_with_layer_shell = false;

        if !shown_with_layer_shell {
            let size_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let _ =
                overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            #[cfg(target_os = "windows")]
            WINDOWS_OVERLAY_IS_STREAMING.store(state == "streaming", Ordering::Relaxed);
            let size_elapsed = size_started.elapsed();

            let pos_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let set_pos_elapsed =
                if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                    let set_pos_started = std::time::Instant::now();
                    let _ = overlay_window
                        .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
                    set_pos_started.elapsed()
                } else {
                    std::time::Duration::ZERO
                };
            #[cfg(target_os = "windows")]
            let set_pos_elapsed = {
                let set_pos_started = std::time::Instant::now();
                if let Err(error) =
                    place_windows_overlay(app_handle, &overlay_window, width, height)
                {
                    log::error!("Failed to place recording overlay: {error}");
                }
                set_pos_started.elapsed()
            };
            let pos_calc_elapsed = pos_started.elapsed() - set_pos_elapsed;

            let show_started = std::time::Instant::now();
            let _ = overlay_window.show();
            let show_elapsed = show_started.elapsed();

            // On Windows, aggressively re-assert "topmost" in the native Z-order after showing
            #[cfg(target_os = "windows")]
            force_overlay_topmost(&overlay_window);

            // Re-assert bounds after show(): the pre-show move crosses the DPI
            // boundary, and tao's WM_DPICHANGED reflow clobbers the first placement.
            #[cfg(target_os = "windows")]
            if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
                log::error!("Failed to re-assert recording overlay position: {error}");
            }

            log::debug!(
                "overlay '{}': set_size={:?} pos_calc={:?} set_pos={:?} show={:?}",
                state,
                size_elapsed,
                pos_calc_elapsed,
                set_pos_elapsed,
                show_elapsed
            );
        }

        let _ = overlay_window.emit("show-overlay", state);
    }
}

/// Notify the visible recording overlay that the input stream has delivered its
/// first sample chunk. Audio feedback uses the same backend readiness signal,
/// but this targeted event is skipped when overlays are disabled.
pub fn emit_recording_ready(app_handle: &AppHandle) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Showing the overlay is also queued onto the main thread. Queue readiness
    // there as well so a very fast always-on stream cannot overtake show-overlay
    // and then get reset back to the arming state by the frontend.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let _ = handle.emit_to("recording_overlay", "recording-ready", ());
    });
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the larger streaming overlay that displays live transcription text
pub fn show_streaming_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "streaming");
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
    // Positioning queries monitors/cursor (GDK/Xlib on Linux) and moves the
    // window, so it must run on the main thread — see show_overlay_state.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || update_overlay_position_on_main(&handle));
}

fn update_overlay_position_on_main(app_handle: &AppHandle) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        #[cfg(target_os = "linux")]
        if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                Ok(gtk_window) => configure_layer_shell_position(&gtk_window, position),
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            return;
        }

        #[cfg(target_os = "windows")]
        {
            let state = if WINDOWS_OVERLAY_IS_STREAMING.load(Ordering::Relaxed) {
                "streaming"
            } else {
                "recording"
            };
            let (width, height) = overlay_dimensions(state);
            if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
                log::error!("Failed to update recording overlay position: {error}");
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // Use the window's current size so centering stays correct whether the
            // overlay is in compact or streaming layout.
            let (width, height) = current_overlay_logical_size(&overlay_window)
                .unwrap_or((OVERLAY_WIDTH, OVERLAY_HEIGHT));
            if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                let _ = overlay_window
                    .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            }
        }
    }
}

/// Hides the recording overlay window.
///
/// `operation_id` must be the id of the operation that owns the overlay being
/// hidden (see `begin_operation`). A hide carrying an older id is a no-op, so
/// a stale hide from a finished transcription can never unmap the overlay of
/// a newer session that started meanwhile.
pub fn hide_recording_overlay(app_handle: &AppHandle, operation_id: u64) {
    {
        let mut controller = OVERLAY.lock().unwrap();
        match controller.request_hide(operation_id) {
            HideDecision::Ignore | HideDecision::RetargetChain => return,
            HideDecision::StartChain => {}
        }
    }

    // Always hide the overlay regardless of settings - if setting was changed while recording,
    // we still want to hide it properly
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Emit event so the frontend unmounts the card. Note: the frontend
        // returns null when not visible, so no fade actually plays and there
        // is no animation delay worth waiting for.
        let _ = overlay_window.emit("hide-overlay", ());
        let handle = app_handle.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "linux")]
            if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
                // Give the webview a beat to process the hide-overlay event
                // and unmount the card (it returns null when not visible), so
                // the parked frame below repaints a clean transparent scene
                // instead of the old card clipped to the compact viewport.
                std::thread::sleep(std::time::Duration::from_millis(80));

                // WebKitGTK on Wayland repaints only damaged regions, so a
                // surface unmapped mid-transition keeps stale pixels in its
                // reused buffers (the "ghost card" glitch) — or maps again
                // with no fresh frame at all, showing nothing. Shrink the
                // layer surface back to the compact size while it is still
                // mapped: a real size change forces a full repaint with fresh
                // pixels. This runs UNCONDITIONALLY, even when a re-trigger
                // will re-show the overlay at the chain end — the park is the
                // repaint mechanism, skipping it is what brought the ghost
                // back on mid-chain re-triggers.
                let park_window = overlay_window.clone();
                let _ = overlay_window.run_on_main_thread(move || {
                    if let Ok(gtk_window) = park_window.gtk_window() {
                        let position =
                            settings::get_settings(park_window.app_handle()).overlay_position;
                        configure_layer_shell_surface(
                            &gtk_window,
                            position,
                            OVERLAY_WIDTH,
                            OVERLAY_HEIGHT,
                        );
                    }
                });
                // Give the GTK main loop a moment to commit the parked frame
                // before the surface is unmapped. This must happen off the
                // main thread — sleeping there would block the commit (and
                // pumping events manually via gtk::main_iteration_do kills
                // the layer surface on niri in any visibility state).
                std::thread::sleep(std::time::Duration::from_millis(130));
            }

            let end_window = overlay_window.clone();
            let _ = overlay_window.run_on_main_thread(move || {
                let want = {
                    let mut controller = OVERLAY.lock().unwrap();
                    match controller.settle_chain_end() {
                        want @ Desired::Visible(_) => want,
                        Desired::Hidden => {
                            // settle_chain_end already flipped the phase to
                            // Hidden; keep holding the lock across hide() so a
                            // concurrent show cannot slip between the unmap
                            // and the phase transition.
                            let _ = end_window.hide();
                            return;
                        }
                    }
                };
                // A show arrived while the chain was parking: apply it now.
                // The surface stayed mapped; resizing compact -> target forces
                // a full repaint, so no stale pixels can survive.
                if let Desired::Visible(state) = want {
                    show_overlay_state_on_main(&handle, &state);
                }
            });
        });
    } else {
        // Window is gone — settle the controller so future shows apply
        // immediately instead of waiting for a chain that will never end.
        OVERLAY.lock().unwrap().reset_to_hidden();
    }
}

// Cached "overlay is enabled" flag, kept in sync with overlay_style. Avoids
// reading the Tauri store on every audio callback (~24 Hz during recording).
// Defaults to false so the audio path doesn't emit until lib.rs::setup
// populates the cache from initial settings.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Tracks whether gtk-layer-shell was successfully initialized (Linux only).
/// Used to skip layer-shell calls when the window is a regular fallback.
#[cfg(target_os = "linux")]
static LAYER_SHELL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Update the cached overlay-enabled flag. Called from `lib.rs` at
/// startup after settings load, and from `change_overlay_style_setting`
/// whenever the user changes whether the overlay is shown.
pub fn update_overlay_enabled_cache(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn emit_levels(app_handle: &AppHandle, levels: &[f32]) {
    // Skip emission when the overlay is disabled. The recording_overlay
    // window is created at boot regardless of overlay_style, so without this
    // guard a hidden overlay's WebKit subprocess still
    // processes every event. Each event drives some kind of WebKit
    // C++ allocation that accumulates without bound (mechanism not
    // directly characterized; see issue #1279 for the investigation).
    // For users with `overlay_style: none` (the Linux default) this skip
    // eliminates the upstream driver of that accumulation.
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Throttle to ~30 FPS. Even with the overlay enabled, the raw audio
    // callback fires far faster than the UI needs; capping emission rate
    // cuts the per-frame `eval_script`/IPC volume that drives the wry
    // memory growth in issue #1279 (upstream tauri-apps/wry#1489).
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last = LAST_MIC_LEVEL_EMIT.load(Ordering::Relaxed);
    if now.saturating_sub(last) < EMIT_THROTTLE_MS {
        return;
    }
    LAST_MIC_LEVEL_EMIT.store(now, Ordering::Relaxed);

    // Target only the overlay window. In Tauri 2 both `AppHandle::emit`
    // and `WebviewWindow::emit` broadcast to all webviews; Tauri's
    // listener filter then skips webviews with no registered listener
    // for the event, so the settings webview never received `mic-level`.
    // But the previous dual-call pattern still produced two `eval_script`
    // calls to the overlay per audio callback (one from each .emit()).
    // `emit_to` with the overlay's window label produces a single
    // eval_script call per callback, cutting the per-callback WebKit
    // dispatch work in half.
    let _ = app_handle.emit_to("recording_overlay", "mic-level", levels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_hit_test_uses_half_open_physical_bounds() {
        let position = PhysicalPosition::new(-2560, -200);
        let size = PhysicalSize::new(2560, 1440);

        assert!(is_mouse_within_monitor((-2560, -200), &position, &size));
        assert!(is_mouse_within_monitor((-1, 1239), &position, &size));
        assert!(!is_mouse_within_monitor((0, 0), &position, &size));
        assert!(!is_mouse_within_monitor((-1, 1240), &position, &size));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_cursor_hit_test_does_not_scale_physical_monitor_bounds() {
        let position = PhysicalPosition::new(1920, 0);
        let size = PhysicalSize::new(3840, 2160);
        let cursor = (5000, 1000);

        assert!(is_mouse_within_monitor(cursor, &position, &size));

        // This is the old mixed-coordinate comparison. It excludes a cursor
        // that is visibly inside a secondary display running at 150%.
        let scale = 1.5;
        let logical_position = PhysicalPosition::new(
            (position.x as f64 / scale) as i32,
            (position.y as f64 / scale) as i32,
        );
        let logical_size = PhysicalSize::new(
            (size.width as f64 / scale) as u32,
            (size.height as f64 / scale) as u32,
        );
        assert!(!is_mouse_within_monitor(
            cursor,
            &logical_position,
            &logical_size
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_use_destination_monitor_scale() {
        let monitor_position = PhysicalPosition::new(1920, 0);
        let monitor_size = PhysicalSize::new(3840, 2160);

        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (3648, 2031, 384, 69)
        );
        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Top,
            ),
            (3648, 6, 384, 69)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_support_negative_monitor_origins() {
        assert_eq!(
            windows_overlay_bounds(
                PhysicalPosition::new(-2560, -200),
                PhysicalSize::new(2560, 1440),
                1.25,
                OVERLAY_STREAM_WIDTH,
                OVERLAY_STREAM_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (-1530, 1040, 500, 150)
        );
    }
}

/// Exhaustive transition coverage for the overlay visibility controller
/// (`OverlayPhase` × op freshness → `HideDecision`, `OverlayPhase` →
/// `ShowEffect`) plus multi-step scenario sequences that mirror the
/// controller mutations performed by `show_overlay_state`,
/// `hide_recording_overlay`, and the hide-chain end.
#[cfg(test)]
mod visibility_controller_tests {
    use super::*;

    const OP1: u64 = 1;
    const OP2: u64 = 2;

    /// A fresh controller in the state the real OVERLAY static is in after
    /// operation OP1 took ownership. Sequence tests drive the same
    /// `request_show` / `request_hide` / `settle_chain_end` methods the
    /// production show/hide paths call, so they cannot drift from them.
    fn controller() -> OverlayController {
        OverlayController {
            phase: OverlayPhase::Hidden,
            operation_id: OP1,
            want: Desired::Hidden,
        }
    }

    // ---- decide_hide: full (phase x op freshness) matrix ----

    #[test]
    fn hide_on_visible_with_current_op_starts_chain() {
        let decision = decide_hide(OverlayPhase::Visible, OP1, OP1);
        assert_eq!(decision, HideDecision::StartChain);
    }

    #[test]
    fn hide_on_hiding_with_current_op_retargets_chain() {
        let decision = decide_hide(OverlayPhase::Hiding, OP1, OP1);
        assert_eq!(decision, HideDecision::RetargetChain);
    }

    #[test]
    fn hide_on_hidden_with_current_op_is_ignored() {
        let decision = decide_hide(OverlayPhase::Hidden, OP1, OP1);
        assert_eq!(decision, HideDecision::Ignore);
    }

    /// A stale op id must lose to the phase in every phase — otherwise a late
    /// hide from a finished operation could unmap a newer session's overlay.
    #[test]
    fn stale_hide_is_ignored_in_every_phase() {
        for phase in [
            OverlayPhase::Hidden,
            OverlayPhase::Visible,
            OverlayPhase::Hiding,
        ] {
            let decision = decide_hide(phase, OP1, OP2);
            assert_eq!(decision, HideDecision::Ignore, "phase {phase:?}");
        }
    }

    // ---- decide_show: every phase ----

    #[test]
    fn show_on_hidden_applies_immediately() {
        let effect = decide_show(OverlayPhase::Hidden);
        assert_eq!(effect, ShowEffect::Immediate);
    }

    #[test]
    fn show_on_visible_applies_immediately() {
        // A state switch (recording -> streaming) while already up must not
        // wait for anything.
        let effect = decide_show(OverlayPhase::Visible);
        assert_eq!(effect, ShowEffect::Immediate);
    }

    #[test]
    fn show_on_hiding_is_deferred_to_chain_end() {
        let effect = decide_show(OverlayPhase::Hiding);
        assert_eq!(effect, ShowEffect::Deferred);
    }

    // ---- scenario sequences ----

    /// The boring common path: show, then hide; the chain end unmaps.
    #[test]
    fn normal_lifecycle_show_then_hide_settles_hidden() {
        let mut c = controller();

        let effect = c.request_show("recording");
        assert_eq!(effect, ShowEffect::Immediate);
        assert_eq!(c.phase, OverlayPhase::Visible);

        let decision = c.request_hide(OP1);
        assert_eq!(decision, HideDecision::StartChain);
        assert_eq!(c.phase, OverlayPhase::Hiding);
        assert_eq!(c.want, Desired::Hidden);

        let settled = c.settle_chain_end();
        assert_eq!(settled, Desired::Hidden);
        assert_eq!(c.phase, OverlayPhase::Hidden);
    }

    /// A re-trigger while the hide chain parks the surface: the show defers,
    /// the chain end re-shows instead of unmapping, and the old operation's
    /// late hide is powerless.
    #[test]
    fn new_operation_mid_chain_reshows_at_chain_end() {
        let mut c = controller();

        c.request_show("recording");
        let decision = c.request_hide(OP1);
        assert_eq!(decision, HideDecision::StartChain);

        // A new transcription starts mid-chain and takes ownership.
        let _ = c.begin_operation();
        let effect = c.request_show("streaming");
        assert_eq!(effect, ShowEffect::Deferred);
        assert_eq!(c.phase, OverlayPhase::Hiding);

        // The old operation's pipeline finishes late and tries to hide.
        let stale = c.request_hide(OP1);
        assert_eq!(stale, HideDecision::Ignore);
        assert_eq!(c.want, Desired::Visible("streaming".to_string()));

        let settled = c.settle_chain_end();
        assert_eq!(settled, Desired::Visible("streaming".to_string()));
        assert_eq!(c.phase, OverlayPhase::Visible);
    }

    /// A rapid show -> hide -> show -> hide burst on one operation must
    /// settle Hidden: the first hide starts the chain, the show re-targets
    /// `want`, the second hide overwrites it back.
    #[test]
    fn rapid_burst_settles_hidden() {
        let mut c = controller();

        c.request_show("recording");

        let first_hide = c.request_hide(OP1);
        assert_eq!(first_hide, HideDecision::StartChain);

        let reshow = c.request_show("streaming");
        assert_eq!(reshow, ShowEffect::Deferred);

        let second_hide = c.request_hide(OP1);
        assert_eq!(second_hide, HideDecision::RetargetChain);
        assert_eq!(c.phase, OverlayPhase::Hiding);
        assert_eq!(c.want, Desired::Hidden);

        let settled = c.settle_chain_end();
        assert_eq!(settled, Desired::Hidden);
        assert_eq!(c.phase, OverlayPhase::Hidden);
    }

    /// Stopping twice in a row mid-chain (double SIGUSR2 / double toggle):
    /// every extra hide re-targets the same chain; the end still unmaps.
    #[test]
    fn duplicate_hides_mid_chain_keep_hidden_target() {
        let mut c = controller();

        c.request_show("recording");

        let first_hide = c.request_hide(OP1);
        assert_eq!(first_hide, HideDecision::StartChain);

        let second_hide = c.request_hide(OP1);
        assert_eq!(second_hide, HideDecision::RetargetChain);

        let third_hide = c.request_hide(OP1);
        assert_eq!(third_hide, HideDecision::RetargetChain);
        assert_eq!(c.want, Desired::Hidden);

        let settled = c.settle_chain_end();
        assert_eq!(settled, Desired::Hidden);
        assert_eq!(c.phase, OverlayPhase::Hidden);
    }

    /// Switching the visible state (recording -> streaming) while the overlay
    /// is up applies immediately and keeps the phase Visible.
    #[test]
    fn state_switch_while_visible_is_immediate() {
        let mut c = controller();

        c.request_show("recording");
        let effect = c.request_show("streaming");

        assert_eq!(effect, ShowEffect::Immediate);
        assert_eq!(c.phase, OverlayPhase::Visible);
        assert_eq!(c.want, Desired::Visible("streaming".to_string()));
    }

    /// After a completed hide cycle the overlay is fully reusable: a new show
    /// applies immediately, a new hide starts a fresh chain.
    #[test]
    fn overlay_is_reusable_after_chain_completes() {
        let mut c = controller();

        c.request_show("recording");
        c.request_hide(OP1);
        c.settle_chain_end();
        assert_eq!(c.phase, OverlayPhase::Hidden);

        let op2 = c.begin_operation();
        let effect = c.request_show("recording");
        assert_eq!(effect, ShowEffect::Immediate);
        assert_eq!(c.phase, OverlayPhase::Visible);

        let decision = c.request_hide(op2);
        assert_eq!(decision, HideDecision::StartChain);

        c.settle_chain_end();
        assert_eq!(c.phase, OverlayPhase::Hidden);
    }

    /// A hide for an operation that never showed anything (e.g. recording
    /// failed to start) must not disturb a settled Hidden controller.
    #[test]
    fn hide_without_prior_show_is_a_noop() {
        let mut c = controller();

        let decision = c.request_hide(OP1);
        assert_eq!(decision, HideDecision::Ignore);
        assert_eq!(c.phase, OverlayPhase::Hidden);
        assert_eq!(c.want, Desired::Hidden);
    }

    /// If the overlay window is gone mid-chain, the controller settles back
    /// to Hidden so a later show applies immediately instead of deferring to
    /// a chain that will never end.
    #[test]
    fn gone_window_resets_controller_to_hidden() {
        let mut c = controller();

        c.request_show("recording");
        let decision = c.request_hide(OP1);
        assert_eq!(decision, HideDecision::StartChain);
        assert_eq!(c.phase, OverlayPhase::Hiding);

        c.reset_to_hidden();

        assert_eq!(c.phase, OverlayPhase::Hidden);
        assert_eq!(c.want, Desired::Hidden);
        let effect = c.request_show("recording");
        assert_eq!(effect, ShowEffect::Immediate);
    }
}
