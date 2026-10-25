//! A recording's live decode: a stream the model decodes as audio arrives,
//! or long-form chunks decoded as the recording goes.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use log::{debug, error, info};
use tauri_specta::Event;
use transcribe_cpp::StreamOptions;

use super::chunked::ChunkedDecode;
use super::policy::LiveDecode;
use super::{
    effective_language_for_model, emit_stream_text, join_chunks, lock_perf, run_setup,
    PreviewScript, RunOutcome, RunSetup, StreamFinalizeReply, StreamPerf, StreamPhase,
    StreamPhaseEvent, StreamWorkKind, TranscriptionManager,
};
use crate::audio_toolkit::OutputLanguageEvidence;
use crate::engine_supervisor::{EngineError, LoadedInfo, StreamHandle, StreamProgress};
use crate::settings::{get_settings, AppSettings};

/// One recording's live decode, with the run setup its result finishes with.
pub(super) struct LiveSession<'a> {
    setup: RunSetup,
    decoder: LiveDecoder<'a>,
}

enum LiveDecoder<'a> {
    Stream(StreamSession<'a>),
    Chunked(Box<ChunkedDecode<'a>>),
}

/// A decoded text and the language the model detected in it, if any.
type Decoded = (String, Option<String>);

impl<'a> LiveSession<'a> {
    /// Begins the live decode `mode` picks on the loaded model. `None` when
    /// there is none or it couldn't begin: the recording is then transcribed
    /// in batch when it stops.
    pub(super) fn begin(
        tm: &'a TranscriptionManager,
        mode: LiveDecode,
        info: &LoadedInfo,
        model_id: &str,
    ) -> Option<Self> {
        if mode == LiveDecode::None {
            return None;
        }
        // The same run setup as the batch path: task + language gated
        // against what the model actually advertises.
        let settings = get_settings(&tm.app_handle);
        let effective_language =
            effective_language_for_model(&settings, tm.model_manager.as_ref(), model_id);
        let setup = run_setup(info, &settings, &effective_language);
        let decoder = match mode {
            LiveDecode::None => unreachable!("returned above"),
            LiveDecode::Stream => {
                LiveDecoder::Stream(StreamSession::begin(tm, info, &settings, &setup, model_id)?)
            }
            LiveDecode::Chunked => {
                info!("Live long-form chunking started (model '{}')", model_id);
                LiveDecoder::Chunked(Box::new(ChunkedDecode::new(
                    tm,
                    tm.engine.cancel_mark(),
                    setup.run_options.clone(),
                    model_id,
                    settings.vad_backend,
                )))
            }
        };
        Some(Self { setup, decoder })
    }

    pub(super) fn feed(&mut self, pcm: Vec<f32>) {
        match &mut self.decoder {
            LiveDecoder::Stream(stream) => stream.feed(pcm),
            LiveDecoder::Chunked(chunks) => chunks.push(pcm),
        }
    }

    /// The live result; `None` to transcribe the recording in batch instead.
    /// `recorded_samples` is the whole recording's length.
    pub(super) fn finish(self, recorded_samples: usize) -> StreamFinalizeReply {
        let decoded = match self.decoder {
            LiveDecoder::Stream(stream) => stream.finish()?,
            LiveDecoder::Chunked(chunks) => finish_chunks(*chunks, recorded_samples)?,
        };
        Ok(decoded.map(|(text, model_detected_language)| RunOutcome {
            setup: self.setup,
            text,
            model_detected_language,
        }))
    }
}

fn finish_chunks(
    chunks: ChunkedDecode<'_>,
    recorded_samples: usize,
) -> Result<Option<Decoded>, EngineError> {
    // A failure stops the decode short of the recording; report the failure.
    if chunks.samples() != recorded_samples && !chunks.failed() {
        debug!(
            "Live chunks saw {} of the recording's {} samples; transcribing it in batch",
            chunks.samples(),
            recorded_samples
        );
        return Ok(None);
    }
    let while_recording = chunks.decoded();
    match chunks.finish() {
        Ok(transcripts) => {
            if while_recording > 0 {
                info!(
                    "{} of {} chunks were decoded while recording",
                    while_recording,
                    transcripts.len()
                );
            }
            Ok(Some(join_chunks(transcripts)))
        }
        Err(EngineError::Cancelled) => Err(EngineError::Cancelled),
        Err(e) => {
            info!("Live chunks failed ({}); transcribing in batch", e);
            Ok(None)
        }
    }
}

/// A stream the model decodes as audio arrives, with preview text emitted
/// to the overlay.
struct StreamSession<'a> {
    tm: &'a TranscriptionManager,
    model_id: String,
    stream: StreamHandle,
    perf: Arc<Mutex<StreamPerf>>,
    /// In auto mode the model's own LID is the best remaining evidence; the
    /// snapshot is only materialized when it can change the outcome.
    want_language: bool,
}

impl<'a> StreamSession<'a> {
    fn begin(
        tm: &'a TranscriptionManager,
        info: &LoadedInfo,
        settings: &AppSettings,
        setup: &RunSetup,
        model_id: &str,
    ) -> Option<Self> {
        let output_language = setup.output_language(settings, None);

        // Feed results arrive on the engine's thread; this callback only
        // converts and emits preview text.
        let perf = Arc::new(Mutex::new(StreamPerf::new()));
        let on_progress = {
            let perf = Arc::clone(&perf);
            let app_handle = tm.app_handle.clone();
            let languages = setup.languages.clone();
            let mut preview_script = PreviewScript::new(settings.chinese_script, &output_language);
            move |progress: StreamProgress| {
                let mut perf = lock_perf(&perf);
                perf.record_compute(progress.elapsed);
                perf.record_update(&progress.update);
                if let Some(text) = progress.text {
                    perf.record_emit();
                    let (committed, tentative) =
                        preview_script.convert(&text.committed, &text.tentative, &languages);
                    emit_stream_text(&app_handle, &committed, &tentative);
                }
                perf.maybe_log();
            }
        };

        // Run the stream in the engine's worker process. Feeds are queued
        // without waiting; a crashed or hung worker makes finalize report no
        // result, and the caller falls back to batch transcription.
        // StreamOptions::default() uses CommitPolicy::Auto and lets the
        // family pick its own streaming strategy (no family-specific ext).
        let recovery_app = tm.app_handle.clone();
        let stream = match tm.engine.start_stream_with_failure_handler(
            setup.run_options.clone(),
            StreamOptions::default(),
            on_progress,
            move || {
                // The audio recorder keeps the full clip for batch recovery
                // after stop; make the unavailable live preview explicit now.
                // Decoder state died with its worker. Replaying into a new live
                // decoder would need duplicate-text and repeated-failure handling;
                // defer full-clip decoding until stop while capture continues.
                let _ = StreamPhaseEvent {
                    phase: StreamPhase::Working,
                    kind: Some(StreamWorkKind::Recovering),
                }
                .emit(&recovery_app);
            },
        ) {
            Ok(stream) => stream,
            Err(e) => {
                error!("Failed to begin stream: {}", e);
                tm.forget_model_if_engine_dropped(model_id, &e.to_string());
                return None;
            }
        };

        tm.stream_active.store(true, Ordering::Release);
        info!(
            "Live streaming transcription started (model '{}', backend '{}')",
            model_id, info.backend
        );
        Some(Self {
            tm,
            model_id: model_id.to_string(),
            stream,
            perf,
            want_language: matches!(output_language, OutputLanguageEvidence::Unknown),
        })
    }

    fn feed(&mut self, pcm: Vec<f32>) {
        lock_perf(&self.perf).record_feed(pcm.len());
        self.stream.feed(pcm);
    }

    fn finish(self) -> Result<Option<Decoded>, EngineError> {
        let result = match self.stream.finalize(self.want_language) {
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
                    Ok(Some((finalized.text.full, finalized.language)))
                }
            }
            Ok(None) => {
                error!(
                    "Live transcription could not be finalized; \
                     falling back to batch transcription"
                );
                self.tm.forget_model_if_engine_dropped(
                    &self.model_id,
                    "live transcription failed and the model could not be restarted",
                );
                Ok(None)
            }
            Err(e) => {
                info!("Live transcription finalize stopped: {}", e);
                Err(e)
            }
        };
        let chars = match &result {
            Ok(Some((text, _))) => text.len(),
            _ => 0,
        };
        lock_perf(&self.perf).log_finalized(chars);
        result
    }
}
