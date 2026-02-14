use std::sync::atomic::{AtomicU8, Ordering};

/// Lifecycle state for the transcription pipeline.
///
/// All entry points (SIGUSR2 signal handler, keyboard shortcuts) check this
/// before starting or stopping to prevent races where a new recording begins
/// while the async transcribe → paste pipeline is still running.
pub struct TranscriptionState(AtomicU8);

impl TranscriptionState {
    pub const IDLE: u8 = 0;
    pub const RECORDING: u8 = 1;
    pub const PROCESSING: u8 = 2;

    pub fn new() -> Self {
        Self(AtomicU8::new(Self::IDLE))
    }

    /// Try to transition Idle → Recording. Returns false if not idle.
    pub fn try_start(&self) -> bool {
        self.0
            .compare_exchange(
                Self::IDLE,
                Self::RECORDING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Try to transition Recording → Processing. Returns false if not recording.
    pub fn try_stop(&self) -> bool {
        self.0
            .compare_exchange(
                Self::RECORDING,
                Self::PROCESSING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Reset to Idle from any state. Called when the async pipeline finishes
    /// or on cancellation.
    pub fn reset(&self) {
        self.0.store(Self::IDLE, Ordering::SeqCst);
    }

    pub fn current(&self) -> u8 {
        self.0.load(Ordering::SeqCst)
    }
}
