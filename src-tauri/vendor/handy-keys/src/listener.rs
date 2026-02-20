//! Keyboard listener for streaming raw key events
//!
//! This module provides a `KeyboardListener` that streams all keyboard events,
//! useful for implementing "record hotkey" UI flows.
//!
//! # Platform Notes
//!
//! - **macOS**: Uses CGEventTap. Requires accessibility permissions.
//! - **Windows**: Uses low-level keyboard hooks. Clean thread shutdown.
//! - **Linux**: Uses rdev. On Wayland, blocking may not work due to
//!   compositor restrictions. Thread cleanup is limited.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::types::KeyEvent;

pub use crate::platform::state::BlockingHotkeys;

/// Platform-agnostic Keyboard Listener
///
/// Streams all keyboard events. Can optionally block events that match
/// registered hotkeys.
pub struct KeyboardListener {
    event_receiver: Receiver<KeyEvent>,
    _thread_handle: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    blocking_hotkeys: Option<BlockingHotkeys>,
}

impl KeyboardListener {
    /// Create a new KeyboardListener (non-blocking mode)
    ///
    /// Events are observed but not blocked. Use this for "record hotkey" UI flows.
    ///
    /// On macOS, this will check for accessibility permissions and fail if not granted.
    pub fn new() -> Result<Self> {
        Self::new_internal(None)
    }

    /// Create a new KeyboardListener with blocking support
    ///
    /// Events matching hotkeys in the provided set will be blocked from reaching
    /// other applications. The set can be modified after creation to add/remove
    /// hotkeys dynamically.
    ///
    /// Note: On Wayland, blocking may not work due to compositor restrictions.
    pub fn new_with_blocking(blocking_hotkeys: BlockingHotkeys) -> Result<Self> {
        Self::new_internal(Some(blocking_hotkeys))
    }

    fn new_internal(blocking_hotkeys: Option<BlockingHotkeys>) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            use crate::platform::macos::listener;
            let state = listener::spawn(blocking_hotkeys)?;
            Ok(KeyboardListener {
                event_receiver: state.event_receiver,
                _thread_handle: state.thread_handle,
                running: state.running,
                blocking_hotkeys: state.blocking_hotkeys,
            })
        }

        #[cfg(target_os = "windows")]
        {
            use crate::platform::windows::listener;
            let state = listener::spawn(blocking_hotkeys)?;
            Ok(KeyboardListener {
                event_receiver: state.event_receiver,
                _thread_handle: state.thread_handle,
                running: state.running,
                blocking_hotkeys: state.blocking_hotkeys,
            })
        }

        #[cfg(target_os = "linux")]
        {
            use crate::platform::linux::listener;
            let state = listener::spawn(blocking_hotkeys)?;
            Ok(KeyboardListener {
                event_receiver: state.event_receiver,
                _thread_handle: state.thread_handle,
                running: state.running,
                blocking_hotkeys: state.blocking_hotkeys,
            })
        }
    }

    /// Get a reference to the blocking hotkeys set (if blocking is enabled)
    pub fn blocking_hotkeys(&self) -> Option<&BlockingHotkeys> {
        self.blocking_hotkeys.as_ref()
    }

    /// Blocking receive for key events
    ///
    /// Blocks until a key event is received or the listener stops.
    pub fn recv(&self) -> Result<KeyEvent> {
        self.event_receiver
            .recv()
            .map_err(|_| Error::EventLoopNotRunning)
    }

    /// Blocking receive with timeout
    ///
    /// Blocks until a key event is received, the timeout expires, or the listener stops.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<KeyEvent> {
        self.event_receiver
            .recv_timeout(timeout)
            .map_err(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => Error::Timeout,
                std::sync::mpsc::RecvTimeoutError::Disconnected => Error::EventLoopNotRunning,
            })
    }

    /// Non-blocking receive for key events
    ///
    /// Returns `Some(event)` if an event is available, `None` otherwise.
    pub fn try_recv(&self) -> Option<KeyEvent> {
        match self.event_receiver.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for KeyboardListener {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        // On macOS and Windows, we can join the thread for clean shutdown.
        // On Linux (rdev), the thread continues running but becomes idle
        // because rdev::grab() blocks indefinitely.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(handle) = self._thread_handle.take() {
            let _ = handle.join();
        }
    }
}
