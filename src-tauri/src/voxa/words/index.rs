//! The daily word aggregates and how they follow the dictations table.
//!
//! Every dictation adds its words, phrases and opener to the aggregates in the
//! same transaction that stores it. An explicit delete from the history screen
//! takes the contribution out again; cleanup of old history entries (by count or
//! age) does not touch the aggregates, because the statistics must outlive the
//! history. Dictations that existed before the word tables were created are
//! indexed once, in the background, in batches.

use super::analyze::{analyze, Analysis};
use super::stopwords::stop_set;
use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;

/// Schema of the word statistics (migration 2 of `voxa_stats.db`).
///
/// `word_forms` lists every form ever seen for a stem; the queries read
/// `form_daily`, which also has the counts. `ngram_daily.stop_only` marks
/// phrases made of stop words only, so "frequent phrases" can leave them out
/// while filler phrases such as "как бы" are still counted.
///
/// Key order matters for the write cost. A dictation touches a few dozen words
/// and about a hundred phrases; phrases are keyed by day first, so all of today's
/// rows share a few pages, while words keep `(stem, day)` order because the
/// queries group by word. There are no secondary indexes on the big tables: each
/// one would be another random page to dirty per row.
pub const SCHEMA: &str = "
CREATE TABLE word_forms (
    stem TEXT NOT NULL,
    form TEXT NOT NULL,
    PRIMARY KEY (stem, form)
) WITHOUT ROWID;
CREATE TABLE word_daily (
    stem TEXT NOT NULL,
    day TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (stem, day)
) WITHOUT ROWID;
CREATE TABLE form_daily (
    stem TEXT NOT NULL,
    form TEXT NOT NULL,
    day TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (stem, form, day)
) WITHOUT ROWID;
CREATE TABLE ngram_daily (
    day TEXT NOT NULL,
    ngram TEXT NOT NULL,
    n INTEGER NOT NULL,
    stop_only INTEGER NOT NULL DEFAULT 0,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, ngram)
) WITHOUT ROWID;
CREATE TABLE dictation_openers (
    dictation_id INTEGER PRIMARY KEY,
    opener1 TEXT,
    opener2 TEXT,
    day TEXT NOT NULL
);
CREATE INDEX idx_word_daily_day ON word_daily (day);
CREATE INDEX idx_dictation_openers_day ON dictation_openers (day);
";

pub const META_TARGET: &str = "words_backfill_target";
pub const META_CURSOR: &str = "words_backfill_cursor";
pub const META_DONE: &str = "words_backfill_done";
pub const META_NGRAM_CLEANUP: &str = "ngram_cleanup_at";

/// Dictations indexed per transaction while backfilling.
pub const BACKFILL_BATCH: i64 = 500;
/// Phrases seen once and older than this are dropped by the weekly cleanup.
pub const NGRAM_KEEP_DAYS: i64 = 90;
const CLEANUP_EVERY_SECONDS: i64 = 7 * 86_400;

/// Local calendar day of a unix timestamp, `YYYY-MM-DD`.
pub fn local_day(timestamp: i64) -> String {
    day_in(timestamp, &Local)
}

pub fn day_in<Tz: TimeZone>(timestamp: i64, tz: &Tz) -> String {
    tz.timestamp_opt(timestamp, 0)
        .single()
        .map(|d| d.date_naive().format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

/// What one text contributes to the aggregates.
#[derive(Debug, Default)]
struct Contribution {
    /// (stem, form) -> occurrences
    forms: HashMap<(String, String), i64>,
    stems: HashMap<String, i64>,
    /// ngram -> (n, stop_only, occurrences)
    ngrams: HashMap<String, (i64, bool, i64)>,
    openers: (Option<String>, Option<String>),
}

fn contribution(analysis: &Analysis) -> Contribution {
    let stops = stop_set();
    let mut c = Contribution::default();
    for t in &analysis.tokens {
        *c.forms.entry((t.stem.clone(), t.form.clone())).or_default() += 1;
        *c.stems.entry(t.stem.clone()).or_default() += 1;
    }
    for n in [2usize, 3] {
        for window in analysis.windows(n) {
            let text = window
                .iter()
                .map(|t| t.form.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let stop_only = window.iter().all(|t| stops.is_stop_form(&t.form));
            let entry = c.ngrams.entry(text).or_insert((n as i64, stop_only, 0));
            entry.2 += 1;
        }
    }
    c.openers = analysis.openers();
    c
}

fn apply(conn: &Connection, day: &str, c: &Contribution, sign: i64) -> Result<()> {
    for ((stem, form), count) in &c.forms {
        if sign > 0 {
            conn.prepare_cached("INSERT OR IGNORE INTO word_forms (stem, form) VALUES (?1, ?2)")?
                .execute(params![stem, form])?;
            conn.prepare_cached(
                "INSERT INTO form_daily (stem, form, day, count) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (stem, form, day) DO UPDATE SET count = count + excluded.count",
            )?
            .execute(params![stem, form, day, count])?;
        } else {
            conn.prepare_cached(
                "UPDATE form_daily SET count = count - ?4 WHERE stem = ?1 AND form = ?2 AND day = ?3",
            )?
            .execute(params![stem, form, day, count])?;
            conn.prepare_cached(
                "DELETE FROM form_daily WHERE stem = ?1 AND form = ?2 AND day = ?3 AND count <= 0",
            )?
            .execute(params![stem, form, day])?;
        }
    }
    for (stem, count) in &c.stems {
        if sign > 0 {
            conn.prepare_cached(
                "INSERT INTO word_daily (stem, day, count) VALUES (?1, ?2, ?3)
                 ON CONFLICT (stem, day) DO UPDATE SET count = count + excluded.count",
            )?
            .execute(params![stem, day, count])?;
        } else {
            conn.prepare_cached(
                "UPDATE word_daily SET count = count - ?3 WHERE stem = ?1 AND day = ?2",
            )?
            .execute(params![stem, day, count])?;
            conn.prepare_cached(
                "DELETE FROM word_daily WHERE stem = ?1 AND day = ?2 AND count <= 0",
            )?
            .execute(params![stem, day])?;
        }
    }
    for (ngram, (n, stop_only, count)) in &c.ngrams {
        if sign > 0 {
            conn.prepare_cached(
                "INSERT INTO ngram_daily (ngram, n, day, stop_only, count) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (day, ngram) DO UPDATE SET count = count + excluded.count",
            )?
            .execute(params![ngram, n, day, *stop_only as i64, count])?;
        } else {
            conn.prepare_cached(
                "UPDATE ngram_daily SET count = count - ?3 WHERE ngram = ?1 AND day = ?2",
            )?
            .execute(params![ngram, day, count])?;
            conn.prepare_cached(
                "DELETE FROM ngram_daily WHERE ngram = ?1 AND day = ?2 AND count <= 0",
            )?
            .execute(params![ngram, day])?;
        }
    }
    Ok(())
}

/// Adds one dictation's words to the aggregates. Run it in the transaction that
/// stores the dictation.
pub fn add_dictation(conn: &Connection, dictation_id: i64, day: &str, text: &str) -> Result<()> {
    let c = contribution(&analyze(text));
    if c.stems.is_empty() {
        return Ok(());
    }
    apply(conn, day, &c, 1)?;
    conn.prepare_cached(
        "INSERT OR REPLACE INTO dictation_openers (dictation_id, opener1, opener2, day)
         VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![dictation_id, c.openers.0, c.openers.1, day])?;
    Ok(())
}

/// Takes one dictation's words out again. A dictation without an opener row was
/// never indexed (no words, or the backfill has not reached it) and is left alone.
pub fn remove_dictation(conn: &Connection, dictation_id: i64, text: &str) -> Result<bool> {
    let day: Option<String> = conn
        .query_row(
            "SELECT day FROM dictation_openers WHERE dictation_id = ?1",
            [dictation_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(day) = day else {
        return Ok(false);
    };
    apply(conn, &day, &contribution(&analyze(text)), -1)?;
    conn.execute(
        "DELETE FROM dictation_openers WHERE dictation_id = ?1",
        [dictation_id],
    )?;
    Ok(true)
}

/// Empties every word table (used by "clear statistics").
pub fn clear_all(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM word_forms; DELETE FROM word_daily; DELETE FROM form_daily;
         DELETE FROM ngram_daily; DELETE FROM dictation_openers;",
    )?;
    Ok(())
}

// ---------------------------------------------------------------- meta

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM voxa_meta WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO voxa_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

// ------------------------------------------------------------ backfill

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackfillState {
    /// Nothing to index (or not started).
    Idle,
    Running,
    Done,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct BackfillStatus {
    pub state: BackfillState,
    pub processed: u32,
    pub total: u32,
}

impl BackfillStatus {
    pub fn idle() -> Self {
        Self {
            state: BackfillState::Idle,
            processed: 0,
            total: 0,
        }
    }
}

/// Called once after the migrations: remembers which dictations the backfill
/// has to index (the ones that exist now); later ones are indexed as they are
/// recorded. Returns the status to start from.
pub fn prepare_backfill(conn: &Connection) -> Result<BackfillStatus> {
    if meta_get(conn, META_DONE)?.is_some() {
        return Ok(BackfillStatus {
            state: BackfillState::Done,
            processed: 0,
            total: 0,
        });
    }
    if meta_get(conn, META_TARGET)?.is_none() {
        let target: i64 =
            conn.query_row("SELECT COALESCE(MAX(id), 0) FROM dictations", [], |r| r.get(0))?;
        meta_set(conn, META_TARGET, &target.to_string())?;
        meta_set(conn, META_CURSOR, "0")?;
    }
    let (target, cursor) = backfill_position(conn)?;
    if cursor >= target {
        meta_set(conn, META_DONE, "1")?;
        return Ok(BackfillStatus {
            state: BackfillState::Done,
            processed: 0,
            total: 0,
        });
    }
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dictations WHERE id <= ?1",
        [target],
        |r| r.get(0),
    )?;
    let processed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dictations WHERE id <= ?1",
        [cursor],
        |r| r.get(0),
    )?;
    Ok(BackfillStatus {
        state: BackfillState::Running,
        processed: processed as u32,
        total: total as u32,
    })
}

fn backfill_position(conn: &Connection) -> Result<(i64, i64)> {
    let read = |key| -> Result<i64> {
        Ok(meta_get(conn, key)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    };
    Ok((read(META_TARGET)?, read(META_CURSOR)?))
}

/// Indexes the next batch. Returns `false` when nothing is left. The cursor
/// moves in the same transaction as the aggregates, so an interrupted run
/// resumes exactly where it stopped and nothing is counted twice.
pub fn backfill_batch(conn: &Connection, batch: i64) -> Result<bool> {
    let (target, cursor) = backfill_position(conn)?;
    if meta_get(conn, META_DONE)?.is_some() || cursor >= target {
        meta_set(conn, META_DONE, "1")?;
        return Ok(false);
    }
    let tx = conn.unchecked_transaction()?;
    let rows: Vec<(i64, i64, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, created_at, final_text FROM dictations
             WHERE id > ?1 AND id <= ?2 ORDER BY id LIMIT ?3",
        )?;
        let mapped = stmt.query_map(params![cursor, target, batch], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        mapped.collect::<std::result::Result<_, _>>()?
    };
    let mut last = cursor;
    for (id, created_at, text) in &rows {
        add_dictation(&tx, *id, &local_day(*created_at), text)
            .with_context(|| format!("index dictation {id}"))?;
        last = *id;
    }
    if rows.len() < batch as usize {
        // reached the end of what existed when the tables were created
        last = target;
        meta_set(&tx, META_DONE, "1")?;
    }
    meta_set(&tx, META_CURSOR, &last.to_string())?;
    tx.commit()?;
    Ok(last < target)
}

// ------------------------------------------------------------- cleanup

/// Drops phrases that were seen exactly once, ever, and are older than
/// [`NGRAM_KEEP_DAYS`]: three-word phrases are mostly unique, so they would
/// otherwise make up most of the table. Runs at most once a week. Phrases with
/// two or more uses are kept whatever their age.
pub fn cleanup_ngrams(conn: &Connection, now_unix: i64, today: NaiveDate) -> Result<usize> {
    let last: i64 = meta_get(conn, META_NGRAM_CLEANUP)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if now_unix - last < CLEANUP_EVERY_SECONDS {
        return Ok(0);
    }
    let cutoff = (today - Duration::days(NGRAM_KEEP_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let removed = conn.execute(
        "DELETE FROM ngram_daily
         WHERE day < ?1 AND count = 1
           AND ngram IN (SELECT ngram FROM ngram_daily GROUP BY ngram HAVING SUM(count) = 1)",
        [cutoff],
    )?;
    meta_set(conn, META_NGRAM_CLEANUP, &now_unix.to_string())?;
    Ok(removed)
}
