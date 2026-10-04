//! Process isolation for transcribe.cpp.
//!
//! All transcribe.cpp native code (backend init, device enumeration, model
//! load, inference, streaming) runs in a child process: the Handy executable
//! re-launched with [`WORKER_FLAG`]. A GPU driver fault, `GGML_ASSERT` abort,
//! or hang then kills only the worker.
//!
//! [`EngineSupervisor`] is the single owner of that worker. It loads and
//! unloads models, runs and streams transcriptions, cancels them, and
//! recovers from crashes and hangs (retrying on CPU when the GPU was at
//! fault), so the rest of the app never sees processes, restarts or fallback.
//! At most one worker is alive at a time, and it lives exactly as long as a
//! loaded model, so unloading returns all of its CPU and GPU memory to the OS.
//!
//! - `protocol`: the framed request/response protocol between the two.
//! - `supervisor`: the parent side ([`EngineSupervisor`]).
//! - `worker`: the child process.

mod protocol;
mod supervisor;
mod worker;

pub use protocol::{DeviceInfo, DeviceSelector, LoadedInfo};
pub use supervisor::{
    DeviceList, EngineError, EngineSupervisor, Finalized, LoadSpec, StreamHandle, StreamProgress,
    Unloading,
};

/// Hidden first argument that turns the executable into a worker.
pub const WORKER_FLAG: &str = "--transcribe-worker";
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
