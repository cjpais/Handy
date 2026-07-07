//! Typed family-extension options.
//!
//! Each family exposes model-specific knobs via a kind-tagged extension struct
//! (`include/transcribe/<family>.h`). These typed options mirror those structs:
//! every field is an `Option`, and only the fields you set override the
//! defaults the C `*_init()` stamps. A [`RunExtension`] attaches to
//! [`RunOptions`](crate::RunOptions); a [`StreamExtension`] attaches to
//! [`StreamOptions`](crate::StreamOptions).
//!
//! Probe [`Model::accepts_ext`](crate::Model::accepts_ext) to learn whether a
//! loaded model accepts a given kind on a slot; an unaccepted extension is
//! rejected by `run`/`stream` with [`Error::InvalidArgument`](crate::Error).

use std::ffi::CString;

use transcribe_cpp_sys as sys;

use crate::error::Result;

/// Whisper run-extension knobs (run slot): initial prompt, temperature
/// fallback, and decode thresholds. `None` keeps the family default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WhisperRunOptions {
    pub initial_prompt: Option<String>,
    pub condition_on_prev_tokens: Option<bool>,
    pub temperature: Option<f32>,
    pub temperature_inc: Option<f32>,
    pub compression_ratio_thold: Option<f32>,
    pub logprob_thold: Option<f32>,
    pub no_speech_thold: Option<f32>,
    pub max_prev_context_tokens: Option<i32>,
    pub seed: Option<u32>,
    pub max_initial_timestamp: Option<f32>,
}

/// Moonshine-streaming stream-extension knobs (stream slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoonshineStreamingOptions {
    pub min_decode_interval_ms: Option<i32>,
}

/// Parakeet cache-aware streaming knobs (stream slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParakeetStreamOptions {
    pub att_context_right: Option<i32>,
}

/// Parakeet chunked-attention buffered streaming knobs (stream slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParakeetBufferedStreamOptions {
    pub left_ms: Option<i32>,
    pub chunk_ms: Option<i32>,
    pub right_ms: Option<i32>,
}

/// Voxtral-realtime streaming knobs (stream slot).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoxtralRealtimeStreamOptions {
    pub num_delay_tokens: Option<i32>,
    pub min_decode_interval_ms: Option<i32>,
}

/// A family extension for the run slot (offline `run`/`run_batch`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RunExtension {
    Whisper(WhisperRunOptions),
}

/// A family extension for the stream slot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamExtension {
    ParakeetStream(ParakeetStreamOptions),
    ParakeetBuffered(ParakeetBufferedStreamOptions),
    MoonshineStreaming(MoonshineStreamingOptions),
    VoxtralRealtime(VoxtralRealtimeStreamOptions),
}

/// Owns a materialized run-slot C extension struct (and any strings it points
/// at) so its address — and the `transcribe_ext` pointer handed to the C API —
/// stays valid for the duration of the native call. Boxed for a stable address
/// across moves of the holder.
pub(crate) enum RunExtRaw {
    Whisper {
        ext: Box<sys::transcribe_whisper_run_ext>,
        _prompt: Option<CString>,
    },
}

impl RunExtRaw {
    pub(crate) fn ext_ptr(&self) -> *const sys::transcribe_ext {
        match self {
            // `ext` is field 0, so &ext == &the family struct.
            RunExtRaw::Whisper { ext, .. } => {
                (&**ext) as *const sys::transcribe_whisper_run_ext as *const sys::transcribe_ext
            }
        }
    }
}

impl RunExtension {
    pub(crate) fn materialize(&self) -> Result<RunExtRaw> {
        match self {
            RunExtension::Whisper(o) => {
                let mut ext: sys::transcribe_whisper_run_ext = unsafe { std::mem::zeroed() };
                unsafe { sys::transcribe_whisper_run_ext_init(&mut ext) };
                // Propagate an interior NUL as Error::Nul (like language /
                // target_language) instead of silently dropping the prompt.
                let prompt = o.initial_prompt.as_deref().map(CString::new).transpose()?;
                if let Some(c) = prompt.as_ref() {
                    ext.initial_prompt = c.as_ptr();
                }
                set(
                    &mut ext.condition_on_prev_tokens,
                    o.condition_on_prev_tokens,
                );
                set(&mut ext.temperature, o.temperature);
                set(&mut ext.temperature_inc, o.temperature_inc);
                set(&mut ext.compression_ratio_thold, o.compression_ratio_thold);
                set(&mut ext.logprob_thold, o.logprob_thold);
                set(&mut ext.no_speech_thold, o.no_speech_thold);
                set(&mut ext.max_prev_context_tokens, o.max_prev_context_tokens);
                set(&mut ext.seed, o.seed);
                set(&mut ext.max_initial_timestamp, o.max_initial_timestamp);
                Ok(RunExtRaw::Whisper {
                    ext: Box::new(ext),
                    _prompt: prompt,
                })
            }
        }
    }
}

/// Owns a materialized stream-slot C extension struct (none carry strings).
pub(crate) enum StreamExtRaw {
    ParakeetStream(Box<sys::transcribe_parakeet_stream_ext>),
    ParakeetBuffered(Box<sys::transcribe_parakeet_buffered_stream_ext>),
    MoonshineStreaming(Box<sys::transcribe_moonshine_streaming_stream_ext>),
    VoxtralRealtime(Box<sys::transcribe_voxtral_realtime_stream_ext>),
}

impl StreamExtRaw {
    pub(crate) fn ext_ptr(&self) -> *const sys::transcribe_ext {
        match self {
            StreamExtRaw::ParakeetStream(e) => {
                (&**e) as *const sys::transcribe_parakeet_stream_ext as *const sys::transcribe_ext
            }
            StreamExtRaw::ParakeetBuffered(e) => {
                (&**e) as *const sys::transcribe_parakeet_buffered_stream_ext
                    as *const sys::transcribe_ext
            }
            StreamExtRaw::MoonshineStreaming(e) => {
                (&**e) as *const sys::transcribe_moonshine_streaming_stream_ext
                    as *const sys::transcribe_ext
            }
            StreamExtRaw::VoxtralRealtime(e) => {
                (&**e) as *const sys::transcribe_voxtral_realtime_stream_ext
                    as *const sys::transcribe_ext
            }
        }
    }
}

impl StreamExtension {
    pub(crate) fn materialize(&self) -> StreamExtRaw {
        match self {
            StreamExtension::ParakeetStream(o) => {
                let mut e: sys::transcribe_parakeet_stream_ext = unsafe { std::mem::zeroed() };
                unsafe { sys::transcribe_parakeet_stream_ext_init(&mut e) };
                set(&mut e.att_context_right, o.att_context_right);
                StreamExtRaw::ParakeetStream(Box::new(e))
            }
            StreamExtension::ParakeetBuffered(o) => {
                let mut e: sys::transcribe_parakeet_buffered_stream_ext =
                    unsafe { std::mem::zeroed() };
                unsafe { sys::transcribe_parakeet_buffered_stream_ext_init(&mut e) };
                set(&mut e.left_ms, o.left_ms);
                set(&mut e.chunk_ms, o.chunk_ms);
                set(&mut e.right_ms, o.right_ms);
                StreamExtRaw::ParakeetBuffered(Box::new(e))
            }
            StreamExtension::MoonshineStreaming(o) => {
                let mut e: sys::transcribe_moonshine_streaming_stream_ext =
                    unsafe { std::mem::zeroed() };
                unsafe { sys::transcribe_moonshine_streaming_stream_ext_init(&mut e) };
                set(&mut e.min_decode_interval_ms, o.min_decode_interval_ms);
                StreamExtRaw::MoonshineStreaming(Box::new(e))
            }
            StreamExtension::VoxtralRealtime(o) => {
                let mut e: sys::transcribe_voxtral_realtime_stream_ext =
                    unsafe { std::mem::zeroed() };
                unsafe { sys::transcribe_voxtral_realtime_stream_ext_init(&mut e) };
                set(&mut e.num_delay_tokens, o.num_delay_tokens);
                set(&mut e.min_decode_interval_ms, o.min_decode_interval_ms);
                StreamExtRaw::VoxtralRealtime(Box::new(e))
            }
        }
    }
}

/// Overwrite `slot` only when the caller provided a value.
fn set<T>(slot: &mut T, value: Option<T>) {
    if let Some(v) = value {
        *slot = v;
    }
}
