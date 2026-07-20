// Settings store operations: load, save, flush, debounced writer,
// sanitization, and migration helpers.

use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::RwLock;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::defaults::get_default_settings;
use super::types::*;

/// In-memory cache of the current settings. This is the single source of truth
/// for all reads — `get_settings_safe` reads from here, not from the store.
/// `write_settings_safe` updates this cache immediately and schedules a debounced
/// disk flush. This eliminates the read-modify-write race where a quick second
/// settings change could read stale data from the store before the first
/// debounced write flushed.
pub struct SettingsCache {
    settings: RwLock<AppSettings>,
}

impl SettingsCache {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings: RwLock::new(settings),
        }
    }

    pub fn get(&self) -> AppSettings {
        self.settings.read().unwrap().clone()
    }

    pub fn update(&self, settings: AppSettings) {
        *self.settings.write().unwrap() = settings;
        trace!("[settings] cache updated");
    }
}

/// Validate that float fields are not NaN before serialization.
pub(crate) fn sanitize_floats(settings: &mut AppSettings) {
    if settings.audio_feedback_volume.is_nan() {
        error!("audio_feedback_volume is NaN, resetting to default");
        settings.audio_feedback_volume = default_audio_feedback_volume();
    }
    if settings.word_correction_threshold.is_nan() {
        error!("word_correction_threshold is NaN, resetting to default");
        settings.word_correction_threshold = default_word_correction_threshold();
    }
    if settings.overlay_scale.is_nan() {
        error!("overlay_scale is NaN, resetting to default");
        settings.overlay_scale = default_overlay_scale();
    }
    if settings.hybrid_threshold_secs.is_nan() {
        error!("hybrid_threshold_secs is NaN, resetting to default");
        settings.hybrid_threshold_secs = default_hybrid_threshold_secs();
    }
}

/// Helper: serialize settings to a serde_json::Value, logging errors instead of panicking.
pub(crate) fn settings_to_value(settings: &AppSettings) -> Option<serde_json::Value> {
    match serde_json::to_value(settings) {
        Ok(v) => Some(v),
        Err(e) => {
            error!("Failed to serialize settings to JSON: {}", e);
            None
        }
    }
}

/// Helper: open the settings store, logging errors instead of panicking.
pub(crate) fn open_settings_store(
    app: &AppHandle,
) -> Option<Arc<tauri_plugin_store::Store<tauri::Wry>>> {
    match app.store(crate::portable::store_path(SETTINGS_STORE_PATH)) {
        Ok(store) => Some(store),
        Err(e) => {
            error!("Failed to initialize settings store: {}", e);
            None
        }
    }
}

/// Execute a settings operation safely, catching any panics before they can
/// propagate to WebKit's URL scheme handler (which calls `abort()` on panic).
pub(crate) fn safe_settings_operation<F, T>(label: &str, op: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    match catch_unwind(AssertUnwindSafe(op)) {
        Ok(result) => Some(result),
        Err(panic_info) => {
            error!(
                "Panic in settings operation ({}) — caught to prevent WebKit abort: {:?}",
                label, panic_info
            );
            None
        }
    }
}

/// Ensure post-process providers have default entries. Returns true if settings changed.
pub(crate) fn ensure_post_process_defaults(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    for provider in default_post_process_providers() {
        // Use match to do a single lookup - either sync existing or add new
        match settings
            .post_process_providers
            .iter_mut()
            .find(|p| p.id == provider.id)
        {
            Some(existing) => {
                // Sync supports_structured_output field for existing providers (migration)
                if existing.supports_structured_output != provider.supports_structured_output {
                    debug!(
                        "Updating supports_structured_output for provider '{}' from {} to {}",
                        provider.id,
                        existing.supports_structured_output,
                        provider.supports_structured_output
                    );
                    existing.supports_structured_output = provider.supports_structured_output;
                    changed = true;
                }
            }
            None => {
                // Provider doesn't exist, add it
                settings.post_process_providers.push(provider.clone());
                changed = true;
            }
        }

        if !settings.post_process_api_keys.contains_key(&provider.id) {
            settings
                .post_process_api_keys
                .insert(provider.id.clone(), String::new());
            changed = true;
        }

        let default_model = default_model_for_provider(&provider.id);
        match settings.post_process_models.get_mut(&provider.id) {
            Some(existing) => {
                if existing.is_empty() && !default_model.is_empty() {
                    *existing = default_model.clone();
                    changed = true;
                }
            }
            None => {
                settings
                    .post_process_models
                    .insert(provider.id.clone(), default_model);
                changed = true;
            }
        }
    }

    changed
}

// ── Safe wrappers ──

/// Safe wrapper around [`load_or_create_app_settings`] that catches panics
/// and falls back to defaults.
pub fn load_or_create_app_settings_safe(app: &AppHandle) -> AppSettings {
    safe_settings_operation("load_or_create_app_settings", || {
        load_or_create_app_settings(app)
    })
    .unwrap_or_else(|| {
        error!("Falling back to default settings after panic in load_or_create_app_settings");
        get_default_settings()
    })
}

/// Safe wrapper around [`get_settings`] that catches panics and falls back to defaults.
/// Reads from the in-memory cache when available, eliminating the read-modify-write
/// race with the debounced disk writer.
pub fn get_settings_safe(app: &AppHandle) -> AppSettings {
    safe_settings_operation("get_settings_safe", || {
        if let Some(cache) = app.try_state::<Arc<SettingsCache>>() {
            trace!("[settings] get_safe (cache hit)");
            return cache.get();
        }
        get_settings(app)
    })
    .unwrap_or_else(|| {
        error!("Falling back to default settings after panic in get_settings_safe");
        get_default_settings()
    })
}

/// Safe wrapper around [`write_settings`] that catches panics.
/// Delegates to `write_settings` which updates the cache and schedules a
/// debounced disk write. The `_safe` variant wraps the call in a panic
/// catcher as an extra safety net for callers that prefer it.
pub fn write_settings_safe(app: &AppHandle, settings: AppSettings) {
    let _ = safe_settings_operation("write_settings_safe", || {
        write_settings(app, settings);
    });
}

/// Safe wrapper around [`write_settings_immediate`] that catches panics.
/// Delegates to `write_settings_immediate` which updates the cache and
/// writes to disk synchronously.
#[allow(dead_code)]
pub fn write_settings_immediate_safe(app: &AppHandle, settings: AppSettings) {
    let _ = safe_settings_operation("write_settings_immediate", || {
        write_settings_immediate(app, settings);
    });
}

// ── Core load/save functions ──

/// Startup entry point. Same load-or-create/salvage/migrate behavior as
/// `get_settings`; kept as a named alias for call-site clarity, plus a
/// one-time debug dump of the loaded settings.
pub fn load_or_create_app_settings(app: &AppHandle) -> AppSettings {
    // Initialize store
    let Some(store) = open_settings_store(app) else {
        error!("Cannot load settings: store initialization failed, returning defaults");
        return get_default_settings();
    };

    let mut settings = if let Some(settings_value) = store.get("settings") {
        match serde_json::from_value::<AppSettings>(settings_value.clone()) {
            Ok(mut settings) => {
                debug!("Found existing settings: {:?}", settings);
                let default_settings = get_default_settings();
                let mut updated = false;

                // Merge default bindings into existing settings
                for (key, value) in default_settings.bindings {
                    if !settings.bindings.contains_key(&key) {
                        debug!("Adding missing binding: {}", key);
                        settings.bindings.insert(key, value);
                        updated = true;
                    }
                }

                // Migrate new settings fields that may be None in existing configs
                if settings.router_script_path.is_none()
                    && default_settings.router_script_path.is_some()
                {
                    debug!("Migrating router_script_path from default");
                    settings.router_script_path = default_settings.router_script_path.clone();
                    updated = true;
                }
                if settings.router_env_file.is_none() && default_settings.router_env_file.is_some() {
                    debug!("Migrating router_env_file from default");
                    settings.router_env_file = default_settings.router_env_file.clone();
                    updated = true;
                }

                // Migrate usb_watchdog_cycle_on_wake
                if settings.usb_watchdog_enabled && !settings.usb_watchdog_cycle_on_wake {
                    debug!("Migrating usb_watchdog_cycle_on_wake to true for enabled watchdog");
                    settings.usb_watchdog_cycle_on_wake = true;
                    updated = true;
                }

                // Migrate use_advanced_custom_words to word_correction_mode
                if settings.use_advanced_custom_words
                    && settings.word_correction_mode == WordCorrectionMode::WordBias
                {
                    debug!("Migrating use_advanced_custom_words=true to word_correction_mode=Pronunciation");
                    settings.word_correction_mode = WordCorrectionMode::Pronunciation;
                    updated = true;
                }

                // Run one-time settings migrations
                if apply_settings_migrations(&mut settings, &settings_value) {
                    updated = true;
                }

                if updated {
                    debug!("Settings updated with new bindings/migrations");
                    sanitize_floats(&mut settings);
                    if let Some(value) = settings_to_value(&settings) {
                        store.set("settings", value);
                        match store.save() {
                            Ok(()) => info!("[settings] flushed to disk"),
                            Err(e) => error!("[settings] disk write FAILED: {e}"),
                        }
                    }
                }

                settings
            }
            Err(e) => {
                warn!("Failed to parse settings: {}", e);
                let salvaged = salvage_settings(&settings_value);
                sanitize_floats(&mut salvaged.clone());
                if let Some(value) = settings_to_value(&salvaged) {
                    store.set("settings", value);
                    match store.save() {
                        Ok(()) => info!("[settings] flushed to disk"),
                        Err(e) => error!("[settings] disk write FAILED: {e}"),
                    }
                }
                salvaged
            }
        }
    } else {
        let default_settings = get_default_settings();
        if let Some(value) = settings_to_value(&default_settings) {
            store.set("settings", value);
        }
        default_settings
    };

    if ensure_post_process_defaults(&mut settings) {
        sanitize_floats(&mut settings);
        if let Some(value) = settings_to_value(&settings) {
            store.set("settings", value);
        }
    }

    settings
}

pub fn get_settings(app: &AppHandle) -> AppSettings {
    // Prefer the in-memory cache — this is the single source of truth at runtime.
    // Only fall back to the plugin-store path during early startup before
    // SettingsCache is managed (e.g. during load_or_create_app_settings).
    if let Some(cache) = app.try_state::<Arc<SettingsCache>>() {
        trace!("[settings] get (cache hit)");
        return cache.get();
    }

    warn!("[settings] get: cache not managed yet, falling back to plugin-store");

    let Some(store) = open_settings_store(app) else {
        error!("Cannot get settings: store initialization failed, returning defaults");
        return get_default_settings();
    };

    let mut settings = if let Some(settings_value) = store.get("settings") {
        serde_json::from_value::<AppSettings>(settings_value).unwrap_or_else(|e| {
            warn!("Failed to parse settings: {}, returning defaults", e);
            let default_settings = get_default_settings();
            if let Some(value) = settings_to_value(&default_settings) {
                store.set("settings", value);
            }
            default_settings
        })
    } else {
        let default_settings = get_default_settings();
        if let Some(value) = settings_to_value(&default_settings) {
            store.set("settings", value);
        }
        default_settings
    };

    // Migrate new settings fields that may be None in existing configs
    let default_settings = get_default_settings();
    let mut needs_save = false;

    if settings.router_script_path.is_none() && default_settings.router_script_path.is_some() {
        debug!("Migrating router_script_path from default");
        settings.router_script_path = default_settings.router_script_path.clone();
        needs_save = true;
    }
    if settings.router_env_file.is_none() && default_settings.router_env_file.is_some() {
        debug!("Migrating router_env_file from default");
        settings.router_env_file = default_settings.router_env_file.clone();
        needs_save = true;
    }

    if settings.usb_watchdog_enabled && !settings.usb_watchdog_cycle_on_wake {
        debug!("Migrating usb_watchdog_cycle_on_wake to true for enabled watchdog");
        settings.usb_watchdog_cycle_on_wake = true;
        needs_save = true;
    }

    // Merge missing bindings too
    for (key, value) in default_settings.bindings {
        if !settings.bindings.contains_key(&key) {
            debug!("Adding missing binding: {}", key);
            settings.bindings.insert(key, value);
            needs_save = true;
        }
    }

    if needs_save {
        sanitize_floats(&mut settings);
        if let Some(value) = settings_to_value(&settings) {
            store.set("settings", value);
            match store.save() {
                Ok(()) => info!("[settings] flushed to disk"),
                Err(e) => error!("[settings] disk write FAILED: {e}"),
            }
        }
    }

    if ensure_post_process_defaults(&mut settings) {
        sanitize_floats(&mut settings);
        if let Some(value) = settings_to_value(&settings) {
            store.set("settings", value);
        }
    }

    settings
}

/// Rebuilds settings from a store value that failed to deserialize as a whole.
/// Every stored field that is individually valid is kept; only broken values
/// (e.g. an enum variant written by a newer or older version) fall back to
/// their default. This means one bad field can never reset the rest of the
/// user's configuration (#1619).
fn salvage_settings(stored: &serde_json::Value) -> AppSettings {
    let Some(stored_map) = stored.as_object() else {
        warn!("Stored settings are not a JSON object; falling back to defaults");
        return get_default_settings();
    };

    let mut merged = serde_json::to_value(get_default_settings())
        .expect("default settings serialize to a JSON object");

    for (key, value) in stored_map {
        let previous = merged
            .as_object_mut()
            .expect("merged settings stay an object")
            .insert(key.clone(), value.clone());
        if serde_json::from_value::<AppSettings>(merged.clone()).is_err() {
            // Log only the key: values may hold secrets (e.g. API keys).
            warn!("Dropping invalid settings field '{key}', keeping its default");
            let map = merged
                .as_object_mut()
                .expect("merged settings stay an object");
            match previous {
                Some(previous) => map.insert(key.clone(), previous),
                None => map.remove(key),
            };
        }
    }

    serde_json::from_value(merged).unwrap_or_else(|e| {
        warn!("Failed to reassemble salvaged settings ({e}); falling back to defaults");
        get_default_settings()
    })
}

fn apply_settings_migrations(
    settings: &mut AppSettings,
    settings_value: &serde_json::Value,
) -> bool {
    let mut updated = false;

    // One-time onboarding migration: users with an explicit selected model have
    // already made it through model selection. Users who merely have compatible
    // files on disk should still see onboarding.
    if settings_value.get("onboarding_completed").is_none() {
        settings.onboarding_completed = !settings.selected_model.is_empty();
        updated = true;
    }

    // One-time What's New migration: migrations only run on an existing store
    // (fresh installs stamp the current version via get_default_settings). A
    // missing key here means a user upgrading from before it existed — blank it
    // so they see the current release's What's New, mirroring the onboarding
    // migration's explicit first-run-vs-upgrade decision.
    if settings_value.get("whats_new_last_seen_version").is_none() {
        settings.whats_new_last_seen_version = String::new();
        updated = true;
    }

    let stored_schema_version = settings_value
        .get("settings_schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if stored_schema_version < 1 {
        // `transcribe_gpu_device` used to be a UI ordinal; it is now a
        // transcribe.cpp registry index. A positive legacy value can point at a
        // different GPU after CPU/accelerator/backend devices are included in
        // the registry, so reset ambiguous explicit selections to Auto once.
        if settings.transcribe_gpu_device > 0 {
            settings.transcribe_accelerator = TranscribeAcceleratorSetting::Auto;
            settings.transcribe_gpu_device = default_transcribe_gpu_device();
        }
        settings.settings_schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
        updated = true;
    }

    // One-time overlay migration (only while the new key is absent): the retired
    // overlay_position `none` meant "hide the overlay" → OverlayStyle::None; any
    // other position had it visible → Live. The position enum no longer has a
    // `none` variant (legacy "none" deserializes to Bottom via a serde alias), so
    // read the raw stored string to recover the old intent.
    if settings_value.get("overlay_style").is_none() {
        let was_hidden = settings_value
            .get("overlay_position")
            .and_then(|v| v.as_str())
            == Some("none");
        settings.overlay_style = if was_hidden {
            OverlayStyle::None
        } else {
            OverlayStyle::Live
        };
        updated = true;
    }

    updated
}

/// Write settings to disk using the debounced writer.
/// Also updates the in-memory cache immediately so that subsequent reads
/// (via `get_settings` or `get_settings_safe`) see the new value, eliminating
/// the read-modify-write race with the debounced disk writer.
pub fn write_settings(app: &AppHandle, settings: AppSettings) {
    let _ = safe_settings_operation("write_settings", || {
        // Update the in-memory cache first — mirror what write_settings_safe does.
        if let Some(cache) = app.try_state::<Arc<SettingsCache>>() {
            cache.update(settings.clone());
        }

        debug!(
            "[settings] write: active_model_id={:?} live_captions={} selected_language={} post_process_enabled={} overlay_position={:?}",
            settings.selected_model,
            settings.live_captions_enabled,
            settings.selected_language,
            settings.post_process_enabled,
            settings.overlay_position,
        );

        if let Some(writer) = app.try_state::<Arc<SettingsWriter>>() {
            let app_clone = app.clone();
            let writer = writer.inner().clone();
            tokio::spawn(async move {
                writer.write(app_clone, settings).await;
            });
        } else {
            write_settings_immediate(app, settings);
        }
    });
}

/// Write settings to disk immediately, bypassing the debounce timer.
/// Also updates the in-memory cache if available, to keep reads consistent.
pub fn write_settings_immediate(app: &AppHandle, mut settings: AppSettings) {
    let _ = safe_settings_operation("write_settings_immediate", || {
        // Update the in-memory cache so reads reflect the immediate write.
        if let Some(cache) = app.try_state::<Arc<SettingsCache>>() {
            cache.update(settings.clone());
        }

        let Some(store) = open_settings_store(app) else {
            error!("Cannot write settings: store initialization failed, settings not saved");
            return;
        };

        sanitize_floats(&mut settings);

        let Some(value) = settings_to_value(&settings) else {
            error!("Cannot write settings: serialization failed, settings not saved");
            return;
        };

        store.set("settings", value);
        match store.save() {
            Ok(()) => info!("[settings] flushed to disk"),
            Err(e) => error!("[settings] disk write FAILED: {e}"),
        }
    });
}

/// Flush any pending debounced settings to disk.
///
/// On exit (`RunEvent::ExitRequested`) we must ensure in-flight settings
/// are persisted. The previous implementation unconditionally called
/// `block_in_place` + `block_on` which can deadlock if the Tokio runtime
/// is already tearing down.
///
/// This version:
/// 1. Checks whether there is a pending write first (cheap, async lock).
/// 2. If no pending write, returns immediately — no blocking needed.
/// 3. If there IS a pending write, does a synchronous disk write via
///    `write_settings_immediate` (which calls `store.save()` synchronously)
///    wrapped in a `block_in_place` + `block_on` with a 2-second timeout
///    guard so a deadlocked runtime cannot hang the app on quit.
pub fn flush_settings(app: &AppHandle) {
    let _ = safe_settings_operation("flush_settings", || {
        let Some(writer) = app.try_state::<Arc<SettingsWriter>>() else {
            info!("[settings] exit flush: no writer registered, nothing to flush");
            return;
        };

        // Use block_in_place because we're in a sync context (Tauri run callback)
        // but need to check the async pending lock.
        let has_pending = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                writer.has_pending().await
            })
        });

        info!("[settings] exit flush: pending={}", has_pending);

        if !has_pending {
            info!("[settings] exit flush complete (nothing to write)");
            return;
        }

        // There is a pending write. Flush it with a timeout so we don't
        // hang if the runtime is shutting down.
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    writer.flush(app),
                )
                .await
            })
        });

        match result {
            Ok(()) => info!("[settings] exit flush complete"),
            Err(_) => {
                warn!("[settings] exit flush timed out after 2s — performing direct disk write as fallback");
                // Fallback: write directly from the pending settings if we can
                // grab them, then do a synchronous save.
                if let Some(cache) = app.try_state::<Arc<SettingsCache>>() {
                    write_settings_immediate(app, cache.get());
                    info!("[settings] exit flush complete (via direct fallback)");
                } else {
                    warn!("[settings] exit flush fallback: cache not available, settings may be lost");
                }
            }
        }
    });
}

pub fn get_bindings(app: &AppHandle) -> HashMap<String, ShortcutBinding> {
    let settings = get_settings_safe(app);
    settings.bindings
}

pub fn get_stored_binding(app: &AppHandle, id: &str) -> ShortcutBinding {
    let bindings = get_bindings(app);

    if let Some(binding) = bindings.get(id) {
        return binding.clone();
    }

    warn!(
        "Binding '{}' not found in current settings, falling back to defaults",
        id
    );
    let default_settings = get_default_settings();

    if let Some(default_binding) = default_settings.bindings.get(id) {
        return default_binding.clone();
    }

    warn!(
        "Binding '{}' not found in defaults either, creating fallback binding",
        id
    );
    ShortcutBinding {
        id: id.to_string(),
        name: id.to_string(),
        description: format!("{} shortcut", id),
        default_binding: String::new(),
        current_binding: String::new(),
    }
}

pub fn get_history_limit(app: &AppHandle) -> usize {
    let settings = get_settings_safe(app);
    settings.history_limit
}

pub fn get_recording_retention_period(app: &AppHandle) -> RecordingRetentionPeriod {
    let settings = get_settings_safe(app);
    settings.recording_retention_period
}

// ── Debounced settings writer ──

/// Default debounce interval in milliseconds.
pub const SETTINGS_DEBOUNCE_MS: u64 = 500;

/// State for the debounced settings writer.
pub struct SettingsWriter {
    pending: Mutex<Option<AppSettings>>,
    timer: Mutex<Option<JoinHandle<()>>>,
    debounce_ms: u64,
}

impl SettingsWriter {
    /// Create a new writer with the default debounce interval.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            timer: Mutex::new(None),
            debounce_ms: SETTINGS_DEBOUNCE_MS,
        }
    }

    /// Create a writer with a custom debounce interval (useful in tests).
    #[allow(dead_code)]
    pub fn with_debounce_ms(ms: u64) -> Self {
        Self {
            pending: Mutex::new(None),
            timer: Mutex::new(None),
            debounce_ms: ms,
        }
    }

    /// Check whether there is a pending write that hasn't been flushed yet.
    /// Used by `flush_settings` to skip the expensive blocking path when
    /// there's nothing to write.
    pub async fn has_pending(&self) -> bool {
        self.pending.lock().await.is_some()
    }

    /// Schedule a settings write. If a write is already pending the new value
    /// replaces it and the debounce timer is restarted.
    pub async fn write(&self, app: AppHandle, settings: AppSettings) {
        {
            let mut pending = self.pending.lock().await;
            *pending = Some(settings);
        }

        {
            let mut timer = self.timer.lock().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }

        let debounce_ms = self.debounce_ms;
        let new_handle = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(debounce_ms)).await;

            let Some(writer) = app.try_state::<Arc<SettingsWriter>>() else {
                warn!("SettingsWriter not available, skipping debounced flush");
                return;
            };
            writer.flush_inner(&app).await;
        });

        {
            let mut timer = self.timer.lock().await;
            *timer = Some(new_handle);
        }
    }

    /// Flush any pending settings to disk immediately.
    pub async fn flush(&self, app: &AppHandle) {
        {
            let mut timer = self.timer.lock().await;
            if let Some(handle) = timer.take() {
                handle.abort();
            }
        }
        self.flush_inner(app).await;
    }

    /// Internal flush: write the pending settings (if any) to the store.
    async fn flush_inner(&self, app: &AppHandle) {
        let maybe_settings = {
            let mut pending = self.pending.lock().await;
            pending.take()
        };

        if let Some(settings) = maybe_settings {
            debug!("[settings] flushing debounced settings to disk");
            write_settings_immediate(app, settings);
        }
    }
}

// ── Tests ──

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_disable_auto_submit() {
        let settings = get_default_settings();
        assert!(!settings.auto_submit);
        assert_eq!(settings.auto_submit_key, AutoSubmitKey::Enter);
        assert_eq!(
            settings.settings_schema_version,
            CURRENT_SETTINGS_SCHEMA_VERSION
        );
    }

    #[test]
    fn debug_output_redacts_api_keys() {
        let mut settings = get_default_settings();
        settings
            .post_process_api_keys
            .insert("openai".to_string(), "sk-proj-secret-key-12345".to_string());
        settings.post_process_api_keys.insert(
            "anthropic".to_string(),
            "sk-ant-secret-key-67890".to_string(),
        );
        settings
            .post_process_api_keys
            .insert("empty_provider".to_string(), "".to_string());

        let debug_output = format!("{:?}", settings);

        assert!(!debug_output.contains("sk-proj-secret-key-12345"));
        assert!(!debug_output.contains("sk-ant-secret-key-67890"));
        assert!(debug_output.contains("[REDACTED]"));
    }

    #[test]
    fn secret_map_debug_redacts_values() {
        let map = SecretMap(HashMap::from([("key".into(), "secret".into())]));
        let out = format!("{:?}", map);
        assert!(!out.contains("secret"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn empty_store_parses_with_defaults() {
        let settings: AppSettings = serde_json::from_value(serde_json::json!({}))
            .expect("all AppSettings fields need serde defaults");
        assert!(settings.push_to_talk);
        assert!(!settings.audio_feedback);
        assert!(settings.bindings.is_empty());
    }

    #[test]
    fn legacy_none_overlay_position_deserializes_to_bottom() {
        let raw = serde_json::json!({ "overlay_position": "none" });
        let position: OverlayPosition =
            serde_json::from_value(raw.get("overlay_position").unwrap().clone())
                .expect("legacy \"none\" should deserialize, not error");
        assert_eq!(position, OverlayPosition::Bottom);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn default_overlay_style_is_live_when_overlay_defaults_on() {
        let settings = get_default_settings();
        assert_eq!(settings.overlay_style, OverlayStyle::Live);
    }

    #[test]
    fn overlay_migration_keeps_disabled_overlay_off() {
        let mut settings = get_default_settings();

        // Legacy store: overlay was hidden via the retired position "none".
        let raw = serde_json::json!({
            "selected_model": "",
            "overlay_position": "none"
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.overlay_style, OverlayStyle::None);
    }

    #[test]
    fn overlay_migration_promotes_enabled_overlay_to_live() {
        let mut settings = get_default_settings();
        settings.overlay_position = OverlayPosition::Top;
        settings.overlay_style = OverlayStyle::Minimal;

        let raw = serde_json::json!({
            "selected_model": "",
            "overlay_position": "top"
        });

        assert!(apply_settings_migrations(&mut settings, &raw));
        assert_eq!(settings.overlay_style, OverlayStyle::Live);
        assert_eq!(settings.overlay_position, OverlayPosition::Top);
    }

    #[test]
    fn salvage_preserves_valid_fields_when_one_value_is_invalid() {
        let mut stored = serde_json::to_value(get_default_settings()).unwrap();
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "selected_model".into(),
            serde_json::json!("parakeet-tdt-0.6b-v3"),
        );
        map.insert("onboarding_completed".into(), serde_json::json!(true));
        // An enum variant this build doesn't know
        map.insert("sound_theme".into(), serde_json::json!("theremin"));
        stored["bindings"]["transcribe"]["current_binding"] = serde_json::json!("f13");

        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.selected_model, "parakeet-tdt-0.6b-v3");
        assert!(salvaged.onboarding_completed);
        assert_eq!(salvaged.bindings["transcribe"].current_binding, "f13");
        assert_eq!(salvaged.sound_theme, default_sound_theme());
    }
}