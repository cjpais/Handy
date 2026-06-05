//! Active-window highlight overlay. Windows-only.
//!
//! On Windows, this module spawns a 100ms polling thread that tracks the
//! foreground window via GetForegroundWindow + GetWindowRect, and repositions
//! a dedicated transparent click-through webview to frame it. On other
//! platforms, every public function is a no-op stub so the rest of the app
//! keeps calling them unconditionally.
//!
//! **Shutdown behaviour.** The tracker thread is cancelled via the shared
//! AtomicBool. It does not hold a Weak<AppHandle> — during app teardown, if
//! Tauri destroys the webview before the tracker notices the cancel flag,
//! the thread's next `window.show()` / `window.hide()` call is a harmless
//! no-op because Tauri's `WebviewWindow` methods silently return an error
//! when the underlying HWND is gone.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
#[cfg(target_os = "windows")]
use tauri::WebviewWindowBuilder;

/// Physical-pixel rectangle describing a foreground window's bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Abstracts the foreground-window query so the tracker loop can be
/// tested without real Win32 calls.
pub trait ForegroundRectProvider: Send + Sync {
    /// Returns the current foreground window's rect in physical pixels,
    /// or None if no valid target exists (null foreground, minimized,
    /// cloaked, or rect query failed).
    fn get(&self) -> Option<Rect>;
}

/// Lifecycle state for the tracker thread, stored in Tauri managed state.
///
/// The cancel flag is wrapped in `Mutex<Arc<_>>` so that `show_target_highlight`
/// can atomically swap in a FRESH `AtomicBool` when starting a new tracker,
/// rather than resetting the old flag (which would race against a stale thread
/// that hadn't yet seen the cancel signal).
pub struct TargetHighlightState {
    pub cancel: Mutex<Arc<AtomicBool>>,
    generation: Arc<AtomicU64>,
}

impl TargetHighlightState {
    pub fn new() -> Self {
        Self {
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn begin_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn should_apply_delayed_hide(&self, captured_generation: u64) -> bool {
        self.current_generation() == captured_generation
    }
}

/// Create the highlight webview window (hidden by default).
/// Called once at startup from `lib.rs::initialize_core_logic`.
#[cfg(target_os = "windows")]
pub fn create_target_highlight_window(app: &AppHandle) {
    use log::debug;

    let mut builder = WebviewWindowBuilder::new(
        app,
        "target_highlight",
        tauri::WebviewUrl::App("src/target-highlight/index.html".into()),
    )
    .title("Target Highlight")
    .resizable(false)
    .inner_size(100.0, 100.0) // Tiny placeholder — tracker thread resizes it.
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(false)
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
        Ok(window) => {
            apply_click_through_style(&window);
            debug!("Target highlight window created successfully (hidden)");
        }
        Err(e) => {
            log::error!("Failed to create target highlight window: {}", e);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn create_target_highlight_window(_app: &AppHandle) {
    // No-op on non-Windows.
}

/// Show the highlight and start tracking the foreground window.
/// Idempotent — safe to call multiple times. If the setting is disabled, returns early.
#[cfg(target_os = "windows")]
pub fn show_target_highlight(app: &AppHandle) {
    use log::{debug, warn};
    use tauri::Manager;

    // Respect the user setting.
    let settings = crate::settings::get_settings(app);
    if !settings.highlight_target_window {
        return;
    }

    let state = match app.try_state::<TargetHighlightState>() {
        Some(s) => s,
        None => {
            warn!("TargetHighlightState not managed; cannot show highlight");
            return;
        }
    };

    // Atomically signal any stale tracker to exit AND install a fresh cancel
    // flag for the new one. The old Arc is kept alive only by the stale thread
    // and drops when it exits — no coordination needed beyond this swap.
    let fresh_cancel = {
        let mut guard = state.cancel.lock().unwrap();
        guard.store(true, Ordering::Relaxed); // signal old thread
        *guard = Arc::new(AtomicBool::new(false)); // install new flag
        state.begin_generation();
        guard.clone()
    };

    let window = match app.get_webview_window("target_highlight") {
        Some(w) => w,
        None => {
            warn!("target_highlight window not found");
            return;
        }
    };

    let provider: Arc<dyn ForegroundRectProvider> = Arc::new(WinRectProvider);
    let highlight: Arc<dyn HighlightWindow> = Arc::new(TauriHighlightWindow::new(window));

    debug!("show_target_highlight: spawning tracker thread");
    std::thread::spawn(move || {
        run_tracker_loop(
            provider,
            highlight,
            fresh_cancel,
            Duration::from_millis(100),
        );
        debug!("target_highlight tracker thread exited");
    });
}

#[cfg(not(target_os = "windows"))]
pub fn show_target_highlight(_app: &AppHandle) {
    // No-op on non-Windows.
}

/// Hide the highlight. If `flash` is true, emits the flash event first and
/// defers the hide by 200ms so the confirmation animation can play.
/// Idempotent — safe to call multiple times.
#[cfg(target_os = "windows")]
pub fn hide_target_highlight(app: &AppHandle, flash: bool) {
    use log::debug;
    use tauri::{Emitter, Manager};

    // Stop the tracker thread. Capture the current generation so a delayed hide
    // cannot hide a newer recording's highlight window.
    let delayed_hide_generation = if let Some(state) = app.try_state::<TargetHighlightState>() {
        state.cancel.lock().unwrap().store(true, Ordering::Relaxed);
        Some((state.generation.clone(), state.current_generation()))
    } else {
        None
    };

    let window = match app.get_webview_window("target_highlight") {
        Some(w) => w,
        None => return,
    };

    if flash {
        debug!("hide_target_highlight: flash requested, deferring hide by 200ms");
        let _ = window.emit("target-highlight-flash", ());
        let window_clone = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            if let Some((generation, captured_generation)) = delayed_hide_generation {
                if generation.load(Ordering::Relaxed) != captured_generation {
                    return;
                }
            }
            let _ = window_clone.hide();
        });
    } else {
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "windows"))]
pub fn hide_target_highlight(_app: &AppHandle, _flash: bool) {
    // No-op on non-Windows.
}

/// Minimum size (in physical pixels) for a foreground rect to be considered
/// a valid highlight target. Anything smaller is treated as "no valid target"
/// per the design doc — prevents weird 2px frames around 10x10 collapsed panels.
const MIN_RECT_DIMENSION: i32 = 40;

#[cfg(target_os = "windows")]
pub struct WinRectProvider;

#[cfg(target_os = "windows")]
impl ForegroundRectProvider for WinRectProvider {
    fn get(&self) -> Option<Rect> {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Dwm::{
            DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, IsIconic,
        };

        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }

            if IsIconic(hwnd).as_bool() {
                return None;
            }

            // DWM cloak check — catches Win10/11 virtual-desktop hidden windows
            // and some UWP suspended windows.
            let mut cloaked: u32 = 0;
            let result = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            );
            if result.is_ok() && cloaked != 0 {
                return None;
            }

            // GetWindowRect includes invisible resize borders on modern Windows.
            // DWM's extended frame bounds match the visible target window.
            let mut rect = RECT::default();
            let result = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut rect as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            );

            if result.is_err() && GetWindowRect(hwnd, &mut rect).is_err() {
                return None;
            }

            rect_from_bounds(rect.left, rect.top, rect.right, rect.bottom)
        }
    }
}

fn rect_from_bounds(left: i32, top: i32, right: i32, bottom: i32) -> Option<Rect> {
    let width = right - left;
    let height = bottom - top;

    if width < MIN_RECT_DIMENSION || height < MIN_RECT_DIMENSION {
        return None;
    }

    Some(Rect {
        x: left,
        y: top,
        width,
        height,
    })
}

/// Abstracts the webview window operations so the tracker loop can be
/// tested without constructing a real Tauri window.
pub trait HighlightWindow: Send + Sync {
    fn set_geometry(&self, rect: &Rect);
    fn show(&self);
    fn hide(&self);
}

#[cfg(target_os = "windows")]
pub struct TauriHighlightWindow {
    window: tauri::WebviewWindow,
}

#[cfg(target_os = "windows")]
impl TauriHighlightWindow {
    pub fn new(window: tauri::WebviewWindow) -> Self {
        Self { window }
    }
}

#[cfg(target_os = "windows")]
impl HighlightWindow for TauriHighlightWindow {
    fn set_geometry(&self, rect: &Rect) {
        let _ = self
            .window
            .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: rect.x,
                y: rect.y,
            }));
        let _ = self
            .window
            .set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: rect.width.max(1) as u32,
                height: rect.height.max(1) as u32,
            }));
    }

    fn show(&self) {
        let _ = self.window.show();
        reinforce_topmost(&self.window);
    }

    fn hide(&self) {
        let _ = self.window.hide();
    }
}

#[cfg(target_os = "windows")]
fn apply_click_through_style(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };

    let window_clone = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(hwnd) = window_clone.hwnd() {
            unsafe {
                let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let new_style =
                    current | (WS_EX_TRANSPARENT.0 as isize) | (WS_EX_NOACTIVATE.0 as isize);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn reinforce_topmost(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    let window_clone = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(hwnd) = window_clone.hwnd() {
            unsafe {
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

/// Polling loop that drives the highlight window. Runs until the cancel flag
/// is set, polling the provider every `poll_interval`. On each tick:
/// - provider returns None → hide the window (keep looping)
/// - provider returns Some(rect) → set_geometry + show
///
/// This function is the unit of code the tests cover — all decision logic
/// lives here, and the Win32 / Tauri details live behind the traits.
///
/// **Cancellation race window.** The cancel flag is re-checked after the
/// provider call and before the window mutations. Without this, a caller
/// that flips `cancel` and calls `window.hide()` concurrently could find
/// the tracker thread subsequently re-showing the window — the tracker
/// having already entered the match arm with a fresh rect, then calling
/// `show()` after the caller's `hide()` completed. The post-provider
/// check narrows the race to a ~microsecond window between the load and
/// the Tauri IPC calls, which is small enough to be imperceptible.
pub fn run_tracker_loop<P, W>(
    provider: Arc<P>,
    window: Arc<W>,
    cancel: Arc<AtomicBool>,
    poll_interval: Duration,
) where
    P: ForegroundRectProvider + ?Sized,
    W: HighlightWindow + ?Sized,
{
    while !cancel.load(Ordering::Relaxed) {
        let rect = provider.get();
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match rect {
            Some(rect) => {
                window.set_geometry(&rect);
                window.show();
            }
            None => {
                window.hide();
            }
        }
        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test double — returns a pre-programmed sequence of rects.
    struct MockRectProvider {
        responses: Mutex<Vec<Option<Rect>>>,
    }

    impl MockRectProvider {
        fn new(responses: Vec<Option<Rect>>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    impl ForegroundRectProvider for MockRectProvider {
        fn get(&self) -> Option<Rect> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                None
            } else {
                responses.remove(0)
            }
        }
    }

    #[derive(Default)]
    struct MockHighlightWindowState {
        geometry_calls: Vec<Rect>,
        show_calls: u32,
        hide_calls: u32,
        currently_shown: bool,
    }

    struct MockHighlightWindow {
        state: Mutex<MockHighlightWindowState>,
    }

    impl MockHighlightWindow {
        fn new() -> Self {
            Self {
                state: Mutex::new(MockHighlightWindowState::default()),
            }
        }

        fn snapshot(&self) -> MockHighlightWindowState {
            let s = self.state.lock().unwrap();
            MockHighlightWindowState {
                geometry_calls: s.geometry_calls.clone(),
                show_calls: s.show_calls,
                hide_calls: s.hide_calls,
                currently_shown: s.currently_shown,
            }
        }
    }

    impl HighlightWindow for MockHighlightWindow {
        fn set_geometry(&self, rect: &Rect) {
            self.state.lock().unwrap().geometry_calls.push(rect.clone());
        }

        fn show(&self) {
            let mut s = self.state.lock().unwrap();
            s.show_calls += 1;
            s.currently_shown = true;
        }

        fn hide(&self) {
            let mut s = self.state.lock().unwrap();
            s.hide_calls += 1;
            s.currently_shown = false;
        }
    }

    #[test]
    fn rect_construction_and_fields() {
        let rect = Rect {
            x: 100,
            y: 200,
            width: 800,
            height: 600,
        };
        assert_eq!(rect.x, 100);
        assert_eq!(rect.y, 200);
        assert_eq!(rect.width, 800);
        assert_eq!(rect.height, 600);
    }

    #[test]
    fn rect_from_bounds_uses_visible_bounds() {
        assert_eq!(
            rect_from_bounds(0, 0, 1920, 1080),
            Some(Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            })
        );
    }

    #[test]
    fn rect_from_bounds_rejects_tiny_targets() {
        assert_eq!(rect_from_bounds(10, 20, 30, 80), None);
        assert_eq!(rect_from_bounds(10, 20, 80, 30), None);
    }

    #[test]
    fn mock_provider_returns_programmed_responses() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        };
        let provider = MockRectProvider::new(vec![Some(rect.clone()), None, Some(rect.clone())]);

        assert_eq!(provider.get(), Some(rect.clone()));
        assert_eq!(provider.get(), None);
        assert_eq!(provider.get(), Some(rect));
        assert_eq!(provider.get(), None); // drained
    }

    #[test]
    fn tracker_loop_exits_when_cancel_flag_set() {
        let provider = Arc::new(MockRectProvider::new(vec![]));
        let window = Arc::new(MockHighlightWindow::new());
        let cancel = Arc::new(AtomicBool::new(true)); // pre-cancelled

        run_tracker_loop(
            provider,
            window.clone(),
            cancel,
            std::time::Duration::from_millis(1),
        );

        // If we got here without hanging, the loop exited. Additionally,
        // a pre-cancelled loop should not have made any window calls.
        let snap = window.snapshot();
        assert_eq!(snap.show_calls, 0);
        assert_eq!(snap.hide_calls, 0);
        assert_eq!(snap.geometry_calls.len(), 0);
    }

    #[test]
    fn tracker_loop_hides_on_none_then_continues() {
        let rect = Rect {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        };
        let provider = Arc::new(MockRectProvider::new(vec![
            None,               // tick 1: no foreground → hide
            Some(rect.clone()), // tick 2: recover → show + set_geometry
        ]));
        let window = Arc::new(MockHighlightWindow::new());
        let cancel = Arc::new(AtomicBool::new(false));

        // Cancel after ~25ms — enough for ~2 ticks at 10ms cadence.
        let cancel_ctl = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            cancel_ctl.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        run_tracker_loop(
            provider,
            window.clone(),
            cancel,
            std::time::Duration::from_millis(10),
        );

        let snap = window.snapshot();
        assert!(snap.hide_calls >= 1, "expected at least one hide on None");
        assert!(
            snap.show_calls >= 1,
            "expected at least one show when provider recovered"
        );
        assert!(
            snap.geometry_calls.contains(&rect),
            "expected set_geometry called with the recovered rect"
        );
    }

    #[test]
    fn tracker_loop_sets_geometry_and_shows_on_valid_rect() {
        let rect = Rect {
            x: 100,
            y: 200,
            width: 1024,
            height: 768,
        };
        let provider = Arc::new(MockRectProvider::new(vec![Some(rect.clone())]));
        let window = Arc::new(MockHighlightWindow::new());
        let cancel = Arc::new(AtomicBool::new(false));

        let cancel_ctl = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(15));
            cancel_ctl.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        run_tracker_loop(
            provider,
            window.clone(),
            cancel,
            std::time::Duration::from_millis(5),
        );

        let snap = window.snapshot();
        assert!(snap.geometry_calls.contains(&rect));
        assert!(snap.show_calls >= 1);
    }

    #[test]
    fn delayed_hide_is_skipped_after_new_generation_starts() {
        let state = TargetHighlightState::new();
        let captured_generation = state.current_generation();

        state.begin_generation();

        assert!(!state.should_apply_delayed_hide(captured_generation));
    }

    #[test]
    fn delayed_hide_is_allowed_for_same_generation() {
        let state = TargetHighlightState::new();
        let captured_generation = state.current_generation();

        assert!(state.should_apply_delayed_hide(captured_generation));
    }
}
