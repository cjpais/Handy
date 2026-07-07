//! Owned result types.
//!
//! Every result is fully materialized into these types — all text is copied at
//! the FFI boundary (`CStr -> String`), so no borrowed session pointer ever
//! escapes to user code (the "copy at the FFI boundary" rule). Holding a
//! [`Transcript`] across the next `run`/`stream` call is therefore always safe.

use std::ffi::CStr;
use std::os::raw::c_char;

use transcribe_cpp_sys as sys;

use crate::types::TimestampKind;

/// A fully-materialized transcription result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transcript {
    /// The full transcript text.
    pub text: String,
    /// The model-detected language (ISO code), if it predicted one. `None`
    /// when a language hint was given, the model lacks LID, or none applies.
    pub language: Option<String>,
    /// The finest timestamp granularity actually populated below.
    pub timestamp_kind: TimestampKind,
    /// Segment rows (empty unless segment-or-finer timestamps were produced).
    pub segments: Vec<Segment>,
    /// Word rows (empty unless word-or-finer timestamps were produced).
    pub words: Vec<Word>,
    /// Token rows (empty unless token timestamps were produced).
    pub tokens: Vec<Token>,
    /// Stage timings for the run.
    pub timings: Timings,
}

/// One segment row. Times are milliseconds relative to the original audio.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Segment {
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub first_word: i32,
    pub n_words: i32,
    pub first_token: i32,
    pub n_tokens: i32,
    pub text: String,
}

/// One word row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Word {
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub seg_index: i32,
    pub first_token: i32,
    pub n_tokens: i32,
    pub text: String,
}

/// One token row. `p` is a per-token confidence hint (NaN when the family
/// produces none); its exact meaning is family-specific.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Token {
    pub id: i32,
    pub p: f32,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub seg_index: i32,
    pub word_index: i32,
    pub text: String,
}

/// Per-run stage timings, in milliseconds. Zero means "unknown / not measured".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timings {
    pub load_ms: f32,
    pub mel_ms: f32,
    pub encode_ms: f32,
    pub decode_ms: f32,
}

/// Copy a borrowed C string into an owned `String` (empty for NULL).
pub(crate) fn owned_str(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Copy a borrowed C string into `Some(String)`, or `None` for NULL/empty.
pub(crate) fn owned_opt_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

impl Segment {
    pub(crate) fn from_raw(raw: &sys::transcribe_segment) -> Self {
        Segment {
            t0_ms: raw.t0_ms,
            t1_ms: raw.t1_ms,
            first_word: raw.first_word,
            n_words: raw.n_words,
            first_token: raw.first_token,
            n_tokens: raw.n_tokens,
            text: owned_str(raw.text),
        }
    }
}

impl Word {
    pub(crate) fn from_raw(raw: &sys::transcribe_word) -> Self {
        Word {
            t0_ms: raw.t0_ms,
            t1_ms: raw.t1_ms,
            seg_index: raw.seg_index,
            first_token: raw.first_token,
            n_tokens: raw.n_tokens,
            text: owned_str(raw.text),
        }
    }
}

impl Token {
    pub(crate) fn from_raw(raw: &sys::transcribe_token) -> Self {
        Token {
            id: raw.id,
            p: raw.p,
            t0_ms: raw.t0_ms,
            t1_ms: raw.t1_ms,
            seg_index: raw.seg_index,
            word_index: raw.word_index,
            text: owned_str(raw.text),
        }
    }
}

impl Timings {
    pub(crate) fn from_raw(raw: &sys::transcribe_timings) -> Self {
        Timings {
            load_ms: raw.load_ms,
            mel_ms: raw.mel_ms,
            encode_ms: raw.encode_ms,
            decode_ms: raw.decode_ms,
        }
    }
}
