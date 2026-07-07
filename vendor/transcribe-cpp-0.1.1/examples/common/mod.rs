//! Shared fixture resolution for the canonical examples (the example-runner
//! analog of `tests/common/mod.rs`).
//!
//! Each example resolves its model + audio from, in order: (1) a CLI arg, (2)
//! the `TRANSCRIBE_SMOKE_*` env var CI exports via `fetch-canary`, (3) an
//! in-repo default. When nothing resolves the example prints a skip note and
//! exits 0 — the same skip rule the model-gated tests use, so every example
//! runs headless in CI (and on forks, where the canary is absent).

#![allow(dead_code)]

use std::path::PathBuf;

/// Register backend modules before the first model load (idempotent; a no-op in
/// compiled-in builds, loads the per-ISA CPU / GPU modules in a `dynamic-backends`
/// build). Folded into the model resolvers so every example works in every
/// posture; `backend-select` also calls it explicitly to demonstrate the pattern.
fn ensure_backends() {
    transcribe_cpp::init_backends_default().expect("init_backends_default");
}

/// Repo root: walk up from this crate until the marker only the root carries.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("include/transcribe.h").is_file() && p.join("ggml").is_dir())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The offline model: `arg` | `$TRANSCRIBE_SMOKE_MODEL` | in-repo whisper canary.
pub fn model_path(arg: Option<&str>) -> Option<PathBuf> {
    ensure_backends();
    resolve(
        arg,
        "TRANSCRIBE_SMOKE_MODEL",
        "models/whisper-tiny.en/whisper-tiny.en-Q5_K_M.gguf",
    )
}

/// The streaming model: `arg` | `$TRANSCRIBE_SMOKE_STREAMING_MODEL` | canary.
pub fn streaming_model_path(arg: Option<&str>) -> Option<PathBuf> {
    ensure_backends();
    resolve(
        arg,
        "TRANSCRIBE_SMOKE_STREAMING_MODEL",
        "models/moonshine-streaming-tiny/moonshine-streaming-tiny-Q8_0.gguf",
    )
}

/// The audio: `arg` | `$TRANSCRIBE_SMOKE_AUDIO` | the in-repo `samples/jfk.wav`.
pub fn audio_path(arg: Option<&str>) -> Option<PathBuf> {
    resolve(arg, "TRANSCRIBE_SMOKE_AUDIO", "samples/jfk.wav")
}

fn resolve(arg: Option<&str>, env: &str, default_rel: &str) -> Option<PathBuf> {
    let path = arg
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(env).map(PathBuf::from))
        .unwrap_or_else(|| repo_root().join(default_rel));
    path.is_file().then_some(path)
}

/// Load a 16 kHz mono 16-bit WAV as f32 PCM — the exact format the binding
/// consumes (it does not decode containers or resample).
pub fn load_wav(path: &std::path::Path) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "{path:?} must be mono");
    assert_eq!(spec.sample_rate, 16_000, "{path:?} must be 16 kHz");
    assert_eq!(spec.bits_per_sample, 16, "{path:?} must be 16-bit");
    reader
        .samples::<i16>()
        .map(|s| s.expect("wav sample") as f32 / 32768.0)
        .collect()
}
