//! Background transcriptions (a file, or an entry transcribed again), one at
//! a time in chunks that yield to dictation. Progress is kept in memory.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::cell::Cell;
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
use crate::audio_toolkit::{decode_audio_file, read_wav_samples, save_wav_file};
use crate::managers::history::{HistoryEntry, HistoryManager};
use crate::managers::transcription::{
    panic_payload_message, ChunkControl, Transcription, TranscriptionManager,
};

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

struct Running {
    entry_id: i64,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct Jobs {
    /// Entry ids waiting to be transcribed again. They go before files:
    /// someone is waiting on them.
    retries: VecDeque<i64>,
    /// Entry ids of files waiting to be transcribed.
    files: VecDeque<i64>,
    running: Option<Running>,
}

impl Jobs {
    fn has(&self, entry_id: i64) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.entry_id == entry_id)
            || self.retries.contains(&entry_id)
            || self.files.contains(&entry_id)
    }

    fn is_idle(&self) -> bool {
        self.running.is_none() && self.retries.is_empty() && self.files.is_empty()
    }

    /// Drops the entry from the queues; whether it was in one.
    fn unqueue(&mut self, entry_id: i64) -> bool {
        let before = self.retries.len() + self.files.len();
        self.retries.retain(|&id| id != entry_id);
        self.files.retain(|&id| id != entry_id);
        self.retries.len() + self.files.len() != before
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

    /// Decodes `path` into a recording, then adds it to History and queues
    /// it. The recording exists first, so no entry is ever without audio.
    pub fn transcribe_file(&self, path: PathBuf) -> Result<HistoryEntry> {
        // A malformed file may panic the decoder.
        let samples = catch_unwind(AssertUnwindSafe(|| decode_audio_file(&path))).unwrap_or_else(
            |payload| {
                Err(anyhow!(
                    "Failed to read the file: {}",
                    panic_payload_message(payload.as_ref())
                ))
            },
        )?;
        if samples.is_empty() {
            return Err(anyhow!("The file has no audio"));
        }

        let hm = self.app.state::<Arc<HistoryManager>>();
        let file_name = format!("handy-file-{}.wav", chrono::Utc::now().timestamp_millis());
        let recording = hm.get_audio_file_path(&file_name);
        // Written aside and renamed, so a crash never leaves a partial one.
        let partial = recording.with_extension("wav.partial");
        if let Err(e) = save_wav_file(&partial, &samples)
            .and_then(|()| std::fs::rename(&partial, &recording).map_err(Into::into))
        {
            remove_recording(&partial);
            return Err(e);
        }
        drop(samples);

        let source_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let entry = match hm.save_file_entry(file_name, source_name) {
            Ok(entry) => entry,
            Err(e) => {
                remove_recording(&recording);
                return Err(e);
            }
        };
        info!("Queued file transcription for history entry {}", entry.id);
        self.queue(entry.id, false);
        Ok(entry)
    }

    /// Queues the entry to be transcribed again from its recording, ahead
    /// of any file.
    pub fn retry(&self, entry_id: i64) {
        self.queue(entry_id, true);
    }

    /// Queues the entry to be transcribed from its recording, unless it
    /// already is.
    fn queue(&self, entry_id: i64, retry: bool) {
        {
            let mut jobs = self.lock();
            if jobs.has(entry_id) {
                return;
            }
            if retry {
                jobs.retries.push_back(entry_id);
            } else {
                jobs.files.push_back(entry_id);
            }
            self.sync_job_active(&jobs);
        }
        self.emit(entry_id, TranscriptionJobStatus::Queued);
        crate::tray::update_tray_menu(&self.app);
        self.wake.notify_one();
    }

    /// Stops the entry's job: a queued one is dropped, a running one stops
    /// before its next chunk.
    pub fn cancel(&self, entry_id: i64) {
        let (was_queued, idle) = {
            let mut jobs = self.lock();
            let was_queued = jobs.unqueue(entry_id);
            if let Some(running) = jobs.running.as_ref().filter(|r| r.entry_id == entry_id) {
                running.cancelled.store(true, Ordering::Release);
            }
            self.sync_job_active(&jobs);
            (was_queued, jobs.is_idle())
        };
        // A running job reports its own end from the job loop; a queued one
        // never reaches it, so it is reported here.
        if was_queued {
            if idle && !crate::tray::dictation_in_progress(&self.app) {
                self.app
                    .state::<Arc<TranscriptionManager>>()
                    .maybe_unload_immediately("background transcription");
            }
            crate::tray::update_tray_menu(&self.app);
            self.emit(entry_id, TranscriptionJobStatus::Finished);
        }
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
            let (entry_id, cancelled) = {
                let mut jobs = self.lock();
                let entry_id = loop {
                    if let Some(entry_id) =
                        jobs.retries.pop_front().or_else(|| jobs.files.pop_front())
                    {
                        break entry_id;
                    }
                    jobs = self
                        .wake
                        .wait(jobs)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                };
                let cancelled = Arc::new(AtomicBool::new(false));
                jobs.running = Some(Running {
                    entry_id,
                    cancelled: Arc::clone(&cancelled),
                });
                (entry_id, cancelled)
            };
            self.emit(entry_id, TranscriptionJobStatus::Running { percent: 0 });

            // A panic fails only this job; the rest of the loop still has to
            // run, or the model would stay held and the queue stuck.
            let result = catch_unwind(AssertUnwindSafe(|| self.run_job(entry_id, &cancelled)))
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

    fn run_job(&self, entry_id: i64, cancelled: &AtomicBool) -> Result<()> {
        let hm = Arc::clone(&self.app.state::<Arc<HistoryManager>>());
        let tm = Arc::clone(&self.app.state::<Arc<TranscriptionManager>>());
        let entry = tauri::async_runtime::block_on(hm.get_entry_by_id(entry_id))?
            .ok_or_else(|| anyhow!("History entry {} not found", entry_id))?;
        let samples = read_wav_samples(hm.get_audio_file_path(&entry.file_name))
            .map_err(|e| anyhow!("Failed to load audio: {}", e))?;
        if samples.is_empty() {
            return Err(anyhow!("Recording has no audio samples"));
        }

        let control = Control {
            manager: self,
            entry_id: entry.id,
            cancelled,
            percent: Cell::new(0),
        };
        let Some(Transcription { text, language }) = tm.transcribe_in_chunks(samples, &control)?
        else {
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
        // Post-processing can take a while; a cancel during it still wins.
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        hm.update_transcription(
            entry.id,
            text,
            processed.post_processed_text,
            processed.post_process_prompt,
            language,
        )?;
        Ok(())
    }
}

/// Best-effort cleanup; the caller is already reporting the failure.
fn remove_recording(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            error!("Failed to remove {}: {}", path.display(), e);
        }
    }
}

fn crashed(payload: &(dyn std::any::Any + Send)) -> anyhow::Error {
    anyhow!("Transcription crashed: {}", panic_payload_message(payload))
}

struct Control<'a> {
    manager: &'a TranscriptionJobManager,
    entry_id: i64,
    cancelled: &'a AtomicBool,
    /// The last percent reported; the job starts at 0.
    percent: Cell<u8>,
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
        // The chunk a cancel caught still finishes; don't report it as running.
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let percent = (done * 100 / total.max(1)).min(100) as u8;
        if self.percent.replace(percent) == percent {
            return;
        }
        self.manager
            .emit(self.entry_id, TranscriptionJobStatus::Running { percent });
    }
}
