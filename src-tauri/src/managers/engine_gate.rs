//! Single admission authority for the transcription engine.
//!
//! Every operation that uses or mutates the loaded engine — inference, model
//! load/switch/unload, streaming — must hold an [`EngineReservation`]. The
//! reservation is issued only by
//! [`TranscriptionManager`](crate::managers::transcription::TranscriptionManager)
//! (`try_reserve` / `reserve_dictation`), is validated against the issuing
//! gate on use, and is released when its last clone drops. Background jobs
//! (retry, IPC, maintenance) get an immediate typed [`EngineBusy`] instead of
//! queueing.
//!
//! # Dictation policy: capture first, engine second
//!
//! Handy is dictation-first, so the gate must never prevent the user's speech
//! from being captured. [`EngineGate::reserve_dictation`] therefore never
//! fails while a background job runs:
//!
//! - Gate free → the dictation owns the engine immediately (model preload and
//!   live streaming may begin under the reservation).
//! - Gate held → the dictation registers in the single **pending slot** and
//!   records anyway (without live streaming). When the current holder's last
//!   clone drops, ownership transfers to the pending dictation *atomically
//!   under the gate lock* — a concurrent `try_reserve` can never jump the
//!   queue, so interactive priority is guaranteed, not just likely. At
//!   recording stop the dictation waits (bounded) via
//!   [`EngineReservation::wait_active_cancellable`] only for the job that was
//!   already running when it started.
//!
//! The pending slot is deliberately a one-item, dictation-only queue: the
//! [`TranscriptionCoordinator`](crate::TranscriptionCoordinator) serializes
//! dictation sessions, so at most one can be pending, and background jobs are
//! never queued at all.
//!
//! # What the reservation does and does not prove
//!
//! A reservation serializes *logical jobs*: no two jobs (dictation session,
//! retry, IPC call, maintenance) can use the engine concurrently. It does not
//! prove per-call exclusivity *within* a job — reservations are cloneable so
//! one dictation can span the coordinator, the model preload thread, the
//! streaming worker, and the stop pipeline. Sequencing inside a job remains
//! the responsibility of the engine mutex, the `is_loading` condvar, and the
//! stream-worker state in `TranscriptionManager`, exactly as before.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The kind of job holding (or requesting) the engine, used for busy
/// diagnostics and error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineJobKind {
    /// A live dictation session: reserved at recording start, held through
    /// model preload, streaming, and the final batch/finalize step.
    Dictation,
    /// Retranscription of a history entry from the UI.
    Retry,
    /// A `transcribe.file` request from the local IPC server.
    Ipc,
    /// The one-shot headless `--transcribe-file` CLI path.
    Cli,
    /// Housekeeping that mutates the loaded engine outside a transcription:
    /// idle unload, manual unload, model switching.
    Maintenance,
}

impl fmt::Display for EngineJobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EngineJobKind::Dictation => "dictation",
            EngineJobKind::Retry => "a history retranscription",
            EngineJobKind::Ipc => "an IPC transcription",
            EngineJobKind::Cli => "a command-line transcription",
            EngineJobKind::Maintenance => "engine maintenance",
        };
        f.write_str(s)
    }
}

/// Typed "engine is busy" failure carrying who holds the reservation, so each
/// entry point can map it to its own UX (IPC → error 1000, retry/maintenance →
/// toast, dictation stop → transcription-error event).
#[derive(Debug)]
pub struct EngineBusy {
    pub holder: EngineJobKind,
}

impl fmt::Display for EngineBusy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the transcription engine is busy with {}", self.holder)
    }
}

impl std::error::Error for EngineBusy {}

#[derive(Default)]
struct GateState {
    /// The job currently owning the engine: `(token, kind)`. `None` = free.
    holder: Option<(u64, EngineJobKind)>,
    /// Token of the dictation waiting for handoff. Invariant: `Some` implies
    /// `holder` is `Some` — release transfers ownership atomically, so the
    /// gate is never observed free while a dictation is pending.
    pending_dictation: Option<u64>,
}

/// Mutual exclusion + one-slot dictation handoff for engine jobs. Constructed
/// only by `TranscriptionManager`; see the module docs for the contract.
pub struct EngineGate {
    state: Mutex<GateState>,
    /// Notified on every release/handoff; `wait_active_cancellable` blocks on
    /// this.
    changed: Condvar,
    next_token: AtomicU64,
}

impl EngineGate {
    /// Managers-only: callers obtain reservations through
    /// `TranscriptionManager`, never by constructing their own gate (a foreign
    /// gate's reservation is rejected by `belongs_to` validation).
    pub(in crate::managers) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GateState::default()),
            changed: Condvar::new(),
            next_token: AtomicU64::new(1),
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, GateState> {
        // The critical sections here are tiny assignments that cannot panic,
        // but recover from poison anyway: release() runs inside Drop, where a
        // panic would abort.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve the engine for a background job. Never waits and never queues:
    /// a held gate (or a pending dictation, which implies a held gate) yields
    /// an immediate [`EngineBusy`].
    pub(in crate::managers) fn try_reserve(
        self: &Arc<Self>,
        kind: EngineJobKind,
    ) -> Result<EngineReservation, EngineBusy> {
        let mut state = self.lock_state();
        if let Some((_, holder_kind)) = state.holder {
            return Err(EngineBusy {
                holder: holder_kind,
            });
        }
        // Unreachable if the transfer invariant holds (pending implies held);
        // if it ever doesn't, refuse — a spurious `busy` beats stealing a
        // pending dictation's handoff.
        if state.pending_dictation.is_some() {
            debug_assert!(false, "pending dictation without a holder");
            return Err(EngineBusy {
                holder: EngineJobKind::Dictation,
            });
        }
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        state.holder = Some((token, kind));
        Ok(self.make_reservation(token))
    }

    /// Reserve the engine for a dictation session, capture-first: if the gate
    /// is free the reservation is active immediately; if a job holds it, the
    /// dictation is registered in the pending slot and the reservation
    /// becomes active when the holder releases (atomic handoff). Errs only if
    /// the pending slot is already occupied, which the coordinator's
    /// one-session-at-a-time state machine prevents.
    pub(in crate::managers) fn reserve_dictation(
        self: &Arc<Self>,
    ) -> Result<EngineReservation, EngineBusy> {
        let mut state = self.lock_state();
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        match state.holder {
            None => {
                debug_assert!(state.pending_dictation.is_none());
                state.holder = Some((token, EngineJobKind::Dictation));
                Ok(self.make_reservation(token))
            }
            Some((_, holder_kind)) => {
                if state.pending_dictation.is_some() {
                    return Err(EngineBusy {
                        holder: holder_kind,
                    });
                }
                state.pending_dictation = Some(token);
                Ok(self.make_reservation(token))
            }
        }
    }

    /// The kind of job currently holding the engine, if any. Used for
    /// status/busy reporting (e.g. the IPC `status` method).
    pub(in crate::managers) fn holder_kind(&self) -> Option<EngineJobKind> {
        self.lock_state().holder.map(|(_, kind)| kind)
    }

    fn make_reservation(self: &Arc<Self>, token: u64) -> EngineReservation {
        EngineReservation {
            inner: Arc::new(ReservationInner {
                gate: Arc::clone(self),
                token,
            }),
        }
    }

    /// Called when a reservation's last clone drops. If the released token
    /// held the gate and a dictation is pending, ownership transfers to the
    /// dictation under the same lock — a concurrent `try_reserve` can never
    /// observe the gate free in between.
    fn release(&self, token: u64) {
        let mut state = self.lock_state();
        match state.holder {
            Some((held, _)) if held == token => {
                state.holder = state
                    .pending_dictation
                    .take()
                    .map(|pending| (pending, EngineJobKind::Dictation));
            }
            _ if state.pending_dictation == Some(token) => {
                state.pending_dictation = None;
            }
            _ => {}
        }
        self.changed.notify_all();
    }

    fn is_active_token(&self, token: u64) -> bool {
        matches!(self.lock_state().holder, Some((held, _)) if held == token)
    }

    fn wait_active_token(
        &self,
        token: u64,
        timeout: Duration,
        should_abort: &dyn Fn() -> bool,
    ) -> Result<(), EngineBusy> {
        // Abort signals (cancellation) have no channel into this condvar, so
        // wait in short slices and poll the predicate between them.
        const ABORT_POLL_INTERVAL: Duration = Duration::from_millis(25);

        let deadline = Instant::now() + timeout;
        let mut state = self.lock_state();
        loop {
            match state.holder {
                Some((held, _)) if held == token => return Ok(()),
                None if state.pending_dictation == Some(token) => {
                    // Unreachable if the transfer invariant holds; self-heal
                    // by claiming the free gate rather than spinning.
                    state.pending_dictation = None;
                    state.holder = Some((token, EngineJobKind::Dictation));
                    return Ok(());
                }
                _ => {
                    let now = Instant::now();
                    if now >= deadline || should_abort() {
                        return Err(EngineBusy {
                            holder: state
                                .holder
                                .map(|(_, kind)| kind)
                                .unwrap_or(EngineJobKind::Dictation),
                        });
                    }
                    let slice = (deadline - now).min(ABORT_POLL_INTERVAL);
                    state = self
                        .changed
                        .wait_timeout(state, slice)
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .0;
                }
            }
        }
    }
}

struct ReservationInner {
    gate: Arc<EngineGate>,
    token: u64,
}

impl Drop for ReservationInner {
    fn drop(&mut self) {
        self.gate.release(self.token);
    }
}

/// A handle to one logical engine job. Cloneable so a dictation session can
/// span the recording lifecycle, the model preload thread, the streaming
/// worker, and the stop pipeline: all clones share one token, and the gate is
/// released (with handoff to a pending dictation) when the last clone drops.
///
/// A reservation is either *active* (its token holds the gate) or *pending*
/// (dictation registered behind a background job). Manager operations
/// validate that a reservation is active and was issued by their own gate
/// before touching the engine.
pub struct EngineReservation {
    inner: Arc<ReservationInner>,
}

impl Clone for EngineReservation {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl EngineReservation {
    /// Whether this reservation's token currently holds the gate.
    pub fn is_active(&self) -> bool {
        self.inner.gate.is_active_token(self.inner.token)
    }

    /// Block until this reservation becomes active (handoff from the current
    /// holder), `timeout` elapses, or `cancelled` reports true. Used by a
    /// stopped dictation that started in the pending slot: it waits only for
    /// the job that was already running (the pending slot bars new jobs from
    /// acquiring first), and a cancelled dictation must not take the handoff
    /// and start engine work just to have its output discarded. The predicate
    /// is polled every ~25 ms while waiting; pass `|| false` when no
    /// cancellation source applies.
    pub fn wait_active_cancellable(
        &self,
        timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), EngineBusy> {
        self.inner
            .gate
            .wait_active_token(self.inner.token, timeout, &cancelled)
    }

    /// Whether `other` is a clone of this reservation (same logical job).
    /// Used to clear session state only for the session that owns it.
    pub fn same_session(&self, other: &EngineReservation) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether this reservation was issued by `gate` (identity, not
    /// equality). A reservation from a foreign gate must not authorize
    /// operations on this manager's engine.
    pub(in crate::managers) fn belongs_to(&self, gate: &Arc<EngineGate>) -> bool {
        Arc::ptr_eq(&self.inner.gate, gate)
    }
}

impl fmt::Debug for EngineReservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineReservation")
            .field("token", &self.inner.token)
            .field("active", &self.is_active())
            .finish()
    }
}

/// Test-only constructor so sibling modules (e.g. `DictationSession` tests in
/// `actions.rs`) can build reservations without a full `TranscriptionManager`.
#[cfg(test)]
pub(crate) fn test_reservation() -> EngineReservation {
    EngineGate::new()
        .try_reserve(EngineJobKind::Dictation)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn try_reserve_succeeds_when_free_and_releases_on_drop() {
        let gate = EngineGate::new();
        assert!(gate.holder_kind().is_none());
        let reservation = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        assert!(reservation.is_active());
        assert_eq!(gate.holder_kind(), Some(EngineJobKind::Ipc));
        drop(reservation);
        assert!(gate.holder_kind().is_none());
    }

    #[test]
    fn try_reserve_reports_busy_with_current_holder() {
        let gate = EngineGate::new();
        let _held = gate.try_reserve(EngineJobKind::Retry).unwrap();
        let err = gate.try_reserve(EngineJobKind::Ipc).unwrap_err();
        assert_eq!(err.holder, EngineJobKind::Retry);
    }

    #[test]
    fn dictation_reserves_immediately_when_free() {
        let gate = EngineGate::new();
        let reservation = gate.reserve_dictation().unwrap();
        assert!(reservation.is_active());
        assert_eq!(gate.holder_kind(), Some(EngineJobKind::Dictation));
    }

    #[test]
    fn dictation_pends_behind_background_job_and_takes_handoff() {
        let gate = EngineGate::new();
        let ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();

        let dictation = gate.reserve_dictation().unwrap();
        assert!(!dictation.is_active(), "must pend while IPC holds the gate");
        assert_eq!(gate.holder_kind(), Some(EngineJobKind::Ipc));

        drop(ipc);
        assert!(
            dictation.is_active(),
            "handoff must transfer ownership atomically on release"
        );
        assert_eq!(gate.holder_kind(), Some(EngineJobKind::Dictation));
    }

    #[test]
    fn background_job_cannot_jump_a_pending_dictation() {
        let gate = EngineGate::new();
        let ipc_a = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let dictation = gate.reserve_dictation().unwrap();

        // While A holds: busy (named after the holder).
        assert_eq!(
            gate.try_reserve(EngineJobKind::Ipc).unwrap_err().holder,
            EngineJobKind::Ipc
        );

        // After A releases, the dictation owns the gate before any other
        // caller can observe it free.
        drop(ipc_a);
        assert_eq!(
            gate.try_reserve(EngineJobKind::Ipc).unwrap_err().holder,
            EngineJobKind::Dictation
        );
        drop(dictation);
        assert!(gate.try_reserve(EngineJobKind::Ipc).is_ok());
    }

    #[test]
    fn dropping_a_pending_dictation_deregisters_it() {
        let gate = EngineGate::new();
        let ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let dictation = gate.reserve_dictation().unwrap();
        drop(dictation); // user cancelled recording while pending
        drop(ipc);
        assert!(gate.holder_kind().is_none(), "no stale handoff");
    }

    #[test]
    fn second_pending_dictation_is_rejected() {
        let gate = EngineGate::new();
        let _ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let _pending = gate.reserve_dictation().unwrap();
        let err = gate.reserve_dictation().unwrap_err();
        assert_eq!(err.holder, EngineJobKind::Ipc);
    }

    #[test]
    fn clones_share_ownership_and_last_drop_releases() {
        let gate = EngineGate::new();
        let reservation = gate.try_reserve(EngineJobKind::Retry).unwrap();
        let clone = reservation.clone();
        drop(reservation);
        assert_eq!(
            gate.holder_kind(),
            Some(EngineJobKind::Retry),
            "clone must keep the gate held"
        );
        drop(clone);
        assert!(gate.holder_kind().is_none());
    }

    #[test]
    fn wait_active_returns_immediately_when_already_active() {
        let gate = EngineGate::new();
        let reservation = gate.reserve_dictation().unwrap();
        reservation
            .wait_active_cancellable(Duration::ZERO, || false)
            .unwrap();
    }

    #[test]
    fn wait_active_observes_handoff_across_threads() {
        let gate = EngineGate::new();
        let ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let dictation = gate.reserve_dictation().unwrap();

        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            drop(ipc);
        });

        dictation
            .wait_active_cancellable(Duration::from_secs(5), || false)
            .expect("handoff should activate the pending dictation");
        assert!(dictation.is_active());
        releaser.join().unwrap();
    }

    #[test]
    fn wait_active_times_out_naming_the_holder() {
        let gate = EngineGate::new();
        let _ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let dictation = gate.reserve_dictation().unwrap();
        let err = dictation
            .wait_active_cancellable(Duration::from_millis(50), || false)
            .unwrap_err();
        assert_eq!(err.holder, EngineJobKind::Ipc);
    }

    #[test]
    fn wait_active_cancellable_aborts_on_cancellation_while_holder_runs() {
        use std::sync::atomic::AtomicBool;

        let gate = EngineGate::new();
        let _ipc = gate.try_reserve(EngineJobKind::Ipc).unwrap();
        let dictation = gate.reserve_dictation().unwrap();

        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_thread = Arc::clone(&cancelled);
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancelled_for_thread.store(true, Ordering::Release);
        });

        let started = Instant::now();
        let err = dictation
            .wait_active_cancellable(Duration::from_secs(30), || {
                cancelled.load(Ordering::Acquire)
            })
            .unwrap_err();
        assert_eq!(err.holder, EngineJobKind::Ipc);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancellation must abort the wait long before the timeout"
        );
        canceller.join().unwrap();
    }

    #[test]
    fn same_session_is_true_for_clones_only() {
        let gate = EngineGate::new();
        let reservation = gate.try_reserve(EngineJobKind::Retry).unwrap();
        let clone = reservation.clone();
        assert!(reservation.same_session(&clone));

        drop(reservation);
        drop(clone);
        let other = gate.try_reserve(EngineJobKind::Retry).unwrap();
        let unrelated = test_reservation();
        assert!(!other.same_session(&unrelated));
    }

    #[test]
    fn reservation_is_bound_to_its_gate() {
        let gate_a = EngineGate::new();
        let gate_b = EngineGate::new();
        let reservation = gate_a.try_reserve(EngineJobKind::Ipc).unwrap();
        assert!(reservation.belongs_to(&gate_a));
        assert!(!reservation.belongs_to(&gate_b));
    }

    #[test]
    fn busy_error_names_the_holder() {
        let err = EngineBusy {
            holder: EngineJobKind::Maintenance,
        };
        assert_eq!(
            err.to_string(),
            "the transcription engine is busy with engine maintenance"
        );
    }
}
