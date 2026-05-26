//! Desktop wrapper around `handy_core::history::HistoryManager`.
//! Resolves the app-data dir via tauri's path resolver and bridges events
//! through the desktop's `EventSink`.

use anyhow::Result;
use handy_core::history::RetentionConfig;
use handy_platform::EventSink;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub use handy_core::history::{HistoryEntry, HistoryManager};

struct TauriEventSink(AppHandle);

impl EventSink for TauriEventSink {
    fn emit_json(&self, event_name: &str, payload: Value) {
        if let Err(e) = self.0.emit(event_name, payload) {
            log::warn!("emit {event_name}: {e}");
        }
    }
}

/// Construct a `HistoryManager` configured for the desktop app, using current settings.
pub fn open_for_app(app: &AppHandle) -> Result<HistoryManager> {
    let app_data_dir = crate::portable::app_data_dir(app)
        .map_err(|e| anyhow::anyhow!("app_data_dir: {e}"))?;

    let retention_config = RetentionConfig {
        period: crate::settings::get_recording_retention_period(app).into(),
        history_limit: crate::settings::get_history_limit(app),
    };

    let sink = Arc::new(TauriEventSink(app.clone()));
    HistoryManager::new(app_data_dir, sink, retention_config)
}
