//! [`Session`] — a transcription context bound to a [`Model`].
//!
//! A session is single-threaded (`Send` but not `Sync`): `run` takes `&mut
//! self`, so the type system enforces "one call at a time" on a given session.
//! Across *different* sessions of one model the per-model compute lock
//! ([`ModelInner::compute_lock`](crate::model::ModelInner)) enforces the C
//! library's "one in-flight compute per model" rule: one-shot runs/batches
//! serialize on it, and while a stream is in flight any overlapping
//! run/batch/stream is refused with [`Error::Busy`].

use std::ffi::CString;
use std::os::raw::c_void;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use transcribe_cpp_sys as sys;

use crate::cancel::{abort_trampoline, CancelToken};
use crate::error::{check, error_for_status, Error, Result};
use crate::family::{RunExtRaw, RunExtension};
use crate::model::{Model, ModelInner, SessionLimits, SessionOptions};
use crate::result::{owned_opt_str, owned_str, Segment, Timings, Token, Transcript, Word};
use crate::streaming::{build_stream_params, StreamOptions, StreamText, StreamUpdate};
use crate::types::{Itn, Pnc, StreamState, Task, TimestampKind};

/// Per-run parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    pub task: Task,
    pub timestamps: TimestampKind,
    pub pnc: Pnc,
    pub itn: Itn,
    /// Source language hint (ISO code), or `None` to autodetect.
    pub language: Option<String>,
    /// Target language for translation, or `None`.
    pub target_language: Option<String>,
    /// Keep special vocab tags (e.g. `<|...|>`) in the returned text.
    pub keep_special_tags: bool,
    /// Speculative-decode draft length. `-1` = family default, `0` = disabled.
    pub spec_k_drafts: i32,
    /// Maximum new transcript tokens. `-1` = family default.
    pub max_new_tokens: i32,
    /// Optional family-specific run extension (e.g. whisper decode knobs).
    pub family: Option<RunExtension>,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            task: Task::Transcribe,
            timestamps: TimestampKind::Auto,
            pnc: Pnc::Default,
            itn: Itn::Default,
            language: None,
            target_language: None,
            keep_special_tags: false,
            spec_k_drafts: -1,
            max_new_tokens: -1,
            family: None,
        }
    }
}

/// A transcription session.
pub struct Session {
    ptr: *mut sys::transcribe_session,
    // Keeps the native model alive (and carries the per-model run lock). The
    // model is freed only after this Arc — and every other handle — is dropped.
    model: Arc<ModelInner>,
    // Retained so the abort callback's userdata pointer stays valid while
    // installed. `None` when no cancel token is set.
    cancel: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

// SAFETY: a session may be moved between threads as long as it is used by at
// most one at a time, which `&mut self` on the mutating calls enforces. It is
// deliberately NOT Sync.
unsafe impl Send for Session {}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { sys::transcribe_session_free(self.ptr) };
        // self.model Arc is dropped here, possibly freeing the model.
    }
}

impl Session {
    pub(crate) fn new(model: &Model, options: &SessionOptions) -> Result<Session> {
        let mut params: sys::transcribe_session_params = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_session_params_init(&mut params) };
        params.n_threads = options.n_threads;
        params.kv_type = options.kv_type.to_raw();
        params.n_ctx = options.n_ctx;

        let mut out: *mut sys::transcribe_session = std::ptr::null_mut();
        let status = unsafe { sys::transcribe_session_init(model.inner.ptr, &params, &mut out) };
        check(status, "session init")?;
        debug_assert!(!out.is_null());

        Ok(Session {
            ptr: out,
            model: Arc::clone(&model.inner),
            cancel: None,
        })
    }

    /// Install a [`CancelToken`] so an in-flight run/stream can be aborted from
    /// another thread. Replaces any previously installed token. The token's
    /// flag is shared, so cancel via any clone of it. (Cancellation only takes
    /// effect on models whose family honors the abort callback —
    /// [`Feature::Cancellation`](crate::Feature::Cancellation).)
    pub fn set_cancel_token(&mut self, token: &CancelToken) {
        let flag = Arc::clone(&token.flag);
        let userdata = Arc::as_ptr(&flag) as *mut c_void;
        // SAFETY: single-threaded session contract — no run is in flight when
        // this is called. The new callback is installed before the old retained
        // Arc is dropped, so the previous userdata is never dangling-referenced.
        unsafe { sys::transcribe_set_abort_callback(self.ptr, Some(abort_trampoline), userdata) };
        self.cancel = Some(flag);
    }

    /// Remove any installed cancel token.
    pub fn clear_cancel_token(&mut self) {
        unsafe { sys::transcribe_set_abort_callback(self.ptr, None, std::ptr::null_mut()) };
        self.cancel = None;
    }

    /// Whether the most recent run/stream was ended by cancellation.
    pub fn was_aborted(&self) -> bool {
        unsafe { sys::transcribe_was_aborted(self.ptr) }
    }

    /// Whether the most recent decode stopped at the generation budget before
    /// end-of-stream (the transcript is incomplete).
    pub fn was_truncated(&self) -> bool {
        unsafe { sys::transcribe_was_truncated(self.ptr) }
    }

    /// Transcribe one buffer of 16 kHz mono float32 PCM in `[-1, 1]`.
    ///
    /// On an aborted or truncated decode the partial transcript is preserved
    /// on the returned [`Error::Aborted`] / [`Error::OutputTruncated`].
    pub fn run(&mut self, pcm: &[f32], options: &RunOptions) -> Result<Transcript> {
        let (params, _lang, _target, _family) = build_run_params(options)?;
        let n = clamp_len(pcm.len())?;

        // The compute path is serialized per model; hold the lock for the native
        // call, then materialize this session's own result storage. Refuse a run
        // that would overlap an active stream on another session (the C contract).
        let status = {
            let guard = self
                .model
                .compute_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *guard {
                return Err(Error::Busy(
                    "a stream is active on this model; finish or drop it before run()".into(),
                ));
            }
            unsafe { sys::transcribe_run(self.ptr, pcm.as_ptr(), n, &params) }
        };

        match status {
            s if s == sys::transcribe_status::TRANSCRIBE_OK => Ok(self.materialize_run()),
            s if s == sys::transcribe_status::TRANSCRIBE_ERR_ABORTED
                || s == sys::transcribe_status::TRANSCRIBE_ERR_OUTPUT_TRUNCATED =>
            {
                // Partial transcript is preserved by the C API for both.
                let partial = Box::new(self.materialize_run());
                Err(attach_partial(error_for_status(s, "run"), partial))
            }
            s => Err(error_for_status(s, "run")),
        }
    }

    /// Transcribe several buffers in one call (throughput path).
    ///
    /// Returns the whole-batch outcome as an outer `Result`; each utterance's
    /// individual outcome is an inner `Result` in the returned vector (one per
    /// input, in order). A malformed single utterance fails only its own slot.
    pub fn run_batch(
        &mut self,
        pcms: &[&[f32]],
        options: &RunOptions,
    ) -> Result<Vec<Result<Transcript>>> {
        let (params, _lang, _target, _family) = build_run_params(options)?;
        let ptrs: Vec<*const f32> = pcms.iter().map(|p| p.as_ptr()).collect();
        let lens: Vec<i32> = pcms
            .iter()
            .map(|p| clamp_len(p.len()))
            .collect::<Result<_>>()?;
        let n = clamp_len(pcms.len())?;

        let status = {
            let guard = self
                .model
                .compute_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *guard {
                return Err(Error::Busy(
                    "a stream is active on this model; finish or drop it before run_batch()".into(),
                ));
            }
            unsafe { sys::transcribe_run_batch(self.ptr, ptrs.as_ptr(), lens.as_ptr(), n, &params) }
        };

        // OK and ABORTED both populate one per-utterance slot per input; fold
        // the batch-level abort into the per-utterance view (the C contract).
        let ok = status == sys::transcribe_status::TRANSCRIBE_OK
            || status == sys::transcribe_status::TRANSCRIBE_ERR_ABORTED;
        if !ok {
            return Err(error_for_status(status, "run_batch"));
        }

        let count = unsafe { sys::transcribe_batch_n_results(self.ptr) };
        let mut results = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            let st = unsafe { sys::transcribe_batch_status(self.ptr, i) };
            if st == sys::transcribe_status::TRANSCRIBE_OK {
                results.push(Ok(self.materialize_batch(i)));
            } else if st == sys::transcribe_status::TRANSCRIBE_ERR_ABORTED
                || st == sys::transcribe_status::TRANSCRIBE_ERR_OUTPUT_TRUNCATED
            {
                let partial = Box::new(self.materialize_batch(i));
                let err = attach_partial(error_for_status(st, "run_batch utterance"), partial);
                results.push(Err(err));
            } else {
                results.push(Err(error_for_status(st, "run_batch utterance")));
            }
        }
        Ok(results)
    }

    /// The effective per-session limits.
    pub fn limits(&self) -> Result<SessionLimits> {
        let mut out: sys::transcribe_session_limits = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_session_limits_init(&mut out) };
        let status = unsafe { sys::transcribe_session_get_limits(self.ptr, &mut out) };
        check(status, "session limits")?;
        Ok(SessionLimits {
            effective_n_ctx: out.effective_n_ctx,
            effective_max_audio_ms: out.effective_max_audio_ms,
            max_kv_bytes: out.max_kv_bytes,
        })
    }

    /// The model this session is bound to.
    pub fn model(&self) -> Model {
        Model {
            inner: Arc::clone(&self.model),
        }
    }

    /// Begin a streaming run, returning a [`Stream`] that borrows this session
    /// for its lifetime (so the session can't be used for an offline `run`
    /// while a stream is active). `run` supplies task / language / timestamps;
    /// `stream` supplies the commit policy and any stream-slot family extension.
    /// Dropping the returned `Stream` abandons it and returns the session to
    /// idle.
    pub fn stream(&mut self, run: &RunOptions, stream: &StreamOptions) -> Result<Stream<'_>> {
        let (run_params, _lang, _target, _family) = build_run_params(run)?;
        let (stream_params, _stream_family) = build_stream_params(stream);
        {
            // Claim the model's compute lease for the whole stream lifetime: a
            // stream spans begin..drop, so per-call locking alone would let a
            // second stream (or a run) on another session race it.
            let mut guard = self
                .model
                .compute_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *guard {
                return Err(Error::Busy(
                    "a stream is already active on this model".into(),
                ));
            }
            check(
                unsafe { sys::transcribe_stream_begin(self.ptr, &run_params, &stream_params) },
                "stream begin",
            )?;
            *guard = true; // released at finalize/reset, or by Stream::drop
        }
        Ok(Stream {
            session: self,
            holds_lease: true,
        })
    }

    // --- result materialization (top-level / single accessors) ---------------

    fn materialize_run(&self) -> Transcript {
        let n_seg = unsafe { sys::transcribe_n_segments(self.ptr) };
        let n_word = unsafe { sys::transcribe_n_words(self.ptr) };
        let n_tok = unsafe { sys::transcribe_n_tokens(self.ptr) };

        Transcript {
            text: owned_str(unsafe { sys::transcribe_full_text(self.ptr) }),
            language: owned_opt_str(unsafe { sys::transcribe_detected_language(self.ptr) }),
            timestamp_kind: TimestampKind::from_raw(unsafe {
                sys::transcribe_returned_timestamp_kind(self.ptr)
            }),
            segments: (0..n_seg).map(|i| self.segment(i)).collect(),
            words: (0..n_word).map(|i| self.word(i)).collect(),
            tokens: (0..n_tok).map(|i| self.token(i)).collect(),
            timings: self.timings(),
        }
    }

    fn segment(&self, i: i32) -> Segment {
        let mut raw: sys::transcribe_segment = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_segment_init(&mut raw) };
        let _ = unsafe { sys::transcribe_get_segment(self.ptr, i, &mut raw) };
        Segment::from_raw(&raw)
    }

    fn word(&self, i: i32) -> Word {
        let mut raw: sys::transcribe_word = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_word_init(&mut raw) };
        let _ = unsafe { sys::transcribe_get_word(self.ptr, i, &mut raw) };
        Word::from_raw(&raw)
    }

    fn token(&self, i: i32) -> Token {
        let mut raw: sys::transcribe_token = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_token_init(&mut raw) };
        let _ = unsafe { sys::transcribe_get_token(self.ptr, i, &mut raw) };
        Token::from_raw(&raw)
    }

    fn timings(&self) -> Timings {
        let mut raw: sys::transcribe_timings = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_timings_init(&mut raw) };
        let _ = unsafe { sys::transcribe_get_timings(self.ptr, &mut raw) };
        Timings::from_raw(&raw)
    }

    // --- result materialization (batch accessors, utterance i) ---------------

    fn materialize_batch(&self, i: i32) -> Transcript {
        let n_seg = unsafe { sys::transcribe_batch_n_segments(self.ptr, i) };
        let n_word = unsafe { sys::transcribe_batch_n_words(self.ptr, i) };
        let n_tok = unsafe { sys::transcribe_batch_n_tokens(self.ptr, i) };

        Transcript {
            text: owned_str(unsafe { sys::transcribe_batch_full_text(self.ptr, i) }),
            language: owned_opt_str(unsafe {
                sys::transcribe_batch_detected_language(self.ptr, i)
            }),
            timestamp_kind: TimestampKind::from_raw(unsafe {
                sys::transcribe_batch_returned_timestamp_kind(self.ptr, i)
            }),
            segments: (0..n_seg).map(|j| self.batch_segment(i, j)).collect(),
            words: (0..n_word).map(|j| self.batch_word(i, j)).collect(),
            tokens: (0..n_tok).map(|j| self.batch_token(i, j)).collect(),
            timings: self.batch_timings(i),
        }
    }

    fn batch_segment(&self, i: i32, j: i32) -> Segment {
        let mut raw: sys::transcribe_segment = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_segment_init(&mut raw) };
        let _ = unsafe { sys::transcribe_batch_get_segment(self.ptr, i, j, &mut raw) };
        Segment::from_raw(&raw)
    }

    fn batch_word(&self, i: i32, j: i32) -> Word {
        let mut raw: sys::transcribe_word = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_word_init(&mut raw) };
        let _ = unsafe { sys::transcribe_batch_get_word(self.ptr, i, j, &mut raw) };
        Word::from_raw(&raw)
    }

    fn batch_token(&self, i: i32, j: i32) -> Token {
        let mut raw: sys::transcribe_token = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_token_init(&mut raw) };
        let _ = unsafe { sys::transcribe_batch_get_token(self.ptr, i, j, &mut raw) };
        Token::from_raw(&raw)
    }

    fn batch_timings(&self, i: i32) -> Timings {
        let mut raw: sys::transcribe_timings = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_timings_init(&mut raw) };
        let _ = unsafe { sys::transcribe_batch_get_timings(self.ptr, i, &mut raw) };
        Timings::from_raw(&raw)
    }
}

/// Everything that must outlive a `transcribe_run` call: the params struct
/// plus the heap buffers its pointers borrow (language strings, family ext).
type RunParamsBundle = (
    sys::transcribe_run_params,
    Option<CString>,
    Option<CString>,
    Option<RunExtRaw>,
);

/// Build `transcribe_run_params` from options. The returned keepalives own the
/// buffers the params' pointers borrow, so the caller must hold them for the
/// duration of the native call.
fn build_run_params(o: &RunOptions) -> Result<RunParamsBundle> {
    let mut params: sys::transcribe_run_params = unsafe { std::mem::zeroed() };
    unsafe { sys::transcribe_run_params_init(&mut params) };

    params.task = o.task.to_raw();
    params.timestamps = o.timestamps.to_raw();
    params.pnc = o.pnc.to_raw();
    params.itn = o.itn.to_raw();
    params.keep_special_tags = o.keep_special_tags;
    params.spec_k_drafts = o.spec_k_drafts;
    params.max_new_tokens = o.max_new_tokens;

    let lang = o.language.as_deref().map(CString::new).transpose()?;
    let target = o.target_language.as_deref().map(CString::new).transpose()?;
    params.language = lang.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    params.target_language = target.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

    let family = o
        .family
        .as_ref()
        .map(RunExtension::materialize)
        .transpose()?;
    params.family = family.as_ref().map_or(std::ptr::null(), |f| f.ext_ptr());

    Ok((params, lang, target, family))
}

/// PCM/utterance lengths cross the ABI as `int`; reject anything that overflows.
fn clamp_len(len: usize) -> Result<i32> {
    i32::try_from(len).map_err(|_| Error::InvalidArgument(format!("length {len} exceeds i32::MAX")))
}

/// Replace the (partial-less) Aborted/OutputTruncated error with one carrying
/// the materialized partial transcript.
fn attach_partial(err: Error, partial: Box<Transcript>) -> Error {
    match err {
        Error::Aborted { message, .. } => Error::Aborted {
            message,
            partial: Some(partial),
        },
        Error::OutputTruncated { message, .. } => Error::OutputTruncated {
            message,
            partial: Some(partial),
        },
        other => other,
    }
}

/// An active streaming run, borrowed from a [`Session`].
///
/// Feed PCM with [`Stream::feed`], read the UI-stable text with
/// [`Stream::text`], and end input with [`Stream::finalize`]. Dropping the
/// `Stream` (without finalizing) abandons it and returns the session to idle.
/// `Send` but not `Sync`, like the session it borrows.
pub struct Stream<'a> {
    session: &'a mut Session,
    // True while this stream holds the model's compute lease. Set at begin,
    // cleared the moment the stream stops being active (finalize/reset) or on
    // drop — whichever comes first. Tracked per-stream so drop never releases a
    // lease a *different* session has since acquired.
    holds_lease: bool,
}

impl std::fmt::Debug for Stream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream")
            .field("state", &self.state())
            .field("revision", &self.revision())
            .finish()
    }
}

impl Drop for Stream<'_> {
    fn drop(&mut self) {
        // Abandon any unfinalized stream (reset is idempotent and safe from any
        // state) and release the lease IF this stream still holds it — under the
        // lock. After finalize/reset the lease is already gone (and may now be
        // another session's), so only clear it when holds_lease is still true.
        let mut guard = self
            .session
            .model
            .compute_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe { sys::transcribe_stream_reset(self.session.ptr) };
        if self.holds_lease {
            *guard = false;
        }
    }
}

impl Stream<'_> {
    /// Feed a chunk of 16 kHz mono float32 PCM and return the change metadata.
    pub fn feed(&mut self, pcm: &[f32]) -> Result<StreamUpdate> {
        let n = clamp_len(pcm.len())?;
        let mut update: sys::transcribe_stream_update = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_stream_update_init(&mut update) };
        let status = {
            // This stream already holds the model's compute lease (it is in
            // flight); the lock here just serializes the native call.
            let _guard = self
                .session
                .model
                .compute_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { sys::transcribe_stream_feed(self.session.ptr, pcm.as_ptr(), n, &mut update) }
        };
        check(status, "stream feed")?;
        Ok(StreamUpdate::from_raw(&update))
    }

    /// Signal end of input: flush buffered audio and emit the final text. The
    /// session transitions to finished.
    pub fn finalize(&mut self) -> Result<StreamUpdate> {
        let mut update: sys::transcribe_stream_update = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_stream_update_init(&mut update) };
        let status = {
            let mut guard = self
                .session
                .model
                .compute_lock
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let st = unsafe { sys::transcribe_stream_finalize(self.session.ptr, &mut update) };
            // Finalize ends the active stream (Finished, or Failed on error);
            // either way it is no longer active, so free the model's compute
            // lease now rather than holding other sessions off until drop.
            if self.holds_lease {
                *guard = false;
            }
            st
        };
        self.holds_lease = false;
        check(status, "stream finalize")?;
        Ok(StreamUpdate::from_raw(&update))
    }

    /// Abandon the stream and return the session to idle (clears all snapshot
    /// state). Releases the model's compute lease (the stream is no longer
    /// active), so other sessions of the model can proceed without waiting for
    /// this `Stream` to drop.
    pub fn reset(&mut self) {
        let mut guard = self
            .session
            .model
            .compute_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe { sys::transcribe_stream_reset(self.session.ptr) };
        if self.holds_lease {
            *guard = false;
        }
        self.holds_lease = false;
    }

    /// The current UI-facing text snapshot (owned copies).
    pub fn text(&self) -> StreamText {
        let mut raw: sys::transcribe_stream_text = unsafe { std::mem::zeroed() };
        unsafe { sys::transcribe_stream_text_init(&mut raw) };
        let _ = unsafe { sys::transcribe_stream_get_text(self.session.ptr, &mut raw) };
        StreamText::from_raw(&raw)
    }

    /// A full structured snapshot (segments/words/tokens) of the current
    /// hypothesis. For most UIs [`Stream::text`] is the better choice.
    pub fn snapshot(&self) -> Transcript {
        self.session.materialize_run()
    }

    /// The stream's lifecycle state.
    pub fn state(&self) -> StreamState {
        StreamState::from_raw(unsafe { sys::transcribe_stream_get_state(self.session.ptr) })
    }

    /// The monotonic snapshot revision; diff against the previous value.
    pub fn revision(&self) -> i32 {
        unsafe { sys::transcribe_stream_revision(self.session.ptr) }
    }

    /// The stream's recorded terminal failure, or `None` while it is healthy.
    ///
    /// Set after a [`feed`](Self::feed) / [`finalize`](Self::finalize)
    /// transitioned the stream to [`StreamState::Failed`]; reset by a new
    /// stream. Inspect it when [`state`](Self::state) is `Failed` to learn why.
    pub fn last_status(&self) -> Option<Error> {
        let status = unsafe { sys::transcribe_stream_last_status(self.session.ptr) };
        if status == sys::transcribe_status::TRANSCRIBE_OK {
            None
        } else {
            Some(error_for_status(status, "stream"))
        }
    }

    /// Whether the stream was ended by cancellation.
    pub fn was_aborted(&self) -> bool {
        self.session.was_aborted()
    }
}
