//! The operating system's credential store: Keychain on macOS, Credential
//! Manager on Windows, the Secret Service (GNOME Keyring, KWallet, KeePassXC)
//! on Linux.
//!
//! Every call runs on one long-lived thread of its own, for three reasons:
//! - The Secret Service is reached over D-Bus, which must not run on a thread
//!   that is inside Handy's async runtime.
//! - A locked Linux keyring with nothing to show its unlock prompt never
//!   answers. Callers give up after [`TIMEOUT`] instead of hanging a dictation.
//! - One call at a time means one unlock prompt, not one per caller.
//!
//! A panic in the store is caught and returned as an error, so a broken
//! keyring can't take Handy down with it.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// The service every entry is filed under: the app's bundle identifier.
const SERVICE: &str = "com.pais.handy";

/// How long a caller waits for the store. Long enough to type a password into
/// an unlock prompt; a call that answers later still updates the cache.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A place secrets can be kept. [`OsKeychain`] is the real one; tests use
/// their own.
pub trait SecretStore: Sync {
    /// The secret saved under `account`, or `None` when there is none.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Saves `secret` under `account`, replacing any it had.
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    /// Removes the secret under `account`. Having none already is fine.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The operating system's credential store.
pub struct OsKeychain;

impl SecretStore for OsKeychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        if let Some(secret) = cache().get(account) {
            return Ok(Some(secret.clone()));
        }
        let account = account.to_string();
        run(move || {
            let secret = match entry(&account)?.get_password() {
                Ok(secret) => Some(secret),
                Err(keyring::Error::NoEntry) => None,
                Err(e) => return Err(e.to_string()),
            };
            if let Some(secret) = &secret {
                cache().insert(account, secret.clone());
            }
            Ok(secret)
        })
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        let account = account.to_string();
        let secret = secret.to_string();
        run(move || {
            // Forget the old value first: if this write fails, the store may
            // hold either value, and the next read should ask it.
            cache().remove(&account);
            entry(&account)?
                .set_password(&secret)
                .map_err(|e| e.to_string())?;
            cache().insert(account, secret);
            Ok(())
        })
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let account = account.to_string();
        run(move || {
            cache().remove(&account);
            match entry(&account)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        })
    }
}

/// keyring's errors never include the secret, so their text is safe to log.
fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

/// Secrets read or written this run, by account, so a dictation doesn't go
/// back to the store (and maybe an unlock prompt) every time. Only the
/// keychain thread fills it.
fn cache() -> MutexGuard<'static, HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Forgets what was read, so the next read goes to the store.
#[cfg(test)]
pub(crate) fn clear_cache() {
    cache().clear();
}

type Job = Box<dyn FnOnce() + Send>;

/// Runs `job` on the keychain thread and waits up to [`TIMEOUT`] for it.
fn run<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    run_within(TIMEOUT, job)
}

fn run_within<T: Send + 'static>(
    timeout: Duration,
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    // A call that never answered holds up every call behind it: fail those
    // at once instead of making each wait out its own timeout.
    if stuck_call()
        .as_ref()
        .is_some_and(|done| !done.load(Ordering::SeqCst))
    {
        return Err("the system keychain is still waiting on an earlier request".to_string());
    }
    let done = Arc::new(AtomicBool::new(false));
    let job_done = done.clone();
    let (reply, answer) = mpsc::channel();
    let job: Job = Box::new(move || {
        let result = catch_unwind(AssertUnwindSafe(job))
            .unwrap_or_else(|_| Err("the system keychain crashed".to_string()));
        job_done.store(true, Ordering::SeqCst);
        // The caller may have stopped waiting; that's fine.
        let _ = reply.send(result);
    });
    worker()
        .send(job)
        .map_err(|_| "the keychain thread is gone".to_string())?;
    match answer.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            *stuck_call() = Some(done);
            Err(format!(
                "the system keychain didn't answer within {} seconds",
                timeout.as_secs()
            ))
        }
        Err(RecvTimeoutError::Disconnected) => Err("the keychain thread is gone".to_string()),
    }
}

/// The last call that timed out, and whether it has finished since.
fn stuck_call() -> MutexGuard<'static, Option<Arc<AtomicBool>>> {
    static STUCK: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
    STUCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The keychain thread's queue, starting the thread on first use.
fn worker() -> Sender<Job> {
    static WORKER: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (sender, jobs) = mpsc::channel::<Job>();
            let spawned = std::thread::Builder::new()
                .name("keychain".into())
                .spawn(move || {
                    for job in jobs {
                        job();
                    }
                });
            if let Err(e) = spawned {
                // The queue's receiver is dropped with the closure, so every
                // call reports the thread as gone instead of waiting.
                log::error!("Couldn't start the keychain thread: {e}");
            }
            Mutex::new(sender)
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keychain thread is shared, so tests that hold it up run alone.
    fn serial() -> MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_panic_in_the_store_is_an_error_and_the_thread_survives() {
        let _serial = serial();
        let result: Result<(), String> = run(|| panic!("boom"));
        assert_eq!(result, Err("the system keychain crashed".to_string()));
        assert_eq!(run(|| Ok(7)), Ok(7));
    }

    #[test]
    fn a_store_that_never_answers_times_out_and_holds_up_no_one() {
        let _serial = serial();
        let (release, released) = mpsc::channel::<()>();
        let result: Result<(), String> = run_within(Duration::from_millis(50), move || {
            let _ = released.recv();
            Ok(())
        });
        assert!(result.unwrap_err().contains("didn't answer"));

        // Calls behind it fail at once instead of waiting out a timeout.
        let started = std::time::Instant::now();
        let result: Result<(), String> = run_within(Duration::from_secs(5), || Ok(()));
        assert!(result.unwrap_err().contains("still waiting"));
        assert!(started.elapsed() < Duration::from_secs(1));

        // Once it answers, the store is used again.
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match run_within(Duration::from_secs(5), || Ok(1)) {
                Ok(n) => break assert_eq!(n, 1),
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    /// The real store. Touches the system keychain, so it runs only when
    /// asked: `cargo test --lib keychain -- --ignored --test-threads=1`.
    #[test]
    #[ignore]
    fn the_os_keychain_stores_replaces_and_removes() {
        let _serial = serial();
        let store = OsKeychain;
        let account = "test/handy-keychain-roundtrip";
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);

        store.set(account, "first").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("first"));

        // Unicode, and longer than any real API key.
        let long = format!("sk-ก✓{}", "x".repeat(1000));
        store.set(account, &long).unwrap();
        cache().clear();
        assert_eq!(store.get(account).unwrap(), Some(long));

        store.delete(account).unwrap();
        cache().clear();
        assert_eq!(store.get(account).unwrap(), None);
        store.delete(account).unwrap();
    }

    /// The way a dictation calls it: from inside the app's async runtime.
    #[test]
    #[ignore]
    fn the_os_keychain_works_from_inside_the_async_runtime() {
        let _serial = serial();
        let account = "test/handy-keychain-runtime";
        let result = tauri::async_runtime::block_on(async move {
            tauri::async_runtime::spawn(async move {
                let store = OsKeychain;
                store.set(account, "inside")?;
                cache().clear();
                let read = store.get(account)?;
                store.delete(account)?;
                Ok::<_, String>(read)
            })
            .await
            .unwrap()
        });
        assert_eq!(result.unwrap().as_deref(), Some("inside"));
    }
}
