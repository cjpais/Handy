//! Voxa-specific backend: dictation statistics, owner defaults and the Handy
//! custom-words import. Everything lives here so upstream files only need small
//! hooks and `git merge upstream/main` stays cheap.

pub mod commands;
pub mod defaults;
pub mod handy_import;
pub mod stats;
pub mod store;
pub mod text_stats;
pub mod translit;
pub mod updates;
pub mod words;

use crate::audio_toolkit::WordReplacement;
use serde::{Deserialize, Serialize};
use specta::Type;

pub use store::{DictationInput, VoxaStore};

/// Match threshold of the case-repair pass: only words that equal a custom word
/// once case and spacing are ignored (edit distance 0) are replaced.
pub const CASE_ONLY_THRESHOLD: f64 = 0.005;

/// Voxa-only settings, nested under `AppSettings.voxa`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
#[serde(default)]
pub struct VoxaSettings {
    /// A natural sentence with your terms, passed to Whisper as context so it
    /// spells them right. Empty switches it off.
    pub initial_prompt: String,
    /// Repair the case of custom words the model already saw as a glossary
    /// (`langchain` -> `LangChain`).
    pub fix_custom_word_case: bool,
    /// The owner answered the "import custom words from Handy" offer.
    pub handy_import_handled: bool,
    /// The one-time switch of `update_checks_enabled` to on has been applied
    /// (see `settings::apply_settings_migrations`).
    pub updates_default_applied: bool,
    /// Filler words counted on the Words tab. A multi-word filler ("как бы")
    /// is looked up as a phrase of up to three words.
    pub filler_words: Vec<String>,
}

impl Default for VoxaSettings {
    fn default() -> Self {
        Self {
            initial_prompt: defaults::DEFAULT_INITIAL_PROMPT.to_string(),
            fix_custom_word_case: true,
            handy_import_handled: false,
            updates_default_applied: true,
            filler_words: defaults::default_filler_words(),
        }
    }
}

/// What the dictation pipeline knows about one transcription.
#[derive(Debug, Clone, Default)]
pub struct TranscriptionDetails {
    /// Final text after Handy's post-processing (custom words, fillers, ...).
    pub text: String,
    /// What the model returned, before any post-processing.
    pub raw_text: String,
    /// Custom-word replacements that changed the text.
    pub replacements: Vec<WordReplacement>,
    /// The model that ran.
    pub model_id: String,
    /// The language setting at the time (`auto` when automatic).
    pub language: String,
}

/// Records one finished dictation in the statistics store. Never fails the
/// dictation: problems are logged and dropped.
pub fn record_dictation(app: &tauri::AppHandle, input: DictationInput) {
    use tauri::Manager;
    let Some(store) = app.try_state::<std::sync::Arc<VoxaStore>>() else {
        return;
    };
    let store = store.inner().clone();
    // Off the dictation path: indexing the words is a few milliseconds of
    // database work the owner should never wait for.
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = store.record(&input) {
            log::error!("Failed to record dictation statistics: {e:#}");
        }
    });
}

/// The owner deleted a dictation from the history screen: take it out of the
/// statistics too (rows and word aggregates). Cleanup of old history by count or
/// age does not come through here, so the statistics outlive the history.
pub fn on_history_entry_deleted(app: &tauri::AppHandle, history_id: i64) {
    use tauri::Manager;
    let Some(store) = app.try_state::<std::sync::Arc<VoxaStore>>() else {
        return;
    };
    if let Err(e) = store.remove_by_history_id(history_id) {
        log::error!("Failed to remove dictation {history_id} from statistics: {e:#}");
    }
}

/// Opens the statistics store and puts it into Tauri state. A broken store
/// disables statistics but never blocks the app.
pub fn init(app: &tauri::AppHandle) {
    use tauri::Manager;
    let data_dir = match crate::portable::app_data_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            log::error!("Statistics disabled: no app data dir ({e})");
            return;
        }
    };
    let history_db = data_dir.join("history.db");
    match VoxaStore::open(&data_dir.join(store::DB_FILE_NAME), Some(&history_db)) {
        Ok(store) => {
            let store = std::sync::Arc::new(store);
            app.manage(store.clone());
            start_background_work(app, store);
        }
        Err(e) => log::error!("Statistics disabled: {e:#}"),
    }
}

/// Indexes the words of older dictations (once, in batches) and trims the phrase
/// table (weekly), off the main thread. Nothing else runs in the background.
fn start_background_work(app: &tauri::AppHandle, store: std::sync::Arc<VoxaStore>) {
    use tauri::Emitter;
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("voxa-words-index".to_string())
        .spawn(move || {
            let emit = |status: &words::index::BackfillStatus| {
                let _ = app.emit(words::BACKFILL_EVENT, status);
            };
            if let Err(e) = store.run_words_backfill(emit) {
                log::error!("Word statistics backfill stopped: {e:#}");
            }
            match store.cleanup() {
                Ok(0) => {}
                Ok(n) => log::info!("Dropped {n} old single-use phrases"),
                Err(e) => log::warn!("Phrase cleanup failed: {e:#}"),
            }
        });
    if let Err(e) = spawned {
        log::error!("Could not start the word statistics thread: {e}");
    }
}
