use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use log::{debug, error, info};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_specta::Event;

/// Database migrations for transcription history.
/// Each migration is applied in order. The library tracks which migrations
/// have been applied using SQLite's user_version pragma.
///
/// Note: For users upgrading from tauri-plugin-sql, migrate_from_tauri_plugin_sql()
/// converts the old _sqlx_migrations table tracking to the user_version pragma,
/// ensuring migrations don't re-run on existing databases.
static MIGRATIONS: &[M] = &[
    M::up(
        "CREATE TABLE IF NOT EXISTS transcription_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            saved BOOLEAN NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            transcription_text TEXT NOT NULL
        );",
    ),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_processed_text TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_prompt TEXT;"),
    M::up("ALTER TABLE transcription_history ADD COLUMN post_process_requested BOOLEAN NOT NULL DEFAULT 0;"),
];

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PaginatedHistory {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "action")]
pub enum HistoryUpdatePayload {
    #[serde(rename = "added")]
    Added { entry: HistoryEntry },
    #[serde(rename = "updated")]
    Updated { entry: HistoryEntry },
    #[serde(rename = "deleted")]
    Deleted { id: i64 },
    #[serde(rename = "toggled")]
    Toggled { id: i64 },
}

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
    pub post_process_requested: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct HistoryStats {
    pub total_dictations: i64,
    pub total_words: i64,
    pub this_week: i64,
    pub this_month: i64,
    pub daily_average: f64,
    pub current_streak_days: i64,
    pub longest_streak_days: i64,
    pub post_processed: i64,
    pub post_process_rate: i64,
    pub time_saved_minutes: f64,
    pub avg_wpm: Option<f64>,
}

/// Typing speed baseline and speaking rate used for the "time saved" estimate,
/// matching the convention used by Willow Voice (words * (1/40 - 1/150) minutes).
const TYPING_WPM: f64 = 40.0;
const SPEAKING_WPM: f64 = 150.0;

/// Days since epoch (UTC-midnight bucket) from a "YYYY-MM-DD" local date string.
fn day_string_to_epoch_days(day: &str) -> Option<i64> {
    use chrono::TimeZone;
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|ndt| chrono::Utc.from_utc_datetime(&ndt).timestamp() / 86_400)
}

/// Unix timestamp for local midnight on `date`.
/// Ambiguous DST folds pick the earlier mapping. A DST spring gap falls back
/// to 01:00 local so week/month bounds never collapse to the epoch.
fn local_midnight_timestamp(date: chrono::NaiveDate) -> i64 {
    use chrono::{LocalResult, TimeZone};
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .expect("midnight is a valid naive time");
    match Local.from_local_datetime(&midnight) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.timestamp(),
        LocalResult::None => {
            let shifted = date
                .and_hms_opt(1, 0, 0)
                .expect("01:00 is a valid naive time");
            match Local.from_local_datetime(&shifted) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.timestamp(),
                LocalResult::None => Local::now().timestamp(),
            }
        }
    }
}

/// Current and longest streak of consecutive days with at least one dictation.
/// The current streak stays alive if the last active day is today or yesterday.
fn compute_streaks(days: &[i64], today: i64) -> (i64, i64) {
    let mut current = 0i64;
    let start = if days.contains(&today) {
        Some(today)
    } else if days.contains(&(today - 1)) {
        Some(today - 1)
    } else {
        None
    };
    if let Some(mut d) = start {
        while days.contains(&d) {
            current += 1;
            d -= 1;
        }
    }

    let mut longest = 0i64;
    let mut run = 0i64;
    let mut prev: Option<i64> = None;
    for &d in days {
        run = if prev == Some(d - 1) { run + 1 } else { 1 };
        longest = longest.max(run);
        prev = Some(d);
    }

    (current, longest)
}

/// Whitespace-separated word count. Consecutive spaces and newlines do not
/// create extra words.
fn count_words(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

/// Final text for an entry: post-processed when present and non-empty, else raw.
fn final_text<'a>(transcription: &'a str, post_processed: Option<&'a str>) -> &'a str {
    match post_processed {
        Some(processed) if !processed.trim().is_empty() => processed,
        _ => transcription,
    }
}

/// Audio duration in seconds, read from the WAV header only.
fn wav_duration_seconds(path: &std::path::Path) -> Result<f64> {
    let reader = hound::WavReader::open(path)?;
    let sample_rate = reader.spec().sample_rate;
    if sample_rate == 0 {
        anyhow::bail!("WAV file has zero sample rate: {:?}", path);
    }
    Ok(reader.duration() as f64 / sample_rate as f64)
}

pub struct HistoryManager {
    app_handle: AppHandle,
    recordings_dir: PathBuf,
    db_path: PathBuf,
}

impl HistoryManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        // Create recordings directory in app data dir
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
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

        // Initialize database and run migrations synchronously
        manager.init_database()?;

        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        info!("Initializing database at {:?}", self.db_path);

        let mut conn = Connection::open(&self.db_path)?;

        // Handle migration from tauri-plugin-sql to rusqlite_migration
        // tauri-plugin-sql used _sqlx_migrations table, rusqlite_migration uses user_version pragma
        self.migrate_from_tauri_plugin_sql(&conn)?;

        // Create migrations object and run to latest version
        let migrations = Migrations::new(MIGRATIONS.to_vec());

        // Validate migrations in debug builds
        #[cfg(debug_assertions)]
        migrations.validate().expect("Invalid migrations");

        // Get current version before migration
        let version_before: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        debug!("Database version before migration: {}", version_before);

        // Apply any pending migrations
        migrations.to_latest(&mut conn)?;

        // Get version after migration
        let version_after: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version_after > version_before {
            info!(
                "Database migrated from version {} to {}",
                version_before, version_after
            );
        } else {
            debug!("Database already at latest version {}", version_after);
        }

        Ok(())
    }

    /// Migrate from tauri-plugin-sql's migration tracking to rusqlite_migration's.
    /// tauri-plugin-sql used a _sqlx_migrations table, while rusqlite_migration uses
    /// SQLite's user_version pragma. This function checks if the old system was in use
    /// and sets the user_version accordingly so migrations don't re-run.
    fn migrate_from_tauri_plugin_sql(&self, conn: &Connection) -> Result<()> {
        // Check if the old _sqlx_migrations table exists
        let has_sqlx_migrations: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_sqlx_migrations {
            return Ok(());
        }

        // Check current user_version
        let current_version: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if current_version > 0 {
            // Already migrated to rusqlite_migration system
            return Ok(());
        }

        // Get the highest version from the old migrations table
        let old_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if old_version > 0 {
            info!(
                "Migrating from tauri-plugin-sql (version {}) to rusqlite_migration",
                old_version
            );

            // Set user_version to match the old migration state
            conn.pragma_update(None, "user_version", old_version)?;

            // Optionally drop the old migrations table (keeping it doesn't hurt)
            // conn.execute("DROP TABLE IF EXISTS _sqlx_migrations", [])?;

            info!(
                "Migration tracking converted: user_version set to {}",
                old_version
            );
        }

        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn map_history_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
        Ok(HistoryEntry {
            id: row.get("id")?,
            file_name: row.get("file_name")?,
            timestamp: row.get("timestamp")?,
            saved: row.get("saved")?,
            title: row.get("title")?,
            transcription_text: row.get("transcription_text")?,
            post_processed_text: row.get("post_processed_text")?,
            post_process_prompt: row.get("post_process_prompt")?,
            post_process_requested: row.get("post_process_requested")?,
        })
    }

    pub fn recordings_dir(&self) -> &std::path::Path {
        &self.recordings_dir
    }

    /// Save a new history entry to the database.
    /// The WAV file should already have been written to the recordings directory.
    pub fn save_entry(
        &self,
        file_name: String,
        transcription_text: String,
        post_process_requested: bool,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        let timestamp = Utc::now().timestamp();
        let title = self.format_timestamp_title(timestamp);

        let conn = self.get_connection()?;
        conn.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &file_name,
                timestamp,
                false,
                &title,
                &transcription_text,
                &post_processed_text,
                &post_process_prompt,
                post_process_requested,
            ],
        )?;

        let entry = HistoryEntry {
            id: conn.last_insert_rowid(),
            file_name,
            timestamp,
            saved: false,
            title,
            transcription_text,
            post_processed_text,
            post_process_prompt,
            post_process_requested,
        };

        debug!("Saved history entry with id {}", entry.id);

        self.cleanup_old_entries()?;

        // Emit typed event for real-time frontend updates
        if let Err(e) = (HistoryUpdatePayload::Added {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(entry)
    }

    /// Update an existing history entry with new transcription results (used by retry).
    pub fn update_transcription(
        &self,
        id: i64,
        transcription_text: String,
        post_processed_text: Option<String>,
        post_process_prompt: Option<String>,
    ) -> Result<HistoryEntry> {
        let conn = self.get_connection()?;
        let updated = conn.execute(
            "UPDATE transcription_history
             SET transcription_text = ?1,
                 post_processed_text = ?2,
                 post_process_prompt = ?3
             WHERE id = ?4",
            params![
                transcription_text,
                post_processed_text,
                post_process_prompt,
                id
            ],
        )?;

        if updated == 0 {
            return Err(anyhow!("History entry {} not found", id));
        }

        let entry = conn
            .query_row(
                "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested
                 FROM transcription_history WHERE id = ?1",
                params![id],
                Self::map_history_entry,
            )?;

        debug!("Updated transcription for history entry {}", id);

        if let Err(e) = (HistoryUpdatePayload::Updated {
            entry: entry.clone(),
        })
        .emit(&self.app_handle)
        {
            error!("Failed to emit history-updated event: {}", e);
        }

        Ok(entry)
    }

    pub fn cleanup_old_entries(&self) -> Result<()> {
        let retention_period = crate::settings::get_recording_retention_period(&self.app_handle);

        match retention_period {
            crate::settings::RecordingRetentionPeriod::Never => {
                // Don't delete anything
                Ok(())
            }
            crate::settings::RecordingRetentionPeriod::PreserveLimit => {
                // Use the old count-based logic with history_limit
                let limit = crate::settings::get_history_limit(&self.app_handle);
                self.cleanup_by_count(limit)
            }
            _ => {
                // Use time-based logic
                self.cleanup_by_time(retention_period)
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

    pub async fn get_history_entries(
        &self,
        cursor: Option<i64>,
        limit: Option<usize>,
    ) -> Result<PaginatedHistory> {
        let conn = self.get_connection()?;
        let limit = limit.map(|l| l.min(100));

        let mut entries: Vec<HistoryEntry> = match (cursor, limit) {
            (Some(cursor_id), Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested
                     FROM transcription_history
                     WHERE id < ?1
                     ORDER BY id DESC
                     LIMIT ?2",
                )?;
                let result = stmt
                    .query_map(params![cursor_id, fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (None, Some(lim)) => {
                let fetch_count = (lim + 1) as i64;
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested
                     FROM transcription_history
                     ORDER BY id DESC
                     LIMIT ?1",
                )?;
                let result = stmt
                    .query_map(params![fetch_count], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
            (_, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_prompt, post_process_requested
                     FROM transcription_history
                     ORDER BY id DESC",
                )?;
                let result = stmt
                    .query_map([], Self::map_history_entry)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                result
            }
        };

        let has_more = limit.is_some_and(|lim| entries.len() > lim);
        if has_more {
            entries.pop();
        }

        Ok(PaginatedHistory { entries, has_more })
    }

    /// Compute aggregate usage stats over completed history entries.
    pub fn get_history_stats(&self) -> Result<HistoryStats> {
        let conn = self.get_connection()?;
        Self::get_history_stats_with_conn(&conn, &self.recordings_dir)
    }

    fn get_history_stats_with_conn(
        conn: &Connection,
        recordings_dir: &std::path::Path,
    ) -> Result<HistoryStats> {
        use chrono::{Datelike, Duration};

        let tx = conn.unchecked_transaction()?;
        let now_local = Local::now();
        let now = now_local.timestamp();

        // Start of the local week (Monday 00:00) and month (1st 00:00)
        let monday = now_local.date_naive()
            - Duration::days(now_local.weekday().num_days_from_monday() as i64);
        let week_start = local_midnight_timestamp(monday);
        let first_of_month = now_local
            .date_naive()
            .with_day(1)
            .unwrap_or_else(|| now_local.date_naive());
        let month_start = local_midnight_timestamp(first_of_month);

        let (total, this_week, this_month, post_processed) = tx.query_row(
            "SELECT
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN timestamp >= ?1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN timestamp >= ?2 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN NULLIF(TRIM(post_processed_text), '') IS NOT NULL THEN 1 ELSE 0 END), 0)
                FROM transcription_history
                WHERE transcription_text != ''",
            params![week_start, month_start],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )?;

        // Distinct local days with at least one dictation, for streaks and daily average
        let days: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT strftime('%Y-%m-%d', timestamp, 'unixepoch', 'localtime') AS day
             FROM transcription_history
             WHERE transcription_text != ''
             ORDER BY day",
            )?;
            let day_strings = stmt
                .query_map([], |row| row.get::<_, String>("day"))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            day_strings
                .into_iter()
                .filter_map(|day| day_string_to_epoch_days(&day))
                .collect()
        };

        // Today's bucket derived the same way as the DB rows (local date string)
        // so streak comparisons are consistent in every timezone.
        let today_string = now_local.format("%Y-%m-%d").to_string();
        let today_bucket = day_string_to_epoch_days(&today_string).unwrap_or_else(|| now / 86_400);

        let (current_streak_days, longest_streak_days) = compute_streaks(&days, today_bucket);

        let span_days = if days.is_empty() {
            1
        } else {
            (days[days.len() - 1] - days[0] + 1).max(1)
        };
        let daily_average = ((total as f64 / span_days as f64) * 10.0).round() / 10.0;

        let post_process_rate = if total > 0 {
            post_processed * 100 / total
        } else {
            0
        };

        // Average dictation speed from real audio durations (WAV headers only).
        // Entries whose recording was removed by retention are skipped.
        let rows: Vec<(String, String, Option<String>)> = {
            let mut stmt = tx.prepare(
                "SELECT file_name, transcription_text, post_processed_text
             FROM transcription_history
             WHERE transcription_text != ''",
            )?;
            let mapped = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>("file_name")?,
                    row.get::<_, String>("transcription_text")?,
                    row.get::<_, Option<String>>("post_processed_text")?,
                ))
            })?;
            let rows = mapped.collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        let mut total_words = 0i64;
        let mut words_with_audio = 0i64;
        let mut audio_seconds = 0f64;
        for (file_name, transcription, post_processed_text) in &rows {
            let words = count_words(final_text(transcription, post_processed_text.as_deref()));
            total_words += words;
            let path = recordings_dir.join(file_name);
            if let Ok(secs) = wav_duration_seconds(&path) {
                if secs > 0.0 {
                    words_with_audio += words;
                    audio_seconds += secs;
                }
            }
        }
        let time_saved_minutes = total_words as f64 * (1.0 / TYPING_WPM - 1.0 / SPEAKING_WPM);
        let avg_wpm = if audio_seconds > 0.0 {
            Some((words_with_audio as f64 / (audio_seconds / 60.0)).round())
        } else {
            None
        };

        let stats = HistoryStats {
            total_dictations: total,
            total_words,
            this_week,
            this_month,
            daily_average,
            current_streak_days,
            longest_streak_days,
            post_processed,
            post_process_rate,
            time_saved_minutes,
            avg_wpm,
        };
        tx.commit()?;
        Ok(stats)
    }

    #[cfg(test)]
    fn get_latest_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
             FROM transcription_history
             ORDER BY timestamp DESC
             LIMIT 1",
        )?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
    }

    /// Get the latest entry with non-empty transcription text.
    pub fn get_latest_completed_entry(&self) -> Result<Option<HistoryEntry>> {
        let conn = self.get_connection()?;
        Self::get_latest_completed_entry_with_conn(&conn)
    }

    fn get_latest_completed_entry_with_conn(conn: &Connection) -> Result<Option<HistoryEntry>> {
        let mut stmt = conn.prepare(
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
             FROM transcription_history
             WHERE transcription_text != ''
             ORDER BY timestamp DESC
             LIMIT 1",
        )?;

        let entry = stmt.query_row([], Self::map_history_entry).optional()?;
        Ok(entry)
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
        if let Err(e) = (HistoryUpdatePayload::Toggled { id }).emit(&self.app_handle) {
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
            "SELECT
                id,
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
             FROM transcription_history
             WHERE id = ?1",
        )?;

        let entry = stmt.query_row([id], Self::map_history_entry).optional()?;

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
        if let Err(e) = (HistoryUpdatePayload::Deleted { id }).emit(&self.app_handle) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE transcription_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_name TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                saved BOOLEAN NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                transcription_text TEXT NOT NULL,
                post_processed_text TEXT,
                post_process_prompt TEXT,
                post_process_requested BOOLEAN NOT NULL DEFAULT 0
            );",
        )
        .expect("create transcription_history table");
        conn
    }

    fn insert_entry(conn: &Connection, timestamp: i64, text: &str, post_processed: Option<&str>) {
        conn.execute(
            "INSERT INTO transcription_history (
                file_name,
                timestamp,
                saved,
                title,
                transcription_text,
                post_processed_text,
                post_process_prompt,
                post_process_requested
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                format!("handy-{}.wav", timestamp),
                timestamp,
                false,
                format!("Recording {}", timestamp),
                text,
                post_processed,
                Option::<String>::None,
                false,
            ],
        )
        .expect("insert history entry");
    }

    #[test]
    fn get_latest_entry_returns_none_when_empty() {
        let conn = setup_conn();
        let entry = HistoryManager::get_latest_entry_with_conn(&conn).expect("fetch latest entry");
        assert!(entry.is_none());
    }

    #[test]
    fn get_latest_entry_returns_newest_entry() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "first", None);
        insert_entry(&conn, 200, "second", Some("processed"));

        let entry = HistoryManager::get_latest_entry_with_conn(&conn)
            .expect("fetch latest entry")
            .expect("entry exists");

        assert_eq!(entry.timestamp, 200);
        assert_eq!(entry.transcription_text, "second");
        assert_eq!(entry.post_processed_text.as_deref(), Some("processed"));
    }

    #[test]
    fn get_latest_completed_entry_skips_empty_entries() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "completed", None);
        insert_entry(&conn, 200, "", None);

        let entry = HistoryManager::get_latest_completed_entry_with_conn(&conn)
            .expect("fetch latest completed entry")
            .expect("completed entry exists");

        assert_eq!(entry.timestamp, 100);
        assert_eq!(entry.transcription_text, "completed");
    }

    #[test]
    fn compute_streaks_handles_gaps_and_yesterday() {
        // No activity
        assert_eq!(compute_streaks(&[], 100), (0, 0));

        // Consecutive days ending yesterday: streak still current
        assert_eq!(compute_streaks(&[97, 98, 99], 100), (3, 3));

        // Consecutive days ending today
        assert_eq!(compute_streaks(&[98, 99, 100], 100), (3, 3));

        // Gap: current run (2) is shorter than nothing else; longest is 2
        assert_eq!(compute_streaks(&[97, 99, 100], 100), (2, 2));

        // Longest run in the past beats the current streak
        assert_eq!(compute_streaks(&[90, 91, 92, 93, 97, 99, 100], 100), (2, 4));

        // Activity stopped two days ago: no current streak
        assert_eq!(compute_streaks(&[97, 98], 100), (0, 2));
    }

    #[test]
    fn stats_count_words_and_exclude_failed_entries() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "hello world  foo\nbar", None);
        insert_entry(&conn, 200, "", None); // failed transcription, excluded
        insert_entry(&conn, 300, "raw text ignored", Some("one two")); // final text wins

        let stats = HistoryManager::get_history_stats_with_conn(
            &conn,
            std::path::Path::new("/nonexistent"),
        )
        .expect("compute stats");

        assert_eq!(stats.total_dictations, 2);
        assert_eq!(stats.total_words, 6); // 4 + 2
        assert_eq!(stats.post_processed, 1);
        assert_eq!(stats.post_process_rate, 50);
        assert_eq!(stats.avg_wpm, None);
        let expected_saved = 6.0 * (1.0 / TYPING_WPM - 1.0 / SPEAKING_WPM);
        assert!((stats.time_saved_minutes - expected_saved).abs() < 1e-9);
    }

    #[test]
    fn count_words_collapses_whitespace() {
        assert_eq!(count_words("hello world  foo\nbar"), 4);
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("  \n\t"), 0);
        assert_eq!(count_words("one two"), 2);
    }

    #[test]
    fn stats_streaks_from_consecutive_days() {
        use chrono::{Duration, TimeZone};

        let conn = setup_conn();
        let today = Local::now().date_naive();
        let noon_ts = |days_ago: i64| {
            let date = today - Duration::days(days_ago);
            let noon = date
                .and_hms_opt(12, 0, 0)
                .expect("noon is a valid naive time");
            match Local.from_local_datetime(&noon) {
                chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                    dt.timestamp()
                }
                chrono::LocalResult::None => date
                    .and_hms_opt(13, 0, 0)
                    .and_then(|shifted| Local.from_local_datetime(&shifted).single())
                    .map(|dt| dt.timestamp())
                    .expect("local noon mapping"),
            }
        };
        insert_entry(&conn, noon_ts(0), "today", None);
        insert_entry(&conn, noon_ts(1), "yesterday", None);
        insert_entry(&conn, noon_ts(2), "two days ago", None);
        insert_entry(&conn, noon_ts(4), "gap before", None);

        let stats = HistoryManager::get_history_stats_with_conn(
            &conn,
            std::path::Path::new("/nonexistent"),
        )
        .expect("compute stats");

        assert_eq!(stats.current_streak_days, 3);
        assert_eq!(stats.longest_streak_days, 3);
        assert_eq!(stats.total_dictations, 4);
    }

    #[test]
    fn empty_post_processed_text_does_not_count_as_processed() {
        let conn = setup_conn();
        insert_entry(&conn, 100, "hello", Some(""));

        let stats = HistoryManager::get_history_stats_with_conn(
            &conn,
            std::path::Path::new("/nonexistent"),
        )
        .expect("compute stats");

        assert_eq!(stats.total_dictations, 1);
        assert_eq!(stats.total_words, 1);
        assert_eq!(stats.post_processed, 0);
        assert_eq!(stats.post_process_rate, 0);
    }

    #[test]
    fn stats_avg_wpm_from_wav_durations() {
        let conn = setup_conn();
        let dir = std::env::temp_dir().join(format!(
            "handy-stats-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let wav_path = dir.join("handy-wpm.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav_path, spec).expect("create wav");
        for _ in 0..16_000 {
            writer.write_sample(0i16).expect("write sample");
        }
        writer.finalize().expect("finalize wav");

        // 5 words over 1 second of audio = 300 WPM
        conn.execute(
            "INSERT INTO transcription_history (
                file_name, timestamp, saved, title, transcription_text
            ) VALUES ('handy-wpm.wav', 100, false, 't', 'one two three four five')",
            [],
        )
        .expect("insert entry");

        let stats =
            HistoryManager::get_history_stats_with_conn(&conn, &dir).expect("compute stats");

        assert_eq!(stats.avg_wpm, Some(300.0));
        assert_eq!(stats.total_words, 5);

        std::fs::remove_file(&wav_path).ok();
        std::fs::remove_dir(&dir).ok();
    }
}
