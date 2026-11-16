use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::TranscriptionManager;
use crate::shortcut;
use crate::TranscriptionCoordinator;
use log::info;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

// Re-export all utility modules for easy access
// pub use crate::audio_feedback::*;
pub use crate::clipboard::*;
pub use crate::overlay::*;
pub use crate::tray::*;

/// Preserve diagnostic text in development builds, but redact it in releases.
/// Do not use for secrets such as API keys, which must always be redacted.
pub fn redact_text(text: &str) -> &str {
    if cfg!(debug_assertions) {
        text
    } else {
        "[REDACTED]"
    }
}

/// Main window's opening and minimum page size, in logical pixels.
pub const MAIN_WINDOW_SIZE: (f64, f64) = (720.0, 600.0);

/// Title bar height in logical pixels (macOS draws it inside the inner size).
/// Call on the main thread.
#[cfg(target_os = "macos")]
pub fn measure_title_bar(window: &tauri::WebviewWindow) -> f64 {
    let Ok(ptr) = window.ns_window() else {
        return 0.0;
    };
    // SAFETY: `ns_window` is the live NSWindow behind `window`, and this runs
    // on the main thread.
    let ns_window = unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
    (ns_window.frame().size.height - ns_window.contentLayoutRect().size.height).max(0.0)
}

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
const IMAGE_FILE_MACHINE_ARM64: u16 = 0xaa64;

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
fn native_machine_is_arm64(native_machine: Option<u16>) -> bool {
    native_machine == Some(IMAGE_FILE_MACHINE_ARM64)
}

/// Whether this is the x64 Windows build running under emulation on Windows ARM64.
///
/// Only that exact process/host pairing disables the transcribe.cpp GPU path.
/// Detection is deliberately fail-open: a native x64 host, an older Windows
/// version without `IsWow64Process2`, or any API error leaves existing behavior
/// unchanged.
pub fn is_windows_x64_emulated_on_arm64() -> bool {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        use std::sync::OnceLock;

        static DETECTED: OnceLock<bool> = OnceLock::new();
        *DETECTED.get_or_init(|| native_machine_is_arm64(native_windows_machine()))
    }

    #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
    {
        false
    }
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn native_windows_machine() -> Option<u16> {
    use windows::core::{s, w, BOOL};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Threading::GetCurrentProcess;

    type IsWow64Process2 = unsafe extern "system" fn(HANDLE, *mut u16, *mut u16) -> BOOL;

    // Resolve IsWow64Process2 dynamically so merely starting Handy never raises
    // the minimum Windows version. Windows-on-ARM versions provide this API,
    // while a missing symbol or failed query safely preserves the x64 behavior.
    unsafe {
        let kernel32 = GetModuleHandleW(w!("kernel32.dll")).ok()?;
        let address = GetProcAddress(kernel32, s!("IsWow64Process2"))?;
        // SAFETY: GetProcAddress returned the documented IsWow64Process2 symbol;
        // function pointers have the same representation on supported Windows.
        let is_wow64_process2: IsWow64Process2 = std::mem::transmute(address);
        let mut process_machine = 0u16;
        let mut native_machine = 0u16;
        is_wow64_process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
        .as_bool()
        .then_some(native_machine)
    }
}

/// What Windows said about the integrity level of the focused window's process.
#[cfg(any(test, target_os = "windows"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForegroundIntegrity {
    Level(u32),
    /// Windows refused the query. It does that for a process that is elevated
    /// under another account, which Handy can't send input to either.
    Denied,
    /// No focused window, or the query failed for another reason.
    Unknown,
}

/// Fail-open: when either level is unknown, Handy pastes as usual.
#[cfg(any(test, target_os = "windows"))]
fn integrity_blocks_input(foreground: ForegroundIntegrity, ours: Option<u32>) -> bool {
    match foreground {
        ForegroundIntegrity::Level(theirs) => ours.is_some_and(|ours| theirs > ours),
        ForegroundIntegrity::Denied => true,
        ForegroundIntegrity::Unknown => false,
    }
}

/// Whether the focused window drops the keystrokes Handy sends it.
///
/// Windows blocks input sent to a process at a higher integrity level, such as
/// an app started with "Run as administrator", and `SendInput` isn't documented
/// to report it. Always `false` on other platforms. See issue #434.
pub fn foreground_window_is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::Threading::GetCurrentProcess;

        // SAFETY: the pseudo-handle from GetCurrentProcess is always valid and
        // needs no closing.
        let ours = unsafe { process_integrity_level(GetCurrentProcess()) }.ok();
        let foreground = foreground_integrity();
        log::debug!("Integrity levels: foreground {foreground:?}, Handy {ours:?}");
        integrity_blocks_input(foreground, ours)
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn foreground_integrity() -> ForegroundIntegrity {
    use windows::Win32::Foundation::{CloseHandle, E_ACCESSDENIED};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    // SAFETY: every handle is checked before use, and the process handle is
    // closed before returning.
    let level = unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return ForegroundIntegrity::Unknown;
        }
        let mut process_id = 0u32;
        if GetWindowThreadProcessId(window, Some(&mut process_id)) == 0 {
            return ForegroundIntegrity::Unknown;
        }
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).and_then(|process| {
            let level = process_integrity_level(process);
            let _ = CloseHandle(process);
            level
        })
    };

    match level {
        Ok(level) => ForegroundIntegrity::Level(level),
        Err(e) if e.code() == E_ACCESSDENIED => ForegroundIntegrity::Denied,
        Err(e) => {
            log::debug!("Couldn't read the focused window's integrity level: {e}");
            ForegroundIntegrity::Unknown
        }
    }
}

/// The integrity level of `process`, e.g. `0x2000` for a normal app and
/// `0x3000` for one run as administrator.
///
/// # Safety
/// `process` must be a valid handle with `PROCESS_QUERY_LIMITED_INFORMATION`.
#[cfg(target_os = "windows")]
unsafe fn process_integrity_level(
    process: windows::Win32::Foundation::HANDLE,
) -> windows::core::Result<u32> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;

    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token)?;

    // A TOKEN_MANDATORY_LABEL followed by the SID it points to; a SID is at
    // most 68 bytes. u64 elements keep the buffer aligned for the struct.
    let mut buffer = [0u64; 16];
    let mut written = 0u32;
    let queried = GetTokenInformation(
        token,
        TokenIntegrityLevel,
        Some(buffer.as_mut_ptr().cast()),
        std::mem::size_of_val(&buffer) as u32,
        &mut written,
    );
    let _ = CloseHandle(token);
    queried?;

    // The level is the SID's last sub-authority.
    let sid = (*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid;
    let count = u32::from(*GetSidSubAuthorityCount(sid));
    Ok(*GetSidSubAuthority(sid, count.saturating_sub(1)))
}

/// Centralized cancellation function that can be called from anywhere in the app.
/// Handles cancelling both recording and transcription operations and updates UI state.
pub fn cancel_current_operation(app: &AppHandle) {
    info!("Initiating operation cancellation...");

    // Unregister the cancel shortcut asynchronously
    shortcut::unregister_cancel_shortcut(app);

    // Cancel any ongoing recording
    let audio_manager = app.state::<Arc<AudioRecordingManager>>();
    let recording_was_active = audio_manager.is_recording();
    audio_manager.cancel_recording();

    // Abandon any live streaming transcription, and stop a transcription
    // already in progress
    let tm = app.state::<Arc<TranscriptionManager>>();
    tm.cancel_stream();
    tm.cancel_transcription();

    // Update tray icon and hide overlay
    set_tray_state(app, crate::tray::TrayIconState::Idle);
    hide_recording_overlay(app);

    // Unload model if immediate unload is enabled. Done before notifying the
    // coordinator, so a recording started next always loads afresh; it
    // doesn't wait for the cancelled work, so it never blocks this thread.
    tm.maybe_unload_immediately("cancellation");

    // Notify coordinator so it can keep lifecycle state coherent.
    if let Some(coordinator) = app.try_state::<TranscriptionCoordinator>() {
        coordinator.notify_cancel(recording_was_active);
    }

    info!("Operation cancellation completed - returned to idle state");
}

/// Check if using the Wayland display server protocol
#[cfg(target_os = "linux")]
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.to_lowercase() == "wayland")
            .unwrap_or(false)
}

/// Check if running on KDE Plasma desktop environment
#[cfg(target_os = "linux")]
pub fn is_kde_plasma() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_uppercase().contains("KDE"))
        .unwrap_or(false)
        || std::env::var("KDE_SESSION_VERSION").is_ok()
}

/// Check if running on KDE Plasma with Wayland
#[cfg(target_os = "linux")]
pub fn is_kde_wayland() -> bool {
    is_wayland() && is_kde_plasma()
}

/// Check if running on GNOME desktop environment
#[cfg(target_os = "linux")]
pub fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_uppercase().contains("GNOME"))
        .unwrap_or(false)
}

/// Check if running on GNOME with Wayland
#[cfg(target_os = "linux")]
pub fn is_gnome_wayland() -> bool {
    is_wayland() && is_gnome()
}

/// Returns true when the environment variable is set to a truthy value
/// (e.g. "1", "true", "yes", "on").
/// "0", "false", "no", "off" and empty string are treated as falsy (case-insensitive).
/// Returns false when the variable is not set.
pub fn env_flag_enabled(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        ),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
static GAMESCOPE_WSI_DISABLED_BY_HANDY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Only set `DISABLE_GAMESCOPE_WSI` when the user hasn't set it (even to empty).
#[cfg(target_os = "linux")]
fn should_disable_gamescope_wsi(existing: Option<&std::ffi::OsStr>) -> bool {
    existing.is_none()
}

/// Disable the Gamescope WSI layer, which aborts during Vulkan device creation
/// on SteamOS (#2264). Handy only uses Vulkan for compute, so it isn't needed.
#[cfg(target_os = "linux")]
pub fn disable_gamescope_wsi_layer() {
    let existing = std::env::var_os("DISABLE_GAMESCOPE_WSI");
    if should_disable_gamescope_wsi(existing.as_deref()) {
        std::env::set_var("DISABLE_GAMESCOPE_WSI", "1");
        GAMESCOPE_WSI_DISABLED_BY_HANDY.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// True when [`disable_gamescope_wsi_layer`] set the variable itself.
#[cfg(target_os = "linux")]
pub fn gamescope_wsi_disabled_by_handy() -> bool {
    GAMESCOPE_WSI_DISABLED_BY_HANDY.load(std::sync::atomic::Ordering::Relaxed)
}

/// The `GDK_BACKEND` value requested through `HANDY_GDK_BACKEND`, if any.
/// Empty or whitespace-only values count as unset.
#[cfg(target_os = "linux")]
fn requested_gdk_backend(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// Copy `HANDY_GDK_BACKEND` into `GDK_BACKEND` when it is set.
///
/// The AppImage's `apprun-hooks/linuxdeploy-plugin-gtk.sh` unconditionally
/// runs `export GDK_BACKEND=x11` (a workaround for tauri-apps/tauri#8541) after
/// the user's environment, so `GDK_BACKEND=wayland ./Handy.AppImage` never
/// reaches Handy. The hook cannot overwrite a variable it does not know about,
/// so `HANDY_GDK_BACKEND=wayland ./Handy.AppImage` is the opt-in to native
/// Wayland (cjpais/Handy#1729, #2034). Must run before GTK initializes.
#[cfg(target_os = "linux")]
pub fn apply_handy_gdk_backend() {
    let requested = std::env::var("HANDY_GDK_BACKEND").ok();
    if let Some(backend) = requested_gdk_backend(requested.as_deref()) {
        std::env::set_var("GDK_BACKEND", backend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_higher_integrity_level_blocks_input() {
        const MEDIUM: u32 = 0x2000;
        const HIGH: u32 = 0x3000;
        use ForegroundIntegrity::*;

        assert!(integrity_blocks_input(Level(HIGH), Some(MEDIUM)));
        assert!(!integrity_blocks_input(Level(MEDIUM), Some(MEDIUM)));
        // Handy itself running as administrator can paste anywhere.
        assert!(!integrity_blocks_input(Level(HIGH), Some(HIGH)));
        assert!(!integrity_blocks_input(Level(MEDIUM), Some(HIGH)));
        assert!(integrity_blocks_input(Denied, Some(MEDIUM)));
        assert!(!integrity_blocks_input(Unknown, Some(MEDIUM)));
        assert!(!integrity_blocks_input(Level(HIGH), None));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gamescope_wsi_is_disabled_only_when_the_user_set_nothing() {
        use std::ffi::OsStr;
        assert!(should_disable_gamescope_wsi(None));
        assert!(!should_disable_gamescope_wsi(Some(OsStr::new("1"))));
        assert!(!should_disable_gamescope_wsi(Some(OsStr::new("0"))));
        assert!(!should_disable_gamescope_wsi(Some(OsStr::new(""))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn requested_gdk_backend_ignores_unset_and_blank() {
        assert_eq!(requested_gdk_backend(None), None);
        assert_eq!(requested_gdk_backend(Some("")), None);
        assert_eq!(requested_gdk_backend(Some("  ")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn requested_gdk_backend_keeps_the_value() {
        assert_eq!(requested_gdk_backend(Some("wayland")), Some("wayland"));
        assert_eq!(requested_gdk_backend(Some(" x11 ")), Some("x11"));
        assert_eq!(
            requested_gdk_backend(Some("wayland,x11")),
            Some("wayland,x11")
        );
    }

    #[test]
    fn arm64_native_machine_is_the_only_match() {
        assert!(native_machine_is_arm64(Some(IMAGE_FILE_MACHINE_ARM64)));
        assert!(!native_machine_is_arm64(Some(0x8664))); // AMD64
        assert!(!native_machine_is_arm64(Some(0x014c))); // I386
        assert!(!native_machine_is_arm64(None)); // API unavailable or failed
    }

    #[test]
    fn env_flag_enabled_true_for_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on", " 1 "] {
            std::env::set_var("HANDY_TEST_FLAG_TRUTHY", value);
            assert!(env_flag_enabled("HANDY_TEST_FLAG_TRUTHY"), "{value:?}");
        }
        std::env::remove_var("HANDY_TEST_FLAG_TRUTHY");
    }

    #[test]
    fn env_flag_enabled_false_for_falsy_or_unset() {
        assert!(!env_flag_enabled("HANDY_TEST_FLAG_UNSET"));

        for value in ["0", "false", "FALSE", "no", "off", ""] {
            std::env::set_var("HANDY_TEST_FLAG_FALSY", value);
            assert!(!env_flag_enabled("HANDY_TEST_FLAG_FALSY"), "{value:?}");
        }
        std::env::remove_var("HANDY_TEST_FLAG_FALSY");
    }
}
