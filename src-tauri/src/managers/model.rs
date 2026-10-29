use super::model_capabilities::{
    CapabilityProbe, CapabilityProber, Compatibility, GgufHeaderProber,
};
use crate::settings::{get_settings, write_settings};
use anyhow::Result;
use hf_hub::api::tokio::{ApiBuilder, CancellationToken, Progress};
use hf_hub::{Cache, Repo, RepoType};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

mod download;

use download::{
    HttpDownloadOutcome, RateMeter, DOWNLOAD_STALL_TIMEOUT, PROGRESS_THROTTLE, PROGRESS_TICK,
};

/// Where a model comes from and how Handy obtains it — the routing discriminant
/// for downloading and on-disk resolution.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum ModelSource {
    /// Direct HTTP download from a URL (current blob.handy.computer hosting).
    Url {
        url: String,
        /// Expected SHA-256 for integrity verification; `None` skips it.
        sha256: Option<String>,
    },
    /// A file inside a Hugging Face Hub repo, fetched via hf-hub into the shared
    /// HF cache (so other tools reuse it). The file within the repo is
    /// [`ModelInfo::filename`].
    HuggingFace { repo_id: String, revision: String },
    /// Already present on disk — a user-provided custom model, or one discovered
    /// in a shared cache. Nothing to download.
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub filename: String,
    pub source: ModelSource,
    pub size_mb: u64,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
    pub accuracy_score: f32,        // 0.0 to 1.0, higher is more accurate
    pub speed_score: f32,           // 0.0 to 1.0, higher is faster
    pub supports_translation: bool, // Whether the model supports translating to English
    pub is_recommended: bool,       // Whether this is the recommended model for new users
    pub supported_languages: Vec<String>, // Languages this model can transcribe
    pub supports_language_selection: bool, // Whether the user can explicitly pick a language
    pub is_custom: bool,            // Whether this is a user-provided custom model
    pub supports_streaming: bool, // Whether this model supports live streaming preview (transcribe-cpp)
    pub supports_language_detection: bool, // Whether the model can auto-detect language (gates the "Auto" option)
}

const CHINESE_LANGUAGE_CODE: &str = "zh";

fn recognition_language(language: &str) -> &str {
    match language {
        "zh-Hans" | "zh-Hant" => CHINESE_LANGUAGE_CODE,
        other => other,
    }
}

/// A tag's primary language subtag, with any BCP-47 region/script suffix
/// dropped (`en-US` → `en`, `zh-Hant` → `zh`).
fn base_language(language: &str) -> &str {
    language.split(&['-', '_'][..]).next().unwrap_or(language)
}

/// The package's `languageIdentity` for a code (`no`→`nb`, `tl`→`fil`, `iw`→`he`,
/// `jw`→`jv`). For matching only: pass on the model's own code.
pub(crate) fn canonical_language_code(language: &str) -> &str {
    match base_language(language) {
        "no" => "nb",
        "tl" => "fil",
        "iw" => "he",
        "jw" => "jv",
        base => base,
    }
}

fn canonicalize_supported_languages(languages: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut canonical = Vec::with_capacity(languages.len());

    for language in languages {
        let language = recognition_language(&language).to_string();
        if seen.insert(language.clone()) {
            canonical.push(language);
        }
    }

    canonical
}

/// One downloadable quantization of a catalog model. (Its sha256, the trust
/// anchor for mirror downloads, stays in the catalog: see
/// `catalog::mirror_fallbacks`.)
#[derive(Debug, Clone)]
pub struct QuantFile {
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
}

/// Pick the default quant among `files`: the one whose `quant` matches
/// `default_quant`, else the first file. The single source of the "which file do
/// we surface" rule — shared by [`ModelDescriptor::default_file`] and the
/// catalog's id construction so the two can never drift.
pub(crate) fn default_quant_file<'a>(
    files: &'a [QuantFile],
    default_quant: Option<&str>,
) -> Option<&'a QuantFile> {
    files
        .iter()
        .find(|f| Some(f.quant.as_str()) == default_quant)
        .or_else(|| files.first())
}

/// Live, on-disk status — the half of [`ModelInfo`] that isn't part of the
/// static spec. Kept separate so a descriptor stays purely descriptive and
/// status can be recomputed without rebuilding it.
#[derive(Debug, Clone, Default)]
pub struct DiskStatus {
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub partial_size: u64,
}

/// The spec of a bundled catalog model: everything in `catalog.json` normalised
/// into one shape, rendered into the frontend-facing [`ModelInfo`] via
/// [`ModelDescriptor::to_model_info`] by combining it with a [`DiskStatus`].
/// (The catalog is the only producer that routes through this; the legacy table
/// and on-disk scans build `ModelInfo` directly.)
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    pub id: String,
    pub source: ModelSource,
    pub name: String,
    pub description: String,
    pub caps: CapabilityProbe,
    pub files: Vec<QuantFile>,
    pub default_quant: Option<String>,
    pub speed_score: f32,
    pub accuracy_score: f32,
    /// Editorial sort priority across the whole catalog (lower = higher). Drives
    /// list ordering; independent of `recommended`.
    pub recommended_rank: Option<u32>,
    /// Whether this is part of the small curated set shown to new users in
    /// onboarding (and badged "Recommended"). A model can be ranked for ordering
    /// without being in this set.
    pub recommended: bool,
}

impl ModelDescriptor {
    /// The quant we surface for download/size: the declared default, else the
    /// first file.
    pub(crate) fn default_file(&self) -> Option<&QuantFile> {
        default_quant_file(&self.files, self.default_quant.as_deref())
    }

    /// Render the frontend-facing [`ModelInfo`] by combining this spec with live
    /// disk `status`.
    pub fn to_model_info(&self, status: &DiskStatus) -> ModelInfo {
        self.render_model_info(self.default_file(), status)
    }

    /// [`ModelInfo`] for one specific quant `file` of this catalog model — how
    /// alternate-quant files found on disk surface with full catalog metadata
    /// instead of as anonymous customs. The default quant keeps the plain
    /// catalog name; any other quant appends it ("Name (Q4_K_M)") so two
    /// quants of one model stay tellable apart, and only the default carries
    /// the Recommended badge. Identity stays `"{repo_id}/{filename}"`, so the
    /// default quant renders identically to the seeded entry.
    pub fn to_model_info_for_file(&self, file: &QuantFile, status: &DiskStatus) -> ModelInfo {
        self.render_model_info(Some(file), status)
    }

    fn render_model_info(&self, file: Option<&QuantFile>, status: &DiskStatus) -> ModelInfo {
        let is_default = match (file, self.default_file()) {
            (Some(f), Some(d)) => f.filename == d.filename,
            _ => true,
        };
        let id = match (&self.source, file) {
            (ModelSource::HuggingFace { repo_id, .. }, Some(f)) => {
                format!("{}/{}", repo_id, f.filename)
            }
            _ => self.id.clone(),
        };
        let name = if is_default {
            self.name.clone()
        } else {
            format!(
                "{} ({})",
                self.name,
                file.map(|f| f.quant.as_str()).unwrap_or("")
            )
        };
        let languages =
            canonicalize_supported_languages(self.caps.languages.clone().unwrap_or_default());
        ModelInfo {
            id,
            name,
            description: self.description.clone(),
            filename: file.map(|f| f.filename.clone()).unwrap_or_default(),
            source: self.source.clone(),
            size_mb: file.map(|f| f.size_bytes / (1024 * 1024)).unwrap_or(0),
            is_downloaded: status.is_downloaded,
            is_downloading: status.is_downloading,
            partial_size: status.partial_size,
            accuracy_score: self.accuracy_score,
            speed_score: self.speed_score,
            supports_translation: self.caps.supports_translation.unwrap_or(false),
            is_recommended: self.recommended && is_default,
            supports_language_selection: languages.len() > 1,
            supported_languages: languages,
            // Catalog models are always HF-sourced downloads, never user-dropped
            // custom files (those bypass the descriptor and set this directly).
            is_custom: false,
            supports_streaming: self.caps.supports_streaming.unwrap_or(false),
            supports_language_detection: self.caps.supports_language_detect.unwrap_or(false),
        }
    }
}

/// Resolve the user's persisted language *intent* (`"auto"` or a language code)
/// into the language a given model will actually use.
///
/// The canonical coercion used on every transcription path: computed at the
/// point of use and **never written back** to settings, so the user's last
/// explicit intent survives switching to an incompatible model and back.
///
/// Matching is base-aware ([`base_language`]) and returns the model's own
/// *concrete* code, so a bare intent (`en`) resolves to the exact string the
/// engine's prompt table expects (`en-US`) for models that advertise full
/// BCP-47 locales.
///
/// `preferred_languages` (`spoken_languages`) only matters when the model can't
/// detect and the intent isn't usable: a single spoken language beats English.
pub fn effective_language(
    intent: &str,
    preferred_languages: &[String],
    supported_languages: &[String],
    supports_language_detection: bool,
) -> String {
    if supported_languages.is_empty() {
        return intent.to_string();
    }

    if intent != "auto" {
        if let Some(code) = supported_code(intent, supported_languages) {
            return code.clone();
        }
    }

    if supports_language_detection {
        return "auto".to_string();
    }

    // Model can't auto-detect and the intent isn't usable: fall back to a
    // concrete language so we never hand the engine "auto". The one language
    // they speak, if the model hears it; else English; else the model's first.
    if let [only] = preferred_languages {
        if let Some(code) = supported_code(only, supported_languages) {
            return code.clone();
        }
    }
    if let Some(en) = supported_languages
        .iter()
        .find(|language| base_language(language) == "en")
    {
        return en.clone();
    }
    recognition_language(&supported_languages[0]).to_string()
}

/// The model's own code for `language`, if it hears it.
pub(crate) fn supported_code<'a>(
    language: &str,
    supported_languages: &'a [String],
) -> Option<&'a String> {
    // Exact base first, then aliases: with both `no` and `nb` listed, `nb` must pick `nb`.
    supported_languages
        .iter()
        .find(|code| base_language(code) == base_language(language))
        .or_else(|| {
            supported_languages
                .iter()
                .find(|code| canonical_language_code(code) == canonical_language_code(language))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
    /// Measured over a trailing window of this transfer only (bytes resumed
    /// from disk excluded); `None` until there is enough to measure.
    pub bytes_per_sec: Option<f64>,
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

fn hf_cached_path(repo_id: &str, revision: &str, filename: &str) -> Option<PathBuf> {
    hf_cached_path_in(&hf_caches(), repo_id, revision, filename)
}

/// Reuse an existing file regardless of which revision originally downloaded it.
/// Prefer the requested pin, then main, then other snapshots.
fn hf_cached_path_in(
    caches: &[Cache],
    repo_id: &str,
    revision: &str,
    filename: &str,
) -> Option<PathBuf> {
    hf_cached_paths_in(caches, repo_id, revision, filename)
        .into_iter()
        .find(|path| path.is_file())
}

/// Snapshot directories in lookup order, with the active cache winning ties.
/// Shared by lookup, discovery and deletion; no catalog revision history needed.
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

/// Include dangling pointers so deleting a model cleans up all of its copies.
fn hf_cached_paths_in(
    caches: &[Cache],
    repo_id: &str,
    revision: &str,
    filename: &str,
) -> Vec<PathBuf> {
    hf_snapshot_paths(caches, repo_id, revision)
        .into_iter()
        .map(|snapshot| snapshot.join(filename))
        .filter(|path| path.is_file() || path.is_symlink())
        .collect()
}

/// The `models--org--name` dir a cached file lives in.
fn hf_repo_dir(cached_file: &Path) -> Option<PathBuf> {
    cached_file
        .ancestors()
        .find(|dir| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("models--"))
        })
        .map(Path::to_path_buf)
}

/// The blob a snapshot pointer owns, if it may be deleted along with it:
/// `None` for plain files (on Windows hf-hub's symlink fallback renames the
/// blob into the snapshot), dangling pointers (copies at other pins can share
/// a blob that's already gone), targets outside the repo's `blobs/`, and blobs
/// another filename still references.
fn unshared_blob(pointer: &Path, deleting: &HashSet<PathBuf>) -> std::io::Result<Option<PathBuf>> {
    let is_symlink = fs::symlink_metadata(pointer)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    let Some(repo_dir) = hf_repo_dir(pointer).filter(|_| is_symlink) else {
        return Ok(None);
    };
    let blob = match fs::canonicalize(pointer) {
        Ok(blob) => blob,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    // Only delete actual HF blobs, not arbitrary symlink targets.
    let blobs = fs::canonicalize(repo_dir.join("blobs"))?;
    if blob.parent() != Some(blobs.as_path())
        || blob_is_shared(&repo_dir.join("snapshots"), &blob, deleting)?
    {
        return Ok(None);
    }
    Ok(Some(blob))
}

/// Preserve blobs referenced by another filename, including files in subfolders.
/// Listing errors are returned rather than guessing that a blob is unshared.
fn blob_is_shared(dir: &Path, blob: &Path, deleting: &HashSet<PathBuf>) -> std::io::Result<bool> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if deleting.contains(&path) {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if blob_is_shared(&path, blob, deleting)? {
                return Ok(true);
            }
        } else if kind.is_symlink() {
            match fs::canonicalize(&path) {
                Ok(target) if target == blob => return Ok(true),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(false)
}

/// `fs::remove_file`, where a file that's already gone counts as nothing
/// removed rather than an error, so deleting stays idempotent.
fn remove_file_if_present(path: &Path) -> std::io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        // Name the file: a delete can stop partway through several copies.
        Err(e) => Err(std::io::Error::new(
            e.kind(),
            format!("couldn't remove {}: {}", path.display(), e),
        )),
    }
}

/// Friendly name advertised by GGUF metadata, if present. Empty strings are not
/// useful display names, so callers can keep their filename/repo fallback.
fn probed_display_name(probe: &CapabilityProbe) -> Option<String> {
    probe
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Capability fields for a locally-discovered on-disk model, derived from its
/// GGUF header probe. Anything without readable GGUF metadata — a legacy `.bin`
/// file, or a header that simply omits a key — collapses to "no advertised
/// capability"; transcribe-cpp still reconciles the real values at load time.
/// Shared by both local discovery paths (custom models dir + HF cache) so they
/// surface capabilities identically.
struct LocalCaps {
    supports_streaming: bool,
    supports_translation: bool,
    supports_language_selection: bool,
    supports_language_detection: bool,
    supported_languages: Vec<String>,
}

fn local_caps(probe: &CapabilityProbe) -> LocalCaps {
    let languages = canonicalize_supported_languages(probe.languages.clone().unwrap_or_default());
    LocalCaps {
        supports_streaming: probe.supports_streaming.unwrap_or(false),
        supports_translation: probe.supports_translation.unwrap_or(false),
        // Only offer a language picker when there's more than one to choose.
        supports_language_selection: languages.len() > 1,
        supports_language_detection: probe.supports_language_detect.unwrap_or(false),
        supported_languages: languages,
    }
}

/// Download id under which a standalone language detector download reports
/// progress, completion and cancellation, like a model's.
pub const LANGUAGE_DETECTOR_ID: &str = "language-detector";

/// Where one file's download progress is reported: under `model_id`, its bytes
/// counted from `offset`, out of `total` (default: `offset` + the file's own
/// size). Lets two files (the language detector, then a model) fill one bar.
#[derive(Clone)]
pub(super) struct ProgressTarget {
    pub model_id: String,
    pub offset: u64,
    pub total: Option<u64>,
}

impl ProgressTarget {
    /// A file reporting on its own.
    fn own(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            offset: 0,
            total: None,
        }
    }

    /// The event for `downloaded` of this file's `total` bytes.
    pub(super) fn progress(
        &self,
        downloaded: u64,
        total: u64,
        bytes_per_sec: Option<f64>,
    ) -> DownloadProgress {
        let downloaded = self.offset + downloaded;
        let total = self.total.unwrap_or(self.offset + total).max(downloaded);
        DownloadProgress {
            model_id: self.model_id.clone(),
            downloaded,
            total,
            percentage: if total > 0 {
                downloaded as f64 / total as f64 * 100.0
            } else {
                0.0
            },
            bytes_per_sec,
        }
    }
}

/// One file to fetch from Hugging Face, with mirror fallback.
struct FileTransfer<'a> {
    /// What the logs call it.
    label: &'a str,
    repo_id: &'a str,
    revision: &'a str,
    filename: &'a str,
    mirrors: Vec<crate::catalog::MirrorFile>,
    progress: ProgressTarget,
}

/// Bridges hf-hub's async download progress to Handy's `model-download-progress`
/// event. hf-hub clones the reporter, so shared state lives behind an `Arc`.
#[derive(Clone)]
struct HfDownloadProgress {
    app_handle: AppHandle,
    target: ProgressTarget,
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
            total: 0,
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
    fn new(app_handle: AppHandle, target: ProgressTarget) -> Self {
        Self {
            app_handle,
            target,
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
        let _ = self.app_handle.emit(
            "model-download-progress",
            &self.target.progress(downloaded, total, bytes_per_sec),
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

/// RAII guard that clears the `is_rescanning` single-flight flag on drop, so the
/// slot is released on every exit path (including early returns and `?`).
struct RescanGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for RescanGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

/// RAII guard that cleans up download state (`is_downloading` flag and cancel flag)
/// when dropped, unless explicitly disarmed. This ensures consistent cleanup on
/// every error path without requiring manual cleanup at each `?` or `return Err`.
struct DownloadCleanup<'a> {
    available_models: &'a Mutex<HashMap<String, ModelInfo>>,
    cancel_flags: &'a Arc<Mutex<HashMap<String, CancellationToken>>>,
    model_id: String,
    disarmed: bool,
}

impl<'a> Drop for DownloadCleanup<'a> {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(self.model_id.as_str()) {
                model.is_downloading = false;
            }
        }
        self.cancel_flags.lock().unwrap().remove(&self.model_id);
    }
}

/// A retired ONNX model: nothing can load it anymore, but it may still be on
/// disk. The update screen offers its GGUF replacement and then deletes it.
/// This table is the single source of truth for retired models; the frontend
/// only renders what `get_retired_models` reports.
struct RetiredOnnxModel {
    id: &'static str,
    name: &'static str,
    /// Every name the model was stored under in the models dir: a directory,
    /// or for GigaAM before #1088, a single `.onnx` file.
    disk_names: &'static [&'static str],
    size_mb: u64,
    /// Hugging Face repo of the catalog model that replaces it.
    replacement_repo: &'static str,
}

const RETIRED_ONNX_MODELS: &[RetiredOnnxModel] = &[
    RetiredOnnxModel {
        id: "parakeet-tdt-0.6b-v2",
        name: "Parakeet V2",
        disk_names: &["parakeet-tdt-0.6b-v2-int8"],
        size_mb: 451,
        replacement_repo: "handy-computer/parakeet-tdt-0.6b-v2-gguf",
    },
    RetiredOnnxModel {
        id: "parakeet-tdt-0.6b-v3",
        name: "Parakeet V3",
        disk_names: &["parakeet-tdt-0.6b-v3-int8"],
        size_mb: 456,
        replacement_repo: "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    },
    RetiredOnnxModel {
        id: "moonshine-base",
        name: "Moonshine Base",
        disk_names: &["moonshine-base"],
        size_mb: 55,
        replacement_repo: "handy-computer/moonshine-base-gguf",
    },
    RetiredOnnxModel {
        id: "moonshine-tiny-streaming-en",
        name: "Moonshine V2 Tiny",
        disk_names: &["moonshine-tiny-streaming-en"],
        size_mb: 31,
        replacement_repo: "handy-computer/moonshine-streaming-tiny-gguf",
    },
    RetiredOnnxModel {
        id: "moonshine-small-streaming-en",
        name: "Moonshine V2 Small",
        disk_names: &["moonshine-small-streaming-en"],
        size_mb: 99,
        replacement_repo: "handy-computer/moonshine-streaming-small-gguf",
    },
    RetiredOnnxModel {
        id: "moonshine-medium-streaming-en",
        name: "Moonshine V2 Medium",
        disk_names: &["moonshine-medium-streaming-en"],
        size_mb: 192,
        replacement_repo: "handy-computer/moonshine-streaming-medium-gguf",
    },
    RetiredOnnxModel {
        id: "sense-voice-int8",
        name: "SenseVoice",
        disk_names: &["sense-voice-int8"],
        size_mb: 152,
        replacement_repo: "handy-computer/SenseVoiceSmall-gguf",
    },
    RetiredOnnxModel {
        id: "gigaam-v3-e2e-ctc",
        name: "GigaAM v3",
        disk_names: &["giga-am-v3-int8", "giga-am-v3.int8.onnx"],
        size_mb: 151,
        replacement_repo: "handy-computer/gigaam-v3-e2e-ctc-gguf",
    },
    RetiredOnnxModel {
        id: "canary-180m-flash",
        name: "Canary 180M Flash",
        disk_names: &["canary-180m-flash"],
        size_mb: 146,
        replacement_repo: "handy-computer/canary-180m-flash-gguf",
    },
    RetiredOnnxModel {
        id: "canary-1b-v2",
        name: "Canary 1B v2",
        disk_names: &["canary-1b-v2"],
        size_mb: 691,
        replacement_repo: "handy-computer/canary-1b-v2-gguf",
    },
    RetiredOnnxModel {
        id: "cohere-int8",
        name: "Cohere",
        disk_names: &["cohere-int8"],
        size_mb: 1708,
        replacement_repo: "handy-computer/cohere-transcribe-03-2026-gguf",
    },
];

/// A retired ONNX model found on disk.
#[derive(Serialize, Clone, Debug, Type)]
pub struct RetiredModel {
    pub id: String,
    pub name: String,
    pub size_mb: u64,
    pub replacement_repo: String,
}

pub struct ModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    available_models: Mutex<HashMap<String, ModelInfo>>,
    cancel_flags: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Single-flight guard for [`Self::rescan_local_models`] so concurrent
    /// refresh requests coalesce instead of scanning the disk in parallel.
    is_rescanning: Arc<AtomicBool>,
    /// One language detector transfer at a time, however many downloads
    /// want it.
    detector_download: tokio::sync::Mutex<()>,
}

impl ModelManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create models directory in app data
        let models_dir = crate::portable::app_data_dir(app_handle)
            .map_err(|e| anyhow::anyhow!("Failed to get app data dir: {}", e))?
            .join("models");

        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let mut available_models = HashMap::new();

        // Whisper supported languages (99 languages from tokenizer)
        let whisper_languages: Vec<String> = vec![
            "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar",
            "sv", "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu",
            "ta", "no", "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa",
            "lv", "bn", "sr", "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy", "ne", "mn",
            "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc",
            "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn",
            "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln", "ha", "ba", "jw",
            "su", "yue",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        available_models.insert(
            "small".to_string(),
            ModelInfo {
                id: "small".to_string(),
                name: "Whisper Small".to_string(),
                description: "Fast and fairly accurate.".to_string(),
                filename: "ggml-small.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://blob.handy.computer/ggml-small.bin".to_string(),
                    sha256: Some(
                        "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
                            .to_string(),
                    ),
                },
                size_mb: 465,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.60,
                speed_score: 0.85,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Add downloadable models
        available_models.insert(
            "medium".to_string(),
            ModelInfo {
                id: "medium".to_string(),
                name: "Whisper Medium".to_string(),
                description: "Good accuracy, medium speed".to_string(),
                filename: "whisper-medium-q4_1.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://blob.handy.computer/whisper-medium-q4_1.bin".to_string(),
                    sha256: Some(
                        "79283fc1f9fe12ca3248543fbd54b73292164d8df5a16e095e2bceeaaabddf57"
                            .to_string(),
                    ),
                },
                size_mb: 469,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.75,
                speed_score: 0.60,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "turbo".to_string(),
            ModelInfo {
                id: "turbo".to_string(),
                name: "Whisper Turbo".to_string(),
                description: "Balanced accuracy and speed.".to_string(),
                filename: "ggml-large-v3-turbo.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://blob.handy.computer/ggml-large-v3-turbo.bin".to_string(),
                    sha256: Some(
                        "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"
                            .to_string(),
                    ),
                },
                size_mb: 1549,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.80,
                speed_score: 0.40,
                supports_translation: false, // Turbo doesn't support translation
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "large".to_string(),
            ModelInfo {
                id: "large".to_string(),
                name: "Whisper Large".to_string(),
                description: "Good accuracy, but slow.".to_string(),
                filename: "ggml-large-v3-q5_0.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://blob.handy.computer/ggml-large-v3-q5_0.bin".to_string(),
                    sha256: Some(
                        "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1"
                            .to_string(),
                    ),
                },
                size_mb: 1031,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.85,
                speed_score: 0.30,
                supports_translation: true,
                is_recommended: false,
                supported_languages: whisper_languages.clone(),
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        available_models.insert(
            "breeze-asr".to_string(),
            ModelInfo {
                id: "breeze-asr".to_string(),
                name: "Breeze ASR".to_string(),
                description: "Optimized for Taiwanese Mandarin. Code-switching support."
                    .to_string(),
                filename: "breeze-asr-q5_k.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://blob.handy.computer/breeze-asr-q5_k.bin".to_string(),
                    sha256: Some(
                        "8efbf0ce8a3f50fe332b7617da787fb81354b358c288b008d3bdef8359df64c6"
                            .to_string(),
                    ),
                },
                size_mb: 1030,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.85,
                speed_score: 0.35,
                supports_translation: false,
                is_recommended: false,
                supported_languages: whisper_languages,
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: true,
            },
        );

        // Seed the bundled offline catalog before the on-disk scans, so a model
        // already in the HF cache dedups onto its richer catalog entry (the scans
        // only insert ids not already present) instead of showing as a bare cache
        // find. Additive — see `seed_catalog_models`.
        Self::seed_catalog_models(&mut available_models);

        // Auto-discover custom transcribe-cpp models (.bin / .gguf) in the models directory
        if let Err(e) = Self::discover_custom_transcribe_models(&models_dir, &mut available_models)
        {
            warn!("Failed to discover custom models: {}", e);
        }

        // Auto-discover transcribe-cpp GGUF models already in the shared HF cache.
        Self::discover_hf_cache_models(&mut available_models);

        let manager = Self {
            app_handle: app_handle.clone(),
            models_dir,
            available_models: Mutex::new(available_models),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            is_rescanning: Arc::new(AtomicBool::new(false)),
            detector_download: tokio::sync::Mutex::new(()),
        };

        // Migrate any bundled models to user directory
        manager.migrate_bundled_models()?;

        // Check which models are already downloaded
        manager.update_download_status()?;

        // Auto-select a model if none is currently selected
        manager.auto_select_model_if_needed()?;

        Ok(manager)
    }

    pub fn get_available_models(&self) -> Vec<ModelInfo> {
        let mut list: Vec<ModelInfo> = {
            let models = self.available_models.lock().unwrap();
            models.values().cloned().collect()
        };
        // Stable, reasonable order: catalog editorial rank first (lower = higher
        // priority), then any other recommended model, then by accuracy, speed,
        // and name. `ModelInfo` doesn't carry rank, so resolve it by id from the
        // catalog here.
        list.sort_by(|a, b| {
            crate::catalog::rank_of(&a.id)
                .cmp(&crate::catalog::rank_of(&b.id))
                .then((!a.is_recommended).cmp(&(!b.is_recommended)))
                .then(b.accuracy_score.total_cmp(&a.accuracy_score))
                .then(b.speed_score.total_cmp(&a.speed_score))
                .then_with(|| a.name.cmp(&b.name))
        });
        list
    }

    /// Seed every bundled catalog default into the registry (additive).
    /// Alternate quants are discovered from disk instead.
    ///
    /// Catalog (`.gguf`, `HuggingFace`) and legacy (`.bin`/ONNX, `Url`) entries
    /// stay SEPARATE — different files, ids, and runtimes. Nothing is merged or
    /// removed; the UI just hides not-on-disk `Url` entries to deprecate legacy
    /// downloads, while already-downloaded ones stay runnable. Runs before the
    /// on-disk scans so a cached model dedups onto its catalog entry.
    fn seed_catalog_models(available_models: &mut HashMap<String, ModelInfo>) {
        use std::collections::hash_map::Entry;
        let mut added = 0usize;
        for desc in crate::catalog::CATALOG.iter() {
            if let Entry::Vacant(slot) = available_models.entry(desc.id.clone()) {
                slot.insert(desc.to_model_info(&DiskStatus::default()));
                added += 1;
            }
        }
        info!("Seeded {} catalog model(s) into the registry", added);
    }

    /// Claim the single rescan slot. Returns a guard that releases it on drop,
    /// or `None` if a rescan is already running (callers should just skip).
    fn try_start_rescan(&self) -> Option<RescanGuard> {
        if self.is_rescanning.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(RescanGuard {
                flag: self.is_rescanning.clone(),
            })
        }
    }

    /// Re-run the local discovery scans (custom models dir + shared HF cache) so
    /// models dropped in or downloaded outside Handy show up without a restart.
    /// The merge is additive: only new ids are inserted, so existing entries keep
    /// their values — including runtime-probed capabilities from
    /// [`Self::set_runtime_capabilities`]. It then runs [`Self::update_download_status`],
    /// which recomputes disk-derived flags for *every* entry; a rescan racing an
    /// in-flight download can briefly clear its `is_downloading`, but the download
    /// continues and the event-driven UI self-corrects.
    ///
    /// The disk walk and 64 KiB header probes run against a cloned snapshot
    /// *off-lock* so readers never block on I/O; only the brief merge takes the
    /// registry lock. Concurrent calls coalesce via [`Self::try_start_rescan`].
    pub fn rescan_local_models(&self) -> Result<()> {
        let _guard = match self.try_start_rescan() {
            Some(g) => g,
            None => {
                debug!("Model rescan already in progress; skipping");
                return Ok(());
            }
        };

        // Snapshot the current registry and discover against the copy off-lock.
        // The discover_* helpers are purely additive (they skip ids already in
        // the map), so the snapshot ends up as {current} ∪ {newly-found}.
        let mut snapshot = self.available_models.lock().unwrap().clone();
        if let Err(e) = Self::discover_custom_transcribe_models(&self.models_dir, &mut snapshot) {
            warn!("Rescan: failed to discover custom models: {}", e);
        }
        Self::discover_hf_cache_models(&mut snapshot);

        // Merge only the genuinely-new ids back into the live registry. `or_insert`
        // leaves every existing entry exactly as it was.
        let mut added = 0usize;
        {
            let mut live = self.available_models.lock().unwrap();
            for (id, info) in snapshot {
                if let std::collections::hash_map::Entry::Vacant(entry) = live.entry(id) {
                    entry.insert(info);
                    added += 1;
                }
            }
        }

        self.update_download_status()?;
        self.auto_select_model_if_needed()?;
        if added > 0 {
            info!("Model rescan discovered {} new model(s)", added);
        }
        let _ = self.app_handle.emit("models-updated", ());
        Ok(())
    }

    pub fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        let models = self.available_models.lock().unwrap();
        models.get(model_id).cloned()
    }

    /// Reconcile a model's advertised capabilities with the ground truth from the
    /// loaded model (transcribe-cpp's GGUF-derived capabilities), overwriting the
    /// pre-download view (catalog metadata or a header probe — see
    /// [`super::model_capabilities`]).
    ///
    /// This corrects the header probe's gaps. It matters most for **streaming**
    /// (transcribe-cpp infers it at load for parakeet/streaming families, where
    /// the flat GGUF key can be absent, and it gates whether streaming is even
    /// attempted — see `actions.rs`) and for **language detection** / the
    /// **supported-language set**, which feed [`effective_language`]; a mislabeled
    /// header would otherwise coerce an "auto" intent to a forced language for good.
    /// Translate is reconciled too for badge accuracy, though run paths re-read it
    /// live regardless.
    pub fn set_runtime_capabilities(
        &self,
        model_id: &str,
        supports_streaming: bool,
        supports_translation: bool,
        supports_language_detection: bool,
        supported_languages: Vec<String>,
    ) {
        let supported_languages = canonicalize_supported_languages(supported_languages);
        let mut models = self.available_models.lock().unwrap();
        if let Some(model) = models.get_mut(model_id) {
            model.supports_streaming = supports_streaming;
            model.supports_translation = supports_translation;
            model.supports_language_detection = supports_language_detection;
            // An empty set means the model is language-agnostic — but it is also
            // what a failed capability read leaves behind, so keep the probed /
            // catalog list rather than blanking a known one to nothing.
            if !supported_languages.is_empty() {
                model.supports_language_selection = supported_languages.len() > 1;
                model.supported_languages = supported_languages;
            }
        }
    }

    fn migrate_bundled_models(&self) -> Result<()> {
        // Check for bundled models and copy them to user directory
        let bundled_models = ["ggml-small.bin"]; // Add other bundled models here if any

        for filename in &bundled_models {
            let bundled_path = self.app_handle.path().resolve(
                format!("resources/models/{}", filename),
                tauri::path::BaseDirectory::Resource,
            );

            if let Ok(bundled_path) = bundled_path {
                if bundled_path.exists() {
                    let user_path = self.models_dir.join(filename);

                    // Only copy if user doesn't already have the model
                    if !user_path.exists() {
                        info!("Migrating bundled model {} to user directory", filename);
                        fs::copy(&bundled_path, &user_path)?;
                        info!("Successfully migrated {}", filename);
                    }
                }
            }
        }

        Ok(())
    }

    fn is_retired_on_disk(&self, model: &RetiredOnnxModel) -> bool {
        model
            .disk_names
            .iter()
            .any(|name| self.models_dir.join(name).exists())
    }

    /// Retired ONNX models still on disk, in any layout they were stored in.
    pub fn get_retired_models(&self) -> Vec<RetiredModel> {
        RETIRED_ONNX_MODELS
            .iter()
            .filter(|model| self.is_retired_on_disk(model))
            .map(|model| RetiredModel {
                id: model.id.to_string(),
                name: model.name.to_string(),
                size_mb: model.size_mb,
                replacement_repo: model.replacement_repo.to_string(),
            })
            .collect()
    }

    /// Delete everything a retired ONNX model left on disk, in any layout,
    /// including interrupted downloads (`.partial`) and extractions
    /// (`.extracting`).
    pub fn delete_retired_model(&self, model_id: &str) -> Result<()> {
        let model = RETIRED_ONNX_MODELS
            .iter()
            .find(|model| model.id == model_id)
            .ok_or_else(|| anyhow::anyhow!("Not a retired model: {}", model_id))?;
        for name in model.disk_names {
            for suffix in ["", ".partial", ".extracting"] {
                let path = self.models_dir.join(format!("{name}{suffix}"));
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else if path.exists() {
                    fs::remove_file(&path)?;
                } else {
                    continue;
                }
                info!("Deleted retired model files at {:?}", path);
            }
        }
        Ok(())
    }

    fn update_download_status(&self) -> Result<()> {
        // Snapshot in-flight download ids before taking the registry lock (the
        // two locks are never nested) so a mid-download entry is never dropped.
        let downloading_ids: HashSet<String> =
            self.cancel_flags.lock().unwrap().keys().cloned().collect();
        let mut models = self.available_models.lock().unwrap();
        let mut vanished_models: Vec<String> = Vec::new();

        for model in models.values_mut() {
            if let ModelSource::HuggingFace { repo_id, revision } = &model.source {
                // A models-dir copy counts too: mirror-fallback downloads land
                // there, and it makes manual drop-ins of catalog files work.
                let local_path = self.models_dir.join(&model.filename);
                let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));
                model.is_downloaded = hf_cached_path(repo_id, revision, &model.filename).is_some()
                    || local_path.exists();
                model.is_downloading = false;
                model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
                // Alternate-quant entries exist only because their file was
                // discovered on disk — the catalog offers just the default
                // quant, so they are never presented for download. When the
                // file is gone, the entry goes with it.
                if !model.is_downloaded
                    && !downloading_ids.contains(&model.id)
                    && Self::disappears_when_missing(model)
                {
                    vanished_models.push(model.id.clone());
                }
                continue;
            }
            let model_path = self.models_dir.join(&model.filename);
            let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));

            model.is_downloaded = model_path.exists();
            model.is_downloading = false;

            // Get partial file size if it exists
            if partial_path.exists() {
                model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
            } else {
                model.partial_size = 0;
            }

            // Custom entries are discovered from files and have no download
            // source. Once their file is gone, keeping the entry would offer a
            // Download action that can never succeed.
            if !model.is_downloaded
                && !downloading_ids.contains(&model.id)
                && Self::disappears_when_missing(model)
            {
                vanished_models.push(model.id.clone());
            }
        }

        for id in vanished_models {
            models.remove(&id);
        }

        Ok(())
    }

    /// Whether `filename` is a catalog-listed quant of `repo_id` other than
    /// the default — the only quant the catalog seeds and offers for download.
    fn is_catalog_alternate_quant(repo_id: &str, filename: &str) -> bool {
        crate::catalog::file_in_catalog(filename, Some(repo_id)).is_some_and(|(desc, file)| {
            desc.default_file()
                .is_some_and(|d| d.filename != file.filename)
        })
    }

    /// Entries discovered exclusively from disk should disappear with their
    /// files. Catalog defaults remain so the UI can offer them for download.
    fn disappears_when_missing(model: &ModelInfo) -> bool {
        model.is_custom
            || match &model.source {
                ModelSource::HuggingFace { repo_id, .. } => {
                    Self::is_catalog_alternate_quant(repo_id, &model.filename)
                }
                _ => false,
            }
    }

    fn selected_model_is_available(models: &HashMap<String, ModelInfo>, model_id: &str) -> bool {
        models
            .get(model_id)
            .is_some_and(|model| model.is_downloaded)
    }

    /// Remove a filename's snapshot pointers and unshared blobs from the HF
    /// cache. Everything else in the repo (other quants, refs) is left
    /// untouched. Returns whether anything was removed, or the error that kept
    /// the model's file on disk.
    fn delete_hf_cache_file(
        repo_id: &str,
        revision: &str,
        filename: &str,
    ) -> std::io::Result<bool> {
        Self::delete_hf_cache_file_in(&hf_caches(), repo_id, revision, filename)
    }

    /// Remove every cached copy of this filename and its unshared blobs.
    /// Leave the repo's other files and empty directories alone.
    fn delete_hf_cache_file_in(
        caches: &[Cache],
        repo_id: &str,
        revision: &str,
        filename: &str,
    ) -> std::io::Result<bool> {
        let pointers = hf_cached_paths_in(caches, repo_id, revision, filename);
        let deleting: HashSet<PathBuf> = pointers.iter().cloned().collect();

        let mut deleted = false;
        for pointer in pointers {
            // The blob goes first: if it can't be removed, the pointer still
            // marks the model as downloaded, which stays true.
            match unshared_blob(&pointer, &deleting) {
                Ok(Some(blob)) => {
                    info!("Deleting HF cache blob at: {:?}", blob);
                    deleted |= remove_file_if_present(&blob)?;
                }
                Ok(None) => {}
                // Can't tell which blob it is or whether it's shared (e.g.
                // canonicalize is unsupported on some Windows volumes): keep
                // the blob rather than fail the delete or guess. At worst
                // its space stays used.
                Err(e) => warn!("Keeping HF cache blob for {:?}: {}", pointer, e),
            }
            info!("Deleting HF cache file at: {:?}", pointer);
            deleted |= remove_file_if_present(&pointer)?;
        }

        Ok(deleted)
    }

    fn auto_select_model_if_needed(&self) -> Result<()> {
        let mut settings = get_settings(&self.app_handle);

        // A model cannot remain selected after its files disappear. Catalog
        // entries stay in the registry so they can be downloaded again, so
        // checking only whether the id exists is not sufficient.
        if !settings.selected_model.is_empty() {
            let is_available = {
                let models = self.available_models.lock().unwrap();
                Self::selected_model_is_available(&models, &settings.selected_model)
            };

            // A retired model on disk stays selected so the frontend can
            // offer its replacement. It can't load: it isn't in the registry.
            let is_retired = RETIRED_ONNX_MODELS
                .iter()
                .any(|model| model.id == settings.selected_model && self.is_retired_on_disk(model));

            if !is_available && !is_retired {
                info!(
                    "Selected model '{}' is not available on disk; clearing selection",
                    settings.selected_model
                );
                settings.selected_model = String::new();
                write_settings(&self.app_handle, settings.clone());
            }
        }

        // If onboarding is still pending, do not auto-select just because a
        // compatible model exists on disk or in the shared HF cache. The
        // onboarding model step should present that choice explicitly.
        if !settings.onboarding_completed {
            debug!("Skipping model auto-selection until onboarding is complete");
            return Ok(());
        }

        // If no model is selected, pick the first downloaded one using the same
        // ranked order the UI receives.
        if settings.selected_model.is_empty() {
            if let Some(available_model) = self
                .get_available_models()
                .into_iter()
                .find(|model| model.is_downloaded)
            {
                info!(
                    "Auto-selecting model: {} ({})",
                    available_model.id, available_model.name
                );

                // Update settings with the selected model
                let mut updated_settings = settings;
                updated_settings.selected_model = available_model.id.clone();
                write_settings(&self.app_handle, updated_settings);

                info!("Successfully auto-selected model: {}", available_model.id);
            }
        }

        Ok(())
    }

    /// Discover custom Whisper-family models in the models directory: legacy
    /// GGML `.bin` files and `.gguf` files (both load through transcribe-cpp).
    /// Skips files that match predefined model filenames.
    fn discover_custom_transcribe_models(
        models_dir: &Path,
        available_models: &mut HashMap<String, ModelInfo>,
    ) -> Result<()> {
        if !models_dir.exists() {
            return Ok(());
        }

        // Collect filenames of predefined models to skip
        let predefined_filenames: HashSet<String> = available_models
            .values()
            .map(|m| m.filename.clone())
            .collect();

        // Scan models directory for .bin / .gguf files
        for entry in fs::read_dir(models_dir)? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();

            // Skip directories; the .bin / .gguf extension filter is below.
            if !path.is_file() {
                continue;
            }

            let filename = match path.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Skip hidden files
            if filename.starts_with('.') {
                continue;
            }

            // Only process Whisper-family model files: legacy GGML `.bin` or
            // GGUF `.gguf` (both load through transcribe-cpp). Anything else —
            // including `.partial` downloads like "model.bin.partial" — is
            // skipped, since it ends in neither extension. The model ID is the
            // filename with its extension removed.
            let (model_id, is_gguf) = if let Some(stem) = filename.strip_suffix(".bin") {
                (stem.to_string(), false)
            } else if let Some(stem) = filename.strip_suffix(".gguf") {
                (stem.to_string(), true)
            } else {
                continue;
            };

            // Skip predefined model files, and the language detector (a mirror
            // download lands here), which isn't a transcription model.
            if predefined_filenames.contains(&filename)
                || crate::catalog::LANGUAGE_DETECTOR
                    .as_ref()
                    .is_some_and(|detector| detector.filename == filename)
            {
                continue;
            }

            // A file matching ANY catalog-listed quant surfaces as that catalog
            // model — full name/description/scores, quant-suffixed name for
            // non-defaults — instead of as an anonymous custom entry. (Default
            // quants never reach here: they're in `predefined_filenames`.)
            if let Some((desc, quant_file)) = crate::catalog::file_in_catalog(&filename, None) {
                let info = desc.to_model_info_for_file(
                    quant_file,
                    &DiskStatus {
                        is_downloaded: true,
                        ..Default::default()
                    },
                );
                if !available_models.contains_key(&info.id) {
                    info!(
                        "Discovered catalog quant in models dir: {} ({})",
                        info.id, filename
                    );
                    available_models.insert(info.id.clone(), info);
                }
                continue;
            }

            // Skip if model ID already exists (shouldn't happen, but be safe)
            if available_models.contains_key(&model_id) {
                continue;
            }

            // Generate display name: replace - and _ with space, capitalize words
            let fallback_display_name = model_id
                .replace(['-', '_'], " ")
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");

            // Get file size in MB
            let size_mb = match path.metadata() {
                Ok(meta) => meta.len() / (1024 * 1024),
                Err(e) => {
                    warn!("Failed to get metadata for {}: {}", filename, e);
                    0
                }
            };

            // Probe GGUF headers for advertised capabilities so a dropped-in
            // model surfaces streaming / translation / languages just like a
            // Handy-downloaded one. Legacy `.bin` files have no GGUF header, so
            // they stay "unknown" until transcribe-cpp reconciles them at load.
            let probe = if is_gguf {
                GgufHeaderProber.probe_file(&path)
            } else {
                CapabilityProbe::default()
            };
            let caps = local_caps(&probe);
            let display_name = probed_display_name(&probe).unwrap_or(fallback_display_name);

            info!(
                "Discovered custom transcribe-cpp model: {} ({}, {} MB, streaming={})",
                model_id, filename, size_mb, caps.supports_streaming
            );

            available_models.insert(
                model_id.clone(),
                ModelInfo {
                    id: model_id,
                    name: display_name,
                    description: "Not officially supported".to_string(),
                    filename,
                    source: ModelSource::Local, // already on disk; nothing to download
                    size_mb,
                    is_downloaded: true, // Already present on disk
                    is_downloading: false,
                    partial_size: 0,
                    accuracy_score: 0.0, // Sentinel: UI hides score bars when both are 0
                    speed_score: 0.0,
                    supports_translation: caps.supports_translation,
                    is_recommended: false,
                    supported_languages: caps.supported_languages,
                    supports_language_selection: caps.supports_language_selection,
                    is_custom: true,
                    supports_streaming: caps.supports_streaming,
                    supports_language_detection: caps.supports_language_detection,
                },
            );
        }

        Ok(())
    }

    /// Discover transcribe-cpp-compatible GGUF models already present in the
    /// shared Hugging Face cache, so models downloaded by Handy (or any other
    /// tool) appear in "Your Models" without re-downloading. Only architectures
    /// transcribe-cpp recognises are surfaced; arbitrary (e.g. LLM) GGUFs that
    /// share the cache are ignored.
    fn discover_hf_cache_models(available_models: &mut HashMap<String, ModelInfo>) {
        for cache in hf_caches() {
            Self::discover_hf_cache_models_in(cache.path(), available_models);
        }
    }

    /// Scan a Hugging Face cache root (`<cache>/models--*`) for GGUF snapshots.
    /// Split from [`Self::discover_hf_cache_models`] so it can be tested against
    /// a synthetic cache directory.
    fn discover_hf_cache_models_in(
        cache_root: &Path,
        available_models: &mut HashMap<String, ModelInfo>,
    ) {
        if !cache_root.is_dir() {
            return;
        }

        // Repo+file pairs already represented (e.g. recommended/added models) so
        // the same file is not listed twice.
        let known_hf: HashSet<(String, String)> = available_models
            .values()
            .filter_map(|m| match &m.source {
                ModelSource::HuggingFace { repo_id, .. } => {
                    Some((repo_id.clone(), m.filename.clone()))
                }
                _ => None,
            })
            .collect();

        let prober = GgufHeaderProber;

        let entries = match fs::read_dir(cache_root) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let folder = entry.file_name();
            let folder = folder.to_string_lossy();
            let Some(rest) = folder.strip_prefix("models--") else {
                continue;
            };
            // Reverse hf-hub's `org/name` -> `models--org--name` folder naming.
            let repo_id = rest.replace("--", "/");

            let caches = [Cache::new(cache_root.to_path_buf())];
            for snapshot in hf_snapshot_paths(&caches, &repo_id, "main") {
                let Some(revision) = snapshot.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Ok(files) = fs::read_dir(&snapshot) else {
                    continue;
                };

                for file in files.flatten() {
                    let fname = file.file_name().to_string_lossy().to_string();
                    if !fname.ends_with(".gguf") || !file.path().is_file() {
                        continue;
                    }
                    if known_hf.contains(&(repo_id.clone(), fname.clone())) {
                        continue;
                    }
                    let model_id = format!("{}/{}", repo_id, fname);
                    if available_models.contains_key(&model_id) {
                        continue;
                    }

                    // Catalog-listed quants (same repo) surface with full catalog
                    // metadata — quant-suffixed name for non-defaults — and skip
                    // the header probe (the catalog is authoritative for its own
                    // models). Everything else keeps the generic probed path.
                    if let Some((desc, quant_file)) =
                        crate::catalog::file_in_catalog(&fname, Some(&repo_id))
                    {
                        let info = desc.to_model_info_for_file(
                            quant_file,
                            &DiskStatus {
                                is_downloaded: true,
                                ..Default::default()
                            },
                        );
                        info!(
                            "Discovered catalog quant in HF cache: {} ({})",
                            info.id, repo_id
                        );
                        available_models.insert(info.id.clone(), info);
                        continue;
                    }

                    let path = snapshot.join(&fname);
                    let probe = prober.probe_file(&path);
                    // Only surface models transcribe-cpp recognises.
                    if probe.verdict != Compatibility::Compatible {
                        continue;
                    }
                    let caps = local_caps(&probe);

                    let size_mb = path
                        .metadata()
                        .map(|m| m.len() / (1024 * 1024))
                        .unwrap_or(0);
                    let display = probed_display_name(&probe)
                        .unwrap_or_else(|| fname.trim_end_matches(".gguf").to_string());

                    info!("Discovered HF cache model: {} ({})", model_id, repo_id);
                    available_models.insert(
                        model_id.clone(),
                        ModelInfo {
                            id: model_id,
                            name: display,
                            description: format!("From Hugging Face cache: {}", repo_id),
                            filename: fname,
                            source: ModelSource::HuggingFace {
                                repo_id: repo_id.clone(),
                                revision: revision.to_string(),
                            },
                            size_mb,
                            is_downloaded: true,
                            is_downloading: false,
                            partial_size: 0,
                            accuracy_score: 0.0,
                            speed_score: 0.0,
                            supports_translation: caps.supports_translation,
                            is_recommended: false,
                            supported_languages: caps.supported_languages,
                            supports_language_selection: caps.supports_language_selection,
                            is_custom: false,
                            supports_streaming: caps.supports_streaming,
                            supports_language_detection: caps.supports_language_detection,
                        },
                    );
                }
            }
        }
    }

    /// Download a Hugging Face-sourced model into the shared HF cache via
    /// hf-hub, reporting progress through the same `model-download-progress`
    /// event the URL path uses. Uses hf-hub's stock cache, but deliberately
    /// disables authentication because every catalog repository is public.
    /// A model that needs the language detector gets it first, in one bar.
    async fn download_hf_model(
        &self,
        model_info: &ModelInfo,
        repo_id: String,
        revision: String,
    ) -> Result<()> {
        let model_id = model_info.id.clone();
        let filename = model_info.filename.clone();

        // Already in the shared cache (possibly from another tool), or dropped
        // into the models dir (mirror fallback / manual install)?
        let model_present = hf_cached_path(&repo_id, &revision, &filename).is_some()
            || self.models_dir.join(&filename).exists();
        let detector = self.missing_detector_for(model_info);
        if model_present && detector.is_none() {
            self.update_download_status()?;
            let _ = self.app_handle.emit("model-download-complete", &model_id);
            return Ok(());
        }

        // Mark downloading; the guard resets the flag on any error path.
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(&model_id) {
                model.is_downloading = true;
            }
        }

        // Register a cancellation token so `cancel_download` can abort this
        // transfer promptly. The guard removes it on every exit path.
        let cancel_token = CancellationToken::new();
        {
            let mut flags = self.cancel_flags.lock().unwrap();
            flags.insert(model_id.clone(), cancel_token.clone());
        }

        let mut cleanup = DownloadCleanup {
            available_models: &self.available_models,
            cancel_flags: &self.cancel_flags,
            model_id: model_id.clone(),
            disarmed: false,
        };

        // The detector, then the model, as one bar: the detector's bytes come
        // first, out of both files' total.
        let mut offset = 0;
        if let Some(detector) = detector {
            let model_bytes = if model_present {
                0
            } else {
                crate::catalog::file_in_catalog(&filename, Some(&repo_id))
                    .map(|(_, file)| file.size_bytes)
                    .unwrap_or(model_info.size_mb * 1024 * 1024)
            };
            let progress = ProgressTarget {
                model_id: model_id.clone(),
                offset: 0,
                total: Some(detector.size_bytes + model_bytes),
            };
            match self.fetch_language_detector(progress, &cancel_token).await {
                Ok(true) => offset = detector.size_bytes,
                Ok(false) => {
                    info!("HF download cancelled for: {}", model_id);
                    return Ok(());
                }
                // The model works without it: Auto falls back to the first
                // preferred language and the language picker offers the
                // download, so it doesn't cost them the model.
                Err(e) => warn!(
                    "Language detector download failed; continuing with {}: {:#}",
                    model_id, e
                ),
            }
        }

        if !model_present {
            let transfer = FileTransfer {
                label: &model_id,
                repo_id: &repo_id,
                revision: &revision,
                filename: &filename,
                mirrors: crate::catalog::mirror_fallbacks(&model_id),
                progress: ProgressTarget {
                    model_id: model_id.clone(),
                    offset,
                    total: None,
                },
            };
            if !self.fetch_hf_file(&transfer, &cancel_token).await? {
                return Ok(());
            }
        }

        cleanup.disarmed = true;
        self.update_download_status()?;
        self.cancel_flags.lock().unwrap().remove(&model_id);
        let _ = self.app_handle.emit("model-download-complete", &model_id);
        info!("HF model {} downloaded", model_id);
        Ok(())
    }

    /// The language detector, when `model` needs it for Auto and it isn't on
    /// disk.
    fn missing_detector_for(
        &self,
        model: &ModelInfo,
    ) -> Option<&'static crate::catalog::LanguageDetectorFile> {
        let languages = crate::language_detection::policy::ModelLanguages {
            languages: &model.supported_languages,
            detects: model.supports_language_detection,
            streaming: model.supports_streaming,
        };
        if !crate::language_detection::policy::requires_language_detector(&languages)
            || self.language_detector_path().is_some()
        {
            return None;
        }
        crate::catalog::LANGUAGE_DETECTOR.as_ref()
    }

    /// Download the language detector on its own, tracked and cancelled under
    /// [`LANGUAGE_DETECTOR_ID`] like a model.
    pub async fn download_language_detector(&self) -> Result<()> {
        let cancel_token = CancellationToken::new();
        self.cancel_flags
            .lock()
            .unwrap()
            .insert(LANGUAGE_DETECTOR_ID.to_string(), cancel_token.clone());
        let fetched = self
            .fetch_language_detector(ProgressTarget::own(LANGUAGE_DETECTOR_ID), &cancel_token)
            .await;
        self.cancel_flags
            .lock()
            .unwrap()
            .remove(LANGUAGE_DETECTOR_ID);
        if fetched? {
            let _ = self
                .app_handle
                .emit("model-download-complete", LANGUAGE_DETECTOR_ID);
        }
        Ok(())
    }

    /// Fetch the language detector unless it's on disk. One transfer at a
    /// time: a second caller waits, then finds the file there. `Ok(false)` if
    /// cancelled.
    async fn fetch_language_detector(
        &self,
        progress: ProgressTarget,
        cancel_token: &CancellationToken,
    ) -> Result<bool> {
        let detector = crate::catalog::LANGUAGE_DETECTOR
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("The catalog names no language detector"))?;
        let _single = tokio::select! {
            guard = self.detector_download.lock() => guard,
            _ = cancel_token.cancelled() => return Ok(false),
        };
        if self.language_detector_path().is_some() {
            return Ok(true);
        }
        let transfer = FileTransfer {
            label: "the language detector",
            repo_id: &detector.repo_id,
            revision: &detector.revision,
            filename: &detector.filename,
            mirrors: crate::catalog::language_detector_mirrors(),
            progress,
        };
        self.fetch_hf_file(&transfer, cancel_token).await
    }

    /// Fetch one file from Hugging Face into the shared cache, retrying, then
    /// falling back to the mirrors into the models dir. `Ok(false)` if
    /// cancelled (partial kept for resume).
    async fn fetch_hf_file(
        &self,
        transfer: &FileTransfer<'_>,
        cancel_token: &CancellationToken,
    ) -> Result<bool> {
        let model_id = transfer.label;
        let repo_id = transfer.repo_id.to_string();
        let revision = transfer.revision.to_string();
        let filename = transfer.filename;
        let cancel_token = cancel_token.clone();

        info!(
            "Downloading {} from {}@{} ({})",
            model_id, repo_id, revision, filename
        );

        // hf-hub has no working internal retry (its retry knobs are hardcoded
        // to zero), so a single transient fault — dropped connection, a 429
        // from the resolve endpoint, a CDN blip — would otherwise fail the
        // whole download. Each attempt resumes from the `.sync.part`
        // committed-offset marker, so a retry only re-fetches what the failed
        // attempt hadn't finished.
        // Start moderately parallel for normal-network throughput, then stay
        // sequential after the first failure. Eight simultaneous connections
        // were all reset on an affected network in #1579, while one stream
        // succeeded; four is a less aggressive fast path, and every retry uses
        // the known-compatible request pattern.
        const ATTEMPT_STREAMS: [usize; 4] = [4, 1, 1, 1];
        let mut attempt: usize = 1;
        let hf_error = loop {
            let stream_count = ATTEMPT_STREAMS[attempt - 1];
            info!(
                "HF download attempt {}/{} for {} using {} concurrent stream(s)",
                attempt,
                ATTEMPT_STREAMS.len(),
                model_id,
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
                repo_id.clone(),
                RepoType::Model,
                revision.clone(),
            ));
            let progress =
                HfDownloadProgress::new(self.app_handle.clone(), transfer.progress.clone());

            // hf-hub has no internal timeouts, so a wedged connection would
            // otherwise hang this attempt forever and neither the retry loop
            // nor the mirror fallback would ever fire. The watchdog cancels a
            // per-attempt child token when progress goes stale; a user cancel
            // on the parent propagates through the same child. Each wake also
            // re-emits progress, so the reported rate decays through a lull.
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
            // hf-hub only observes its token inside the chunk loop — the
            // metadata/resolve request and cache lock run before it, so a hang
            // there would ignore the cancel entirely. Race the whole future
            // against the token: on cancel, grant a short grace so an attempt
            // that IS in the chunk loop can unwind gracefully (committing the
            // `.sync.part` resume offset), then drop the future outright,
            // which aborts whatever request it was wedged in.
            let mut download = std::pin::pin!(repo.download_with_progress_cancellable(
                filename,
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

            match result {
                Ok(_) => break None,
                Err(hf_hub::api::tokio::ApiError::Cancelled) if cancel_token.is_cancelled() => {
                    // User cancelled. hf-hub leaves the partially downloaded
                    // `.sync.part` in the shared cache, so a later attempt resumes
                    // instead of restarting. The guard resets is_downloading and
                    // drops the token; `cancel_download` already emitted
                    // `model-download-cancelled`.
                    info!("HF download cancelled for: {}", model_id);
                    return Ok(false);
                }
                Err(hf_hub::api::tokio::ApiError::Cancelled) => {
                    let err = anyhow::anyhow!(
                        "transfer stalled: no progress for {}s",
                        DOWNLOAD_STALL_TIMEOUT.as_secs()
                    );
                    // A parallel attempt may be what wedged the network. Give
                    // the connection pool a brief pause, then retry once using
                    // the known-compatible single-stream path. A sequential
                    // stall already cost DOWNLOAD_STALL_TIMEOUT, so further
                    // retries would likely just repeat it — use the mirror.
                    if stream_count == 1 || attempt >= ATTEMPT_STREAMS.len() {
                        break Some(err);
                    }
                    let delay = Duration::from_secs(1_u64 << attempt);
                    warn!(
                        "HF download attempt {}/{} stalled for {} using {} concurrent stream(s); retrying with {} stream(s) in {}s",
                        attempt,
                        ATTEMPT_STREAMS.len(),
                        model_id,
                        stream_count,
                        ATTEMPT_STREAMS[attempt],
                        delay.as_secs()
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel_token.cancelled() => {
                            info!("HF download cancelled for: {}", model_id);
                            return Ok(false);
                        }
                    }
                    attempt += 1;
                }
                Err(e) => {
                    // {:?} keeps the error source chain (reset vs TLS vs timeout);
                    // Display truncates it to "error sending request".
                    let err = anyhow::anyhow!("{:?}", e);
                    if attempt >= ATTEMPT_STREAMS.len() {
                        break Some(err);
                    }
                    let delay = Duration::from_secs(1_u64 << attempt);
                    warn!(
                        "HF download attempt {}/{} failed for {} using {} concurrent stream(s): {}; retrying with {} stream(s) in {}s",
                        attempt,
                        ATTEMPT_STREAMS.len(),
                        model_id,
                        stream_count,
                        err,
                        ATTEMPT_STREAMS[attempt],
                        delay.as_secs()
                    );
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel_token.cancelled() => {
                            info!("HF download cancelled for: {}", model_id);
                            return Ok(false);
                        }
                    }
                    attempt += 1;
                }
            }
        };

        if let Some(hf_error) = hf_error {
            // `attempt`, not the schedule length: a sequential stall breaks out early.
            error!(
                "HF download failed for {} after {} attempt(s): {:?}",
                model_id, attempt, hf_error
            );
            let mirrors = &transfer.mirrors;
            if mirrors.is_empty() {
                return Err(anyhow::anyhow!(
                    "Hugging Face download failed after {} attempt(s): {}",
                    attempt,
                    hf_error
                ));
            }
            let mut completed = false;
            for mirror in mirrors {
                info!("Falling back to mirror for {}: {}", model_id, mirror.url);
                match self
                    .download_from_mirror(
                        &transfer.progress,
                        filename,
                        mirror,
                        cancel_token.clone(),
                    )
                    .await
                {
                    Ok(true) => {
                        completed = true;
                        break;
                    }
                    Ok(false) => {
                        info!("Mirror download cancelled for: {}", model_id);
                        return Ok(false);
                    }
                    Err(e) => {
                        warn!(
                            "Mirror download failed for {} from {}: {:?}",
                            model_id, mirror.url, e
                        );
                    }
                }
            }
            if !completed {
                return Err(anyhow::anyhow!(
                    "Download failed from Hugging Face ({}) and {} mirror(s)",
                    hf_error,
                    mirrors.len()
                ));
            }
        }

        Ok(true)
    }

    /// Direct-HTTP download of a catalog model's file from a mirror into the
    /// models dir (HF-model resolution accepts that location too). Returns
    /// `Ok(true)` on completion, `Ok(false)` if cancelled (partial kept).
    async fn download_from_mirror(
        &self,
        progress: &ProgressTarget,
        filename: &str,
        mirror: &crate::catalog::MirrorFile,
        cancel_token: CancellationToken,
    ) -> Result<bool> {
        fs::create_dir_all(&self.models_dir)?;
        let model_path = self.models_dir.join(filename);
        let partial_path = self.models_dir.join(format!("{}.partial", filename));

        if model_path.exists() {
            return Ok(true);
        }

        match self
            .download_http_resumable(
                progress,
                &mirror.url,
                &partial_path,
                Some(mirror.size_bytes),
                Some(&mirror.sha256),
                &cancel_token,
            )
            .await?
        {
            HttpDownloadOutcome::Cancelled => Ok(false),
            HttpDownloadOutcome::Completed => {
                fs::rename(&partial_path, &model_path)?;
                info!(
                    "Mirror download of {} completed and verified ({:?})",
                    filename, model_path
                );
                Ok(true)
            }
        }
    }

    pub async fn download_model(&self, model_id: &str) -> Result<()> {
        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        let (url, expected_sha256) = match &model_info.source {
            ModelSource::Url { url, sha256 } => (url.clone(), sha256.clone()),
            ModelSource::HuggingFace { repo_id, revision } => {
                return self
                    .download_hf_model(&model_info, repo_id.clone(), revision.clone())
                    .await;
            }
            ModelSource::Local => {
                return Err(anyhow::anyhow!("No download source for model"));
            }
        };
        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        // Don't download if complete version already exists
        if model_path.exists() {
            // Clean up any partial file that might exist
            if partial_path.exists() {
                let _ = fs::remove_file(&partial_path);
            }
            self.update_download_status()?;
            return Ok(());
        }

        // Mark as downloading
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = true;
            }
        }

        // Create cancellation token for this download
        let cancel_token = CancellationToken::new();
        {
            let mut flags = self.cancel_flags.lock().unwrap();
            flags.insert(model_id.to_string(), cancel_token.clone());
        }

        // Guard ensures is_downloading and cancel_flags are cleaned up on every
        // error path. Disarmed only on success (which sets is_downloaded = true).
        let mut cleanup = DownloadCleanup {
            available_models: &self.available_models,
            cancel_flags: &self.cancel_flags,
            model_id: model_id.to_string(),
            disarmed: false,
        };

        // URL sources carry no authoritative size, so the helper falls back to
        // the server's content-length for progress and completeness checks.
        match self
            .download_http_resumable(
                &ProgressTarget::own(model_id),
                &url,
                &partial_path,
                None,
                expected_sha256.as_deref(),
                &cancel_token,
            )
            .await?
        {
            HttpDownloadOutcome::Cancelled => {
                info!("Download cancelled for: {}", model_id);
                // Keep partial file for resume functionality.
                // Guard handles is_downloading + cancel_flags cleanup on drop.
                return Ok(());
            }
            HttpDownloadOutcome::Completed => {}
        }

        fs::rename(&partial_path, &model_path)?;

        // Disarm the guard — success path does its own cleanup because it
        // additionally sets is_downloaded = true.
        cleanup.disarmed = true;
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
                model.is_downloaded = true;
                model.partial_size = 0;
            }
        }
        self.cancel_flags.lock().unwrap().remove(model_id);

        // Emit completion event
        let _ = self.app_handle.emit("model-download-complete", model_id);

        info!(
            "Successfully downloaded model {} to {:?}",
            model_id, model_path
        );

        Ok(())
    }

    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: delete_model called for: {}", model_id);

        let model_info = {
            let models = self.available_models.lock().unwrap();
            models.get(model_id).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        debug!("ModelManager: Found model info: {:?}", model_info);

        if let ModelSource::HuggingFace { repo_id, revision } = &model_info.source {
            // Only this entry's own file, every cached copy of it. Other
            // quants of the repo are separate entries: deleting the Q8_0
            // default must not take a user's Q5_K_M with it. Per product
            // decision this hard-removes from every HF cache, including one
            // shared with other apps. A file that can't be removed (in use on
            // Windows, permissions) fails the delete so the UI says so; copies
            // removed before it stay removed, and the entry keeps reading as
            // downloaded while any copy remains.
            let mut deleted = Self::delete_hf_cache_file(repo_id, revision, &model_info.filename)?;
            // Also remove a models-dir copy (mirror fallback / manual drop-in)
            // and any resumable partial next to it.
            for path in [
                self.models_dir.join(&model_info.filename),
                self.models_dir
                    .join(format!("{}.partial", &model_info.filename)),
            ] {
                if path.exists() {
                    info!("Deleting model file at: {:?}", path);
                    fs::remove_file(&path)?;
                    deleted = true;
                }
            }
            // Files already missing (e.g. removed outside Handy) is not a failure —
            // deleting is idempotent, so this still needs to fall through and clear
            // the stale "Downloaded" entry rather than erroring out and leaving it stuck.
            if !deleted {
                debug!(
                    "ModelManager: no HF cache/model files found on disk for {}; clearing stale entry",
                    model_id
                );
            }
            // Alternate quants exist only while their files are on disk.
            if Self::is_catalog_alternate_quant(repo_id, &model_info.filename) {
                self.available_models.lock().unwrap().remove(model_id);
            }
            self.update_download_status()?;
            let _ = self.app_handle.emit("model-deleted", model_id);
            return Ok(());
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));
        debug!("ModelManager: Model path: {:?}", model_path);
        debug!("ModelManager: Partial path: {:?}", partial_path);

        let mut deleted_something = false;

        // Delete complete model file if it exists
        if model_path.exists() {
            info!("Deleting model file at: {:?}", model_path);
            fs::remove_file(&model_path)?;
            info!("Model file deleted successfully");
            deleted_something = true;
        }

        // Delete partial file if it exists
        if partial_path.exists() {
            info!("Deleting partial file at: {:?}", partial_path);
            fs::remove_file(&partial_path)?;
            info!("Partial file deleted successfully");
            deleted_something = true;
        }

        // Files already missing (e.g. removed outside Handy) is not a failure —
        // deleting is idempotent, so this still needs to fall through and clear
        // the stale "Downloaded" entry rather than erroring out and leaving it stuck.
        if !deleted_something {
            debug!(
                "ModelManager: no files found on disk for {}; clearing stale entry",
                model_id
            );
        }

        // Custom models should be removed from the list entirely since they
        // have no download URL and can't be re-downloaded
        if model_info.is_custom {
            let mut models = self.available_models.lock().unwrap();
            models.remove(model_id);
            debug!("ModelManager: removed custom model from available models");
        } else {
            // Update download status (marks predefined models as not downloaded)
            self.update_download_status()?;
            debug!("ModelManager: download status updated");
        }

        // Emit event to notify UI
        let _ = self.app_handle.emit("model-deleted", model_id);

        Ok(())
    }

    /// Reconcile a model that was advertised as downloaded but whose path has
    /// since disappeared. This is deliberately narrower than a full rescan: it
    /// only runs after a definitive missing-path check and cannot disturb an
    /// unrelated in-flight download.
    fn mark_model_unavailable(&self, model_id: &str) {
        let removed = {
            let mut models = self.available_models.lock().unwrap();
            let should_remove = models
                .get(model_id)
                .is_some_and(Self::disappears_when_missing);
            if should_remove {
                models.remove(model_id);
                true
            } else if let Some(model) = models.get_mut(model_id) {
                model.is_downloaded = false;
                false
            } else {
                return;
            }
        };

        if removed {
            info!("Removing vanished discovered model '{}'", model_id);
        } else {
            info!("Marking model '{}' as unavailable on disk", model_id);
        }
        // Keep the persisted preference here: an already-loaded engine may
        // still be usable, and re-downloading this model should restore it.
        // Startup and explicit Rescan perform the broader selection cleanup.
        let _ = self.app_handle.emit("models-updated", ());
    }

    pub fn get_model_path(&self, model_id: &str) -> Result<PathBuf> {
        let model_info = self
            .get_model_info(model_id)
            .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

        if !model_info.is_downloaded {
            return Err(anyhow::anyhow!("Model not available: {}", model_id));
        }

        // Ensure we don't return partial files/directories
        if model_info.is_downloading {
            return Err(anyhow::anyhow!(
                "Model is currently downloading: {}",
                model_id
            ));
        }

        if let ModelSource::HuggingFace { repo_id, revision } = &model_info.source {
            if let Some(path) = hf_cached_path(repo_id, revision, &model_info.filename) {
                return Ok(path);
            }
            // Mirror-fallback download or manual drop-in in the models dir.
            // The complete file only ever appears after verification, so a
            // stale `.partial` alongside it is leftover noise, not a veto —
            // clear it rather than declaring the model missing.
            let local_path = self.models_dir.join(&model_info.filename);
            if local_path.exists() {
                let partial_path = self
                    .models_dir
                    .join(format!("{}.partial", &model_info.filename));
                if partial_path.exists() {
                    let _ = fs::remove_file(&partial_path);
                }
                return Ok(local_path);
            }
            self.mark_model_unavailable(model_id);
            return Err(anyhow::anyhow!(
                "Complete model file not found in HF cache or models dir: {}",
                model_id
            ));
        }

        let model_path = self.models_dir.join(&model_info.filename);
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));

        if !model_path.exists() {
            self.mark_model_unavailable(model_id);
            return Err(anyhow::anyhow!(
                "Complete model file not found: {}",
                model_id
            ));
        }
        if partial_path.exists() {
            return Err(anyhow::anyhow!("Model file is incomplete: {}", model_id));
        }
        Ok(model_path)
    }

    /// The spoken-language detector's file, when it is fully on disk: in the
    /// HF cache, or in the models dir (mirror fallback / manual drop-in).
    pub fn language_detector_path(&self) -> Option<PathBuf> {
        let detector = crate::catalog::LANGUAGE_DETECTOR.as_ref()?;
        hf_cached_path(&detector.repo_id, &detector.revision, &detector.filename).or_else(|| {
            let local = self.models_dir.join(&detector.filename);
            local.is_file().then_some(local)
        })
    }

    pub fn cancel_download(&self, model_id: &str) -> Result<()> {
        debug!("ModelManager: cancel_download called for: {}", model_id);

        // Trigger the cancellation token to stop the download. The HF path
        // aborts its in-flight chunk tasks and unwinds promptly; the URL path
        // observes it on the next chunk of its stream loop.
        {
            let flags = self.cancel_flags.lock().unwrap();
            if let Some(token) = flags.get(model_id) {
                token.cancel();
                info!("Cancellation token triggered for: {}", model_id);
            } else {
                warn!("No active download found for: {}", model_id);
            }
        }

        // Update state immediately for UI responsiveness
        {
            let mut models = self.available_models.lock().unwrap();
            if let Some(model) = models.get_mut(model_id) {
                model.is_downloading = false;
            }
        }

        // Update download status to reflect current state
        self.update_download_status()?;

        // Emit cancellation event so all UI components can clear their state
        let _ = self.app_handle.emit("model-download-cancelled", model_id);

        info!("Download cancellation initiated for: {}", model_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_then_model_report_as_one_bar() {
        // 45 MB detector, then a 700 MB model: one bar from 0 to 745 MB.
        let detector = ProgressTarget {
            model_id: "m".into(),
            offset: 0,
            total: Some(745),
        };
        let p = detector.progress(45, 45, None);
        assert_eq!((p.model_id.as_str(), p.downloaded, p.total), ("m", 45, 745));

        let model = ProgressTarget {
            model_id: "m".into(),
            offset: 45,
            total: None,
        };
        let p = model.progress(350, 700, Some(1.0));
        assert_eq!((p.downloaded, p.total), (395, 745));
    }
    use std::fs::File;
    use std::io::Write;
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

    #[test]
    fn test_hf_cached_path_falls_back_to_legacy_cache() {
        let active = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        let expected = add_cached_model(
            legacy.path(),
            "handy-computer/test-model",
            "old-revision",
            "model.gguf",
        );
        let caches = vec![
            Cache::new(active.path().to_path_buf()),
            Cache::new(legacy.path().to_path_buf()),
        ];

        assert_eq!(
            hf_cached_path_in(
                &caches,
                "handy-computer/test-model",
                "old-revision",
                "model.gguf"
            ),
            Some(expected)
        );
    }

    #[test]
    fn test_hf_cached_path_prefers_active_cache() {
        let active = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        let expected = add_cached_model(
            active.path(),
            "handy-computer/test-model",
            "revision",
            "model.gguf",
        );
        add_cached_model(
            legacy.path(),
            "handy-computer/test-model",
            "revision",
            "model.gguf",
        );
        let caches = vec![
            Cache::new(active.path().to_path_buf()),
            Cache::new(legacy.path().to_path_buf()),
        ];

        assert_eq!(
            hf_cached_path_in(
                &caches,
                "handy-computer/test-model",
                "revision",
                "model.gguf"
            ),
            Some(expected)
        );
    }

    /// A file in snapshot `commit` with no ref pointing at it — how
    /// huggingface_hub leaves a download made at a commit revision, and how an
    /// earlier pin looks once the catalog moves on.
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

    fn catalog_model(id_prefix: &str) -> &'static ModelDescriptor {
        crate::catalog::CATALOG
            .iter()
            .find(|d| d.id.starts_with(id_prefix))
            .unwrap_or_else(|| panic!("catalog has {id_prefix}"))
    }

    fn hf_source(desc: &ModelDescriptor) -> (&str, &str) {
        match &desc.source {
            ModelSource::HuggingFace { repo_id, revision } => (repo_id, revision),
            _ => panic!("catalog descriptors are HF-sourced"),
        }
    }

    #[test]
    fn test_every_retired_model_has_a_downloadable_replacement() {
        // A missing replacement would silently hide a row in the migration UI.
        let mut registry = HashMap::new();
        ModelManager::seed_catalog_models(&mut registry);
        for retired in RETIRED_ONNX_MODELS {
            let replacement = registry
                .values()
                .find(|info| {
                    matches!(&info.source, ModelSource::HuggingFace { repo_id, .. }
                        if repo_id == retired.replacement_repo)
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{}: replacement {} is not offered",
                        retired.id, retired.replacement_repo
                    )
                });
            assert!(
                !crate::catalog::mirror_fallbacks(&replacement.id).is_empty(),
                "{}: replacement {} has no mirror fallback",
                retired.id,
                replacement.id
            );
        }
    }

    #[test]
    fn test_cache_lookup_prefers_pin_then_main_then_any_snapshot() {
        let cache = TempDir::new().unwrap();
        let repo = "someone/model";
        let revision = "a".repeat(40);
        let old = add_unreferenced_snapshot_file(cache.path(), repo, "unknown", "model.gguf");
        let main = add_cached_model(cache.path(), repo, "main", "model.gguf");
        let pin = add_unreferenced_snapshot_file(cache.path(), repo, &revision, "model.gguf");
        let caches = vec![Cache::new(cache.path().to_path_buf())];

        assert_eq!(
            hf_cached_paths_in(&caches, repo, &revision, "model.gguf"),
            vec![pin.clone(), main.clone(), old.clone()]
        );
        for expected in [pin, main, old] {
            assert_eq!(
                hf_cached_path_in(&caches, repo, &revision, "model.gguf"),
                Some(expected.clone())
            );
            fs::remove_file(expected).unwrap();
        }
        assert!(hf_cached_path_in(&caches, repo, &revision, "model.gguf").is_none());
    }

    #[test]
    fn test_old_q5_is_discovered_and_survives_deleting_all_q8_copies() {
        let desc = catalog_model("handy-computer/whisper-large-v3-gguf/");
        let (repo, revision) = hf_source(desc);
        let q5 = desc.files.iter().find(|f| f.quant == "Q5_K_M").unwrap();
        let q8 = desc.default_file().unwrap();
        assert_eq!(q8.quant, "Q8_0");

        let cache = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        let old = add_unreferenced_snapshot_file(cache.path(), repo, "unknown", &q5.filename);
        let caches = vec![
            Cache::new(cache.path().to_path_buf()),
            Cache::new(legacy.path().to_path_buf()),
        ];
        for cache in &caches {
            for rev in [revision, "main", "unknown"] {
                add_unreferenced_snapshot_file(cache.path(), repo, rev, &q8.filename);
            }
        }
        let readme = old.parent().unwrap().join("README.md");
        fs::write(&readme, "unrelated file").unwrap();

        let mut models = HashMap::new();
        ModelManager::seed_catalog_models(&mut models);
        ModelManager::discover_hf_cache_models_in(cache.path(), &mut models);
        let info = &models[&format!("{repo}/{}", q5.filename)];
        assert!(info.is_downloaded);
        assert_eq!(info.name, format!("{} (Q5_K_M)", desc.name));
        assert!(models.contains_key(&desc.id));

        assert!(
            ModelManager::delete_hf_cache_file_in(&caches, repo, revision, &q8.filename).unwrap()
        );
        assert!(hf_cached_paths_in(&caches, repo, revision, &q8.filename).is_empty());
        assert_eq!(
            hf_cached_path_in(&caches, repo, revision, &q5.filename),
            Some(old)
        );
        assert!(readme.exists());
    }

    /// An HF-style cached file: a snapshot symlink to a blob named after its
    /// content, so copies at several revisions can share one blob.
    #[cfg(unix)]
    fn add_cached_blob(cache: &Path, repo_id: &str, commit: &str, filename: &str) -> PathBuf {
        let repo_dir = cache.join(format!("models--{}", repo_id.replace('/', "--")));
        let blob = repo_dir.join("blobs").join("b".repeat(64));
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        fs::write(&blob, b"model").unwrap();
        let pointer = repo_dir.join("snapshots").join(commit).join(filename);
        fs::create_dir_all(pointer.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&blob, &pointer).unwrap();
        pointer
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_preserves_other_filenames_sharing_a_blob() {
        let cache = TempDir::new().unwrap();
        let repo = "someone/model";
        let first = add_cached_blob(cache.path(), repo, "first", "model.gguf");
        let second = add_cached_blob(cache.path(), repo, "second", "model.gguf");
        let other = add_cached_blob(cache.path(), repo, "second", "other.gguf");
        let blob = fs::canonicalize(&other).unwrap();
        let caches = vec![Cache::new(cache.path().to_path_buf())];

        assert!(
            ModelManager::delete_hf_cache_file_in(&caches, repo, "main", "model.gguf").unwrap()
        );
        assert!(fs::symlink_metadata(first).is_err());
        assert!(fs::symlink_metadata(second).is_err());
        assert!(other.is_file());
        assert!(
            ModelManager::delete_hf_cache_file_in(&caches, repo, "main", "other.gguf").unwrap()
        );
        assert!(!blob.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_reports_a_file_it_cant_remove() {
        // A file that can't be removed (in use on Windows, permissions) must
        // fail the delete, not report success while the model stays on disk.
        use std::os::unix::fs::PermissionsExt;

        let desc = catalog_model("handy-computer/canary-180m-flash-gguf/");
        let (repo_id, revision) = hf_source(desc);
        let filename = &desc.default_file().unwrap().filename;
        let caches_in = |cache: &TempDir| vec![Cache::new(cache.path().to_path_buf())];
        let lock =
            |dir: &Path, mode| fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();

        // Plain file (hf-hub's no-symlink layout) in a read-only snapshot.
        let cache = TempDir::new().unwrap();
        let file = add_cached_model(cache.path(), repo_id, revision, filename);
        lock(file.parent().unwrap(), 0o555);
        let result =
            ModelManager::delete_hf_cache_file_in(&caches_in(&cache), repo_id, revision, filename);
        lock(file.parent().unwrap(), 0o755);
        assert!(result.is_err(), "undeletable file read as {result:?}");
        assert!(file.exists());

        // Symlinked blob in a read-only blobs dir: the pointer stays, so the
        // entry still (correctly) reads as downloaded.
        let cache = TempDir::new().unwrap();
        let pointer = add_cached_blob(cache.path(), repo_id, revision, filename);
        let blobs = hf_repo_dir(&pointer).unwrap().join("blobs");
        lock(&blobs, 0o555);
        let result =
            ModelManager::delete_hf_cache_file_in(&caches_in(&cache), repo_id, revision, filename);
        lock(&blobs, 0o755);
        assert!(result.is_err(), "undeletable blob read as {result:?}");
        assert!(pointer.exists(), "pointer removed while its blob stayed");
    }

    #[test]
    fn test_effective_language_falls_back_to_canonical_chinese() {
        let languages = vec!["zh-Hant".to_string()];

        assert_eq!(effective_language("auto", &[], &languages, false), "zh");
    }

    #[test]
    fn test_effective_language_falls_back_to_the_one_preferred_language() {
        let languages = vec!["en".to_string(), "es-ES".to_string(), "de".to_string()];
        let spanish = vec!["es".to_string()];
        let several = vec!["es".to_string(), "de".to_string()];
        let unheard = vec!["ja".to_string()];

        // A model that must be told: the one language they speak.
        assert_eq!(
            effective_language("auto", &spanish, &languages, false),
            "es-ES"
        );
        // Several, or one the model doesn't hear: the English fallback.
        assert_eq!(
            effective_language("auto", &several, &languages, false),
            "en"
        );
        assert_eq!(
            effective_language("auto", &unheard, &languages, false),
            "en"
        );
        // A model that detects stays on auto; a usable intent still wins.
        assert_eq!(
            effective_language("auto", &spanish, &languages, true),
            "auto"
        );
        assert_eq!(effective_language("de", &spanish, &languages, false), "de");
    }

    #[test]
    fn test_effective_language_resolves_bare_intent_to_concrete_locale() {
        // A model advertising full BCP-47 locales (e.g. Nemotron Streaming):
        // a bare intent must resolve to the exact code the engine expects, not
        // be handed back as the bare form the prompt table may not contain.
        let languages = vec![
            "en-US".to_string(),
            "en-GB".to_string(),
            "es-ES".to_string(),
            "zh-CN".to_string(),
            "ja-JP".to_string(),
        ];

        assert_eq!(effective_language("en", &[], &languages, true), "en-US");
        assert_eq!(effective_language("es", &[], &languages, true), "es-ES");
        // `zh`/`ja` have no bare entry in this model's table; resolve to locale.
        assert_eq!(effective_language("zh", &[], &languages, true), "zh-CN");
        assert_eq!(effective_language("ja", &[], &languages, true), "ja-JP");
        // An unsupported intent still auto-detects when the model can.
        assert_eq!(effective_language("fr", &[], &languages, true), "auto");
    }

    #[test]
    fn test_effective_language_preserves_intent_across_model_code_variants() {
        assert_eq!(
            effective_language("no", &[], &["nb".to_string()], true),
            "nb"
        );
        assert_eq!(
            effective_language("nb", &[], &["no".to_string()], true),
            "no"
        );
        assert_eq!(
            effective_language("tl", &[], &["fil".to_string()], true),
            "fil"
        );
        assert_eq!(
            effective_language("fil", &[], &["tl".to_string()], true),
            "tl"
        );
        assert_eq!(
            effective_language("jv", &[], &["jw".to_string()], true),
            "jw"
        );
        assert_eq!(
            effective_language("he", &[], &["iw".to_string()], true),
            "iw"
        );
    }

    #[test]
    fn test_effective_language_prefers_exact_norwegian_code() {
        let languages = vec!["no".to_string(), "nb".to_string()];

        assert_eq!(effective_language("nb", &[], &languages, true), "nb");
        assert_eq!(effective_language("no", &[], &languages, true), "no");
    }

    #[test]
    fn test_canonicalize_supported_languages_collapses_chinese_scripts() {
        let languages = canonicalize_supported_languages(
            vec!["en", "zh", "zh-Hans", "zh-Hant", "yue"]
                .into_iter()
                .map(String::from)
                .collect(),
        );

        assert_eq!(languages, vec!["en", "zh", "yue"]);
    }

    fn build_test_gguf_string_metadata(kvs: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GGUF");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
        for (key, value) in kvs {
            push_gguf_str(&mut out, key);
            out.extend_from_slice(&8u32.to_le_bytes()); // GGUF string value type.
            push_gguf_str(&mut out, value);
        }
        out
    }

    #[test]
    fn test_discover_custom_transcribe_models() {
        let temp_dir = TempDir::new().unwrap();
        let models_dir = temp_dir.path().to_path_buf();

        // Create test .bin files
        let mut custom_file = File::create(models_dir.join("my-custom-model.bin")).unwrap();
        custom_file.write_all(b"fake model data").unwrap();

        let mut another_file = File::create(models_dir.join("whisper_medical_v2.bin")).unwrap();
        another_file.write_all(b"another fake model").unwrap();

        // Custom GGUF model (also supported by transcribe-cpp)
        let mut gguf_file = File::create(models_dir.join("my-gguf-model.gguf")).unwrap();
        gguf_file
            .write_all(&build_test_gguf_string_metadata(&[(
                "general.name",
                "Friendly GGUF Name",
            )]))
            .unwrap();

        // Create files that should be ignored
        File::create(models_dir.join(".hidden-model.bin")).unwrap(); // Hidden file
        File::create(models_dir.join("readme.txt")).unwrap(); // Non-model file
        File::create(models_dir.join("ggml-small.bin")).unwrap(); // Predefined filename
        File::create(models_dir.join("download.bin.partial")).unwrap(); // Partial download
        fs::create_dir(models_dir.join("some-directory.bin")).unwrap(); // Directory

        // Set up available_models with a predefined Whisper model
        let mut models = HashMap::new();
        models.insert(
            "small".to_string(),
            ModelInfo {
                id: "small".to_string(),
                name: "Whisper Small".to_string(),
                description: "Test".to_string(),
                filename: "ggml-small.bin".to_string(),
                source: ModelSource::Url {
                    url: "https://example.com".to_string(),
                    sha256: None,
                },
                size_mb: 100,
                is_downloaded: false,
                is_downloading: false,
                partial_size: 0,
                accuracy_score: 0.5,
                speed_score: 0.5,
                supports_translation: true,
                is_recommended: false,
                supported_languages: vec!["en".to_string()],
                supports_language_selection: true,
                is_custom: false,
                supports_streaming: false,
                // Legacy entry: preserve the historical "Auto offered" behavior.
                // (Catalog GGUFs and on-disk probes derive this from metadata.)
                supports_language_detection: true,
            },
        );

        // Discover custom models
        ModelManager::discover_custom_transcribe_models(&models_dir, &mut models).unwrap();

        // Should have discovered 2 custom models (my-custom-model and whisper_medical_v2)
        assert!(models.contains_key("my-custom-model"));
        assert!(models.contains_key("whisper_medical_v2"));

        // Verify custom model properties
        let custom = models.get("my-custom-model").unwrap();
        assert_eq!(custom.name, "My Custom Model");
        assert_eq!(custom.filename, "my-custom-model.bin");
        assert!(matches!(custom.source, ModelSource::Local)); // Custom models have no remote source
        assert!(custom.is_downloaded);
        assert!(custom.is_custom);
        assert!(ModelManager::disappears_when_missing(custom));
        assert_eq!(custom.accuracy_score, 0.0);
        assert_eq!(custom.speed_score, 0.0);
        assert!(custom.supported_languages.is_empty());

        // Verify underscore handling
        let medical = models.get("whisper_medical_v2").unwrap();
        assert_eq!(medical.name, "Whisper Medical V2");

        // Verify .gguf models are discovered too (extension stripped for the id)
        assert!(models.contains_key("my-gguf-model"));
        let gguf = models.get("my-gguf-model").unwrap();
        assert_eq!(gguf.filename, "my-gguf-model.gguf");
        assert_eq!(gguf.name, "Friendly GGUF Name");
        assert!(gguf.is_custom);

        // Should NOT have discovered hidden, non-model, predefined, partial, or directories
        assert!(!models.contains_key(".hidden-model"));
        assert!(!models.contains_key("readme"));
        assert!(!models.contains_key("download.bin"));
        assert!(!models.contains_key("some-directory"));
    }

    #[test]
    fn test_discover_custom_models_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let models_dir = temp_dir.path().to_path_buf();

        let mut models = HashMap::new();
        let count_before = models.len();

        ModelManager::discover_custom_transcribe_models(&models_dir, &mut models).unwrap();

        // No new models should be added
        assert_eq!(models.len(), count_before);
    }

    #[test]
    fn a_mirrored_language_detector_is_not_a_custom_model() {
        let temp_dir = TempDir::new().unwrap();
        let detector = crate::catalog::LANGUAGE_DETECTOR.as_ref().unwrap();
        fs::write(temp_dir.path().join(&detector.filename), b"gguf").unwrap();

        let mut models = HashMap::new();
        ModelManager::discover_custom_transcribe_models(temp_dir.path(), &mut models).unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn test_discover_custom_models_nonexistent_dir() {
        let models_dir = PathBuf::from("/nonexistent/path/that/does/not/exist");

        let mut models = HashMap::new();
        let count_before = models.len();

        // Should not error, just return Ok
        let result = ModelManager::discover_custom_transcribe_models(&models_dir, &mut models);
        assert!(result.is_ok());
        assert_eq!(models.len(), count_before);
    }

    #[test]
    fn test_catalog_quant_rendering() {
        let desc = ModelDescriptor {
            id: "org/repo/model-Q8_0.gguf".to_string(),
            source: ModelSource::HuggingFace {
                repo_id: "org/repo".to_string(),
                revision: "main".to_string(),
            },
            name: "Model".to_string(),
            description: "desc".to_string(),
            caps: CapabilityProbe::default(),
            files: vec![
                QuantFile {
                    filename: "model-Q4_K_M.gguf".to_string(),
                    quant: "Q4_K_M".to_string(),
                    size_bytes: 1,
                },
                QuantFile {
                    filename: "model-Q8_0.gguf".to_string(),
                    quant: "Q8_0".to_string(),
                    size_bytes: 2,
                },
            ],
            default_quant: Some("Q8_0".to_string()),
            speed_score: 0.5,
            accuracy_score: 0.5,
            recommended_rank: None,
            recommended: true,
        };
        let status = DiskStatus::default();

        // Default quant: plain name, badge intact.
        let default_info = desc.to_model_info(&status);
        assert_eq!(default_info.name, "Model");
        assert!(default_info.is_recommended);

        // Alternate quant: suffixed name, own id, no badge.
        let alt_info = desc.to_model_info_for_file(&desc.files[0], &status);
        assert_eq!(alt_info.id, "org/repo/model-Q4_K_M.gguf");
        assert_eq!(alt_info.name, "Model (Q4_K_M)");
        assert_eq!(alt_info.filename, "model-Q4_K_M.gguf");
        assert!(!alt_info.is_recommended);
        assert!(!alt_info.is_custom);

        // Catalog defaults stay visible for re-download when their file is gone.
        assert!(!ModelManager::disappears_when_missing(&default_info));

        let mut selection_models = HashMap::new();
        selection_models.insert(default_info.id.clone(), default_info.clone());
        assert!(!ModelManager::selected_model_is_available(
            &selection_models,
            &default_info.id
        ));
        selection_models
            .get_mut(&default_info.id)
            .unwrap()
            .is_downloaded = true;
        assert!(ModelManager::selected_model_is_available(
            &selection_models,
            &default_info.id
        ));

        // The default rendered through the per-file path matches to_model_info,
        // so seeded entries and discovered defaults can never diverge.
        let same = desc.to_model_info_for_file(&desc.files[1], &status);
        assert_eq!(same.id, default_info.id);
        assert_eq!(same.name, default_info.name);
        assert_eq!(same.is_recommended, default_info.is_recommended);
    }

    #[test]
    fn test_discover_catalog_alternate_quant_in_models_dir() {
        // A real catalog model with more than one quant.
        let desc = crate::catalog::CATALOG
            .iter()
            .find(|d| d.files.len() > 1)
            .expect("catalog offers multi-quant models");
        let default_filename = default_quant_file(&desc.files, desc.default_quant.as_deref())
            .unwrap()
            .filename
            .clone();
        let alt = desc
            .files
            .iter()
            .find(|f| f.filename != default_filename)
            .unwrap();

        let temp_dir = TempDir::new().unwrap();
        // Content is never probed for catalog-matched files, so empty files do.
        fs::write(temp_dir.path().join(&alt.filename), b"").unwrap();
        fs::write(temp_dir.path().join(&default_filename), b"").unwrap();

        let mut models = HashMap::new();
        ModelManager::seed_catalog_models(&mut models);
        let seeded = models.len();
        ModelManager::discover_custom_transcribe_models(temp_dir.path(), &mut models).unwrap();

        // The alternate quant surfaces as a catalog-grade HF entry…
        let ModelSource::HuggingFace { repo_id, .. } = &desc.source else {
            panic!("catalog descriptors are HF-sourced");
        };
        let alt_id = format!("{}/{}", repo_id, alt.filename);
        let info = models.get(&alt_id).expect("alternate quant discovered");
        assert_eq!(info.name, format!("{} ({})", desc.name, alt.quant));
        assert_eq!(info.description, desc.description);
        assert!(info.is_downloaded);
        assert!(!info.is_custom);
        assert!(matches!(info.source, ModelSource::HuggingFace { .. }));
        assert!(ModelManager::disappears_when_missing(info));

        // …while the default-quant file dedups onto its seeded entry: exactly
        // one new id, and no filename-stem custom entries for either file.
        assert_eq!(models.len(), seeded + 1);
        assert!(!models.contains_key(alt.filename.trim_end_matches(".gguf")));
        assert!(!models.contains_key(default_filename.trim_end_matches(".gguf")));
    }

    fn push_gguf_str(out: &mut Vec<u8>, val: &str) {
        out.extend_from_slice(&(val.len() as u64).to_le_bytes());
        out.extend_from_slice(val.as_bytes());
    }

    fn write_synthetic_gguf(path: &Path, arch: &str, languages: &[&str]) {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&0x4655_4747u32.to_le_bytes()); // magic "GGUF"
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&2u64.to_le_bytes()); // kv_count
                                                    // general.architecture : string
        push_gguf_str(&mut out, "general.architecture");
        out.extend_from_slice(&8u32.to_le_bytes()); // STRING
        push_gguf_str(&mut out, arch);
        // general.languages : array<string>
        push_gguf_str(&mut out, "general.languages");
        out.extend_from_slice(&9u32.to_le_bytes()); // ARRAY
        out.extend_from_slice(&8u32.to_le_bytes()); // elem STRING
        out.extend_from_slice(&(languages.len() as u64).to_le_bytes());
        for l in languages {
            push_gguf_str(&mut out, l);
        }
        fs::write(path, out).unwrap();
    }

    #[test]
    fn test_discover_hf_cache_keeps_the_cache_folder_casing() {
        // HF repo ids are case-insensitive, so another tool can cache a catalog
        // repo under other casing. On a case-sensitive filesystem only that
        // casing finds the file again, so it must not take the catalog's.
        let filename = "SenseVoiceSmall-Q8_0.gguf";
        let folder_repo = "handy-computer/sensevoicesmall-gguf";
        assert!(crate::catalog::file_in_catalog(
            filename,
            Some("handy-computer/SenseVoiceSmall-gguf")
        )
        .is_some());

        let cache = TempDir::new().unwrap();
        let path = add_cached_model(cache.path(), folder_repo, "main", filename);
        write_synthetic_gguf(&path, "sensevoice", &["en", "zh"]);

        let mut models = HashMap::new();
        ModelManager::discover_hf_cache_models_in(cache.path(), &mut models);

        let info = models
            .values()
            .find(|m| m.filename == filename)
            .expect("cached file is listed");
        assert!(
            matches!(&info.source, ModelSource::HuggingFace { repo_id, .. } if repo_id == folder_repo),
            "listed under {:?}, not the cache folder's casing",
            info.source
        );
    }

    #[test]
    fn test_discover_hf_cache_models_filters_by_arch() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // ASR repo: a whisper gguf -> should be discovered.
        let repo = root.join("models--handy-computer--whisper-test");
        fs::create_dir_all(repo.join("snapshots").join("abc123")).unwrap();
        fs::create_dir_all(repo.join("refs")).unwrap();
        fs::write(repo.join("refs").join("main"), "abc123").unwrap();
        write_synthetic_gguf(
            &repo
                .join("snapshots")
                .join("abc123")
                .join("whisper-q8.gguf"),
            "whisper",
            &["en", "de"],
        );

        // Non-ASR (llama) gguf -> must be ignored.
        let repo2 = root.join("models--someone--llama-7b");
        fs::create_dir_all(repo2.join("snapshots").join("def456")).unwrap();
        fs::create_dir_all(repo2.join("refs")).unwrap();
        fs::write(repo2.join("refs").join("main"), "def456").unwrap();
        write_synthetic_gguf(
            &repo2.join("snapshots").join("def456").join("llama-q8.gguf"),
            "llama",
            &[],
        );

        let mut models = HashMap::new();
        ModelManager::discover_hf_cache_models_in(root, &mut models);

        let id = "handy-computer/whisper-test/whisper-q8.gguf";
        let m = models.get(id).expect("whisper gguf should be discovered");
        assert!(m.is_downloaded);
        assert!(
            matches!(&m.source, ModelSource::HuggingFace { repo_id, revision }
            if repo_id == "handy-computer/whisper-test" && revision == "abc123")
        );
        assert_eq!(
            m.supported_languages,
            vec!["en".to_string(), "de".to_string()]
        );
        assert!(
            !models.contains_key("someone/llama-7b/llama-q8.gguf"),
            "non-ASR gguf must be ignored"
        );
    }
}
