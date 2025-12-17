//! Operation Coordinator
//!
//! Manages the lifecycle of transcription operations to prevent race conditions
//! when rapidly toggling push-to-talk. Ensures only one operation is active at a time
//! and provides clean cancellation of stale operations.

use log::{debug, info, warn};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::AppHandle;

/// Represents the current phase of an operation
#[derive(Debug, Clone)]
pub enum OperationPhase {
    /// No operation in progress
    Idle,
    /// Currently recording audio
    Recording {
        operation_id: u64,
        binding_id: String,
        #[allow(dead_code)] // Useful for debugging/logging
        started_at: Instant,
    },
    /// Recording stopped, transcription in progress
    Processing {
        operation_id: u64,
        #[allow(dead_code)] // Useful for debugging/logging
        binding_id: String,
        #[allow(dead_code)] // Useful for debugging/logging
        started_at: Instant,
    },
}

impl OperationPhase {
    pub fn operation_id(&self) -> Option<u64> {
        match self {
            OperationPhase::Idle => None,
            OperationPhase::Recording { operation_id, .. } => Some(*operation_id),
            OperationPhase::Processing { operation_id, .. } => Some(*operation_id),
        }
    }

    #[allow(dead_code)] // Public API for future use
    pub fn is_idle(&self) -> bool {
        matches!(self, OperationPhase::Idle)
    }
}

/// Coordinates transcription operations to prevent race conditions.
///
/// The coordinator ensures:
/// 1. Only one operation can be active at a time
/// 2. Starting a new operation while one is in progress marks the old one as stale
/// 3. Operations can check if they're still valid before proceeding
/// 4. Clean state transitions with proper logging
pub struct OperationCoordinator {
    /// Monotonically increasing operation ID. Each new operation gets a unique ID.
    next_operation_id: AtomicU64,

    /// The currently active operation ID. Operations compare their ID to this
    /// to determine if they should continue or abort.
    active_operation_id: AtomicU64,

    /// The current phase of the operation lifecycle
    phase: Mutex<OperationPhase>,

    /// App handle for potential UI updates
    #[allow(dead_code)]
    app_handle: AppHandle,
}

impl OperationCoordinator {
    pub fn new(app_handle: &AppHandle) -> Self {
        Self {
            next_operation_id: AtomicU64::new(1),
            active_operation_id: AtomicU64::new(0),
            phase: Mutex::new(OperationPhase::Idle),
            app_handle: app_handle.clone(),
        }
    }

    /// Start a new recording operation.
    ///
    /// If an operation is already in progress, it will be marked as stale.
    /// Returns the operation ID that should be used to track this operation.
    pub fn start_recording(&self, binding_id: &str) -> u64 {
        let operation_id = self.next_operation_id.fetch_add(1, Ordering::SeqCst);
        let previous_active = self.active_operation_id.swap(operation_id, Ordering::SeqCst);

        let mut phase = self.phase.lock().unwrap();
        let previous_phase = phase.clone();

        *phase = OperationPhase::Recording {
            operation_id,
            binding_id: binding_id.to_string(),
            started_at: Instant::now(),
        };

        if previous_active != 0 {
            warn!(
                "Starting new operation {} while operation {} was still active (phase: {:?})",
                operation_id, previous_active, previous_phase
            );
        } else {
            debug!(
                "Started recording operation {} for binding '{}'",
                operation_id, binding_id
            );
        }

        operation_id
    }

    /// Transition from recording to processing phase.
    ///
    /// Returns true if the transition was successful (operation is still active).
    /// Returns false if the operation has been superseded.
    pub fn transition_to_processing(&self, operation_id: u64) -> bool {
        if !self.is_active(operation_id) {
            debug!(
                "Operation {} is no longer active, skipping transition to processing",
                operation_id
            );
            return false;
        }

        let mut phase = self.phase.lock().unwrap();

        // Verify we're transitioning from the right state
        match &*phase {
            OperationPhase::Recording {
                operation_id: phase_op_id,
                binding_id,
                ..
            } if *phase_op_id == operation_id => {
                *phase = OperationPhase::Processing {
                    operation_id,
                    binding_id: binding_id.clone(),
                    started_at: Instant::now(),
                };
                debug!("Operation {} transitioned to processing", operation_id);
                true
            }
            _ => {
                warn!(
                    "Cannot transition operation {} to processing from phase {:?}",
                    operation_id, *phase
                );
                false
            }
        }
    }

    /// Mark an operation as complete and return to idle state.
    ///
    /// Only completes if the given operation_id matches the active operation.
    pub fn complete(&self, operation_id: u64) {
        let was_active = self
            .active_operation_id
            .compare_exchange(operation_id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();

        if was_active {
            let mut phase = self.phase.lock().unwrap();
            if let Some(phase_op_id) = phase.operation_id() {
                if phase_op_id == operation_id {
                    info!("Operation {} completed successfully", operation_id);
                    *phase = OperationPhase::Idle;
                }
            }
        } else {
            debug!(
                "Operation {} was already superseded, not marking as complete",
                operation_id
            );
        }
    }

    /// Cancel the current operation and return to idle state.
    ///
    /// This is used for explicit cancellation (e.g., user pressing cancel).
    pub fn cancel(&self) {
        let previous_active = self.active_operation_id.swap(0, Ordering::SeqCst);

        let mut phase = self.phase.lock().unwrap();
        let previous_phase = phase.clone();
        *phase = OperationPhase::Idle;

        if previous_active != 0 {
            info!(
                "Cancelled operation {} (was in phase {:?})",
                previous_active, previous_phase
            );
        }
    }

    /// Check if the given operation is still the active operation.
    ///
    /// Operations should call this before performing significant work
    /// to avoid wasting resources on stale operations.
    pub fn is_active(&self, operation_id: u64) -> bool {
        self.active_operation_id.load(Ordering::SeqCst) == operation_id
    }

    /// Get the current phase of the operation lifecycle.
    #[allow(dead_code)] // Public API for debugging/monitoring
    pub fn current_phase(&self) -> OperationPhase {
        self.phase.lock().unwrap().clone()
    }

    /// Check if any operation is currently active.
    #[allow(dead_code)] // Public API for future use
    pub fn has_active_operation(&self) -> bool {
        self.active_operation_id.load(Ordering::SeqCst) != 0
    }

    /// Get the active operation ID, if any.
    pub fn active_operation_id(&self) -> Option<u64> {
        let id = self.active_operation_id.load(Ordering::SeqCst);
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }
}
