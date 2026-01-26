//! Global lock preventing concurrent operations.

use std::sync::Mutex;

pub enum GlobalPhase {
    Idle,
    Recording,
    Processing,
}

pub struct GlobalController {
    phase: Mutex<GlobalPhase>,
}

impl GlobalController {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new(GlobalPhase::Idle),
        }
    }

    /// Acquire lock. Returns false if busy.
    pub fn begin(&self) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if matches!(*phase, GlobalPhase::Idle) {
            *phase = GlobalPhase::Recording;
            true
        } else {
            false
        }
    }

    /// Recording -> Processing.
    pub fn advance(&self) {
        let mut phase = self.phase.lock().unwrap();
        if matches!(*phase, GlobalPhase::Recording) {
            *phase = GlobalPhase::Processing;
        }
    }

    /// Release lock.
    pub fn complete(&self) {
        *self.phase.lock().unwrap() = GlobalPhase::Idle;
    }

    pub fn is_busy(&self) -> bool {
        !matches!(*self.phase.lock().unwrap(), GlobalPhase::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_idle() {
        let c = GlobalController::new();
        assert!(!c.is_busy());
    }

    #[test]
    fn begin_acquires_lock() {
        let c = GlobalController::new();
        assert!(c.begin());
        assert!(c.is_busy());
    }

    #[test]
    fn begin_fails_when_busy() {
        let c = GlobalController::new();
        assert!(c.begin());
        assert!(!c.begin());
    }

    #[test]
    fn complete_releases_lock() {
        let c = GlobalController::new();
        c.begin();
        c.complete();
        assert!(!c.is_busy());
    }

    #[test]
    fn full_lifecycle() {
        let c = GlobalController::new();
        assert!(c.begin());   // Idle -> Recording
        c.advance();          // Recording -> Processing
        c.complete();         // Processing -> Idle
        assert!(!c.is_busy());
        assert!(c.begin());   // Can start again
    }
}
