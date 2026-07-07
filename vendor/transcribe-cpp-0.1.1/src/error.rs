//! Error type mapped from `transcribe_status`.
//!
//! The C API reports failures as a `transcribe_status` int; this module maps
//! each one to a distinct [`Error`] variant (requirements §3: "distinct
//! failures stay distinct"). The grouping mirrors the Python binding's
//! exception hierarchy — the conformance precedent — and the raw status is
//! preserved via [`Error::raw_status`].

use crate::result::Transcript;
use transcribe_cpp_sys as sys;

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong talking to the native library.
///
/// Match on the variant for typed handling; [`Error::raw_status`] recovers the
/// underlying `transcribe_status` code (0 for errors raised purely on the Rust
/// side, e.g. the load-time version gate or a `NUL` in a caller string).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// `TRANSCRIBE_ERR_INVALID_ARG` (or `_SAMPLE_RATE`).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// `TRANSCRIBE_ERR_NOT_IMPLEMENTED` — the model has no path for this call.
    #[error("not implemented by this model: {0}")]
    NotImplemented(String),
    /// `TRANSCRIBE_ERR_FILE_NOT_FOUND` — the GGUF path does not exist.
    #[error("model file not found: {0}")]
    ModelFileNotFound(String),
    /// `TRANSCRIBE_ERR_GGUF` / `_UNSUPPORTED_ARCH` / `_UNSUPPORTED_VARIANT`.
    #[error("model load failed: {0}")]
    ModelLoad(String),
    /// `TRANSCRIBE_ERR_OOM`.
    #[error("out of memory: {0}")]
    OutOfMemory(String),
    /// `TRANSCRIBE_ERR_BACKEND` — the requested backend could not be satisfied.
    #[error("backend error: {0}")]
    Backend(String),
    /// `TRANSCRIBE_ERR_UNSUPPORTED_{TASK,LANGUAGE,TIMESTAMPS,PNC,ITN}` — the
    /// model cannot satisfy this request shape.
    #[error("unsupported request: {0}")]
    Unsupported(String),
    /// `TRANSCRIBE_ERR_BAD_STRUCT_SIZE` — an ABI/struct-size fault. With the
    /// generated FFI this indicates a header/library skew, not caller error.
    #[error("ABI struct-size mismatch: {0}")]
    BadStructSize(String),
    /// `TRANSCRIBE_ERR_INPUT_TOO_LONG` — audio longer than the model can decode.
    #[error("input too long: {0}")]
    InputTooLong(String),
    /// `TRANSCRIBE_ERR_ABORTED` — the abort callback returned true. The
    /// transcript of chunks that completed before the abort is preserved.
    #[error("operation aborted: {message}")]
    Aborted {
        message: String,
        /// Partial transcript from chunks that completed before the abort.
        partial: Option<Box<Transcript>>,
    },
    /// `TRANSCRIBE_ERR_OUTPUT_TRUNCATED` — the decode hit the generation budget
    /// before end-of-stream; the transcript is incomplete by contract. The
    /// partial transcript is always preserved.
    #[error("output truncated before end-of-stream: {message}")]
    OutputTruncated {
        message: String,
        /// The (incomplete) transcript produced before truncation.
        partial: Option<Box<Transcript>>,
    },
    /// The loaded library's base version disagrees with the headers this crate
    /// was generated against (the pre-1.0 version lock). Raised on first use.
    #[error("native library version mismatch: {0}")]
    VersionMismatch(String),
    /// A caller-supplied string contained an interior NUL byte.
    #[error("string contains an interior NUL byte: {0}")]
    Nul(#[from] std::ffi::NulError),
    /// Another session of the same model already has a compute operation in
    /// flight. The C library serializes compute per model (the 0.x limitation):
    /// at most one run / batch / active stream across ALL of a model's sessions
    /// at a time. Finish or drop the in-flight stream (one-shot runs/batches
    /// just queue) before starting another, or give each concurrent worker its
    /// own model. Raised purely on the Rust side, so [`Error::raw_status`] is 0.
    #[error("model busy: {0}")]
    Busy(String),
    /// A non-OK status with no more specific mapping.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// The `transcribe_status` this error was mapped from, or `0` for errors
    /// raised on the Rust side (version gate, NUL in a string, …).
    pub fn raw_status(&self) -> i32 {
        use sys::transcribe_status as S;
        let s = match self {
            Error::InvalidArgument(_) => S::TRANSCRIBE_ERR_INVALID_ARG,
            Error::NotImplemented(_) => S::TRANSCRIBE_ERR_NOT_IMPLEMENTED,
            Error::ModelFileNotFound(_) => S::TRANSCRIBE_ERR_FILE_NOT_FOUND,
            Error::ModelLoad(_) => S::TRANSCRIBE_ERR_GGUF,
            Error::OutOfMemory(_) => S::TRANSCRIBE_ERR_OOM,
            Error::Backend(_) => S::TRANSCRIBE_ERR_BACKEND,
            Error::Unsupported(_) => S::TRANSCRIBE_ERR_UNSUPPORTED_TASK,
            Error::BadStructSize(_) => S::TRANSCRIBE_ERR_BAD_STRUCT_SIZE,
            Error::InputTooLong(_) => S::TRANSCRIBE_ERR_INPUT_TOO_LONG,
            Error::Aborted { .. } => S::TRANSCRIBE_ERR_ABORTED,
            Error::OutputTruncated { .. } => S::TRANSCRIBE_ERR_OUTPUT_TRUNCATED,
            _ => S::TRANSCRIBE_OK,
        };
        s.0 as i32
    }

    /// The partial transcript carried by [`Error::Aborted`] /
    /// [`Error::OutputTruncated`], if any. `None` for every other variant.
    pub fn partial(&self) -> Option<&Transcript> {
        match self {
            Error::Aborted { partial, .. } | Error::OutputTruncated { partial, .. } => {
                partial.as_deref()
            }
            _ => None,
        }
    }
}

/// Human-readable text for a `transcribe_status` (from the C side).
pub(crate) fn status_string(status: i32) -> String {
    // Borrowed pointer into static storage; copy at the boundary.
    let ptr = unsafe { sys::transcribe_status_string(status) };
    if ptr.is_null() {
        return format!("status {status}");
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Build the mapped error for a non-OK status (without a partial transcript).
/// The run path re-attaches partials for the two result-bearing statuses.
pub(crate) fn error_for_status(status: sys::transcribe_status, context: &str) -> Error {
    use sys::transcribe_status as S;
    let code = status.0 as i32;
    let msg = {
        let s = status_string(code);
        if context.is_empty() {
            format!("{s} (status {code})")
        } else {
            format!("{context}: {s} (status {code})")
        }
    };
    match status {
        S::TRANSCRIBE_ERR_INVALID_ARG | S::TRANSCRIBE_ERR_SAMPLE_RATE => {
            Error::InvalidArgument(msg)
        }
        S::TRANSCRIBE_ERR_NOT_IMPLEMENTED => Error::NotImplemented(msg),
        S::TRANSCRIBE_ERR_FILE_NOT_FOUND => Error::ModelFileNotFound(msg),
        S::TRANSCRIBE_ERR_GGUF
        | S::TRANSCRIBE_ERR_UNSUPPORTED_ARCH
        | S::TRANSCRIBE_ERR_UNSUPPORTED_VARIANT => Error::ModelLoad(msg),
        S::TRANSCRIBE_ERR_OOM => Error::OutOfMemory(msg),
        S::TRANSCRIBE_ERR_BACKEND => Error::Backend(msg),
        S::TRANSCRIBE_ERR_UNSUPPORTED_LANGUAGE
        | S::TRANSCRIBE_ERR_UNSUPPORTED_TASK
        | S::TRANSCRIBE_ERR_UNSUPPORTED_TIMESTAMPS
        | S::TRANSCRIBE_ERR_UNSUPPORTED_PNC
        | S::TRANSCRIBE_ERR_UNSUPPORTED_ITN => Error::Unsupported(msg),
        S::TRANSCRIBE_ERR_BAD_STRUCT_SIZE => Error::BadStructSize(msg),
        S::TRANSCRIBE_ERR_INPUT_TOO_LONG => Error::InputTooLong(msg),
        S::TRANSCRIBE_ERR_ABORTED => Error::Aborted {
            message: msg,
            partial: None,
        },
        S::TRANSCRIBE_ERR_OUTPUT_TRUNCATED => Error::OutputTruncated {
            message: msg,
            partial: None,
        },
        _ => Error::Other(msg),
    }
}

/// `Ok(())` for `TRANSCRIBE_OK`, otherwise the mapped error.
pub(crate) fn check(status: sys::transcribe_status, context: &str) -> Result<()> {
    if status == sys::transcribe_status::TRANSCRIBE_OK {
        Ok(())
    } else {
        Err(error_for_status(status, context))
    }
}
