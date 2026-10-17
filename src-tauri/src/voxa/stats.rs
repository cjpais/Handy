//! Aggregates of the dictation statistics store: counts per period, per-day
//! words, per-model latency and dictionary candidates.
//!
//! Timestamps are stored in UTC; "today", "this week" (Monday to Sunday) and the
//! day buckets follow the calendar of the timezone passed to [`compute`], which
//! is the system timezone in the app.

use super::translit;
use crate::audio_toolkit::WordReplacement;
use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{BTreeMap, HashMap};

/// Latency numbers need at least this many timed dictations per model.
pub const MIN_LATENCY_SAMPLES: usize = 20;
/// Days shown in the words-per-day chart, today included.
pub const DAILY_WINDOW_DAYS: i64 = 30;
/// How many candidates of each kind are returned.
const MAX_CANDIDATES: usize = 10;
/// How many recent dictations are scanned for candidates.
const CANDIDATE_SCAN_ROWS: i64 = 2000;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Type)]
pub struct PeriodStats {
    pub dictations: u32,
    pub words: u32,
    /// Minutes of recorded audio (rows without a known length add nothing).
    pub minutes: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct DailyWords {
    /// Local calendar day, `YYYY-MM-DD`.
    pub date: String,
    pub words: u32,
    pub dictations: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct ModelStats {
    /// Registry id of the model; empty for imported history without one.
    pub model_id: String,
    pub dictations: u32,
    pub words: u32,
    /// Share of Latin words, `None` without words.
    pub latin_share: Option<f64>,
    /// Dictations with a measured latency.
    pub latency_samples: u32,
    pub avg_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    /// `latency_samples >= MIN_LATENCY_SAMPLES`; below it the UI says "not enough data".
    pub enough_latency_data: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// A custom-word replacement that fired: the model got the word wrong.
    Replacement,
    /// The model wrote a known technical term in Cyrillic.
    Translit,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct DictionaryCandidate {
    pub kind: CandidateKind,
    /// The spelling to add to the dictionary.
    pub term: String,
    pub count: u32,
    /// How the model wrote it instead, most frequent first.
    pub variants: Vec<String>,
    pub in_dictionary: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct VoxaStats {
    pub today: PeriodStats,
    /// Monday to Sunday of the current week.
    pub week: PeriodStats,
    pub all_time: PeriodStats,
    pub avg_words_per_dictation: f64,
    pub avg_seconds_per_dictation: Option<f64>,
    pub latin_share: Option<f64>,
    pub daily: Vec<DailyWords>,
    pub models: Vec<ModelStats>,
    pub candidates: Vec<DictionaryCandidate>,
}

/// Nearest-rank percentile of an ascending slice: the smallest value with at
/// least `pct` percent of the data at or below it.
pub fn percentile(sorted: &[i64], pct: u32) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    let rank = (pct as usize * n).div_ceil(100).clamp(1, n);
    Some(sorted[rank - 1])
}

fn share(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

pub fn compute<Tz: TimeZone>(
    conn: &Connection,
    now: DateTime<Tz>,
    custom_words: &[String],
) -> Result<VoxaStats> {
    let tz = now.timezone();
    let today = now.date_naive();
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let week_end = week_start + Duration::days(6);

    // ---- all time ----
    let (dictations, words, latin, duration_sum, timed): (i64, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(word_count), 0), COALESCE(SUM(latin_word_count), 0),
                    COALESCE(SUM(duration_ms), 0), COUNT(duration_ms)
             FROM dictations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
    let all_time = PeriodStats {
        dictations: dictations as u32,
        words: words as u32,
        minutes: duration_sum as f64 / 60_000.0,
    };

    // ---- today / this week / last 30 days, bucketed by local calendar day ----
    // 31 days of rows plus a day of slack for timezone offsets.
    let since = now.timestamp() - (DAILY_WINDOW_DAYS + 2) * 86_400;
    let mut stmt = conn.prepare(
        "SELECT created_at, word_count, duration_ms FROM dictations WHERE created_at >= ?1",
    )?;
    let rows = stmt.query_map([since], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;

    let mut per_day: BTreeMap<NaiveDate, (u32, u32)> = BTreeMap::new();
    let mut today_stats = PeriodStats::default();
    let mut week_stats = PeriodStats::default();
    for row in rows {
        let (created_at, word_count, duration_ms) = row?;
        let Some(local) = tz.timestamp_opt(created_at, 0).single() else {
            continue;
        };
        let day = local.date_naive();
        let minutes = duration_ms.unwrap_or(0) as f64 / 60_000.0;
        let entry = per_day.entry(day).or_default();
        entry.0 += word_count as u32;
        entry.1 += 1;
        if day == today {
            today_stats.dictations += 1;
            today_stats.words += word_count as u32;
            today_stats.minutes += minutes;
        }
        if day >= week_start && day <= week_end {
            week_stats.dictations += 1;
            week_stats.words += word_count as u32;
            week_stats.minutes += minutes;
        }
    }
    let daily = (0..DAILY_WINDOW_DAYS)
        .rev()
        .map(|back| {
            let day = today - Duration::days(back);
            let (words, dictations) = per_day.get(&day).copied().unwrap_or_default();
            DailyWords {
                date: day.format("%Y-%m-%d").to_string(),
                words,
                dictations,
            }
        })
        .collect();

    Ok(VoxaStats {
        today: today_stats,
        week: week_stats,
        all_time,
        avg_words_per_dictation: if dictations > 0 {
            words as f64 / dictations as f64
        } else {
            0.0
        },
        avg_seconds_per_dictation: (timed > 0).then(|| duration_sum as f64 / timed as f64 / 1000.0),
        latin_share: share(latin, words),
        daily,
        models: model_stats(conn)?,
        candidates: candidates(conn, custom_words)?,
    })
}

fn model_stats(conn: &Connection) -> Result<Vec<ModelStats>> {
    let mut latencies: HashMap<String, Vec<i64>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT COALESCE(model_id, ''), transcribe_ms FROM dictations
             WHERE transcribe_ms IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (model, ms) = row?;
            latencies.entry(model).or_default().push(ms);
        }
    }

    let mut stmt = conn.prepare(
        "SELECT COALESCE(model_id, ''), COUNT(*), COALESCE(SUM(word_count), 0),
                COALESCE(SUM(latin_word_count), 0)
         FROM dictations GROUP BY COALESCE(model_id, '')",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (model_id, dictations, words, latin) = row?;
        let mut samples = latencies.remove(&model_id).unwrap_or_default();
        samples.sort_unstable();
        let avg = (!samples.is_empty())
            .then(|| samples.iter().sum::<i64>() as f64 / samples.len() as f64);
        out.push(ModelStats {
            model_id,
            dictations: dictations as u32,
            words: words as u32,
            latin_share: share(latin, words),
            latency_samples: samples.len() as u32,
            avg_latency_ms: avg,
            p95_latency_ms: percentile(&samples, 95).map(|v| v as f64),
            enough_latency_data: samples.len() >= MIN_LATENCY_SAMPLES,
        });
    }
    out.sort_by(|a, b| {
        b.dictations
            .cmp(&a.dictations)
            .then_with(|| a.model_id.cmp(&b.model_id))
    });
    Ok(out)
}

fn in_dictionary(custom_words: &[String], term: &str) -> bool {
    let wanted = term.to_lowercase();
    custom_words.iter().any(|w| w.to_lowercase() == wanted)
}

/// Most frequent first; ties alphabetical, so the order is stable.
fn ranked(counts: HashMap<String, u32>) -> Vec<(String, u32)> {
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

fn candidates(conn: &Connection, custom_words: &[String]) -> Result<Vec<DictionaryCandidate>> {
    let mut out = replacement_candidates(conn, custom_words)?;
    out.extend(translit_candidates(conn, custom_words)?);
    Ok(out)
}

fn replacement_candidates(
    conn: &Connection,
    custom_words: &[String],
) -> Result<Vec<DictionaryCandidate>> {
    struct Acc {
        count: u32,
        spellings: HashMap<String, u32>,
        variants: HashMap<String, u32>,
    }
    let mut by_term: HashMap<String, Acc> = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT replacements_json FROM dictations
         WHERE replacements_json != '[]' ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([CANDIDATE_SCAN_ROWS], |r| r.get::<_, String>(0))?;
    for row in rows {
        let Ok(list) = serde_json::from_str::<Vec<WordReplacement>>(&row?) else {
            continue;
        };
        for replacement in list {
            let acc = by_term
                .entry(replacement.to.to_lowercase())
                .or_insert_with(|| Acc {
                    count: 0,
                    spellings: HashMap::new(),
                    variants: HashMap::new(),
                });
            acc.count += 1;
            *acc.spellings.entry(replacement.to).or_default() += 1;
            *acc.variants.entry(replacement.from).or_default() += 1;
        }
    }

    let mut out: Vec<DictionaryCandidate> = by_term
        .into_values()
        .map(|acc| {
            let term = ranked(acc.spellings)
                .into_iter()
                .next()
                .map(|(t, _)| t)
                .unwrap_or_default();
            DictionaryCandidate {
                kind: CandidateKind::Replacement,
                in_dictionary: in_dictionary(custom_words, &term),
                count: acc.count,
                variants: ranked(acc.variants)
                    .into_iter()
                    .take(3)
                    .map(|(v, _)| v)
                    .collect(),
                term,
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.term.cmp(&b.term)));
    out.truncate(MAX_CANDIDATES);
    Ok(out)
}

fn translit_candidates(
    conn: &Connection,
    custom_words: &[String],
) -> Result<Vec<DictionaryCandidate>> {
    let mut by_term: HashMap<&'static str, (u32, HashMap<String, u32>)> = HashMap::new();

    let mut stmt = conn.prepare("SELECT final_text FROM dictations ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map([CANDIDATE_SCAN_ROWS], |r| r.get::<_, String>(0))?;
    for row in rows {
        for (term, spelling) in translit::find_in_text(&row?) {
            let entry = by_term.entry(term.term).or_default();
            entry.0 += 1;
            *entry.1.entry(spelling).or_default() += 1;
        }
    }

    let mut out: Vec<DictionaryCandidate> = by_term
        .into_iter()
        .map(|(term, (count, spellings))| DictionaryCandidate {
            kind: CandidateKind::Translit,
            term: term.to_string(),
            count,
            variants: ranked(spellings)
                .into_iter()
                .take(3)
                .map(|(v, _)| v)
                .collect(),
            in_dictionary: in_dictionary(custom_words, term),
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.term.cmp(&b.term)));
    out.truncate(MAX_CANDIDATES);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxa::store::{DictationInput, VoxaStore, DB_FILE_NAME};
    use chrono::{FixedOffset, Utc};
    use tempfile::TempDir;

    fn moscow() -> FixedOffset {
        FixedOffset::east_opt(3 * 3600).unwrap()
    }

    /// Unix time of a Moscow wall-clock moment.
    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        moscow()
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    fn open() -> (TempDir, VoxaStore) {
        let dir = TempDir::new().unwrap();
        let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
        (dir, store)
    }

    fn rec(store: &VoxaStore, created_at: i64, text: &str) {
        store
            .record(&DictationInput {
                created_at,
                duration_ms: Some(6_000),
                transcribe_ms: Some(800),
                model_id: "turbo".to_string(),
                language: "ru".to_string(),
                raw_text: text.to_string(),
                final_text: text.to_string(),
                ..Default::default()
            })
            .unwrap();
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<i64> = (1..=100).collect();
        assert_eq!(percentile(&v, 95), Some(95));
        assert_eq!(percentile(&v, 50), Some(50));
        assert_eq!(percentile(&v, 100), Some(100));
        let twenty: Vec<i64> = (1..=20).collect();
        assert_eq!(percentile(&twenty, 95), Some(19));
        assert_eq!(percentile(&[7], 95), Some(7));
        assert_eq!(percentile(&[], 95), None);
        // not the mean, not the max: 5 values, 95% rank is the 5th
        assert_eq!(percentile(&[10, 20, 30, 40, 1000], 95), Some(1000));
        assert_eq!(percentile(&[10, 20, 30, 40, 1000], 80), Some(40));
    }

    #[test]
    fn empty_store_gives_zeroes_and_a_full_daily_window() {
        let (_dir, store) = open();
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();
        let stats = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert_eq!(stats.all_time, PeriodStats::default());
        assert_eq!(stats.avg_words_per_dictation, 0.0);
        assert_eq!(stats.avg_seconds_per_dictation, None);
        assert_eq!(stats.latin_share, None);
        assert_eq!(stats.daily.len(), 30);
        assert_eq!(stats.daily.last().unwrap().date, "2026-03-11");
        assert_eq!(stats.daily.first().unwrap().date, "2026-02-10");
        assert!(stats.models.is_empty() && stats.candidates.is_empty());
    }

    #[test]
    fn periods_follow_the_local_calendar_not_utc() {
        let (_dir, store) = open();
        // Wednesday 2026-03-11, 12:00 Moscow
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();

        // 00:30 local on the 11th is still the 10th in UTC: it is "today".
        rec(&store, at(2026, 3, 11, 0, 30), "раз два три");
        // 23:50 local on the 10th is the 10th in UTC too, but not "today".
        rec(&store, at(2026, 3, 10, 23, 50), "четыре пять");
        // Monday 9th: this week. Sunday 8th 23:59: last week.
        rec(&store, at(2026, 3, 9, 8, 0), "шесть");
        rec(&store, at(2026, 3, 8, 23, 59), "семь восемь девять десять");
        // far in the past: all-time only
        rec(&store, at(2025, 12, 1, 10, 0), "один");

        let stats = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert_eq!((stats.today.dictations, stats.today.words), (1, 3), "today");
        assert_eq!((stats.week.dictations, stats.week.words), (3, 6), "week");
        assert_eq!(
            (stats.all_time.dictations, stats.all_time.words),
            (5, 11),
            "all time"
        );
        // 6 s per dictation
        assert!((stats.today.minutes - 0.1).abs() < 1e-9);
        assert!((stats.all_time.minutes - 0.5).abs() < 1e-9);
        assert_eq!(stats.avg_seconds_per_dictation, Some(6.0));
        assert!((stats.avg_words_per_dictation - 2.2).abs() < 1e-9);
    }

    #[test]
    fn week_runs_monday_to_sunday() {
        let (_dir, store) = open();
        rec(&store, at(2026, 3, 9, 0, 0), "понедельник утро");
        rec(&store, at(2026, 3, 15, 23, 59), "воскресенье ночью");
        rec(&store, at(2026, 3, 16, 0, 1), "следующий понедельник");

        // asking on a Sunday: Monday 9th .. Sunday 15th
        let sunday = moscow().with_ymd_and_hms(2026, 3, 15, 23, 59, 30).unwrap();
        let s = compute(&store.connection().unwrap(), sunday, &[]).unwrap();
        assert_eq!(s.week.dictations, 2);
        assert_eq!(s.today.dictations, 1);

        // asking on Monday 16th: only the new week
        let monday = moscow().with_ymd_and_hms(2026, 3, 16, 9, 0, 0).unwrap();
        let s = compute(&store.connection().unwrap(), monday, &[]).unwrap();
        assert_eq!(s.week.dictations, 1);
        assert_eq!(s.week.words, 2);
    }

    #[test]
    fn daily_buckets_fill_gaps_with_zero_and_stay_in_order() {
        let (_dir, store) = open();
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();
        rec(&store, at(2026, 3, 11, 9, 0), "раз два");
        rec(&store, at(2026, 3, 11, 10, 0), "три");
        rec(&store, at(2026, 3, 9, 10, 0), "четыре пять шесть");
        rec(&store, at(2026, 2, 10, 10, 0), "самый старый день окна");
        rec(&store, at(2026, 2, 9, 10, 0), "вне окна тридцати дней");

        let daily = compute(&store.connection().unwrap(), now, &[])
            .unwrap()
            .daily;
        assert_eq!(daily.len(), 30);
        let by_date = |d: &str| daily.iter().find(|x| x.date == d).unwrap();
        assert_eq!(
            (
                by_date("2026-03-11").words,
                by_date("2026-03-11").dictations
            ),
            (3, 2)
        );
        assert_eq!(by_date("2026-03-10").words, 0);
        assert_eq!(by_date("2026-03-09").words, 3);
        assert_eq!(by_date("2026-02-10").words, 4);
        assert!(!daily.iter().any(|d| d.date == "2026-02-09"));
        let dates: Vec<_> = daily.iter().map(|d| d.date.clone()).collect();
        let mut sorted = dates.clone();
        sorted.sort();
        assert_eq!(dates, sorted);
    }

    #[test]
    fn model_table_has_average_p95_and_the_enough_data_flag() {
        let (_dir, store) = open();
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();
        // 25 timed dictations on `turbo`: 100, 200, ... 2500 ms
        for i in 1..=25 {
            store
                .record(&DictationInput {
                    created_at: at(2026, 3, 11, 9, 0) + i,
                    duration_ms: Some(3000),
                    transcribe_ms: Some(i * 100),
                    model_id: "turbo".to_string(),
                    final_text: "Docker и Python".to_string(),
                    ..Default::default()
                })
                .unwrap();
        }
        // 3 timed dictations on `gigaam`
        for i in 1..=3 {
            store
                .record(&DictationInput {
                    created_at: at(2026, 3, 11, 9, 0) + 100 + i,
                    transcribe_ms: Some(i * 50),
                    model_id: "gigaam".to_string(),
                    final_text: "привет мир".to_string(),
                    ..Default::default()
                })
                .unwrap();
        }
        // one imported row with no model
        store
            .record(&DictationInput {
                created_at: at(2026, 3, 1, 9, 0),
                final_text: "старая запись".to_string(),
                ..Default::default()
            })
            .unwrap();

        let stats = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert_eq!(stats.models.len(), 3);
        let turbo = &stats.models[0];
        assert_eq!(turbo.model_id, "turbo");
        assert_eq!((turbo.dictations, turbo.words), (25, 75));
        assert_eq!(turbo.latency_samples, 25);
        assert_eq!(turbo.avg_latency_ms, Some(1300.0));
        // ceil(0.95 * 25) = 24th value
        assert_eq!(turbo.p95_latency_ms, Some(2400.0));
        assert!(turbo.enough_latency_data);
        assert_eq!(turbo.latin_share, Some(50.0 / 75.0));

        let gigaam = stats
            .models
            .iter()
            .find(|m| m.model_id == "gigaam")
            .unwrap();
        assert_eq!(gigaam.latency_samples, 3);
        assert_eq!(gigaam.avg_latency_ms, Some(100.0));
        assert_eq!(gigaam.p95_latency_ms, Some(150.0));
        assert!(!gigaam.enough_latency_data);
        assert_eq!(gigaam.latin_share, Some(0.0));

        let unknown = stats.models.iter().find(|m| m.model_id.is_empty()).unwrap();
        assert_eq!(unknown.latency_samples, 0);
        assert_eq!(unknown.avg_latency_ms, None);
        assert_eq!(unknown.p95_latency_ms, None);
    }

    #[test]
    fn enough_data_starts_at_twenty_samples() {
        let (_dir, store) = open();
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();
        for i in 1..=19 {
            rec(&store, at(2026, 3, 11, 9, 0) + i, "слово");
        }
        let s19 = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert!(!s19.models[0].enough_latency_data);
        rec(&store, at(2026, 3, 11, 9, 0) + 20, "слово");
        let s20 = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert!(s20.models[0].enough_latency_data);
    }

    #[test]
    fn latin_share_counts_latin_words_over_all_words() {
        let (_dir, store) = open();
        let now = moscow().with_ymd_and_hms(2026, 3, 11, 12, 0, 0).unwrap();
        // 5 words: RAG-пайплайн, LangChain are Latin; 42 is neither
        rec(
            &store,
            at(2026, 3, 11, 9, 0),
            "RAG-пайплайн на LangChain 42 штуки",
        );
        let stats = compute(&store.connection().unwrap(), now, &[]).unwrap();
        assert_eq!(stats.all_time.words, 5);
        assert_eq!(stats.latin_share, Some(2.0 / 5.0));
        assert_eq!(stats.models[0].latin_share, Some(2.0 / 5.0));
    }

    #[test]
    fn replacement_candidates_rank_by_frequency_and_flag_dictionary_terms() {
        let (_dir, store) = open();
        let now = Utc::now().with_timezone(&moscow());
        let mk = |from: &str, to: &str| WordReplacement {
            from: from.to_string(),
            to: to.to_string(),
        };
        let add = |replacements: Vec<WordReplacement>| {
            store
                .record(&DictationInput {
                    created_at: now.timestamp(),
                    final_text: "текст".to_string(),
                    replacements,
                    ..Default::default()
                })
                .unwrap();
        };
        add(vec![
            mk("Obsidan", "Obsidian"),
            mk("langchain", "LangChain"),
        ]);
        add(vec![mk("Obsedian", "Obsidian")]);
        add(vec![mk("Obsidan", "Obsidian")]);
        add(vec![mk("fastapi", "FastAPI")]);

        let words = vec!["fastapi".to_string()];
        let stats = compute(&store.connection().unwrap(), now, &words).unwrap();
        let replaced: Vec<_> = stats
            .candidates
            .iter()
            .filter(|c| c.kind == CandidateKind::Replacement)
            .collect();
        assert_eq!(replaced.len(), 3);
        assert_eq!(replaced[0].term, "Obsidian");
        assert_eq!(replaced[0].count, 3);
        assert_eq!(replaced[0].variants, vec!["Obsidan", "Obsedian"]);
        assert!(!replaced[0].in_dictionary);
        // ties break alphabetically: FastAPI before LangChain
        assert_eq!(replaced[1].term, "FastAPI");
        assert!(
            replaced[1].in_dictionary,
            "case-insensitive dictionary check"
        );
        assert_eq!(replaced[2].term, "LangChain");
    }

    #[test]
    fn translit_candidates_match_by_stem_across_cases() {
        let (_dir, store) = open();
        let now = Utc::now().with_timezone(&moscow());
        let add = |text: &str| {
            store
                .record(&DictationInput {
                    created_at: now.timestamp(),
                    final_text: text.to_string(),
                    ..Default::default()
                })
                .unwrap();
        };
        add("Написал скрипт на питоне");
        add("Питон лучше, чем питоном по-другому");
        add("запустил в докере, потом докером собрал");
        add("всё про Python и Docker уже латиницей");

        let words = vec!["Docker".to_string()];
        let stats = compute(&store.connection().unwrap(), now, &words).unwrap();
        let translit: Vec<_> = stats
            .candidates
            .iter()
            .filter(|c| c.kind == CandidateKind::Translit)
            .collect();
        assert_eq!(translit.len(), 2);
        assert_eq!(translit[0].term, "Python");
        assert_eq!(translit[0].count, 3);
        assert!(translit[0].variants.contains(&"питоне".to_string()));
        assert!(!translit[0].in_dictionary);
        assert_eq!(translit[1].term, "Docker");
        assert_eq!(translit[1].count, 2);
        assert!(translit[1].in_dictionary);
    }
}
