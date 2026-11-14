//! macOS Secure Event Input detection, monitoring, and fallback.
//!
//! When any process enables secure event input (password fields, Terminal's
//! "Secure Keyboard Entry", a stuck `loginwindow`), CGEventTaps stop receiving
//! KeyDown/KeyUp events while FlagsChanged still flows. The handy-keys
//! implementation is tap-based, so keyed shortcuts (e.g. Option+Space) die
//! silently while modifier-only shortcuts keep working. See issue #1578.
//!
//! This module:
//! - polls `IsSecureEventInputEnabled()` and tracks state transitions
//! - looks up the holding process (best effort — Apple documents no reliable
//!   API; the IORegistry PID is frequently wrong or absent)
//! - while secure input is sustained, shadow-registers vulnerable *keyed*
//!   bindings through the Carbon-backed Tauri global-shortcut path, which is
//!   not affected by secure input (modifier-only bindings need no fallback)
//! - dynamically shadows the Cancel binding while recording, so Escape and
//!   other keyed cancellation shortcuts remain available under secure input
//! - exposes a count-only keyboard diagnostic for the debug window. Only
//!   event *kinds* are counted — key identity is never logged or returned.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Type)]
pub struct SecureInputStatus {
    /// Secure input is currently enabled (live check)
    pub enabled: bool,
    /// Enabled continuously long enough to be considered stuck (not just a
    /// password field gaining momentary focus)
    pub sustained: bool,
    pub culprit_pid: Option<i32>,
    pub culprit_name: Option<String>,
    /// Carbon fallback registrations are currently active
    pub fallback_active: bool,
    /// Binding ids shadow-registered with identical semantics
    pub covered_bindings: Vec<String>,
    /// Side-specific binding ids widened to match either side while shadowed
    pub degraded_bindings: Vec<String>,
    /// Binding ids that cannot fire at all (e.g. fn+key, registration failure)
    pub uncovered_bindings: Vec<String>,
    /// The user tried to record a shortcut while secure input was active.
    /// Treated as user impact even when every binding is covered, so the
    /// warning banner appears and explains why recording refused.
    pub recorder_blocked: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct KeyboardDiagnosticReport {
    pub secure_input_enabled: bool,
    pub culprit_pid: Option<i32>,
    pub culprit_name: Option<String>,
    /// Counts only — key identity is deliberately never captured.
    pub key_down: u32,
    pub key_up: u32,
    pub flags_changed: u32,
    pub mouse: u32,
    pub duration_ms: u32,
}

#[tauri::command]
#[specta::specta]
pub fn get_secure_input_status(app: AppHandle) -> SecureInputStatus {
    imp::status(&app)
}

#[tauri::command]
#[specta::specta]
pub async fn run_keyboard_diagnostic(
    duration_secs: Option<u32>,
) -> Result<KeyboardDiagnosticReport, String> {
    imp::run_diagnostic(duration_secs.unwrap_or(10).clamp(3, 30)).await
}

/// True if secure input is enabled right now (live check, macOS only).
pub fn is_enabled_now() -> bool {
    imp::is_enabled()
}

/// Record that a shortcut-recording attempt was refused because secure input
/// is active. Flips the warning state so the banner/tray explain the refusal
/// even when every registered binding is covered by the fallback.
pub fn note_recorder_blocked(app: &AppHandle) {
    imp::note_recorder_blocked(app)
}

/// Register/unregister the dynamic Cancel binding through the Carbon fallback
/// while a recording and sustained Secure Input overlap.
pub fn register_cancel_fallback(app: &AppHandle) {
    imp::register_cancel_fallback(app)
}

pub fn unregister_cancel_fallback(app: &AppHandle) {
    imp::unregister_cancel_fallback(app)
}

/// Synchronize Carbon fallback registrations with current settings and
/// lifecycle state while preserving unchanged registrations.
pub fn reconcile_fallback(app: &AppHandle) {
    imp::reconcile_fallback(app)
}

/// Managed state + monitor startup. On non-macOS platforms the state exists
/// but the monitor never runs and everything reports disabled.
pub fn init(app: &AppHandle) {
    app.manage(imp::SecureInputState::new());
    imp::start_monitor(app);
}

/// Whether the tray should show the warning badge / menu entry.
///
/// Only when the user is actually impacted: a binding is degraded or dead.
/// When every affected binding is covered transparently by the fallback (or
/// none are affected), the experience is seamless and nothing is shown.
pub fn tray_warning_active(app: &AppHandle) -> bool {
    app.try_state::<imp::SecureInputState>()
        .map(|s| s.warning_active())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::settings::{self, KeyboardImplementation, ShortcutBinding};
    use log::{debug, error, info, warn};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// How often the monitor thread polls.
    const POLL_INTERVAL: Duration = Duration::from_secs(1);
    /// Secure input must be held this long before we treat it as stuck.
    /// Momentary activation (a password field gaining focus) is normal.
    const SUSTAIN_THRESHOLD: Duration = Duration::from_secs(3);

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        // Carbon HIToolbox; Boolean is an unsigned char
        fn IsSecureEventInputEnabled() -> u8;
    }

    pub fn is_enabled() -> bool {
        unsafe { IsSecureEventInputEnabled() != 0 }
    }

    #[derive(Debug, Clone)]
    struct Culprit {
        pid: i32,
        name: String,
    }

    #[derive(Default)]
    struct FallbackState {
        /// Bindings shadow-registered through the Tauri/Carbon path (possibly
        /// with widened modifiers), kept so deactivation unregisters the
        /// exact strings we registered.
        registered: Vec<ShortcutBinding>,
        /// Shadowed with identical semantics
        covered: Vec<String>,
        /// Shadowed, but side-specific modifiers widened to either side
        degraded: Vec<String>,
        /// Cannot fire at all while secure input is held
        uncovered: Vec<String>,
    }

    pub struct SecureInputState {
        enabled: AtomicBool,
        sustained: AtomicBool,
        enabled_since: Mutex<Option<Instant>>,
        culprit: Mutex<Option<Culprit>>,
        fallback: Mutex<FallbackState>,
        /// Serializes fallback registration changes without requiring the
        /// fallback state lock to be held across global-shortcut plugin calls.
        fallback_operation: Mutex<()>,
        recorder_blocked: AtomicBool,
        cancel_requested: AtomicBool,
        monitor_started: AtomicBool,
    }

    impl SecureInputState {
        pub fn new() -> Self {
            Self {
                enabled: AtomicBool::new(false),
                sustained: AtomicBool::new(false),
                enabled_since: Mutex::new(None),
                culprit: Mutex::new(None),
                fallback: Mutex::new(FallbackState::default()),
                fallback_operation: Mutex::new(()),
                recorder_blocked: AtomicBool::new(false),
                cancel_requested: AtomicBool::new(false),
                monitor_started: AtomicBool::new(false),
            }
        }

        pub fn is_sustained(&self) -> bool {
            self.sustained.load(Ordering::SeqCst)
        }

        /// User-visible impact exists: some binding is degraded or dead, or
        /// the user ran into the blocked shortcut recorder.
        pub fn warning_active(&self) -> bool {
            if self.recorder_blocked.load(Ordering::SeqCst) {
                return true;
            }
            if !self.is_sustained() {
                return false;
            }
            let fallback = self.fallback.lock().unwrap();
            !fallback.degraded.is_empty() || !fallback.uncovered.is_empty()
        }
    }

    /// Best-effort culprit lookup via the IORegistry session property.
    /// Apple documents no reliable API for this; the PID may be missing
    /// (an app quit while holding secure input) or point at the wrong
    /// process (often the responsible parent, or `loginwindow`).
    fn lookup_culprit() -> Option<Culprit> {
        let out = Command::new("ioreg")
            .args(["-l", "-w", "0"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let pid: i32 = text
            .lines()
            .find_map(|l| l.split("\"kCGSSessionSecureInputPID\"=").nth(1))?
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()?;

        // `ps -o comm=` returns the full executable path; show just the
        // binary name ("Terminal", not ".../Terminal.app/Contents/MacOS/Terminal")
        let name = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| {
                let raw = String::from_utf8_lossy(&o.stdout);
                let trimmed = raw.trim();
                (!trimmed.is_empty())
                    .then(|| trimmed.rsplit('/').next().unwrap_or(trimmed).to_string())
            })
            .unwrap_or_else(|| "(process no longer running)".to_string());

        Some(Culprit { pid, name })
    }

    pub fn status(app: &AppHandle) -> SecureInputStatus {
        let enabled = is_enabled();
        let state = app.state::<SecureInputState>();

        // Culprit discovery shells out to ioreg and is intentionally performed
        // only by the monitor (or the blocking diagnostic), never by this
        // synchronous Tauri command.
        let culprit = state.culprit.lock().unwrap().clone();
        let fallback = state.fallback.lock().unwrap();
        SecureInputStatus {
            enabled,
            sustained: state.sustained.load(Ordering::SeqCst),
            culprit_pid: culprit.as_ref().map(|c| c.pid),
            culprit_name: culprit.map(|c| c.name),
            fallback_active: !fallback.registered.is_empty(),
            covered_bindings: fallback.covered.clone(),
            degraded_bindings: fallback.degraded.clone(),
            uncovered_bindings: fallback.uncovered.clone(),
            recorder_blocked: state.recorder_blocked.load(Ordering::SeqCst),
        }
    }

    pub fn note_recorder_blocked(app: &AppHandle) {
        let state = app.state::<SecureInputState>();
        if !state.recorder_blocked.swap(true, Ordering::SeqCst) {
            warn!("SecureInput: shortcut recording attempt blocked — surfacing warning");
            refresh_tray(app);
            emit_status(app);
        }
    }

    fn emit_status(app: &AppHandle) {
        let payload = status(app);
        if let Err(e) = app.emit("secure-input-changed", &payload) {
            error!("Failed to emit secure-input-changed: {e}");
        }
    }

    fn refresh_tray(app: &AppHandle) {
        // No-op before the tray is built; otherwise a diffed, coalesced,
        // main-thread apply that never blocks this thread.
        crate::tray::refresh_tray_icon(app);
    }

    pub fn start_monitor(app: &AppHandle) {
        let state = app.state::<SecureInputState>();
        if state.monitor_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let app = app.clone();
        std::thread::spawn(move || {
            info!("secure-input monitor started");
            loop {
                std::thread::sleep(POLL_INTERVAL);
                let state = app.state::<SecureInputState>();
                let now_enabled = is_enabled();
                let was_enabled = state.enabled.swap(now_enabled, Ordering::SeqCst);

                if now_enabled && !was_enabled {
                    let culprit = lookup_culprit();
                    match &culprit {
                        Some(c) => {
                            info!("SecureInput ENABLED (held by pid {} '{}')", c.pid, c.name)
                        }
                        None => info!("SecureInput ENABLED (no visible holder)"),
                    }
                    *state.enabled_since.lock().unwrap() = Some(Instant::now());
                    *state.culprit.lock().unwrap() = culprit;
                }

                if !now_enabled {
                    // Clear recorder impact on every disabled sample. A short
                    // Secure Input episode can otherwise occur entirely
                    // between polls and leave this flag latched indefinitely.
                    let was_blocked = state.recorder_blocked.swap(false, Ordering::SeqCst);
                    if was_enabled {
                        info!("SecureInput DISABLED");
                        *state.enabled_since.lock().unwrap() = None;
                        *state.culprit.lock().unwrap() = None;
                    }

                    if state.sustained.swap(false, Ordering::SeqCst) {
                        reconcile_fallback(&app);
                    } else if was_enabled || was_blocked {
                        refresh_tray(&app);
                        emit_status(&app);
                    }
                    continue;
                }

                // Promote to "sustained" after the threshold.
                if !state.sustained.load(Ordering::SeqCst) {
                    let held_long_enough = state
                        .enabled_since
                        .lock()
                        .unwrap()
                        .map(|t| t.elapsed() >= SUSTAIN_THRESHOLD)
                        .unwrap_or(false);
                    if held_long_enough {
                        warn!(
                            "SecureInput held for {}s — keyed shortcuts are blocked; activating fallback",
                            SUSTAIN_THRESHOLD.as_secs()
                        );
                        state.sustained.store(true, Ordering::SeqCst);
                        reconcile_fallback(&app);
                    }
                }
            }
        });
    }

    fn is_mouse_key(key: &handy_keys::Key) -> bool {
        key.to_string().to_lowercase().starts_with("mouse")
    }

    /// Build the Carbon-registrable equivalent of a keyed hotkey.
    ///
    /// Carbon has no concept of left/right modifiers, so side-specific
    /// modifiers widen to the whole group — returned as `degraded: true` so
    /// the UI can call out the changed matching. The fn key cannot be
    /// expressed at all (`None`).
    fn carbon_equivalent(hotkey: &handy_keys::Hotkey) -> Option<(String, bool)> {
        use handy_keys::Modifiers as M;

        if hotkey.modifiers.contains(M::FN) {
            return None;
        }

        let mut widened = M::empty();
        let mut degraded = false;
        for group in [M::CTRL, M::OPT, M::SHIFT, M::CMD] {
            if hotkey.modifiers.intersects(group) {
                widened |= group;
                if !hotkey.modifiers.contains(group) {
                    // Only one side was specified — matching gets wider
                    degraded = true;
                }
            }
        }

        let carbon_hotkey = handy_keys::Hotkey::new(widened, hotkey.key).ok()?;
        Some((carbon_hotkey.to_handy_string(), degraded))
    }

    /// Desired fallback for one binding, computed without plugin calls.
    enum ShadowPlan {
        /// Modifier-only or mouse-based; unaffected by secure input.
        Immune,
        /// Cannot be represented through Carbon.
        Uncovered,
        /// Register this shadow through Carbon.
        Shadow {
            shadow: ShortcutBinding,
            degraded: bool,
        },
    }

    fn plan_fallback_binding(id: &str, binding: &ShortcutBinding) -> ShadowPlan {
        let Ok(hotkey) = binding.current_binding.parse::<handy_keys::Hotkey>() else {
            warn!(
                "SecureInput fallback: '{}' has unparseable binding '{}', skipping",
                id, binding.current_binding
            );
            return ShadowPlan::Uncovered;
        };

        match &hotkey.key {
            None => {
                debug!(
                    "SecureInput fallback: '{}' ('{}') is modifier-only — immune, no shadow needed",
                    id, binding.current_binding
                );
                return ShadowPlan::Immune;
            }
            Some(k) if is_mouse_key(k) => {
                debug!(
                    "SecureInput fallback: '{}' ('{}') is mouse-based — immune, no shadow needed",
                    id, binding.current_binding
                );
                return ShadowPlan::Immune;
            }
            Some(_) => {}
        }

        let Some((carbon_binding, degraded)) = carbon_equivalent(&hotkey) else {
            warn!(
                "SecureInput fallback: '{}' ('{}') cannot be expressed via Carbon",
                id, binding.current_binding
            );
            return ShadowPlan::Uncovered;
        };

        let mut shadow = binding.clone();
        shadow.current_binding = carbon_binding;
        ShadowPlan::Shadow { shadow, degraded }
    }

    /// Registrations match only when the callback id and Carbon string match.
    fn same_shadow(a: &ShortcutBinding, b: &ShortcutBinding) -> bool {
        a.id == b.id && a.current_binding == b.current_binding
    }

    /// Reconcile fallback registrations without replacing unchanged shadows.
    /// The operation mutex serializes reconciliations; fallback state is
    /// unlocked around plugin calls to avoid lock-order inversion.
    ///
    /// Carbon sends a release only to the registration that received the press.
    /// Replacing a held push-to-talk registration loses its release. See #1999.
    pub fn reconcile_fallback(app: &AppHandle) {
        let state = app.state::<SecureInputState>();
        let _operation = state.fallback_operation.lock().unwrap();

        let previous = {
            let mut fallback = state.fallback.lock().unwrap();
            std::mem::take(&mut *fallback)
        };

        let settings = settings::get_settings(app);
        let eligible = state.is_sustained()
            && app
                .try_state::<crate::commands::ShortcutsInitialized>()
                .is_some()
            && settings.keyboard_implementation == KeyboardImplementation::HandyKeys;

        let mut next = FallbackState::default();
        let mut immune = 0usize;
        let mut wanted: Vec<(String, ShortcutBinding, bool)> = Vec::new();
        if eligible {
            for (id, binding) in &settings.bindings {
                if id == "cancel" && !state.cancel_requested.load(Ordering::SeqCst) {
                    continue;
                }
                if id == "transcribe_with_post_process" && !settings.post_process_enabled {
                    continue;
                }

                match plan_fallback_binding(id, binding) {
                    ShadowPlan::Immune => immune += 1,
                    ShadowPlan::Uncovered => next.uncovered.push(id.clone()),
                    ShadowPlan::Shadow { shadow, degraded } => {
                        wanted.push((id.clone(), shadow, degraded))
                    }
                }
            }
        }

        // Preserve unchanged registrations; unregister only stale shadows.
        let (kept, stale): (Vec<ShortcutBinding>, Vec<ShortcutBinding>) =
            previous.registered.into_iter().partition(|prev| {
                wanted
                    .iter()
                    .any(|(_, shadow, _)| same_shadow(shadow, prev))
            });

        if !stale.is_empty() {
            info!(
                "SecureInput fallback reconciling: removing {} Carbon shadow(s), keeping {}",
                stale.len(),
                kept.len()
            );
        }
        for binding in stale {
            if let Err(e) = crate::shortcut::tauri_impl::unregister_shortcut(app, binding.clone()) {
                warn!(
                    "SecureInput fallback: failed to unregister '{}': {}",
                    binding.current_binding, e
                );
            }
        }

        for (id, shadow, degraded) in wanted {
            if kept.iter().any(|k| same_shadow(k, &shadow)) {
                debug!(
                    "SecureInput fallback: '{}' still registered via Carbon as '{}', left untouched",
                    id, shadow.current_binding
                );
                next.registered.push(shadow);
                if degraded {
                    next.degraded.push(id);
                } else {
                    next.covered.push(id);
                }
                continue;
            }

            match crate::shortcut::tauri_impl::register_shortcut(app, shadow.clone()) {
                Ok(()) => {
                    info!(
                        "SecureInput fallback: '{}' registered via Carbon as '{}'{}",
                        id,
                        shadow.current_binding,
                        if degraded {
                            " (widened to either side)"
                        } else {
                            ""
                        }
                    );
                    next.registered.push(shadow);
                    if degraded {
                        next.degraded.push(id);
                    } else {
                        next.covered.push(id);
                    }
                }
                Err(e) => {
                    warn!(
                        "SecureInput fallback: could not cover '{}' ('{}'): {}",
                        id, shadow.current_binding, e
                    );
                    next.uncovered.push(id);
                }
            }
        }

        if eligible {
            info!(
                "SecureInput fallback active: {} covered, {} degraded, {} uncovered, {} immune (user impact: {})",
                next.covered.len(),
                next.degraded.len(),
                next.uncovered.len(),
                immune,
                !next.degraded.is_empty() || !next.uncovered.is_empty()
            );
        } else if state.is_sustained()
            && app
                .try_state::<crate::commands::ShortcutsInitialized>()
                .is_none()
        {
            debug!("SecureInput fallback deferred until shortcuts are initialized");
        }

        *state.fallback.lock().unwrap() = next;
        drop(_operation);

        // The tray sync diffs against what is displayed, so this is free when
        // the warning state did not change. Lock is released first: the sync
        // reads app state and must not nest under the operation mutex.
        refresh_tray(app);
        emit_status(app);
    }

    fn schedule_reconcile(app: &AppHandle) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            reconcile_fallback(&app);
        });
    }

    pub fn register_cancel_fallback(app: &AppHandle) {
        let state = app.state::<SecureInputState>();
        state.cancel_requested.store(true, Ordering::SeqCst);
        // Without sustained Secure Input there are no Carbon shadows to
        // update. The monitor performs reconciliation when sustained mode is
        // entered or left, so spawning here would only race the normal tray
        // state transition for every recording start.
        if state.is_sustained() {
            schedule_reconcile(app);
        }
    }

    pub fn unregister_cancel_fallback(app: &AppHandle) {
        let state = app.state::<SecureInputState>();
        state.cancel_requested.store(false, Ordering::SeqCst);
        if state.is_sustained() {
            schedule_reconcile(app);
        }
    }

    /// Count-only capture test for the debug window. Opens a short-lived
    /// keyboard listener and tallies event kinds; key identity is never
    /// inspected beyond the mouse/keyboard distinction, and nothing about
    /// individual events is logged or returned.
    pub async fn run_diagnostic(duration_secs: u32) -> Result<KeyboardDiagnosticReport, String> {
        tauri::async_runtime::spawn_blocking(move || {
            let listener = handy_keys::KeyboardListener::new()
                .map_err(|e| format!("Failed to create keyboard listener: {e}"))?;

            let enabled_at_start = is_enabled();
            let start = Instant::now();
            let deadline = start + Duration::from_secs(duration_secs as u64);
            let (mut key_down, mut key_up, mut flags_changed, mut mouse) = (0u32, 0u32, 0u32, 0u32);

            while Instant::now() < deadline {
                match listener.try_recv() {
                    Some(event) => match &event.key {
                        Some(k) if is_mouse_key(k) => mouse += 1,
                        Some(_) if event.is_key_down => key_down += 1,
                        Some(_) => key_up += 1,
                        None => flags_changed += 1,
                    },
                    None => std::thread::sleep(Duration::from_millis(10)),
                }
            }

            let enabled = enabled_at_start || is_enabled();
            let culprit = if enabled { lookup_culprit() } else { None };
            info!(
                "keyboard diagnostic: secure_input={} key_down={} key_up={} flags_changed={} mouse={}",
                enabled, key_down, key_up, flags_changed, mouse
            );

            Ok(KeyboardDiagnosticReport {
                secure_input_enabled: enabled,
                culprit_pid: culprit.as_ref().map(|c| c.pid),
                culprit_name: culprit.map(|c| c.name),
                key_down,
                key_up,
                flags_changed,
                mouse,
                duration_ms: start.elapsed().as_millis() as u32,
            })
        })
        .await
        .map_err(|e| format!("Diagnostic task failed: {e}"))?
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    pub struct SecureInputState;

    impl SecureInputState {
        pub fn new() -> Self {
            Self
        }
        pub fn warning_active(&self) -> bool {
            false
        }
    }

    pub fn is_enabled() -> bool {
        false
    }

    pub fn start_monitor(_app: &AppHandle) {}

    pub fn status(_app: &AppHandle) -> SecureInputStatus {
        SecureInputStatus {
            enabled: false,
            sustained: false,
            culprit_pid: None,
            culprit_name: None,
            fallback_active: false,
            covered_bindings: Vec::new(),
            degraded_bindings: Vec::new(),
            uncovered_bindings: Vec::new(),
            recorder_blocked: false,
        }
    }

    pub fn note_recorder_blocked(_app: &AppHandle) {}

    pub fn register_cancel_fallback(_app: &AppHandle) {}

    pub fn unregister_cancel_fallback(_app: &AppHandle) {}

    pub fn reconcile_fallback(_app: &AppHandle) {}

    pub async fn run_diagnostic(_duration_secs: u32) -> Result<KeyboardDiagnosticReport, String> {
        Err("The keyboard diagnostic is only supported on macOS".to_string())
    }
}
