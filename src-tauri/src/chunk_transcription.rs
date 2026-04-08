use crate::managers::transcription::TranscriptionManager;
use log::error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};

const CHUNK_QUEUE_CAPACITY: usize = 8;

#[derive(Debug, Default)]
pub struct ChunkSessionResult {
    pub complete: bool,
    pub had_errors: bool,
    pub transcripts: Vec<String>,
}

/// Shared state that owns the currently active chunk transcription session.
pub struct ChunkSessionState {
    inner: Mutex<Option<ChunkSession>>,
}

impl ChunkSessionState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Starts a new chunk transcription session if one is not already running.
    /// Returns a sender that the audio recorder can use to push chunk audio.
    pub fn start(
        &self,
        tm: Arc<TranscriptionManager>,
    ) -> Result<mpsc::SyncSender<Vec<f32>>, String> {
        let mut guard = self.inner.lock().unwrap();
        if guard.is_some() {
            return Err("Chunk session already running".to_string());
        }
        let session = ChunkSession::new(tm);
        let sender = session.chunk_sender();
        *guard = Some(session);
        Ok(sender)
    }

    /// Stops the active session, waits for any pending work, and returns
    /// session metadata plus raw chunk transcripts.
    pub fn stop_and_collect(&self) -> ChunkSessionResult {
        let session = self.inner.lock().unwrap().take();
        session.map(|s| s.finalize()).unwrap_or_default()
    }

    /// Forcibly aborts the active session, discarding any transcripts.
    pub fn abort(&self) {
        let session = self.inner.lock().unwrap().take();
        if let Some(session) = session {
            session.abort();
        }
    }
}

struct ChunkSession {
    chunk_sender: Option<mpsc::SyncSender<Vec<f32>>>,
    worker_handle: Option<JoinHandle<()>>,
    transcripts: Arc<Mutex<Vec<String>>>,
    had_errors: Arc<AtomicBool>,
    abort_requested: Arc<AtomicBool>,
}

impl ChunkSession {
    fn new(tm: Arc<TranscriptionManager>) -> Self {
        let transcripts = Arc::new(Mutex::new(Vec::new()));
        let transcripts_clone = Arc::clone(&transcripts);
        let had_errors = Arc::new(AtomicBool::new(false));
        let had_errors_clone = Arc::clone(&had_errors);
        let abort_requested = Arc::new(AtomicBool::new(false));
        let abort_requested_clone = Arc::clone(&abort_requested);
        let (tx, rx) = mpsc::sync_channel(CHUNK_QUEUE_CAPACITY);

        let handle = thread::spawn(move || {
            for chunk in rx {
                if abort_requested_clone.load(Ordering::Relaxed) {
                    break;
                }
                match tm.transcribe(chunk) {
                    Ok(transcript) => {
                        let mut guard = transcripts_clone.lock().unwrap();
                        guard.push(transcript);
                    }
                    Err(err) => {
                        had_errors_clone.store(true, Ordering::Relaxed);
                        error!(
                            "Chunk transcription failed: {}. The chunk will be skipped.",
                            err
                        );
                    }
                }
            }
        });

        Self {
            chunk_sender: Some(tx),
            worker_handle: Some(handle),
            transcripts,
            had_errors,
            abort_requested,
        }
    }

    fn chunk_sender(&self) -> mpsc::SyncSender<Vec<f32>> {
        self.chunk_sender
            .as_ref()
            .expect("chunk sender should exist")
            .clone()
    }

    fn finalize(mut self) -> ChunkSessionResult {
        self.chunk_sender.take();
        let mut complete = true;
        if let Some(handle) = self.worker_handle.take() {
            if handle.join().is_err() {
                complete = false;
            }
        }
        let guard = self.transcripts.lock().unwrap();
        ChunkSessionResult {
            complete,
            had_errors: !complete || self.had_errors.load(Ordering::Relaxed),
            transcripts: guard.clone(),
        }
    }

    fn abort(mut self) {
        self.abort_requested.store(true, Ordering::Relaxed);
        self.chunk_sender.take();
        if let Some(handle) = self.worker_handle.take() {
            thread::spawn(move || {
                let _ = handle.join();
            });
        }
    }
}
