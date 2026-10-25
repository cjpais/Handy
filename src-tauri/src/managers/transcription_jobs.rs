//! Background transcriptions (a file brought into History, or an entry
//! transcribed again), run one at a time in chunks that yield to dictation.
//! Progress lives here in memory; the entry gets its text only when done.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::actions::process_transcription_output;
use crate::audio_toolkit::{decode_audio_file, probe_audio_file, read_wav_samples, save_wav_file};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::{panic_payload_message, ChunkControl, TranscriptionManager};

/// A recording longer than this (one chunk) is transcribed again in the
/// background; a shorter one directly.
pub const BACKGROUND_RETRY_MIN_SAMPLES: usize = 40 * 16_000;

/// How often a waiting chunk checks whether dictation has finished.
const DICTATION_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TranscriptionJobStatus {
    /// Waiting behind another job.
    Queued,
    /// `percent` of the audio is transcribed.
    Running {
        percent: u8,
    },
    /// The entry has its new text, or the job was cancelled.
    Finished,
    Failed {
        error: String,
    },
}

/// A background transcription of one History entry.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct TranscriptionJob {
    pub entry_id: i64,
    pub status: TranscriptionJobStatus,
}

enum JobKind {
    /// Decode this file into the entry's recording first.
    File(PathBuf),
    /// The entry's recording is already there.
    Retry,
}

struct Job {
    entry_id: i64,
    kind: JobKind,
}

struct Running {
    entry_id: i64,
    percent: u8,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct Jobs {
    queue: VecDeque<Job>,
    running: Option<Running>,
}

impl Jobs {
    fn has(&self, entry_id: i64) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.entry_id == entry_id)
            || self.queue.iter().any(|j| j.entry_id == entry_id)
    }

    fn is_idle(&self) -> bool {
        self.running.is_none() && self.queue.is_empty()
    }
}

pub struct TranscriptionJobManager {
    app: AppHandle,
    jobs: Mutex<Jobs>,
    wake: Condvar,
}

impl TranscriptionJobManager {
    /// Starts the thread that runs the jobs.
    pub fn new(app: &AppHandle) -> Arc<Self> {
        let manager = Arc::new(Self {
            app: app.clone(),
            jobs: Mutex::new(Jobs::default()),
            wake: Condvar::new(),
        });
        let runner = Arc::clone(&manager);
        thread::Builder::new()
            .name("transcription-jobs".into())
            .spawn(move || runner.run_jobs())
            .expect("Failed to start the transcription job thread");
        manager
    }

    fn lock(&self) -> MutexGuard<'_, Jobs> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Adds `path` to History as a starred entry and queues its
    /// transcription. Fails at once, without an entry, if the file has no
    /// audio Handy can read.
    pub fn transcribe_file(&self, path: PathBuf) -> Result<HistoryEntry> {
        probe_audio_file(&path)?;
        let source_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let file_name = format!("handy-file-{}.wav", chrono::Utc::now().timestamp_millis());
        let entry = self
            .app
            .state::<Arc<HistoryManager>>()
            .save_file_entry(file_name, source_name)?;
        info!("Queued file transcription for history entry {}", entry.id);
        self.enqueue(Job {
            entry_id: entry.id,
            kind: JobKind::File(path),
        });
        Ok(entry)
    }

    /// Whether the entry is queued or being transcribed.
    pub fn has(&self, entry_id: i64) -> bool {
        self.lock().has(entry_id)
    }

    /// Queues the entry to be transcribed again from its recording.
    pub fn retry(&self, entry_id: i64) -> Result<()> {
        if self.has(entry_id) {
            return Ok(());
        }
        self.enqueue(Job {
            entry_id,
            kind: JobKind::Retry,
        });
        Ok(())
    }

    /// Stops the entry's job: a queued one is dropped, a running one stops
    /// before its next chunk.
    pub fn cancel(&self, entry_id: i64) {
        let mut jobs = self.lock();
        jobs.queue.retain(|j| j.entry_id != entry_id);
        if let Some(running) = jobs.running.as_ref().filter(|r| r.entry_id == entry_id) {
            running.cancelled.store(true, Ordering::Release);
        }
        self.sync_job_active(&jobs);
    }

    /// Every job not yet finished, running first.
    pub fn list(&self) -> Vec<TranscriptionJob> {
        let jobs = self.lock();
        let running = jobs.running.iter().map(|r| TranscriptionJob {
            entry_id: r.entry_id,
            status: TranscriptionJobStatus::Running { percent: r.percent },
        });
        let queued = jobs.queue.iter().map(|j| TranscriptionJob {
            entry_id: j.entry_id,
            status: TranscriptionJobStatus::Queued,
        });
        running.chain(queued).collect()
    }

    fn enqueue(&self, job: Job) {
        let entry_id = job.entry_id;
        {
            let mut jobs = self.lock();
            jobs.queue.push_back(job);
            self.sync_job_active(&jobs);
        }
        self.emit(entry_id, TranscriptionJobStatus::Queued);
        crate::tray::update_tray_menu(&self.app);
        self.wake.notify_one();
    }

    /// Keeps the model held for as long as any job is queued or running.
    fn sync_job_active(&self, jobs: &Jobs) {
        self.app
            .state::<Arc<TranscriptionManager>>()
            .set_job_active(!jobs.is_idle());
    }

    fn emit(&self, entry_id: i64, status: TranscriptionJobStatus) {
        if let Err(e) = (TranscriptionJob { entry_id, status }).emit(&self.app) {
            error!("Failed to emit transcription job event: {}", e);
        }
    }

    fn run_jobs(&self) {
        loop {
            let (job, cancelled) = {
                let mut jobs = self.lock();
                let job = loop {
                    if let Some(job) = jobs.queue.pop_front() {
                        break job;
                    }
                    jobs = self
                        .wake
                        .wait(jobs)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                };
                let cancelled = Arc::new(AtomicBool::new(false));
                jobs.running = Some(Running {
                    entry_id: job.entry_id,
                    percent: 0,
                    cancelled: Arc::clone(&cancelled),
                });
                (job, cancelled)
            };
            let entry_id = job.entry_id;
            self.emit(entry_id, TranscriptionJobStatus::Running { percent: 0 });

            // A panic fails only this job; the rest of the loop still has to
            // run, or the model would stay held and the queue stuck.
            let result = catch_unwind(AssertUnwindSafe(|| self.run_job(job, &cancelled)))
                .unwrap_or_else(|payload| Err(crashed(payload.as_ref())));

            let idle = {
                let mut jobs = self.lock();
                jobs.running = None;
                self.sync_job_active(&jobs);
                jobs.is_idle()
            };
            let tm = self.app.state::<Arc<TranscriptionManager>>();
            if idle {
                // A dictation under way unloads the model itself when it ends.
                if !crate::tray::dictation_in_progress(&self.app) {
                    tm.maybe_unload_immediately("background transcription");
                }
                crate::tray::update_tray_menu(&self.app);
            }
            let status = match result {
                Err(e) if !cancelled.load(Ordering::Acquire) => {
                    warn!("Transcription of history entry {} failed: {}", entry_id, e);
                    TranscriptionJobStatus::Failed {
                        error: e.to_string(),
                    }
                }
                _ => TranscriptionJobStatus::Finished,
            };
            self.emit(entry_id, status);
        }
    }

    fn run_job(&self, job: Job, cancelled: &Arc<AtomicBool>) -> Result<()> {
        let hm = Arc::clone(&self.app.state::<Arc<HistoryManager>>());
        let tm = Arc::clone(&self.app.state::<Arc<TranscriptionManager>>());
        let entry = tauri::async_runtime::block_on(hm.get_entry_by_id(job.entry_id))?
            .ok_or_else(|| anyhow!("History entry {} not found", job.entry_id))?;
        let recording = hm.get_audio_file_path(&entry.file_name);

        let samples = match job.kind {
            JobKind::File(source) => match self.copy_file(&source, &recording, cancelled) {
                Ok(Some(samples)) => samples,
                Ok(None) => return Ok(()),
                Err(e) => {
                    // An entry with no recording could never be transcribed;
                    // the file is still where it was, to bring in again.
                    if let Err(delete_err) =
                        tauri::async_runtime::block_on(hm.delete_entry(entry.id))
                    {
                        error!("Failed to remove entry {}: {}", entry.id, delete_err);
                    }
                    return Err(e);
                }
            },
            JobKind::Retry => {
                read_wav_samples(&recording).map_err(|e| anyhow!("Failed to load audio: {}", e))?
            }
        };
        if samples.is_empty() {
            return Err(anyhow!("Recording has no audio samples"));
        }

        let control = Control {
            manager: self,
            entry_id: entry.id,
            cancelled,
        };
        let Some(text) = tm.transcribe_in_chunks(samples, &control)? else {
            return Ok(());
        };
        if text.is_empty() {
            return Err(anyhow!("Recording contains no speech"));
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }

        let processed = tauri::async_runtime::block_on(process_transcription_output(
            &self.app,
            &text,
            entry.post_process_requested,
        ));
        hm.update_transcription(
            entry.id,
            text,
            processed.post_processed_text,
            processed.post_process_prompt,
        )?;
        Ok(())
    }

    /// Decodes `source` once and keeps it as the entry's recording. `None` if
    /// the job was cancelled meanwhile.
    fn copy_file(
        &self,
        source: &Path,
        recording: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Option<Vec<f32>>> {
        // A malformed file may panic the decoder; that fails the copy like
        // any unreadable file, which removes the entry.
        let samples = catch_unwind(AssertUnwindSafe(|| decode_audio_file(source)))
            .unwrap_or_else(|payload| Err(crashed(payload.as_ref())))?;
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        save_wav_file(recording, &samples)?;
        // The entry may have been deleted while the copy was written.
        if cancelled.load(Ordering::Acquire) {
            let _ = std::fs::remove_file(recording);
            return Ok(None);
        }
        Ok(Some(samples))
    }
}

fn crashed(payload: &(dyn std::any::Any + Send)) -> anyhow::Error {
    anyhow!("Transcription crashed: {}", panic_payload_message(payload))
}

struct Control<'a> {
    manager: &'a TranscriptionJobManager,
    entry_id: i64,
    cancelled: &'a AtomicBool,
}

impl ChunkControl for Control<'_> {
    fn proceed(&self) -> bool {
        // Dictation goes first: wait out a recording and its transcription.
        // A chunk started while a stream is open would end that stream.
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return false;
            }
            if !crate::tray::dictation_in_progress(&self.manager.app) {
                return true;
            }
            thread::sleep(DICTATION_POLL);
        }
    }

    fn progress(&self, done: usize, total: usize) {
        let percent = (done * 100 / total.max(1)).min(100) as u8;
        {
            let mut jobs = self.manager.lock();
            match jobs.running.as_mut() {
                Some(running) if running.entry_id == self.entry_id => {
                    if running.percent == percent && done != 0 {
                        return;
                    }
                    running.percent = percent;
                }
                _ => return,
            }
        }
        self.manager
            .emit(self.entry_id, TranscriptionJobStatus::Running { percent });
    }
}
