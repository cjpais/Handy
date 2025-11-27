use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use log::{debug, error};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_sql::{Migration, MigrationKind};

use crate::audio_toolkit::save_wav_file;

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct HistoryEntry {
    pub id: i64,
    pub file_name: String,
    pub timestamp: i64,
    pub saved: bool,
    pub title: String,
    pub transcription_text: String,
    pub post_processed_text: Option<String>,
    pub post_process_prompt: Option<String>,
}

pub struct HistoryManager {
    app_handle: AppHandle,
    recordings_dir: PathBuf,
    db_path: PathBuf,
}

impl HistoryManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create recordings directory in app data dir
        let app_data_dir = app_handle.path().app_data_dir()?;
        let recordings_dir = app_data_dir.join("recordings");
        let db_path = app_data_dir.join("history.db");

        // Ensure recordings directory exists
        if !recordings_dir.exists() {
            fs::create_dir_all(&recordings_dir)?;
            debug!("Created recordings directory: {:?}", recordings_dir);
        }

        let manager = Self {
            app_handle: app_handle.clone(),
            recordings_dir,
            db_path,
        };

        // Initialize database
        manager.init_database()?;

        Ok(manager)
    }

    pub fn get_migrations() -> Vec<Migration> {
        vec![
            Migration {
                version: 1,
                description: "create_transcription_history_table",
                sql: "CREATE TABLE IF NOT EXISTS transcription_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_name TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    saved BOOLEAN NOT NULL DEFAULT 0,
                    title TEXT NOT NULL,
                    transcription_text TEXT NOT NULL
                );",
                kind: MigrationKind::Up,
            },
            Migration {
                version: 2,
                description: "add_post_processed_text_column",
                sql: "ALTER TABLE transcription_history ADD COLUMN post_processed_text TEXT;",
                kind: MigrationKind::Up,
            },
            Migration {
                version: 3,
                description: "add_post_process_prompt_column",
                sql: "ALTER TABLE transcription_history ADD COLUMN post_process_prompt TEXT;",
                kind: MigrationKind::Up,
            },
        ]
    }

    fn init_database(&self) -> Result<()> {
        // Database initialization and migrations are handled by tauri-plugin-sql
        // via the preload configuration in tauri.conf.json
        debug!("Database path: {:?}", self.db_path);
        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    /// Save a transcription to history (both database and WAV file)
    pub async fn save_transcription(
        &self,
        audio_samples: Vec<f32>,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<()> {
        let timestamp = Utc::now().timestamp();
        let file_name = format!("handy-{}.wav", timestamp);
        let title = self.format_timestamp_title(timestamp);

        // Save WAV file
        let file_path = self.recordings_dir.join(&file_name);
        save_wav_file(file_path, &audio_samples).await?;

        // Save to database
        self.save_to_database(
            file_name,
            timestamp,
            title,
            transcription_text,
            post_processed_text,
            post_process_prompt,
        )?;

        // Clean up old entries
        self.cleanup_old_entries()?;

        // Emit history updated event
        if let Err(e) = self.app_handle.emit("history-updated", ()) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    fn save_to_database(
        &self,
        file_name: String,
        timestamp: i64,
        title: String,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![file_name, timestamp, false, title, transcription_text, post_processed_text, post_process_prompt],
        )?;

        debug!("Saved transcription to database");
        Ok(())
    }

    pub fn cleanup_old_entries(&self) -> Result<()> {
        let retention_period = crate::settings::get_recording_retention_period(&self.app_handle);

        match retention_period {
            crate::settings::RecordingRetentionPeriod::Never => {
                // Don't delete anything
                return Ok(());
            }
            crate::settings::RecordingRetentionPeriod::PreserveLimit => {
                // Use the old count-based logic with history_limit
                let limit = crate::settings::get_history_limit(&self.app_handle);
                return self.cleanup_by_count(limit);
            }
            _ => {
                // Use time-based logic
                return self.cleanup_by_time(retention_period);
            }
        }
    }

    fn delete_entries_and_files(&self, entries: &[(i64, String)]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }

        let conn = self.get_connection()?;
        let mut deleted_count = 0;

        for (id, file_name) in entries {
            // Delete database entry
            conn.execute(
                "DELETE FROM transcription_history WHERE id = ?1",
                params![id],
            )?;

            // Delete WAV file
            let file_path = self.recordings_dir.join(file_name);
            if file_path.exists() {
                if let Err(e) = fs::remove_file(&file_path) {
                    error!("Failed to delete WAV file {}: {}", file_name, e);
                } else {
                    debug!("Deleted old WAV file: {}", file_name);
                    deleted_count += 1;
                }
            }
        }

        Ok(deleted_count)
    }

    fn cleanup_by_count(&self, limit: usize) -> Result<()> {
        let conn = self.get_connection()?;

        // Get all entries that are not saved, ordered by timestamp desc
        let mut stmt = conn.prepare(
            "SELECT id, file_name FROM transcription_history WHERE saved = 0 ORDER BY timestamp DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
        })?;

        let mut entries: Vec<(i64, String)> = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        if entries.len() > limit {
            let entries_to_delete = &entries[limit..];
            let deleted_count = self.delete_entries_and_files(entries_to_delete)?;

            if deleted_count > 0 {
                debug!("Cleaned up {} old history entries by count", deleted_count);
            }
        }

        Ok(())
    }

    fn cleanup_by_time(
        &self,
        retention_period: crate::settings::RecordingRetentionPeriod,
    ) -> Result<()> {
        let conn = self.get_connection()?;

        // Calculate cutoff timestamp (current time minus retention period)
        let now = Utc::now().timestamp();
        let cutoff_timestamp = match retention_period {
            crate::settings::RecordingRetentionPeriod::Days3 => now - (3 * 24 * 60 * 60), // 3 days in seconds
            crate::settings::RecordingRetentionPeriod::Weeks2 => now - (2 * 7 * 24 * 60 * 60), // 2 weeks in seconds
            crate::settings::RecordingRetentionPeriod::Months3 => now - (3 * 30 * 24 * 60 * 60), // 3 months in seconds (approximate)
            _ => unreachable!("Should not reach here"),
        };

        // Get all unsaved entries older than the cutoff timestamp
        let mut stmt = conn.prepare(
            "SELECT id, file_name FROM transcription_history WHERE saved = 0 AND timestamp < ?1",
        )?;

        let rows = stmt.query_map(params![cutoff_timestamp], |row| {
            Ok((row.get::<_, i64>("id")?, row.get::<_, String>("file_name")?))
        })?;

        let mut entries_to_delete: Vec<(i64, String)> = Vec::new();
        for row in rows {
            entries_to_delete.push(row?);
        }

        let deleted_count = self.delete_entries_and_files(&entries_to_delete)?;

        if deleted_count > 0 {
            debug!(
                "Cleaned up {} old history entries based on retention period",
                deleted_count
            );
        }

        Ok(())
    }

    pub async fn get_history_entries(&self) -> Result<Vec<HistoryEntry>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt FROM transcription_history ORDER BY timestamp DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(HistoryEntry {
                id: row.get("id")?,
                file_name: row.get("file_name")?,
                timestamp: row.get("timestamp")?,
                saved: row.get("saved")?,
                title: row.get("title")?,
                transcription_text: row.get("transcription_text")?,
                post_processed_text: row.get("post_processed_text")?,
                post_process_prompt: row.get("post_process_prompt")?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        Ok(entries)
    }

    pub async fn toggle_saved_status(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;

        // Get current saved status
        let current_saved: bool = conn.query_row(
            "SELECT saved FROM transcription_history WHERE id = ?1",
            params![id],
            |row| row.get("saved"),
        )?;

        let new_saved = !current_saved;

        conn.execute(
            "UPDATE transcription_history SET saved = ?1 WHERE id = ?2",
            params![new_saved, id],
        )?;

        debug!("Toggled saved status for entry {}: {}", id, new_saved);

        // Emit history updated event
        if let Err(e) = self.app_handle.emit("history-updated", ()) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    pub fn get_audio_file_path(&self, file_name: &str) -> PathBuf {
        self.recordings_dir.join(file_name)
    }

    pub async fn get_entry_by_id(&self, id: i64) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt
             FROM transcription_history WHERE id = ?1",
        )?;

        let entry = stmt
            .query_row([id], |row| {
                Ok(HistoryEntry {
                    id: row.get("id")?,
                    file_name: row.get("file_name")?,
                    timestamp: row.get("timestamp")?,
                    saved: row.get("saved")?,
                    title: row.get("title")?,
                    transcription_text: row.get("transcription_text")?,
                    post_processed_text: row.get("post_processed_text")?,
                    post_process_prompt: row.get("post_process_prompt")?,
                })
            })
            .optional()?;

        Ok(entry)
    }

    pub async fn delete_entry(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;

        // Get the entry to find the file name
        if let Some(entry) = self.get_entry_by_id(id).await? {
            // Delete the audio file first
            let file_path = self.get_audio_file_path(&entry.file_name);
            if file_path.exists() {
                if let Err(e) = fs::remove_file(&file_path) {
                    error!("Failed to delete audio file {}: {}", entry.file_name, e);
                    // Continue with database deletion even if file deletion fails
                }
            }
        }

        // Delete from database
        conn.execute(
            "DELETE FROM transcription_history WHERE id = ?1",
            params![id],
        )?;

        debug!("Deleted history entry with id: {}", id);

        // Emit history updated event
        if let Err(e) = self.app_handle.emit("history-updated", ()) {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(())
    }

    fn format_timestamp_title(&self, timestamp: i64) -> String {
        if let Some(utc_datetime) = DateTime::from_timestamp(timestamp, 0) {
            // Convert UTC to local timezone
            let local_datetime = utc_datetime.with_timezone(&Local);
            local_datetime.format("%B %e, %Y - %l:%M%p").to_string()
        } else {
            format!("Recording {}", timestamp)
        }
    }
}
