//! A recording's live decode: a stream the model decodes as audio arrives,
//! with preview text emitted to the overlay.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use log::{error, info};
use transcribe_cpp::StreamOptions;

use super::{
    emit_stream_text, lock_perf, run_options, StreamFinalizeReply, StreamPerf, TranscriptionManager,
};
use crate::engine_supervisor::{LoadedInfo, StreamHandle, StreamProgress};

/// One recording's stream.
pub(super) struct StreamSession<'a> {
    tm: &'a TranscriptionManager,
    stream: StreamHandle,
    perf: Arc<Mutex<StreamPerf>>,
}

impl<'a> StreamSession<'a> {
    /// Begins the stream on the loaded model. `None` when it couldn't begin:
    /// the recording is then transcribed in batch when it stops.
    pub(super) fn begin(tm: &'a TranscriptionManager, info: &LoadedInfo) -> Option<Self> {
        // Feed results arrive on the engine's thread; this callback only
        // emits preview text.
        let perf = Arc::new(Mutex::new(StreamPerf::new()));
        let on_progress = {
            let perf = Arc::clone(&perf);
            let app_handle = tm.app_handle.clone();
            move |progress: StreamProgress| {
                let mut perf = lock_perf(&perf);
                perf.record_compute(progress.elapsed);
                perf.record_update(&progress.update);
                if let Some(text) = progress.text {
                    perf.record_emit();
                    emit_stream_text(&app_handle, &text.committed, &text.tentative);
                }
                perf.maybe_log();
            }
        };

        // Run the stream in the engine's worker process. Feeds are queued
        // without waiting; a crashed or hung worker makes finalize report no
        // result, and the caller falls back to batch transcription.
        // StreamOptions::default() uses CommitPolicy::Auto and lets the
        // family pick its own streaming strategy.
        let stream =
            match tm
                .engine
                .start_stream(run_options(info), StreamOptions::default(), on_progress)
            {
                Ok(stream) => stream,
                Err(e) => {
                    error!("Failed to begin stream: {}", e);
                    return None;
                }
            };

        tm.stream_active.store(true, Ordering::Release);
        info!(
            "Live streaming transcription started (backend '{}')",
            info.backend
        );
        Some(Self { tm, stream, perf })
    }

    pub(super) fn feed(&mut self, pcm: Vec<f32>) {
        lock_perf(&self.perf).record_feed(pcm.len());
        self.stream.feed(pcm);
    }

    /// The stream's text; `None` to transcribe the recording in batch instead.
    pub(super) fn finish(self) -> StreamFinalizeReply {
        let result = match self.stream.finalize(false) {
            // After finalize the committed prefix holds the full text;
            // display() = committed + tentative is the safe read.
            Ok(Some(finalized)) => {
                let mut perf = lock_perf(&self.perf);
                perf.record_compute(finalized.elapsed);
                perf.record_update(&finalized.update);
                if finalized.text.full.trim().is_empty() {
                    // A stream can miss what a run over the whole recording
                    // finds, so empty text retries in batch.
                    info!("Live transcription produced no text; retrying in batch");
                    Ok(None)
                } else {
                    Ok(Some(finalized.text.full))
                }
            }
            Ok(None) => {
                error!(
                    "Live transcription could not be finalized; \
                     falling back to batch transcription"
                );
                Ok(None)
            }
            Err(e) => {
                info!("Live transcription finalize stopped: {}", e);
                Err(e)
            }
        };
        let chars = match &result {
            Ok(Some(text)) => text.len(),
            _ => 0,
        };
        lock_perf(&self.perf).log_finalized(chars);
        let _ = self.tm;
        result
    }
}
