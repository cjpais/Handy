//! xdg-desktop-portal GlobalShortcuts implementation
//!
//! This module provides shortcut functionality using the freedesktop
//! GlobalShortcuts portal interface, which works natively on Wayland
//! compositors that implement it (GNOME 45+, KDE Plasma 6+, etc.).
//!
//! ## Architecture
//!
//! The portal API is async (D-Bus via zbus), while Handy's shortcut
//! interface is synchronous. A background tokio task owns the portal
//! session and processes commands sent over an mpsc channel:
//!
//! ```text
//! ┌──────────────────┐   PortalCommand    ┌────────────────────────┐
//! │   Main Thread    │ ─────────────────▶ │   Portal Task (async)  │
//! │                  │   (tokio mpsc)     │                        │
//! │ - register()     │                    │ - owns Session         │
//! │ - unregister()   │ ◀───────────────── │ - calls bind_shortcuts │
//! │                  │   (std mpsc resp)  │ - listens for signals  │
//! └──────────────────┘                    └────────────────────────┘
//! ```
//!
//! The portal's `bind_shortcuts` is declarative: it replaces the full
//! set of shortcuts on each call. The task maintains the complete
//! shortcut map and rebinds everything whenever a binding changes.

use ashpd::desktop::global_shortcuts::{
    Activated, Deactivated, GlobalShortcuts, NewShortcut,
};
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::settings::{self, ShortcutBinding};

use super::handler::handle_shortcut_event;

/// Commands sent from the synchronous API to the portal background task.
enum PortalCommand {
    Register {
        binding: ShortcutBinding,
        response: std_mpsc::Sender<Result<(), String>>,
    },
    RegisterBatch {
        bindings: Vec<ShortcutBinding>,
        response: std_mpsc::Sender<Result<(), String>>,
    },
    Unregister {
        binding_id: String,
        response: std_mpsc::Sender<Result<(), String>>,
    },
    Shutdown,
}

/// Tauri-managed state for the portal shortcut backend.
pub struct PortalState {
    command_tx: mpsc::UnboundedSender<PortalCommand>,
}

impl PortalState {
    fn register_batch(&self, bindings: Vec<ShortcutBinding>) -> Result<(), String> {
        let (tx, rx) = std_mpsc::channel();
        self.command_tx
            .send(PortalCommand::RegisterBatch {
                bindings,
                response: tx,
            })
            .map_err(|_| "Portal task is not running".to_string())?;
        rx.recv()
            .map_err(|_| "Failed to receive portal response".to_string())?
    }

    fn register(&self, binding: &ShortcutBinding) -> Result<(), String> {
        let (tx, rx) = std_mpsc::channel();
        self.command_tx
            .send(PortalCommand::Register {
                binding: binding.clone(),
                response: tx,
            })
            .map_err(|_| "Portal task is not running".to_string())?;
        rx.recv()
            .map_err(|_| "Failed to receive portal response".to_string())?
    }

    fn unregister(&self, binding_id: &str) -> Result<(), String> {
        let (tx, rx) = std_mpsc::channel();
        self.command_tx
            .send(PortalCommand::Unregister {
                binding_id: binding_id.to_string(),
                response: tx,
            })
            .map_err(|_| "Portal task is not running".to_string())?;
        rx.recv()
            .map_err(|_| "Failed to receive portal response".to_string())?
    }
}

impl Drop for PortalState {
    fn drop(&mut self) {
        let _ = self.command_tx.send(PortalCommand::Shutdown);
    }
}

/// Background task that owns the portal session and processes shortcut
/// commands. Runs on the tokio runtime for the lifetime of the app.
async fn portal_task(mut cmd_rx: mpsc::UnboundedReceiver<PortalCommand>, app: AppHandle) {
    let proxy = match GlobalShortcuts::new().await {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to connect to GlobalShortcuts portal: {}", e);
            // Drain any pending commands so callers don't hang
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    PortalCommand::Register { response, .. }
                    | PortalCommand::RegisterBatch { response, .. }
                    | PortalCommand::Unregister { response, .. } => {
                        let _ = response.send(Err(format!("Portal unavailable: {}", e)));
                    }
                    PortalCommand::Shutdown => break,
                }
            }
            return;
        }
    };

    let session = match proxy.create_session(Default::default()).await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to create GlobalShortcuts session: {}", e);
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    PortalCommand::Register { response, .. }
                    | PortalCommand::RegisterBatch { response, .. }
                    | PortalCommand::Unregister { response, .. } => {
                        let _ = response.send(Err(format!("Session creation failed: {}", e)));
                    }
                    PortalCommand::Shutdown => break,
                }
            }
            return;
        }
    };

    info!("GlobalShortcuts portal session created");

    let mut activated = match proxy.receive_activated().await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to subscribe to Activated signal: {}", e);
            return;
        }
    };

    let mut deactivated = match proxy.receive_deactivated().await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to subscribe to Deactivated signal: {}", e);
            return;
        }
    };

    // The full set of currently registered shortcuts, keyed by binding id.
    let mut shortcuts: HashMap<String, ShortcutBinding> = HashMap::new();

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    PortalCommand::Register { binding, response } => {
                        debug!("Portal: registering shortcut '{}'", binding.id);
                        shortcuts.insert(binding.id.clone(), binding);
                        let result = rebind_all(&proxy, &session, &shortcuts).await;
                        let _ = response.send(result);
                    }
                    PortalCommand::RegisterBatch { bindings, response } => {
                        debug!("Portal: registering {} shortcuts at once", bindings.len());
                        for binding in bindings {
                            shortcuts.insert(binding.id.clone(), binding);
                        }
                        let result = rebind_all(&proxy, &session, &shortcuts).await;
                        let _ = response.send(result);
                    }
                    PortalCommand::Unregister { binding_id, response } => {
                        debug!("Portal: unregistering shortcut '{}'", binding_id);
                        shortcuts.remove(&binding_id);
                        let result = rebind_all(&proxy, &session, &shortcuts).await;
                        let _ = response.send(result);
                    }
                    PortalCommand::Shutdown => {
                        info!("Portal shortcut task shutting down");
                        break;
                    }
                }
            }
            Some(signal) = activated.next() => {
                let signal: Activated = signal;
                let id = signal.shortcut_id();
                if let Some(binding) = shortcuts.get(id) {
                    debug!("Portal shortcut activated: {}", id);
                    handle_shortcut_event(&app, &binding.id, &binding.current_binding, true);
                } else {
                    warn!("Activated signal for unknown shortcut '{}'", id);
                }
            }
            Some(signal) = deactivated.next() => {
                let signal: Deactivated = signal;
                let id = signal.shortcut_id();
                if let Some(binding) = shortcuts.get(id) {
                    debug!("Portal shortcut deactivated: {}", id);
                    handle_shortcut_event(&app, &binding.id, &binding.current_binding, false);
                }
            }
            else => {
                warn!("Portal event streams closed unexpectedly");
                break;
            }
        }
    }

    debug!("Portal shortcut task stopped");
}

/// Rebind the full set of shortcuts via the portal. The portal API is
/// declarative: each `bind_shortcuts` call replaces the previous set.
async fn rebind_all(
    proxy: &GlobalShortcuts,
    session: &ashpd::desktop::Session<GlobalShortcuts>,
    shortcuts: &HashMap<String, ShortcutBinding>,
) -> Result<(), String> {
    let portal_shortcuts: Vec<NewShortcut> = shortcuts
        .values()
        .map(|b| {
            let trigger = to_portal_trigger(&b.current_binding);
            NewShortcut::new(&b.id, &b.id).preferred_trigger(Some(trigger.as_str()))
        })
        .collect();

    if portal_shortcuts.is_empty() {
        debug!("No shortcuts to bind");
        return Ok(());
    }

    let request = proxy
        .bind_shortcuts(session, &portal_shortcuts, None, Default::default())
        .await
        .map_err(|e| format!("bind_shortcuts failed: {}", e))?;

    match request.response() {
        Ok(bound) => {
            debug!(
                "Portal bound {} shortcut(s): {:?}",
                bound.shortcuts().len(),
                bound
                    .shortcuts()
                    .iter()
                    .map(|s| s.id().to_string())
                    .collect::<Vec<_>>()
            );
        }
        Err(e) => {
            return Err(format!("bind_shortcuts response: {}", e));
        }
    }

    Ok(())
}

/// Convert Handy's shortcut format to the portal trigger format.
///
/// Handy uses lowercase "ctrl+space", "alt+shift+a", etc. KDE's portal
/// expects capitalised modifier names (Ctrl, Shift, Alt, Meta) and a
/// capitalised key name. Passing lowercase "ctrl" causes KDE to log
/// "Unknown modifier" and silently ignore the preferred trigger.
fn to_portal_trigger(handy_format: &str) -> String {
    handy_format
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" | "option" => "Alt".to_string(),
            "super" | "meta" | "command" | "cmd" => "Meta".to_string(),
            other => {
                // Capitalise the first letter of key names (space -> Space, etc.)
                let mut chars = other.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

// ============================================================================
// Public API (matches the interface expected by shortcut/mod.rs)
// ============================================================================

/// Initialize shortcuts using the GlobalShortcuts portal.
pub fn init_shortcuts(app: &AppHandle) -> Result<(), String> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let state = PortalState { command_tx: cmd_tx };

    // Spawn the background task that manages the portal session
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        portal_task(cmd_rx, app_clone).await;
    });

    // Give the async task a moment to establish the D-Bus connection.
    // Without this the first register call can race ahead of session
    // creation. 200ms is more than enough for local D-Bus.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let default_bindings = settings::get_default_settings().bindings;
    let user_settings = settings::load_or_create_app_settings(app);

    // Collect all bindings and register them in a single bind_shortcuts
    // call. The portal API is declarative -- each call replaces the full
    // set, so individual registration would cause earlier shortcuts to be
    // overwritten. KDE shows one confirmation dialog for all new shortcuts
    // on first launch, then remembers them for subsequent launches (as
    // long as the app_id is stable).
    let mut initial_bindings = Vec::new();
    for (id, default_binding) in default_bindings {
        if id == "transcribe_with_post_process" && !user_settings.post_process_enabled {
            continue;
        }

        let binding = user_settings
            .bindings
            .get(&id)
            .cloned()
            .unwrap_or(default_binding);

        initial_bindings.push(binding);
    }

    if let Err(e) = state.register_batch(initial_bindings) {
        error!("Failed to register portal shortcuts during init: {}", e);
    }

    app.manage(state);
    info!("Portal shortcuts initialized");
    Ok(())
}

/// Register a shortcut via the portal.
pub fn register_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<PortalState>()
        .ok_or("PortalState not initialized")?;
    state.register(&binding)
}

/// Unregister a shortcut from the portal.
pub fn unregister_shortcut(app: &AppHandle, binding: ShortcutBinding) -> Result<(), String> {
    let state = app
        .try_state::<PortalState>()
        .ok_or("PortalState not initialized")?;
    state.unregister(&binding.id)
}

/// Register the cancel shortcut (no-op for portal).
///
/// The portal backend registers cancel at init along with all other
/// shortcuts. Dynamic registration is avoided because each
/// bind_shortcuts call triggers a compositor confirmation dialog.
pub fn register_cancel_shortcut(_app: &AppHandle) {}

/// Unregister the cancel shortcut (no-op for portal).
pub fn unregister_cancel_shortcut(_app: &AppHandle) {}

/// Validate a shortcut string for the portal backend.
///
/// The portal is lenient: the trigger string is treated as a preferred
/// hint and the compositor decides what to actually assign. We just
/// reject obviously empty input.
pub fn validate_shortcut(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    Ok(())
}
