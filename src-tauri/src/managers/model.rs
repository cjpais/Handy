//! The one transcription model this build uses, and how it gets onto disk.
//!
//! The model is pinned to a Hugging Face file. It is looked up in the shared
//! Hugging Face cache (where other tools may already have put it) and in
//! Handy's own models directory, and downloaded into the cache if missing.

use anyhow::Result;
use hf_hub::api::tokio::{ApiBuilder, CancellationToken, Progress};
use hf_hub::{Cache, Repo, RepoType};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Hugging Face repository the model file lives in.
pub const MODEL_REPO: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";
/// Commit the file is pinned to, so the downloaded bytes are the tested ones.
pub const MODEL_REVISION: &str = "7e948f21b7bdbac698d3318db9d350f1096f3b6c";
/// The file within the repository.
pub const MODEL_FILENAME: &str = "parakeet-unified-en-0.6b-Q8_0.gguf";
/// The file's size, for the progress bar before the server reports one.
pub const MODEL_SIZE_BYTES: u64 = 731_357_568;
/// Human-readable name shown in the settings window.
pub const MODEL_NAME: &str = "Parakeet Unified English 0.6B";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
    /// Measured over a trailing window of this transfer only (bytes resumed
    /// from disk excluded); `None` until there is enough to measure.
    pub bytes_per_sec: Option<f64>,
}

/// No hf-hub progress for this long means the transfer is wedged, not slow:
/// the attempt is cancelled and retried.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Throttle for progress events while bytes are flowing.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

/// Re-emit interval with no bytes arriving, so the rate decays instead of freezing.
const PROGRESS_TICK: Duration = Duration::from_secs(1);

/// Rate window: smooths hf-hub's bursty parallel chunks, still shows a slowdown.
const RATE_WINDOW: Duration = Duration::from_secs(5);

/// No rate until bytes have flowed this long; shorter samples are burst noise.
const RATE_MIN_SPAN: Duration = Duration::from_secs(1);

/// Transfer rate over a trailing window, excluding resumed bytes and timed from
/// the first byte received (so connecting doesn't drag it down).
struct RateMeter {
    start_bytes: u64,
    /// `(when, cumulative bytes)`; the front is the window's anchor. Empty
    /// until the first sample past `start_bytes`.
    samples: VecDeque<(Instant, u64)>,
}

impl RateMeter {
    fn new(start_bytes: u64) -> Self {
        Self {
            start_bytes,
            samples: VecDeque::new(),
        }
    }

    /// Records the cumulative byte count at `now` and returns the rate in
    /// bytes/sec, or `None` until bytes have flowed for [`RATE_MIN_SPAN`].
    fn sample(&mut self, bytes: u64, now: Instant) -> Option<f64> {
        if self.samples.is_empty() && bytes <= self.start_bytes {
            return None;
        }
        self.samples.push_back((now, bytes));
        // Keep one sample at or before the window start as the anchor.
        while self.samples.len() > 2
            && now.saturating_duration_since(self.samples[1].0) >= RATE_WINDOW
        {
            self.samples.pop_front();
        }
        let (anchor_at, anchor_bytes) = self.samples[0];
        let span = now.saturating_duration_since(anchor_at);
        if span < RATE_MIN_SPAN {
            return None;
        }
        Some(bytes.saturating_sub(anchor_bytes) as f64 / span.as_secs_f64())
    }
}

/// Return the active HF cache followed by the pre-v0.9.6 cache for portable
/// upgrades. Downloads still use `Cache::from_env()` and therefore only write
/// to the active portable cache.
fn hf_caches() -> Vec<Cache> {
    let current = Cache::from_env();
    let mut caches = vec![current.clone()];

    if crate::portable::is_portable() {
        let legacy = crate::portable::previous_hf_home()
            .map(|home| Cache::new(home.join("hub")))
            .unwrap_or_default();
        if legacy.path() != current.path() {
            caches.push(legacy);
        }
    }

    caches
}

/// Reuse an existing file regardless of which revision originally downloaded it.
/// Prefer the requested pin, then main, then other snapshots.
fn hf_cached_path_in(
    caches: &[Cache],
    repo_id: &str,
    revision: &str,
    filename: &str,
) -> Option<PathBuf> {
    hf_snapshot_paths(caches, repo_id, revision)
        .into_iter()
        .map(|snapshot| snapshot.join(filename))
        .find(|path| path.is_file())
}

/// Snapshot directories in lookup order, with the active cache winning ties.
fn hf_snapshot_paths(caches: &[Cache], repo_id: &str, revision: &str) -> Vec<PathBuf> {
    let mut snapshots = Vec::new();
    for (cache_index, cache) in caches.iter().enumerate() {
        let repo_dir = cache
            .path()
            .join(Repo::new(repo_id.to_string(), RepoType::Model).folder_name());
        let root = repo_dir.join("snapshots");
        let refs = repo_dir.join("refs");
        let read_ref = |name: &str| {
            fs::read_to_string(refs.join(name))
                .ok()
                .map(|s| s.trim().to_string())
        };
        let pin = read_ref(revision).unwrap_or_else(|| revision.to_string());
        let main = read_ref("main");
        // Known refs still work when directory listing is unavailable.
        let mut paths = vec![root.join(&pin), root.join(revision)];
        if let Some(main) = &main {
            paths.push(root.join(main));
        }
        match fs::read_dir(&root) {
            Ok(entries) => paths.extend(entries.flatten().map(|e| e.path())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!("Couldn't list HF snapshots at {:?}: {}", root, e),
        }
        for path in paths.into_iter().filter(|path| path.is_dir()) {
            let Some(commit) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let rank = if commit == revision || commit == pin {
                0
            } else if main.as_deref() == Some(commit) {
                1
            } else {
                2
            };
            snapshots.push((rank, cache_index, path));
        }
    }
    snapshots.sort();
    snapshots.dedup();
    snapshots.into_iter().map(|(_, _, path)| path).collect()
}

/// Bridges hf-hub's async download progress to Handy's `model-download-progress`
/// event. hf-hub clones the reporter, so shared state lives behind an `Arc`.
#[derive(Clone)]
struct HfDownloadProgress {
    app_handle: AppHandle,
    state: Arc<Mutex<HfProgressState>>,
}

/// `(downloaded, total, bytes_per_sec)` to emit.
type HfProgressSnapshot = (u64, u64, Option<f64>);

struct HfProgressState {
    total: u64,
    downloaded: u64,
    /// `None` until hf-hub reports the resume offset; see [`Self::on_update`].
    rate: Option<RateMeter>,
    last_emit: Instant,
    /// Every callback (even throttled-out ones) bumps this; the stall watchdog
    /// reads it. Starts at construction so a hang before the first byte —
    /// e.g. a wedged metadata/resolve request — also counts as a stall.
    last_activity: Instant,
}

impl HfProgressState {
    fn new(now: Instant) -> Self {
        Self {
            total: MODEL_SIZE_BYTES,
            downloaded: 0,
            rate: None,
            last_emit: now,
            last_activity: now,
        }
    }

    fn on_init(&mut self, size: u64, now: Instant) {
        self.total = size;
        self.downloaded = 0;
        self.rate = None;
        self.last_activity = now;
    }

    fn on_update(&mut self, size: u64, now: Instant) -> Option<HfProgressSnapshot> {
        self.last_activity = now;
        let Some(rate) = self.rate.as_mut() else {
            // hf-hub's first `update(start)` after `init` is the resume offset already
            // on disk, not transfer: use it as the rate baseline and emit it at once
            // so the bar doesn't flash 0%.
            self.downloaded = size;
            self.rate = Some(RateMeter::new(size));
            self.last_emit = now;
            return Some((self.downloaded, self.total, None));
        };
        self.downloaded = self.downloaded.saturating_add(size);
        // Throttle, but always emit the final byte.
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
    fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            state: Arc::new(Mutex::new(HfProgressState::new(Instant::now()))),
        }
    }

    /// Instant of the most recent sign of life from the transfer.
    fn last_activity(&self) -> Instant {
        self.state.lock().unwrap().last_activity
    }

    /// Called periodically by the stall watchdog.
    fn tick(&self) {
        let snapshot = self.state.lock().unwrap().on_tick(Instant::now());
        if let Some(snapshot) = snapshot {
            self.emit(snapshot);
        }
    }

    fn emit(&self, (downloaded, total, bytes_per_sec): HfProgressSnapshot) {
        let total = total.max(downloaded);
        let _ = self.app_handle.emit(
            "model-download-progress",
            DownloadProgress {
                downloaded,
                total,
                percentage: if total > 0 {
                    downloaded as f64 / total as f64 * 100.0
                } else {
                    0.0
                },
                bytes_per_sec,
            },
        );
    }
}

impl Progress for HfDownloadProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        // No emit: the resume offset follows immediately (see `on_update`).
        self.state
            .lock()
            .unwrap()
            .on_init(size as u64, Instant::now());
    }

    async fn update(&mut self, size: usize) {
        let snapshot = self
            .state
            .lock()
            .unwrap()
            .on_update(size as u64, Instant::now());
        if let Some(snapshot) = snapshot {
            self.emit(snapshot);
        }
    }

    async fn finish(&mut self) {
        let total = {
            let st = self.state.lock().unwrap();
            st.total.max(st.downloaded)
        };
        self.emit((total, total, None));
    }
}

/// Clears the downloading flag and the cancel token when a download ends on
/// any path.
struct DownloadGuard<'a> {
    manager: &'a ModelManager,
}

impl Drop for DownloadGuard<'_> {
    fn drop(&mut self) {
        self.manager.is_downloading.store(false, Ordering::SeqCst);
        *self.manager.cancel_token.lock().unwrap() = None;
    }
}

pub struct ModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    is_downloading: Arc<AtomicBool>,
    cancel_token: Mutex<Option<CancellationToken>>,
}

impl ModelManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Handy's own models directory: a manual drop-in of the file counts.
        let models_dir = crate::portable::app_data_dir(app_handle)
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
            .join("models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            app_handle: app_handle.clone(),
            models_dir,
            is_downloading: Arc::new(AtomicBool::new(false)),
            cancel_token: Mutex::new(None),
        })
    }

    /// The model file, if it is fully on disk: in the shared HF cache, or in
    /// the models directory.
    pub fn model_path(&self) -> Option<PathBuf> {
        hf_cached_path_in(&hf_caches(), MODEL_REPO, MODEL_REVISION, MODEL_FILENAME).or_else(|| {
            let local = self.models_dir.join(MODEL_FILENAME);
            local.is_file().then_some(local)
        })
    }

    pub fn is_downloaded(&self) -> bool {
        self.model_path().is_some()
    }

    pub fn is_downloading(&self) -> bool {
        self.is_downloading.load(Ordering::SeqCst)
    }

    /// Download the model into the shared HF cache unless it is already on
    /// disk. Emits `model-download-progress` while transferring and
    /// `model-download-complete` when the file is there. Returns `Ok(false)`
    /// if the download was cancelled (its partial file is kept for resume).
    pub async fn download(&self) -> Result<bool> {
        if self.is_downloaded() {
            let _ = self.app_handle.emit("model-download-complete", ());
            return Ok(true);
        }
        if self.is_downloading.swap(true, Ordering::SeqCst) {
            info!("Model download already in progress");
            return Ok(true);
        }
        let cancel_token = CancellationToken::new();
        *self.cancel_token.lock().unwrap() = Some(cancel_token.clone());
        let _guard = DownloadGuard { manager: self };

        if !self.fetch(&cancel_token).await? {
            return Ok(false);
        }
        let _ = self.app_handle.emit("model-download-complete", ());
        info!("Model {} downloaded", MODEL_FILENAME);
        Ok(true)
    }

    /// Stop a download in progress. The partial file stays for resume.
    pub fn cancel_download(&self) {
        if let Some(token) = self.cancel_token.lock().unwrap().as_ref() {
            token.cancel();
            info!("Model download cancelled");
        }
        let _ = self.app_handle.emit("model-download-cancelled", ());
    }

    /// Fetch the file from Hugging Face into the shared cache, retrying
    /// transient failures. `Ok(false)` if cancelled.
    async fn fetch(&self, cancel_token: &CancellationToken) -> Result<bool> {
        info!(
            "Downloading {} from {}@{}",
            MODEL_FILENAME, MODEL_REPO, MODEL_REVISION
        );

        // hf-hub has no working internal retry, so a single transient fault
        // would otherwise fail the whole download. Each attempt resumes from
        // the committed offset, so a retry only re-fetches what the failed
        // attempt hadn't finished. Start moderately parallel, then stay
        // sequential after the first failure (#1579).
        const ATTEMPT_STREAMS: [usize; 4] = [4, 1, 1, 1];
        let mut attempt: usize = 1;
        loop {
            let stream_count = ATTEMPT_STREAMS[attempt - 1];
            info!(
                "HF download attempt {}/{} using {} concurrent stream(s)",
                attempt,
                ATTEMPT_STREAMS.len(),
                stream_count
            );

            // Fresh client per attempt so a wedged connection from the previous
            // try can't poison the retry.
            let api = ApiBuilder::from_env()
                // Ignore cached and environment-provided credentials. A stale token
                // can make otherwise-public downloads fail authentication.
                .with_token(None)
                .with_progress(false)
                .with_max_files(stream_count)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to init Hugging Face API: {}", e))?;
            let repo = api.repo(Repo::with_revision(
                MODEL_REPO.to_string(),
                RepoType::Model,
                MODEL_REVISION.to_string(),
            ));
            let progress = HfDownloadProgress::new(self.app_handle.clone());

            // hf-hub has no internal timeouts, so a wedged connection would
            // otherwise hang this attempt forever. The watchdog cancels a
            // per-attempt child token when progress goes stale; a user cancel
            // on the parent propagates through the same child.
            let attempt_token = cancel_token.child_token();
            let watchdog = tokio::spawn({
                let probe = progress.clone();
                let attempt_token = attempt_token.clone();
                async move {
                    loop {
                        tokio::time::sleep(PROGRESS_TICK).await;
                        if attempt_token.is_cancelled() {
                            break;
                        }
                        probe.tick();
                        if probe.last_activity().elapsed() > DOWNLOAD_STALL_TIMEOUT {
                            attempt_token.cancel();
                            break;
                        }
                    }
                }
            });
            // hf-hub only observes its token inside the chunk loop. Race the
            // whole future against the token: on cancel, grant a short grace so
            // an attempt in the chunk loop can commit its resume offset, then
            // drop the future outright.
            let mut download = std::pin::pin!(repo.download_with_progress_cancellable(
                MODEL_FILENAME,
                progress,
                attempt_token.clone()
            ));
            let result = tokio::select! {
                r = &mut download => r,
                _ = attempt_token.cancelled() => {
                    match tokio::time::timeout(Duration::from_secs(5), &mut download).await {
                        Ok(r) => r,
                        Err(_) => Err(hf_hub::api::tokio::ApiError::Cancelled),
                    }
                }
            };
            watchdog.abort();

            let err = match result {
                Ok(_) => return Ok(true),
                Err(hf_hub::api::tokio::ApiError::Cancelled) if cancel_token.is_cancelled() => {
                    info!("HF download cancelled");
                    return Ok(false);
                }
                Err(hf_hub::api::tokio::ApiError::Cancelled) => anyhow::anyhow!(
                    "transfer stalled: no progress for {}s",
                    DOWNLOAD_STALL_TIMEOUT.as_secs()
                ),
                // {:?} keeps the error source chain; Display truncates it.
                Err(e) => anyhow::anyhow!("{:?}", e),
            };
            if attempt >= ATTEMPT_STREAMS.len() {
                error!("HF download failed after {} attempt(s): {:?}", attempt, err);
                return Err(anyhow::anyhow!(
                    "Hugging Face download failed after {} attempt(s): {}",
                    attempt,
                    err
                ));
            }
            let delay = Duration::from_secs(1_u64 << attempt);
            warn!(
                "HF download attempt {}/{} failed: {}; retrying with {} stream(s) in {}s",
                attempt,
                ATTEMPT_STREAMS.len(),
                err,
                ATTEMPT_STREAMS[attempt],
                delay.as_secs()
            );
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = cancel_token.cancelled() => {
                    info!("HF download cancelled");
                    return Ok(false);
                }
            }
            attempt += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn add_cached_model(cache: &Path, repo_id: &str, revision: &str, filename: &str) -> PathBuf {
        let repo_dir = cache.join(format!("models--{}", repo_id.replace('/', "--")));
        let snapshot = repo_dir.join("snapshots").join(revision).join(filename);
        fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
        fs::create_dir_all(repo_dir.join("refs")).unwrap();
        fs::write(repo_dir.join("refs").join(revision), revision).unwrap();
        fs::write(&snapshot, b"model").unwrap();
        snapshot
    }

    /// A file in snapshot `commit` with no ref pointing at it — how
    /// huggingface_hub leaves a download made at a commit revision.
    fn add_unreferenced_snapshot_file(
        cache: &Path,
        repo_id: &str,
        commit: &str,
        filename: &str,
    ) -> PathBuf {
        let path = cache
            .join(format!("models--{}", repo_id.replace('/', "--")))
            .join("snapshots")
            .join(commit)
            .join(filename);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"model").unwrap();
        path
    }

    #[test]
    fn cached_path_falls_back_to_legacy_cache() {
        let active = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        let expected = add_cached_model(legacy.path(), MODEL_REPO, "old-revision", MODEL_FILENAME);
        let caches = vec![
            Cache::new(active.path().to_path_buf()),
            Cache::new(legacy.path().to_path_buf()),
        ];

        assert_eq!(
            hf_cached_path_in(&caches, MODEL_REPO, "old-revision", MODEL_FILENAME),
            Some(expected)
        );
    }

    #[test]
    fn cached_path_prefers_active_cache() {
        let active = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        let expected = add_cached_model(active.path(), MODEL_REPO, "revision", MODEL_FILENAME);
        add_cached_model(legacy.path(), MODEL_REPO, "revision", MODEL_FILENAME);
        let caches = vec![
            Cache::new(active.path().to_path_buf()),
            Cache::new(legacy.path().to_path_buf()),
        ];

        assert_eq!(
            hf_cached_path_in(&caches, MODEL_REPO, "revision", MODEL_FILENAME),
            Some(expected)
        );
    }

    /// A file downloaded at another commit (by another tool, or an earlier
    /// pin) is reused instead of downloaded again.
    #[test]
    fn cache_lookup_prefers_pin_then_main_then_any_snapshot() {
        let cache = TempDir::new().unwrap();
        let old =
            add_unreferenced_snapshot_file(cache.path(), MODEL_REPO, "unknown", MODEL_FILENAME);
        let main = add_cached_model(cache.path(), MODEL_REPO, "main", MODEL_FILENAME);
        let pin = add_unreferenced_snapshot_file(
            cache.path(),
            MODEL_REPO,
            MODEL_REVISION,
            MODEL_FILENAME,
        );
        let caches = vec![Cache::new(cache.path().to_path_buf())];

        for expected in [pin, main, old] {
            assert_eq!(
                hf_cached_path_in(&caches, MODEL_REPO, MODEL_REVISION, MODEL_FILENAME),
                Some(expected.clone())
            );
            fs::remove_file(expected).unwrap();
        }
        assert!(hf_cached_path_in(&caches, MODEL_REPO, MODEL_REVISION, MODEL_FILENAME).is_none());
    }

    #[test]
    fn rate_meter_ignores_resumed_bytes_and_short_spans() {
        let start = Instant::now();
        let mut meter = RateMeter::new(100);
        assert_eq!(meter.sample(100, start), None);
        assert_eq!(meter.sample(150, start + Duration::from_millis(500)), None);
        let rate = meter.sample(300, start + Duration::from_secs(2)).unwrap();
        assert!((rate - 100.0).abs() < 1.0, "{rate}");
    }
}
