//! Process isolation for transcribe.cpp.
//!
//! All transcribe.cpp native code (backend init, device enumeration, model
//! load, inference, streaming) runs in a child process: the Handy executable
//! re-launched with [`WORKER_FLAG`]. A GPU driver fault, `GGML_ASSERT` abort,
//! or hang then kills only the worker.
//!
//! [`EngineSupervisor`] is the single owner of that worker. It loads and
//! unloads models, runs and streams transcriptions, cancels them, and
//! recovers from crashes and hangs (trying another healthy GPU before CPU
//! after a device failure), so the rest of the app never sees processes, restarts
//! or fallback. At most one worker holds a model at a time, and it lives
//! exactly as long as that model, so unloading returns all of its CPU and GPU
//! memory to the OS. Listing compute devices may briefly run a second,
//! model-less worker.
//!
//! A CPU-only worker ([`CPU_ONLY_FLAG`]) restricts transcribe.cpp to its CPU
//! backends before any are registered, so no GPU driver code runs in it at
//! all: a driver that crashes or hangs while initializing can't take the CPU
//! fallback down with it.
//!
//! GPU recovery policy (shared by Windows, macOS and Linux):
//!
//! An attributed failure quarantines that GPU's stable identity. Try another
//! healthy GPU under Auto, then strict CPU, preserving the recording and selection.
//! A named GPU permits only that device or CPU; it never falls back to another GPU.
//! Each GPU retries after 30s, then 60/120/240/300s on failed trials. Duplicate
//! reports do not extend the cooldown; only successful nonempty batch inference
//! or stream finalization clears that GPU's history. Idle unload preserves it.
//!
//! Checks run at dictation boundaries, not on a background timer or during an
//! active stream. Routine discovery runs every 60s; a device cooldown can bring
//! the next check forward. Expiry permits retry without observed disappearance;
//! an observed absent-to-present registry change also permits an early trial.
//! Auto prefers dedicated over integrated GPUs, then memory within that class.
//! A named selection returns only to that GPU. Do not reload the active GPU or
//! downgrade Auto to a weaker GPU.
//!
//! Admit only the chosen retry candidate; other quarantined GPUs remain excluded
//! even after expiry. Revoke admission if that candidate never loads. Listing
//! uses a fresh model-less worker while the current model stays loaded. Switching
//! unloads the current model first; a failed upgrade restores the previous GPU
//! by identity, then strict CPU. Restoration can add latency and can itself fail.
//!
//! Unattributed backend-init failures stay CPU-only until an explicit accelerator
//! change or restart, which also clear device history. Recovery cannot repair a
//! driver. Cooldowns control eligibility; a later dictation must trigger retry.
//!
//! Explicit CPU preferences and host GPU restrictions prevent GPU upgrades.
//! CLI device indices are strict diagnostic selections without fallback.
//!
//! - `protocol`: the framed request/response protocol between the two.
//! - `supervisor`: the parent side ([`EngineSupervisor`]).
//! - `worker`: the child process.

mod protocol;
mod supervisor;
mod worker;

pub use protocol::{DeviceInfo, DeviceSelector, LoadedInfo};
pub use supervisor::{
    CancelMark, EngineError, EngineSupervisor, Finalized, LoadSpec, StreamHandle, StreamProgress,
    Unloading,
};

/// Hidden first argument that turns the executable into a worker.
pub const WORKER_FLAG: &str = "--transcribe-worker";
/// Worker argument: register only the CPU backends.
const CPU_ONLY_FLAG: &str = "--cpu-only";
const LOG_LEVEL_ENV: &str = "HANDY_TRANSCRIBE_WORKER_LOG";

/// Whether this process was launched as a transcription worker. Checked in
/// `main` before CLI parsing, Tauri, or single-instance handling.
pub fn is_worker_invocation() -> bool {
    std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == WORKER_FLAG)
}

/// Run the worker until the parent closes its stdin. Returns the exit code.
pub fn run_worker() -> i32 {
    worker::run()
}
