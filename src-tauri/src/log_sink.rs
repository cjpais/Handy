use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter};

pub const RING_CAPACITY: usize = 2000;
pub const LOG_EVENT: &str = "app://log";

static JWT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*").unwrap());
static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)access[_-]?token=[^\s&\]]+").unwrap());
static REFRESH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)refresh[_-]?token=[^\s&\]]+").unwrap());
// tauri-plugin-log formats records as: [YYYY-MM-DD][HH:MM:SS][target][LEVEL] message
// Strip this prefix so we don't double-display metadata already in LogEntry fields.
static LOG_PREFIX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\[\d{4}-\d{2}-\d{2}\]\[\d{2}:\d{2}:\d{2}\]\[[^\]]*\]\[[A-Z]+\]\s*").unwrap()
});

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogEntry {
    pub ts_ms: u64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub source: String,
}

pub struct LogRing {
    entries: Mutex<VecDeque<LogEntry>>,
    app_handle: Mutex<Option<AppHandle>>,
}

impl Default for LogRing {
    fn default() -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
            app_handle: Mutex::new(None),
        }
    }
}

impl LogRing {
    pub fn set_app_handle(&self, handle: AppHandle) {
        if let Ok(mut guard) = self.app_handle.lock() {
            *guard = Some(handle);
        }
    }

    pub fn push(&self, entry: LogEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= RING_CAPACITY {
                entries.pop_front();
            }
            entries.push_back(entry.clone());
        }
        if let Ok(guard) = self.app_handle.lock() {
            if let Some(handle) = guard.as_ref() {
                let _ = handle.emit(LOG_EVENT, &entry);
            }
        }
    }

    pub fn get_all(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

pub fn make_fern_dispatch(ring: Arc<LogRing>) -> tauri_plugin_log::fern::Dispatch {
    tauri_plugin_log::fern::Dispatch::new().chain(tauri_plugin_log::fern::Output::call(
        move |record| {
            let ts_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            let raw = record.args().to_string();
            let message = LOG_PREFIX_RE.replace(&raw, "").into_owned();
            let entry = LogEntry {
                ts_ms,
                level: record.level().to_string().to_lowercase(),
                target: record.target().to_string(),
                message: redact_sensitive(&message),
                source: "rust".to_string(),
            };

            ring.push(entry);
        },
    ))
}

fn redact_sensitive(message: &str) -> String {
    let s = JWT_RE.replace_all(message, "[REDACTED]");
    let s = TOKEN_RE.replace_all(&s, "access_token=[REDACTED]");
    let s = REFRESH_RE.replace_all(&s, "refresh_token=[REDACTED]");
    s.into_owned()
}
