#[cfg(target_os = "linux")]
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
#[cfg(target_os = "linux")]
use ashpd::desktop::PersistMode;
#[cfg(target_os = "linux")]
use ashpd::zbus::{self, zvariant::OwnedValue};
#[cfg(target_os = "linux")]
use unicode_normalization::UnicodeNormalization;
#[cfg(target_os = "linux")]
use log::{debug, warn};
#[cfg(target_os = "linux")]
use once_cell::sync::{Lazy, OnceCell};
#[cfg(target_os = "linux")]
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "linux")]
use tokio::runtime::RuntimeFlavor;

#[cfg(target_os = "linux")]
static REMOTE_DESKTOP_TOKEN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
#[cfg(target_os = "linux")]
static PORTAL_RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
#[cfg(target_os = "linux")]
static PORTAL_APP_HANDLE: OnceCell<AppHandle> = OnceCell::new();
#[cfg(target_os = "linux")]
static AUTHORIZED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "linux")]
fn portal_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    PORTAL_RT
        .get_or_try_init(|| {
            tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to initialize portal runtime: {}", e))
        })
        .map_err(|e| e.to_string())
}

// Safely run portal async code even when we're already inside a Tokio runtime.
// Tokio panics if `block_on` is called on a worker thread that is currently
// driving the runtime. If a runtime handle exists and is multi-threaded, we
// hop into a blocking section so nested `block_on` is allowed. For non-
// multithreaded runtimes we bail out with an explicit error.
#[cfg(target_os = "linux")]
fn block_on_portal<F, Fut, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| handle.block_on(f()))
        }
        Ok(_) => Err("remote desktop requires a multi-thread Tokio runtime".into()),
        Err(_) => {
            let runtime = portal_runtime()?;
            runtime.block_on(f())
        }
    }
}

// ============================================================================
// Token State (Memory)
// ============================================================================
#[cfg(target_os = "linux")]
fn set_token_memory(token: &str) {
    if let Ok(mut stored) = REMOTE_DESKTOP_TOKEN.lock() {
        *stored = Some(token.to_string());
    }
}

#[cfg(target_os = "linux")]
fn delete_token_memory() {
    if let Ok(mut stored) = REMOTE_DESKTOP_TOKEN.lock() {
        *stored = None;
    }
}

#[cfg(target_os = "linux")]
fn get_token_memory() -> Option<String> {
    REMOTE_DESKTOP_TOKEN
        .lock()
        .ok()
        .and_then(|token| token.clone())
}

// ============================================================================
// Token Settings (Persistent Storage)
// ============================================================================
#[cfg(target_os = "linux")]
fn set_token_setting(token: &str) {
    if let Some(app) = PORTAL_APP_HANDLE.get() {
        crate::settings::set_remote_desktop_token(app, Some(token.to_string()));
    }
}

#[cfg(target_os = "linux")]
fn delete_token_setting() {
    if let Some(app) = PORTAL_APP_HANDLE.get() {
        crate::settings::set_remote_desktop_token(app, None);
    }
}

#[cfg(target_os = "linux")]
fn get_token_setting() -> Option<String> {
    PORTAL_APP_HANDLE
        .get()
        .and_then(|app| crate::settings::get_remote_desktop_token(app))
}

// ============================================================================
// Authorization State (Memory)
// ============================================================================
#[cfg(target_os = "linux")]
fn set_authorized(value: bool) {
    let previous = AUTHORIZED.swap(value, Ordering::Relaxed);
    if previous != value {
        if let Some(app) = PORTAL_APP_HANDLE.get() {
            let _ = app.emit("remote-desktop-auth-changed", value);
        }
    }
}

#[cfg(target_os = "linux")]
fn get_authorized() -> bool {
    AUTHORIZED.load(Ordering::Relaxed)
}

// ============================================================================
// Token Portal Store (D-Bus)
// ============================================================================
#[cfg(target_os = "linux")]
async fn delete_token_store_async(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Ok(());
    }

    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let connection = zbus::Connection::session().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.impl.portal.PermissionStore",
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore",
        )
        .await?;

        let args = ("remote-desktop", token);
        let _: () = proxy.call("Delete", &args).await?;
        Ok::<(), zbus::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            warn!("PermissionStore.Delete failed for token {}: {}", token, err);
            Err(format!("Failed to delete permission entry: {}", err))
        }
        Err(_) => {
            warn!("PermissionStore.Delete timed out for token: {}", token);
            Err("PermissionStore.Delete timed out".to_string())
        }
    }
}

#[cfg(target_os = "linux")]
fn delete_token_store(token: &str) -> Result<(), String> {
    block_on_portal(|| delete_token_store_async(token))
}

#[cfg(target_os = "linux")]
async fn exists_token_store_async(token: &str) -> Result<bool, String> {
    if token.is_empty() {
        return Ok(false);
    }

    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let connection = zbus::Connection::session().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.impl.portal.PermissionStore",
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore",
        )
        .await?;

        let args = ("remote-desktop", token);
        let _: (HashMap<String, Vec<String>>, OwnedValue) = proxy.call("Lookup", &args).await?;
        Ok::<bool, zbus::Error>(true)
    })
    .await;

    match result {
        Ok(Ok(exists)) => Ok(exists),
        Ok(Err(err)) => {
            debug!("remote_desktop: token lookup error: {}", err);
            Ok(false)
        }
        Err(err) => {
            debug!("remote_desktop: token lookup timeout: {}", err);
            Ok(false)
        }
    }
}

#[cfg(target_os = "linux")]
fn validate_token_store() {
    // Check if the stored token exists in the portal store.
    let token = get_token_memory().or_else(get_token_setting);
    let Some(token) = token else {
        debug!("remote_desktop: no token found, AUTHORIZED set false");
        delete_token_everywhere();
        return;
    };

    let exists = match block_on_portal(|| exists_token_store_async(&token)) {
        Ok(res) => res,
        Err(err) => {
            debug!("remote_desktop: portal runtime init failed: {}", err);
            return;
        }
    };

    if !exists {
        debug!("remote_desktop: token missing, deleting via delete_token_everywhere()");
        delete_token_everywhere();
    }
}

#[cfg(target_os = "linux")]
fn delete_token_everywhere() {
    let token_memory = get_token_memory();
    let token_setting = get_token_setting();
    let token = token_memory.as_deref().or(token_setting.as_deref());

    set_authorized(false);
    if let Some(token) = token {
        let _ = delete_token_store(token);
    }
    if token_memory.is_some() {
        delete_token_memory();
    }
    if token_setting.is_some() {
        delete_token_setting();
    }
}
// ============================================================================
// Keyboard Input via Portal
// ============================================================================
#[cfg(target_os = "linux")]
fn keysym_for_char(ch: char) -> Option<u32> {
    match ch {
        '\n' | '\r' => Some(0xFF0D), // XK_Return
        '\t' => Some(0xFF09),        // XK_Tab
        '\u{8}' => Some(0xFF08),     // XK_BackSpace
        // Characters in the ISO‑8859‑1 range (including most accented Latin letters)
        // are represented directly as keysyms. Only higher code points should use
        // the "Unicode keysym" prefix (0x0100_0000).
        _ if (ch as u32) <= 0xFF => Some(ch as u32),
        _ => Some(0x0100_0000 | (ch as u32)), // Unicode keysym
    }
}

#[cfg(target_os = "linux")]
async fn type_text_async(text: &str) -> Result<(), String> {
    let (proxy, session) = open_session_async(false).await?;

    // Helper to send a press/release pair for a keysym.
    async fn send_keysym(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        keysym: u32,
    ) -> Result<(), String> {
        proxy
            .notify_keyboard_keysym(session, keysym as i32, KeyState::Pressed)
            .await
            .map_err(|e| format!("Failed to send keysym press: {}", e))?;
        proxy
            .notify_keyboard_keysym(session, keysym as i32, KeyState::Released)
            .await
            .map_err(|e| format!("Failed to send keysym release: {}", e))
    }

    // Send a non-ASCII character through the Ctrl+Shift+U unicode input sequence to
    // stay independent of the current keyboard layout.
    async fn send_unicode_via_ctrl_shift_u(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        ch: char,
    ) -> Result<(), String> {
        // Keysyms for modifiers and validation.
        const XK_CONTROL_L: u32 = 0xFFE3;
        const XK_SHIFT_L: u32 = 0xFFE1;
        const XK_RETURN: u32 = 0xFF0D;
        // 1) Press Control_L then Shift_L
        proxy
            .notify_keyboard_keysym(session, XK_CONTROL_L as i32, KeyState::Pressed)
            .await
            .map_err(|e| format!("unicode-input failed pressing Control: {e}"))?;
        proxy
            .notify_keyboard_keysym(session, XK_SHIFT_L as i32, KeyState::Pressed)
            .await
            .map_err(|e| format!("unicode-input failed pressing Shift: {e}"))?;
        // 2) Press/Release 'u'
        send_keysym(proxy, session, 'u' as u32).await?;
        // 3) Release Shift_L then Control_L
        proxy
            .notify_keyboard_keysym(session, XK_SHIFT_L as i32, KeyState::Released)
            .await
            .map_err(|e| format!("unicode-input failed releasing Shift: {e}"))?;
        proxy
            .notify_keyboard_keysym(session, XK_CONTROL_L as i32, KeyState::Released)
            .await
            .map_err(|e| format!("unicode-input failed releasing Control: {e}"))?;

        // 4) Send hex digits of the codepoint (lowercase).
        let hex = format!("{:x}", ch as u32);
        for (idx, digit) in hex.chars().enumerate() {
            let keysym = keysym_for_char(digit)
                .ok_or_else(|| format!("unicode-input: unsupported hex digit '{digit}'"))?;
            send_keysym(proxy, session, keysym)
                .await
                .map_err(|e| format!("unicode-input failed at hex digit #{idx} '{digit}': {e}"))?;
        }

        // 5) Validate with Return.
        send_keysym(proxy, session, XK_RETURN).await?;
        // Give the portal a brief moment to exit the Ctrl+Shift+U compose state
        // before the next character, to avoid the following key being swallowed.
        tokio::time::sleep(Duration::from_millis(8)).await;
        Ok(())
    }

    let result = (|| async {
        // Normalize to NFC so we send precomposed characters (é, ô, …) as single keysyms.
        let normalized = text.nfc().collect::<String>();
        for ch in normalized.chars() {
            if (ch as u32) > 0x7F {
                send_unicode_via_ctrl_shift_u(&proxy, &session, ch).await?;
            } else {
                let keysym =
                    keysym_for_char(ch).ok_or_else(|| "Unsupported character".to_string())?;
                send_keysym(&proxy, &session, keysym).await?;
            }
        }
        Ok(())
    })()
    .await;

    if let Err(err) = close_session_async(&session).await {
        debug!("remote_desktop: {}", err);
    }

    result
}

// ============================================================================
// Remote Desktop Session Management
// ============================================================================
#[cfg(target_os = "linux")]
async fn close_session_async(
    session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
) -> Result<(), String> {
    session
        .close()
        .await
        .map_err(|e| format!("Failed to close RemoteDesktop session: {}", e))
}

#[cfg(target_os = "linux")]
async fn open_session_async(
    allow_prompt: bool,
) -> Result<
    (
        RemoteDesktop<'static>,
        ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
    ),
    String,
> {
    // Connect to the RemoteDesktop portal.
    let proxy = RemoteDesktop::new()
        .await
        .map_err(|e| format!("Failed to connect to RemoteDesktop portal: {}", e))?;

    // Create a new portal session.
    let session = proxy
        .create_session()
        .await
        .map_err(|e| format!("Failed to create RemoteDesktop session: {}", e))?;

    // Check existing token if no prompt is allowed.
    let remote_desktop_token = get_token_memory();
    if !allow_prompt {
        let Some(token) = remote_desktop_token.as_deref() else {
            delete_token_everywhere();
            return Err("portal-permission-not-granted".into());
        };
        let exists = exists_token_store_async(token).await?;
        if !exists {
            delete_token_everywhere();
            return Err("portal-permission-not-granted".into());
        }
    }

    // Request keyboard device access via the portal.
    let device_types = DeviceType::Keyboard.into();
    proxy
        .select_devices(
            &session,
            device_types,
            remote_desktop_token.as_deref(),
            PersistMode::ExplicitlyRevoked,
        )
        .await
        .map_err(|e| format!("Failed to request RemoteDesktop devices: {}", e))?
        .response()
        .map_err(|e| format!("RemoteDesktop device request denied: {}", e))?;
    // Start the session (may trigger permission UI).
    let response = proxy
        .start(&session, None)
        .await
        .map_err(|e| format!("Failed to start RemoteDesktop session: {}", e))?
        .response()
        .map_err(|e| format!("portal-permission-denied: {e}"))?;

    // Persist any new token returned by the portal.
    if let Some(token) = response.restore_token() {
        set_authorized(true);
        set_token_memory(token);
        set_token_setting(token);
    }

    Ok((proxy, session))
}

// ============================================================================
// Public Functions - Keyboard Input via Portal
// ============================================================================
#[cfg(target_os = "linux")]
pub fn send_type_text(text: &str) -> Result<(), String> {
    if !crate::utils::is_wayland() {
        return Err("not running on Wayland".into());
    }
    if !get_authorized() {
        return Err("authorization not granted".into());
    }
    block_on_portal(|| type_text_async(text))
}

#[cfg(target_os = "linux")]
pub fn is_available() -> bool {
    crate::utils::is_wayland() && get_authorized()
}

#[cfg(target_os = "linux")]
pub fn get_authorization() -> bool {
    get_authorized()
}

#[cfg(target_os = "linux")]
pub fn request_authorization() -> Result<(), String> {
    if !crate::utils::is_wayland() {
        return Ok(());
    }

    let (proxy, session) = block_on_portal(|| open_session_async(true))?;
    // Drop proxy after closing to avoid holding session references.
    let result = block_on_portal(|| close_session_async(&session));
    drop(proxy);
    result
}

#[cfg(target_os = "linux")]
pub fn delete_authorization() {
    delete_token_everywhere();
}

#[cfg(target_os = "linux")]
pub fn init_authorization(app: &AppHandle) {
    if !crate::utils::is_wayland() {
        return;
    }
    let _ = PORTAL_APP_HANDLE.set(app.clone());
    let token = get_token_setting();
    if let Some(token) = token {
        set_authorized(true);
        set_token_memory(&token);
        validate_token_store();
        debug!("remote_desktop: REMOTE_DESKTOP_TOKEN initialized from settings");
    } else {
        debug!("remote_desktop: no REMOTE_DESKTOP_TOKEN in settings");
    }
}
