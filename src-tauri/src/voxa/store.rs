//! The dictation statistics database (`voxa_stats.db`).
//!
//! It is a file of its own with its own migration chain, separate from Handy's
//! `history.db`: Handy's history cleanup (by count or age) deletes rows and
//! audio, and must not take the statistics with it; and upstream appends its
//! own history migrations by position, which Voxa columns would collide with.

use super::text_stats::count_words;
use super::words::index::{self, BackfillStatus};
use crate::audio_toolkit::WordReplacement;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const DB_FILE_NAME: &str = "voxa_stats.db";

const META_HISTORY_BACKFILL: &str = "history_backfill";

/// Each migration is applied once, in order; `PRAGMA user_version` tracks them.
static MIGRATIONS: &[M] = &[
    M::up(
    "CREATE TABLE dictations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        -- unix seconds, UTC
        created_at INTEGER NOT NULL,
        -- transcription_history.id of the same dictation, when one was saved
        history_id INTEGER,
        -- recorded audio length; NULL for rows imported from old history
        duration_ms INTEGER,
        -- end of recording to finished text (model + post-processing)
        transcribe_ms INTEGER,
        model_id TEXT,
        language TEXT,
        -- model output before any post-processing (custom words, fillers, ...)
        raw_text TEXT NOT NULL DEFAULT '',
        -- the text that was inserted
        final_text TEXT NOT NULL DEFAULT '',
        -- [{\"from\": \"...\", \"to\": \"...\"}]: custom-word replacements that fired
        replacements_json TEXT NOT NULL DEFAULT '[]',
        word_count INTEGER NOT NULL,
        latin_word_count INTEGER NOT NULL,
        cyrillic_word_count INTEGER NOT NULL,
        -- 'live' or 'backfill' (imported from history.db)
        source TEXT NOT NULL DEFAULT 'live'
    );
    CREATE INDEX idx_dictations_created_at ON dictations (created_at);
    CREATE UNIQUE INDEX idx_dictations_history_id ON dictations (history_id)
        WHERE history_id IS NOT NULL;
    CREATE TABLE voxa_meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );"),
    // word statistics (voxa/words/index.rs)
    M::up(index::SCHEMA),
];

/// One finished dictation, as the pipeline reports it.
#[derive(Debug, Clone, Default)]
pub struct DictationInput {
    /// Unix seconds, UTC.
    pub created_at: i64,
    /// Id of the saved history entry, if there is one.
    pub history_id: Option<i64>,
    pub duration_ms: Option<i64>,
    pub transcribe_ms: Option<i64>,
    pub model_id: String,
    pub language: String,
    pub raw_text: String,
    pub final_text: String,
    pub replacements: Vec<WordReplacement>,
}

pub struct VoxaStore {
    path: PathBuf,
    /// One long-lived connection for the dictation pipeline's writes. Its page
    /// cache stays warm between dictations, which is what keeps recording a
    /// dictation (hundreds of small upserts) in the millisecond range.
    writer: Mutex<Connection>,
    /// Progress of the one-time word indexing of older dictations.
    backfill: Mutex<BackfillStatus>,
}

impl VoxaStore {
    /// Opens (creating it if needed) the database at `path`, applies pending
    /// migrations and, once, imports word counts from an existing Handy
    /// history database. The history file is only ever read.
    pub fn open(path: &Path, history_db: Option<&Path>) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let mut conn = open_connection(path)?;
        let store = Self {
            path: path.to_path_buf(),
            writer: Mutex::new(open_connection(path)?),
            backfill: Mutex::new(BackfillStatus::idle()),
        };
        // WAL: the background indexing and a new dictation never block each other.
        conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))
            .context("enable WAL")?;
        Migrations::new(MIGRATIONS.to_vec())
            .to_latest(&mut conn)
            .context("migrate the statistics database")?;
        if let Some(history_db) = history_db {
            if let Err(e) = backfill_from_history(&conn, history_db) {
                // Statistics still work without the old rows.
                log::warn!("Could not import history into statistics: {e:#}");
            }
        }
        match index::prepare_backfill(&conn) {
            Ok(status) => *store.backfill.lock().expect("backfill status") = status,
            Err(e) => log::warn!("Could not prepare word statistics: {e:#}"),
        }
        Ok(store)
    }

    pub fn backfill_status(&self) -> BackfillStatus {
        self.backfill.lock().expect("backfill status").clone()
    }

    fn set_backfill_status(&self, status: BackfillStatus) {
        *self.backfill.lock().expect("backfill status") = status;
    }

    /// Indexes the words of dictations that existed before the word tables did,
    /// batch by batch. Safe to call again: it resumes at the saved cursor.
    /// `on_progress` gets the status after every batch.
    pub fn run_words_backfill(&self, mut on_progress: impl FnMut(&BackfillStatus)) -> Result<()> {
        let conn = self.connection()?;
        let mut status = self.backfill_status();
        if status.state != index::BackfillState::Running {
            return Ok(());
        }
        loop {
            let more = index::backfill_batch(&conn, index::BACKFILL_BATCH)?;
            status.processed = (status.processed + index::BACKFILL_BATCH as u32).min(status.total);
            if !more {
                status.state = index::BackfillState::Done;
                status.processed = status.total;
            }
            self.set_backfill_status(status.clone());
            on_progress(&status);
            if !more {
                return Ok(());
            }
            // a pause so a long backfill never starves a dictation's own write
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Folds the days up to yesterday into the all-time totals (once per day).
    /// Returns the date the totals now reach.
    pub fn ensure_rolled(&self) -> Result<String> {
        let conn = self.writer.lock().expect("statistics writer");
        index::roll_up(&conn, &index::yesterday(chrono::Local::now().date_naive()))?;
        index::rolled_through(&conn)
    }

    /// Weekly housekeeping of the phrase table.
    pub fn cleanup(&self) -> Result<usize> {
        let conn = self.connection()?;
        let now = chrono::Local::now();
        index::cleanup_ngrams(&conn, now.timestamp(), now.date_naive())
    }

    /// Takes one dictation out of the statistics, words included: the owner
    /// deleted it from the history. Returns whether a row was removed.
    pub fn remove_by_history_id(&self, history_id: i64) -> Result<bool> {
        let mut conn = self.writer.lock().expect("statistics writer");
        let tx = conn.transaction()?;
        let row: Option<(i64, String)> = tx
            .query_row(
                "SELECT id, final_text FROM dictations WHERE history_id = ?1",
                [history_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, text)) = row else {
            return Ok(false);
        };
        index::remove_dictation(&tx, id, &text)?;
        tx.execute("DELETE FROM dictations WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(true)
    }

    /// A fresh connection for reading (and for the one-off background jobs).
    pub fn connection(&self) -> Result<Connection> {
        open_connection(&self.path)
    }

    /// Stores one dictation. Returns the row id, or `None` when the text has no
    /// words (nothing worth counting).
    pub fn record(&self, input: &DictationInput) -> Result<Option<i64>> {
        let mut conn = self.writer.lock().expect("statistics writer");
        let tx = conn.transaction()?;
        let id = insert_dictation(&tx, input, "live")?;
        if let Some(id) = id {
            index::add_dictation(&tx, id, &index::local_day(input.created_at), &input.final_text)?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Deletes every recorded dictation. The "history imported" marker stays,
    /// so cleared statistics are not re-imported on the next start.
    pub fn clear(&self) -> Result<usize> {
        let mut conn = self.writer.lock().expect("statistics writer");
        let tx = conn.transaction()?;
        let removed = tx.execute("DELETE FROM dictations", [])?;
        index::clear_all(&tx)?;
        // whatever the backfill had left is gone with the rows
        index::meta_set(&tx, index::META_DONE, "1")?;
        tx.commit()?;
        self.set_backfill_status(BackfillStatus {
            state: index::BackfillState::Done,
            processed: 0,
            total: 0,
        });
        Ok(removed)
    }
}

fn open_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(3))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // 16 MB of page cache and a checkpoint every 4000 pages: a dictation dirties
    // pages all over the aggregate tables, and the defaults would flush and sync
    // after every dictation or two.
    conn.pragma_update(None, "cache_size", -16_384)?;
    conn.pragma_update(None, "wal_autocheckpoint", 4_000)?;
    // Reads through a memory map: scanning the aggregates for a long period
    // would otherwise cost one system call per 4 KB page.
    conn.pragma_update(None, "mmap_size", 268_435_456i64)?;
    Ok(conn)
}

fn insert_dictation(
    conn: &Connection,
    input: &DictationInput,
    source: &str,
) -> Result<Option<i64>> {
    let counts = count_words(&input.final_text);
    if counts.words == 0 {
        return Ok(None);
    }
    let replacements_json =
        serde_json::to_string(&input.replacements).context("serialize replacements")?;
    let changed = conn.execute(
        "INSERT OR IGNORE INTO dictations (
            created_at, history_id, duration_ms, transcribe_ms, model_id, language,
            raw_text, final_text, replacements_json,
            word_count, latin_word_count, cyrillic_word_count, source
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            input.created_at,
            input.history_id,
            input.duration_ms,
            input.transcribe_ms,
            non_empty(&input.model_id),
            non_empty(&input.language),
            input.raw_text,
            input.final_text,
            replacements_json,
            counts.words,
            counts.latin,
            counts.cyrillic,
            source,
        ],
    )?;
    Ok((changed > 0).then(|| conn.last_insert_rowid()))
}

fn non_empty(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// One-time import of the dictations already in Handy's `history.db`: their
/// time and word counts (the rest was never recorded). Rows are matched by
/// `history_id`, so running it twice adds nothing.
fn backfill_from_history(conn: &Connection, history_db: &Path) -> Result<()> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM voxa_meta WHERE key = ?1",
            [META_HISTORY_BACKFILL],
            |row| row.get(0),
        )
        .ok();
    if done.is_some() || !history_db.exists() {
        return Ok(());
    }

    let history = Connection::open_with_flags(history_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open {} read-only", history_db.display()))?;
    let has_table: bool = history.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'transcription_history'",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(());
    }

    let rows: Vec<(i64, i64, String)> = {
        let mut stmt = history.prepare(
            "SELECT id, timestamp, transcription_text
             FROM transcription_history
             WHERE transcription_text != ''
             ORDER BY id",
        )?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        mapped.collect::<std::result::Result<_, _>>()?
    };

    let tx = conn.unchecked_transaction()?;
    for (history_id, timestamp, text) in rows {
        let input = DictationInput {
            created_at: timestamp,
            history_id: Some(history_id),
            final_text: text,
            ..Default::default()
        };
        insert_dictation(&tx, &input, "backfill")?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO voxa_meta (key, value) VALUES (?1, '1')",
        [META_HISTORY_BACKFILL],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Handy's history database as it looks after its four migrations
    /// (`user_version = 4`), with a few rows. Mirrors `managers/history.rs`.
    pub(crate) fn legacy_history_db(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE transcription_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_name TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                saved BOOLEAN NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                transcription_text TEXT NOT NULL
            );
            ALTER TABLE transcription_history ADD COLUMN post_processed_text TEXT;
            ALTER TABLE transcription_history ADD COLUMN post_process_prompt TEXT;
            ALTER TABLE transcription_history ADD COLUMN post_process_requested BOOLEAN NOT NULL DEFAULT 0;
            PRAGMA user_version = 4;
            INSERT INTO transcription_history (file_name, timestamp, saved, title, transcription_text, post_processed_text, post_process_requested)
              VALUES ('handy-1.wav', 1700000000, 0, 'one', 'Привет мир, это GPT-4o', NULL, 0),
                     ('handy-2.wav', 1700000100, 1, 'two', 'пустая запись без текста', 'LLM output', 1),
                     ('handy-3.wav', 1700000200, 0, 'three', '', NULL, 0),
                     ('handy-4.wav', 1700000300, 0, 'four', 'one two three', NULL, 0);",
        )
        .unwrap();
    }

    /// A statistics database as the first Voxa build left it (migration 1 only,
    /// no word tables), holding `(created_at, final_text)` rows.
    pub(crate) fn legacy_v1_db(path: &Path, rows: &[(i64, &str)]) {
        let mut conn = Connection::open(path).unwrap();
        Migrations::new(vec![MIGRATIONS[0].clone()])
            .to_latest(&mut conn)
            .unwrap();
        for (created_at, text) in rows {
            let input = DictationInput {
                created_at: *created_at,
                final_text: text.to_string(),
                raw_text: text.to_string(),
                ..Default::default()
            };
            insert_dictation(&conn, &input, "live").unwrap();
        }
    }

    fn dump_history(path: &Path) -> Vec<(i64, String, i64, String, Option<String>)> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, file_name, timestamp, transcription_text, post_processed_text
                 FROM transcription_history ORDER BY id",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn input(text: &str) -> DictationInput {
        DictationInput {
            created_at: 1_700_001_000,
            history_id: None,
            duration_ms: Some(4200),
            transcribe_ms: Some(900),
            model_id: "whisper-large-v3-turbo".to_string(),
            language: "ru".to_string(),
            raw_text: text.to_lowercase(),
            final_text: text.to_string(),
            replacements: vec![],
        }
    }

    #[test]
    fn creates_schema_on_a_fresh_database() {
        let dir = TempDir::new().unwrap();
        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
        let conn = store.connection().unwrap();
        assert_eq!(count(&conn, "PRAGMA user_version"), 2);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM dictations"), 0);
        // every column of PLAN.md 5.1 exists
        let cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('dictations')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for expected in [
            "duration_ms",
            "transcribe_ms",
            "model_id",
            "language",
            "raw_text",
            "final_text",
            "replacements_json",
            "word_count",
            "latin_word_count",
            "cyrillic_word_count",
        ] {
            assert!(cols.iter().any(|c| c == expected), "missing {expected}");
        }
    }

    #[test]
    fn reopening_keeps_data_and_does_not_rerun_migrations() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(DB_FILE_NAME);
        let store = VoxaStore::open(&path, None).unwrap();
        store.record(&input("Привет мир")).unwrap();
        drop(store);
        let store = VoxaStore::open(&path, None).unwrap();
        let conn = store.connection().unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM dictations"), 1);
        assert_eq!(count(&conn, "PRAGMA user_version"), 2);
    }

    #[test]
    fn migrates_next_to_a_legacy_history_database_without_touching_it() {
        let dir = TempDir::new().unwrap();
        let history = dir.path().join("history.db");
        legacy_history_db(&history);
        let before = dump_history(&history);
        let version_before = {
            let c = Connection::open(&history).unwrap();
            count(&c, "PRAGMA user_version")
        };

        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), Some(&history)).unwrap();

        // Old data is byte-for-byte what it was, schema version included.
        assert_eq!(dump_history(&history), before);
        let c = Connection::open(&history).unwrap();
        assert_eq!(count(&c, "PRAGMA user_version"), version_before);
        assert_eq!(count(&c, "SELECT COUNT(*) FROM transcription_history"), 4);

        // Rows with text were imported with their counts; the empty one was not.
        let conn = store.connection().unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM dictations"), 3);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM dictations WHERE source = 'backfill'"
            ),
            3
        );
        let (created, words, latin, cyr): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT created_at, word_count, latin_word_count, cyrillic_word_count
                 FROM dictations WHERE history_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!((created, words, latin, cyr), (1_700_000_000, 4, 1, 3));
        // No audio length or timing is known for imported rows.
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM dictations WHERE duration_ms IS NULL AND transcribe_ms IS NULL"
            ),
            3
        );
    }

    #[test]
    fn history_import_is_idempotent_and_survives_clear() {
        let dir = TempDir::new().unwrap();
        let history = dir.path().join("history.db");
        legacy_history_db(&history);
        let path = dir.path().join(DB_FILE_NAME);

        let store = VoxaStore::open(&path, Some(&history)).unwrap();
        drop(store);
        let store = VoxaStore::open(&path, Some(&history)).unwrap();
        assert_eq!(
            count(
                &store.connection().unwrap(),
                "SELECT COUNT(*) FROM dictations"
            ),
            3
        );

        assert_eq!(store.clear().unwrap(), 3);
        drop(store);
        let store = VoxaStore::open(&path, Some(&history)).unwrap();
        assert_eq!(
            count(
                &store.connection().unwrap(),
                "SELECT COUNT(*) FROM dictations"
            ),
            0,
            "cleared statistics must not come back from history"
        );
    }

    #[test]
    fn missing_history_file_is_not_an_error_and_does_not_set_the_marker() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(DB_FILE_NAME);
        let missing = dir.path().join("nope.db");
        let store = VoxaStore::open(&path, Some(&missing)).unwrap();
        let conn = store.connection().unwrap();
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM voxa_meta WHERE key = 'history_backfill'"
            ),
            0
        );

        // the history appears later: it is imported then
        legacy_history_db(&missing);
        drop(store);
        let store = VoxaStore::open(&path, Some(&missing)).unwrap();
        assert_eq!(
            count(
                &store.connection().unwrap(),
                "SELECT COUNT(*) FROM dictations"
            ),
            3
        );
    }

    #[test]
    fn live_record_stores_every_field_and_counts() {
        let dir = TempDir::new().unwrap();
        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
        let mut rec = input("Настроил RAG-пайплайн на LangChain");
        rec.history_id = Some(42);
        rec.raw_text = "Настроил rag-пайплайн на langchain".to_string();
        rec.replacements = vec![WordReplacement {
            from: "langchain".to_string(),
            to: "LangChain".to_string(),
        }];
        let id = store.record(&rec).unwrap().expect("recorded");
        let conn = store.connection().unwrap();
        #[allow(clippy::type_complexity)]
        let row: (
            i64,
            i64,
            i64,
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            String,
        ) = conn
            .query_row(
                "SELECT created_at, duration_ms, transcribe_ms, model_id, language, raw_text,
                        final_text, replacements_json, word_count, latin_word_count,
                        cyrillic_word_count, source
                 FROM dictations WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                        r.get(11)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, 1_700_001_000);
        assert_eq!((row.1, row.2), (4200, 900));
        assert_eq!(row.3, "whisper-large-v3-turbo");
        assert_eq!(row.4, "ru");
        assert_eq!(row.5, "Настроил rag-пайплайн на langchain");
        assert_eq!(row.6, "Настроил RAG-пайплайн на LangChain");
        assert_eq!(row.7, r#"[{"from":"langchain","to":"LangChain"}]"#);
        assert_eq!((row.8, row.9, row.10), (4, 2, 2));
        assert_eq!(row.11, "live");
    }

    #[test]
    fn empty_and_punctuation_only_text_is_not_recorded() {
        let dir = TempDir::new().unwrap();
        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
        assert_eq!(store.record(&input("")).unwrap(), None);
        assert_eq!(store.record(&input("... — ?!")).unwrap(), None);
        assert_eq!(
            count(
                &store.connection().unwrap(),
                "SELECT COUNT(*) FROM dictations"
            ),
            0
        );
    }

    #[test]
    fn a_history_entry_is_recorded_at_most_once() {
        let dir = TempDir::new().unwrap();
        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
        let mut rec = input("раз два");
        rec.history_id = Some(7);
        assert!(store.record(&rec).unwrap().is_some());
        assert_eq!(store.record(&rec).unwrap(), None);
    }
}
