#[cfg(target_os = "linux")]
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
#[cfg(target_os = "linux")]
use ashpd::desktop::PersistMode;
#[cfg(target_os = "linux")]
use ashpd::zbus::{self, zvariant::OwnedValue};
#[cfg(target_os = "linux")]
use log::{debug, info, warn};
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
use unicode_normalization::UnicodeNormalization;

#[cfg(target_os = "linux")]
mod keymap;

#[cfg(target_os = "linux")]
static REMOTE_DESKTOP_TOKEN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
#[cfg(target_os = "linux")]
static PORTAL_RUNTIME: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
#[cfg(target_os = "linux")]
static PORTAL_APP_HANDLE: OnceCell<AppHandle> = OnceCell::new();
#[cfg(target_os = "linux")]
static AUTHORIZED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "linux")]
fn portal_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    PORTAL_RUNTIME
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
        .and_then(crate::settings::get_remote_desktop_token)
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
        clear_authorization_state();
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
        debug!("remote_desktop: token missing, clearing authorization state");
        clear_authorization_state();
    }
}

#[cfg(target_os = "linux")]
fn clear_authorization_state() {
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
async fn type_text_async(text: &str) -> Result<(), String> {
    let keyboard_map = keymap::KeyboardMap::load_current()?;
    let settings = PORTAL_APP_HANDLE.get().map(crate::settings::get_settings);
    let key_event_delay_ms = settings
        .as_ref()
        .map(|settings| settings.remote_desktop_key_event_delay_ms)
        .unwrap_or(crate::settings::DEFAULT_REMOTE_DESKTOP_KEY_EVENT_DELAY_MS);
    info!(
        "remote_desktop: using key event delay: {}ms",
        key_event_delay_ms
    );
    let (proxy, session) = open_session_async(false).await?;

    async fn send_keycode(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        keycode: i32,
        key_event_delay_ms: u64,
    ) -> Result<(), String> {
        send_key_event(
            proxy,
            session,
            keycode,
            KeyState::Pressed,
            "Failed to send keycode press",
            key_event_delay_ms,
        )
        .await?;
        send_key_event(
            proxy,
            session,
            keycode,
            KeyState::Released,
            "Failed to send keycode release",
            key_event_delay_ms,
        )
        .await
    }

    async fn send_key_event(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        keycode: i32,
        state: KeyState,
        error_context: &str,
        key_event_delay_ms: u64,
    ) -> Result<(), String> {
        proxy
            .notify_keyboard_keycode(session, keycode, state)
            .await
            .map_err(|e| format!("{}: {}", error_context, e))?;
        if key_event_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(key_event_delay_ms)).await;
        }
        Ok(())
    }

    async fn send_stroke(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        stroke: &keymap::KeyStroke,
        key_event_delay_ms: u64,
    ) -> Result<(), String> {
        let mut pressed_modifiers: Vec<keymap::ModifierKey> = Vec::new();
        for modifier in &stroke.modifiers {
            if let Err(err) = send_key_event(
                proxy,
                session,
                modifier.keycode(),
                KeyState::Pressed,
                "Failed to press modifier",
                key_event_delay_ms,
            )
            .await
            {
                for pressed_modifier in pressed_modifiers.iter().rev() {
                    if let Err(release_err) = send_key_event(
                        proxy,
                        session,
                        pressed_modifier.keycode(),
                        KeyState::Released,
                        "Failed to release modifier",
                        key_event_delay_ms,
                    )
                    .await
                    {
                        debug!(
                            "remote_desktop: failed to release modifier: {}",
                            release_err
                        );
                    }
                }

                return Err(err);
            }
            pressed_modifiers.push(*modifier);
        }

        let mut result = send_keycode(proxy, session, stroke.keycode, key_event_delay_ms).await;

        for modifier in pressed_modifiers.iter().rev() {
            if let Err(err) = send_key_event(
                proxy,
                session,
                modifier.keycode(),
                KeyState::Released,
                "Failed to release modifier",
                key_event_delay_ms,
            )
            .await
            {
                if result.is_ok() {
                    result = Err(format!("Failed to release modifier: {}", err));
                } else {
                    debug!("remote_desktop: failed to release modifier: {}", err);
                }
            }
        }

        result
    }

    async fn send_unicode_via_ctrl_shift_u(
        proxy: &RemoteDesktop<'static>,
        session: &ashpd::desktop::Session<'static, RemoteDesktop<'static>>,
        keyboard_map: &keymap::KeyboardMap,
        ch: char,
        key_event_delay_ms: u64,
    ) -> Result<(), String> {
        // Use the GTK/IBus Unicode input sequence for characters that are not
        // directly present in the active compositor keymap.
        let trigger_stroke = keyboard_map
            .find_character('u')
            .ok_or_else(|| "unicode-input: cannot find keycode for 'u'".to_string())?;
        let trigger_modifiers = [keymap::ModifierKey::Control, keymap::ModifierKey::Shift];
        let mut pressed_modifiers: Vec<keymap::ModifierKey> = Vec::new();
        let mut trigger_result = Ok(());

        for modifier in &trigger_modifiers {
            if let Err(err) = send_key_event(
                proxy,
                session,
                modifier.keycode(),
                KeyState::Pressed,
                "unicode-input failed pressing modifier",
                key_event_delay_ms,
            )
            .await
            {
                trigger_result = Err(err);
                break;
            }
            pressed_modifiers.push(*modifier);
        }

        if trigger_result.is_ok() {
            trigger_result =
                send_keycode(proxy, session, trigger_stroke.keycode, key_event_delay_ms).await;
        }

        for modifier in pressed_modifiers.iter().rev() {
            if let Err(err) = send_key_event(
                proxy,
                session,
                modifier.keycode(),
                KeyState::Released,
                "unicode-input failed releasing modifier",
                key_event_delay_ms,
            )
            .await
            {
                if trigger_result.is_ok() {
                    trigger_result = Err(err);
                } else {
                    debug!(
                        "remote_desktop: failed to release unicode modifier: {}",
                        err
                    );
                }
            }
        }

        trigger_result?;

        let hex = format!("{:x}", ch as u32);
        for (idx, digit) in hex.chars().enumerate() {
            let stroke = keyboard_map
                .find_character(digit)
                .ok_or_else(|| format!("unicode-input: cannot find keycode for '{digit}'"))?;
            send_stroke(proxy, session, &stroke, key_event_delay_ms)
                .await
                .map_err(|e| format!("unicode-input failed at hex digit #{idx} '{digit}': {e}"))?;
        }

        let enter_stroke = keyboard_map
            .find_character('\n')
            .ok_or_else(|| "unicode-input: cannot find keycode for Enter".to_string())?;
        send_stroke(proxy, session, &enter_stroke, key_event_delay_ms).await?;
        Ok(())
    }

    let result = async {
        // Normalize to NFC so precomposed characters (é, ô, …) are handled as one character.
        let normalized = text.nfc().collect::<String>();
        for ch in normalized.chars() {
            if let Some(stroke) = keyboard_map.find_character(ch) {
                send_stroke(&proxy, &session, &stroke, key_event_delay_ms).await?;
            } else {
                send_unicode_via_ctrl_shift_u(
                    &proxy,
                    &session,
                    &keyboard_map,
                    ch,
                    key_event_delay_ms,
                )
                .await?;
            }
        }
        Ok(())
    }
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
            clear_authorization_state();
            return Err("portal-permission-not-granted".into());
        };
        let exists = exists_token_store_async(token).await?;
        if !exists {
            clear_authorization_state();
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
/// Sends text through the Remote Desktop portal when Wayland authorization exists.
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

/// Returns whether Remote Desktop direct typing can be used right now.
#[cfg(target_os = "linux")]
pub fn is_available() -> bool {
    crate::utils::is_wayland() && get_authorized()
}

/// Returns the cached Remote Desktop authorization state.
#[cfg(target_os = "linux")]
pub fn get_authorization() -> bool {
    get_authorized()
}

/// Requests persistent Remote Desktop keyboard authorization from the portal.
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

/// Revokes the stored Remote Desktop authorization token everywhere Handy tracks it.
#[cfg(target_os = "linux")]
pub fn delete_authorization() {
    clear_authorization_state();
}

/// Initializes cached authorization from persisted settings and validates it.
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

/// Sends a Ctrl+V keystroke through the Remote Desktop portal.
#[cfg(target_os = "linux")]
pub fn send_ctrl_v() -> Result<(), String> {
    if !crate::utils::is_wayland() {
        return Err("not running on Wayland".into());
    }
    if !get_authorized() {
        return Err("authorization not granted".into());
    }
    block_on_portal(|| async {
        let settings = PORTAL_APP_HANDLE.get().map(crate::settings::get_settings);
        let delay = settings
            .as_ref()
            .map(|s| s.remote_desktop_key_event_delay_ms)
            .unwrap_or(crate::settings::DEFAULT_REMOTE_DESKTOP_KEY_EVENT_DELAY_MS);

        let (proxy, session) = open_session_async(false).await?;

        // KEY_LEFTCTRL=29, KEY_V=47
        proxy.notify_keyboard_keycode(&session, 29, KeyState::Pressed).await
            .map_err(|e| format!("ctrl_v: ctrl press failed: {}", e))?;
        if delay > 0 { tokio::time::sleep(Duration::from_millis(delay)).await; }

        proxy.notify_keyboard_keycode(&session, 47, KeyState::Pressed).await
            .map_err(|e| format!("ctrl_v: v press failed: {}", e))?;
        if delay > 0 { tokio::time::sleep(Duration::from_millis(delay)).await; }

        proxy.notify_keyboard_keycode(&session, 47, KeyState::Released).await
            .map_err(|e| format!("ctrl_v: v release failed: {}", e))?;
        if delay > 0 { tokio::time::sleep(Duration::from_millis(delay)).await; }

        proxy.notify_keyboard_keycode(&session, 29, KeyState::Released).await
            .map_err(|e| format!("ctrl_v: ctrl release failed: {}", e))?;

        let _ = close_session_async(&session).await;
        Ok(())
    })
}
