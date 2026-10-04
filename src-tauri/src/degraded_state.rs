//! Conditions under which Handy keeps working in a reduced way, such as
//! transcribing on CPU after a GPU crash.
//!
//! One atomic bitmask: several conditions can hold at once, and reading them
//! is a single atomic load from anywhere. Only the condition is recorded;
//! the details (which device, which signal, native output) go to the log when
//! a flag is set. How these surface to the user is decided later, with the
//! error system.

use log::{info, warn};
use std::fmt::Display;
use std::sync::atomic::{AtomicU32, Ordering};

static STATE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Degraded {
    /// Listing transcribe.cpp compute devices crashed, hung or failed.
    /// Models load on CPU and devices are not listed again until an explicit
    /// transcribe.cpp accelerator or GPU device change, or an app restart.
    DeviceProbeFailed = 1 << 0,
    /// A transcribe.cpp worker crashed or hung on a GPU. Models load on CPU
    /// until an explicit transcribe.cpp accelerator or GPU device change, or
    /// an app restart.
    GpuUnavailable = 1 << 1,
}

impl Degraded {
    pub fn is_set(self) -> bool {
        STATE.load(Ordering::Acquire) & self as u32 != 0
    }

    /// Set the flag. `reason` is logged when this changes the state.
    pub fn set(self, reason: impl Display) {
        if STATE.fetch_or(self as u32, Ordering::AcqRel) & self as u32 == 0 {
            warn!("Degraded state {:?} set: {}", self, reason);
        }
    }

    /// Clear the flag. `reason` is logged when this changes the state.
    pub fn clear(self, reason: impl Display) {
        if STATE.fetch_and(!(self as u32), Ordering::AcqRel) & self as u32 != 0 {
            info!("Degraded state {:?} cleared: {}", self, reason);
        }
    }
}
