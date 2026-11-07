//! Bounded progress and cancellation for hf-hub's chunked downloads.

use super::download::{RateMeter, PROGRESS_THROTTLE, PROGRESS_TICK};
use hf_hub::api::tokio::{CancellationToken, Progress};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// hf-hub clones the reporter for each concurrent chunk. Keep the byte budget
/// shared, and cancel only this attempt if a server sends more than the file.
#[derive(Clone)]
pub(super) struct HfDownloadProgress {
    state: Arc<Mutex<HfProgressState>>,
    cancel: CancellationToken,
    emit: Arc<dyn Fn(u64, u64, Option<f64>) + Send + Sync>,
}

/// `(downloaded, total, bytes_per_sec)` to emit.
type HfProgressSnapshot = (u64, u64, Option<f64>);

struct HfProgressState {
    total: u64,
    downloaded: u64,
    exceeded_total: bool,
    /// `None` until hf-hub reports the resume offset; see [`Self::on_update`].
    rate: Option<RateMeter>,
    last_emit: Instant,
    /// Every callback (even throttled-out ones) bumps this; the stall watchdog
    /// reads it. Construction also covers a stalled metadata probe.
    last_activity: Instant,
}

impl HfProgressState {
    fn new(now: Instant) -> Self {
        Self {
            total: 0,
            downloaded: 0,
            exceeded_total: false,
            rate: None,
            last_emit: now,
            last_activity: now,
        }
    }

    fn on_init(&mut self, size: u64, now: Instant) {
        self.total = size;
        self.downloaded = 0;
        self.exceeded_total = false;
        self.rate = None;
        self.last_activity = now;
    }

    fn on_update(&mut self, size: u64, now: Instant) -> Option<HfProgressSnapshot> {
        if self.exceeded_total {
            return None;
        }
        self.last_activity = now;
        let downloaded = self.downloaded.checked_add(size);
        self.downloaded = downloaded.unwrap_or(u64::MAX);
        // A full-file 200 response to Range otherwise keeps the stall watchdog
        // alive while hf-hub buffers more than the model's advertised size.
        if downloaded.is_none() || self.downloaded > self.total {
            self.exceeded_total = true;
            return None;
        }
        let Some(rate) = self.rate.as_mut() else {
            // The first update is the already-committed resume offset, not
            // transfer. Preserve upstream's rate baseline and immediate emit.
            self.rate = Some(RateMeter::new(size));
            self.last_emit = now;
            return Some((self.downloaded, self.total, None));
        };
        let done = self.total > 0 && self.downloaded >= self.total;
        if !done && now.duration_since(self.last_emit) < PROGRESS_THROTTLE {
            return None;
        }
        self.last_emit = now;
        Some((
            self.downloaded,
            self.total,
            rate.sample(self.downloaded, now),
        ))
    }

    /// Re-emit during a lull so the rate decays instead of freezing.
    fn on_tick(&mut self, now: Instant) -> Option<HfProgressSnapshot> {
        if self.exceeded_total {
            return None;
        }
        let rate = self.rate.as_mut()?;
        if now.duration_since(self.last_emit) < PROGRESS_TICK {
            return None;
        }
        self.last_emit = now;
        Some((
            self.downloaded,
            self.total,
            rate.sample(self.downloaded, now),
        ))
    }
}

impl HfDownloadProgress {
    pub(super) fn new(
        cancel: CancellationToken,
        emit: impl Fn(u64, u64, Option<f64>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(HfProgressState::new(Instant::now()))),
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

    pub(super) fn tick(&self) {
        let snapshot = self.state.lock().unwrap().on_tick(Instant::now());
        if let Some((downloaded, total, bytes_per_sec)) = snapshot {
            (self.emit)(downloaded, total, bytes_per_sec);
        }
    }
}

impl Progress for HfDownloadProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        // No emit: the resume offset follows immediately.
        self.state
            .lock()
            .unwrap()
            .on_init(size as u64, Instant::now());
    }

    async fn update(&mut self, size: usize) {
        let (snapshot, exceeded_total) = {
            let mut state = self.state.lock().unwrap();
            let snapshot = state.on_update(size as u64, Instant::now());
            (snapshot, state.exceeded_total)
        };
        if exceeded_total {
            // Leave the user's parent token usable for verified mirror fallback.
            self.cancel.cancel();
        } else if let Some((downloaded, total, bytes_per_sec)) = snapshot {
            (self.emit)(downloaded, total, bytes_per_sec);
        }
    }

    async fn finish(&mut self) {
        let total = {
            let state = self.state.lock().unwrap();
            (!state.exceeded_total).then_some(state.total.max(state.downloaded))
        };
        if let Some(total) = total {
            (self.emit)(total, total, None);
        }
    }
}

#[cfg(test)]
mod tests;
