use crate::input::{self, EnigoState};
#[cfg(target_os = "linux")]
use crate::settings::TypingTool;
use crate::settings::{get_settings, AutoSubmitKey, ClipboardHandling, PasteMethod};
use enigo::{Direction, Enigo, Key, Keyboard};
use log::info;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "linux")]
use crate::utils::{is_gnome_wayland, is_kde_wayland, is_wayland};

/// Serializes pastes. Each one snapshots and restores the clipboard, so two
/// overlapping pastes would save each other's transcript as "the user's
/// clipboard". Only taken by `paste`, which never runs on the main thread.
static PASTE_LOCK: Mutex<()> = Mutex::new(());

/// Runs `f` with the shared Enigo on the main thread and waits for its result.
///
/// Paste runs on a worker thread so the main thread stays free while it
/// sleeps, which lets Handy's own windows handle a paste aimed at them. Input
/// injection hops back here because macOS's TIS keyboard layout APIs must run
/// on the main thread. Tauri runs the closure inline when already on the main
/// thread, so nested calls can't deadlock.
fn with_enigo<T: Send + 'static>(
    app_handle: &AppHandle,
    f: impl FnOnce(&mut Enigo) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let app = app_handle.clone();
    app_handle
        .run_on_main_thread(move || {
            let result = (|| {
                let enigo_state = app
                    .try_state::<EnigoState>()
                    .ok_or("Enigo state not initialized")?;
                let mut enigo = enigo_state
                    .0
                    .lock()
                    .map_err(|e| format!("Failed to lock Enigo: {}", e))?;
                f(&mut enigo)
            })();
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Failed to run input on the main thread: {}", e))?;
    // A dropped closure (event loop shutting down) closes the channel.
    rx.recv()
        .map_err(|_| "Main thread dropped the input request".to_string())?
}

fn write_text_to_clipboard(app_handle: &AppHandle, text: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if is_wayland() && is_wl_copy_available() {
        info!("Using wl-copy for clipboard write on Wayland");
        return write_clipboard_via_wl_copy(text);
    }

    app_handle
        .clipboard()
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))
}

fn finish_clipboard_paste(
    paste_result: Result<(), String>,
    paste_delay_after_ms: u64,
    restore_clipboard: impl FnOnce(),
) -> Result<(), String> {
    std::thread::sleep(Duration::from_millis(paste_delay_after_ms));
    restore_clipboard();
    paste_result
}

/// Bounds on reading the clipboard, see `read_with_limits`.
#[cfg(target_os = "linux")]
struct ReadLimits {
    /// Longest wait for the next chunk of data.
    stall_timeout: Duration,
    /// Longest the whole read may take.
    total_timeout: Duration,
    /// Most data read before giving up.
    max_bytes: usize,
}

/// Limits for saving clipboard text before a paste; past them the paste goes
/// ahead without saving it. Apps hand text over almost at once, so a frozen
/// owner costs at most a second.
#[cfg(target_os = "linux")]
const TEXT_SAVE_LIMITS: ReadLimits = ReadLimits {
    stall_timeout: Duration::from_secs(1),
    total_timeout: Duration::from_secs(10),
    max_bytes: 256 * 1024 * 1024,
};

/// Limits for saving a clipboard image before a paste. The stall timeout
/// leaves room for a large screenshot, since Qt apps encode an image in full
/// before sending its first byte (about 1 s for 7680x2160).
#[cfg(target_os = "linux")]
const IMAGE_SAVE_LIMITS: ReadLimits = ReadLimits {
    stall_timeout: Duration::from_secs(3),
    ..TEXT_SAVE_LIMITS
};

/// The clipboard contents saved before a paste, put back once it is done.
enum SavedClipboard {
    Text(String),
    Image(tauri::image::Image<'static>),
    #[cfg(target_os = "linux")]
    Png(Vec<u8>),
    Empty,
    /// The contents could not be saved, so the transcript stays on the
    /// clipboard instead of clearing what the user had.
    #[cfg(target_os = "linux")]
    Unreadable,
}

fn save_clipboard(app_handle: &AppHandle) -> SavedClipboard {
    #[cfg(target_os = "linux")]
    if is_wayland() {
        if let Some(saved_clipboard) = save_clipboard_via_data_control() {
            return saved_clipboard;
        }
    }

    let clipboard = app_handle.clipboard();
    if let Some(text) = clipboard.read_text().ok().filter(|t| !t.is_empty()) {
        return SavedClipboard::Text(text);
    }
    // Only probe for an image when there is no text to restore. Text is by far the
    // common case, and reading an image decodes the full bitmap, so this keeps the
    // text path exactly as cheap as it was before.
    match clipboard.read_image() {
        Ok(image) => SavedClipboard::Image(image.to_owned()),
        Err(_) => SavedClipboard::Empty,
    }
}

/// Outcome of reading the clipboard as one MIME type.
#[cfg(target_os = "linux")]
enum ClipboardRead {
    Contents(Vec<u8>),
    /// The clipboard is empty or holds nothing of that type.
    NotOffered,
    /// The compositor lacks the data-control protocol.
    Unavailable,
    /// The owning app did not hand its contents over within the limits.
    Failed(String),
}

/// Saves the clipboard through wl-clipboard-rs, the library the clipboard
/// plugin reads with on Wayland, but within `TEXT_SAVE_LIMITS` and
/// `IMAGE_SAVE_LIMITS`.
///
/// A Wayland clipboard read waits for the owning app to send its contents, and
/// a frozen owner never does. Through the plugin that read blocks forever and
/// keeps holding the plugin's lock, which stalls every later clipboard call.
/// Returns `None` when the compositor lacks data-control, leaving the save to
/// the plugin, which then falls back to X11.
#[cfg(target_os = "linux")]
fn save_clipboard_via_data_control() -> Option<SavedClipboard> {
    use wl_clipboard_rs::paste::MimeType;

    let mut text_is_not_utf8 = false;
    match read_clipboard_via_data_control(MimeType::Text, &TEXT_SAVE_LIMITS) {
        ClipboardRead::Contents(bytes) if !bytes.is_empty() => match String::from_utf8(bytes) {
            Ok(text) => return Some(SavedClipboard::Text(text)),
            Err(_) => text_is_not_utf8 = true,
        },
        ClipboardRead::Contents(_) | ClipboardRead::NotOffered => {}
        ClipboardRead::Unavailable => return None,
        ClipboardRead::Failed(error) => return Some(clipboard_not_saved(&error)),
    }

    // Same order as the plugin path: an image only when there is no usable text.
    match read_clipboard_via_data_control(MimeType::Specific("image/png"), &IMAGE_SAVE_LIMITS) {
        ClipboardRead::Contents(png) if !png.is_empty() => Some(SavedClipboard::Png(png)),
        ClipboardRead::Contents(_) | ClipboardRead::NotOffered if text_is_not_utf8 => {
            Some(clipboard_not_saved("its text is not UTF-8"))
        }
        ClipboardRead::Contents(_) | ClipboardRead::NotOffered => Some(SavedClipboard::Empty),
        ClipboardRead::Unavailable => None,
        ClipboardRead::Failed(error) => Some(clipboard_not_saved(&error)),
    }
}

#[cfg(target_os = "linux")]
fn clipboard_not_saved(reason: &str) -> SavedClipboard {
    log::warn!("Pasting without saving the clipboard: {}", reason);
    SavedClipboard::Unreadable
}

#[cfg(target_os = "linux")]
fn read_clipboard_via_data_control(
    mime_type: wl_clipboard_rs::paste::MimeType,
    limits: &ReadLimits,
) -> ClipboardRead {
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, Error, Seat};

    // Only talks to the compositor; the wait for the owning app happens when
    // reading the returned pipe.
    match get_contents(ClipboardType::Regular, Seat::Unspecified, mime_type) {
        Ok((pipe, _)) => match read_with_limits(pipe, limits) {
            Ok(contents) => ClipboardRead::Contents(contents),
            Err(error) => ClipboardRead::Failed(error),
        },
        Err(Error::NoSeats | Error::ClipboardEmpty | Error::NoMimeType) => {
            ClipboardRead::NotOffered
        }
        // Only these may fall back: without data-control the plugin reads
        // through X11, which has its own stall timeout, but its Wayland read
        // has none.
        Err(error @ (Error::MissingProtocol { .. } | Error::WaylandConnection(_))) => {
            info!("Saving the clipboard through the plugin instead: {}", error);
            ClipboardRead::Unavailable
        }
        Err(error) => ClipboardRead::Failed(error.to_string()),
    }
}

/// Reads `source` until its writer closes it, giving up when it sends nothing
/// for `limits.stall_timeout` or exceeds the other limits. Dropping the source
/// afterwards closes it, which releases the writer.
#[cfg(target_os = "linux")]
fn read_with_limits(
    mut source: impl std::io::Read + std::os::fd::AsFd,
    limits: &ReadLimits,
) -> Result<Vec<u8>, String> {
    use std::io::ErrorKind;
    use std::os::fd::AsRawFd;
    use std::time::Instant;

    let deadline = Instant::now() + limits.total_timeout;
    let mut contents = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let time_left = deadline.saturating_duration_since(Instant::now());
        if time_left.is_zero() {
            return Err(format!(
                "reading it took longer than {:?}",
                limits.total_timeout
            ));
        }

        let mut poll_fd = libc::pollfd {
            fd: source.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let wait = limits.stall_timeout.min(time_left);
        // Rounded up to 1 ms so a sub-millisecond wait doesn't spin.
        let wait_ms = libc::c_int::try_from(wait.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
        let ready = unsafe { libc::poll(&mut poll_fd, 1, wait_ms) };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("waiting for its data failed: {}", error));
        }
        if ready == 0 {
            if wait < limits.stall_timeout {
                // The total deadline passed, which the loop's start reports.
                continue;
            }
            return Err(format!(
                "its owner sent no data for {:?}",
                limits.stall_timeout
            ));
        }

        // The source is readable or closed, so this read returns without blocking.
        match source.read(&mut chunk) {
            Ok(0) => return Ok(contents),
            Ok(read) => {
                contents.extend_from_slice(&chunk[..read]);
                if contents.len() > limits.max_bytes {
                    return Err(format!("it holds more than {} bytes", limits.max_bytes));
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("reading it failed: {}", error)),
        }
    }
}

fn restore_clipboard(app_handle: &AppHandle, saved_clipboard: SavedClipboard) {
    let clipboard = app_handle.clipboard();
    match saved_clipboard {
        SavedClipboard::Text(text) => {
            let _ = write_text_to_clipboard(app_handle, &text);
        }
        SavedClipboard::Image(image) => {
            info!("Restoring image to clipboard");
            let _ = clipboard.write_image(&image);
        }
        #[cfg(target_os = "linux")]
        SavedClipboard::Png(png) => {
            use wl_clipboard_rs::copy::{MimeType, Options, Source};
            info!("Restoring image to clipboard");
            let _ = Options::new().copy(
                Source::Bytes(png.into_boxed_slice()),
                MimeType::Specific("image/png".to_string()),
            );
        }
        SavedClipboard::Empty => {
            // Nothing was there to begin with — don't leave the transcription behind.
            let _ = clipboard.clear();
        }
        #[cfg(target_os = "linux")]
        SavedClipboard::Unreadable => {}
    }
}

/// Pastes text using the clipboard: saves current content, writes text, sends paste keystroke, restores clipboard.
fn paste_via_clipboard(
    text: &str,
    app_handle: &AppHandle,
    paste_method: &PasteMethod,
    paste_delay_ms: u64,
    paste_delay_after_ms: u64,
) -> Result<(), String> {
    let saved_clipboard = save_clipboard(app_handle);

    // Write text to clipboard first
    write_text_to_clipboard(app_handle, text)?;

    std::thread::sleep(Duration::from_millis(paste_delay_ms));

    // Capture key injection errors so the original clipboard is restored before
    // propagating them to the caller.
    let paste_result = (|| -> Result<(), String> {
        // Send paste key combo
        #[cfg(target_os = "linux")]
        let key_combo_sent = try_send_key_combo_linux(paste_method)?;

        #[cfg(not(target_os = "linux"))]
        let key_combo_sent = false;

        // Fall back to enigo if no native tool handled it
        if !key_combo_sent {
            let paste_method = *paste_method;
            with_enigo(app_handle, move |enigo| match paste_method {
                // The legacy path cannot detect a mistimed chord, so it keeps the
                // conservative 100ms modifier hold.
                PasteMethod::CtrlV => input::send_paste_ctrl_v(enigo, 100),
                PasteMethod::CtrlShiftV => input::send_paste_ctrl_shift_v(enigo, 100),
                PasteMethod::ShiftInsert => input::send_paste_shift_insert(enigo, 100),
                _ => Err("Invalid paste method for clipboard paste".into()),
            })?;
        }

        Ok(())
    })();

    finish_clipboard_paste(paste_result, paste_delay_after_ms, || {
        // Restore original clipboard content even when key injection failed.
        restore_clipboard(app_handle, saved_clipboard);
    })
}

/// Attempts to send a key combination using Linux-native tools.
/// Returns `Ok(true)` if a native tool handled it, `Ok(false)` to fall back to enigo.
#[cfg(target_os = "linux")]
fn try_send_key_combo_linux(paste_method: &PasteMethod) -> Result<bool, String> {
    if is_wayland() {
        // Wayland: prefer wtype (but not on KDE or GNOME), then dotool, then ydotool
        // Note: wtype doesn't work on KDE (no zwp_virtual_keyboard_manager_v1 support)
        // or on GNOME/Mutter (same reason — Mutter deliberately does not implement
        // the virtual-keyboard-v1 protocol).
        if !is_kde_wayland() && !is_gnome_wayland() && is_wtype_available() {
            info!("Using wtype for key combo");
            send_key_combo_via_wtype(paste_method)?;
            return Ok(true);
        }
        if is_dotool_available() {
            info!("Using dotool for key combo");
            send_key_combo_via_dotool(paste_method)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for key combo");
            send_key_combo_via_ydotool(paste_method)?;
            return Ok(true);
        }
    } else {
        // X11: prefer xdotool, then ydotool
        if is_xdotool_available() {
            info!("Using xdotool for key combo");
            send_key_combo_via_xdotool(paste_method)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for key combo");
            send_key_combo_via_ydotool(paste_method)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Attempts to type text directly using Linux-native tools.
/// Returns `Ok(true)` if a native tool handled it, `Ok(false)` to fall back to enigo.
#[cfg(target_os = "linux")]
fn try_direct_typing_linux(text: &str, preferred_tool: TypingTool) -> Result<bool, String> {
    // If user specified a tool, try only that one
    if preferred_tool != TypingTool::Auto {
        return match preferred_tool {
            TypingTool::Wtype if is_wtype_available() => {
                info!("Using user-specified wtype");
                type_text_via_wtype(text)?;
                Ok(true)
            }
            TypingTool::Kwtype if is_kwtype_available() => {
                info!("Using user-specified kwtype");
                type_text_via_kwtype(text)?;
                Ok(true)
            }
            TypingTool::Dotool if is_dotool_available() => {
                info!("Using user-specified dotool");
                type_text_via_dotool(text)?;
                Ok(true)
            }
            TypingTool::Ydotool if is_ydotool_available() => {
                info!("Using user-specified ydotool");
                type_text_via_ydotool(text)?;
                Ok(true)
            }
            TypingTool::Xdotool if is_xdotool_available() => {
                info!("Using user-specified xdotool");
                type_text_via_xdotool(text)?;
                Ok(true)
            }
            _ => Err(format!(
                "Typing tool {:?} is not available on this system",
                preferred_tool
            )),
        };
    }

    // Auto mode - existing fallback chain
    if is_wayland() {
        // KDE Wayland: prefer kwtype (uses KDE Fake Input protocol, supports umlauts)
        if is_kde_wayland() && is_kwtype_available() {
            info!("Using kwtype for direct text input on KDE Wayland");
            type_text_via_kwtype(text)?;
            return Ok(true);
        }
        // Wayland: prefer wtype, then dotool, then ydotool
        // Note: wtype doesn't work on KDE (no zwp_virtual_keyboard_manager_v1 support)
        // or on GNOME/Mutter (same reason — Mutter deliberately does not implement
        // the virtual-keyboard-v1 protocol).
        if !is_kde_wayland() && !is_gnome_wayland() && is_wtype_available() {
            info!("Using wtype for direct text input");
            type_text_via_wtype(text)?;
            return Ok(true);
        }
        if is_dotool_available() {
            info!("Using dotool for direct text input");
            type_text_via_dotool(text)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for direct text input");
            type_text_via_ydotool(text)?;
            return Ok(true);
        }
    } else {
        // X11: prefer xdotool, then ydotool
        if is_xdotool_available() {
            info!("Using xdotool for direct text input");
            type_text_via_xdotool(text)?;
            return Ok(true);
        }
        if is_ydotool_available() {
            info!("Using ydotool for direct text input");
            type_text_via_ydotool(text)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Returns the list of available typing tools on this system.
/// Always includes "auto" as the first entry.
#[cfg(target_os = "linux")]
pub fn get_available_typing_tools() -> Vec<String> {
    let mut tools = vec!["auto".to_string()];
    if is_wtype_available() {
        tools.push("wtype".to_string());
    }
    if is_kwtype_available() {
        tools.push("kwtype".to_string());
    }
    if is_dotool_available() {
        tools.push("dotool".to_string());
    }
    if is_ydotool_available() {
        tools.push("ydotool".to_string());
    }
    if is_xdotool_available() {
        tools.push("xdotool".to_string());
    }
    tools
}

/// Check if wtype is available (Wayland text input tool)
#[cfg(target_os = "linux")]
fn is_wtype_available() -> bool {
    Command::new("which")
        .arg("wtype")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if dotool is available (another Wayland text input tool)
#[cfg(target_os = "linux")]
fn is_dotool_available() -> bool {
    Command::new("which")
        .arg("dotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum YdotoolKeySyntax {
    Symbolic,
    RawKeycodes,
}

#[cfg(target_os = "linux")]
const YDOTOOL_UNKNOWN_HELP_FALLBACK: YdotoolKeySyntax = YdotoolKeySyntax::RawKeycodes;

#[cfg(target_os = "linux")]
static YDOTOOL_KEY_SYNTAX: OnceLock<YdotoolKeySyntax> = OnceLock::new();

/// Classifies `ydotool key --help` output without relying on version or distro metadata.
#[cfg(target_os = "linux")]
fn classify_ydotool_key_syntax(help: &str) -> Option<YdotoolKeySyntax> {
    let help = help.to_ascii_lowercase();

    // Check modern markers first in case a future help message mentions the legacy syntax.
    if help.contains("syntax: <keycode>:<pressed>")
        || help.contains("[keycodes]")
        || help.contains("using raw keycodes")
    {
        Some(YdotoolKeySyntax::RawKeycodes)
    } else if help.contains("separated by plus (+)")
        || (help.contains("<key sequence>") && help.contains("alt+r"))
    {
        Some(YdotoolKeySyntax::Symbolic)
    } else {
        None
    }
}

/// Detects and caches a recognized ydotool key syntax. Unknown or failed probes are not cached,
/// allowing a transient daemon or PATH problem to recover on a later paste attempt.
#[cfg(target_os = "linux")]
fn detect_ydotool_key_syntax() -> YdotoolKeySyntax {
    if let Some(syntax) = YDOTOOL_KEY_SYNTAX.get() {
        return *syntax;
    }

    match Command::new("ydotool").args(["key", "--help"]).output() {
        Ok(output) => {
            // ydotool 0.x writes help to stderr and its exit status varies by build.
            let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
            if !help.is_empty() && !output.stderr.is_empty() {
                help.push('\n');
            }
            help.push_str(&String::from_utf8_lossy(&output.stderr));

            if let Some(syntax) = classify_ydotool_key_syntax(&help) {
                *YDOTOOL_KEY_SYNTAX.get_or_init(|| {
                    info!("Detected ydotool key syntax: {:?}", syntax);
                    syntax
                })
            } else {
                // Preserve Handy's existing behavior and compatibility with current ydotool.
                log::warn!(
                    "Could not recognize ydotool key --help output (exit status {:?}); using raw-keycode syntax",
                    output.status.code()
                );
                YDOTOOL_UNKNOWN_HELP_FALLBACK
            }
        }
        Err(error) => {
            log::warn!(
                "Could not query ydotool key syntax: {}; using raw-keycode syntax",
                error
            );
            YDOTOOL_UNKNOWN_HELP_FALLBACK
        }
    }
}

/// Check if ydotool is available (uinput-based, works on both Wayland and X11)
#[cfg(target_os = "linux")]
fn is_ydotool_available() -> bool {
    Command::new("which")
        .arg("ydotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_xdotool_available() -> bool {
    Command::new("which")
        .arg("xdotool")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if kwtype is available (KDE Wayland virtual keyboard input tool)
#[cfg(target_os = "linux")]
fn is_kwtype_available() -> bool {
    Command::new("which")
        .arg("kwtype")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check if wl-copy is available (Wayland clipboard tool)
#[cfg(target_os = "linux")]
fn is_wl_copy_available() -> bool {
    Command::new("which")
        .arg("wl-copy")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Type text directly via wtype on Wayland.
#[cfg(target_os = "linux")]
fn type_text_via_wtype(text: &str) -> Result<(), String> {
    let output = Command::new("wtype")
        .arg("--") // Protect against text starting with -
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute wtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wtype failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via xdotool on X11.
#[cfg(target_os = "linux")]
fn type_text_via_xdotool(text: &str) -> Result<(), String> {
    let output = Command::new("xdotool")
        .arg("type")
        .arg("--clearmodifiers")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute xdotool: {}", e))?;

    // `--clearmodifiers` restores the modifiers that were held when xdotool
    // started. If the user releases one while xdotool is typing, that synthetic
    // restore can leave the modifier latched on the XTEST keyboard (#1817).
    // Release both sides of Handy's supported push-style modifiers to clear any
    // stale restore. Lock keys are intentionally excluded because key events
    // toggle them.
    //
    // The release is unconditional, so it can make a modifier that is still
    // physically held appear released until the next physical event. This is
    // preferable to leaving a synthetic modifier latched system-wide.
    //
    // Clean up before checking the typing status because xdotool may have
    // changed modifier state before returning an error. Cleanup remains
    // best-effort because the text may already have been partially or fully
    // typed, but failures are logged so they can be diagnosed.
    match Command::new("xdotool")
        .arg("keyup")
        .args([
            "Control_L",
            "Control_R",
            "Shift_L",
            "Shift_R",
            "Alt_L",
            "Alt_R",
            "Super_L",
            "Super_R",
        ])
        .output()
    {
        Ok(cleanup_output) if !cleanup_output.status.success() => {
            let stderr = String::from_utf8_lossy(&cleanup_output.stderr);
            log::warn!(
                "xdotool modifier cleanup failed with status {:?}: {}",
                cleanup_output.status.code(),
                stderr.trim()
            );
        }
        Err(error) => log::warn!("Failed to execute xdotool modifier cleanup: {}", error),
        _ => {}
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("xdotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via dotool (works on both Wayland and X11 via uinput).
#[cfg(target_os = "linux")]
fn type_text_via_dotool(text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn dotool: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        // dotool uses "type <text>" command
        writeln!(stdin, "type {}", text)
            .map_err(|e| format!("Failed to write to dotool stdin: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for dotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("dotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via ydotool (uinput-based, requires ydotoold daemon).
#[cfg(target_os = "linux")]
fn type_text_via_ydotool(text: &str) -> Result<(), String> {
    let output = Command::new("ydotool")
        .arg("type")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute ydotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ydotool failed: {}", stderr));
    }

    Ok(())
}

/// Type text directly via kwtype (KDE Wayland virtual keyboard, uses KDE Fake Input protocol).
#[cfg(target_os = "linux")]
fn type_text_via_kwtype(text: &str) -> Result<(), String> {
    let output = Command::new("kwtype")
        .arg("--")
        .arg(text)
        .output()
        .map_err(|e| format!("Failed to execute kwtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kwtype failed: {}", stderr));
    }

    Ok(())
}

/// Write text to clipboard via wl-copy (Wayland clipboard tool).
/// Uses Stdio::null() to avoid blocking on repeated calls — wl-copy forks a
/// daemon that inherits piped fds, causing read_to_end to hang indefinitely.
#[cfg(target_os = "linux")]
fn write_clipboard_via_wl_copy(text: &str) -> Result<(), String> {
    use std::process::Stdio;
    let status = Command::new("wl-copy")
        .arg("--")
        .arg(text)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to execute wl-copy: {}", e))?;

    if !status.success() {
        return Err("wl-copy failed".into());
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via wtype on Wayland.
#[cfg(target_os = "linux")]
fn send_key_combo_via_wtype(paste_method: &PasteMethod) -> Result<(), String> {
    let args: Vec<&str> = match paste_method {
        PasteMethod::CtrlV => vec!["-M", "ctrl", "-k", "v", "-m", "ctrl"],
        PasteMethod::ShiftInsert => vec!["-M", "shift", "-k", "Insert", "-m", "shift"],
        PasteMethod::CtrlShiftV => vec![
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ],
        _ => return Err("Unsupported paste method".into()),
    };

    let output = Command::new("wtype")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to execute wtype: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wtype failed: {}", stderr));
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via dotool.
#[cfg(target_os = "linux")]
fn send_key_combo_via_dotool(paste_method: &PasteMethod) -> Result<(), String> {
    let command;
    match paste_method {
        PasteMethod::CtrlV => command = "echo key ctrl+v | dotool",
        PasteMethod::ShiftInsert => command = "echo key shift+insert | dotool",
        PasteMethod::CtrlShiftV => command = "echo key ctrl+shift+v | dotool",
        _ => return Err("Unsupported paste method".into()),
    }
    use std::process::Stdio;
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to execute dotool: {}", e))?;
    if !status.success() {
        return Err("dotool failed".into());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn ydotool_key_args(
    paste_method: &PasteMethod,
    syntax: YdotoolKeySyntax,
) -> Result<&'static [&'static str], String> {
    let args = match (paste_method, syntax) {
        (PasteMethod::CtrlV, YdotoolKeySyntax::Symbolic) => &["key", "ctrl+v"][..],
        (PasteMethod::CtrlShiftV, YdotoolKeySyntax::Symbolic) => &["key", "ctrl+shift+v"][..],
        (PasteMethod::ShiftInsert, YdotoolKeySyntax::Symbolic) => &["key", "shift+insert"][..],
        (PasteMethod::CtrlV, YdotoolKeySyntax::RawKeycodes) => {
            &["key", "29:1", "47:1", "47:0", "29:0"][..]
        }
        (PasteMethod::CtrlShiftV, YdotoolKeySyntax::RawKeycodes) => {
            &["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"][..]
        }
        (PasteMethod::ShiftInsert, YdotoolKeySyntax::RawKeycodes) => {
            &["key", "42:1", "110:1", "110:0", "42:0"][..]
        }
        _ => return Err("Unsupported paste method".into()),
    };

    Ok(args)
}

/// Send a key combination (e.g., Ctrl+V) via ydotool (requires ydotoold daemon).
#[cfg(target_os = "linux")]
fn send_key_combo_via_ydotool(paste_method: &PasteMethod) -> Result<(), String> {
    let syntax = detect_ydotool_key_syntax();
    let args = ydotool_key_args(paste_method, syntax)?;

    let output = Command::new("ydotool")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute ydotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ydotool failed: {}", stderr));
    }

    Ok(())
}

/// Send a key combination (e.g., Ctrl+V) via xdotool on X11.
#[cfg(target_os = "linux")]
fn send_key_combo_via_xdotool(paste_method: &PasteMethod) -> Result<(), String> {
    let key_combo = match paste_method {
        PasteMethod::CtrlV => "ctrl+v",
        PasteMethod::CtrlShiftV => "ctrl+shift+v",
        PasteMethod::ShiftInsert => "shift+Insert",
        _ => return Err("Unsupported paste method".into()),
    };

    let output = Command::new("xdotool")
        .arg("key")
        .arg("--clearmodifiers")
        .arg(key_combo)
        .output()
        .map_err(|e| format!("Failed to execute xdotool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("xdotool failed: {}", stderr));
    }

    Ok(())
}

/// Pastes text by invoking an external script.
/// The script receives the text to paste as a single argument.
fn paste_via_external_script(text: &str, script_path: &str) -> Result<(), String> {
    info!("Pasting via external script: {}", script_path);

    // Do not capture the script's stdio. Wayland clipboard helpers such as
    // wl-copy may fork a background selection daemon that inherits those file
    // descriptors; waiting for captured output would then block until the
    // clipboard selection is replaced instead of returning after the script
    // itself exits.
    use std::process::Stdio;
    let status = Command::new(script_path)
        .arg(text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Failed to execute external script '{}': {}", script_path, e))?;

    if !status.success() {
        return Err(format!(
            "External script '{}' failed with exit code {:?}",
            script_path,
            status.code()
        ));
    }

    Ok(())
}

/// Types text directly by simulating individual key presses.
fn paste_direct(
    text: &str,
    app_handle: &AppHandle,
    #[cfg(target_os = "linux")] typing_tool: TypingTool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if try_direct_typing_linux(text, typing_tool)? {
            return Ok(());
        }
        info!("Falling back to enigo for direct text input");
    }

    let text = text.to_string();
    with_enigo(app_handle, move |enigo| {
        input::paste_text_direct(enigo, &text)
    })
}

pub(crate) fn send_return_key(enigo: &mut Enigo, key_type: AutoSubmitKey) -> Result<(), String> {
    match key_type {
        AutoSubmitKey::Enter => {
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
        }
        AutoSubmitKey::CtrlEnter => {
            enigo
                .key(Key::Control, Direction::Press)
                .map_err(|e| format!("Failed to press Control key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
            enigo
                .key(Key::Control, Direction::Release)
                .map_err(|e| format!("Failed to release Control key: {}", e))?;
        }
        AutoSubmitKey::CmdEnter => {
            enigo
                .key(Key::Meta, Direction::Press)
                .map_err(|e| format!("Failed to press Meta/Cmd key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Press)
                .map_err(|e| format!("Failed to press Return key: {}", e))?;
            enigo
                .key(Key::Return, Direction::Release)
                .map_err(|e| format!("Failed to release Return key: {}", e))?;
            enigo
                .key(Key::Meta, Direction::Release)
                .map_err(|e| format!("Failed to release Meta/Cmd key: {}", e))?;
        }
    }

    Ok(())
}

fn should_send_auto_submit(auto_submit: bool, paste_method: PasteMethod) -> bool {
    auto_submit && paste_method != PasteMethod::None
}

/// Pastes `text` into the focused application. Call it from a worker thread,
/// never the main thread: it sleeps between steps, and only the input
/// injection hops to the main thread (see `with_enigo`).
pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let _paste = PASTE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = get_settings(&app_handle);
    let paste_method = settings.paste_method;
    let paste_delay_ms = settings.paste_delay_ms;
    let paste_delay_after_ms = settings.paste_delay_after_ms;

    // Append trailing space if setting is enabled
    let text = if settings.append_trailing_space {
        format!("{} ", text)
    } else {
        text
    };

    info!(
        "Using paste method: {:?}, delay before: {}ms, delay after: {}ms",
        paste_method, paste_delay_ms, paste_delay_after_ms
    );

    // Perform the paste operation
    match paste_method {
        PasteMethod::None => {
            info!("PasteMethod::None selected - skipping paste action");
        }
        PasteMethod::Direct => {
            paste_direct(
                &text,
                &app_handle,
                #[cfg(target_os = "linux")]
                settings.typing_tool,
            )?;
        }
        PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert => {
            // Debug-gated receipt-sequenced paste (#502): restore the clipboard
            // after the target actually reads the transcript, not on a timer.
            // On success it fully handles the paste (including auto-submit and
            // clipboard handling) asynchronously; on failure fall through to
            // the legacy path untouched.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if settings.reliable_paste {
                // Runs entirely on the main thread, as it requires.
                let (text, app) = (text.clone(), app_handle.clone());
                let (auto_submit, auto_submit_key, clipboard_handling) = (
                    settings.auto_submit,
                    settings.auto_submit_key,
                    settings.clipboard_handling,
                );
                let reliable_result = with_enigo(&app_handle, move |enigo| {
                    crate::paste_tx::try_reliable_paste(
                        &text,
                        &app,
                        &paste_method,
                        enigo,
                        auto_submit,
                        auto_submit_key,
                        clipboard_handling,
                    )
                });
                match reliable_result {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        log::warn!("Reliable paste unavailable ({e}); falling back to legacy paste")
                    }
                }
            }
            paste_via_clipboard(
                &text,
                &app_handle,
                &paste_method,
                paste_delay_ms,
                paste_delay_after_ms,
            )?
        }
        PasteMethod::ExternalScript => {
            let script_path = settings
                .external_script_path
                .as_ref()
                .filter(|p| !p.is_empty())
                .ok_or("External script path is not configured")?;
            paste_via_external_script(&text, script_path)?;
        }
    }

    if should_send_auto_submit(settings.auto_submit, paste_method) {
        std::thread::sleep(Duration::from_millis(50));
        let auto_submit_key = settings.auto_submit_key;
        if let Err(error) = with_enigo(&app_handle, move |enigo| {
            send_return_key(enigo, auto_submit_key)
        }) {
            log::warn!("Paste succeeded, but auto-submit failed: {error}");
        }
    }

    // After pasting, optionally copy to clipboard based on settings
    if settings.clipboard_handling == ClipboardHandling::CopyToClipboard {
        write_text_to_clipboard(&app_handle, &text)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[cfg(target_os = "linux")]
    const YDOTOOL_0_1_8_HELP: &str = r#"
Usage: key [--delay <ms>] [--key-delay <ms>] [--repeat <times>] [--repeat-delay <ms>] <key sequence> ...
Each key sequence can be any number of modifiers and keys, separated by plus (+)
For example: alt+r Alt+F4 CTRL+alt+f3 aLT+1+2+3 ctrl+Backspace
"#;

    #[cfg(target_os = "linux")]
    const YDOTOOL_1_0_4_HELP: &str = r#"
Usage: key [OPTION]... [KEYCODES]...
Since there's no way to know how many keyboard layouts are there in the world,
we're using raw keycodes now.
Syntax: <keycode>:<pressed>
e.g. 28:1 28:0 means pressing on the Enter button on a standard US keyboard.
"#;

    #[cfg(target_os = "linux")]
    #[test]
    fn classifies_ydotool_0_1_8_symbolic_help() {
        assert_eq!(
            classify_ydotool_key_syntax(YDOTOOL_0_1_8_HELP),
            Some(YdotoolKeySyntax::Symbolic)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn classifies_ydotool_1_0_4_raw_keycode_help() {
        assert_eq!(
            classify_ydotool_key_syntax(YDOTOOL_1_0_4_HELP),
            Some(YdotoolKeySyntax::RawKeycodes)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unknown_ydotool_help_falls_back_to_raw_keycodes() {
        let syntax = classify_ydotool_key_syntax("unrecognized help output")
            .unwrap_or(YDOTOOL_UNKNOWN_HELP_FALLBACK);

        assert_eq!(syntax, YdotoolKeySyntax::RawKeycodes);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn generates_symbolic_ydotool_arguments_for_all_paste_methods() {
        assert_eq!(
            ydotool_key_args(&PasteMethod::CtrlV, YdotoolKeySyntax::Symbolic).unwrap(),
            ["key", "ctrl+v"]
        );
        assert_eq!(
            ydotool_key_args(&PasteMethod::CtrlShiftV, YdotoolKeySyntax::Symbolic).unwrap(),
            ["key", "ctrl+shift+v"]
        );
        assert_eq!(
            ydotool_key_args(&PasteMethod::ShiftInsert, YdotoolKeySyntax::Symbolic).unwrap(),
            ["key", "shift+insert"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn generates_raw_ydotool_arguments_for_all_paste_methods() {
        assert_eq!(
            ydotool_key_args(&PasteMethod::CtrlV, YdotoolKeySyntax::RawKeycodes).unwrap(),
            ["key", "29:1", "47:1", "47:0", "29:0"]
        );
        assert_eq!(
            ydotool_key_args(&PasteMethod::CtrlShiftV, YdotoolKeySyntax::RawKeycodes).unwrap(),
            ["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"]
        );
        assert_eq!(
            ydotool_key_args(&PasteMethod::ShiftInsert, YdotoolKeySyntax::RawKeycodes).unwrap(),
            ["key", "42:1", "110:1", "110:0", "42:0"]
        );
    }

    #[test]
    fn auto_submit_requires_setting_enabled() {
        assert!(!should_send_auto_submit(false, PasteMethod::CtrlV));
        assert!(!should_send_auto_submit(false, PasteMethod::Direct));
    }

    #[test]
    fn auto_submit_skips_none_paste_method() {
        assert!(!should_send_auto_submit(true, PasteMethod::None));
    }

    #[test]
    fn auto_submit_runs_for_active_paste_methods() {
        assert!(should_send_auto_submit(true, PasteMethod::CtrlV));
        assert!(should_send_auto_submit(true, PasteMethod::Direct));
        assert!(should_send_auto_submit(true, PasteMethod::CtrlShiftV));
        assert!(should_send_auto_submit(true, PasteMethod::ShiftInsert));
    }

    #[test]
    fn clipboard_is_restored_before_key_injection_error_is_returned() {
        let restored = Cell::new(false);
        let result = finish_clipboard_paste(Err("input failed".into()), 0, || {
            restored.set(true);
        });

        assert_eq!(result.unwrap_err(), "input failed");
        assert!(restored.get());
    }

    #[cfg(target_os = "linux")]
    fn test_limits(stall_timeout: Duration, total_timeout: Duration) -> ReadLimits {
        ReadLimits {
            stall_timeout,
            total_timeout,
            max_bytes: 1024 * 1024,
        }
    }

    /// Runs `write` against the writing end of a socket pair on its own
    /// thread, standing in for the app that owns the clipboard.
    #[cfg(target_os = "linux")]
    fn clipboard_owner(
        write: impl FnOnce(std::os::unix::net::UnixStream) + Send + 'static,
    ) -> std::os::unix::net::UnixStream {
        let (reader, writer) = std::os::unix::net::UnixStream::pair().expect("create socket pair");
        std::thread::spawn(move || write(writer));
        reader
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_with_limits_reads_until_the_owner_closes() {
        let reader = clipboard_owner(|mut writer| {
            use std::io::Write;
            let _ = writer.write_all(&[7u8; 300_000]);
        });

        let limits = test_limits(Duration::from_secs(5), Duration::from_secs(10));
        let contents = read_with_limits(reader, &limits).unwrap();

        assert_eq!(contents.len(), 300_000);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_with_limits_gives_up_on_an_owner_that_sends_nothing() {
        let reader = clipboard_owner(|writer| {
            std::thread::sleep(Duration::from_secs(30));
            drop(writer);
        });
        let started = std::time::Instant::now();

        let limits = test_limits(Duration::from_millis(200), Duration::from_secs(10));
        let result = read_with_limits(reader, &limits);

        assert!(result.unwrap_err().contains("no data"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_with_limits_waits_while_data_keeps_arriving() {
        // Six 0.15 s gaps outlast the 0.5 s stall timeout only in total.
        let reader = clipboard_owner(|mut writer| {
            use std::io::Write;
            for byte in b"abcdef" {
                std::thread::sleep(Duration::from_millis(150));
                let _ = writer.write_all(&[*byte]);
            }
        });

        let limits = test_limits(Duration::from_millis(500), Duration::from_secs(10));
        let contents = read_with_limits(reader, &limits).unwrap();

        assert_eq!(contents, b"abcdef");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_with_limits_stops_data_that_never_ends() {
        let reader = clipboard_owner(|mut writer| {
            use std::io::Write;
            while writer.write_all(b"x").is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let started = std::time::Instant::now();

        let limits = test_limits(Duration::from_secs(1), Duration::from_millis(300));
        let result = read_with_limits(reader, &limits);

        assert!(result.unwrap_err().contains("longer than"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn read_with_limits_stops_a_flood_at_the_size_limit() {
        let reader = clipboard_owner(|mut writer| {
            use std::io::Write;
            let chunk = [0u8; 64 * 1024];
            while writer.write_all(&chunk).is_ok() {}
        });

        let limits = test_limits(Duration::from_secs(5), Duration::from_secs(10));
        let result = read_with_limits(reader, &limits);

        assert!(result.unwrap_err().contains("more than"));
    }

    #[cfg(unix)]
    #[test]
    fn external_script_does_not_wait_for_inherited_stdio() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::thread;

        let script_path = std::env::temp_dir().join(format!(
            "handy-external-script-{}-{}.sh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after UNIX_EPOCH")
                .as_nanos()
        ));
        fs::write(&script_path, "#!/bin/sh\nsleep 3 &\nexit 0\n").expect("write external script");
        let mut permissions = fs::metadata(&script_path)
            .expect("read external script metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script_path, permissions).expect("make external script executable");

        let (sender, receiver) = mpsc::channel();
        let script_path_for_thread = script_path.clone();
        thread::spawn(move || {
            let result =
                paste_via_external_script("test", script_path_for_thread.to_str().unwrap());
            sender.send(result).expect("send script result");
        });

        let result = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("external script should return without waiting for its child");
        fs::remove_file(script_path).expect("remove external script");
        assert!(result.is_ok());
    }
}
