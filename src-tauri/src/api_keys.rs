//! Post-processing API keys: saved in the system keychain, read back when a
//! request needs one.
//!
//! The settings file records only that a provider has a key
//! (`post_process_saved_api_keys`: where it is and its last characters). The
//! key itself goes to the keychain. It stays in the settings file
//! (`post_process_api_keys`) in two cases:
//! - portable mode, where everything Handy keeps travels with its folder;
//! - no usable keychain, such as a Linux desktop without a Secret Service.
//!   The settings page says so, and every launch tries to move it again.
//!
//! A key in the settings file is always the current one: a key older Handy
//! versions saved there, or one saved after a downgrade, replaces whatever the
//! keychain holds the next time it's moved.

use crate::keychain::{OsKeychain, SecretStore};
use crate::settings::{self, ApiKeyStorage, AppSettings, SavedApiKey};
use log::{info, warn};
use tauri::AppHandle;

/// The keychain account a provider's key is filed under.
fn account(provider_id: &str) -> String {
    format!("post-processing/{provider_id}")
}

/// The last four characters of a key long enough to spare them, so people can
/// tell which key they saved.
fn hint(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        return String::new();
    }
    chars[chars.len() - 4..].iter().collect()
}

fn keychain_enabled() -> bool {
    !crate::portable::is_portable()
}

/// The provider's key, or `None` when it has none. Reads the keychain (from
/// memory after the first time), so call it off the async runtime's threads.
pub fn load(settings: &AppSettings, provider_id: &str) -> Result<Option<String>, String> {
    load_from(settings, &OsKeychain, provider_id)
}

/// [`load`], for async callers.
pub async fn load_async(
    settings: &AppSettings,
    provider_id: &str,
) -> Result<Option<String>, String> {
    let settings = settings.clone();
    let provider_id = provider_id.to_string();
    tauri::async_runtime::spawn_blocking(move || load(&settings, &provider_id))
        .await
        .map_err(|e| e.to_string())?
}

fn load_from(
    settings: &AppSettings,
    store: &dyn SecretStore,
    provider_id: &str,
) -> Result<Option<String>, String> {
    if let Some(key) = file_key(settings, provider_id) {
        return Ok(Some(key.to_string()));
    }
    match settings.post_process_saved_api_keys.get(provider_id) {
        Some(saved) if saved.storage == ApiKeyStorage::Keychain => {
            match store.get(&account(provider_id)) {
                Ok(Some(key)) => Ok(Some(key)),
                Ok(None) => Err(
                    "the API key is missing from the system keychain; enter it again".to_string(),
                ),
                Err(e) => Err(format!(
                    "couldn't read the API key from the system keychain: {e}"
                )),
            }
        }
        _ => Ok(None),
    }
}

fn file_key<'a>(settings: &'a AppSettings, provider_id: &str) -> Option<&'a str> {
    settings
        .post_process_api_keys
        .get(provider_id)
        .map(String::as_str)
        .filter(|key| !key.is_empty())
}

/// Saves the provider's key, or removes it when `key` is empty.
pub fn save(app: &AppHandle, provider_id: &str, key: &str) {
    // The keychain can take a while (an unlock prompt), so settings are read
    // only after it answered, and written right away.
    let stored = store_key(&OsKeychain, keychain_enabled(), provider_id, key);
    let mut settings = settings::get_settings(app);
    record(&mut settings, provider_id, key, stored);
    settings::write_settings(app, settings);
}

/// [`save`] without an app, for tests against the real keychain. Fails
/// instead of falling back to the settings file.
#[cfg(test)]
pub(crate) fn save_with_os_keychain(
    settings: &mut AppSettings,
    provider_id: &str,
    key: &str,
) -> Result<(), String> {
    let stored = store_key(&OsKeychain, true, provider_id, key);
    if stored == Stored::SettingsFile {
        return Err("the system keychain refused the key".to_string());
    }
    record(settings, provider_id, key, stored);
    Ok(())
}

/// Where [`store_key`] put a key.
#[derive(Debug, PartialEq, Eq)]
enum Stored {
    Removed,
    Keychain,
    SettingsFile,
}

fn store_key(store: &dyn SecretStore, use_keychain: bool, provider_id: &str, key: &str) -> Stored {
    if key.is_empty() {
        // Gone from the settings file either way; a keychain entry that
        // can't be removed now is overwritten by the next key saved.
        if use_keychain {
            if let Err(e) = store.delete(&account(provider_id)) {
                warn!("Couldn't remove the '{provider_id}' API key from the system keychain: {e}");
            }
        }
        return Stored::Removed;
    }
    if !use_keychain {
        return Stored::SettingsFile;
    }
    match store.set(&account(provider_id), key) {
        Ok(()) => Stored::Keychain,
        Err(e) => {
            warn!(
                "Couldn't save the '{provider_id}' API key in the system keychain, keeping it in the settings file: {e}"
            );
            Stored::SettingsFile
        }
    }
}

fn record(settings: &mut AppSettings, provider_id: &str, key: &str, stored: Stored) {
    let saved = |storage| SavedApiKey {
        storage,
        hint: hint(key),
    };
    match stored {
        Stored::Removed => {
            settings.post_process_api_keys.remove(provider_id);
            settings.post_process_saved_api_keys.remove(provider_id);
        }
        Stored::Keychain => {
            settings.post_process_api_keys.remove(provider_id);
            settings
                .post_process_saved_api_keys
                .insert(provider_id.to_string(), saved(ApiKeyStorage::Keychain));
        }
        Stored::SettingsFile => {
            settings
                .post_process_api_keys
                .insert(provider_id.to_string(), key.to_string());
            settings
                .post_process_saved_api_keys
                .insert(provider_id.to_string(), saved(ApiKeyStorage::SettingsFile));
        }
    }
}

/// Makes `post_process_saved_api_keys` describe the keys in the settings
/// file: a key there is the current one. Returns whether anything changed.
/// Runs on every settings read, so keys written by older versions show up.
pub fn reconcile(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    let file_keys: Vec<(String, String)> = settings
        .post_process_api_keys
        .iter()
        .filter(|(_, key)| !key.is_empty())
        .map(|(id, key)| (id.clone(), key.clone()))
        .collect();
    for (provider_id, key) in &file_keys {
        let saved = SavedApiKey {
            storage: ApiKeyStorage::SettingsFile,
            hint: hint(key),
        };
        if settings.post_process_saved_api_keys.get(provider_id) != Some(&saved) {
            settings
                .post_process_saved_api_keys
                .insert(provider_id.clone(), saved);
            changed = true;
        }
    }
    let before = settings.post_process_saved_api_keys.len();
    settings
        .post_process_saved_api_keys
        .retain(|provider_id, saved| {
            saved.storage == ApiKeyStorage::Keychain
                || file_keys.iter().any(|(id, _)| id == provider_id)
        });
    changed || settings.post_process_saved_api_keys.len() != before
}

/// Moves the keys kept in the settings file into the keychain, when there is
/// one. Run at startup, off the main thread: it can wait on an unlock prompt.
pub fn move_to_keychain(app: &AppHandle) {
    if !keychain_enabled() {
        return;
    }
    let moved = move_keys(&OsKeychain, &settings::get_settings(app));
    if moved.is_empty() {
        return;
    }
    let mut settings = settings::get_settings(app);
    for (provider_id, key) in &moved {
        // Skip a key that changed while it was being moved.
        if file_key(&settings, provider_id) == Some(key.as_str()) {
            record(&mut settings, provider_id, key, Stored::Keychain);
            info!("Moved the '{provider_id}' API key into the system keychain");
        }
    }
    settings::write_settings(app, settings);
}

/// Saves each key in the settings file to the keychain and reads it back.
/// Returns the keys that made it, by provider.
fn move_keys(store: &dyn SecretStore, settings: &AppSettings) -> Vec<(String, String)> {
    let mut moved = Vec::new();
    let mut keys: Vec<(&String, &String)> = settings
        .post_process_api_keys
        .iter()
        .filter(|(_, key)| !key.is_empty())
        .collect();
    keys.sort();
    for (provider_id, key) in keys {
        let account = account(provider_id);
        let result = store
            .set(&account, key)
            .and_then(|()| store.get(&account))
            .and_then(|read| match read {
                Some(read) if read == *key => Ok(()),
                _ => Err("it read back differently".to_string()),
            });
        match result {
            Ok(()) => moved.push((provider_id.clone(), key.clone())),
            Err(e) => {
                warn!("Couldn't move the '{provider_id}' API key into the system keychain, keeping it in the settings file: {e}");
                // Without a keychain the rest would fail the same way.
                break;
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// An in-memory keychain that can be switched off.
    #[derive(Default)]
    struct FakeStore {
        secrets: Mutex<HashMap<String, String>>,
        broken: bool,
    }

    impl FakeStore {
        fn broken() -> Self {
            Self {
                broken: true,
                ..Default::default()
            }
        }

        fn secret(&self, provider_id: &str) -> Option<String> {
            self.secrets
                .lock()
                .unwrap()
                .get(&account(provider_id))
                .cloned()
        }
    }

    impl SecretStore for FakeStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            if self.broken {
                return Err("no keychain".into());
            }
            Ok(self.secrets.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            self.secrets
                .lock()
                .unwrap()
                .insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            if self.broken {
                return Err("no keychain".into());
            }
            self.secrets.lock().unwrap().remove(account);
            Ok(())
        }
    }

    const KEY: &str = "sk-proj-0123456789abcd";

    fn save_with(
        store: &dyn SecretStore,
        use_keychain: bool,
        settings: &mut AppSettings,
        provider_id: &str,
        key: &str,
    ) {
        let stored = store_key(store, use_keychain, provider_id, key);
        record(settings, provider_id, key, stored);
    }

    fn saved(settings: &AppSettings, provider_id: &str) -> Option<SavedApiKey> {
        settings
            .post_process_saved_api_keys
            .get(provider_id)
            .cloned()
    }

    #[test]
    fn a_key_goes_to_the_keychain_and_not_the_settings_file() {
        let store = FakeStore::default();
        let mut settings = get_default_settings();
        save_with(&store, true, &mut settings, "openai", KEY);

        assert_eq!(store.secret("openai").as_deref(), Some(KEY));
        assert!(!serde_json::to_string(&settings).unwrap().contains(KEY));
        assert_eq!(
            saved(&settings, "openai"),
            Some(SavedApiKey {
                storage: ApiKeyStorage::Keychain,
                hint: "abcd".into()
            })
        );
        assert_eq!(
            load_from(&settings, &store, "openai").unwrap().as_deref(),
            Some(KEY)
        );
    }

    #[test]
    fn without_a_keychain_the_key_stays_in_the_settings_file() {
        let store = FakeStore::broken();
        let mut settings = get_default_settings();
        save_with(&store, true, &mut settings, "openai", KEY);

        assert_eq!(
            saved(&settings, "openai").unwrap().storage,
            ApiKeyStorage::SettingsFile
        );
        assert_eq!(
            load_from(&settings, &store, "openai").unwrap().as_deref(),
            Some(KEY)
        );
    }

    #[test]
    fn portable_mode_keeps_keys_in_the_settings_file() {
        let store = FakeStore::default();
        let mut settings = get_default_settings();
        save_with(&store, false, &mut settings, "openai", KEY);

        assert_eq!(store.secret("openai"), None);
        assert_eq!(
            saved(&settings, "openai").unwrap().storage,
            ApiKeyStorage::SettingsFile
        );
    }

    #[test]
    fn an_empty_key_removes_it_everywhere() {
        let store = FakeStore::default();
        let mut settings = get_default_settings();
        save_with(&store, true, &mut settings, "openai", KEY);
        save_with(&store, true, &mut settings, "openai", "");

        assert_eq!(store.secret("openai"), None);
        assert_eq!(saved(&settings, "openai"), None);
        assert_eq!(load_from(&settings, &store, "openai").unwrap(), None);
    }

    #[test]
    fn a_key_missing_from_the_keychain_is_an_error_not_an_empty_key() {
        let store = FakeStore::default();
        let mut settings = get_default_settings();
        save_with(&store, true, &mut settings, "openai", KEY);
        store.secrets.lock().unwrap().clear();

        let error = load_from(&settings, &store, "openai").unwrap_err();
        assert!(error.contains("enter it again"), "{error}");
        assert!(load_from(&settings, &FakeStore::broken(), "openai").is_err());
    }

    #[test]
    fn a_provider_without_a_key_has_none() {
        let settings = get_default_settings();
        assert_eq!(
            load_from(&settings, &FakeStore::broken(), "custom").unwrap(),
            None
        );
    }

    #[test]
    fn keys_from_older_versions_are_recorded_then_moved_to_the_keychain() {
        let mut settings = get_default_settings();
        settings
            .post_process_api_keys
            .insert("openai".into(), KEY.into());
        settings
            .post_process_api_keys
            .insert("groq".into(), "gsk-short".into());

        assert!(reconcile(&mut settings));
        assert!(!reconcile(&mut settings), "reconcile settles");
        assert_eq!(
            saved(&settings, "groq"),
            Some(SavedApiKey {
                storage: ApiKeyStorage::SettingsFile,
                hint: String::new()
            })
        );

        let store = FakeStore::default();
        let moved = move_keys(&store, &settings);
        assert_eq!(moved.len(), 2);
        for (provider_id, key) in &moved {
            record(&mut settings, provider_id, key, Stored::Keychain);
        }
        assert!(!serde_json::to_string(&settings).unwrap().contains(KEY));
        assert!(!reconcile(&mut settings));
        assert_eq!(
            load_from(&settings, &store, "groq").unwrap().as_deref(),
            Some("gsk-short")
        );
        assert_eq!(
            saved(&settings, "openai").unwrap().storage,
            ApiKeyStorage::Keychain
        );
    }

    #[test]
    fn a_failed_move_keeps_every_key_in_the_settings_file() {
        let mut settings = get_default_settings();
        settings
            .post_process_api_keys
            .insert("openai".into(), KEY.into());
        reconcile(&mut settings);

        assert!(move_keys(&FakeStore::broken(), &settings).is_empty());
        assert_eq!(
            load_from(&settings, &FakeStore::broken(), "openai")
                .unwrap()
                .as_deref(),
            Some(KEY)
        );
    }

    #[test]
    fn a_key_in_the_settings_file_wins_over_the_keychain() {
        // Saved in the keychain, then a downgraded Handy wrote a new key
        // to the settings file.
        let store = FakeStore::default();
        let mut settings = get_default_settings();
        save_with(&store, true, &mut settings, "openai", KEY);
        settings
            .post_process_api_keys
            .insert("openai".into(), "sk-newer-key-from-downgrade".into());

        assert!(reconcile(&mut settings));
        assert_eq!(
            saved(&settings, "openai").unwrap().storage,
            ApiKeyStorage::SettingsFile
        );
        assert_eq!(
            load_from(&settings, &store, "openai").unwrap().as_deref(),
            Some("sk-newer-key-from-downgrade")
        );
    }

    #[test]
    fn reconcile_drops_records_of_keys_that_are_gone() {
        let mut settings = get_default_settings();
        settings.post_process_saved_api_keys.insert(
            "openai".into(),
            SavedApiKey {
                storage: ApiKeyStorage::SettingsFile,
                hint: String::new(),
            },
        );
        assert!(reconcile(&mut settings));
        assert_eq!(saved(&settings, "openai"), None);
    }

    #[test]
    fn hints_show_only_the_end_of_long_keys() {
        assert_eq!(hint("sk-proj-0123456789abcd"), "abcd");
        assert_eq!(hint("short-key"), "");
        assert_eq!(hint("ключ-ключ-ключ"), "ключ");
    }
}
