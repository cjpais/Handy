use super::model_capabilities::{
    CapabilityProbe, CapabilityProber, Compatibility, GgufHeaderProber,
};
use crate::settings::{get_settings, write_settings};
use anyhow::Result;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use hf_hub::api::tokio::{ApiBuilder, CancellationToken, Progress};
use hf_hub::{Cache, Repo, RepoType};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tar::Archive;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum EngineType {
    /// Any GGML/GGUF model loaded through transcribe-cpp (Whisper, Parakeet,
    /// Voxtral, Qwen3-ASR, Nemotron, …). The architecture is auto-detected from
    /// the file, so this one variant covers the whole transcribe-cpp family.
    TranscribeCpp,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
    Cohere,
}

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
    pub is_directory: bool,
    pub engine_type: EngineType,
    pub accuracy_score: f32,        // 0.0 to 1.0, higher is more accurate
    pub speed_score: f32,           // 0.0 to 1.0, higher is faster
    pub supports_translation: bool, // Whether the model supports translating to English
    pub is_recommended: bool,       // Whether this is the recommended model for new users
    pub supported_languages: Vec<String>, // Languages this model can transcribe
    pub supports_language_selection: bool, // Whether the user can explicitly pick a language
    pub is_custom: bool,            // Whether this is a user-provided custom model
    pub supports_streaming: bool, // Whether this model supports live streaming preview (transcribe-cpp)
    pub supports_language_detection: bool, // Whether the model can auto-detect language (gates the "Auto" option)
    /// Retired legacy models (frozen in `legacy.json`): kept working for users
    /// who already downloaded them, hidden from new downloads in the UI.
    #[serde(default)]
    pub deprecated: bool,
}

const CHINESE_LANGUAGE_CODE: &str = "zh";

fn recognition_language(language: &str) -> &str {
    match language {
        "zh-Hans" | "zh-Hant" => CHINESE_LANGUAGE_CODE,
        other => other,
    }
}

/// The base code Handy matches a language *intent* on: a tag's primary subtag,
/// with any BCP-47 region or script suffix dropped (`en-US` → `en`, `zh-CN` →
/// `zh`, `zh-Hant` → `zh`). Bare and three-letter codes (`haw`) pass through
/// unchanged. Lets a bare intent (`en`) match a model that advertises full
/// locales (`en-US`) without discarding the real code the engine needs.
fn base_language(language: &str) -> &str {
    match language.split_once('-') {
        Some((base, _)) => base,
        None => language,
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

/// One downloadable quantization of a model. Mirrors a `files[]` entry in
/// `catalog.json`, so it deserializes straight from the catalog.
#[derive(Debug, Clone, Deserialize)]
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

/// The spec of a bundled model: everything in `catalog.json` / `legacy.json`
/// normalised into one shape, rendered into the frontend-facing [`ModelInfo`]
/// via [`ModelDescriptor::to_model_info`] by combining it with a [`DiskStatus`].
/// (The on-disk scans still build `ModelInfo` directly.)
#[derive(Debug, Clone)]
pub struct ModelDescriptor {
    pub id: String,
    pub source: ModelSource,
    pub name: String,
    pub description: String,
    pub engine_type: EngineType,
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
    /// The artifact is an extracted directory (legacy ONNX bundles), not a file.
    pub is_directory: bool,
    /// Retired legacy model — see [`ModelInfo::deprecated`].
    pub deprecated: bool,
    /// Explicit override for whether the user can pick a language; `None`
    /// derives it from the language count. Needed because some legacy engines
    /// (e.g. ONNX Parakeet V3) are multilingual but take no language parameter.
    pub supports_language_selection: Option<bool>,
}

impl ModelDescriptor {
    /// The quant we surface for download/size: the declared default, else the
    /// first file.
    fn default_file(&self) -> Option<&QuantFile> {
        default_quant_file(&self.files, self.default_quant.as_deref())
    }

    /// Render the frontend-facing [`ModelInfo`] by combining this spec with live
    /// disk `status`.
    pub fn to_model_info(&self, status: &DiskStatus) -> ModelInfo {
        let file = self.default_file();
        let languages =
            canonicalize_supported_languages(self.caps.languages.clone().unwrap_or_default());
        ModelInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            filename: file.map(|f| f.filename.clone()).unwrap_or_default(),
            source: self.source.clone(),
            size_mb: file.map(|f| f.size_bytes / (1024 * 1024)).unwrap_or(0),
            is_downloaded: status.is_downloaded,
            is_downloading: status.is_downloading,
            partial_size: status.partial_size,
            is_directory: self.is_directory,
            engine_type: self.engine_type.clone(),
            accuracy_score: self.accuracy_score,
            speed_score: self.speed_score,
            supports_translation: self.caps.supports_translation.unwrap_or(false),
            is_recommended: self.recommended,
            supports_language_selection: self
                .supports_language_selection
                .unwrap_or(languages.len() > 1),
            supported_languages: languages,
            // Descriptor models are always bundled downloads, never user-dropped
            // custom files (those bypass the descriptor and set this directly).
            is_custom: false,
            supports_streaming: self.caps.supports_streaming.unwrap_or(false),
            supports_language_detection: self.caps.supports_language_detect.unwrap_or(false),
            deprecated: self.deprecated,
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
/// BCP-47 locales. Chinese *script* intents (`zh-Hans`/`zh-Hant`) are the sole
/// exception: they pass through unchanged so the downstream Simplified /
/// Traditional output conversion still fires (the engine path collapses them to
/// a plain Chinese code separately).
pub fn effective_language(
    intent: &str,
    supported_languages: &[String],
    supports_language_detection: bool,
) -> String {
    if supported_languages.is_empty() {
        return intent.to_string();
    }

    if intent != "auto" {
        if let Some(code) = supported_languages
            .iter()
            .find(|language| base_language(language) == base_language(intent))
        {
            if intent == "zh-Hans" || intent == "zh-Hant" {
                return intent.to_string();
            }
            return code.clone();
        }
    }

    if supports_language_detection {
        return "auto".to_string();
    }

    // Model can't auto-detect and the intent isn't usable: fall back to a
    // concrete language (prefer English) so we never hand the engine "auto".
    if let Some(en) = supported_languages
        .iter()
        .find(|language| base_language(language) == "en")
    {
        return en.clone();
    }
    recognition_language(&supported_languages[0]).to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f64,
}

/// Resolve a Hugging Face model file in the shared HF cache, if already present.
/// Uses hf-hub's stock location (HF_HOME or ~/.cache/huggingface/hub) so
/// downloads are shared with other tools.
fn hf_cached_path(repo_id: &str, revision: &str, filename: &str) -> Option<PathBuf> {
    Cache::from_env()
        .repo(Repo::with_revision(
            repo_id.to_string(),
            RepoType::Model,
            revision.to_string(),
        ))
        .get(filename)
}

/// Log when an HF cache entry is in a *broken* state — the "downloaded but
/// invisible" failure that otherwise stays silent. Deliberately narrow to
/// avoid false alarms: a missing repo dir is simply not-downloaded, and a
/// resolved ref without our file just means a sibling file (e.g. another
/// quant) from the same repo was downloaded. What we flag:
/// - repo dir present but the ref unreadable (interrupted download), or
/// - the snapshot entry exists but doesn't resolve (dangling symlink).
///
/// A `.sync.part` under `blobs/` is exempt from the first case: hf-hub writes
/// the ref only after the blob completes, so a repo dir holding a `.sync.part`
/// and no ref is the *healthy* state of a cancelled or in-flight transfer —
/// the next download resumes it rather than needing manual cleanup.
fn warn_if_hf_cache_entry_broken(repo_id: &str, revision: &str, filename: &str) {
    let repo = Repo::with_revision(repo_id.to_string(), RepoType::Model, revision.to_string());
    let repo_dir = Cache::from_env().path().join(repo.folder_name());
    if !repo_dir.exists() {
        return;
    }
    if let Ok(blobs) = fs::read_dir(repo_dir.join("blobs")) {
        if blobs.flatten().any(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.ends_with(".sync.part"))
        }) {
            return;
        }
    }
    let Ok(commit) = fs::read_to_string(repo_dir.join("refs").join(revision)) else {
        warn!(
            "HF cache dir {:?} exists but has no usable ref for revision {}; \
             likely an interrupted download. Re-download the model or delete that directory.",
            repo_dir, revision
        );
        return;
    };
    let pointer = repo_dir
        .join("snapshots")
        .join(commit.trim())
        .join(filename);
    // symlink_metadata succeeding while the followed path doesn't exist is
    // exactly a dangling symlink.
    if pointer.symlink_metadata().is_ok() && !pointer.exists() {
        warn!(
            "HF cache entry {:?} is a dangling symlink (its blob is missing); \
             re-download the model or delete {:?}.",
            pointer, repo_dir
        );
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

/// Bridges hf-hub's async download progress to Handy's `model-download-progress`
/// event. hf-hub clones the reporter, so shared state lives behind an `Arc`.
#[derive(Clone)]
struct HfDownloadProgress {
    app_handle: AppHandle,
    model_id: String,
    state: Arc<Mutex<HfProgressState>>,
}

struct HfProgressState {
    total: u64,
    downloaded: u64,
    last_emit: Instant,
}

impl HfDownloadProgress {
    fn new(app_handle: AppHandle, model_id: String) -> Self {
        Self {
            app_handle,
            model_id,
            state: Arc::new(Mutex::new(HfProgressState {
                total: 0,
                downloaded: 0,
                last_emit: Instant::now(),
            })),
        }
    }

    fn emit(&self, downloaded: u64, total: u64) {
        let percentage = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let _ = self.app_handle.emit(
            "model-download-progress",
            &DownloadProgress {
                model_id: self.model_id.clone(),
                downloaded,
                total,
                percentage,
            },
        );
    }
}

impl Progress for HfDownloadProgress {
    async fn init(&mut self, size: usize, _filename: &str) {
        {
            let mut st = self.state.lock().unwrap();
            st.total = size as u64;
            st.downloaded = 0;
            st.last_emit = Instant::now();
        }
        self.emit(0, size as u64);
    }

    async fn update(&mut self, size: usize) {
        let (downloaded, total, emit) = {
            let mut st = self.state.lock().unwrap();
            st.downloaded = st.downloaded.saturating_add(size as u64);
            let now = Instant::now();
            // Throttle to ~10 updates/sec, but always emit the final byte.
            let emit = now.duration_since(st.last_emit) >= Duration::from_millis(100)
                || (st.total > 0 && st.downloaded >= st.total);
            if emit {
                st.last_emit = now;
            }
            (st.downloaded, st.total, emit)
        };
        if emit {
            self.emit(downloaded, total);
        }
    }

    async fn finish(&mut self) {
        let total = {
            let st = self.state.lock().unwrap();
            st.total.max(st.downloaded)
        };
        self.emit(total, total);
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

pub struct ModelManager {
    app_handle: AppHandle,
    models_dir: PathBuf,
    available_models: Mutex<HashMap<String, ModelInfo>>,
    cancel_flags: Arc<Mutex<HashMap<String, CancellationToken>>>,
    extracting_models: Arc<Mutex<HashSet<String>>>,
    /// Single-flight guard for [`Self::rescan_local_models`] so concurrent
    /// refresh requests coalesce instead of scanning the disk in parallel.
    is_rescanning: Arc<AtomicBool>,
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

        // Seed spec-defined models before the on-disk scans, so a model already
        // in the HF cache dedups onto its richer catalog entry (the scans only
        // insert ids not already present) instead of showing as a bare cache
        // find. Legacy first, preserving the retired table's insertion order.
        // Additive — see `seed_descriptors`.
        let legacy = Self::seed_descriptors(&mut available_models, &crate::catalog::LEGACY);
        let catalog = Self::seed_descriptors(&mut available_models, &crate::catalog::CATALOG);
        info!(
            "Seeded {} legacy + {} catalog model(s) into the registry",
            legacy, catalog
        );

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
            extracting_models: Arc::new(Mutex::new(HashSet::new())),
            is_rescanning: Arc::new(AtomicBool::new(false)),
        };

        // Migrate any bundled models to user directory
        manager.migrate_bundled_models()?;

        // Migrate GigaAM from single-file to directory format
        manager.migrate_gigaam_to_directory()?;

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

    /// Seed spec-defined models (bundled catalog / frozen legacy list) into the
    /// registry, inserting each model whose id isn't already present (additive).
    /// Rendering goes through [`ModelDescriptor::to_model_info`] — the single
    /// constructor for every spec-produced entry.
    ///
    /// Catalog (`.gguf`, `HuggingFace`) and legacy (`.bin`/ONNX, `Url`) entries
    /// stay SEPARATE — different files, ids, and runtimes. Nothing is merged or
    /// removed; the UI just hides not-on-disk deprecated entries to retire legacy
    /// downloads, while already-downloaded ones stay runnable. Runs before the
    /// on-disk scans so a cached model dedups onto its catalog entry.
    fn seed_descriptors(
        available_models: &mut HashMap<String, ModelInfo>,
        descriptors: &[ModelDescriptor],
    ) -> usize {
        use std::collections::hash_map::Entry;
        let mut added = 0usize;
        for desc in descriptors {
            if let Entry::Vacant(slot) = available_models.entry(desc.id.clone()) {
                slot.insert(desc.to_model_info(&DiskStatus::default()));
                added += 1;
            }
        }
        added
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

    /// Migrate GigaAM from the old single-file format (giga-am-v3.int8.onnx)
    /// to the new directory format (giga-am-v3-int8/model.int8.onnx + vocab.txt).
    /// This was required by the transcribe-rs 0.3.x upgrade.
    fn migrate_gigaam_to_directory(&self) -> Result<()> {
        let old_file = self.models_dir.join("giga-am-v3.int8.onnx");
        let new_dir = self.models_dir.join("giga-am-v3-int8");

        if !old_file.exists() || new_dir.exists() {
            return Ok(());
        }

        info!("Migrating GigaAM from single-file to directory format");

        let vocab_path = self
            .app_handle
            .path()
            .resolve(
                "resources/models/gigaam_vocab.txt",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve GigaAM vocab path: {}", e))?;

        info!(
            "Resolved vocab path: {:?} (exists: {})",
            vocab_path,
            vocab_path.exists()
        );
        info!("Old file: {:?} (exists: {})", old_file, old_file.exists());
        info!("New dir: {:?} (exists: {})", new_dir, new_dir.exists());

        fs::create_dir_all(&new_dir)?;
        fs::rename(&old_file, new_dir.join("model.int8.onnx"))?;
        fs::copy(&vocab_path, new_dir.join("vocab.txt"))?;

        // Clean up old partial file if it exists
        let old_partial = self.models_dir.join("giga-am-v3.int8.onnx.partial");
        if old_partial.exists() {
            let _ = fs::remove_file(&old_partial);
        }

        info!("GigaAM migration complete");
        Ok(())
    }

    /// The models-dir drop-in that satisfies a *catalog* HF entry, if present.
    /// An explicitly placed file is user intent and the documented fallback
    /// when HF downloads are unavailable, so it wins over the shared HF cache.
    /// Scoped to catalog ids: a cache-discovered entry that merely shares a
    /// filename must never resolve to (or delete) a file it doesn't own.
    fn hf_drop_in(&self, model: &ModelInfo) -> Option<PathBuf> {
        if !matches!(model.source, ModelSource::HuggingFace { .. }) {
            return None;
        }
        if !crate::catalog::is_catalog_model(&model.id) {
            return None;
        }
        let path = self.models_dir.join(&model.filename);
        path.is_file().then_some(path)
    }

    /// The single authority on where a model's complete local bytes live.
    /// Every consumer of "is this model on disk / where" — download status,
    /// path resolution, download short-circuits — goes through here so the
    /// resolution policy can't drift between call sites. `is_downloaded` is
    /// defined as this returning `Some`.
    ///
    /// "Complete" means the artifact itself exists; transfer-in-progress
    /// guards (`is_downloading`, `.partial`) stay with the callers that need
    /// them.
    fn local_artifact(&self, model: &ModelInfo) -> Option<PathBuf> {
        if let ModelSource::HuggingFace { repo_id, revision } = &model.source {
            return self
                .hf_drop_in(model)
                .or_else(|| hf_cached_path(repo_id, revision, &model.filename));
        }
        let path = self.models_dir.join(&model.filename);
        if model.is_directory {
            path.is_dir().then_some(path)
        } else {
            path.is_file().then_some(path)
        }
    }

    fn update_download_status(&self) -> Result<()> {
        let mut models = self.available_models.lock().unwrap();

        for model in models.values_mut() {
            if let ModelSource::HuggingFace { repo_id, revision } = &model.source {
                let artifact = self.local_artifact(model);
                // `is_downloading` still holds its pre-refresh value here, so an
                // in-flight transfer (whose ref hf-hub only writes at the end)
                // isn't misreported as a broken cache entry.
                if artifact.is_none() && !model.is_downloading {
                    warn_if_hf_cache_entry_broken(repo_id, revision, &model.filename);
                }
                model.is_downloaded = artifact.is_some();
                model.is_downloading = false;
                model.partial_size = 0;
                continue;
            }
            if model.is_directory {
                let extracting_path = self
                    .models_dir
                    .join(format!("{}.extracting", &model.filename));

                // Clean up any leftover .extracting directories from interrupted extractions
                // But only if this model is NOT currently being extracted
                let is_currently_extracting = {
                    let extracting = self.extracting_models.lock().unwrap();
                    extracting.contains(&model.id)
                };
                if extracting_path.exists() && !is_currently_extracting {
                    warn!("Cleaning up interrupted extraction for model: {}", model.id);
                    let _ = fs::remove_dir_all(&extracting_path);
                }
            }

            let partial_path = self.models_dir.join(format!("{}.partial", &model.filename));
            model.is_downloaded = self.local_artifact(model).is_some();
            model.is_downloading = false;
            // Partial size of the in-progress artifact (.tar.gz for directory
            // models), if any.
            if partial_path.exists() {
                model.partial_size = partial_path.metadata().map(|m| m.len()).unwrap_or(0);
            } else {
                model.partial_size = 0;
            }
        }

        Ok(())
    }

    fn auto_select_model_if_needed(&self) -> Result<()> {
        let mut settings = get_settings(&self.app_handle);

        // Clear stale selection: selected model is set but doesn't exist
        // in available_models (e.g. deleted custom model file)
        if !settings.selected_model.is_empty() {
            let models = self.available_models.lock().unwrap();
            let exists = models.contains_key(&settings.selected_model);
            drop(models);

            if !exists {
                info!(
                    "Selected model '{}' not found in available models, clearing selection",
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

        // Filenames a spec-defined entry can claim from the models dir:
        // Url-sourced (legacy) files live here, and catalog HF entries accept a
        // drop-in override here (`hf_drop_in`). Scoped to exactly those so a
        // cache-discovered entry that merely shares a filename can't shadow a
        // dropped-in file it doesn't own — drop-ins never resolve to
        // non-catalog entries, so such a file must surface as custom instead,
        // regardless of whether this scan runs before or after the cache scan.
        let predefined_filenames: HashSet<String> = available_models
            .values()
            .filter(|m| matches!(m.engine_type, EngineType::TranscribeCpp) && !m.is_directory)
            .filter(|m| match &m.source {
                ModelSource::Url { .. } => true,
                ModelSource::HuggingFace { .. } => crate::catalog::is_catalog_model(&m.id),
                ModelSource::Local => false,
            })
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

            // A file whose name matches a catalog model is not surfaced as a
            // separate custom entry — it satisfies the catalog entry itself
            // (`hf_drop_in` resolves a models-dir file as the local override
            // for catalog HF models).
            if predefined_filenames.contains(&filename) {
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
                    is_directory: false,
                    engine_type: EngineType::TranscribeCpp,
                    accuracy_score: 0.0, // Sentinel: UI hides score bars when both are 0
                    speed_score: 0.0,
                    supports_translation: caps.supports_translation,
                    is_recommended: false,
                    supported_languages: caps.supported_languages,
                    supports_language_selection: caps.supports_language_selection,
                    is_custom: true,
                    supports_streaming: caps.supports_streaming,
                    supports_language_detection: caps.supports_language_detection,
                    deprecated: false,
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
        Self::discover_hf_cache_models_in(Cache::from_env().path(), available_models);
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

            let refs_dir = entry.path().join("refs");
            let Some(revision) = Self::pick_hf_revision(&refs_dir) else {
                continue;
            };
            let Ok(commit) = fs::read_to_string(refs_dir.join(&revision)) else {
                continue;
            };
            let snapshot = entry.path().join("snapshots").join(commit.trim());
            let Ok(files) = fs::read_dir(&snapshot) else {
                continue;
            };

            for file in files.flatten() {
                let fname = file.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".gguf") {
                    continue;
                }
                if known_hf.contains(&(repo_id.clone(), fname.clone())) {
                    continue;
                }
                let model_id = format!("{}/{}", repo_id, fname);
                if available_models.contains_key(&model_id) {
                    continue;
                }

                let path = snapshot.join(&fname);
                let probe = prober.probe_file(&path);
                // Only surface models transcribe-cpp recognises. Unreadable
                // files already warn inside probe_file; valid-but-foreign GGUFs
                // (e.g. LLMs sharing the cache) are expected and stay quiet.
                if probe.verdict != Compatibility::Compatible {
                    debug!(
                        "HF cache scan skipping {} (verdict: {:?})",
                        path.display(),
                        probe.verdict
                    );
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
                            revision: revision.clone(),
                        },
                        size_mb,
                        is_downloaded: true,
                        is_downloading: false,
                        partial_size: 0,
                        is_directory: false,
                        engine_type: EngineType::TranscribeCpp,
                        accuracy_score: 0.0,
                        speed_score: 0.0,
                        supports_translation: caps.supports_translation,
                        is_recommended: false,
                        supported_languages: caps.supported_languages,
                        supports_language_selection: caps.supports_language_selection,
                        is_custom: false,
                        supports_streaming: caps.supports_streaming,
                        supports_language_detection: caps.supports_language_detection,
                        deprecated: false,
                    },
                );
            }
        }
    }

    /// Pick a cache ref to resolve a snapshot from, preferring `main`.
    fn pick_hf_revision(refs_dir: &Path) -> Option<String> {
        if refs_dir.join("main").is_file() {
            return Some("main".to_string());
        }
        fs::read_dir(refs_dir).ok()?.flatten().find_map(|e| {
            if e.path().is_file() {
                e.file_name().to_str().map(str::to_string)
            } else {
                None
            }
        })
    }

    /// Verifies the SHA256 of `path` against `expected_sha256` (if provided).
    /// On mismatch or read error the partial file is deleted and an error is returned,
    /// so the next download attempt always starts from a clean state.
    /// When `expected_sha256` is `None` (custom user models) verification is skipped.
    fn verify_sha256(path: &Path, expected_sha256: Option<&str>, model_id: &str) -> Result<()> {
        let Some(expected) = expected_sha256 else {
            return Ok(());
        };
        match Self::compute_sha256(path) {
            Ok(actual) if actual == expected => {
                info!("SHA256 verified for model {}", model_id);
                Ok(())
            }
            Ok(actual) => {
                warn!(
                    "SHA256 mismatch for model {}: expected {}, got {}",
                    model_id, expected, actual
                );
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Download verification failed for model {}: file is corrupt. Please retry.",
                    model_id
                ))
            }
            Err(e) => {
                let _ = fs::remove_file(path);
                Err(anyhow::anyhow!(
                    "Failed to verify download for model {}: {}. Please retry.",
                    model_id,
                    e
                ))
            }
        }
    }

    /// Computes the SHA256 hex digest of a file, reading in 64KB chunks to handle large models.
    fn compute_sha256(path: &Path) -> Result<String> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Download a Hugging Face-sourced model into the shared HF cache via
    /// hf-hub, reporting progress through the same `model-download-progress`
    /// event the URL path uses. Uses hf-hub's stock cache location, but
    /// deliberately strips ambient credentials (see the `with_token` call).
    async fn download_hf_model(
        &self,
        model_info: &ModelInfo,
        repo_id: String,
        revision: String,
    ) -> Result<()> {
        let model_id = model_info.id.clone();
        let filename = model_info.filename.clone();

        // Already satisfied locally (drop-in, or cached possibly by another
        // tool)? Done.
        if self.local_artifact(model_info).is_some() {
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

        info!(
            "Downloading HF model {} from {}@{} ({})",
            model_id, repo_id, revision, filename
        );

        // Download chunks in parallel (default is 1 = sequential). Throughput
        // scales near-linearly with this count because each connection is capped
        // (~8 MB/s observed per stream), so we stack several to approach the
        // link's real bandwidth. 8 stays light on CPU/RAM (~80 MB peak buffers)
        // even on older machines and is browser-like in connection count.
        let api = ApiBuilder::from_env()
            // Never attach ambient HF credentials (~/.cache/huggingface/token).
            // Every repo Handy downloads is public, and a stale token left by an
            // old `huggingface-cli login` turns downloads into opaque 401s.
            .with_token(None)
            .with_progress(false)
            .with_max_files(8)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to init Hugging Face API: {}", e))?;
        let repo = api.repo(Repo::with_revision(repo_id, RepoType::Model, revision));
        let progress = HfDownloadProgress::new(self.app_handle.clone(), model_id.clone());
        match repo
            .download_with_progress_cancellable(&filename, progress, cancel_token)
            .await
        {
            Ok(_) => {}
            Err(hf_hub::api::tokio::ApiError::Cancelled) => {
                // User cancelled. hf-hub leaves the partially downloaded
                // `.sync.part` in the shared cache, so a later attempt resumes
                // instead of restarting. The guard resets is_downloading and
                // drops the token; `cancel_download` already emitted
                // `model-download-cancelled`.
                info!("HF download cancelled for: {}", model_id);
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Hugging Face download failed: {}", e));
            }
        }

        cleanup.disarmed = true;
        self.update_download_status()?;
        self.cancel_flags.lock().unwrap().remove(&model_id);
        let _ = self.app_handle.emit("model-download-complete", &model_id);
        info!("HF model {} downloaded", model_id);
        Ok(())
    }

    pub async fn download_model(&self, model_id: &str) -> Result<()> {
        let result = self.download_model_inner(model_id).await;
        if result.is_err() {
            // Every mutation leaves fresh status behind, error paths included:
            // the cleanup guard already reset the in-memory flags, but the UI
            // only learns about it from an event. (Success paths emit
            // `model-download-complete` instead.)
            let _ = self.update_download_status();
            let _ = self.app_handle.emit("models-updated", ());
        }
        result
    }

    async fn download_model_inner(&self, model_id: &str) -> Result<()> {
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
        if self.local_artifact(&model_info).is_some() {
            // Clean up any partial file that might exist
            if partial_path.exists() {
                let _ = fs::remove_file(&partial_path);
            }
            self.update_download_status()?;
            // Mirror the HF short-circuit: the frontend set optimistic
            // downloading state and clears it on this event.
            let _ = self.app_handle.emit("model-download-complete", model_id);
            return Ok(());
        }

        // Check if we have a partial download to resume
        let mut resume_from = if partial_path.exists() {
            let size = partial_path.metadata()?.len();
            info!("Resuming download of model {} from byte {}", model_id, size);
            size
        } else {
            info!("Starting fresh download of model {} from {}", model_id, url);
            0
        };

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

        // Create HTTP client with range request for resuming
        let client = reqwest::Client::new();
        let mut request = client.get(&url);

        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }

        let mut response = request.send().await?;

        // If we tried to resume but server returned 200 (not 206 Partial Content),
        // the server doesn't support range requests. Delete partial file and restart
        // fresh to avoid file corruption (appending full file to partial).
        if resume_from > 0 && response.status() == reqwest::StatusCode::OK {
            warn!(
                "Server doesn't support range requests for model {}, restarting download",
                model_id
            );
            drop(response);
            let _ = fs::remove_file(&partial_path);

            // Reset resume_from since we're starting fresh
            resume_from = 0;

            // Restart download without range header
            response = client.get(&url).send().await?;
        }

        // Check for success or partial content status
        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(anyhow::anyhow!(
                "Failed to download model: HTTP {}",
                response.status()
            ));
        }

        let total_size = if resume_from > 0 {
            // For resumed downloads, add the resume point to content length
            resume_from + response.content_length().unwrap_or(0)
        } else {
            response.content_length().unwrap_or(0)
        };

        let mut downloaded = resume_from;
        let mut stream = response.bytes_stream();

        // Open file for appending if resuming, or create new if starting fresh
        let mut file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&partial_path)?
        } else {
            std::fs::File::create(&partial_path)?
        };

        // Emit initial progress
        let initial_progress = DownloadProgress {
            model_id: model_id.to_string(),
            downloaded,
            total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            },
        };
        let _ = self
            .app_handle
            .emit("model-download-progress", &initial_progress);

        // Throttle progress events to max 10/sec (100ms intervals)
        let mut last_emit = Instant::now();
        let throttle_duration = Duration::from_millis(100);

        // Download with progress
        while let Some(chunk) = stream.next().await {
            // Check if download was cancelled
            if cancel_token.is_cancelled() {
                drop(file);
                info!("Download cancelled for: {}", model_id);
                // Keep partial file for resume functionality.
                // Guard handles is_downloading + cancel_flags cleanup on drop.
                return Ok(());
            }

            let chunk = chunk?;

            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;

            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };

            // Emit progress event (throttled to avoid UI freeze)
            if last_emit.elapsed() >= throttle_duration {
                let progress = DownloadProgress {
                    model_id: model_id.to_string(),
                    downloaded,
                    total: total_size,
                    percentage,
                };
                let _ = self.app_handle.emit("model-download-progress", &progress);
                last_emit = Instant::now();
            }
        }

        // Emit final progress to ensure 100% is shown
        let final_progress = DownloadProgress {
            model_id: model_id.to_string(),
            downloaded,
            total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                100.0
            },
        };
        let _ = self
            .app_handle
            .emit("model-download-progress", &final_progress);

        file.flush()?;
        drop(file); // Ensure file is closed before moving

        // Verify downloaded file size matches expected size
        if total_size > 0 {
            let actual_size = partial_path.metadata()?.len();
            if actual_size != total_size {
                // Download is incomplete/corrupted - delete partial and return error
                let _ = fs::remove_file(&partial_path);
                return Err(anyhow::anyhow!(
                    "Download incomplete: expected {} bytes, got {} bytes",
                    total_size,
                    actual_size
                ));
            }
        }

        // Verify SHA256 checksum. Runs in a blocking thread so the async executor is not
        // stalled while hashing large model files (up to 1.6 GB). On failure the partial
        // is deleted inside verify_sha256 so the next attempt always starts fresh.
        let _ = self.app_handle.emit("model-verification-started", model_id);
        info!("Verifying SHA256 for model {}...", model_id);
        let verify_path = partial_path.clone();
        let verify_expected = expected_sha256.clone();
        let verify_model_id = model_id.to_string();
        let verify_result = tokio::task::spawn_blocking(move || {
            Self::verify_sha256(&verify_path, verify_expected.as_deref(), &verify_model_id)
        })
        .await
        .map_err(|e| anyhow::anyhow!("SHA256 task panicked: {}", e))?;
        verify_result?;
        let _ = self
            .app_handle
            .emit("model-verification-completed", model_id);

        // Handle directory-based models (extract tar.gz) vs file-based models
        if model_info.is_directory {
            // Track that this model is being extracted
            {
                let mut extracting = self.extracting_models.lock().unwrap();
                extracting.insert(model_id.to_string());
            }

            // Emit extraction started event
            let _ = self.app_handle.emit("model-extraction-started", model_id);
            info!("Extracting archive for directory-based model: {}", model_id);

            // Use a temporary extraction directory to ensure atomic operations
            let temp_extract_dir = self
                .models_dir
                .join(format!("{}.extracting", &model_info.filename));
            let final_model_dir = self.models_dir.join(&model_info.filename);

            // Clean up any previous incomplete extraction
            if temp_extract_dir.exists() {
                let _ = fs::remove_dir_all(&temp_extract_dir);
            }

            // Create temporary extraction directory
            fs::create_dir_all(&temp_extract_dir)?;

            // Open the downloaded tar.gz file
            let tar_gz = File::open(&partial_path)?;
            let tar = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(tar);

            // Extract to the temporary directory first
            archive.unpack(&temp_extract_dir).map_err(|e| {
                let error_msg = format!("Failed to extract archive: {}", e);
                // Clean up failed extraction
                let _ = fs::remove_dir_all(&temp_extract_dir);
                // Delete the corrupt partial file so the next download attempt starts fresh
                // instead of resuming from a broken archive (issue #858).
                let _ = fs::remove_file(&partial_path);
                // Remove from extracting set
                {
                    let mut extracting = self.extracting_models.lock().unwrap();
                    extracting.remove(model_id);
                }
                let _ = self.app_handle.emit(
                    "model-extraction-failed",
                    &serde_json::json!({
                        "model_id": model_id,
                        "error": error_msg
                    }),
                );
                anyhow::anyhow!(error_msg)
            })?;

            // Find the actual extracted directory (archive might have a nested structure)
            let extracted_dirs: Vec<_> = fs::read_dir(&temp_extract_dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();

            if extracted_dirs.len() == 1 {
                // Single directory extracted, move it to the final location
                let source_dir = extracted_dirs[0].path();
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&source_dir, &final_model_dir)?;
                // Clean up temp directory
                let _ = fs::remove_dir_all(&temp_extract_dir);
            } else {
                // Multiple items or no directories, rename the temp directory itself
                if final_model_dir.exists() {
                    fs::remove_dir_all(&final_model_dir)?;
                }
                fs::rename(&temp_extract_dir, &final_model_dir)?;
            }

            info!("Successfully extracted archive for model: {}", model_id);
            // Remove from extracting set
            {
                let mut extracting = self.extracting_models.lock().unwrap();
                extracting.remove(model_id);
            }
            // Emit extraction completed event
            let _ = self.app_handle.emit("model-extraction-completed", model_id);

            // Remove the downloaded tar.gz file
            let _ = fs::remove_file(&partial_path);
        } else {
            // Move partial file to final location for file-based models
            fs::rename(&partial_path, &model_path)?;
        }

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

        // Removal is best-effort across every artifact the entry owns: one
        // failure must not skip the remaining removals, and status refresh +
        // UI notification happen regardless so a partial failure can't leave
        // the UI stale.
        let mut deleted_something = false;
        let mut first_err: Option<anyhow::Error> = None;

        if let ModelSource::HuggingFace { repo_id, revision } = &model_info.source {
            // A manual drop-in satisfies a catalog entry the same way a cache
            // download does, so delete must remove it too — otherwise the
            // model would still show as downloaded afterwards. `hf_drop_in`
            // scopes this to catalog ids: a cache-discovered entry sharing the
            // filename doesn't own the drop-in and must not delete it.
            if let Some(drop_in) = self.hf_drop_in(&model_info) {
                info!("Deleting local model file at: {:?}", drop_in);
                match fs::remove_file(&drop_in) {
                    Ok(()) => deleted_something = true,
                    Err(e) => first_err = Some(e.into()),
                }
            }
            // Cached at <cache>/models--org--name/snapshots/<rev>/<file>; remove
            // the whole repo dir (blobs + refs + snapshots). Per product decision,
            // delete hard-removes from the shared HF cache.
            if let Some(file) = hf_cached_path(repo_id, revision, &model_info.filename) {
                if let Some(repo_dir) = file.ancestors().nth(3) {
                    if repo_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("models--"))
                    {
                        info!("Deleting HF cache repo at: {:?}", repo_dir);
                        match fs::remove_dir_all(repo_dir) {
                            Ok(()) => deleted_something = true,
                            Err(e) => first_err = first_err.or(Some(e.into())),
                        }
                    }
                }
            }
        } else {
            let model_path = self.models_dir.join(&model_info.filename);
            let partial_path = self
                .models_dir
                .join(format!("{}.partial", &model_info.filename));
            debug!("ModelManager: Model path: {:?}", model_path);
            debug!("ModelManager: Partial path: {:?}", partial_path);

            if model_info.is_directory {
                if model_path.is_dir() {
                    info!("Deleting model directory at: {:?}", model_path);
                    match fs::remove_dir_all(&model_path) {
                        Ok(()) => deleted_something = true,
                        Err(e) => first_err = Some(e.into()),
                    }
                }
            } else if model_path.exists() {
                info!("Deleting model file at: {:?}", model_path);
                match fs::remove_file(&model_path) {
                    Ok(()) => deleted_something = true,
                    Err(e) => first_err = Some(e.into()),
                }
            }

            if partial_path.exists() {
                info!("Deleting partial file at: {:?}", partial_path);
                match fs::remove_file(&partial_path) {
                    Ok(()) => deleted_something = true,
                    Err(e) => first_err = first_err.or(Some(e.into())),
                }
            }
        }

        // Custom models are removed from the list entirely since they have no
        // download source — but only after a fully successful delete, so the
        // entry can't vanish while its file is still on disk.
        if model_info.is_custom && deleted_something && first_err.is_none() {
            let mut models = self.available_models.lock().unwrap();
            models.remove(model_id);
            debug!("ModelManager: removed custom model from available models");
        } else {
            self.update_download_status()?;
            debug!("ModelManager: download status updated");
        }

        if deleted_something {
            let _ = self.app_handle.emit("model-deleted", model_id);
        }

        if let Some(e) = first_err {
            return Err(e.context(format!("Failed to fully delete model {}", model_id)));
        }
        if !deleted_something {
            return Err(anyhow::anyhow!("No model files found to delete"));
        }
        Ok(())
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

        if matches!(model_info.source, ModelSource::HuggingFace { .. }) {
            return self.local_artifact(&model_info).ok_or_else(|| {
                anyhow::anyhow!(
                    "Complete model file not found in models dir or HF cache: {}",
                    model_id
                )
            });
        }

        // For models-dir models, refuse while a .partial download is in flight.
        let partial_path = self
            .models_dir
            .join(format!("{}.partial", &model_info.filename));
        match self.local_artifact(&model_info) {
            Some(path) if !partial_path.exists() => Ok(path),
            _ if model_info.is_directory => Err(anyhow::anyhow!(
                "Complete model directory not found: {}",
                model_id
            )),
            _ => Err(anyhow::anyhow!(
                "Complete model file not found: {}",
                model_id
            )),
        }
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
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_effective_language_accepts_chinese_script_intent_for_zh_capability() {
        let languages = vec!["zh".to_string()];

        assert_eq!(effective_language("zh-Hans", &languages, false), "zh-Hans");
        assert_eq!(effective_language("zh-Hant", &languages, false), "zh-Hant");
    }

    #[test]
    fn test_effective_language_falls_back_to_canonical_chinese() {
        let languages = vec!["zh-Hant".to_string()];

        assert_eq!(effective_language("auto", &languages, false), "zh");
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

        assert_eq!(effective_language("en", &languages, true), "en-US");
        assert_eq!(effective_language("es", &languages, true), "es-ES");
        // `zh`/`ja` have no bare entry in this model's table; resolve to locale.
        assert_eq!(effective_language("zh", &languages, true), "zh-CN");
        assert_eq!(effective_language("ja", &languages, true), "ja-JP");
        // An unsupported intent still auto-detects when the model can.
        assert_eq!(effective_language("fr", &languages, true), "auto");
    }

    #[test]
    fn test_effective_language_preserves_chinese_script_intent_for_locale_model() {
        // Script intents survive so Simplified/Traditional output conversion
        // still fires, even when the model advertises a regioned Chinese code.
        let languages = vec!["en-US".to_string(), "zh-CN".to_string()];

        assert_eq!(effective_language("zh-Hans", &languages, true), "zh-Hans");
        assert_eq!(effective_language("zh-Hant", &languages, true), "zh-Hant");
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
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
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
                deprecated: true,
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
        assert!(matches!(gguf.engine_type, EngineType::TranscribeCpp));

        // Should NOT have discovered hidden, non-model, predefined, partial, or directories
        assert!(!models.contains_key(".hidden-model"));
        assert!(!models.contains_key("readme"));
        assert!(!models.contains_key("download.bin"));
        assert!(!models.contains_key("some-directory"));
    }

    #[test]
    fn test_drop_in_shadowing_scoped_to_claimable_filenames() {
        let temp_dir = TempDir::new().unwrap();
        let models_dir = temp_dir.path().to_path_buf();

        // A drop-in that shares its filename with a model discovered from the
        // shared HF cache (non-catalog id). The cache entry doesn't own the
        // models-dir file (`hf_drop_in` is catalog-scoped), so the drop-in must
        // surface as its own custom entry even when the cache entry is already
        // registered — the rescan case, where the cache scan ran first.
        let mut f = File::create(models_dir.join("whisper-q8.gguf")).unwrap();
        f.write_all(&build_test_gguf_string_metadata(&[(
            "general.name",
            "Dropped In",
        )]))
        .unwrap();

        // A drop-in matching a *catalog* entry's default filename. That file
        // satisfies the catalog entry itself, so it must NOT become a custom
        // entry.
        let catalog_info = crate::catalog::CATALOG[0].to_model_info(&DiskStatus::default());
        File::create(models_dir.join(&catalog_info.filename)).unwrap();

        let mut models = HashMap::new();
        models.insert(catalog_info.id.clone(), catalog_info.clone());
        models.insert(
            "someorg/whisper-repo/whisper-q8.gguf".to_string(),
            ModelInfo {
                id: "someorg/whisper-repo/whisper-q8.gguf".to_string(),
                name: "Cache Find".to_string(),
                description: "From Hugging Face cache: someorg/whisper-repo".to_string(),
                filename: "whisper-q8.gguf".to_string(),
                source: ModelSource::HuggingFace {
                    repo_id: "someorg/whisper-repo".to_string(),
                    revision: "main".to_string(),
                },
                size_mb: 100,
                is_downloaded: true,
                is_downloading: false,
                partial_size: 0,
                is_directory: false,
                engine_type: EngineType::TranscribeCpp,
                accuracy_score: 0.0,
                speed_score: 0.0,
                supports_translation: false,
                is_recommended: false,
                supported_languages: vec![],
                supports_language_selection: false,
                is_custom: false,
                supports_streaming: false,
                supports_language_detection: false,
                deprecated: false,
            },
        );

        ModelManager::discover_custom_transcribe_models(&models_dir, &mut models).unwrap();

        let dropped = models
            .get("whisper-q8")
            .expect("drop-in sharing a cache entry's filename must still be discovered");
        assert!(dropped.is_custom);
        assert_eq!(dropped.name, "Dropped In");

        let catalog_stem = catalog_info.filename.trim_end_matches(".gguf");
        assert!(
            !models.contains_key(catalog_stem),
            "catalog-filename drop-in must satisfy the catalog entry, not become custom"
        );
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
    fn test_discover_custom_models_nonexistent_dir() {
        let models_dir = PathBuf::from("/nonexistent/path/that/does/not/exist");

        let mut models = HashMap::new();
        let count_before = models.len();

        // Should not error, just return Ok
        let result = ModelManager::discover_custom_transcribe_models(&models_dir, &mut models);
        assert!(result.is_ok());
        assert_eq!(models.len(), count_before);
    }

    // ── SHA256 verification tests ─────────────────────────────────────────────

    /// Helper: write `data` to a temp file and return (TempDir, path).
    /// TempDir must be kept alive for the duration of the test.
    fn write_temp_file(data: &[u8]) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("model.partial");
        let mut f = File::create(&path).unwrap();
        f.write_all(data).unwrap();
        (dir, path)
    }

    #[test]
    fn test_verify_sha256_skipped_when_none() {
        // Custom models have no expected hash — verification must be a no-op.
        let (_dir, path) = write_temp_file(b"anything");
        assert!(ModelManager::verify_sha256(&path, None, "custom").is_ok());
        assert!(
            path.exists(),
            "file must be untouched when verification is skipped"
        );
    }

    #[test]
    fn test_verify_sha256_passes_on_correct_hash() {
        // Compute the real hash so the test is self-consistent.
        let (_dir, path) = write_temp_file(b"hello world");
        let actual = ModelManager::compute_sha256(&path).unwrap();
        assert!(
            ModelManager::verify_sha256(&path, Some(&actual), "test_model").is_ok(),
            "should pass when hash matches"
        );
        assert!(
            path.exists(),
            "file must be kept on successful verification"
        );
    }

    #[test]
    fn test_verify_sha256_fails_and_deletes_partial_on_mismatch() {
        let (_dir, path) = write_temp_file(b"this is not the real model");
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = ModelManager::verify_sha256(&path, Some(wrong_hash), "bad_model");

        assert!(result.is_err(), "mismatch must return an error");
        assert!(
            result.unwrap_err().to_string().contains("corrupt"),
            "error message should mention corruption"
        );
        assert!(
            !path.exists(),
            "partial file must be deleted after hash mismatch"
        );
    }

    #[test]
    fn test_verify_sha256_fails_and_deletes_partial_when_file_missing() {
        // Simulate a partial file that was already removed (e.g. disk full mid-download).
        let dir = TempDir::new().unwrap();
        let missing_path = dir.path().join("gone.partial");
        // Don't create the file — it should not exist.

        let result =
            ModelManager::verify_sha256(&missing_path, Some("anyexpectedhash"), "missing_model");

        assert!(result.is_err(), "missing file must return an error");
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
            if repo_id == "handy-computer/whisper-test" && revision == "main")
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
