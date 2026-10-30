use crate::input::{self, EnigoState};
use enigo::{Enigo, Keyboard};
use std::sync::{mpsc, Mutex, PoisonError};
use tauri::{AppHandle, Manager};

/// Serializes typing: two transcripts typed at once would interleave their
/// characters. Only taken by `paste`, which never runs on the main thread.
static PASTE_LOCK: Mutex<()> = Mutex::new(());

/// Runs `f` with the shared Enigo on the main thread (macOS TIS APIs require it)
/// and waits for the result. Runs inline if already on the main thread, so no deadlock.
fn with_enigo<T: Send + 'static>(
    app_handle: &AppHandle,
    f: impl FnOnce(&mut Enigo) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    let app = app_handle.clone();
    app_handle
        .run_on_main_thread(move || {
            let result = (|| {
                // The frontend creates this after its permission screen, but
                // typing must not depend on that screen having been reached:
                // transcripts arrive from the tray and the CLI too. Creating it
                // here is safe because this already runs on the main thread.
                if app.try_state::<EnigoState>().is_none() {
                    app.manage(EnigoState::new()?);
                }
                let enigo_state = app
                    .try_state::<EnigoState>()
                    .ok_or("Enigo state unavailable")?;
                let mut enigo = enigo_state
                    .0
                    .lock()
                    .map_err(|e| format!("Failed to lock Enigo: {}", e))?;
                f(&mut enigo)
            })();
            let _ = tx.send(result);
        })
        .map_err(|e| format!("Failed to run input on the main thread: {}", e))?;
    // A dropped closure (event loop shutting down) closes the channel.
    rx.recv()
        .map_err(|_| "Main thread dropped the input request".to_string())?
}

/// The process ID of the app in front. Call on the main thread.
#[cfg(target_os = "macos")]
fn front_app_pid() -> Option<i32> {
    let app = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(app.processIdentifier())
}

/// Not checked on other platforms, so typing never stops for this reason.
#[cfg(not(target_os = "macos"))]
fn front_app_pid() -> Option<i32> {
    None
}

/// Types text directly by simulating individual key presses.
fn paste_direct(text: &str, app_handle: &AppHandle) -> Result<(), String> {
    // Keys go to whatever is in front. A live test typed the second half of a
    // reply into another window after the user clicked away, so typing stops
    // as soon as a different app comes to the front.
    let front_app = with_enigo(app_handle, |_| Ok(front_app_pid()))?;

    // The waiting and the pressing have to happen on different threads. Pacing
    // sleeps once per character, so doing that on the main thread would freeze
    // the window for as long as the transcript takes to type; but enigo's macOS
    // backend traps when it is driven from anywhere else. So this worker thread
    // keeps the rhythm and hands each individual key press to the main thread,
    // where each one costs microseconds.
    input::type_text_as_keystrokes(
        |stroke| {
            with_enigo(app_handle, move |enigo| {
                if front_app_pid() != front_app {
                    return Err("another app came to the front, so typing stopped".to_string());
                }
                match stroke {
                    input::Stroke::Key(key, direction) => enigo.key(key, direction),
                    input::Stroke::Text(character) => enigo.text(&character.to_string()),
                }
                .map_err(|e| format!("{}", e))
            })
        },
        text,
        input::TYPING_KEY_DELAY_MS,
    )
}

/// Types `text` into the focused app, one key press per character. Call from a
/// worker thread, never the main thread: it sleeps between keys (input
/// injection hops via `with_enigo`).
pub fn paste(text: String, app_handle: AppHandle) -> Result<(), String> {
    let _paste = PASTE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    paste_direct(&text, &app_handle)
}
