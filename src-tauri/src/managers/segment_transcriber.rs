use crate::managers::transcription::TranscriptionManager;
use log::{debug, error};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Messages sent to the segment-transcription worker thread.
enum SegmentMsg {
    /// A completed speech segment (16kHz mono f32) to transcribe.
    Audio(Vec<f32>),
    /// Recording finished — stop the worker and return accumulated texts.
    End,
}

/// Transcribes VAD-delimited speech segments on a dedicated worker thread while
/// recording is still in progress. Each segment is run through the same
/// `TranscriptionManager` used by the batch path, so this works for every engine.
/// Results are kept in arrival order; the engine's own mutex serialises the calls,
/// so segments are transcribed one at a time, in order.
///
/// Only raw ASR happens here. Post-processing (Chinese conversion, LLM cleanup)
/// is intentionally NOT done per segment — the caller joins the segment texts and
/// runs post-processing once on the full transcript.
pub struct SegmentTranscriber {
    tx: Sender<SegmentMsg>,
    handle: JoinHandle<Vec<String>>,
    cancelled: Arc<AtomicBool>,
}

impl SegmentTranscriber {
    /// Spawn the worker. It blocks on `transcribe_segment` (which itself waits for
    /// the model to finish loading), so segments queued before the model is ready
    /// are handled correctly and in order.
    pub fn spawn(tm: Arc<TranscriptionManager>) -> Self {
        let (tx, rx) = mpsc::channel::<SegmentMsg>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();

        let handle = thread::spawn(move || {
            let mut texts: Vec<String> = Vec::new();
            while let Ok(msg) = rx.recv() {
                match msg {
                    SegmentMsg::Audio(samples) => {
                        // On cancel, drain the queue without transcribing so the
                        // engine is freed promptly for the next recording.
                        if worker_cancelled.load(Ordering::Relaxed) {
                            continue;
                        }
                        // Use transcribe_segment so an "Immediately" unload setting
                        // doesn't unload the model between segments of one recording.
                        match tm.transcribe_segment(samples) {
                            Ok(text) => {
                                debug!("Segment transcribed ({} chars)", text.len());
                                texts.push(text);
                            }
                            Err(e) => {
                                // Don't abort the whole recording on one bad segment —
                                // keep ordering by pushing an empty placeholder.
                                error!("Segment transcription failed: {}", e);
                                texts.push(String::new());
                            }
                        }
                    }
                    SegmentMsg::End => break,
                }
            }
            texts
        });

        Self {
            tx,
            handle,
            cancelled,
        }
    }

    /// Queue a completed speech segment for transcription. Non-blocking.
    pub fn push(&self, segment: Vec<f32>) {
        let _ = self.tx.send(SegmentMsg::Audio(segment));
    }

    /// Signal end-of-recording, wait for all queued segments to finish, and return
    /// the transcribed texts in arrival order.
    pub fn finish(self) -> Vec<String> {
        let _ = self.tx.send(SegmentMsg::End);
        match self.handle.join() {
            Ok(texts) => texts,
            Err(e) => {
                // A panic in the worker would otherwise be swallowed, leaving the
                // user with no paste and no error. Surface it instead.
                error!("Segment transcriber thread panicked: {:?}", e);
                Vec::new()
            }
        }
    }

    /// Abandon the worker (recording cancelled). Sets the cancel flag so any
    /// already-queued segments are skipped instead of transcribed, then drops the
    /// sender so the worker exits. The thread is detached (results discarded); at
    /// most one in-flight segment finishes before the flag takes effect.
    pub fn cancel(self) {
        self.cancelled.store(true, Ordering::Relaxed);
        drop(self.tx);
    }
}
