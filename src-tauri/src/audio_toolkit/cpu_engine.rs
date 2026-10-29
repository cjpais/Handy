//! transcribe.cpp inside Handy's own process, for the small CPU-only models:
//! Silero VAD and the spoken-language detector. Everything that can touch a
//! GPU runs in the engine worker instead (`engine_supervisor`).

use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

use transcribe_cpp::BackendMask;

/// Register transcribe.cpp's CPU backends once per process. The allowed mask
/// is fixed at first registration, so it must never include a GPU here.
pub(crate) fn init_cpu_backends() -> Result<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        // Route native/ggml diagnostics into Handy's log; must precede any
        // native call.
        transcribe_cpp::init_logging();
        transcribe_cpp::init_backends_with(None::<&Path>, BackendMask::CPU)
            .map_err(|e| e.to_string())
    })
    .clone()
    .map_err(|e| anyhow::anyhow!("Failed to initialize transcribe.cpp CPU backend: {e}"))
}
