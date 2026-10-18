//! In-app updates: a quiet background check against the GitHub release feed,
//! and a download-verify-replace-restart flow that only runs when the owner
//! presses the button.
//!
//! The frontend is a thin view over [`UpdateStatus`]: it reads it with
//! `voxa_update_status` and follows the `voxa-update-status` event. Checking
//! here (not in the webview) keeps it working while the window is closed, and
//! lets the tray menu show the badge.
//!
//! Network use is limited to the feed (`latest.json`) and, after the button, the
//! signed archive it points at. Nothing is installed without a click.

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Event carrying every [`UpdateStatus`] change.
pub const STATUS_EVENT: &str = "voxa-update-status";
/// Event asking the window to open the page with the update card.
pub const OPEN_EVENT: &str = "voxa-open-updates";

/// Background check period.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Grace period after start so the check never competes with startup work.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// `tauri.conf.json` holds this until the owner's updater key is set up
/// (docs/SETUP_SECRETS.md); the release workflow injects the real key.
const PUBKEY_PLACEHOLDER_PREFIX: &str = "PLACEHOLDER";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    /// Nothing checked yet.
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateErrorKind {
    /// Switched off by `HANDY_DISABLE_UPDATER`.
    Disabled,
    /// The release signing key is not set up yet.
    NotConfigured,
    Network,
    /// The archive does not match the signature, or the key is invalid.
    Signature,
    /// The feed answered but has no release.
    NoRelease,
    Other,
}

/// Everything the UI shows about updates.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct UpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    /// The announced version while `phase` is `Available`, `Downloading`,
    /// `Installing`, or `Error` after a failed install.
    pub version: Option<String>,
    /// Release notes (the commit list the release workflow wrote).
    pub notes: Option<String>,
    /// Publication date, RFC 3339.
    pub published: Option<String>,
    /// Download progress in percent, while `Downloading`.
    pub progress: Option<u32>,
    pub error_kind: Option<UpdateErrorKind>,
    /// Technical detail of the error, for the log line under the message.
    pub error_detail: Option<String>,
    /// When the last finished check ended, RFC 3339.
    pub last_checked: Option<String>,
}

impl UpdateStatus {
    fn idle(current_version: &str) -> Self {
        Self {
            phase: UpdatePhase::Idle,
            current_version: current_version.to_string(),
            version: None,
            notes: None,
            published: None,
            progress: None,
            error_kind: None,
            error_detail: None,
            last_checked: None,
        }
    }

    fn busy(&self) -> bool {
        matches!(
            self.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading | UpdatePhase::Installing
        )
    }

    fn error(mut self, kind: UpdateErrorKind, detail: Option<String>) -> Self {
        self.phase = UpdatePhase::Error;
        self.progress = None;
        self.error_kind = Some(kind);
        self.error_detail = detail;
        self
    }
}

struct Inner {
    status: UpdateStatus,
    /// The update found by the last check, kept so the button installs exactly
    /// what the owner saw.
    update: Option<Update>,
}

/// Tauri state.
pub struct UpdateManager {
    inner: Mutex<Inner>,
}

impl UpdateManager {
    pub fn new(current_version: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                status: UpdateStatus::idle(current_version),
                update: None,
            }),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.inner.lock().expect("update state").status.clone()
    }

    /// Version to announce in the tray menu, when an update is waiting.
    pub fn available_version(&self) -> Option<String> {
        let inner = self.inner.lock().expect("update state");
        match inner.status.phase {
            UpdatePhase::Available => inner.status.version.clone(),
            _ => None,
        }
    }

    /// Moves to `next` unless a check or install is already running.
    /// Returns the status to restore when the step ends quietly.
    fn begin(&self, app: &AppHandle, next: UpdatePhase) -> Option<UpdateStatus> {
        let previous = {
            let mut inner = self.inner.lock().expect("update state");
            if inner.status.busy() {
                return None;
            }
            let previous = inner.status.clone();
            inner.status.phase = next;
            inner.status.progress = None;
            inner.status.error_kind = None;
            inner.status.error_detail = None;
            previous
        };
        self.publish(app);
        Some(previous)
    }

    fn set(&self, app: &AppHandle, status: UpdateStatus, update: Option<Update>) {
        {
            let mut inner = self.inner.lock().expect("update state");
            inner.status = status;
            if let Some(update) = update {
                inner.update = Some(update);
            }
        }
        self.publish(app);
    }

    fn announced_update(&self) -> Option<Update> {
        self.inner.lock().expect("update state").update.clone()
    }

    fn clear_update(&self) {
        self.inner.lock().expect("update state").update = None;
    }

    fn update_progress(&self, app: &AppHandle, percent: u32) {
        {
            let mut inner = self.inner.lock().expect("update state");
            if inner.status.progress == Some(percent) {
                return;
            }
            inner.status.progress = Some(percent);
        }
        self.publish(app);
    }

    fn publish(&self, app: &AppHandle) {
        let _ = app.emit(STATUS_EVENT, self.status());
        crate::tray::update_tray_menu(app);
    }
}

/// Classifies an updater error for the message the owner reads.
pub fn classify_error(error: &tauri_plugin_updater::Error) -> UpdateErrorKind {
    use tauri_plugin_updater::Error as E;
    match error {
        E::ReleaseNotFound | E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            UpdateErrorKind::NoRelease
        }
        E::Reqwest(_) | E::Network(_) => UpdateErrorKind::Network,
        E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) => UpdateErrorKind::Signature,
        _ => UpdateErrorKind::Other,
    }
}

/// True while `tauri.conf.json` still carries the placeholder key.
fn pubkey_is_placeholder(pubkey: Option<&str>) -> bool {
    match pubkey {
        None => true,
        Some(key) => {
            let key = key.trim();
            key.is_empty() || key.starts_with(PUBKEY_PLACEHOLDER_PREFIX)
        }
    }
}

fn configured_pubkey(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn now_rfc3339() -> Option<String> {
    Some(chrono::Utc::now().to_rfc3339())
}

/// Release notes as shown: trimmed, `None` when empty.
fn clean_notes(body: Option<String>) -> Option<String> {
    body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty())
}

/// Checks the feed. `manual` is a click on "Check for updates": it reports
/// errors and works with automatic checks off. A background check (`false`)
/// does nothing while automatic checks are off and fails silently, leaving an
/// already announced update in place.
pub async fn check(app: &AppHandle, manual: bool) -> UpdateStatus {
    let manager = app.state::<UpdateManager>();
    let current = manager.status();

    if crate::settings::update_checks_forced_disabled() {
        return current;
    }
    if !manual && !crate::settings::get_settings(app).update_checks_enabled {
        return current;
    }
    let Some(previous) = manager.begin(app, UpdatePhase::Checking) else {
        return manager.status();
    };

    if pubkey_is_placeholder(configured_pubkey(app).as_deref()) {
        info!("Update check skipped: no updater public key in tauri.conf.json");
        let status = previous
            .error(UpdateErrorKind::NotConfigured, None)
            .into_quiet(manual);
        manager.set(app, status, None);
        return manager.status();
    }

    let result = match app.updater_builder().timeout(REQUEST_TIMEOUT).build() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };

    let mut status = previous.clone();
    status.last_checked = now_rfc3339();
    let mut found = None;
    match result {
        Ok(Some(update)) => {
            info!(
                "Update available: {} (running {})",
                update.version, update.current_version
            );
            status.phase = UpdatePhase::Available;
            status.version = Some(update.version.clone());
            status.notes = clean_notes(update.body.clone());
            status.published = update
                .date
                .and_then(|d| chrono::DateTime::from_timestamp(d.unix_timestamp(), 0))
                .map(|d| d.to_rfc3339());
            status.error_kind = None;
            status.error_detail = None;
            found = Some(update);
        }
        Ok(None) => {
            debug!("No update available");
            status.phase = UpdatePhase::UpToDate;
            status.version = None;
            status.notes = None;
            status.published = None;
            status.error_kind = None;
            status.error_detail = None;
            manager.clear_update();
        }
        Err(e) => {
            let kind = classify_error(&e);
            warn!("Update check failed ({kind:?}): {e}");
            status = previous.error(kind, Some(e.to_string())).into_quiet(manual);
            status.last_checked = now_rfc3339();
        }
    }
    manager.set(app, status, found);
    manager.status()
}

impl UpdateStatus {
    /// A background check that failed must not replace what the owner already
    /// sees: restore the previous state, keep the failure in the log only.
    fn into_quiet(self, manual: bool) -> Self {
        if manual {
            return self;
        }
        // `self` is `previous` with the error fields set; undo them.
        let mut status = self;
        status.error_kind = None;
        status.error_detail = None;
        status.phase = if status.version.is_some() {
            UpdatePhase::Available
        } else if status.last_checked.is_some() {
            UpdatePhase::UpToDate
        } else {
            UpdatePhase::Idle
        };
        status
    }
}

/// Downloads, verifies and installs the announced update, then restarts the
/// app. Only reached from the button.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if crate::settings::update_checks_forced_disabled() {
        return Err("Updates are disabled by system configuration".into());
    }
    let manager = app.state::<UpdateManager>();

    let mut announced = manager.announced_update();
    if announced.is_none() {
        // Lost the handle (e.g. the status was restored after a failed
        // install): look again.
        check(app, true).await;
        announced = manager.announced_update();
    }
    let update = announced.ok_or_else(|| "No update available".to_string())?;

    let Some(previous) = manager.begin(app, UpdatePhase::Downloading) else {
        return Err("An update step is already running".into());
    };
    manager.update_progress(app, 0);

    let mut downloaded: u64 = 0;
    let progress_app = app.clone();
    let finish_app = app.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                if let Some(total) = total.filter(|t| *t > 0) {
                    let percent = ((downloaded * 100) / total).min(100) as u32;
                    progress_app
                        .state::<UpdateManager>()
                        .update_progress(&progress_app, percent);
                }
            },
            move || {
                let manager = finish_app.state::<UpdateManager>();
                let mut status = manager.status();
                status.phase = UpdatePhase::Installing;
                status.progress = Some(100);
                manager.set(&finish_app, status, None);
            },
        )
        .await;

    match result {
        Ok(()) => {
            info!("Update installed, restarting");
            // The engine worker exits when its parent goes away.
            app.restart()
        }
        Err(e) => {
            let kind = classify_error(&e);
            warn!("Update install failed ({kind:?}): {e}");
            let mut status = previous;
            status.last_checked = now_rfc3339();
            let status = status.error(kind, Some(e.to_string()));
            manager.set(app, status, None);
            Err(e.to_string())
        }
    }
}

/// Registers the state and starts the background loop.
pub fn init(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    app.manage(UpdateManager::new(&version));

    if crate::settings::update_checks_forced_disabled() {
        info!("In-app updates are disabled by HANDY_DISABLE_UPDATER");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            check(&app, false).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_updater::Error as E;

    #[test]
    fn placeholder_key_is_detected() {
        assert!(pubkey_is_placeholder(None));
        assert!(pubkey_is_placeholder(Some("")));
        assert!(pubkey_is_placeholder(Some("  ")));
        assert!(pubkey_is_placeholder(Some("PLACEHOLDER_SET_IN_RELEASE")));
        assert!(!pubkey_is_placeholder(Some(
            "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWdu"
        )));
    }

    #[test]
    fn errors_are_classified_for_the_message() {
        assert_eq!(
            classify_error(&E::ReleaseNotFound),
            UpdateErrorKind::NoRelease
        );
        assert_eq!(
            classify_error(&E::Network("connection reset".into())),
            UpdateErrorKind::Network
        );
        assert_eq!(
            classify_error(&E::SignatureUtf8("x".into())),
            UpdateErrorKind::Signature
        );
        assert_eq!(classify_error(&E::EmptyEndpoints), UpdateErrorKind::Other);
    }

    #[test]
    fn notes_are_trimmed_and_empty_ones_dropped() {
        assert_eq!(clean_notes(None), None);
        assert_eq!(clean_notes(Some("  \n ".into())), None);
        assert_eq!(
            clean_notes(Some("\n- a\n- b\n".into())),
            Some("- a\n- b".to_string())
        );
    }

    fn announced() -> UpdateStatus {
        let mut s = UpdateStatus::idle("0.10.1");
        s.phase = UpdatePhase::Available;
        s.version = Some("0.10.2".into());
        s.last_checked = Some("2026-10-18T10:00:00Z".into());
        s
    }

    #[test]
    fn a_failed_background_check_keeps_the_announced_update() {
        let failed = announced().error(UpdateErrorKind::Network, Some("offline".into()));
        let shown = failed.into_quiet(false);
        assert_eq!(shown.phase, UpdatePhase::Available);
        assert_eq!(shown.version.as_deref(), Some("0.10.2"));
        assert_eq!(shown.error_kind, None);
    }

    #[test]
    fn a_failed_background_check_without_history_stays_idle() {
        let failed = UpdateStatus::idle("0.10.1").error(UpdateErrorKind::Network, None);
        assert_eq!(failed.into_quiet(false).phase, UpdatePhase::Idle);
    }

    #[test]
    fn a_failed_manual_check_reports_the_error() {
        let failed = announced().error(UpdateErrorKind::Network, Some("offline".into()));
        let shown = failed.into_quiet(true);
        assert_eq!(shown.phase, UpdatePhase::Error);
        assert_eq!(shown.error_kind, Some(UpdateErrorKind::Network));
    }

    #[test]
    fn busy_phases_block_a_second_step() {
        for (phase, busy) in [
            (UpdatePhase::Idle, false),
            (UpdatePhase::UpToDate, false),
            (UpdatePhase::Available, false),
            (UpdatePhase::Error, false),
            (UpdatePhase::Checking, true),
            (UpdatePhase::Downloading, true),
            (UpdatePhase::Installing, true),
        ] {
            let mut s = UpdateStatus::idle("0.10.1");
            s.phase = phase;
            assert_eq!(s.busy(), busy, "{phase:?}");
        }
    }
}
