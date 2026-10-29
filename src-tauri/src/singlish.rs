//! Singlish vocabulary layer: loads the user-editable JSON from the app data
//! directory (seeded from the bundled default) and applies it to transcripts.
//! The matching logic lives in the `singlish-tools` crate.

use log::{debug, warn};
use singlish_tools::{Vocabulary, DEFAULT_VOCABULARY_JSON};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::AppHandle;

const FILE_NAME: &str = "singlish_vocabulary.json";

struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    vocab: Vocabulary,
}

static STATE: Mutex<Option<Loaded>> = Mutex::new(None);

fn modified_time(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_vocab(path: &PathBuf) -> Vocabulary {
    match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|s| Vocabulary::from_json(&s).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => {
            warn!("Invalid Singlish vocabulary at {}: {}", path.display(), e);
            Vocabulary::bundled()
        }
    }
}

/// Returns the vocabulary file path, creating it from the bundled default on first run.
pub fn ensure_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = crate::portable::app_data_dir(app)
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        std::fs::write(&path, DEFAULT_VOCABULARY_JSON)
            .map_err(|e| format!("Failed to create vocabulary file: {}", e))?;
    }
    Ok(path)
}

/// Creates the file if needed and loads it. Called once at startup.
pub fn init(app: &AppHandle) {
    match ensure_file(app) {
        Ok(path) => {
            let vocab = read_vocab(&path);
            let modified = modified_time(&path);
            if let Ok(mut state) = STATE.lock() {
                *state = Some(Loaded {
                    path,
                    modified,
                    vocab,
                });
            }
        }
        Err(e) => warn!("Singlish vocabulary unavailable: {}", e),
    }
}

/// Applies the vocabulary, reloading the file if the user edited it. A no-op
/// before [`init`] has run.
pub fn apply(text: &str) -> String {
    let Ok(mut guard) = STATE.lock() else {
        return text.to_string();
    };
    let Some(loaded) = guard.as_mut() else {
        return text.to_string();
    };
    let modified = modified_time(&loaded.path);
    if modified != loaded.modified {
        debug!("Reloading Singlish vocabulary");
        loaded.vocab = read_vocab(&loaded.path);
        loaded.modified = modified;
    }
    singlish_tools::apply(text, &loaded.vocab)
}
