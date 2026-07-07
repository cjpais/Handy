//! Cooperative cancellation for in-flight runs and streams.
//!
//! A [`CancelToken`] wraps a shared atomic flag. Install it on a session, then
//! call [`CancelToken::cancel`] from any thread to abort the in-flight
//! `run`/`feed`/`finalize`: the native abort callback (polled between decode
//! steps) sees the flag and stops, the call returns [`crate::Error::Aborted`]
//! with the partial transcript, and [`crate::Session::was_aborted`] reports
//! true.

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A clonable cancellation handle. Clones share one underlying flag, so a
/// worker thread can hold a clone and cancel a run executing on another thread.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    pub(crate) flag: Arc<AtomicBool>,
}

impl CancelToken {
    /// A fresh, un-cancelled token.
    pub fn new() -> Self {
        CancelToken::default()
    }

    /// Request cancellation of the run/stream this token is installed on.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Clear the cancelled state so the token can be reused for another run.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// The C abort callback. `userdata` is an `Arc<AtomicBool>`'s pointee, kept
/// alive by the session for the duration of any run. Invoked on the run thread
/// (the C library polls it between decode steps), so it must be cheap and
/// thread-safe — an atomic load is both.
pub(crate) extern "C" fn abort_trampoline(userdata: *mut c_void) -> bool {
    if userdata.is_null() {
        return false;
    }
    // SAFETY: `userdata` is the `*const AtomicBool` the session installed and
    // keeps alive (via a retained Arc) for as long as the callback is set.
    let flag = unsafe { &*(userdata as *const AtomicBool) };
    flag.load(Ordering::SeqCst)
}
