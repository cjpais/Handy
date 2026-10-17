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
}

impl Default for VoxaSettings {
    fn default() -> Self {
        Self {
            initial_prompt: defaults::DEFAULT_INITIAL_PROMPT.to_string(),
            fix_custom_word_case: true,
            handy_import_handled: false,
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
    if let Err(e) = store.record(&input) {
        log::error!("Failed to record dictation statistics: {e:#}");
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
            app.manage(std::sync::Arc::new(store));
        }
        Err(e) => log::error!("Statistics disabled: {e:#}"),
    }
}
