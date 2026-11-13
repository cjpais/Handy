//! macOS backend: the window server's window list finds the window under
//! the pointer and identifies it by its window number, the Accessibility
//! API points the application at that window, and keyboard events addressed
//! to one process (`CGEventPostToPid`) carry the text. Nothing here
//! activates an application, so the user's focus stays where it is.
//!
//! The window number is the identity: it holds for the window's lifetime and
//! the window server answers for it without the application's help, which
//! matters for applications that start their accessibility support late
//! (Firefox) or answer slowly. Accessibility is asked only at delivery, and
//! only to focus the window within its application and read its title.
//! Everything here needs the Accessibility permission Handy already asks
//! for; window titles come from it, since the window server only gives them
//! to screen recorders.

use super::{DeliverError, ScreenWindow, WindowInfo};
use crate::settings::{AutoSubmitKey, PasteMethod};
use log::{debug, warn};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use std::ffi::c_void;
use std::fmt;
use std::io::Write;
use std::process::{Command, Stdio};
use std::ptr::NonNull;
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use tauri::AppHandle;

pub const SUPPORTED: bool = true;

/// How long the target gets to answer each accessibility question. A
/// delivery asks a few, so a stalled application costs their sum before the
/// fallback paste.
const AX_TIMEOUT: Duration = Duration::from_millis(400);
/// Key down to key up for a posted chord. The ordinary paste holds its chord
/// 100ms because real systems dropped shorter ones (#165).
const CHORD_HOLD: Duration = Duration::from_millis(100);
/// Key down to key up for a posted text chunk. No modifiers to settle, and
/// a transcript is many chunks.
const TEXT_HOLD: Duration = Duration::from_millis(5);
/// `CGEventKeyboardSetUnicodeString` takes at most this many UTF-16 units
/// per event.
const UNICODE_CHUNK: usize = 20;
/// How far the accessibility frame of a window may be from the window
/// server's and still be the same window.
const FRAME_SLACK: f64 = 2.0;

type AXError = i32;
const AX_SUCCESS: AXError = 0;
const AX_ATTRIBUTE_UNSUPPORTED: AXError = -25205;
const AX_NOT_IMPLEMENTED: AXError = -25208;
const AX_NO_VALUE: AXError = -25212;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> *mut CFType;
    fn AXUIElementCopyAttributeValue(
        element: *const CFType,
        attribute: *const CFString,
        value: *mut *const CFType,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: *const CFType,
        attribute: *const CFString,
        value: *const CFType,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: *const CFType, timeout: f32) -> AXError;
    fn AXValueGetValue(value: *const CFType, value_type: u32, out: *mut c_void) -> bool;
}

type CGWindowID = u32;
const CG_WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
const CG_WINDOW_LIST_INCLUDING_WINDOW: u32 = 1 << 3;
const CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const CG_NULL_WINDOW_ID: CGWindowID = 0;

// Applications read modifiers from the flags of the key event itself, so no
// separate modifier key events are posted.
const FLAG_SHIFT: u64 = 1 << 17;
const FLAG_CONTROL: u64 = 1 << 18;
const FLAG_COMMAND: u64 = 1 << 20;
const KEY_RETURN: u16 = 36;
/// kVK_Help, which is Insert on PC keyboards.
const KEY_INSERT: u16 = 114;

/// iTerm2 routes key events to its key window only, which an application in
/// the background doesn't have, so posted keys vanish. Its scripting
/// interface writes to a session regardless.
const ITERM_BUNDLE_ID: &str = "com.googlecode.iterm2";
/// Writes argv[2] to the current session of the window with id argv[1];
/// argv[3] says whether Enter follows. The window id is the window server's
/// number. Enter is a carriage return: `write text`'s own newline is a line
/// feed, which a terminal program in raw mode (Claude Code, for one) takes
/// as a new line in the input, not as Enter.
const ITERM_WRITE_SCRIPT: &str = r#"on run argv
    set windowId to (item 1 of argv) as integer
    set pressEnter to (item 3 of argv) is "1"
    tell application "iTerm2"
        tell current session of window id windowId
            write text (item 2 of argv) newline false
            if pressEnter then write text (character id 13) newline false
        end tell
    end tell
end run"#;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: CGWindowID) -> *mut CFType;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut CFType;
    fn CGEventSetFlags(event: *const CFType, flags: u64);
    fn CGEventKeyboardSetUnicodeString(
        event: *const CFType,
        string_length: libc::c_ulong,
        unicode_string: *const u16,
    );
    fn CGEventPostToPid(pid: libc::pid_t, event: *const CFType);
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Size {
    width: f64,
    height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn about(&self, other: &Rect) -> bool {
        (self.x - other.x).abs() <= FRAME_SLACK
            && (self.y - other.y).abs() <= FRAME_SLACK
            && (self.width - other.width).abs() <= FRAME_SLACK
            && (self.height - other.height).abs() <= FRAME_SLACK
    }
}

/// An `AXUIElementRef`: an immutable token for an application or one of its
/// windows, safe to use from any thread.
#[derive(Clone)]
struct AxElement(CFRetained<CFType>);

// SAFETY: an AXUIElement is an immutable token; the AX API is called with it
// from any thread (it does its own IPC).
unsafe impl Send for AxElement {}
unsafe impl Sync for AxElement {}

impl PartialEq for AxElement {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl fmt::Debug for AxElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AxElement({:p})", CFRetained::as_ptr(&self.0))
    }
}

impl AxElement {
    fn application(pid: libc::pid_t) -> Self {
        // SAFETY: returns a +1 retained element for any pid, live or not.
        let ptr = unsafe { AXUIElementCreateApplication(pid) };
        let ptr = NonNull::new(ptr).expect("AXUIElementCreateApplication never returns NULL");
        let app = Self(unsafe { CFRetained::from_raw(ptr) });
        app.set_messaging_timeout(AX_TIMEOUT);
        app
    }

    fn as_ptr(&self) -> *const CFType {
        CFRetained::as_ptr(&self.0).as_ptr()
    }

    fn set_messaging_timeout(&self, timeout: Duration) {
        // SAFETY: valid element; the timeout applies to later calls on it.
        unsafe { AXUIElementSetMessagingTimeout(self.as_ptr(), timeout.as_secs_f32()) };
    }

    /// `Ok(None)` when the element has no such attribute or it has no value.
    fn attribute(&self, name: &str) -> Result<Option<CFRetained<CFType>>, AXError> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = std::ptr::null();
        // SAFETY: valid element and attribute name; `value` receives a +1
        // retained object on success.
        let err = unsafe { AXUIElementCopyAttributeValue(self.as_ptr(), &*name, &mut value) };
        match err {
            AX_SUCCESS => {
                Ok(NonNull::new(value as *mut CFType)
                    .map(|ptr| unsafe { CFRetained::from_raw(ptr) }))
            }
            AX_NO_VALUE | AX_ATTRIBUTE_UNSUPPORTED | AX_NOT_IMPLEMENTED => Ok(None),
            err => Err(err),
        }
    }

    fn string_attribute(&self, name: &str) -> Option<String> {
        let value = self.attribute(name).ok()??;
        value.downcast_ref::<CFString>().map(|s| s.to_string())
    }

    fn bool_attribute(&self, name: &str) -> Option<bool> {
        let value = self.attribute(name).ok()??;
        value.downcast_ref::<CFBoolean>().map(|b| b.as_bool())
    }

    fn element_attribute(&self, name: &str) -> Option<AxElement> {
        let element = self.attribute(name).ok()?.map(AxElement)?;
        element.set_messaging_timeout(AX_TIMEOUT);
        Some(element)
    }

    /// An `AXValue` attribute holding a struct of `value_type`.
    fn value_attribute<T: Default>(&self, name: &str, value_type: u32) -> Option<T> {
        let value = self.attribute(name).ok()??;
        let mut out = T::default();
        // SAFETY: `out` is the struct `value_type` names; AXValueGetValue
        // writes it only when the types match and says so.
        let ok = unsafe {
            AXValueGetValue(
                CFRetained::as_ptr(&value).as_ptr(),
                value_type,
                &mut out as *mut T as *mut c_void,
            )
        };
        ok.then_some(out)
    }

    /// Where the window is on screen, per the application.
    fn frame(&self) -> Option<Rect> {
        let Point { x, y } = self.value_attribute("AXPosition", AX_VALUE_CGPOINT)?;
        let Size { width, height } = self.value_attribute("AXSize", AX_VALUE_CGSIZE)?;
        Some(Rect {
            x,
            y,
            width,
            height,
        })
    }

    fn set_bool(&self, name: &str, value: bool) -> AXError {
        let name = CFString::from_str(name);
        let value: &CFType = CFBoolean::new(value).as_ref();
        // SAFETY: valid element, attribute name and value.
        unsafe { AXUIElementSetAttributeValue(self.as_ptr(), &*name, value) }
    }

    /// The application's windows, front to back.
    fn windows(&self) -> Vec<AxElement> {
        let Ok(Some(value)) = self.attribute("AXWindows") else {
            return Vec::new();
        };
        let Ok(windows) = value.downcast::<CFArray>() else {
            return Vec::new();
        };
        // SAFETY: AXWindows holds AXUIElements, which are CF objects.
        let windows: &CFArray<CFType> = unsafe { windows.cast_unchecked() };
        windows
            .iter()
            .map(|window| {
                let window = AxElement(window);
                window.set_messaging_timeout(AX_TIMEOUT);
                window
            })
            .collect()
    }
}

/// A window on macOS: its process and its window server number.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowHandle {
    pid: libc::pid_t,
    number: CGWindowID,
}

/// The application in front when the picker opened.
static FRONT_BEFORE_PICKER: Mutex<Option<libc::pid_t>> = Mutex::new(None);

pub fn remember_front() {
    let own_pid = std::process::id() as libc::pid_t;
    let front = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid != own_pid);
    *FRONT_BEFORE_PICKER
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = front;
}

/// Makes the application from `remember_front` active again. Handy is active
/// after a click on its panel, and this is where the user was.
pub fn restore_front() {
    let Some(pid) = FRONT_BEFORE_PICKER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    else {
        return;
    };
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        // Deprecated since macOS 14 for cooperative activation, which needs
        // the active application to yield; that's Handy itself here, so the
        // old call is the one that does the job.
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
}

/// A window as the window server lists it.
struct OnScreenWindow {
    pid: libc::pid_t,
    number: CGWindowID,
    app_name: String,
    frame: Rect,
}

fn number(dict: &CFDictionary<CFString, CFType>, key: &str) -> Option<f64> {
    dict.get(&CFString::from_str(key))?
        .downcast_ref::<CFNumber>()?
        .as_f64()
}

/// Windows as the window server lists them, front to back: ordinary ones
/// only, and never Handy's own, since the picker's panels cover every screen
/// and must not find themselves.
fn window_list(option: u32, relative_to: CGWindowID) -> Vec<OnScreenWindow> {
    let own_pid = std::process::id() as libc::pid_t;
    // SAFETY: returns a +1 retained array, or NULL.
    let list = unsafe { CGWindowListCopyWindowInfo(option, relative_to) };
    let Some(list) = NonNull::new(list).map(|ptr| unsafe { CFRetained::from_raw(ptr) }) else {
        return Vec::new();
    };
    let Ok(list) = list.downcast::<CFArray>() else {
        return Vec::new();
    };
    // SAFETY: the window list is an array of dictionaries keyed by CFString.
    let list: &CFArray<CFDictionary<CFString, CFType>> = unsafe { list.cast_unchecked() };
    list.iter()
        .filter_map(|info| {
            // Layer 0 is where ordinary windows live; menus, the Dock and
            // the menu bar are above it, the desktop below.
            if number(&info, "kCGWindowLayer")? != 0.0 || number(&info, "kCGWindowAlpha")? == 0.0 {
                return None;
            }
            let pid = number(&info, "kCGWindowOwnerPID")? as libc::pid_t;
            if pid == own_pid {
                return None;
            }
            let bounds = info.get(&CFString::from_str("kCGWindowBounds"))?;
            let bounds = bounds.downcast_ref::<CFDictionary>()?;
            // SAFETY: a bounds dictionary is keyed by CFString with number values.
            let bounds: &CFDictionary<CFString, CFType> = unsafe { bounds.cast_unchecked() };
            let frame = Rect {
                x: number(bounds, "X")?,
                y: number(bounds, "Y")?,
                width: number(bounds, "Width")?,
                height: number(bounds, "Height")?,
            };
            let app_name = info
                .get(&CFString::from_str("kCGWindowOwnerName"))
                .and_then(|name| name.downcast_ref::<CFString>().map(|s| s.to_string()))
                .unwrap_or_default();
            Some(OnScreenWindow {
                pid,
                number: number(&info, "kCGWindowNumber")? as CGWindowID,
                app_name,
                frame,
            })
        })
        .collect()
}

/// The ordinary windows under a screen point, front to back.
fn on_screen_windows_at(x: f64, y: f64) -> impl Iterator<Item = OnScreenWindow> {
    window_list(
        CG_WINDOW_LIST_ON_SCREEN_ONLY | CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
        CG_NULL_WINDOW_ID,
    )
    .into_iter()
    .filter(move |window| window.frame.contains(x, y))
}

pub fn windows_at(x: f64, y: f64) -> Vec<ScreenWindow> {
    on_screen_windows_at(x, y)
        .map(|window| ScreenWindow {
            app_name: window.app_name,
            x: window.frame.x,
            y: window.frame.y,
            width: window.frame.width,
            height: window.frame.height,
        })
        .collect()
}

impl WindowHandle {
    /// The window as the window server knows it now; `None` once it's gone.
    fn on_screen(&self) -> Option<OnScreenWindow> {
        window_list(CG_WINDOW_LIST_INCLUDING_WINDOW, self.number)
            .into_iter()
            .find(|window| window.number == self.number)
    }

    /// The window's accessibility element: the one of its application's
    /// windows that sits where the window server says it is. `None` when the
    /// application doesn't answer.
    fn ax_window(&self, frame: &Rect) -> Option<AxElement> {
        AxElement::application(self.pid)
            .windows()
            .into_iter()
            .find(|window| window.frame().is_some_and(|f| f.about(frame)))
    }

    /// The window this application types into right now.
    fn focused_window(&self) -> Option<AxElement> {
        AxElement::application(self.pid).element_attribute("AXFocusedWindow")
    }
}

/// The window at `depth` under a screen point (0 is the front one) as a
/// redirect target.
pub fn pick(x: f64, y: f64, depth: usize) -> Option<WindowInfo> {
    let hit = on_screen_windows_at(x, y).nth(depth)?;
    let handle = WindowHandle {
        pid: hit.pid,
        number: hit.number,
    };
    // Best effort: an application whose accessibility isn't up yet leaves
    // the title empty, and the window is picked all the same.
    let title = handle
        .ax_window(&hit.frame)
        .and_then(|window| window.string_attribute("AXTitle"))
        .unwrap_or_default();
    Some(WindowInfo {
        handle,
        app_name: hit.app_name,
        title,
    })
}

/// The target is the front application's focused window.
pub fn is_focused(handle: &WindowHandle) -> bool {
    let front = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier());
    if front != Some(handle.pid) {
        return false;
    }
    let Some(on_screen) = handle.on_screen() else {
        return false;
    };
    handle
        .focused_window()
        .and_then(|window| window.frame())
        .is_some_and(|frame| frame.about(&on_screen.frame))
}

/// The window an application was typing into before `prepare` pointed it at
/// the target, if that was another one.
pub struct Previous(Option<AxElement>);

/// Checks the window is still there and makes it its application's focused
/// window, so that's where the application puts what it's sent; a minimized
/// target is restored first and stays so. Setting `AXMain`/`AXFocused`
/// reorders windows within the application only; the application stays in
/// the background, and `restore` puts its previous window back in front.
/// An application that doesn't answer accessibility keeps its focused
/// window, and the keys go wherever it puts them.
pub fn prepare(handle: &WindowHandle) -> Result<Previous, DeliverError> {
    let on_screen = handle.on_screen().ok_or(DeliverError::WindowGone)?;
    let Some(window) = handle.ax_window(&on_screen.frame) else {
        debug!("The application doesn't answer for the target window; sending keys anyway");
        return Ok(Previous(None));
    };
    let previous = handle.focused_window();
    if previous.as_ref() == Some(&window) {
        return Ok(Previous(None));
    }
    if window.bool_attribute("AXMinimized") == Some(true) {
        window.set_bool("AXMinimized", false);
    }
    focus_within_app(&window);
    Ok(Previous(previous))
}

/// Points the window's application at it without activating the application.
fn focus_within_app(window: &AxElement) {
    window.set_bool("AXMain", true);
    let err = window.set_bool("AXFocused", true);
    if err != AX_SUCCESS {
        debug!("AX error {err} focusing a window within its application");
    }
}

/// Gives the application back the window it was typing into. Best effort:
/// that window may be gone.
pub fn restore(previous: Previous) {
    if let Previous(Some(window)) = previous {
        focus_within_app(&window);
    }
}

fn bundle_id(pid: libc::pid_t) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|id| id.to_string())
}

/// Applications that don't take posted keys in the background get the text
/// their own way. Only iTerm2 so far: `write text` into the window's current
/// session, then a carriage return for auto-submit.
pub fn hand_text(
    handle: &WindowHandle,
    text: &str,
    submit: bool,
) -> Result<Option<&'static str>, DeliverError> {
    if bundle_id(handle.pid).as_deref() != Some(ITERM_BUNDLE_ID) {
        return Ok(None);
    }
    let mut child = Command::new("osascript")
        .arg("-")
        .arg(handle.number.to_string())
        .arg(text)
        .arg(if submit { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DeliverError::Failed(format!("could not run osascript: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(ITERM_WRITE_SCRIPT.as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|e| DeliverError::Failed(format!("osascript didn't finish: {e}")))?;
    if output.status.success() {
        Ok(Some("iTerm2 write text"))
    } else {
        Err(DeliverError::Failed(format!(
            "iTerm2 refused the text: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// A key event for the target's process only. `Ok` means it was posted; what
/// the application does with it can't be observed.
fn post_key(
    pid: libc::pid_t,
    keycode: u16,
    flags: u64,
    text: Option<&[u16]>,
    hold: Duration,
) -> Result<(), DeliverError> {
    for down in [true, false] {
        // SAFETY: a NULL source is allowed; the event is released below.
        let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), keycode, down) };
        let event = NonNull::new(event)
            .map(|ptr| unsafe { CFRetained::from_raw(ptr) })
            .ok_or_else(|| DeliverError::Failed("could not create a key event".to_string()))?;
        let ptr = CFRetained::as_ptr(&event).as_ptr();
        // SAFETY: valid event; the string outlives the call, which copies it.
        unsafe {
            CGEventSetFlags(ptr, flags);
            if let Some(text) = text {
                CGEventKeyboardSetUnicodeString(ptr, text.len() as libc::c_ulong, text.as_ptr());
            }
            CGEventPostToPid(pid, ptr);
        }
        if down {
            std::thread::sleep(hold);
        }
    }
    Ok(())
}

/// Types `text` into the window's process as keyboard input, in the chunks
/// one event can carry.
pub fn post_text(handle: &WindowHandle, text: &str) -> Result<(), DeliverError> {
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(UNICODE_CHUNK) {
        post_key(handle.pid, 0, 0, Some(chunk), TEXT_HOLD)?;
    }
    Ok(())
}

/// The keycode Cmd+V needs under the active layout. Resolved on the main
/// thread, where the Text Input Services calls belong; ANSI V when the main
/// thread doesn't answer in time.
fn v_keycode(app: &AppHandle) -> u16 {
    const ANSI_V: u16 = 9;
    let (tx, rx) = mpsc::channel();
    let posted = app.run_on_main_thread(move || {
        let _ = tx.send(crate::input::command_v_keycode());
    });
    match posted {
        Ok(()) => rx.recv_timeout(Duration::from_secs(1)).unwrap_or_else(|_| {
            warn!("Main thread didn't resolve the V keycode in time; using ANSI V");
            ANSI_V
        }),
        Err(e) => {
            warn!("Could not resolve the V keycode on the main thread ({e}); using ANSI V");
            ANSI_V
        }
    }
}

/// Sends the paste chord of `method` to the window's process. The text has
/// to be on the clipboard already.
pub fn post_chord(
    app: &AppHandle,
    handle: &WindowHandle,
    method: PasteMethod,
) -> Result<(), DeliverError> {
    let (keycode, flags) = match method {
        PasteMethod::CtrlV => (v_keycode(app), FLAG_COMMAND),
        PasteMethod::CtrlShiftV => (v_keycode(app), FLAG_COMMAND | FLAG_SHIFT),
        PasteMethod::ShiftInsert => (KEY_INSERT, FLAG_SHIFT),
        other => {
            return Err(DeliverError::Failed(format!(
                "{other:?} is not a paste chord"
            )))
        }
    };
    post_key(handle.pid, keycode, flags, None, CHORD_HOLD)
}

pub fn post_submit(handle: &WindowHandle, key: AutoSubmitKey) -> Result<(), DeliverError> {
    let flags = match key {
        AutoSubmitKey::Enter => 0,
        AutoSubmitKey::CtrlEnter => FLAG_CONTROL,
        AutoSubmitKey::CmdEnter => FLAG_COMMAND,
    };
    post_key(handle.pid, KEY_RETURN, flags, None, CHORD_HOLD)
}

#[cfg(test)]
pub(super) fn test_handle() -> WindowHandle {
    WindowHandle {
        pid: std::process::id() as libc::pid_t,
        number: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_its_inside_and_not_its_far_edge() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(rect.contains(10.0, 20.0));
        assert!(rect.contains(109.0, 69.0));
        assert!(!rect.contains(110.0, 69.0));
        assert!(!rect.contains(9.0, 20.0));
    }

    #[test]
    fn frames_match_within_the_slack() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let b = Rect {
            x: 1.0,
            y: -1.0,
            width: 801.0,
            height: 599.0,
        };
        let c = Rect { x: 3.0, ..a };
        assert!(a.about(&b));
        assert!(!a.about(&c));
    }
}
