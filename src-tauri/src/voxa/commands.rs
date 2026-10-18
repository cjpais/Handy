//! Tauri commands of the Voxa module.

use super::handy_import::{self, HandyImportOffer};
use super::stats::{self, VoxaStats};
use super::updates::{self, UpdateManager, UpdateStatus};
use super::VoxaStore;
use crate::settings::{get_settings, write_settings};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Longest custom word the UI accepts.
const MAX_WORD_CHARS: usize = 50;

/// Same normalization as the custom-words input of the settings screen:
/// quotes and angle brackets dropped, whitespace collapsed, trimmed.
pub fn normalize_word(word: &str) -> String {
    let stripped: String = word
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '"' | '\'') && !c.is_control())
        .collect();
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddWordError {
    Empty,
    TooLong,
}

impl std::fmt::Display for AddWordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddWordError::Empty => f.write_str("The word is empty"),
            AddWordError::TooLong => {
                write!(f, "The word is longer than {MAX_WORD_CHARS} characters")
            }
        }
    }
}

/// Adds `word` to `words` unless it is already there (case-insensitively).
/// Returns the stored spelling when it was added, `None` when it was already known.
pub fn add_word(words: &mut Vec<String>, word: &str) -> Result<Option<String>, AddWordError> {
    let word = normalize_word(word);
    if word.is_empty() {
        return Err(AddWordError::Empty);
    }
    if word.chars().count() > MAX_WORD_CHARS {
        return Err(AddWordError::TooLong);
    }
    let lowered = word.to_lowercase();
    if words.iter().any(|w| w.to_lowercase() == lowered) {
        return Ok(None);
    }
    words.push(word.clone());
    Ok(Some(word))
}

fn notify_custom_words_changed(app: &AppHandle, words: &[String]) {
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "custom_words", "value": words }),
    );
}

fn store(app: &AppHandle) -> Result<Arc<VoxaStore>, String> {
    app.try_state::<Arc<VoxaStore>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| "Statistics are unavailable (the database could not be opened)".to_string())
}

/// Statistics for the "Statistics" screen, computed in the system timezone.
#[tauri::command]
#[specta::specta]
pub async fn get_voxa_stats(app: AppHandle) -> Result<VoxaStats, String> {
    let store = store(&app)?;
    let custom_words = get_settings(&app).custom_words;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = store.connection().map_err(|e| format!("{e:#}"))?;
        stats::compute(&conn, chrono::Local::now(), &custom_words).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("Statistics task failed: {e}"))?
}

/// Deletes all recorded statistics. History and audio are not touched.
#[tauri::command]
#[specta::specta]
pub async fn voxa_clear_stats(app: AppHandle) -> Result<u32, String> {
    let store = store(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        store
            .clear()
            .map(|n| n as u32)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

/// Adds a term to the custom words. Returns the dictionary after the call; the
/// list is unchanged when the term was already in it.
#[tauri::command]
#[specta::specta]
pub fn voxa_add_dictionary_word(app: AppHandle, word: String) -> Result<Vec<String>, String> {
    let mut settings = get_settings(&app);
    let added = add_word(&mut settings.custom_words, &word).map_err(|e| e.to_string())?;
    let words = settings.custom_words.clone();
    if added.is_some() {
        write_settings(&app, settings);
        notify_custom_words_changed(&app, &words);
    }
    Ok(words)
}

/// The "import custom words from Handy" offer, if Handy is installed, has
/// words Voxa lacks and the owner has not answered yet.
#[tauri::command]
#[specta::specta]
pub fn voxa_get_handy_import(app: AppHandle) -> Option<HandyImportOffer> {
    if crate::portable::is_portable() {
        return None;
    }
    let settings = get_settings(&app);
    if settings.voxa.handy_import_handled {
        return None;
    }
    let data_dir = app.path().data_dir().ok()?;
    handy_import::find_offer(&data_dir, &settings.custom_words)
}

/// Adds Handy's new custom words and closes the offer. Returns the dictionary.
#[tauri::command]
#[specta::specta]
pub fn voxa_import_handy_words(app: AppHandle) -> Result<Vec<String>, String> {
    let data_dir = app.path().data_dir().map_err(|e| e.to_string())?;
    let mut settings = get_settings(&app);
    if let Some(offer) = handy_import::find_offer(&data_dir, &settings.custom_words) {
        for word in &offer.new_words {
            let _ = add_word(&mut settings.custom_words, word);
        }
    }
    settings.voxa.handy_import_handled = true;
    let words = settings.custom_words.clone();
    write_settings(&app, settings);
    notify_custom_words_changed(&app, &words);
    Ok(words)
}

/// Closes the import offer without importing anything.
#[tauri::command]
#[specta::specta]
pub fn voxa_dismiss_handy_import(app: AppHandle) {
    let mut settings = get_settings(&app);
    settings.voxa.handy_import_handled = true;
    write_settings(&app, settings);
}

/// The sentence handed to Whisper as context. Empty switches it off.
#[tauri::command]
#[specta::specta]
pub fn voxa_set_initial_prompt(app: AppHandle, prompt: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.voxa.initial_prompt = prompt.trim().chars().take(500).collect();
    write_settings(&app, settings);
    Ok(())
}

/// Whether custom words the model already saw are still repaired for case.
#[tauri::command]
#[specta::specta]
pub fn voxa_set_fix_custom_word_case(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.voxa.fix_custom_word_case = enabled;
    write_settings(&app, settings);
    Ok(())
}

/// Current update state (the UI also follows the `voxa-update-status` event).
#[tauri::command]
#[specta::specta]
pub fn voxa_update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(app.state::<UpdateManager>().status())
}

/// "Check for updates" button: reports errors and works with automatic checks off.
#[tauri::command]
#[specta::specta]
pub async fn voxa_check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(updates::check(&app, true).await)
}

/// "Update" button: downloads, verifies, installs and restarts. Returns only on
/// failure.
#[tauri::command]
#[specta::specta]
pub async fn voxa_install_update(app: AppHandle) -> Result<(), String> {
    updates::install(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_matches_the_settings_input() {
        assert_eq!(normalize_word("  Lang   Chain "), "Lang Chain");
        assert_eq!(normalize_word("a<b>\"c'd"), "abcd");
        assert_eq!(normalize_word("\t\n"), "");
    }

    #[test]
    fn add_word_appends_a_new_term() {
        let mut list = words(&["Docker"]);
        assert_eq!(
            add_word(&mut list, " Kafka ").unwrap(),
            Some("Kafka".to_string())
        );
        assert_eq!(list, words(&["Docker", "Kafka"]));
    }

    #[test]
    fn add_word_is_case_insensitive_and_idempotent() {
        let mut list = words(&["Docker"]);
        assert_eq!(add_word(&mut list, "docker").unwrap(), None);
        assert_eq!(add_word(&mut list, "DOCKER").unwrap(), None);
        assert_eq!(list, words(&["Docker"]));
    }

    #[test]
    fn add_word_rejects_empty_and_long_input() {
        let mut list = Vec::new();
        assert_eq!(add_word(&mut list, "  <> "), Err(AddWordError::Empty));
        let long = "x".repeat(51);
        assert_eq!(add_word(&mut list, &long), Err(AddWordError::TooLong));
        assert!(list.is_empty());
        assert!(add_word(&mut list, &"x".repeat(50)).unwrap().is_some());
    }
}
