//! Health report generator and log query commands for AI agent diagnostics.
//!
//! Provides a structured, AI-readable health summary on demand, plus a
//! filterable JSONL log query endpoint. These are the primary entry points
//! for an AI coding agent to quickly understand "is anything broken?".

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::io::BufRead;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::Manager;

use crate::session::SessionTracker;

static APP_START: OnceLock<Instant> = OnceLock::new();

fn uptime_instant() -> &'static Instant {
    APP_START.get_or_init(Instant::now)
}

// ── Types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Type)]
pub struct HealthReport {
    pub app_version: String,
    pub platform: String,
    pub uptime_secs: u64,
    pub total_sessions: u64,
    pub successful_sessions: u64,
    pub failed_sessions: u64,
    pub avg_transcription_ms: u64,
    pub p95_transcription_ms: u64,
    pub model_load_times: Vec<ModelLoadStat>,
    pub recent_errors: Vec<ErrorRecord>,
    pub usb_watchdog_cycles: u32,
    pub device_changes: u32,
    pub current_mic: String,
    pub current_model: String,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModelLoadStat {
    pub model_id: String,
    pub avg_load_ms: u64,
    pub load_count: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ErrorRecord {
    pub timestamp_ms: u64,
    pub event_type: String,
    pub message: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogFilter {
    pub event_type: Option<String>,
    pub session_id: Option<String>,
    pub level: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub event_type: String,
    pub session_id: Option<String>,
    pub raw_json: String,
}

// ── Helpers ───────────────────────────────────────────────────────

fn jsonl_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::portable::app_log_dir(app)
        .ok()
        .map(|dir| dir.join("handy-events.jsonl"))
}

fn read_jsonl_tail(path: &std::path::Path, max_lines: usize) -> Vec<String> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let reader = std::io::BufReader::new(file);
    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].to_vec()
}

fn extract_event_type(val: &serde_json::Value) -> String {
    val.get("evt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_session_id(val: &serde_json::Value) -> Option<String> {
    val.get("sid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_timestamp_ms(val: &serde_json::Value) -> u64 {
    val.get("ts")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(0)
}

// ── Health report generation ───────────────────────────────────────

pub fn generate_health_report(app: &tauri::AppHandle) -> HealthReport {
    let start = *uptime_instant();
    let uptime_secs = start.elapsed().as_secs();

    let version = app.package_info().version.to_string();
    let platform = if cfg!(target_os = "macos") {
        "macos".to_string()
    } else if cfg!(target_os = "windows") {
        "windows".to_string()
    } else if cfg!(target_os = "linux") {
        "linux".to_string()
    } else {
        "unknown".to_string()
    };

    let tracker = app.state::<std::sync::Arc<SessionTracker>>();
    let sessions = tracker.get_recent_sessions(200);
    let total_sessions = sessions.len() as u64;
    let successful_sessions = sessions.iter().filter(|s| s.success).count() as u64;
    let failed_sessions = total_sessions - successful_sessions;

    let mut transcription_durations: Vec<u64> = sessions
        .iter()
        .filter_map(|s| {
            s.phases_json
                .as_ref()
                .and_then(|json| serde_json::from_str::<HashMap<String, u64>>(json).ok())
                .and_then(|phases| phases.get("transcribing").copied())
        })
        .collect();
    transcription_durations.sort();

    let avg_transcription_ms = if transcription_durations.is_empty() {
        0
    } else {
        transcription_durations.iter().sum::<u64>() / transcription_durations.len() as u64
    };
    let p95_transcription_ms = percentile(&transcription_durations, 95);

    let settings = crate::settings::get_settings_safe(app);
    let current_mic = settings
        .selected_microphone
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let current_model = settings.selected_model.clone();
    let log_level = format!("{:?}", settings.log_level).to_lowercase();

    let jsonl_lines = jsonl_path(app)
        .map(|p| read_jsonl_tail(&p, 5000))
        .unwrap_or_default();

    let mut model_load_times_map: HashMap<String, (u64, u64)> = HashMap::new();
    let mut recent_errors: Vec<ErrorRecord> = Vec::new();
    let mut usb_watchdog_cycles: u32 = 0;
    let mut device_changes: u32 = 0;

    for line in &jsonl_lines {
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let evt = extract_event_type(&val);
        let level = val
            .get("lvl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match evt.as_str() {
            "ModelLoadCompleted" => {
                let model_id = val
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let duration_ms = val.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let entry = model_load_times_map
                    .entry(model_id.to_string())
                    .or_insert((0, 0));
                entry.0 += duration_ms;
                entry.1 += 1;
            }
            "UsbWatchdogCycle" => {
                usb_watchdog_cycles += 1;
            }
            "MicDeviceChanged" => {
                device_changes += 1;
            }
            _ => {}
        }

        if level == "error" || level == "warn" {
            let ts_ms = parse_timestamp_ms(&val);
            let msg = val
                .get("error")
                .and_then(|v| v.as_str())
                .or_else(|| val.get("reason").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            if !msg.is_empty() || !evt.is_empty() {
                recent_errors.push(ErrorRecord {
                    timestamp_ms: ts_ms,
                    event_type: evt.clone(),
                    message: msg,
                    session_id: extract_session_id(&val),
                });
            }
        }
    }

    recent_errors.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    recent_errors.truncate(20);

    let model_load_times: Vec<ModelLoadStat> = model_load_times_map
        .into_iter()
        .map(|(model_id, (total_ms, count))| ModelLoadStat {
            model_id,
            avg_load_ms: if count > 0 { total_ms / count } else { 0 },
            load_count: count,
        })
        .collect();

    HealthReport {
        app_version: version,
        platform,
        uptime_secs,
        total_sessions,
        successful_sessions,
        failed_sessions,
        avg_transcription_ms,
        p95_transcription_ms,
        model_load_times,
        recent_errors,
        usb_watchdog_cycles,
        device_changes,
        current_mic,
        current_model,
        log_level,
    }
}

fn percentile(sorted: &[u64], p: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p as usize) * sorted.len()).saturating_sub(1) / 100;
    sorted[idx.min(sorted.len() - 1)]
}

// ── Log query command ──────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn get_log_entries(app: tauri::AppHandle, filter: LogFilter) -> Result<Vec<LogEntry>, String> {
    let path = jsonl_path(&app).ok_or_else(|| "log directory not available".to_string())?;
    let lines = read_jsonl_tail(&path, 5000);
    let limit = filter.limit.unwrap_or(100) as usize;

    let mut entries: Vec<LogEntry> = Vec::new();

    for line in &lines {
        let val: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let evt = extract_event_type(&val);
        let level = val
            .get("lvl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ts = val
            .get("ts")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let sid = extract_session_id(&val);

        if let Some(ref filter_evt) = filter.event_type {
            if &evt != filter_evt {
                continue;
            }
        }
        if let Some(ref filter_sid) = filter.session_id {
            if sid.as_ref() != Some(filter_sid) {
                continue;
            }
        }
        if let Some(ref filter_level) = filter.level {
            if &level != filter_level {
                continue;
            }
        }

        entries.push(LogEntry {
            timestamp: ts,
            level,
            event_type: evt,
            session_id: sid,
            raw_json: line.clone(),
        });

        if entries.len() >= limit {
            break;
        }
    }

    Ok(entries)
}

// ── Tauri command wrapper ──────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub fn get_health_report(app: tauri::AppHandle) -> Result<HealthReport, String> {
    Ok(generate_health_report(&app))
}
