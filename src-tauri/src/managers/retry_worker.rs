//! Background Retry Worker
//!
//! Periodically processes the transcription retry queue on a background thread.
//! This ensures failed transcriptions are retried automatically without user intervention.

use crate::audio_toolkit::audio::read_wav_samples;
use crate::managers::transcription::TranscriptionManager;
use crate::managers::transcription_retry::TranscriptionRetryQueue;
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Background worker that processes the retry queue periodically.
pub struct RetryWorker {
    /// Thread handle for the worker
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Flag to signal the worker to stop
    stop_signal: Arc<AtomicBool>,
    /// Interval between queue checks
    interval_secs: u64,
}

impl RetryWorker {
    /// Create a new retry worker.
    /// Default interval is 60 seconds.
    pub fn new() -> Self {
        Self {
            handle: Arc::new(Mutex::new(None)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            interval_secs: 60,
        }
    }

    /// Set the interval between queue checks.
    pub fn with_interval(mut self, interval_secs: u64) -> Self {
        self.interval_secs = interval_secs;
        self
    }

    /// Start the background worker.
    pub fn start(&self, app_handle: AppHandle) {
        if self.handle.lock().is_some() {
            warn!("Retry worker already running");
            return;
        }

        let stop_signal = self.stop_signal.clone();
        let interval = Duration::from_secs(self.interval_secs);

        info!("Starting retry worker (checking every {:?})", interval);

        let handle = thread::spawn(move || {
            debug!("Retry worker thread started");

            while !stop_signal.load(Ordering::Relaxed) {
                // Sleep first, then check
                thread::sleep(interval);

                if stop_signal.load(Ordering::Relaxed) {
                    debug!("Retry worker received stop signal");
                    break;
                }

                // Get the retry queue
                let retry_queue =
                    match app_handle.try_state::<Arc<Mutex<TranscriptionRetryQueue>>>() {
                        Some(queue) => queue,
                        None => {
                            warn!("Retry queue not available");
                            continue;
                        }
                    };

                // Get transcription manager
                let tm = match app_handle.try_state::<Arc<Mutex<TranscriptionManager>>>() {
                    Some(tm) => tm,
                    None => {
                        warn!("TranscriptionManager not available for retry worker");
                        continue;
                    }
                };

                // Check if there are pending retries
                let pending_count = retry_queue.lock().count();
                if pending_count == 0 {
                    debug!("No pending retries");
                    continue;
                }

                info!("Processing {} pending retry entries", pending_count);

                // Get all pending entries
                let entries = retry_queue.lock().get_all_pending();

                for entry in entries {
                    // Check if entry is ready for retry
                    if !entry.is_ready() {
                        debug!(
                            "Entry {} not ready for retry (retry at {:?})",
                            entry.id, entry.next_retry_at
                        );
                        continue;
                    }

                    // Check if already being processed
                    if entry.is_processing {
                        debug!("Entry {} already processing", entry.id);
                        continue;
                    }

                    info!("Retrying transcription for entry {}", entry.id);

                    // Load audio samples from WAV file
                    let audio_samples = match read_wav_samples(&entry.audio_path) {
                        Ok(samples) => samples,
                        Err(e) => {
                            error!("Failed to load audio for retry {}: {}", entry.id, e);

                            // Remove corrupted entry from queue
                            if let Err(remove_err) = retry_queue.lock().remove_entry(&entry.id) {
                                error!("Failed to remove corrupted entry: {}", remove_err);
                            }
                            continue;
                        }
                    };

                    // Try transcription
                    match tm.lock().transcribe(audio_samples) {
                        Ok(transcription) if !transcription.is_empty() => {
                            info!(
                                "Retry transcription succeeded for entry {}: '{}'",
                                entry.id,
                                if transcription.len() > 50 {
                                    format!("{}...", &transcription[..50])
                                } else {
                                    transcription.clone()
                                }
                            );

                            // Update history entry if we have one
                            if let Some(history_id) = entry.history_entry_id {
                                let hm = match app_handle
                                    .try_state::<Arc<crate::managers::history::HistoryManager>>()
                                {
                                    Some(hm) => hm,
                                    None => {
                                        warn!("HistoryManager not available");
                                        continue;
                                    }
                                };

                                if let Err(e) = hm.update_transcription(
                                    history_id,
                                    transcription.clone(),
                                    None,
                                    entry.post_process_prompt.clone(),
                                ) {
                                    error!("Failed to update history entry {}: {}", history_id, e);
                                }
                            }

                            // Remove from retry queue on success
                            if let Err(e) = retry_queue.lock().mark_retry_complete(&entry.id) {
                                error!("Failed to mark retry complete: {}", e);
                            }
                        }
                        Ok(_) => {
                            // Empty transcription - may indicate silent audio
                            warn!(
                                "Retry transcription returned empty text for entry {}",
                                entry.id
                            );

                            // Don't retry silent audio
                            if let Err(e) = retry_queue.lock().remove_entry(&entry.id) {
                                error!("Failed to remove silent audio entry: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Retry transcription failed for entry {}: {}", entry.id, e);

                            // Mark as failed - it will be scheduled for next retry
                            let failure = crate::managers::transcription_retry::TranscriptionFailure::Unknown {
                                error: e.to_string(),
                            };

                            match retry_queue.lock().mark_retry_failed(&entry.id, failure) {
                                Ok(can_retry) => {
                                    if !can_retry {
                                        info!(
                                            "Entry {} exhausted all retries, removing from queue",
                                            entry.id
                                        );
                                        if let Err(remove_err) =
                                            retry_queue.lock().remove_entry(&entry.id)
                                        {
                                            error!(
                                                "Failed to remove exhausted entry: {}",
                                                remove_err
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to mark retry failed: {}", e);
                                }
                            }
                        }
                    }
                }

                debug!("Retry worker cycle complete");
            }

            info!("Retry worker thread stopped");
        });

        *self.handle.lock() = Some(handle);
    }

    /// Stop the background worker.
    pub fn stop(&self) {
        self.stop_signal.store(true, Ordering::Relaxed);

        if let Some(handle) = self.handle.lock().take() {
            if let Err(e) = handle.join() {
                warn!("Failed to join retry worker thread: {:?}", e);
            }
        }
    }
}

impl Drop for RetryWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_lifecycle() {
        // This would require a full app setup to test properly
        // In practice, manual testing is used
    }
}
