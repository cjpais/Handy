//! Bounded progress and cancellation for hf-hub's chunked downloads.

use hf_hub::api::tokio::{CancellationToken, Progress};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// hf-hub clones the reporter for each concurrent chunk. Keep the byte budget
/// shared, and cancel only this attempt if a server sends more than the file.
#[derive(Clone)]
pub(super) struct HfDownloadProgress {
    state: Arc<Mutex<HfProgressState>>,
    cancel: CancellationToken,
    emit: Arc<dyn Fn(u64, u64) + Send + Sync>,
}

struct HfProgressState {
    total: u64,
    downloaded: u64,
    exceeded_total: bool,
    last_emit: Instant,
    // Every callback, including throttled-out ones, counts as activity for
    // the stall watchdog. Construction also covers a stalled metadata probe.
    last_activity: Instant,
}

impl HfDownloadProgress {
    pub(super) fn new(
        cancel: CancellationToken,
        emit: impl Fn(u64, u64) + Send + Sync + 'static,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(HfProgressState {
                total: 0,
                downloaded: 0,
                exceeded_total: false,
                last_emit: Instant::now(),
                last_activity: Instant::now(),
            })),
            cancel,
            emit: Arc::new(emit),
        }
    }

    pub(super) fn last_activity(&self) -> Instant {
        self.state.lock().unwrap().last_activity
    }

    pub(super) fn exceeded_total(&self) -> Option<(u64, u64)> {
        let state = self.state.lock().unwrap();
        state
            .exceeded_total
            .then_some((state.total, state.downloaded))
    }
}

impl Progress for HfDownloadProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        {
            let mut state = self.state.lock().unwrap();
            state.total = size as u64;
            state.downloaded = 0;
            state.exceeded_total = false;
            state.last_emit = Instant::now();
            state.last_activity = Instant::now();
        }
        (self.emit)(0, size as u64);
    }

    async fn update(&mut self, size: usize) {
        let (downloaded, total, emit) = {
            let mut state = self.state.lock().unwrap();
            if state.exceeded_total {
                return;
            }
            let downloaded = state.downloaded.checked_add(size as u64);
            state.downloaded = downloaded.unwrap_or(u64::MAX);
            let now = Instant::now();
            state.last_activity = now;
            // A 200 response to Range can otherwise be buffered once per chunk,
            // with progress past 100% keeping the stall watchdog alive forever.
            // Do not publish that progress or cancel the user's parent token:
            // the caller still needs to try its verified mirror fallback.
            if downloaded.is_none() || state.downloaded > state.total {
                state.exceeded_total = true;
                self.cancel.cancel();
                return;
            }
            let emit = now.duration_since(state.last_emit) >= Duration::from_millis(100)
                || state.downloaded == state.total;
            if emit {
                state.last_emit = now;
            }
            (state.downloaded, state.total, emit)
        };
        if emit {
            (self.emit)(downloaded, total);
        }
    }

    async fn finish(&mut self) {
        let total = {
            let state = self.state.lock().unwrap();
            (!state.exceeded_total).then_some(state.total)
        };
        if let Some(total) = total {
            (self.emit)(total, total);
        }
    }
}

#[cfg(test)]
mod tests;
