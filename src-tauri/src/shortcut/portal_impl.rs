//! xdg-desktop-portal GlobalShortcuts implementation
//!
//! This module provides shortcut functionality using the freedesktop
//! GlobalShortcuts portal interface, which works natively on Wayland
//! compositors that implement it (KDE Plasma 5.27+, GNOME 48+, Hyprland, etc.).
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────┐   PortalCommand      ┌────────────────────────┐
//! │   Main Thread    │ ────────────────────▶ │   Event Loop Thread    │
//! │   (PortalState)  │   (crossbeam)        │                        │
//! │                  │ ◀──────────────────── │   - owns tokio runtime │
//! │ - register()     │   (crossbeam resp)   │   - Select multiplexes │
//! │ - unregister()   │                      │     cmd + portal events│
//! └──────────────────┘                      │   - restart recovery   │
//!                                           └────────────────────────┘
//! ```
//!
//! The portal's `bind_shortcuts` is declarative: each call replaces the
//! previous set. We maintain the full shortcut map and rebind on every change.
//!
//! ## Portal restart recovery
//!
//! When the D-Bus signal stream ends (portal restart, compositor crash),
//! we detect it, wait for the portal to become available again, recreate
//! the session and rebind all hotkeys automatically.
//!
//! ## X11 fallback
//!
//! If the GlobalShortcuts portal is unavailable at startup (older
//! compositors), the caller in `shortcut/mod.rs` falls back to the Tauri
//! backend automatically.

use ashpd::desktop::global_shortcuts::{Activated, Deactivated, GlobalShortcuts, NewShortcut};
use ashpd::desktop::Session;
use ashpd::AppID;
use crossbeam_channel::{unbounded, Receiver, Select, Sender};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::str::FromStr;
use tauri::{AppHandle, Manager};

use crate::settings;

use super::handler::handle_shortcut_event;

// ============================================================================
// Commands & Events
// ============================================================================

enum PortalCommand {
    Register {
        binding: settings::ShortcutBinding,
        response: Sender<Result<(), String>>,
    },
    RegisterBatch {
        bindings: Vec<settings::ShortcutBinding>,
        response: Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: Sender<Result<(), String>>,
    },
    TriggerDescription {
        binding_id: String,
        response: Sender<Option<String>>,
    },
    DropThread,
}

enum GSEvent {
    Activated(Activated),
    Deactivated(Deactivated),
    /// The D-Bus signal stream ended (e.g. the portal restarted).
    StreamEnded,
}

// ============================================================================
// Portal Session State
// ============================================================================

struct GlobalShortcutsState {
    proxy: GlobalShortcuts,
    session: Session<GlobalShortcuts>,
}

impl GlobalShortcutsState {
    async fn new(app_id: &str, event_sender: Sender<GSEvent>) -> Result<Self, String> {
        if let Ok(app_id) = AppID::from_str(app_id) {
            if let Err(e) = ashpd::register_host_app(app_id).await {
                debug!("Failed to register app id: {:?}", e);
            }
        }

        let proxy = GlobalShortcuts::new()
            .await
            .map_err(|e| format!("Failed to start global shortcuts portal proxy: {e}"))?;

        let session = proxy
            .create_session(Default::default())
            .await
            .map_err(|e| format!("Failed to start global shortcuts portal session: {e}"))?;

        let activated = proxy
            .receive_activated()
            .await
            .map_err(|e| format!("Failed to receive portal activated stream: {e}"))?;

        let deactivated = proxy
            .receive_deactivated()
            .await
            .map_err(|e| format!("Failed to receive portal deactivated stream: {e}"))?;

        tokio::spawn({
            let sender = event_sender.clone();
            async move {
                use futures_util::StreamExt;
                let mut stream =
                    futures_util::stream::select(activated.map(GSEvent::Activated), deactivated.map(GSEvent::Deactivated));
                while let Some(event) = stream.next().await {
                    if sender.send(event).is_err() {
                        break;
                    }
                }
                let _ = sender.send(GSEvent::StreamEnded);
            }
        });

        Ok(Self { proxy, session })
    }

    async fn close_session(&mut self) {
        if let Err(e) = self.session.close().await {
            debug!("Failed to close old global shortcuts session: {e}");
        }
    }
}

// ============================================================================
// Rebind all shortcuts via the portal
// ============================================================================

async fn rebind_all(
    state: &mut GlobalShortcutsState,
    shortcuts: &HashMap<String, settings::ShortcutBinding>,
) -> Result<(), String> {
    state.close_session().await;

    state.session = state.proxy
        .create_session(Default::default())
        .await
        .map_err(|e| format!("Failed to create portal session: {e}"))?;

    let portal_shortcuts: Vec<NewShortcut> = shortcuts
        .values()
        .map(|b| {
            NewShortcut::new(&b.id, &b.id)
                .preferred_trigger(to_portal_trigger(&b.current_binding).as_deref())
        })
        .collect();

    if portal_shortcuts.is_empty() {
        return Ok(());
    }

    // Not handling error from BindShortcuts due to GNOME 48 bug (fixed in GNOME 49):
    // https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/issues/177
    let _ = state.proxy
        .bind_shortcuts(&state.session, &portal_shortcuts, None, Default::default())
        .await
        .map(|r| r.response());

    Ok(())
}

// ============================================================================
// Trigger → keysym mapping
// ============================================================================

fn to_portal_trigger(handy_format: &str) -> Option<String> {
    let parts: Vec<&str> = handy_format.split('+').map(|s| s.trim()).collect();
    if parts.is_empty() || parts.iter().all(|p| p.is_empty()) {
        return None;
    }

    let mut mods = String::new();
    let mut key = String::new();
    let mut has_shift = false;

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => mods += "CTRL+",
            "shift" => { mods += "SHIFT+"; has_shift = true; }
            "alt" | "option" => mods += "ALT+",
            "super" | "meta" | "command" | "cmd" => mods += "LOGO+",
            other => key = key_to_keysym(other).to_string(),
        }
    }

    if key.is_empty() {
        return None;
    }

    // If shift is held and the key changes characters when shifted (digits,
    // punctuation), the literal trigger cannot fire. Let the compositor prompt.
    if has_shift && shift_changes_keysym(&key) {
        return None;
    }

    Some(mods + &key)
}

/// Keys whose keysym changes with SHIFT depending on keyboard layout.
fn shift_changes_keysym(key: &str) -> bool {
    matches!(
        key,
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
            | "grave" | "minus" | "equal"
            | "bracketleft" | "bracketright" | "backslash"
            | "semicolon" | "apostrophe"
            | "comma" | "period" | "slash"
    )
}

fn key_to_keysym<'a>(key: &'a str) -> &'a str {
    match key {
        "a" => "a",
        "b" => "b",
        "c" => "c",
        "d" => "d",
        "e" => "e",
        "f" => "f",
        "g" => "g",
        "h" => "h",
        "i" => "i",
        "j" => "j",
        "k" => "k",
        "l" => "l",
        "m" => "m",
        "n" => "n",
        "o" => "o",
        "p" => "p",
        "q" => "q",
        "r" => "r",
        "s" => "s",
        "t" => "t",
        "u" => "u",
        "v" => "v",
        "w" => "w",
        "x" => "x",
        "y" => "y",
        "z" => "z",
        "space" => "space",
        "escape" => "Escape",
        "esc" => "Escape",
        "return" | "enter" => "Return",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "page_up" => "Page_Up",
        "pagedown" | "page_down" => "Page_Down",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        "capslock" | "caps_lock" => "Caps_Lock",
        "numlock" | "num_lock" => "Num_Lock",
        "scrolllock" | "scroll_lock" => "Scroll_Lock",
        "printscreen" | "print" => "Print",
        "pause" => "Pause",
        "f1" => "F1",
        "f2" => "F2",
        "f3" => "F3",
        "f4" => "F4",
        "f5" => "F5",
        "f6" => "F6",
        "f7" => "F7",
        "f8" => "F8",
        "f9" => "F9",
        "f10" => "F10",
        "f11" => "F11",
        "f12" => "F12",
        "f13" => "F13",
        "f14" => "F14",
        "f15" => "F15",
        "f16" => "F16",
        "f17" => "F17",
        "f18" => "F18",
        "f19" => "F19",
        "f20" => "F20",
        "f21" => "F21",
        "f22" => "F22",
        "f23" => "F23",
        "f24" => "F24",
        "0" => "0",
        "1" => "1",
        "2" => "2",
        "3" => "3",
        "4" => "4",
        "5" => "5",
        "6" => "6",
        "7" => "7",
        "8" => "8",
        "9" => "9",
        "`" | "grave" => "grave",
        "-" | "minus" => "minus",
        "=" | "equal" => "equal",
        "[" | "bracketleft" => "bracketleft",
        "]" | "bracketright" => "bracketright",
        "\\" | "backslash" => "backslash",
        ";" | "semicolon" => "semicolon",
        "'" | "apostrophe" | "quote" => "apostrophe",
        "," | "comma" => "comma",
        "." | "period" => "period",
        "/" | "slash" => "slash",
        // Numpad
        "kp0" | "numpad0" => "KP_0",
        "kp1" | "numpad1" => "KP_1",
        "kp2" | "numpad2" => "KP_2",
        "kp3" | "numpad3" => "KP_3",
        "kp4" | "numpad4" => "KP_4",
        "kp5" | "numpad5" => "KP_5",
        "kp6" | "numpad6" => "KP_6",
        "kp7" | "numpad7" => "KP_7",
        "kp8" | "numpad8" => "KP_8",
        "kp9" | "numpad9" => "KP_9",
        "kpadd" | "numpadadd" => "KP_Add",
        "kpdecimal" | "numpaddecimal" => "KP_Decimal",
        "kpdivide" | "numpaddivide" => "KP_Divide",
        "kpmultiply" | "numpadmultiply" => "KP_Multiply",
        "kpsubtract" | "numpadsubtract" => "KP_Subtract",
        // XF86 media keys
        "audiovolumedown" | "volumedown" => "XF86AudioLowerVolume",
        "audiovolumemute" | "volumemute" | "mute" => "XF86AudioMute",
        "audiovolumeup" | "volumeup" => "XF86AudioRaiseVolume",
        "mediaplay" | "audioplay" => "XF86AudioPlay",
        "mediapause" | "audiopause" => "XF86AudioPause",
        "mediastop" | "audiostop" => "XF86AudioStop",
        "medianext" | "audionext" => "XF86AudioNext",
        "mediaprev" | "audioprev" => "XF86AudioPrev",
        "mediaselect" => "XF86AudioMedia",
        "browserback" => "XF86Back",
        "browserforward" => "XF86Forward",
        "browserhome" => "XF86HomePage",
        "browserrefresh" => "XF86Refresh",
        "browsersearch" => "XF86Search",
        "browserstop" => "XF86Stop",
        "browserfavorites" => "XF86Favorites",
        "launchmail" => "XF86Mail",
        // Default: use the key as-is (already a keysym name)
        other => other,
    }
}

// ============================================================================
// App ID resolution
// ============================================================================

fn resolve_app_id() -> String {
    std::env::var("FLATPAK_ID")
        .or_else(|_| std::env::var("GLOBAL_HOTKEY_APP_ID"))
        .unwrap_or_else(|_| "com.pais.handy".to_string())
}

// ============================================================================
// Event Loop (runs on background thread)
// ============================================================================

fn event_loop_thread(
    cmd_rx: Receiver<PortalCommand>,
    app: AppHandle,
) -> Result<(), String> {
    let mut registered = HashMap::<String, settings::ShortcutBinding>::new();
    let mut key_pressed = HashMap::<String, bool>::new();

    let (gs_event_tx, gs_event_rx) = unbounded();

    let app_id = resolve_app_id();

    // Create a dedicated tokio runtime for portal D-Bus calls.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create tokio runtime: {e}"))?;

    let mut gs_state = rt.block_on(GlobalShortcutsState::new(&app_id, gs_event_tx.clone()))?;

    let mut select = Select::new();
    let cmd_idx = select.recv(&cmd_rx);
    let evt_idx = select.recv(&gs_event_rx);

    loop {
        let oper = select.select();
        match oper.index() {
            i if i == cmd_idx => match oper.recv(&cmd_rx) {
                Ok(PortalCommand::Register { binding, response }) => {
                    let id = binding.id.clone();
                    let is_new = !registered.contains_key(&id);
                    registered.insert(id.clone(), binding);
                    let result = rt.block_on(rebind_all(&mut gs_state, &registered));
                    if result.is_err() && is_new {
                        registered.remove(&id);
                    }
                    let _ = response.send(result);
                }
                Ok(PortalCommand::RegisterBatch { bindings, response }) => {
                    let mut new_ids = Vec::new();
                    for b in &bindings {
                        if !registered.contains_key(&b.id) {
                            new_ids.push(b.id.clone());
                        }
                        registered.insert(b.id.clone(), b.clone());
                    }
                    let result = rt.block_on(rebind_all(&mut gs_state, &registered));
                    if result.is_err() {
                        for id in new_ids {
                            registered.remove(&id);
                        }
                    }
                    let _ = response.send(result);
                }
                Ok(PortalCommand::Unregister { binding_id, response }) => {
                    registered.remove(&binding_id);
                    key_pressed.remove(&binding_id);
                    let result = rt.block_on(rebind_all(&mut gs_state, &registered));
                    let _ = response.send(result);
                }
                Ok(PortalCommand::TriggerDescription { binding_id, response }) => {
                    let desc = rt.block_on(trigger_description(&gs_state, &binding_id));
                    let _ = response.send(desc);
                }
                Ok(PortalCommand::DropThread) => {
                    info!("Portal shortcut event loop shutting down");
                    return Ok(());
                }
                Err(_) => {
                    info!("Command channel closed, shutting down portal event loop");
                    return Ok(());
                }
            },
            i if i == evt_idx => match oper.recv(&gs_event_rx) {
                Ok(GSEvent::Activated(activated)) => {
                    let id = activated.shortcut_id().to_string();
                    if registered.contains_key(&id) {
                        let already = *key_pressed.get(&id).unwrap_or(&false);
                        if !already {
                            key_pressed.insert(id.clone(), true);
                            let binding = registered.get(&id).unwrap();
                            handle_shortcut_event(&app, &binding.id, &binding.current_binding, true);
                        }
                    } else {
                        debug!("Portal activated for unknown shortcut '{}'", id);
                    }
                }
                Ok(GSEvent::Deactivated(deactivated)) => {
                    let id = deactivated.shortcut_id().to_string();
                    if registered.contains_key(&id) {
                        key_pressed.insert(id.clone(), false);
                        let binding = registered.get(&id).unwrap();
                        handle_shortcut_event(&app, &binding.id, &binding.current_binding, false);
                    }
                }
                Ok(GSEvent::StreamEnded) => {
                    warn!("Portal event stream ended, attempting reconnect...");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let _gs_event_tx = gs_event_tx.clone();
                    match rt.block_on(GlobalShortcutsState::new(&app_id, _gs_event_tx)) {
                        Ok(gs) => {
                            gs_state = gs;
                            if let Err(e) = rt.block_on(rebind_all(&mut gs_state, &registered)) {
                                error!("Failed to rebind after portal restart: {}", e);
                            } else {
                                info!("Portal session recreated and shortcuts rebound");
                            }
                        }
                        Err(e) => {
                            error!("Failed to reconnect to portal: {}", e);
                        }
                    }
                }
                Err(_) => {
                    info!("Portal event channel closed, shutting down");
                    return Ok(());
                }
            },
            _ => unreachable!(),
        }
    }
}

// ============================================================================
// Trigger description query via ListShortcuts
// ============================================================================

async fn trigger_description(
    state: &GlobalShortcutsState,
    binding_id: &str,
) -> Option<String> {
    let response = state.proxy
        .list_shortcuts(&state.session, Default::default())
        .await
        .ok()
        .and_then(|r| r.response().ok())?;

    response
        .shortcuts()
        .iter()
        .find(|s| s.id() == binding_id)
        .map(|s| s.trigger_description().to_string())
        .filter(|d| !d.is_empty())
}

// ============================================================================
// Public API (compatible with shortcut/mod.rs)
// ============================================================================

pub struct PortalState {
    command_tx: Sender<PortalCommand>,
}

impl PortalState {
    fn send(&self, cmd: PortalCommand) -> Result<(), String> {
        self.command_tx.send(cmd)
            .map_err(|_| "Portal task is not running".to_string())
    }

    fn send_with_reply<T>(&self, build_cmd: impl FnOnce(Sender<T>) -> PortalCommand) -> Result<T, String> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(build_cmd(tx))?;
        rx.recv().map_err(|_| "Portal task is not running".to_string())
    }

    fn register(&self, binding: &settings::ShortcutBinding) -> Result<(), String> {
        self.send_with_reply(|tx| PortalCommand::Register {
            binding: binding.clone(),
            response: tx,
        })?
    }

    fn unregister(&self, binding_id: &str) -> Result<(), String> {
        self.send_with_reply(|tx| PortalCommand::Unregister {
            binding_id: binding_id.to_string(),
            response: tx,
        })?
    }

    fn register_batch(&self, bindings: Vec<settings::ShortcutBinding>) -> Result<(), String> {
        self.send_with_reply(|tx| PortalCommand::RegisterBatch {
            bindings,
            response: tx,
        })?
    }

    pub fn trigger_description(&self, binding_id: &str) -> Option<String> {
        self.send_with_reply(|tx| PortalCommand::TriggerDescription {
            binding_id: binding_id.to_string(),
            response: tx,
        }).ok().flatten()
    }
}

impl Drop for PortalState {
    fn drop(&mut self) {
        let _ = self.command_tx.send(PortalCommand::DropThread);
    }
}

pub fn init_shortcuts(app: &AppHandle) -> Result<(), String> {
    let (cmd_tx, cmd_rx) = unbounded();

    let app_clone = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = event_loop_thread(cmd_rx, app_clone) {
            error!("Portal event loop exited with error: {}", e);
        }
    });

    // Brief wait for D-Bus connection to establish before first register call.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    let mut initial = Vec::new();
    for (id, default_binding) in default_bindings {
        if id == "transcribe_with_post_process" && !user_settings.post_process_enabled {
            continue;
        }
        let binding = user_settings.bindings.get(&id).cloned().unwrap_or(default_binding);
        initial.push(binding);
    }

    let state = PortalState { command_tx: cmd_tx };

    if let Err(e) = state.register_batch(initial) {
        error!("Failed to register portal shortcuts during init: {}", e);
    }

    app.manage(state);
    info!("Portal shortcuts initialized");
    Ok(())
}

pub fn register_shortcut(app: &AppHandle, binding: settings::ShortcutBinding) -> Result<(), String> {
    let state = app.try_state::<PortalState>()
        .ok_or("PortalState not initialized")?;
    state.register(&binding)
}

pub fn unregister_shortcut(app: &AppHandle, binding: settings::ShortcutBinding) -> Result<(), String> {
    let state = app.try_state::<PortalState>()
        .ok_or("PortalState not initialized")?;
    state.unregister(&binding.id)
}

pub fn register_cancel_shortcut(_app: &AppHandle) {}

pub fn unregister_cancel_shortcut(_app: &AppHandle) {}

pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    Ok(())
}
