//! Answers for the "Words" tab, read from the daily aggregates. Every function
//! takes the period as local days; nothing here recounts text, except the
//! example sentences of one word.

use super::analyze::{analyze, fold_case};
use super::stopwords::stop_set;
use super::WordsPeriod;
use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;
use specta::Type;
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

pub fn top_words(
    conn: &Connection,
    period: &WordsPeriod,
    include_stop: bool,
    limit: usize,
) -> Result<Vec<WordCount>> {
    let (from, to) = period.bounds();
    let mut stmt = conn.prepare_cached(
        "SELECT stem, SUM(count) AS c FROM word_daily WHERE day BETWEEN ?1 AND ?2
         GROUP BY stem ORDER BY c DESC, stem LIMIT ?3",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![from, to, overfetch(limit, include_stop)], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    with_display_forms(conn, rows, &from, &to, limit, include_stop)
}

// ------------------------------------------------------------ new words

/// Words whose first appearance ever lies inside the period and that were used
/// at least [`NEW_WORD_MIN_COUNT`] times in it. With no start of the period
/// (all time) it looks at the last [`NEW_WORDS_ALL_TIME_DAYS`] days.
pub fn new_words(
    conn: &Connection,
    period: &WordsPeriod,
    today: NaiveDate,
    include_stop: bool,
    limit: usize,
) -> Result<Vec<WordCount>> {
    let (mut from, to) = period.bounds();
    if period.from.is_none() {
        from = day_string(today - Duration::days(NEW_WORDS_ALL_TIME_DAYS - 1));
    }
    let mut stmt = conn.prepare_cached(
        "SELECT stem,
                SUM(CASE WHEN day >= ?1 AND day <= ?2 THEN count ELSE 0 END) AS c,
                MIN(day) AS first_day
         FROM word_daily GROUP BY stem
         HAVING first_day >= ?1 AND first_day <= ?2 AND c >= ?3
         ORDER BY c DESC, stem LIMIT ?4",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(
            params![from, to, NEW_WORD_MIN_COUNT, overfetch(limit, include_stop)],
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

pub fn vocabulary(conn: &Connection, period: &WordsPeriod) -> Result<Vocabulary> {
    let (from, to) = period.bounds();
    let (distinct, total): (i64, i64) = conn.query_row(
        "SELECT COUNT(DISTINCT stem), COALESCE(SUM(count), 0) FROM word_daily
         WHERE day BETWEEN ?1 AND ?2",
        params![from, to],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
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

pub fn phrases(conn: &Connection, period: &WordsPeriod, limit: usize) -> Result<Phrases> {
    let (from, to) = period.bounds();
    let top = |n: i64| -> Result<Vec<PhraseStat>> {
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

pub fn fillers(
    conn: &Connection,
    period: &WordsPeriod,
    filler_words: &[String],
    today: NaiveDate,
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

    let week_index = |day: &str| -> Option<usize> {
        let date = parse_day(day)?;
        let weeks = (monday(date) - first_monday).num_days() / 7;
        (0..week_count as i64).contains(&weeks).then_some(weeks as usize)
    };

    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    let mut filler_per_week = vec![0u32; week_count];
    for raw in filler_words {
        let Some((key, n)) = filler_key(raw) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        let rows: Vec<(String, i64)> = if n == 1 {
            let mut stmt = conn.prepare_cached(
                "SELECT day, SUM(count) FROM form_daily
                 WHERE form = ?1 AND day BETWEEN ?2 AND ?3 GROUP BY day",
            )?;
            let mapped = stmt
                .query_map(params![key, window_from, window_to], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            mapped
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT day, count FROM ngram_daily
                 WHERE ngram = ?1 AND day BETWEEN ?2 AND ?3",
            )?;
            let mapped = stmt
                .query_map(params![key, window_from, window_to], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            mapped
        };
        let mut weekly = vec![0u32; week_count];
        for (day, count) in &rows {
            if let Some(i) = week_index(day) {
                weekly[i] += *count as u32;
                filler_per_week[i] += *count as u32;
            }
        }
        // the counter covers the period, which may be shorter or longer than the window
        let in_period: i64 = if n == 1 {
            conn.query_row(
                "SELECT COALESCE(SUM(count), 0) FROM form_daily
                 WHERE form = ?1 AND day BETWEEN ?2 AND ?3",
                params![key, period_from, period_to],
                |r| r.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COALESCE(SUM(count), 0) FROM ngram_daily
                 WHERE ngram = ?1 AND day BETWEEN ?2 AND ?3",
                params![key, period_from, period_to],
                |r| r.get(0),
            )?
        };
        items.push(FillerStat {
            word: key,
            count: in_period as u32,
            weekly,
        });
    }
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));

    let mut words_per_week = vec![0u32; week_count];
    {
        let mut stmt = conn.prepare_cached(
            "SELECT day, SUM(count) FROM word_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day",
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
        .prepare("SELECT stem, SUM(count) FROM word_daily GROUP BY stem")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect::<BTreeMap<_, _>>()
}
