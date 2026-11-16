//! API keys of remote transcription servers, kept in the operating system's
//! credential store (Keychain on macOS, Credential Manager on Windows, the
//! Secret Service on Linux) instead of Handy's settings file. Settings only
//! record that a server has one.

use keyring::{Entry, Error};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const SERVICE: &str = "com.pais.handy.remote-transcription";

/// Runs `f` on a thread of its own. On Linux the Secret Service is reached
/// over D-Bus with a runtime of its own, which can't start on a thread that
/// is already inside one, and Handy calls this from such threads (a
/// dictation, Test Connection). A plain thread is safe on every platform.
fn isolated<T: Send + 'static>(
    server_id: &str,
    f: impl FnOnce(Entry) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let entry = Entry::new(SERVICE, server_id).map_err(|e| e.to_string())?;
    std::thread::Builder::new()
        .name("keychain".into())
        .spawn(move || f(entry))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the keychain thread panicked".to_string())?
}

/// Keys read this run, by server id, so each dictation doesn't go back to
/// the keychain (which can ask the user every time). Only Handy writes these
/// entries, through `store` and `remove`, which keep it up to date.
fn cache() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Saves `key` for the server, replacing any it had.
pub fn store(server_id: &str, key: &str) -> Result<(), String> {
    let stored = key.to_string();
    isolated(server_id, move |entry| {
        entry.set_password(&stored).map_err(|e| e.to_string())
    })?;
    cache().insert(server_id.to_string(), key.to_string());
    Ok(())
}

/// [`load`], from memory after the first time it was read.
pub fn load_cached(server_id: &str) -> Result<Option<String>, String> {
    if let Some(key) = cache().get(server_id) {
        return Ok(Some(key.clone()));
    }
    let key = load(server_id)?;
    if let Some(key) = &key {
        cache().insert(server_id.to_string(), key.clone());
    }
    Ok(key)
}

/// The server's key, or `None` when it has none.
pub fn load(server_id: &str) -> Result<Option<String>, String> {
    isolated(server_id, |entry| match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    })
}

/// Forgets the server's key. Having none already is fine.
pub fn remove(server_id: &str) -> Result<(), String> {
    cache().remove(server_id);
    isolated(server_id, |entry| match entry.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real credential store, so it runs only when asked for:
    /// `cargo test --lib keychain -- --ignored`.
    #[test]
    #[ignore]
    fn a_key_is_stored_replaced_and_forgotten() {
        let id = "handy-keychain-test";
        remove(id).unwrap();
        assert_eq!(load(id).unwrap(), None);

        store(id, "first").unwrap();
        assert_eq!(load(id).unwrap().as_deref(), Some("first"));
        store(id, "second ก").unwrap();
        assert_eq!(load(id).unwrap().as_deref(), Some("second ก"));

        // Read once, then from memory; a new key replaces it there too.
        assert_eq!(load_cached(id).unwrap().as_deref(), Some("second ก"));
        store(id, "third").unwrap();
        assert_eq!(load_cached(id).unwrap().as_deref(), Some("third"));

        remove(id).unwrap();
        assert_eq!(load(id).unwrap(), None);
        assert_eq!(load_cached(id).unwrap(), None);
        // Removing what isn't there is fine.
        remove(id).unwrap();
    }

    /// The way a dictation and Test Connection call it: from inside the
    /// app's async runtime.
    #[test]
    #[ignore]
    fn works_from_inside_the_runtime() {
        let id = "handy-keychain-runtime-test";
        let (tx, rx) = std::sync::mpsc::channel();
        tauri::async_runtime::spawn(async move {
            let result = store(id, "inside")
                .and_then(|()| load(id))
                .and_then(|key| remove(id).map(|()| key));
            let _ = tx.send(result);
        });
        let key = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the keychain call hung")
            .unwrap();
        assert_eq!(key.as_deref(), Some("inside"));
        assert_eq!(load(id).unwrap(), None);
    }
}
