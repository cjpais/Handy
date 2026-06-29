use crate::audio_toolkit::vad::{SmoothedVad, VadFrame};
use crate::audio_toolkit::{SileroVad, VoiceActivityDetector};
use crate::managers::transcription::TranscriptionManager;
use crate::utils;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};

const SAMPLE_RATE: usize = 16_000;
const MIN_CHUNK_SECS: f32 = 1.0;
const MAX_CHUNK_SECS: f32 = 15.0;
const MIN_FINAL_SECS: f32 = 0.35;

pub struct StreamingFinishResult {
    pub audio: Vec<f32>,
    pub combined_text: String,
}

pub struct StreamingSession {
    tm: Arc<TranscriptionManager>,
    text_buf: Arc<Mutex<Vec<String>>>,
    audio_buf: Arc<Mutex<Vec<f32>>>,
    cancelled: Arc<AtomicBool>,
    worker_handle: Option<thread::JoinHandle<()>>,
    active: bool,
}

impl StreamingSession {
    pub fn start(
        tm: Arc<TranscriptionManager>,
        app: AppHandle,
        chunk_rx: mpsc::Receiver<Vec<f32>>,
        vad_model_path: String,
        live_paste: bool,
    ) -> Self {
        let text_buf = Arc::new(Mutex::new(Vec::new()));
        let audio_buf = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let text_buf_worker = Arc::clone(&text_buf);
        let audio_buf_worker = Arc::clone(&audio_buf);
        let cancelled_worker = Arc::clone(&cancelled);
        let tm_worker = Arc::clone(&tm);

        tm.begin_streaming();

        let worker_handle = thread::spawn(move || {
            info!(
                "VAD chunked streaming worker starting (live_paste: {})",
                live_paste
            );
            let mut chunker = match VadChunker::from_model(&vad_model_path) {
                Ok(chunker) => chunker,
                Err(err) => {
                    error!("Failed to initialize streaming VAD: {err}");
                    return;
                }
            };

            for frame in chunk_rx {
                if cancelled_worker.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(chunk) = chunker.push_frame(&frame) {
                    transcribe_chunk(
                        &tm_worker,
                        &app,
                        &text_buf_worker,
                        &audio_buf_worker,
                        &cancelled_worker,
                        chunk,
                        live_paste,
                    );
                }
            }

            if !cancelled_worker.load(Ordering::Relaxed) {
                info!("VAD chunked streaming input ended; flushing final chunk");
                if let Some(chunk) = chunker.finish() {
                    transcribe_chunk(
                        &tm_worker,
                        &app,
                        &text_buf_worker,
                        &audio_buf_worker,
                        &cancelled_worker,
                        chunk,
                        live_paste,
                    );
                }
            }

            debug!("Streaming transcription worker finished");
        });

        Self {
            tm,
            text_buf,
            audio_buf,
            cancelled,
            worker_handle: Some(worker_handle),
            active: true,
        }
    }

    pub fn finish(mut self) -> StreamingFinishResult {
        if let Some(handle) = self.worker_handle.take() {
            if let Err(err) = handle.join() {
                error!("Streaming transcription worker panicked: {err:?}");
            }
        }

        self.active = false;
        self.tm.end_streaming();

        StreamingFinishResult {
            audio: std::mem::take(&mut *self.audio_buf.lock().unwrap()),
            combined_text: self.text_buf.lock().unwrap().join(" "),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for StreamingSession {
    fn drop(&mut self) {
        if self.active {
            self.cancel();
            self.tm.end_streaming();
        }
    }
}

struct VadChunker {
    vad: Box<dyn VoiceActivityDetector>,
    current_chunk: Vec<f32>,
    min_chunk_samples: usize,
    max_chunk_samples: usize,
    min_final_samples: usize,
    frames_seen: usize,
    speech_frames: usize,
    noise_frames: usize,
    chunks_emitted: usize,
    short_pause_logged: bool,
}

impl VadChunker {
    fn from_model(vad_model_path: &str) -> Result<Self, anyhow::Error> {
        let silero = SileroVad::new(vad_model_path, 0.3)
            .map_err(|e| anyhow::anyhow!("Failed to create SileroVad: {}", e))?;
        let smoothed_vad = SmoothedVad::new(Box::new(silero), 15, 15, 2);
        Ok(Self::new(Box::new(smoothed_vad)))
    }

    fn new(vad: Box<dyn VoiceActivityDetector>) -> Self {
        Self {
            vad,
            current_chunk: Vec::new(),
            min_chunk_samples: (SAMPLE_RATE as f32 * MIN_CHUNK_SECS) as usize,
            max_chunk_samples: (SAMPLE_RATE as f32 * MAX_CHUNK_SECS) as usize,
            min_final_samples: (SAMPLE_RATE as f32 * MIN_FINAL_SECS) as usize,
            frames_seen: 0,
            speech_frames: 0,
            noise_frames: 0,
            chunks_emitted: 0,
            short_pause_logged: false,
        }
    }

    fn push_frame(&mut self, frame: &[f32]) -> Option<Vec<f32>> {
        self.frames_seen += 1;
        match self.vad.push_frame(frame) {
            Ok(VadFrame::Speech(samples)) => {
                self.speech_frames += 1;
                if self.current_chunk.is_empty() {
                    info!(
                        "VAD chunked streaming detected speech start at frame {}",
                        self.frames_seen
                    );
                }
                self.short_pause_logged = false;
                self.current_chunk.extend_from_slice(samples);
                if self.current_chunk.len() >= self.max_chunk_samples {
                    return self.take_current_chunk("max_duration");
                }
            }
            Ok(VadFrame::Noise) => {
                self.noise_frames += 1;
                if self.current_chunk.len() >= self.min_chunk_samples {
                    return self.take_current_chunk("speech_pause");
                } else if !self.current_chunk.is_empty() && !self.short_pause_logged {
                    info!(
                        "VAD chunked streaming saw pause but current chunk is below minimum ({:.2}s < {:.2}s)",
                        samples_to_secs(self.current_chunk.len()),
                        MIN_CHUNK_SECS
                    );
                    self.short_pause_logged = true;
                }
            }
            Err(err) => warn!("Streaming VAD failed for frame: {err}"),
        }
        if self.frames_seen % 100 == 0 {
            info!(
                "VAD chunked streaming stats: frames={}, speech={}, noise={}, buffered={:.2}s, chunks={}",
                self.frames_seen,
                self.speech_frames,
                self.noise_frames,
                samples_to_secs(self.current_chunk.len()),
                self.chunks_emitted
            );
        }
        None
    }

    fn finish(&mut self) -> Option<Vec<f32>> {
        info!(
            "VAD chunked streaming finish: frames={}, speech={}, noise={}, buffered={:.2}s, chunks={}",
            self.frames_seen,
            self.speech_frames,
            self.noise_frames,
            samples_to_secs(self.current_chunk.len()),
            self.chunks_emitted
        );
        if self.current_chunk.len() >= self.min_final_samples {
            self.take_current_chunk("final_flush")
        } else {
            if !self.current_chunk.is_empty() {
                info!(
                    "VAD chunked streaming dropped final buffered audio below minimum ({:.2}s < {:.2}s)",
                    samples_to_secs(self.current_chunk.len()),
                    MIN_FINAL_SECS
                );
            }
            self.current_chunk.clear();
            None
        }
    }

    fn take_current_chunk(&mut self, reason: &str) -> Option<Vec<f32>> {
        if self.current_chunk.is_empty() {
            None
        } else {
            self.chunks_emitted += 1;
            info!(
                "VAD chunked streaming emitting chunk #{} ({:.2}s, reason: {})",
                self.chunks_emitted,
                samples_to_secs(self.current_chunk.len()),
                reason
            );
            Some(std::mem::take(&mut self.current_chunk))
        }
    }
}

fn samples_to_secs(samples: usize) -> f32 {
    samples as f32 / SAMPLE_RATE as f32
}

fn transcribe_chunk(
    tm: &TranscriptionManager,
    app: &AppHandle,
    text_buf: &Arc<Mutex<Vec<String>>>,
    audio_buf: &Arc<Mutex<Vec<f32>>>,
    cancelled: &Arc<AtomicBool>,
    audio: Vec<f32>,
    live_paste: bool,
) {
    if cancelled.load(Ordering::Relaxed) {
        return;
    }

    let sample_count = audio.len();
    info!(
        "Transcribing VAD chunk ({:.2}s, live_paste: {})",
        samples_to_secs(sample_count),
        live_paste
    );
    audio_buf.lock().unwrap().extend_from_slice(&audio);

    match tm.transcribe(audio) {
        Ok(text) => {
            if cancelled.load(Ordering::Relaxed) {
                return;
            }

            let text = text.trim();
            if text.is_empty() {
                debug!("Streaming chunk produced empty text ({sample_count} samples)");
                return;
            }

            debug!("Streaming chunk transcribed: '{text}'");
            text_buf.lock().unwrap().push(text.to_string());

            if live_paste {
                paste_on_main_thread(app, format_live_chunk_for_paste(text));
            }
        }
        Err(err) => error!("Streaming chunk transcription failed: {err}"),
    }
}

fn format_live_chunk_for_paste(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }

    if text.ends_with(['.', '!', '?']) {
        format!("{text} ")
    } else {
        let text = text
            .trim_end_matches(|c: char| matches!(c, ',' | ';' | ':'))
            .trim_end();
        format!("{text}. ")
    }
}

fn paste_on_main_thread(app: &AppHandle, text: String) {
    let app_clone = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(err) = utils::paste(text, app_clone.clone()) {
            error!("Failed to paste streamed transcription chunk: {err}");
            let _ = app_clone.emit("paste-error", ());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{format_live_chunk_for_paste, VadChunker, MAX_CHUNK_SECS, SAMPLE_RATE};
    use crate::audio_toolkit::vad::VadFrame;
    use crate::audio_toolkit::VoiceActivityDetector;
    use anyhow::Result;

    struct AlwaysSpeechVad;

    impl VoiceActivityDetector for AlwaysSpeechVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>> {
            Ok(VadFrame::Speech(frame))
        }
    }

    #[test]
    fn format_live_chunk_preserves_or_adds_sentence_separator() {
        assert_eq!(
            format_live_chunk_for_paste("hello, world!?  "),
            "hello, world!? "
        );
        assert_eq!(format_live_chunk_for_paste("hello world"), "hello world. ");
        assert_eq!(
            format_live_chunk_for_paste("hello world, "),
            "hello world. "
        );
        assert_eq!(
            format_live_chunk_for_paste("hello, world"),
            "hello, world. "
        );
    }

    #[test]
    fn chunker_flushes_at_hard_max_duration() {
        let mut chunker = VadChunker::new(Box::new(AlwaysSpeechVad));
        let frame = vec![0.1; 480];
        let frame_count = ((MAX_CHUNK_SECS * SAMPLE_RATE as f32) as usize / frame.len()) + 1;
        let mut flushed = None;

        for _ in 0..frame_count {
            flushed = chunker.push_frame(&frame);
            if flushed.is_some() {
                break;
            }
        }

        assert!(flushed.is_some());
    }
}
