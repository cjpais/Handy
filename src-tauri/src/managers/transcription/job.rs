//! Background transcription, always chunked: chunks are the unit of
//! progress, of yielding to dictation, and of retry.

use anyhow::Result;
use log::{debug, info, warn};
use std::time::Instant;
use transcribe_cpp::Transcript;

use super::{
    agreed_language, engine_error, post_process_transcription_text, real_time_factor, BatchContext,
    Transcription, TranscriptionManager,
};
use crate::audio_toolkit::chunking::Chunker;
use crate::engine_supervisor::EngineError;
use crate::managers::audio::create_detector;

/// Tries per chunk when a dictation's cancel interrupts it.
const MAX_CHUNK_ATTEMPTS: usize = 3;

/// Audio handed to the chunker at a time: the first chunk is transcribed
/// without scoring the whole file first. Cuts match the whole-file plan (a
/// rare short blip at a feed boundary can nudge one within its pause).
const FEED_SAMPLES: usize = 60 * 16_000;

/// What a background transcription asks of whoever runs it.
pub trait ChunkControl {
    /// Called before each feed to the chunker and before each chunk is
    /// decoded. Blocks while dictation is using the engine; `false` stops the
    /// transcription.
    fn proceed(&self) -> bool;
    /// `done` of `total` samples are transcribed.
    fn progress(&self, done: usize, total: usize);
}

impl TranscriptionManager {
    /// Transcribe `audio` chunk by chunk with the selected model, one
    /// paragraph per chunk. `None` if `control` stopped it.
    pub fn transcribe_in_chunks(
        &self,
        audio: Vec<f32>,
        control: &dyn ChunkControl,
    ) -> Result<Option<Transcription>> {
        let started = Instant::now();
        let total = audio.len();
        if total == 0 {
            return Ok(Some(Transcription {
                text: String::new(),
                language: None,
            }));
        }

        self.touch_activity();
        self.initiate_model_load();
        // Read once: the model can't change until the job ends.
        let BatchContext {
            settings,
            active_model,
            setup,
            ..
        } = self.batch_context(&audio)?;

        let backend = settings.vad_backend;
        let app_handle = self.app_handle.clone();
        let mut chunker = Chunker::new(Box::new(move || create_detector(backend, &app_handle)));
        let mut pieces = audio.chunks(FEED_SAMPLES);

        let mut transcripts: Vec<Transcript> = Vec::new();
        let mut done = 0;
        control.progress(done, total);
        loop {
            // Scoring waits out dictation too, and stops on cancel.
            if !control.proceed() {
                return Ok(None);
            }
            let (chunks, last) = match pieces.next() {
                Some(piece) => (chunker.push(piece), false),
                None => (chunker.finish(), true),
            };
            for chunk in chunks {
                let len = chunk.len();
                let mut attempts = 0;
                let transcript = loop {
                    if !control.proceed() {
                        return Ok(None);
                    }
                    attempts += 1;
                    // A fresh mark per chunk: a dictation's cancel stops only
                    // the chunk it caught, which is then tried again.
                    let mark = self.engine.cancel_mark();
                    match self.run_batch(mark, chunk.clone(), &setup.run_options, &active_model) {
                        Ok(transcript) => break transcript,
                        Err(EngineError::Cancelled) if attempts < MAX_CHUNK_ATTEMPTS => {
                            warn!("A background chunk was cancelled; trying it again");
                        }
                        Err(e) => return Err(engine_error(e)),
                    }
                };
                transcripts.push(transcript);
                done += len;
                control.progress(done, total);
            }
            if last {
                break;
            }
        }

        debug!("Transcribed {} chunks in the background", transcripts.len());
        let language = setup.spoken_language(agreed_language(&transcripts).as_deref());
        let paragraphs: Vec<String> = transcripts
            .into_iter()
            .map(|t| {
                let output_language = setup.output_language(&settings, t.language);
                post_process_transcription_text(
                    t.text,
                    &settings,
                    setup.custom_words_prompted(),
                    &output_language,
                    &setup.languages,
                )
            })
            .filter(|p| !p.trim().is_empty())
            .collect();

        let elapsed_secs = started.elapsed().as_secs_f64();
        let audio_secs = total as f64 / 16_000.0;
        info!(
            "Background transcription of {:.1}s of audio in {} paragraphs took {:.1}s ({:.1}x real time)",
            audio_secs,
            paragraphs.len(),
            elapsed_secs,
            real_time_factor(audio_secs, elapsed_secs)
        );
        Ok(Some(Transcription {
            text: paragraphs.join("\n\n"),
            language: Some(language),
        }))
    }
}
