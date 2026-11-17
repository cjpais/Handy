//! System tray icon and menu.
//!
//! The tray is driven by a single *desired state* snapshot ([`TrayDesired`])
//! that callers update through [`set_tray_state`], [`refresh_tray_icon`] and
//! [`update_tray_menu`]. Every such call just records intent and schedules a
//! single applier on the main thread, which diffs the desired snapshot against
//! what is currently displayed and touches the native tray only for the parts
//! that actually changed. Requests that arrive while an apply is pending are
//! coalesced into it, so bursts of state changes never queue up native work.
//!
//! Why: native tray updates are the lever we control for the macOS tray
//! disappearance bug (tauri-apps/tauri#12060, Handy #1948). Before this, every
//! recording cycle rebuilt the full menu 3-6 times from several threads, and
//! concurrent rebuilds could interleave and leave a stale menu behind.
//!
//! Exception: [`set_tray_visibility`] and [`recreate_tray_icon`] call the tray
//! directly. Visibility is a separate attribute that never participates in the
//! icon/menu diff, both are rare and user-initiated, and Tauri marshals them
//! onto the main thread so they serialize with the applier anyway. Re-showing
//! a hidden tray relies on tray-icon recreating it from the last applied
//! icon/menu/tooltip, so those must only ever be set through the applier.

use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::model::{ModelInfo, ModelManager};
use crate::managers::transcription::TranscriptionManager;
use crate::notices::{self, NoticeCode};
use crate::settings;
use crate::tray_i18n::get_tray_translations;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIcon;
use tauri::{AppHandle, Manager, Theme};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayIconState {
    Idle,
    Recording,
    Transcribing,
}

impl TrayIconState {
    /// Recording and Transcribing share the same menu ("Cancel" instead of the
    /// model submenu), so only the idle/busy distinction matters for the menu.
    fn is_busy(self) -> bool {
        self != TrayIconState::Idle
    }
}

/// Everything the tray *menu* (and tooltip) depends on. When two snapshots
/// compare equal the menu is not rebuilt.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuInputs {
    busy: bool,
    /// A background transcription holds the model, so it can't be switched
    /// or unloaded.
    models_locked: bool,
    /// macOS Secure Input is blocking keyboard shortcuts.
    secure_input: bool,
    /// The notice to show as a menu line.
    notice: Option<NoticeCode>,
    model_loaded: bool,
    selected_model: String,
    /// `(id, name)` of downloaded models, sorted by name.
    downloaded_models: Vec<(String, String)>,
    locale: String,
    update_checks_enabled: bool,
    /// The language submenu, when the selected model has languages to pick.
    language: Option<LanguageMenu>,
}

impl MenuInputs {
    /// Fields that require replacing the menu's labels or item tree. Recording
    /// state, lock/load state, update availability, and the optional warning
    /// lines are applied to the live menu, so they do not call set_menu().
    fn same_structure(&self, other: &Self) -> bool {
        self.selected_model == other.selected_model
            && self.downloaded_models == other.downloaded_models
            && self.locale == other.locale
            && self.language == other.language
    }
}

/// What the language submenu shows: `(code, name)` entries, with codes as
/// language identities, the spelling `selected_language` is saved in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LanguageMenu {
    /// The checked entry: `auto` or a code.
    active: String,
    /// Auto works for this model and these languages. When it doesn't, or
    /// needs the detector download, it's shown greyed out, as in settings.
    auto_enabled: bool,
    /// The person's spoken languages the model hears, in their order, plus
    /// the active language when it isn't one of them.
    main: Vec<(String, String)>,
    /// The model's other languages, by name, in an "Other Languages" submenu.
    others: Vec<(String, String)>,
}

/// Complete description of what the tray should look like.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrayDesired {
    icon_path: &'static str,
    menu: MenuInputs,
}

struct TrayInner {
    /// Intent set by [`set_tray_state`].
    icon_state: TrayIconState,
    /// Latest computed snapshot, waiting to be (or just) applied.
    desired: Option<TrayDesired>,
    /// Icon the native tray currently shows. Only updated when `set_icon`
    /// succeeds, so a failed update is retried on the next sync.
    applied_icon: Option<&'static str>,
    /// Inputs the native menu was last successfully built from. The tooltip
    /// is derived from the same inputs and set best-effort alongside the menu;
    /// it is not tracked separately.
    applied_menu: Option<MenuInputs>,
    /// An apply is scheduled on the main thread.
    pending: bool,
    /// Handles for updating the current menu in place. Replacing the menu on
    /// every recording-state change re-registers the StatusNotifierItem on
    /// Linux, which makes GNOME reread the previous (already deleted) icon.
    menu_controls: Option<MenuControls>,
    /// Decoded icons by resource path so the main thread never touches disk.
    icons: HashMap<&'static str, Image<'static>>,
    /// Handed out to each sync request in trigger order, so a slow request
    /// can't overwrite the snapshot of one that was triggered after it.
    next_seq: u64,
    /// Sequence number of the request that produced `desired`.
    desired_seq: u64,
}

/// Tauri managed state owning the tray's desired/applied snapshots.
pub struct TrayState(Mutex<TrayInner>);

impl TrayState {
    pub fn new() -> Self {
        Self(Mutex::new(TrayInner {
            icon_state: TrayIconState::Idle,
            desired: None,
            applied_icon: None,
            applied_menu: None,
            pending: false,
            menu_controls: None,
            icons: HashMap::new(),
            next_seq: 0,
            desired_seq: 0,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, TrayInner> {
        self.0.lock().unwrap_or_else(|poisoned| {
            warn!("Tray state mutex was poisoned, recovering");
            poisoned.into_inner()
        })
    }
}

/// Mutable handles for the state-dependent part of the tray menu. The menu is
/// still rebuilt when its labels or contents change, but idle/busy transitions
/// only move the relevant items in this already-registered menu.
#[derive(Clone)]
struct MenuControls {
    menu: Menu<tauri::Wry>,
    secure_input_warning: MenuItem<tauri::Wry>,
    notice: MenuItem<tauri::Wry>,
    warning_separator: PredefinedMenuItem<tauri::Wry>,
    cancel: MenuItem<tauri::Wry>,
    copy_last_transcript: MenuItem<tauri::Wry>,
    copy_separator: PredefinedMenuItem<tauri::Wry>,
    model_submenu: Submenu<tauri::Wry>,
    language_submenu: Option<Submenu<tauri::Wry>>,
    unload_model: MenuItem<tauri::Wry>,
    action_separator: PredefinedMenuItem<tauri::Wry>,
    content_start: usize,
    busy: bool,
    secure_input: bool,
    notice_code: Option<NoticeCode>,
    check_updates: MenuItem<tauri::Wry>,
}

impl MenuControls {
    fn update(&mut self, inputs: &MenuInputs) -> tauri::Result<Option<String>> {
        let busy_changed = self.busy != inputs.busy;
        let secure_input_changed = self.secure_input != inputs.secure_input;
        let warning_layout_changed =
            secure_input_changed || self.notice_code.is_some() != inputs.notice.is_some();

        let strings = get_tray_translations(Some(inputs.locale.clone()));
        let secure_input_label = if strings.secure_input_warning.is_empty() {
            get_tray_translations(Some("en".to_string())).secure_input_warning
        } else {
            strings.secure_input_warning.clone()
        };
        self.secure_input_warning.set_text(&secure_input_label)?;
        self.notice.set_text(
            inputs
                .notice
                .map(|code| code.tray_title(&strings))
                .unwrap_or_default(),
        )?;
        self.notice.set_enabled(inputs.notice.is_some())?;

        if busy_changed {
            // Remove the old content and its trailing separator before adding
            // the new content. These mutations are sent over DBusMenu without
            // calling TrayIcon::set_menu(), so the SNI item is not registered
            // a second time.
            self.menu.remove(&self.action_separator)?;
            if self.busy {
                self.menu.remove(&self.cancel)?;
                self.menu.remove(&self.copy_separator)?;
                self.menu.remove(&self.copy_last_transcript)?;
            } else {
                self.menu.remove(&self.copy_last_transcript)?;
                self.menu.remove(&self.copy_separator)?;
                self.menu.remove(&self.model_submenu)?;
                if let Some(language) = &self.language_submenu {
                    self.menu.remove(language)?;
                }
                self.menu.remove(&self.unload_model)?;
            }
        }

        if warning_layout_changed {
            if self.secure_input {
                self.menu.remove(&self.secure_input_warning)?;
            }
            if self.notice_code.is_some() {
                self.menu.remove(&self.notice)?;
            }
            if self.secure_input || self.notice_code.is_some() {
                self.menu.remove(&self.warning_separator)?;
            }

            let mut position = 2;
            if inputs.secure_input {
                self.menu.insert(&self.secure_input_warning, position)?;
                position += 1;
            }
            if inputs.notice.is_some() {
                self.menu.insert(&self.notice, position)?;
                position += 1;
            }
            if inputs.secure_input || inputs.notice.is_some() {
                self.menu.insert(&self.warning_separator, position)?;
                position += 1;
            }
            self.content_start = position;
        }
        self.secure_input = inputs.secure_input;
        self.notice_code = inputs.notice;

        if busy_changed {
            if inputs.busy {
                self.menu.insert(&self.cancel, self.content_start)?;
                self.menu
                    .insert(&self.copy_separator, self.content_start + 1)?;
                self.menu
                    .insert(&self.copy_last_transcript, self.content_start + 2)?;
                self.menu
                    .insert(&self.action_separator, self.content_start + 3)?;
            } else {
                let mut position = self.content_start;
                self.menu.insert(&self.copy_last_transcript, position)?;
                position += 1;
                self.menu.insert(&self.copy_separator, position)?;
                position += 1;
                self.menu.insert(&self.model_submenu, position)?;
                position += 1;
                if let Some(language) = &self.language_submenu {
                    self.menu.insert(language, position)?;
                    position += 1;
                }
                self.menu.insert(&self.unload_model, position)?;
                self.menu.insert(&self.action_separator, position + 1)?;
            }
            self.busy = inputs.busy;
        }

        self.cancel.set_enabled(inputs.busy)?;
        self.model_submenu
            .set_enabled(!inputs.busy && !inputs.models_locked)?;
        self.unload_model
            .set_enabled(inputs.model_loaded && !inputs.busy && !inputs.models_locked)?;
        self.check_updates
            .set_enabled(inputs.update_checks_enabled)?;

        let tooltip = if secure_input_changed {
            Some(if inputs.secure_input {
                format!("{} — {}", version_label(), secure_input_label)
            } else {
                version_label()
            })
        } else {
            None
        };
        Ok(tooltip)
    }
}

impl Default for TrayState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppTheme {
    Dark,
    Light,
    Colored, // Pink/colored theme for Linux
}

/// Gets the current app theme, with Linux defaulting to Colored theme
pub fn get_current_theme(app: &AppHandle) -> AppTheme {
    if cfg!(target_os = "linux") {
        // On Linux, always use the colored theme
        AppTheme::Colored
    } else {
        // On Windows the tray icon sits on the taskbar, which follows the
        // *system* theme (SystemUsesLightTheme), not the app theme. With the
        // "Custom" personalization mode the two can differ (e.g. dark taskbar
        // + light apps), and the window theme would pick an icon that is
        // invisible against the taskbar.
        #[cfg(target_os = "windows")]
        if let Some(theme) = windows_taskbar_theme() {
            return theme;
        }

        // On other platforms, map system theme to our app theme
        if let Some(main_window) = app.get_webview_window("main") {
            match main_window.theme().unwrap_or(Theme::Dark) {
                Theme::Light => AppTheme::Light,
                Theme::Dark => AppTheme::Dark,
                _ => AppTheme::Dark, // Default fallback
            }
        } else {
            AppTheme::Dark
        }
    }
}

/// Reads the Windows taskbar theme from the registry.
///
/// Returns None if the value is missing (older Windows 10 builds default to a
/// dark taskbar there, but falling back to the window theme is safer than
/// guessing).
#[cfg(target_os = "windows")]
fn windows_taskbar_theme() -> Option<AppTheme> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let personalize = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .ok()?;
    let system_uses_light: u32 = personalize.get_value("SystemUsesLightTheme").ok()?;
    Some(if system_uses_light == 1 {
        AppTheme::Light
    } else {
        AppTheme::Dark
    })
}

/// Gets the appropriate icon path for the given theme and state.
///
/// `badge` marks the idle icon while something needs attention (a notice, or
/// macOS Secure Input). Busy states keep their normal icons. The menu line says
/// what's wrong, so the badge doesn't vary by level.
pub fn get_icon_path(theme: AppTheme, state: TrayIconState, badge: bool) -> &'static str {
    if badge && state == TrayIconState::Idle {
        return match theme {
            AppTheme::Dark => "resources/tray_idle_warning.png",
            AppTheme::Light => "resources/tray_idle_warning_dark.png",
            AppTheme::Colored => "resources/handy_warning.png",
        };
    }
    match (theme, state) {
        // Dark theme uses light icons
        (AppTheme::Dark, TrayIconState::Idle) => "resources/tray_idle.png",
        (AppTheme::Dark, TrayIconState::Recording) => "resources/tray_recording.png",
        (AppTheme::Dark, TrayIconState::Transcribing) => "resources/tray_transcribing.png",
        // Light theme uses dark icons
        (AppTheme::Light, TrayIconState::Idle) => "resources/tray_idle_dark.png",
        (AppTheme::Light, TrayIconState::Recording) => "resources/tray_recording_dark.png",
        (AppTheme::Light, TrayIconState::Transcribing) => "resources/tray_transcribing_dark.png",
        // Colored theme uses pink icons (for Linux)
        (AppTheme::Colored, TrayIconState::Idle) => "resources/handy.png",
        (AppTheme::Colored, TrayIconState::Recording) => "resources/recording.png",
        (AppTheme::Colored, TrayIconState::Transcribing) => "resources/transcribing.png",
    }
}

/// Sets the recording state shown by the tray (icon + Cancel/model menu).
pub fn set_tray_state(app: &AppHandle, state: TrayIconState) {
    sync_tray_with(app, |inner| inner.icon_state = state);
}

/// Whether a dictation is recording, transcribing or delivering. Every start,
/// finish and cancel sets the tray state, so it's the record of that.
pub fn dictation_in_progress(app: &AppHandle) -> bool {
    app.try_state::<TrayState>()
        .is_some_and(|state| state.lock().icon_state.is_busy())
}

/// Re-syncs the tray after something other than the recording state changed
/// (theme, Secure Input warning, notices). The recording state itself is
/// preserved.
pub fn refresh_tray_icon(app: &AppHandle) {
    sync_tray(app);
}

/// Re-syncs the tray after something the menu depends on changed (model
/// list/selection/loaded state, language, settings).
pub fn update_tray_menu(app: &AppHandle) {
    sync_tray(app);
}

/// Rebuild the menu even if nothing it shows changed: clicking a check item
/// toggles it natively, so re-picking the active one would leave it unchecked.
pub fn rebuild_tray_menu(app: &AppHandle) {
    sync_tray_with(app, |inner| inner.applied_menu = None);
}

/// Records the current desired tray state and schedules one apply on the main
/// thread (or lets an already-pending apply pick it up). Never blocks on the
/// main thread.
///
/// The snapshot (settings, model list, loaded state) is computed on the
/// *calling* thread on purpose: the main-thread applier must not take manager
/// locks that a worker may hold across slow work (see #1716).
pub fn sync_tray(app: &AppHandle) {
    sync_tray_with(app, |_| {});
}

fn sync_tray_with(app: &AppHandle, update: impl FnOnce(&mut TrayInner)) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };

    // Record intent and claim a sequence number in one critical section, so
    // sequence order == the order in which state changes were requested.
    let (seq, icon_state, cached_language) = {
        let mut inner = state.lock();
        update(&mut inner);
        inner.next_seq += 1;
        let cached_language = inner
            .desired
            .as_ref()
            .map(|desired| desired.menu.language.clone())
            .or_else(|| {
                inner
                    .applied_menu
                    .as_ref()
                    .map(|menu| menu.language.clone())
            });
        (inner.next_seq, inner.icon_state, cached_language)
    };

    // Tray not built yet (early secure-input monitor callbacks). The intent
    // is kept and picked up by the first sync after the tray exists.
    if app.try_state::<TrayIcon>().is_none() {
        return;
    }

    let desired = compute_desired(app, icon_state, cached_language);

    // Decode the icon off the main thread, once per path, outside the lock.
    let needs_icon = !state.lock().icons.contains_key(desired.icon_path);
    let loaded_icon = if needs_icon {
        match load_tray_icon(
            app.path()
                .resolve(desired.icon_path, tauri::path::BaseDirectory::Resource),
        ) {
            Ok(image) => Some(image),
            Err(err) => {
                error!("Failed to load tray icon '{}': {err}", desired.icon_path);
                None
            }
        }
    } else {
        None
    };

    let schedule = {
        let mut inner = state.lock();
        if let Some(image) = loaded_icon {
            inner.icons.insert(desired.icon_path, image);
        }
        if seq < inner.desired_seq {
            // A request triggered after this one already stored its snapshot
            // (and scheduled an apply). Ours is stale; drop it.
            trace!(
                "tray sync: request {seq} superseded by {}",
                inner.desired_seq
            );
            return;
        }
        inner.desired = Some(desired);
        inner.desired_seq = seq;
        // If an apply is already pending it will read the snapshot we just
        // stored; otherwise schedule one.
        !std::mem::replace(&mut inner.pending, true)
    };

    if schedule {
        post_apply(app);
    } else {
        trace!("tray sync: apply already pending");
    }
}

fn compute_desired(
    app: &AppHandle,
    icon_state: TrayIconState,
    cached_language: Option<Option<LanguageMenu>>,
) -> TrayDesired {
    let settings = settings::get_settings(app);
    let theme = get_current_theme(app);
    let secure_input = crate::secure_input::tray_warning_active(app);
    let notice = notices::tray_headline(app);
    let badge = secure_input || notice.is_some();
    let tm = app.state::<Arc<TranscriptionManager>>();
    let model_loaded = tm.is_model_loaded();
    let models_locked = tm.job_active();

    let mut downloaded_models: Vec<(String, String)> = app
        .state::<Arc<ModelManager>>()
        .get_available_models()
        .into_iter()
        .filter(|m| m.is_downloaded)
        .map(|m| (m.id, m.name))
        .collect();
    downloaded_models.sort_by(|a, b| a.1.cmp(&b.1));
    // Keep the language submenu's structure stable across recording-state
    // changes without probing the detector file on the dictation path. It is
    // hidden while busy, and refreshed when the app returns to idle.
    let model_manager = app.state::<Arc<ModelManager>>();
    let compute_language = || {
        model_manager
            .get_model_info(&settings.selected_model)
            .and_then(|model| {
                language_menu(
                    &settings,
                    &model,
                    model_manager.language_detector_path().is_some(),
                )
            })
    };
    let language = if icon_state.is_busy() {
        cached_language.unwrap_or_else(compute_language)
    } else {
        compute_language()
    };

    TrayDesired {
        icon_path: get_icon_path(theme, icon_state, badge),
        menu: MenuInputs {
            busy: icon_state.is_busy(),
            models_locked,
            secure_input,
            notice: notice.map(|notice| notice.code),
            model_loaded,
            selected_model: settings.selected_model,
            downloaded_models,
            locale: settings.app_language,
            update_checks_enabled: settings.update_checks_enabled,
            language,
        },
    }
}

/// The language submenu for `model`, mirroring the settings window's picker:
/// what's checked is what transcription will use. `None` when there's
/// nothing to choose between.
fn language_menu(
    settings: &settings::AppSettings,
    model: &ModelInfo,
    detector_ready: bool,
) -> Option<LanguageMenu> {
    use crate::catalog::language_identity;
    use crate::language_detection::LanguageStatus;

    if !model.supports_language_selection {
        return None;
    }

    let (active, auto_enabled) =
        match crate::language_detection::status_for(settings, Some(model), detector_ready) {
            LanguageStatus::Auto => ("auto".to_string(), true),
            LanguageStatus::Selected { language } => (language_identity(&language), true),
            LanguageStatus::DetectorMissing { fallback: language }
            | LanguageStatus::AutoUnavailable { language, .. } => {
                (language_identity(&language), false)
            }
        };

    let mut codes: Vec<String> = Vec::new();
    for code in model
        .supported_languages
        .iter()
        .map(|c| language_identity(c))
    {
        if !codes.contains(&code) {
            codes.push(code);
        }
    }

    let mut main: Vec<String> = Vec::new();
    let spoken = settings
        .spoken_languages
        .iter()
        .map(|c| language_identity(c));
    for code in spoken.chain((active != "auto").then(|| active.clone())) {
        if codes.contains(&code) && !main.contains(&code) {
            main.push(code);
        }
    }

    let named = |code: &String| (code.clone(), crate::languages::language_name(code));
    let mut others: Vec<(String, String)> = codes
        .iter()
        .filter(|code| !main.contains(code))
        .map(named)
        .collect();
    others.sort_by(|a, b| a.1.cmp(&b.1));

    if usize::from(auto_enabled) + main.len() + others.len() < 2 {
        return None;
    }
    Some(LanguageMenu {
        active,
        auto_enabled,
        main: main.iter().map(named).collect(),
        others,
    })
}

fn language_submenu(
    app: &AppHandle,
    strings: &crate::tray_i18n::TrayStrings,
    menu: &LanguageMenu,
) -> tauri::Result<Submenu<tauri::Wry>> {
    let active_label = if menu.active == "auto" {
        strings.auto_language.clone()
    } else {
        crate::languages::language_name(&menu.active)
    };
    let submenu = Submenu::with_id(
        app,
        "language_submenu",
        format!("{} ({})", strings.language, active_label),
        true,
    )?;
    let item = |code: &str, label: &str, enabled: bool| {
        CheckMenuItem::with_id(
            app,
            format!("language_select:{code}"),
            label,
            enabled,
            menu.active == code,
            None::<&str>,
        )
    };

    submenu.append(&item("auto", &strings.auto_language, menu.auto_enabled)?)?;
    if !menu.main.is_empty() {
        submenu.append(&PredefinedMenuItem::separator(app)?)?;
        for (code, name) in &menu.main {
            submenu.append(&item(code, name, true)?)?;
        }
    }
    if !menu.others.is_empty() {
        submenu.append(&PredefinedMenuItem::separator(app)?)?;
        let others = Submenu::with_id(app, "language_others", &strings.other_languages, true)?;
        for (code, name) in &menu.others {
            others.append(&item(code, name, true)?)?;
        }
        submenu.append(&others)?;
    }
    Ok(submenu)
}

fn post_apply(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || apply_on_main(&handle)) {
        // Event loop is gone (shutdown). Clear `pending` so a later call, if
        // any, doesn't wait forever for an apply that will never run.
        error!("Failed to dispatch tray update to the main thread: {err}");
        if let Some(state) = app.try_state::<TrayState>() {
            state.lock().pending = false;
        }
    }
}

/// The single writer to the native tray. Runs on the main thread.
fn apply_on_main(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let Some(tray) = app.try_state::<TrayIcon>() else {
        return;
    };

    let started = Instant::now();
    let (desired, icon, icon_changed, menu_changed, menu_structure_changed, controls) = {
        let mut inner = state.lock();
        inner.pending = false;
        let Some(desired) = inner.desired.clone() else {
            return;
        };
        let icon_changed = inner.applied_icon != Some(desired.icon_path);
        let menu_changed = inner.applied_menu.as_ref() != Some(&desired.menu);
        if !icon_changed && !menu_changed {
            trace!("tray apply: nothing changed");
            return;
        }
        let icon = inner.icons.get(desired.icon_path).cloned();
        let menu_structure_changed = inner
            .applied_menu
            .as_ref()
            .is_none_or(|applied| !applied.same_structure(&desired.menu));
        (
            desired,
            icon,
            icon_changed,
            menu_changed,
            menu_structure_changed,
            inner.menu_controls.clone(),
        )
    };

    // Each part is recorded as applied only if its native call succeeded, so a
    // transient failure is retried on the next sync instead of being
    // remembered as displayed.
    let mut icon_ok = false;
    if icon_changed {
        match icon {
            Some(image) => match tray.set_icon_with_as_template(Some(image), true) {
                Ok(()) => icon_ok = true,
                Err(err) => error!("Failed to update tray icon '{}': {err}", desired.icon_path),
            },
            None => error!("Tray icon '{}' is not loaded", desired.icon_path),
        }
    }

    let mut menu_ok = false;
    let mut updated_controls = None;
    let mut invalidate_menu_controls = false;
    if menu_changed {
        if menu_structure_changed {
            match build_menu(app, &desired.menu) {
                Ok((menu, tooltip, controls)) => match tray.set_menu(Some(menu)) {
                    Ok(()) => {
                        menu_ok = true;
                        updated_controls = Some(controls);
                        // Best-effort: logged, not retried. The tooltip is
                        // cosmetic and can only fail on Windows, where a
                        // failing Shell_NotifyIcon call means the icon is
                        // failing too.
                        if let Err(err) = tray.set_tooltip(Some(tooltip)) {
                            error!("Failed to set tray tooltip: {err}");
                        }
                    }
                    Err(err) => error!("Failed to set tray menu: {err}"),
                },
                Err(err) => error!("Failed to build tray menu: {err}"),
            }
        } else if let Some(mut controls) = controls {
            match controls.update(&desired.menu) {
                Ok(tooltip) => {
                    menu_ok = true;
                    updated_controls = Some(controls);
                    if let Some(tooltip) = tooltip {
                        if let Err(err) = tray.set_tooltip(Some(tooltip)) {
                            error!("Failed to set tray tooltip: {err}");
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to update tray menu in place: {err}; rebuilding it");
                    invalidate_menu_controls = true;
                    match build_menu(app, &desired.menu) {
                        Ok((menu, tooltip, controls)) => match tray.set_menu(Some(menu)) {
                            Ok(()) => {
                                menu_ok = true;
                                updated_controls = Some(controls);
                                invalidate_menu_controls = false;
                                if let Err(err) = tray.set_tooltip(Some(tooltip)) {
                                    error!("Failed to set tray tooltip: {err}");
                                }
                            }
                            Err(err) => error!("Failed to rebuild tray menu: {err}"),
                        },
                        Err(err) => error!("Failed to build tray menu: {err}"),
                    }
                }
            }
        } else {
            error!("Tray menu controls are missing; rebuilding the menu");
            match build_menu(app, &desired.menu) {
                Ok((menu, tooltip, controls)) => match tray.set_menu(Some(menu)) {
                    Ok(()) => {
                        menu_ok = true;
                        updated_controls = Some(controls);
                        if let Err(err) = tray.set_tooltip(Some(tooltip)) {
                            error!("Failed to set tray tooltip: {err}");
                        }
                    }
                    Err(err) => error!("Failed to set tray menu: {err}"),
                },
                Err(err) => error!("Failed to build tray menu: {err}"),
            }
        }
    }

    {
        let mut inner = state.lock();
        if icon_ok {
            inner.applied_icon = Some(desired.icon_path);
        }
        if menu_ok {
            inner.applied_menu = Some(desired.menu.clone());
            if let Some(controls) = updated_controls {
                inner.menu_controls = Some(controls);
            }
        } else if invalidate_menu_controls {
            // An in-place operation may have succeeded partially before a
            // later GTK/DBusMenu operation failed. Don't retry against stale
            // handles and stale layout assumptions on the next sync.
            inner.menu_controls = None;
        }
    }

    debug!(
        "tray apply: icon={} menu={} busy={} took={:?}",
        if icon_changed {
            desired.icon_path
        } else {
            "unchanged"
        },
        if menu_changed {
            if menu_structure_changed {
                "rebuilt"
            } else {
                "updated"
            }
        } else {
            "unchanged"
        },
        desired.menu.busy,
        started.elapsed()
    );
}

fn load_tray_icon(resolved_icon_path: tauri::Result<PathBuf>) -> tauri::Result<Image<'static>> {
    let resolved_icon_path = resolved_icon_path?;
    Image::from_path(&resolved_icon_path).map(Image::to_owned)
}

pub fn tray_tooltip() -> String {
    version_label()
}

fn version_label() -> String {
    if cfg!(debug_assertions) {
        format!("Handy v{} (Dev)", env!("CARGO_PKG_VERSION"))
    } else {
        format!("Handy v{}", env!("CARGO_PKG_VERSION"))
    }
}

/// Builds the tray menu and tooltip for the given inputs. Pure with respect
/// to app state: everything it depends on is in `inputs`, plus the
/// process-constant `HANDY_DISABLE_UPDATER` env flag behind
/// `update_checks_forced_disabled()`, which cannot change during a run.
fn build_menu(
    app: &AppHandle,
    inputs: &MenuInputs,
) -> tauri::Result<(Menu<tauri::Wry>, String, MenuControls)> {
    let strings = get_tray_translations(Some(inputs.locale.clone()));

    // Secure Input warning entry (macOS): clicking opens the settings window
    // where the full warning banner explains the situation. Locales that
    // haven't translated the key yet get the English string rather than a
    // blank menu item (build.rs emits "" for missing keys).
    let secure_input_label = if strings.secure_input_warning.is_empty() {
        get_tray_translations(Some("en".to_string())).secure_input_warning
    } else {
        strings.secure_input_warning.clone()
    };
    let secure_input_warning = MenuItem::with_id(
        app,
        "secure_input_warning",
        &secure_input_label,
        true,
        None::<&str>,
    )?;

    // Clicking opens the window, where the footer has the details.
    let notice_label = inputs
        .notice
        .map(|code| code.tray_title(&strings))
        .unwrap_or_default();
    let notice = MenuItem::with_id(
        app,
        "notice",
        notice_label,
        inputs.notice.is_some(),
        None::<&str>,
    )?;

    // Platform-specific accelerators
    #[cfg(target_os = "macos")]
    let (settings_accelerator, quit_accelerator) = (Some("Cmd+,"), Some("Cmd+Q"));
    #[cfg(not(target_os = "macos"))]
    let (settings_accelerator, quit_accelerator) = (Some("Ctrl+,"), Some("Ctrl+Q"));

    // Create common menu items
    let version_label = version_label();
    let version_i = MenuItem::with_id(app, "version", &version_label, false, None::<&str>)?;
    let settings_i = MenuItem::with_id(
        app,
        "settings",
        &strings.settings,
        true,
        settings_accelerator,
    )?;
    let check_updates_i = MenuItem::with_id(
        app,
        "check_updates",
        &strings.check_updates,
        inputs.update_checks_enabled,
        None::<&str>,
    )?;
    let copy_last_transcript_i = MenuItem::with_id(
        app,
        "copy_last_transcript",
        &strings.copy_last_transcript,
        true,
        None::<&str>,
    )?;
    let cancel_i = MenuItem::with_id(app, "cancel", &strings.cancel, inputs.busy, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", &strings.quit, true, quit_accelerator)?;
    let separator = || PredefinedMenuItem::separator(app);

    // Build both state-specific groups up front. On state transitions one
    // group is removed and the other inserted into this root menu, preserving
    // the menu's existing visible layout without another set_menu() call.
    let submenu_label = inputs
        .downloaded_models
        .iter()
        .find(|(id, _)| *id == inputs.selected_model)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| strings.model.clone());
    let model_submenu = Submenu::with_id(
        app,
        "model_submenu",
        &submenu_label,
        !inputs.busy && !inputs.models_locked,
    )?;
    for (id, name) in &inputs.downloaded_models {
        let is_active = *id == inputs.selected_model;
        let item_id = format!("model_select:{}", id);
        let item = CheckMenuItem::with_id(app, &item_id, name, true, is_active, None::<&str>)?;
        model_submenu.append(&item)?;
    }
    let language_i = inputs
        .language
        .as_ref()
        .map(|language| language_submenu(app, &strings, language))
        .transpose()?;
    let unload_model_i = MenuItem::with_id(
        app,
        "unload_model",
        &strings.unload_model,
        inputs.model_loaded && !inputs.busy && !inputs.models_locked,
        None::<&str>,
    )?;

    let intro_separator = separator()?;
    let copy_separator = separator()?;
    let warning_separator = separator()?;
    let action_separator = separator()?;
    let bottom_separator = separator()?;
    let menu = Menu::new(app)?;
    menu.append(&version_i)?;
    menu.append(&intro_separator)?;

    let mut tooltip = version_label;
    if inputs.secure_input {
        menu.append(&secure_input_warning)?;
        tooltip = format!("{} — {}", tooltip, secure_input_label);
    }
    if inputs.notice.is_some() {
        menu.append(&notice)?;
    }
    if inputs.secure_input || inputs.notice.is_some() {
        menu.append(&warning_separator)?;
    }

    let content_start = menu.items()?.len();
    if inputs.busy {
        menu.append(&cancel_i)?;
        menu.append(&copy_separator)?;
        menu.append(&copy_last_transcript_i)?;
    } else {
        menu.append(&copy_last_transcript_i)?;
        menu.append(&copy_separator)?;
        menu.append(&model_submenu)?;
        if let Some(language) = &language_i {
            menu.append(language)?;
        }
        menu.append(&unload_model_i)?;
    }
    menu.append(&action_separator)?;
    menu.append(&settings_i)?;

    // When update checks are forced off (e.g. HANDY_DISABLE_UPDATER, set by
    // the Nix package), the item is dropped rather than shown disabled.
    if !settings::update_checks_forced_disabled() {
        menu.append(&check_updates_i)?;
    }
    menu.append(&bottom_separator)?;
    menu.append(&quit_i)?;

    let controls = MenuControls {
        menu: menu.clone(),
        secure_input_warning,
        notice,
        warning_separator,
        cancel: cancel_i,
        copy_last_transcript: copy_last_transcript_i,
        copy_separator,
        model_submenu,
        language_submenu: language_i,
        unload_model: unload_model_i,
        action_separator,
        content_start,
        busy: inputs.busy,
        secure_input: inputs.secure_input,
        notice_code: inputs.notice,
        check_updates: check_updates_i,
    };
    Ok((menu, tooltip, controls))
}

fn last_transcript_text(entry: &HistoryEntry) -> &str {
    entry
        .post_processed_text
        .as_deref()
        .unwrap_or(&entry.transcription_text)
}

pub fn set_tray_visibility(app: &AppHandle, visible: bool) {
    let tray = app.state::<TrayIcon>();
    if let Err(e) = tray.set_visible(visible) {
        error!("Failed to set tray visibility: {}", e);
    } else {
        info!("Tray visibility set to: {}", visible);
    }
}

/// Recovery for the macOS tray-disappearance bug (#1948, tauri-apps/tauri#12060):
/// the `NSStatusItem` can silently vanish with no error surfaced to the app.
/// Hiding and re-showing the tray recreates it with its current icon, menu and
/// tooltip. Called when the user "relaunches" Handy while it is already running
/// (`RunEvent::Reopen` for Spotlight/Finder/Dock, the single-instance callback
/// for a second process) — the natural "where did my icon go?" moment — so a
/// relaunch brings the icon back without a full quit.
#[cfg(target_os = "macos")]
pub fn recreate_tray_icon(app: &AppHandle) {
    let no_tray = app
        .try_state::<crate::cli::CliArgs>()
        .map(|args| args.no_tray)
        .unwrap_or(false);
    if no_tray || !settings::get_settings(app).show_tray_icon {
        return;
    }
    let Some(tray) = app.try_state::<TrayIcon>() else {
        return;
    };
    info!("Recreating tray icon on relaunch");
    if let Err(e) = tray.set_visible(false).and_then(|_| tray.set_visible(true)) {
        error!("Failed to recreate tray icon: {}", e);
    }
}

pub fn copy_last_transcript(app: &AppHandle) {
    let history_manager = app.state::<Arc<HistoryManager>>();
    let entry = match history_manager.get_latest_completed_entry() {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            warn!("No completed transcription history entries available for tray copy.");
            return;
        }
        Err(err) => {
            error!(
                "Failed to fetch last completed transcription entry: {}",
                err
            );
            return;
        }
    };

    let text = last_transcript_text(&entry);
    if text.trim().is_empty() {
        warn!("Last completed transcription is empty; skipping tray copy.");
        return;
    }

    if let Err(err) = app.clipboard().write_text(text) {
        error!("Failed to copy last transcript to clipboard: {}", err);
        return;
    }

    info!("Copied last transcript to clipboard via tray.");
}

#[cfg(test)]
mod tests {
    use super::{
        get_icon_path, language_menu, last_transcript_text, load_tray_icon, AppTheme, MenuInputs,
        TrayDesired, TrayIconState,
    };
    use crate::managers::history::HistoryEntry;
    use crate::managers::model::{ModelInfo, ModelSource};
    use crate::notices::NoticeCode;
    use crate::settings::{get_default_settings, AppSettings};

    fn codes(list: &str) -> Vec<String> {
        list.split_whitespace().map(String::from).collect()
    }

    fn model(languages: &str, detects: bool) -> ModelInfo {
        let supported_languages = codes(languages);
        ModelInfo {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            filename: String::new(),
            source: ModelSource::Url {
                url: String::new(),
                sha256: None,
            },
            size_mb: 0,
            is_downloaded: true,
            is_downloading: false,
            partial_size: 0,
            accuracy_score: 0.0,
            speed_score: 0.0,
            supports_translation: false,
            is_recommended: false,
            supports_language_selection: supported_languages.len() > 1,
            supported_languages,
            is_custom: false,
            supports_streaming: false,
            supports_language_detection: detects,
        }
    }

    fn settings(intent: &str, spoken: &str) -> AppSettings {
        let mut settings = get_default_settings();
        settings.selected_language = intent.to_string();
        settings.spoken_languages = codes(spoken);
        settings
    }

    fn entries(list: &[(String, String)]) -> Vec<&str> {
        list.iter().map(|(code, _)| code.as_str()).collect()
    }

    #[test]
    fn language_menu_leads_with_spoken_languages() {
        let menu = language_menu(
            &settings("auto", "fr en"),
            &model("en de fr ja", true),
            true,
        )
        .expect("menu");
        assert_eq!(menu.active, "auto");
        assert!(menu.auto_enabled);
        assert_eq!(entries(&menu.main), ["fr", "en"]);
        // Others by English name: German, Japanese.
        assert_eq!(entries(&menu.others), ["de", "ja"]);
    }

    #[test]
    fn language_menu_shows_the_active_language_outside_spoken_ones() {
        let menu =
            language_menu(&settings("ja", "en"), &model("en de ja", true), true).expect("menu");
        assert_eq!(menu.active, "ja");
        assert_eq!(entries(&menu.main), ["en", "ja"]);
        assert_eq!(entries(&menu.others), ["de"]);
    }

    #[test]
    fn language_menu_collapses_spellings() {
        let menu = language_menu(&settings("auto", "no"), &model("en-US nb no", true), true)
            .expect("menu");
        assert_eq!(entries(&menu.main), ["nb"]);
        assert_eq!(entries(&menu.others), ["en"]);
    }

    #[test]
    fn language_menu_checks_the_fallback_without_the_detector() {
        let menu = language_menu(&settings("auto", "en fr"), &model("en fr de", false), false)
            .expect("menu");
        assert_eq!(menu.active, "en");
        assert!(!menu.auto_enabled);
    }

    #[test]
    fn language_menu_hidden_without_a_choice() {
        assert_eq!(
            language_menu(&settings("auto", "en"), &model("en", false), true),
            None
        );
    }

    fn build_entry(transcription: &str, post_processed: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: 1,
            file_name: "handy-1.wav".to_string(),
            timestamp: 0,
            saved: false,
            title: "Recording".to_string(),
            transcription_text: transcription.to_string(),
            post_processed_text: post_processed.map(|text| text.to_string()),
            post_process_prompt: None,
            post_process_requested: false,
            source_name: None,
            language: None,
            language_source: None,
        }
    }

    fn inputs(busy: bool) -> MenuInputs {
        MenuInputs {
            busy,
            models_locked: false,
            secure_input: false,
            notice: None,
            model_loaded: true,
            selected_model: "small".to_string(),
            downloaded_models: vec![("small".to_string(), "Small".to_string())],
            locale: "en".to_string(),
            update_checks_enabled: true,
            language: None,
        }
    }

    #[test]
    fn uses_post_processed_text_when_available() {
        let entry = build_entry("raw", Some("processed"));
        assert_eq!(last_transcript_text(&entry), "processed");
    }

    #[test]
    fn falls_back_to_raw_transcription() {
        let entry = build_entry("raw", None);
        assert_eq!(last_transcript_text(&entry), "raw");
    }

    #[test]
    fn tray_icon_resolution_failure_is_returned_instead_of_panicking() {
        assert!(load_tray_icon(Err(tauri::Error::UnknownPath)).is_err());
    }

    #[test]
    fn tray_icon_returns_err_when_file_does_not_exist() {
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let missing = dir.path().join("does_not_exist.png");
        assert!(load_tray_icon(Ok(missing)).is_err());
    }

    #[test]
    fn recording_and_transcribing_share_a_menu() {
        // The icon differs but the menu inputs are identical, so a
        // Recording -> Transcribing transition must not rebuild the menu.
        let recording = TrayDesired {
            icon_path: "resources/tray_recording.png",
            menu: inputs(TrayIconState::Recording.is_busy()),
        };
        let transcribing = TrayDesired {
            icon_path: "resources/tray_transcribing.png",
            menu: inputs(TrayIconState::Transcribing.is_busy()),
        };
        assert_ne!(recording.icon_path, transcribing.icon_path);
        assert_eq!(recording.menu, transcribing.menu);
    }

    #[test]
    fn the_badge_shows_at_idle_only_and_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for theme in [AppTheme::Dark, AppTheme::Light, AppTheme::Colored] {
            let badged = get_icon_path(theme.clone(), TrayIconState::Idle, true);
            assert_ne!(
                badged,
                get_icon_path(theme.clone(), TrayIconState::Idle, false)
            );
            assert!(root.join(badged).is_file(), "{badged} is missing");
            assert_eq!(
                get_icon_path(theme.clone(), TrayIconState::Recording, true),
                get_icon_path(theme, TrayIconState::Recording, false)
            );
        }
    }

    #[test]
    fn idle_and_busy_menus_differ() {
        assert_ne!(inputs(false), inputs(true));
    }

    #[test]
    fn state_changes_do_not_change_menu_structure() {
        let idle = inputs(false);
        let mut busy = inputs(true);
        busy.models_locked = true;
        busy.model_loaded = false;
        busy.update_checks_enabled = false;
        busy.secure_input = true;
        busy.notice = Some(NoticeCode::MicrophoneSilent);
        assert!(idle.same_structure(&busy));

        let mut changed_locale = busy.clone();
        changed_locale.locale = "fr".to_string();
        assert!(!idle.same_structure(&changed_locale));

        let mut changed_model = busy;
        changed_model.selected_model = "large".to_string();
        assert!(!idle.same_structure(&changed_model));
    }
}
