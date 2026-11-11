//! Where the settings live: in memory, loaded once at startup and saved to
//! `settings_store.json` in the background after every change.
//!
//! The file keeps the layout tauri-plugin-store gave it (`{"settings": {...}}`),
//! so upgrades and downgrades read it as before. What's different is what
//! happens when something goes wrong with it:
//!
//! - **Missing:** a fresh install. Start from the defaults and save them.
//! - **Can't be read** (permissions, a lock): use the defaults for this run and
//!   never write, so the user's file survives (#2480).
//! - **Damaged** (invalid JSON, or fields this build can't parse): keep a copy of
//!   the file next to it before saving the repaired settings (#1619, #2139).
//! - **Saving fails:** keep retrying on the next change, and tell the user.
//!
//! Each of these raises a [`NoticeSource::Settings`] notice and logs the path.
//!
//! A save writes a temporary file and renames it over the old one, so a crash
//! or power loss mid-save leaves the old file or the new one, never a
//! truncated one.

use crate::notices::{self, Notice, NoticeCode, NoticeSource};
use crate::settings::{restore_settings, AppSettings, SETTINGS_STORE_PATH};
use log::{debug, error, info, warn};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};
use std::time::Duration;
use tauri::AppHandle;

/// Attempts at reading a file that exists, for locks that clear quickly
/// (antivirus scans, a sync client, another process mid-write).
const READ_ATTEMPTS: u32 = 3;
/// Attempts at replacing the file, for the same locks on Windows.
const RENAME_ATTEMPTS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(100);

static STORE: OnceLock<SettingsStore> = OnceLock::new();

struct SettingsStore {
    app: AppHandle,
    /// `None` when the path couldn't be resolved: nothing is ever written.
    path: Option<PathBuf>,
    /// The file exists but couldn't be read: nothing is ever written.
    read_only: bool,
    settings: RwLock<AppSettings>,
    save: Mutex<SaveState>,
    /// Wakes the writer when there's a change, and `flush` when it's written.
    save_changed: Condvar,
    /// Raised by `init`, once the store is in place (raising refreshes the
    /// tray, which reads the settings).
    pending_notices: Mutex<Vec<Notice>>,
}

#[derive(Default)]
struct SaveState {
    /// Bumped by every change.
    generation: u64,
    /// The last generation the writer tried to save, successfully or not.
    attempted: u64,
    writer_started: bool,
    /// The last save failed; the next success clears its notice.
    failing: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Loads the settings file if nothing has yet, and raises what's wrong with it.
pub fn init(app: &AppHandle) {
    let store = store(app);
    let pending = std::mem::take(&mut *lock(&store.pending_notices));
    for notice in pending {
        notices::raise_without_overlay(app, notice);
    }
}

pub fn get(app: &AppHandle) -> AppSettings {
    store(app)
        .settings
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

pub fn set(app: &AppHandle, settings: AppSettings) {
    let store = store(app);
    *store
        .settings
        .write()
        .unwrap_or_else(PoisonError::into_inner) = settings;
    store.schedule_save();
}

/// Waits until every change made so far has been written (or failed to be).
/// A no-op before the settings were loaded.
pub fn flush() {
    let Some(store) = STORE.get() else {
        return;
    };
    let mut save = lock(&store.save);
    while save.writer_started && save.attempted < save.generation {
        save = store
            .save_changed
            .wait(save)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

fn store(app: &AppHandle) -> &'static SettingsStore {
    if let Some(store) = STORE.get() {
        return store;
    }
    let mut save = false;
    let store = STORE.get_or_init(|| {
        let (store, needs_save) = SettingsStore::load(app);
        save = needs_save;
        store
    });
    if save {
        store.schedule_save();
    }
    store
}

impl SettingsStore {
    /// Also returns whether the loaded settings should be written.
    fn load(app: &AppHandle) -> (Self, bool) {
        let path = crate::portable::app_data_dir(app)
            .map(|dir| dir.join(SETTINGS_STORE_PATH))
            .map_err(|e| error!("Can't resolve the settings path ({e}); settings won't be saved"))
            .ok();

        let loaded = match &path {
            Some(path) => load_file(path),
            None => Loaded {
                settings: restore_settings(None).settings,
                save: false,
                read_only: true,
                notice: Some(Notice::new(
                    NoticeCode::SettingsUnreadable,
                    NoticeSource::Settings,
                )),
            },
        };

        let store = Self {
            app: app.clone(),
            path,
            read_only: loaded.read_only,
            settings: RwLock::new(loaded.settings),
            save: Mutex::new(SaveState::default()),
            save_changed: Condvar::new(),
            pending_notices: Mutex::new(loaded.notice.into_iter().collect()),
        };
        (store, loaded.save)
    }

    fn schedule_save(&'static self) {
        let Some(path) = self.path.as_ref().filter(|_| !self.read_only) else {
            return;
        };
        let start_writer = {
            let mut save = lock(&self.save);
            save.generation += 1;
            !std::mem::replace(&mut save.writer_started, true)
        };
        if start_writer {
            let spawned = std::thread::Builder::new()
                .name("settings-writer".into())
                .spawn(move || self.run_writer(path));
            if let Err(e) = spawned {
                // Save inline instead; `flush` mustn't wait on a writer that
                // doesn't exist.
                error!("Failed to start the settings writer ({e}); saving inline");
                let generation = {
                    let mut save = lock(&self.save);
                    save.writer_started = false;
                    save.generation
                };
                self.save_now(path, generation);
                return;
            }
        }
        self.save_changed.notify_all();
    }

    /// Saves the newest settings whenever they change. Changes that arrive
    /// while a save runs are written together by the next one.
    fn run_writer(&self, path: &Path) {
        loop {
            let generation = {
                let mut save = lock(&self.save);
                while save.attempted >= save.generation {
                    save = self
                        .save_changed
                        .wait(save)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                save.generation
            };
            self.save_now(path, generation);
        }
    }

    fn save_now(&self, path: &Path, generation: u64) {
        let snapshot = self
            .settings
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let result = save_file(path, &snapshot);

        let (recovered, failed) = {
            let mut save = lock(&self.save);
            save.attempted = save.attempted.max(generation);
            let was_failing = save.failing;
            save.failing = result.is_err();
            (
                was_failing && result.is_ok(),
                !was_failing && result.is_err(),
            )
        };
        self.save_changed.notify_all();

        match result {
            Ok(()) => {
                if recovered {
                    info!("Saved settings to {} again", path.display());
                    notices::clear_matching(
                        &self.app,
                        NoticeSource::Settings,
                        Some(NoticeCode::SettingsSaveFailed),
                    );
                }
            }
            Err(e) => {
                error!("Failed to save settings to {}: {e}", path.display());
                if failed {
                    notices::raise_without_overlay(
                        &self.app,
                        Notice::new(NoticeCode::SettingsSaveFailed, NoticeSource::Settings)
                            .with_detail(e.to_string()),
                    );
                }
            }
        }
    }
}

/// What came of reading the settings file.
struct Loaded {
    settings: AppSettings,
    /// The settings differ from the file and should be written.
    save: bool,
    /// Never write: the file is there but couldn't be read.
    read_only: bool,
    notice: Option<Notice>,
}

fn load_file(path: &Path) -> Loaded {
    let bytes = match read_with_retries(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            info!("No settings at {}; starting with defaults", path.display());
            return Loaded {
                settings: restore_settings(None).settings,
                save: true,
                read_only: false,
                notice: None,
            };
        }
        Err(e) => {
            error!(
                "Can't read settings at {}: {e}. Using defaults for this run and leaving the file untouched",
                path.display()
            );
            return Loaded {
                settings: restore_settings(None).settings,
                save: false,
                read_only: true,
                notice: Some(
                    Notice::new(NoticeCode::SettingsUnreadable, NoticeSource::Settings)
                        .with_detail(format!("{} ({e})", path.display())),
                ),
            };
        }
    };

    let parsed = parse_file(&bytes);
    let mut repaired = parsed.repaired;
    let restored = restore_settings(parsed.settings.as_ref());
    if parsed.settings.is_none() && !parsed.file_was_empty {
        // A file we couldn't make sense of: the whole thing goes back to the
        // defaults, unlike a fresh install.
        repaired.push("settings".to_string());
    }
    repaired.extend(restored.reset_fields);

    if repaired.is_empty() {
        if parsed.settings.is_some() {
            info!("Loaded settings from {}", path.display());
        }
        return Loaded {
            settings: restored.settings,
            save: restored.changed,
            read_only: false,
            notice: None,
        };
    }

    // Keep the user's file before the repaired settings replace it.
    warn!(
        "Settings at {} needed repair ({}); backing the file up",
        path.display(),
        repaired.join(", ")
    );
    match back_up(path, &bytes) {
        Ok(backup) => {
            warn!("Backed up the old settings to {}", backup.display());
            Loaded {
                settings: restored.settings,
                save: true,
                read_only: false,
                notice: Some(
                    Notice::new(NoticeCode::SettingsReset, NoticeSource::Settings).with_detail(
                        format!("{}. Backup: {}", repaired.join(", "), backup.display()),
                    ),
                ),
            }
        }
        Err(e) => {
            // Without a copy, writing would lose the original for good.
            error!("Couldn't back up the settings ({e}); leaving the file untouched");
            Loaded {
                settings: restored.settings,
                save: false,
                read_only: true,
                notice: Some(
                    Notice::new(NoticeCode::SettingsUnreadable, NoticeSource::Settings)
                        .with_detail(format!("{} ({e})", path.display())),
                ),
            }
        }
    }
}

/// `Ok(None)` when there's no file.
fn read_with_retries(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let mut attempt = 1;
    loop {
        match std::fs::read(path) {
            Ok(bytes) => return Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if attempt < READ_ATTEMPTS => {
                warn!(
                    "Reading settings at {} failed ({e}); retrying",
                    path.display()
                );
                attempt += 1;
                std::thread::sleep(RETRY_DELAY);
            }
            Err(e) => return Err(e),
        }
    }
}

struct ParsedFile {
    /// The stored `settings` value, if the file has one.
    settings: Option<serde_json::Value>,
    /// What had to be fixed to read it, for the notice.
    repaired: Vec<String>,
    /// Empty or whitespace: treat it like a missing file.
    file_was_empty: bool,
}

/// Reads the file's `settings` value as leniently as is safe: a UTF-8 byte
/// order mark is skipped and bytes that aren't UTF-8 are replaced, as some
/// editors leave them (Windows 10 Notepad saved "ANSI" by default, #2139).
fn parse_file(bytes: &[u8]) -> ParsedFile {
    let mut repaired = Vec::new();
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => std::borrow::Cow::Borrowed(text),
        Err(_) => {
            warn!("Settings file isn't valid UTF-8; replacing the invalid characters");
            repaired.push("text encoding".to_string());
            String::from_utf8_lossy(bytes)
        }
    };
    if text.trim().is_empty() {
        return ParsedFile {
            settings: None,
            repaired: Vec::new(),
            file_was_empty: true,
        };
    }

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut file)) => {
            let settings = file.remove("settings");
            ParsedFile {
                file_was_empty: file.is_empty() && settings.is_none(),
                settings,
                repaired,
            }
        }
        Ok(_) => {
            warn!("Settings file isn't a JSON object");
            ParsedFile {
                settings: None,
                repaired,
                file_was_empty: false,
            }
        }
        Err(e) => {
            warn!("Settings file isn't valid JSON: {e}");
            ParsedFile {
                settings: None,
                repaired,
                file_was_empty: false,
            }
        }
    }
}

/// Copies the file's bytes to `settings_store.json.<unix time>.bak` beside it.
fn back_up(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| SETTINGS_STORE_PATH.to_string());
    let backup = path.with_file_name(format!("{name}.{stamp}.bak"));
    write_synced(&backup, bytes)?;
    Ok(backup)
}

/// Writes the settings in the file's layout, replacing the old file in one
/// step: a temporary file beside it, synced to disk, then renamed over it.
fn save_file(path: &Path, settings: &AppSettings) -> std::io::Result<()> {
    let file = serde_json::json!({ "settings": settings });
    let bytes = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    write_synced(&tmp, &bytes)?;

    let mut attempt = 1;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < RENAME_ATTEMPTS => {
                debug!("Replacing {} failed ({e}); retrying", path.display());
                attempt += 1;
                std::thread::sleep(RETRY_DELAY);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn settings_file(settings: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec_pretty(&serde_json::json!({ "settings": settings })).unwrap()
    }

    fn stored_settings() -> serde_json::Value {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        stored["selected_model"] = serde_json::json!("parakeet-tdt-0.6b-v3");
        stored["onboarding_completed"] = serde_json::json!(true);
        stored["custom_words"] = serde_json::json!(["Handy", "廉政公署"]);
        stored["bindings"]["transcribe"]["current_binding"] = serde_json::json!("f9");
        stored
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "bak"))
            .collect()
    }

    fn assert_user_settings_kept(settings: &AppSettings) {
        assert_eq!(settings.selected_model, "parakeet-tdt-0.6b-v3");
        assert!(settings.onboarding_completed);
        assert_eq!(settings.bindings["transcribe"].current_binding, "f9");
    }

    #[test]
    fn missing_file_starts_fresh_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_file(&dir.path().join(SETTINGS_STORE_PATH));

        assert!(!loaded.settings.onboarding_completed);
        assert!(loaded.save);
        assert!(!loaded.read_only);
        assert!(loaded.notice.is_none());
    }

    #[test]
    fn valid_file_loads_without_rewriting_or_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        // Saved by this build, so nothing needs migrating.
        let mut settings = restore_settings(Some(&stored_settings())).settings;
        settings.custom_words = vec!["Handy".into(), "廉政公署".into()];
        save_file(&path, &settings).unwrap();

        let loaded = load_file(&path);

        assert_user_settings_kept(&loaded.settings);
        assert_eq!(loaded.settings.custom_words, settings.custom_words);
        assert!(!loaded.save, "an up-to-date file is left as it is");
        assert!(!loaded.read_only);
        assert!(loaded.notice.is_none());
        assert!(backups(dir.path()).is_empty());
    }

    #[test]
    fn unreadable_file_is_never_written() {
        // A directory where the file should be: it exists, but reading fails,
        // like a file another process holds locked (#2480).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        std::fs::create_dir(&path).unwrap();

        let loaded = load_file(&path);

        assert!(loaded.read_only);
        assert!(!loaded.save);
        assert_eq!(loaded.notice.unwrap().code, NoticeCode::SettingsUnreadable);
        assert!(path.is_dir(), "the original is left alone");
    }

    #[test]
    fn invalid_json_is_backed_up_before_defaults_replace_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        let original = b"{\"settings\": {\"selected_model\": \"kept\",".to_vec();
        std::fs::write(&path, &original).unwrap();

        let loaded = load_file(&path);

        assert!(loaded.save);
        assert!(!loaded.read_only);
        assert_eq!(loaded.notice.unwrap().code, NoticeCode::SettingsReset);
        let backups = backups(dir.path());
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
    }

    #[test]
    fn invalid_fields_reset_alone_and_are_named() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        let mut stored = stored_settings();
        stored["sound_theme"] = serde_json::json!("theremin");
        std::fs::write(&path, settings_file(stored)).unwrap();

        let loaded = load_file(&path);

        assert_user_settings_kept(&loaded.settings);
        let notice = loaded.notice.unwrap();
        assert_eq!(notice.code, NoticeCode::SettingsReset);
        assert!(notice.detail.unwrap().starts_with("sound_theme. Backup: "));
        assert_eq!(backups(dir.path()).len(), 1);
        assert!(loaded.save);
    }

    #[test]
    fn byte_order_mark_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend(settings_file(stored_settings()));
        std::fs::write(&path, bytes).unwrap();

        let loaded = load_file(&path);

        assert_user_settings_kept(&loaded.settings);
        assert!(loaded.notice.is_none());
    }

    #[test]
    fn non_utf8_text_keeps_the_settings() {
        // An editor saving in the system's legacy code page, as Notepad did by
        // default before Windows 10 1903, turns "café" into bytes that aren't
        // UTF-8 (here Windows-1252), which used to reset everything (#2139).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        let mut stored = stored_settings();
        stored["custom_words"] = serde_json::json!(["cafe"]);
        let text = String::from_utf8(settings_file(stored)).unwrap();
        let (before, after) = text.split_once("\"cafe\"").unwrap();
        let bytes = [before.as_bytes(), b"\"caf\xE9\"", after.as_bytes()].concat();
        std::fs::write(&path, &bytes).unwrap();

        let loaded = load_file(&path);

        assert_user_settings_kept(&loaded.settings);
        assert_eq!(
            loaded.settings.custom_words,
            vec!["caf\u{FFFD}".to_string()]
        );
        let notice = loaded.notice.unwrap();
        assert_eq!(notice.code, NoticeCode::SettingsReset);
        assert!(notice
            .detail
            .unwrap()
            .starts_with("text encoding. Backup: "));
        assert_eq!(std::fs::read(&backups(dir.path())[0]).unwrap(), bytes);
    }

    #[test]
    fn empty_file_starts_fresh_without_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_STORE_PATH);
        std::fs::write(&path, b"  \n").unwrap();

        let loaded = load_file(&path);

        assert!(loaded.notice.is_none());
        assert!(loaded.save);
        assert!(backups(dir.path()).is_empty());
    }

    #[test]
    fn saving_replaces_the_file_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(SETTINGS_STORE_PATH);
        let mut settings = get_default_settings();
        settings.selected_model = "first".into();
        save_file(&path, &settings).unwrap();
        settings.selected_model = "second".into();
        save_file(&path, &settings).unwrap();

        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from(SETTINGS_STORE_PATH)]);

        let file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(file["settings"]["selected_model"], "second");
        assert_eq!(load_file(&path).settings.selected_model, "second");
    }
}
