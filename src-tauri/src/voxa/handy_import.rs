//! Optional, read-only import of custom words from an installed Handy.
//!
//! Handy keeps its settings in `<data dir>/com.pais.handy/settings_store.json`
//! (a tauri-plugin-store file whose `settings` key holds the app settings).
//! Voxa has its own identifier, so it never shares that file; this module only
//! reads it, once, when the owner agrees.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

/// Handy's bundle identifier, the folder name under the user data directory.
const HANDY_IDENTIFIER: &str = "com.pais.handy";
const SETTINGS_FILE: &str = "settings_store.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct HandyImportOffer {
    /// Where the words were read from.
    pub path: String,
    /// Every custom word in Handy.
    pub words: Vec<String>,
    /// The ones Voxa does not have yet; the import adds exactly these.
    pub new_words: Vec<String>,
}

/// `<data_dir>/com.pais.handy/settings_store.json`, where `data_dir` is the
/// per-user application data directory (`~/Library/Application Support`).
pub fn handy_settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join(HANDY_IDENTIFIER).join(SETTINGS_FILE)
}

/// Custom words stored in a Handy settings file.
pub fn read_custom_words(path: &Path) -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    let words = value
        .get("settings")
        .and_then(|s| s.get("custom_words"))
        .and_then(|w| w.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|w| w.as_str())
                .map(super::commands::normalize_word)
                .filter(|w| !w.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(words)
}

/// Words of `incoming` that `existing` lacks, case-insensitively, without
/// duplicates and in their original order.
pub fn new_words(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> =
        existing.iter().map(|w| w.to_lowercase()).collect();
    incoming
        .iter()
        .filter(|w| seen.insert(w.to_lowercase()))
        .cloned()
        .collect()
}

/// The import offer, or `None` when there is no readable Handy file or nothing
/// new in it.
pub fn find_offer(data_dir: &Path, current_words: &[String]) -> Option<HandyImportOffer> {
    let path = handy_settings_path(data_dir);
    if !path.is_file() {
        return None;
    }
    let words = match read_custom_words(&path) {
        Ok(words) => words,
        Err(e) => {
            log::warn!("Could not read Handy settings for import: {e:#}");
            return None;
        }
    };
    let new = new_words(current_words, &words);
    if new.is_empty() {
        return None;
    }
    Some(HandyImportOffer {
        path: path.display().to_string(),
        words,
        new_words: new,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_handy(dir: &Path, json: &str) {
        let folder = dir.join(HANDY_IDENTIFIER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(SETTINGS_FILE), json).unwrap();
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn path_is_the_handy_identifier_folder() {
        let p = handy_settings_path(Path::new("/Users/me/Library/Application Support"));
        assert_eq!(
            p,
            Path::new("/Users/me/Library/Application Support/com.pais.handy/settings_store.json")
        );
    }

    #[test]
    fn reads_custom_words_from_the_settings_key() {
        let dir = TempDir::new().unwrap();
        write_handy(
            dir.path(),
            r#"{"settings": {"custom_words": ["Handy", " GPT-4 ", "", "a\"b"], "other": 1}}"#,
        );
        let words = read_custom_words(&handy_settings_path(dir.path())).unwrap();
        assert_eq!(words, strings(&["Handy", "GPT-4", "ab"]));
    }

    #[test]
    fn missing_or_malformed_data_yields_no_words_or_an_error() {
        let dir = TempDir::new().unwrap();
        write_handy(dir.path(), r#"{"settings": {}}"#);
        assert!(read_custom_words(&handy_settings_path(dir.path()))
            .unwrap()
            .is_empty());
        write_handy(dir.path(), "not json");
        assert!(read_custom_words(&handy_settings_path(dir.path())).is_err());
        assert!(read_custom_words(&dir.path().join("none.json")).is_err());
    }

    #[test]
    fn new_words_skip_known_terms_case_insensitively_and_duplicates() {
        let existing = strings(&["Docker", "rag"]);
        let incoming = strings(&["docker", "RAG", "Kafka", "kafka", "Redis"]);
        assert_eq!(
            new_words(&existing, &incoming),
            strings(&["Kafka", "Redis"])
        );
    }

    #[test]
    fn offer_exists_only_with_a_readable_file_and_new_words() {
        let dir = TempDir::new().unwrap();
        assert_eq!(find_offer(dir.path(), &[]), None, "no Handy installed");

        write_handy(
            dir.path(),
            r#"{"settings": {"custom_words": ["Kafka", "Docker"]}}"#,
        );
        let offer = find_offer(dir.path(), &strings(&["Docker"])).unwrap();
        assert_eq!(offer.words, strings(&["Kafka", "Docker"]));
        assert_eq!(offer.new_words, strings(&["Kafka"]));
        assert!(offer.path.ends_with("settings_store.json"));

        assert_eq!(
            find_offer(dir.path(), &strings(&["docker", "kafka"])),
            None,
            "nothing new"
        );

        write_handy(dir.path(), "garbage");
        assert_eq!(find_offer(dir.path(), &[]), None, "unreadable file");
    }

    #[test]
    fn reading_never_modifies_the_handy_file() {
        let dir = TempDir::new().unwrap();
        let json = r#"{"settings": {"custom_words": ["Kafka"]}}"#;
        write_handy(dir.path(), json);
        let path = handy_settings_path(dir.path());
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let _ = find_offer(dir.path(), &[]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
    }
}
