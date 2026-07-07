//! Safe, idiomatic Rust bindings for
//! [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp), a C/C++
//! speech-to-text library built on ggml.
//!
//! The raw FFI lives in the [`transcribe-cpp-sys`](transcribe_cpp_sys) crate
//! (re-exported as [`sys`]); this crate wraps it with owned result types, an
//! error enum, RAII handles, and a thread-safety model that matches the C
//! contract.
//!
//! # Quickstart
//!
//! ```no_run
//! use transcribe_cpp::{Model, RunOptions};
//!
//! let mut session = Model::load("model.gguf")?.session()?;
//! let pcm: Vec<f32> = vec![/* 16 kHz mono samples in [-1, 1] */];
//! let result = session.run(&pcm, &RunOptions::default())?;
//! println!("{}", result.text);
//! # Ok::<(), transcribe_cpp::Error>(())
//! ```
//!
//! # Threading
//!
//! - [`Model`] is `Send + Sync` and cheap to clone (`Arc`-backed). The native
//!   model is freed only after the last [`Model`] handle and every [`Session`]
//!   derived from it are dropped — in any order.
//! - [`Session`] is `Send` but not `Sync`; its mutating calls take `&mut self`,
//!   so the type system enforces one-call-at-a-time.
//! - **0.x concurrency:** the C library permits at most one in-flight
//!   `run`/stream across all sessions of a model. This crate enforces that with
//!   a per-model mutex (held only for the native compute call), so concurrent
//!   calls from many sessions queue rather than race. For real parallelism, use
//!   one [`Model`] per worker today.
//!
//! # ABI verification
//!
//! The per-field struct-layout check that the ctypes binding performs is
//! **waived** here: bindgen takes every struct's layout from a real compiler at
//! generation time, so the generated FFI cannot disagree with the headers it
//! was built against. The load-time base-version lock (see [`Model::load`]) is
//! retained.

#![doc(html_root_url = "https://docs.rs/transcribe-cpp")]
#![warn(missing_debug_implementations)]

pub use transcribe_cpp_sys as sys;

mod backend;
mod cancel;
mod error;
mod family;
mod logging;
mod model;
mod result;
mod session;
mod streaming;
mod types;
mod version;

pub use backend::{
    backend_available, device_count, devices, init_backends, init_backends_default, Device,
    DeviceType,
};
pub use cancel::CancelToken;
pub use error::{Error, Result};
pub use family::{
    MoonshineStreamingOptions, ParakeetBufferedStreamOptions, ParakeetStreamOptions, RunExtension,
    StreamExtension, VoxtralRealtimeStreamOptions, WhisperRunOptions,
};
pub use logging::{disable_logging, init_logging};
pub use model::{Capabilities, Model, ModelOptions, SessionLimits, SessionOptions};
pub use result::{Segment, Timings, Token, Transcript, Word};
pub use session::{RunOptions, Session, Stream};
pub use streaming::{StreamOptions, StreamText, StreamUpdate};
pub use types::{
    AbiStruct, Backend, CommitPolicy, ExtSlot, Feature, Itn, KvType, Pnc, StreamState, Task,
    TimestampKind,
};
pub use version::{
    abi_struct_align, abi_struct_size, compiled_version, header_hash, version, version_commit,
};

/// Convenience: load a model, transcribe one PCM buffer, and return the result.
///
/// For repeated transcription, hold a [`Session`] instead of paying model load
/// on every call.
pub fn transcribe(
    model_path: impl AsRef<std::path::Path>,
    pcm: &[f32],
    options: &RunOptions,
) -> Result<Transcript> {
    let mut session = Model::load(model_path)?.session()?;
    session.run(pcm, options)
}
