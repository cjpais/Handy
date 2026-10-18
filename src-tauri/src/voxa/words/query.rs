//! Answers for the "Words" tab, read from the daily aggregates. Every function
//! takes the period as local days; nothing here recounts text, except the
//! example sentences of one word.

use super::analyze::{analyze, fold_case, stem_of};
use super::stopwords::stop_set;
use super::WordsPeriod;
use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
#[cfg(test)]
use std::collections::BTreeMap;

/// Weeks the filler trend always shows (more when the period reaches further back).
pub const TREND_WEEKS: i64 = 12;
/// The trend never spans more than this many weeks.
pub const TREND_WEEKS_MAX: i64 = 52;
/// New words are looked for in this many days when the period has no start.
pub const NEW_WORDS_ALL_TIME_DAYS: i64 = 30;
/// A new word must be used at least this often within the period.
pub const NEW_WORD_MIN_COUNT: i64 = 2;
/// Dictations scanned for the example sentences of a word, newest first.
pub const EXAMPLE_SCAN_LIMIT: i64 = 20_000;
pub const EXAMPLE_COUNT: usize = 5;

fn day_string(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn monday(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

// ------------------------------------------------------------ top words

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct WordCount {
    /// Aggregation key (the stem); pass it back to ask for the word's details.
    pub stem: String,
    /// The most frequent real form of the word in the period ("промпт", not "промп").
    pub word: String,
    pub count: u32,
}

/// The most frequent real form of `stem` within the period; all time when the
/// period has none.
fn display_form(conn: &Connection, stem: &str, from: &str, to: &str) -> Result<String> {
    let mut stmt = conn.prepare_cached(
        "SELECT form FROM form_daily WHERE stem = ?1 AND day BETWEEN ?2 AND ?3
         GROUP BY form ORDER BY SUM(count) DESC, length(form), form LIMIT 1",
    )?;
    let in_period: Option<String> = stmt
        .query_row(params![stem, from, to], |r| r.get(0))
        .ok();
    if let Some(form) = in_period {
        return Ok(form);
    }
    let mut stmt = conn.prepare_cached(
        "SELECT form FROM form_daily WHERE stem = ?1
         GROUP BY form ORDER BY SUM(count) DESC, length(form), form LIMIT 1",
    )?;
    Ok(stmt
        .query_row([stem], |r| r.get(0))
        .unwrap_or_else(|_| stem.to_string()))
}

fn with_display_forms(
    conn: &Connection,
    rows: Vec<(String, i64)>,
    from: &str,
    to: &str,
    limit: usize,
    include_stop: bool,
) -> Result<Vec<WordCount>> {
    let stops = stop_set();
    let mut out = Vec::with_capacity(limit);
    for (stem, count) in rows {
        if !include_stop && stops.is_stop_stem(&stem) {
            continue;
        }
        out.push(WordCount {
            word: display_form(conn, &stem, from, to)?,
            stem,
            count: count as u32,
        });
        if out.len() == limit {
            break;
        }
    }
    Ok(out)
}

/// Fetch enough rows that filtering out every stop stem still leaves `limit`.
fn overfetch(limit: usize, include_stop: bool) -> i64 {
    (limit + if include_stop { 0 } else { stop_set().stem_count() }) as i64
}

/// Word counts for the period as `(stem, count)` rows, unsorted. A bounded
/// period sums the daily rows; all time reads the roll-up plus the days after it.
const OPEN_WORDS_SQL: &str = "
    SELECT stem, SUM(c) AS tc FROM (
        SELECT stem, count AS c FROM word_total
        UNION ALL
        SELECT stem, SUM(count) FROM form_daily WHERE day > ?1 GROUP BY stem
    ) GROUP BY stem";

pub fn top_words(
    conn: &Connection,
    period: &WordsPeriod,
    include_stop: bool,
    limit: usize,
    rolled: &str,
) -> Result<Vec<WordCount>> {
    let (from, to) = period.bounds();
    let fetch = overfetch(limit, include_stop);
    let rows: Vec<(String, i64)> = if period.is_open() {
        let mut stmt = conn.prepare_cached(&format!(
            "{OPEN_WORDS_SQL} ORDER BY tc DESC, stem LIMIT ?2"
        ))?;
        let mapped = stmt
            .query_map(params![rolled, fetch], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        mapped
    } else {
        let mut stmt = conn.prepare_cached(
            "SELECT stem, SUM(count) AS c FROM form_daily WHERE day BETWEEN ?1 AND ?2
             GROUP BY stem ORDER BY c DESC, stem LIMIT ?3",
        )?;
        let mapped = stmt
            .query_map(params![from, to, fetch], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        mapped
    };
    with_display_forms(conn, rows, &from, &to, limit, include_stop)
}

// ------------------------------------------------------------ new words

/// Words whose first appearance ever lies inside the period and that were used
/// at least [`NEW_WORD_MIN_COUNT`] times in it. With no start of the period
/// (all time) it looks at the last [`NEW_WORDS_ALL_TIME_DAYS`] days.
///
/// A word is known before the period when the roll-up saw it earlier
/// (`word_total.first_day`) or a day between the roll-up and the period has it.
pub fn new_words(
    conn: &Connection,
    period: &WordsPeriod,
    today: NaiveDate,
    include_stop: bool,
    limit: usize,
    rolled: &str,
) -> Result<Vec<WordCount>> {
    let (mut from, to) = period.bounds();
    if period.from.is_none() {
        from = day_string(today - Duration::days(NEW_WORDS_ALL_TIME_DAYS - 1));
    }
    let mut stmt = conn.prepare_cached(
        "SELECT p.stem, p.c FROM
           (SELECT stem, SUM(count) AS c FROM form_daily
            WHERE day BETWEEN ?1 AND ?2 GROUP BY stem HAVING c >= ?3) p
         WHERE NOT EXISTS (SELECT 1 FROM word_total t WHERE t.stem = p.stem AND t.first_day < ?1)
           AND NOT EXISTS (SELECT 1 FROM form_daily d
                           WHERE d.day > ?4 AND d.day < ?1 AND d.stem = p.stem)
         ORDER BY p.c DESC, p.stem LIMIT ?5",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(
            params![from, to, NEW_WORD_MIN_COUNT, rolled, overfetch(limit, include_stop)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
    with_display_forms(conn, rows, &from, &to, limit, include_stop)
}

// ----------------------------------------------------------- vocabulary

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct Vocabulary {
    /// Different words (stems) used in the period.
    pub distinct: u32,
    /// Word occurrences in the period (numbers are not words).
    pub total: u32,
}

pub fn vocabulary(conn: &Connection, period: &WordsPeriod, rolled: &str) -> Result<Vocabulary> {
    let (from, to) = period.bounds();
    let read = |r: &rusqlite::Row| -> rusqlite::Result<(i64, i64)> { Ok((r.get(0)?, r.get(1)?)) };
    let (distinct, total): (i64, i64) = if period.is_open() {
        conn.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(tc), 0) FROM ({OPEN_WORDS_SQL})"),
            [rolled],
            read,
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(c), 0) FROM
               (SELECT SUM(count) AS c FROM form_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY stem)",
            params![from, to],
            read,
        )?
    };
    Ok(Vocabulary {
        distinct: distinct as u32,
        total: total as u32,
    })
}

// -------------------------------------------------------------- openers

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct OpenerStat {
    pub text: String,
    pub count: u32,
    /// Share of the period's dictations, in percent.
    pub percent: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct Openers {
    /// Dictations with at least one word in the period.
    pub total: u32,
    pub one: Vec<OpenerStat>,
    pub two: Vec<OpenerStat>,
}

pub fn openers(conn: &Connection, period: &WordsPeriod, limit: usize) -> Result<Openers> {
    let (from, to) = period.bounds();
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dictation_openers WHERE day BETWEEN ?1 AND ?2",
        params![from, to],
        |r| r.get(0),
    )?;
    let top = |column: &str| -> Result<Vec<OpenerStat>> {
        let sql = format!(
            "SELECT {column}, COUNT(*) AS c FROM dictation_openers
             WHERE day BETWEEN ?1 AND ?2 AND {column} IS NOT NULL
             GROUP BY {column} ORDER BY c DESC, {column} LIMIT ?3"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows: Vec<(String, i64)> = stmt
            .query_map(params![from, to, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(text, count)| OpenerStat {
                text,
                count: count as u32,
                percent: if total > 0 {
                    count as f64 * 100.0 / total as f64
                } else {
                    0.0
                },
            })
            .collect())
    };
    Ok(Openers {
        total: total as u32,
        one: top("opener1")?,
        two: top("opener2")?,
    })
}

// -------------------------------------------------------------- phrases

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct PhraseStat {
    pub text: String,
    pub count: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct Phrases {
    pub bigrams: Vec<PhraseStat>,
    pub trigrams: Vec<PhraseStat>,
}

/// Phrases must be used at least this often to be listed.
pub const PHRASE_MIN_COUNT: i64 = 2;

pub fn phrases(
    conn: &Connection,
    period: &WordsPeriod,
    limit: usize,
    rolled: &str,
) -> Result<Phrases> {
    let (from, to) = period.bounds();
    let top = |n: i64| -> Result<Vec<PhraseStat>> {
        if period.is_open() {
            return open_phrases(conn, n, limit, rolled);
        }
        let mut stmt = conn.prepare_cached(
            "SELECT ngram, SUM(count) AS c FROM ngram_daily
             WHERE n = ?1 AND stop_only = 0 AND day BETWEEN ?2 AND ?3
             GROUP BY ngram HAVING c >= ?4 ORDER BY c DESC, ngram LIMIT ?5",
        )?;
        let rows = stmt
            .query_map(params![n, from, to, PHRASE_MIN_COUNT, limit as i64], |r| {
                Ok(PhraseStat {
                    text: r.get(0)?,
                    count: r.get::<_, i64>(1)? as u32,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    };
    Ok(Phrases {
        bigrams: top(2)?,
        trigrams: top(3)?,
    })
}

/// All-time phrases: the best of the roll-up (read through the index on
/// `(n, stop_only, count)`), corrected by the days after the roll-up. The
/// roll-up is read far enough that no phrase pushed up by the newer days is missed.
fn open_phrases(conn: &Connection, n: i64, limit: usize, rolled: &str) -> Result<Vec<PhraseStat>> {
    let mut newer: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT ngram, SUM(count) FROM ngram_daily
             WHERE day > ?1 AND n = ?2 AND stop_only = 0 GROUP BY ngram",
        )?;
        let rows = stmt.query_map(params![rolled, n], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (ngram, count) = row?;
            newer.insert(ngram, count);
        }
    }
    let mut merged: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT ngram, count FROM ngram_total
             WHERE n = ?1 AND stop_only = 0 ORDER BY count DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![n, (limit + newer.len()) as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (ngram, count) = row?;
            merged.insert(ngram, count);
        }
    }
    let mut lookup = conn.prepare_cached("SELECT count FROM ngram_total WHERE ngram = ?1")?;
    for (ngram, count) in &newer {
        let known = match merged.get(ngram) {
            Some(c) => *c,
            None => lookup.query_row([ngram], |r| r.get(0)).unwrap_or(0),
        };
        merged.insert(ngram.clone(), known + count);
    }
    let mut rows: Vec<PhraseStat> = merged
        .into_iter()
        .filter(|(_, c)| *c >= PHRASE_MIN_COUNT)
        .map(|(text, count)| PhraseStat {
            text,
            count: count as u32,
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.text.cmp(&b.text)));
    rows.truncate(limit);
    Ok(rows)
}

// -------------------------------------------------------------- fillers

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct FillerStat {
    pub word: String,
    /// Uses within the period.
    pub count: u32,
    /// Uses per week of the trend, aligned with `Fillers::weeks`.
    pub weekly: Vec<u32>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct WeekPoint {
    /// Monday of the week, `YYYY-MM-DD`.
    pub week_start: String,
    /// All filler uses in the week.
    pub fillers: u32,
    /// All words in the week, to turn the line into "per 100 words".
    pub words: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct Fillers {
    pub items: Vec<FillerStat>,
    pub weeks: Vec<WeekPoint>,
}

/// A filler as it is looked up: normalized words joined by a space, 1 to 3 words.
fn filler_key(raw: &str) -> Option<(String, usize)> {
    let forms: Vec<String> = analyze(raw).tokens.into_iter().map(|t| t.form).collect();
    (1..=3).contains(&forms.len()).then(|| (forms.join(" "), forms.len()))
}

/// Daily counts of the multi-word fillers inside `[from, to]`, in one pass:
/// `filler -> [(day, count)]`.
fn phrase_days(
    conn: &Connection,
    phrases: &[&str],
    from: &str,
    to: &str,
) -> Result<HashMap<String, Vec<(String, i64)>>> {
    let mut out: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    if phrases.is_empty() {
        return Ok(out);
    }
    let marks = vec!["?"; phrases.len()].join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT ngram, day, count FROM ngram_daily
         WHERE day BETWEEN ?1 AND ?2 AND ngram IN ({marks})"
    ))?;
    let mut args: Vec<&dyn rusqlite::ToSql> = vec![&from, &to];
    args.extend(phrases.iter().map(|p| p as &dyn rusqlite::ToSql));
    let rows = stmt.query_map(args.as_slice(), |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (ngram, day, count) = row?;
        out.entry(ngram).or_default().push((day, count));
    }
    Ok(out)
}

pub fn fillers(
    conn: &Connection,
    period: &WordsPeriod,
    filler_words: &[String],
    today: NaiveDate,
    rolled: &str,
) -> Result<Fillers> {
    // trend window: this week and the 11 before it, further back when the period reaches there
    let this_monday = monday(today);
    let mut first_monday = this_monday - Duration::weeks(TREND_WEEKS - 1);
    if let Some(from) = period.from.as_deref().and_then(parse_day) {
        first_monday = first_monday.min(monday(from));
    }
    first_monday = first_monday.max(this_monday - Duration::weeks(TREND_WEEKS_MAX - 1));
    let week_count = ((this_monday - first_monday).num_days() / 7 + 1) as usize;
    let window_from = day_string(first_monday);
    let window_to = day_string(this_monday + Duration::days(6));
    let (period_from, period_to) = period.bounds();
    // The window rows also give the period's count when the period lies inside it.
    let period_in_window = period.from.as_deref().is_some_and(|f| f >= window_from.as_str());

    let week_index = |day: &str| -> Option<usize> {
        let date = parse_day(day)?;
        let weeks = (monday(date) - first_monday).num_days() / 7;
        (0..week_count as i64).contains(&weeks).then_some(weeks as usize)
    };

    // distinct fillers, as they are looked up
    let mut seen = std::collections::HashSet::new();
    let mut keys: Vec<(String, usize)> = Vec::new();
    for raw in filler_words {
        if let Some((key, n)) = filler_key(raw) {
            if seen.insert(key.clone()) {
                keys.push((key, n));
            }
        }
    }
    let phrases: Vec<&str> = keys
        .iter()
        .filter(|(_, n)| *n > 1)
        .map(|(k, _)| k.as_str())
        .collect();
    let phrase_rows = phrase_days(conn, &phrases, &window_from, &window_to)?;

    let mut items = Vec::new();
    let mut filler_per_week = vec![0u32; week_count];
    for (key, n) in &keys {
        let rows: Vec<(String, i64)> = if *n == 1 {
            // (stem, form, day) is the primary key: an exact lookup, no index needed
            let mut stmt = conn.prepare_cached(
                "SELECT day, count FROM form_daily
                 WHERE stem = ?1 AND form = ?2 AND day BETWEEN ?3 AND ?4",
            )?;
            let mapped = stmt
                .query_map(params![stem_of(key), key, window_from, window_to], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            mapped
        } else {
            phrase_rows.get(key).cloned().unwrap_or_default()
        };
        let mut weekly = vec![0u32; week_count];
        for (day, count) in &rows {
            if let Some(i) = week_index(day) {
                weekly[i] += *count as u32;
                filler_per_week[i] += *count as u32;
            }
        }
        // the counter covers the period, which may be shorter or longer than the window
        let in_period: i64 = if period_in_window {
            rows.iter()
                .filter(|(d, _)| d.as_str() >= period_from.as_str() && d.as_str() <= period_to.as_str())
                .map(|(_, c)| c)
                .sum()
        } else if *n == 1 {
            conn.query_row(
                "SELECT COALESCE(SUM(count), 0) FROM form_daily
                 WHERE stem = ?1 AND form = ?2 AND day BETWEEN ?3 AND ?4",
                params![stem_of(key), key, period_from, period_to],
                |r| r.get(0),
            )?
        } else if period.is_open() {
            // the roll-up plus what came after it (the window holds those days)
            let known: i64 = conn
                .query_row("SELECT count FROM ngram_total WHERE ngram = ?1", [key], |r| r.get(0))
                .unwrap_or(0);
            known
                + rows
                    .iter()
                    .filter(|(d, _)| d.as_str() > rolled)
                    .map(|(_, c)| c)
                    .sum::<i64>()
        } else {
            conn.query_row(
                "SELECT COALESCE(SUM(count), 0) FROM ngram_daily
                 WHERE ngram = ?1 AND day BETWEEN ?2 AND ?3",
                params![key, period_from, period_to],
                |r| r.get(0),
            )?
        };
        items.push(FillerStat {
            word: key.clone(),
            count: in_period as u32,
            weekly,
        });
    }
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));

    let mut words_per_week = vec![0u32; week_count];
    {
        let mut stmt = conn.prepare_cached(
            "SELECT day, SUM(count) FROM form_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day",
        )?;
        let rows = stmt.query_map(params![window_from, window_to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (day, count) = row?;
            if let Some(i) = week_index(&day) {
                words_per_week[i] += count as u32;
            }
        }
    }
    let weeks = (0..week_count)
        .map(|i| WeekPoint {
            week_start: day_string(first_monday + Duration::weeks(i as i64)),
            fillers: filler_per_week[i],
            words: words_per_week[i],
        })
        .collect();
    Ok(Fillers { items, weeks })
}

// ---------------------------------------------------------- word detail

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct FormCount {
    pub form: String,
    pub count: u32,
}

/// A piece of an example sentence; `hit` pieces are the word to highlight.
#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct ExampleSegment {
    pub text: String,
    pub hit: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct WordExample {
    pub dictation_id: i64,
    pub created_at: i64,
    pub segments: Vec<ExampleSegment>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Type)]
pub struct WordDetail {
    pub word: String,
    pub total: u32,
    pub forms: Vec<FormCount>,
    pub examples: Vec<WordExample>,
}

/// Unix seconds of the start of `day` and of the day after `to`.
fn unix_bounds<Tz: TimeZone>(period: &WordsPeriod, tz: &Tz) -> (i64, i64) {
    let start = |date: NaiveDate| {
        date.and_hms_opt(0, 0, 0)
            .and_then(|dt| tz.from_local_datetime(&dt).earliest())
            .map(|d| d.timestamp())
    };
    let from = period
        .from
        .as_deref()
        .and_then(parse_day)
        .and_then(start)
        .unwrap_or(i64::MIN);
    let to = period
        .to
        .as_deref()
        .and_then(parse_day)
        .and_then(|d| start(d + Duration::days(1)))
        .unwrap_or(i64::MAX);
    (from, to)
}

/// Roughly this many characters of context around the highlighted word.
const CONTEXT_BEFORE: usize = 100;
const CONTEXT_AFTER: usize = 140;

fn char_floor(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn char_ceil(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// The sentence with the word's occurrences marked, cut to a readable window.
fn example_segments(text: &str, hits: &[(usize, usize)]) -> Vec<ExampleSegment> {
    let first = hits.first().map(|h| h.0).unwrap_or(0);
    let last = hits.last().map(|h| h.1).unwrap_or(text.len());
    let mut start = char_floor(text, first.saturating_sub(CONTEXT_BEFORE));
    let mut end = char_ceil(text, (last + CONTEXT_AFTER).min(text.len()));
    // start and end on a word edge, not in the middle of a word
    if start > 0 {
        if let Some(space) = text[start..first].find(char::is_whitespace) {
            start += space + 1;
        }
    }
    if end < text.len() {
        if let Some(space) = text[last..end].rfind(char::is_whitespace) {
            end = last + space;
        }
    }
    let mut out = Vec::new();
    if start > 0 {
        out.push(ExampleSegment {
            text: "\u{2026}".to_string(),
            hit: false,
        });
    }
    let mut at = start;
    for &(s, e) in hits.iter().filter(|h| h.0 >= start && h.1 <= end) {
        if s > at {
            out.push(ExampleSegment {
                text: text[at..s].to_string(),
                hit: false,
            });
        }
        out.push(ExampleSegment {
            text: text[s..e].to_string(),
            hit: true,
        });
        at = e;
    }
    if at < end {
        out.push(ExampleSegment {
            text: text[at..end].to_string(),
            hit: false,
        });
    }
    if end < text.len() {
        out.push(ExampleSegment {
            text: "\u{2026}".to_string(),
            hit: false,
        });
    }
    out
}

#[cfg(test)]
pub fn tests_example_segments(text: &str, hits: &[(usize, usize)]) -> Vec<ExampleSegment> {
    example_segments(text, hits)
}

pub fn word_detail<Tz: TimeZone>(
    conn: &Connection,
    period: &WordsPeriod,
    stem: &str,
    tz: &Tz,
) -> Result<WordDetail> {
    let (from, to) = period.bounds();
    let mut stmt = conn.prepare_cached(
        "SELECT form, SUM(count) AS c FROM form_daily
         WHERE stem = ?1 AND day BETWEEN ?2 AND ?3
         GROUP BY form ORDER BY c DESC, form",
    )?;
    let forms: Vec<FormCount> = stmt
        .query_map(params![stem, from, to], |r| {
            Ok(FormCount {
                form: r.get(0)?,
                count: r.get::<_, i64>(1)? as u32,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    let total = forms.iter().map(|f| f.count).sum();
    let word = forms
        .first()
        .map(|f| f.form.clone())
        .unwrap_or_else(|| stem.to_string());

    // The newest dictations of the period that really contain the word. A cheap
    // check on the first letters of the stem skips most texts before the exact
    // (stemming) test.
    let prefix: String = fold_case(stem).chars().take(3).collect();
    let (ts_from, ts_to) = unix_bounds(period, tz);
    let mut stmt = conn.prepare_cached(
        "SELECT id, created_at, final_text FROM dictations
         WHERE created_at >= ?1 AND created_at < ?2
         ORDER BY created_at DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![ts_from, ts_to, EXAMPLE_SCAN_LIMIT], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut examples = Vec::new();
    for row in rows {
        let (id, created_at, text) = row?;
        if !prefix.is_empty() && !fold_case(&text).contains(&prefix) {
            continue;
        }
        let hits: Vec<(usize, usize)> = analyze(&text)
            .tokens
            .iter()
            .filter(|t| t.stem == stem)
            .map(|t| (t.start, t.end))
            .collect();
        if hits.is_empty() {
            continue;
        }
        examples.push(WordExample {
            dictation_id: id,
            created_at,
            segments: example_segments(&text, &hits),
        });
        if examples.len() == EXAMPLE_COUNT {
            break;
        }
    }
    Ok(WordDetail {
        word,
        total,
        forms,
        examples,
    })
}

/// Counts per stem for tests and diagnostics.
#[cfg(test)]
pub fn stem_counts(conn: &Connection) -> BTreeMap<String, i64> {
    let mut stmt = conn
        .prepare("SELECT stem, SUM(count) FROM form_daily GROUP BY stem")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect::<BTreeMap<_, _>>()
}
