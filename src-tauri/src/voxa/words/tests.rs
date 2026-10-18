//! Tests of the word statistics on a real (temporary) statistics database.

use super::index::{self, BackfillState};
use super::query::{self, stem_counts};
use super::WordsPeriod;
use crate::voxa::store::{DictationInput, VoxaStore, DB_FILE_NAME};
use chrono::{NaiveDate, TimeZone, Utc};
use rusqlite::Connection;
use tempfile::TempDir;

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// Noon UTC: the same calendar day in every timezone the tests may run in.
fn noon(date: NaiveDate) -> i64 {
    Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
        .timestamp()
}

fn open() -> (TempDir, VoxaStore) {
    let dir = TempDir::new().unwrap();
    let store = VoxaStore::open(&dir.path().join(DB_FILE_NAME), None).unwrap();
    (dir, store)
}

fn say(store: &VoxaStore, date: NaiveDate, history_id: Option<i64>, text: &str) -> i64 {
    store
        .record(&DictationInput {
            created_at: noon(date),
            history_id,
            final_text: text.to_string(),
            raw_text: text.to_string(),
            ..Default::default()
        })
        .unwrap()
        .expect("text with words")
}

/// The roll-up date the queries need; nothing is rolled up unless a test does it.
fn rolled(conn: &Connection) -> String {
    index::rolled_through(conn).unwrap()
}

fn all() -> WordsPeriod {
    WordsPeriod::default()
}

fn between(from: NaiveDate, to: NaiveDate) -> WordsPeriod {
    WordsPeriod {
        from: Some(from.format("%Y-%m-%d").to_string()),
        to: Some(to.format("%Y-%m-%d").to_string()),
    }
}

fn top(conn: &Connection, period: &WordsPeriod, stop: bool) -> Vec<(String, u32)> {
    query::top_words(conn, period, stop, 50, &rolled(conn))
        .unwrap()
        .into_iter()
        .map(|w| (w.word, w.count))
        .collect()
}

#[test]
fn forms_of_a_word_group_and_show_the_most_frequent_real_form() {
    let (_dir, store) = open();
    let d = day(2026, 10, 5);
    say(&store, d, None, "Промпт получился хороший. Промпта не хватает. Промпт снова.");
    say(&store, d, None, "С промптом проще. Промпт готов.");
    let conn = store.connection().unwrap();

    let words = top(&conn, &all(), false);
    let (word, count) = &words[0];
    assert_eq!(word, "промпт", "the stem 'промп' is never shown");
    assert_eq!(*count, 5, "промпт ×3, промпта, промптом: one group");

    let stem = query::top_words(&conn, &all(), false, 1, &rolled(&conn)).unwrap().remove(0).stem;
    let detail = query::word_detail(&conn, &all(), &stem, &Utc).unwrap();
    assert_eq!(detail.total, 5);
    assert_eq!(detail.forms[0].form, "промпт");
    assert_eq!(detail.forms[0].count, 3);
    let forms: Vec<&str> = detail.forms.iter().map(|f| f.form.as_str()).collect();
    assert!(forms.contains(&"промпта") && forms.contains(&"промптом"), "{forms:?}");
}

#[test]
fn claude_and_claude_with_a_russian_ending_are_one_word() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "Claude помог. Спросил у Claude'а. Claude снова.");
    let conn = store.connection().unwrap();
    let words = top(&conn, &all(), false);
    assert_eq!(words.iter().find(|(w, _)| w == "claude").map(|w| w.1), Some(3));
}

#[test]
fn numbers_are_not_counted_as_words() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "Python 3.11 и 2026 год");
    let conn = store.connection().unwrap();
    let counts = stem_counts(&conn);
    assert!(counts.keys().all(|k| k.chars().any(char::is_alphabetic)), "{counts:?}");
    assert_eq!(query::vocabulary(&conn, &all(), &rolled(&conn)).unwrap().total, 3);
}

#[test]
fn stop_words_are_filtered_when_asked_not_when_stored() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "Это и в на что промпт и в промпт");
    let conn = store.connection().unwrap();

    // stored: every word is there
    let total: i64 = conn
        .query_row("SELECT SUM(count) FROM form_daily", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 9);

    let without = top(&conn, &all(), false);
    assert_eq!(without, vec![("промпт".to_string(), 2)]);
    let with = top(&conn, &all(), true);
    assert!(with.iter().any(|(w, c)| w == "и" && *c == 2));
    assert!(with.len() > without.len());
    // flipping the switch changed nothing in the database
    let after: i64 = conn
        .query_row("SELECT SUM(count) FROM form_daily", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, 9);
}

#[test]
fn period_filters_every_block() {
    let (_dir, store) = open();
    say(&store, day(2026, 9, 1), None, "Сентябрьский пайплайн готов");
    say(&store, day(2026, 10, 5), None, "Октябрьский промпт готов");
    let conn = store.connection().unwrap();
    let october = between(day(2026, 10, 1), day(2026, 10, 31));
    let words: Vec<String> = top(&conn, &october, false).into_iter().map(|w| w.0).collect();
    assert!(words.contains(&"промпт".to_string()));
    assert!(!words.contains(&"пайплайн".to_string()));
    assert_eq!(query::vocabulary(&conn, &october, &rolled(&conn)).unwrap().total, 3);
    assert_eq!(query::vocabulary(&conn, &all(), &rolled(&conn)).unwrap().total, 6);
    assert_eq!(query::openers(&conn, &october, 10).unwrap().total, 1);
}

#[test]
fn openers_ignore_leading_punctuation_and_report_a_share() {
    let (_dir, store) = open();
    let d = day(2026, 10, 5);
    say(&store, d, None, "— Короче, делаем так");
    say(&store, d, None, "Короче, потом проверим");
    say(&store, d, None, "Давай посмотрим");
    say(&store, d, None, "...Слушай, а где файл");
    let conn = store.connection().unwrap();
    let openers = query::openers(&conn, &all(), 10).unwrap();
    assert_eq!(openers.total, 4);
    assert_eq!(openers.one[0].text, "короче");
    assert_eq!(openers.one[0].count, 2);
    assert!((openers.one[0].percent - 50.0).abs() < 1e-9);
    let two: std::collections::HashMap<&str, u32> =
        openers.two.iter().map(|o| (o.text.as_str(), o.count)).collect();
    assert_eq!(two["короче делаем"], 1);
    assert_eq!(two["короче потом"], 1);
    assert_eq!(two["слушай а"], 1);
}

#[test]
fn phrases_skip_stop_only_ones_and_singletons_and_do_not_cross_sentences() {
    let (_dir, store) = open();
    let d = day(2026, 10, 5);
    say(&store, d, None, "Сделай так, чтобы работало. Потом проверь");
    say(&store, d, None, "Сделай так, чтобы быстрее. Потом проверь");
    say(&store, d, None, "и в том что это");
    say(&store, d, None, "и в том что это");
    let conn = store.connection().unwrap();
    let phrases = query::phrases(&conn, &all(), 10, &rolled(&conn)).unwrap();
    let tri: Vec<&str> = phrases.trigrams.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(tri, vec!["сделай так чтобы"], "stop-only and once-only are out");
    let bi: Vec<&str> = phrases.bigrams.iter().map(|p| p.text.as_str()).collect();
    assert!(bi.contains(&"сделай так") && bi.contains(&"потом проверь"), "{bi:?}");
    assert!(!bi.contains(&"работало потом"), "no phrase across a sentence end");
    assert!(!bi.contains(&"в том"), "{bi:?}");
}

#[test]
fn fillers_count_single_words_and_phrases_with_a_weekly_trend() {
    let (_dir, store) = open();
    // 2026-10-19 is a Monday
    let this_week = day(2026, 10, 19);
    let last_week = day(2026, 10, 12);
    say(&store, last_week, None, "Ну короче как бы да ну");
    say(&store, this_week, None, "Короче как бы типа сделали");
    say(&store, this_week, None, "Как бы да");
    let conn = store.connection().unwrap();
    let list: Vec<String> = ["ну", "короче", "как бы", "типа", "блин", "Ну"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let fillers = query::fillers(&conn, &all(), &list, day(2026, 10, 22), &rolled(&conn)).unwrap();

    let by: std::collections::HashMap<&str, &query::FillerStat> =
        fillers.items.iter().map(|f| (f.word.as_str(), f)).collect();
    assert_eq!(by["ну"].count, 2);
    assert_eq!(by["короче"].count, 2);
    assert_eq!(by["как бы"].count, 3, "a two-word filler is looked up as a phrase");
    assert_eq!(by["типа"].count, 1);
    assert_eq!(by["блин"].count, 0);
    assert_eq!(fillers.items.len(), 5, "the duplicate 'Ну' is not listed twice");
    assert_eq!(fillers.items[0].word, "как бы", "most used first");

    // trend: 12 weeks ending this week; the last two points hold the data
    assert_eq!(fillers.weeks.len(), 12);
    let last = fillers.weeks.last().unwrap();
    assert_eq!(last.week_start, "2026-10-19");
    assert_eq!(last.fillers, 4, "короче, как бы, типа, как бы");
    let previous = &fillers.weeks[10];
    assert_eq!(previous.week_start, "2026-10-12");
    assert_eq!(previous.fillers, 4, "ну, короче, как бы, ну");
    assert_eq!(by["как бы"].weekly[10..], [1, 2]);
    assert!(last.words > 0 && previous.words > 0);
}

#[test]
fn a_period_older_than_twelve_weeks_widens_the_trend() {
    let (_dir, store) = open();
    say(&store, day(2026, 3, 2), None, "ну да");
    let conn = store.connection().unwrap();
    let period = between(day(2026, 3, 1), day(2026, 10, 22));
    let fillers = query::fillers(&conn, &period, &["ну".to_string()], day(2026, 10, 22), &rolled(&conn)).unwrap();
    assert!(fillers.weeks.len() > 12);
    assert_eq!(fillers.weeks.len() as i64 <= query::TREND_WEEKS_MAX, true);
    assert_eq!(fillers.items[0].count, 1);
}

#[test]
fn new_words_are_first_seen_in_the_period_and_used_twice() {
    let (_dir, store) = open();
    say(&store, day(2026, 9, 1), None, "Старый промпт");
    say(&store, day(2026, 10, 5), None, "Новый пайплайн и старый промпт");
    say(&store, day(2026, 10, 6), None, "Пайплайн снова, а ещё эмбеддинг");
    let conn = store.connection().unwrap();
    let october = between(day(2026, 10, 1), day(2026, 10, 31));
    let words: Vec<(String, u32)> = query::new_words(&conn, &october, day(2026, 10, 31), false, 30, &rolled(&conn))
        .unwrap()
        .into_iter()
        .map(|w| (w.word, w.count))
        .collect();
    assert!(words.contains(&("пайплайн".to_string(), 2)), "{words:?}");
    assert!(!words.iter().any(|w| w.0 == "промпт"), "промпт was known in September");
    assert!(!words.iter().any(|w| w.0 == "эмбеддинг"), "used only once");
}

#[test]
fn all_time_new_words_look_at_the_last_thirty_days() {
    let (_dir, store) = open();
    say(&store, day(2026, 1, 10), None, "давний пайплайн и давний пайплайн");
    say(&store, day(2026, 10, 20), None, "свежий агент и свежий агент");
    let conn = store.connection().unwrap();
    let words: Vec<String> = query::new_words(&conn, &all(), day(2026, 10, 22), false, 30, &rolled(&conn))
        .unwrap()
        .into_iter()
        .map(|w| w.word)
        .collect();
    assert!(words.contains(&"агент".to_string()), "{words:?}");
    assert!(!words.contains(&"пайплайн".to_string()), "{words:?}");
}

#[test]
fn vocabulary_counts_different_words() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "промпт промпта промптом агент агент");
    let conn = store.connection().unwrap();
    let v = query::vocabulary(&conn, &all(), &rolled(&conn)).unwrap();
    assert_eq!((v.distinct, v.total), (2, 5));
}

#[test]
fn deleting_a_dictation_takes_its_contribution_out() {
    let (_dir, store) = open();
    let d = day(2026, 10, 5);
    say(&store, d, Some(1), "Промпт для агента. Промпт снова");
    say(&store, d, Some(2), "Ещё один промпт");
    let before = store.connection().unwrap();
    assert_eq!(top(&before, &all(), false)[0], ("промпт".to_string(), 3));
    drop(before);

    assert!(store.remove_by_history_id(1).unwrap());
    let conn = store.connection().unwrap();
    assert_eq!(top(&conn, &all(), false)[0], ("промпт".to_string(), 1));
    let agents = top(&conn, &all(), false).into_iter().filter(|w| w.0.starts_with("агент")).count();
    assert_eq!(agents, 0, "rows that reach zero disappear");
    assert_eq!(query::openers(&conn, &all(), 10).unwrap().total, 1);
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM dictations", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
    assert!(!store.remove_by_history_id(1).unwrap(), "already gone");
    assert!(!store.remove_by_history_id(99).unwrap(), "unknown id");
}

#[test]
fn removing_everything_leaves_no_residue() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), Some(1), "Сделай так, чтобы работало. Потом проверь");
    assert!(store.remove_by_history_id(1).unwrap());
    let conn = store.connection().unwrap();
    for table in [
        "form_daily",
        "ngram_daily",
        "word_total",
        "ngram_total",
        "dictation_openers",
    ] {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[test]
fn clear_statistics_empties_the_word_tables_too() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "Промпт и агент");
    store.clear().unwrap();
    let conn = store.connection().unwrap();
    assert!(top(&conn, &all(), true).is_empty());
    assert_eq!(store.backfill_status().state, BackfillState::Done);
}

#[test]
fn cleanup_drops_old_single_use_phrases_but_keeps_repeated_ones() {
    let (_dir, store) = open();
    let old = day(2026, 1, 5);
    let old2 = day(2026, 1, 6);
    say(&store, old, None, "уникальная фраза здесь");
    // a phrase used once on two different days is NOT single-use
    say(&store, old, None, "повторная фраза тут");
    say(&store, old2, None, "повторная фраза тут");
    // a recent single-use phrase stays for now
    say(&store, day(2026, 10, 20), None, "свежая фраза тоже");
    let conn = store.connection().unwrap();
    let today = day(2026, 10, 22);
    let removed = index::cleanup_ngrams(&conn, noon(today), today).unwrap();
    assert!(removed > 0);
    let has = |ngram: &str| -> bool {
        conn.query_row("SELECT COUNT(*) FROM ngram_daily WHERE ngram = ?1", [ngram], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
            > 0
    };
    assert!(!has("уникальная фраза"));
    assert!(has("повторная фраза"), "used on two days: kept");
    assert!(has("свежая фраза"), "recent: kept");
    // weekly: a second run right away does nothing
    assert_eq!(index::cleanup_ngrams(&conn, noon(today) + 60, today).unwrap(), 0);
}

#[test]
fn word_examples_highlight_the_word_in_the_newest_dictations() {
    let (_dir, store) = open();
    for i in 1..=7 {
        say(&store, day(2026, 10, i), None, &format!("Запись {i}: поправил промпт и пошёл дальше"));
    }
    say(&store, day(2026, 10, 8), None, "Здесь вообще нет нужного слова");
    let conn = store.connection().unwrap();
    let stem = query::top_words(&conn, &all(), false, 50, &rolled(&conn))
        .unwrap()
        .into_iter()
        .find(|w| w.word == "промпт")
        .unwrap()
        .stem;
    let detail = query::word_detail(&conn, &all(), &stem, &Utc).unwrap();
    assert_eq!(detail.examples.len(), query::EXAMPLE_COUNT);
    // newest first
    assert!(detail.examples[0].created_at > detail.examples[4].created_at);
    for example in &detail.examples {
        let hits: Vec<&str> = example.segments.iter().filter(|s| s.hit).map(|s| s.text.as_str()).collect();
        assert_eq!(hits, vec!["промпт"]);
        let text: String = example.segments.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("поправил промпт и"), "{text}");
    }
}

#[test]
fn examples_are_found_after_an_apostrophe_earlier_in_the_text() {
    let (_dir, store) = open();
    say(&store, day(2026, 10, 5), None, "Спросил у Claude'а, потом поправил промпт");
    let conn = store.connection().unwrap();
    let stem = query::top_words(&conn, &all(), false, 50, &rolled(&conn))
        .unwrap()
        .into_iter()
        .find(|w| w.word == "промпт")
        .unwrap()
        .stem;
    let detail = query::word_detail(&conn, &all(), &stem, &Utc).unwrap();
    assert_eq!(detail.examples.len(), 1);
}

#[test]
fn long_examples_are_cut_around_the_word_on_word_edges() {
    let filler = "слово ".repeat(80);
    let text = format!("{filler}промпт {filler}");
    let hit = text.find("промпт").unwrap();
    let segments = query::tests_example_segments(&text, &[(hit, hit + "промпт".len())]);
    let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert!(joined.starts_with('\u{2026}') && joined.ends_with('\u{2026}'));
    assert!(joined.len() < text.len() / 2);
    assert!(segments.iter().any(|s| s.hit && s.text == "промпт"));
    // no half words next to the cut
    let inner = joined.trim_matches('\u{2026}');
    assert!(inner.split_whitespace().all(|w| w == "слово" || w == "промпт"), "{inner}");
}

#[test]
fn backfill_indexes_old_dictations_once_and_resumes() {
    // A database as the first Voxa build left it: version 1, no word tables.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(DB_FILE_NAME);
    crate::voxa::store::tests::legacy_v1_db(
        &path,
        &[
            (noon(day(2026, 10, 1)), "Промпт для агента"),
            (noon(day(2026, 10, 2)), "Промпт и пайплайн"),
            (noon(day(2026, 10, 3)), "Снова промпт"),
            (noon(day(2026, 10, 3)), "Ну короче да"),
            (noon(day(2026, 10, 4)), "Последний агент"),
        ],
    );

    let store = VoxaStore::open(&path, None).unwrap();
    let status = store.backfill_status();
    assert_eq!(status.state, BackfillState::Running);
    assert_eq!(status.total, 5);

    // a dictation recorded while the backfill is pending is indexed right away
    // and must not be counted again when the backfill passes it
    say(&store, day(2026, 10, 5), None, "Живой промпт");

    // interrupted after one batch of two
    let conn = store.connection().unwrap();
    assert!(index::backfill_batch(&conn, 2).unwrap());
    let partial = stem_counts(&conn);

    // the app restarts: reopen and finish
    drop(conn);
    drop(store);
    let store = VoxaStore::open(&path, None).unwrap();
    let mut seen = Vec::new();
    store.run_words_backfill(|s| seen.push(s.clone())).unwrap();
    assert_eq!(store.backfill_status().state, BackfillState::Done);
    assert!(seen.last().is_some_and(|s| s.state == BackfillState::Done && s.processed == s.total));

    let conn = store.connection().unwrap();
    let finished = stem_counts(&conn);
    let prompt = |m: &std::collections::BTreeMap<String, i64>| {
        m.iter().find(|(k, _)| k.starts_with("промп")).map(|(_, v)| *v).unwrap_or(0)
    };
    assert_eq!(
        prompt(&partial),
        3,
        "the live dictation and the first two old rows were indexed before the restart"
    );
    assert_eq!(prompt(&finished), 4, "four промпт in all, none counted twice");
    assert_eq!(query::vocabulary(&conn, &all(), &rolled(&conn)).unwrap().total, 15);

    // running it again changes nothing
    drop(conn);
    store.run_words_backfill(|_| {}).unwrap();
    let again = VoxaStore::open(&path, None).unwrap();
    assert_eq!(again.backfill_status().state, BackfillState::Done);
    let conn = again.connection().unwrap();
    assert_eq!(stem_counts(&conn), finished);
}

#[test]
fn a_fresh_database_needs_no_backfill() {
    let (_dir, store) = open();
    assert_eq!(store.backfill_status().state, BackfillState::Done);
}

#[test]
fn existing_data_is_not_touched_by_the_word_migration() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(DB_FILE_NAME);
    crate::voxa::store::tests::legacy_v1_db(&path, &[(noon(day(2026, 10, 1)), "Промпт для агента")]);
    let before: (i64, String, i64) = Connection::open(&path)
        .unwrap()
        .query_row("SELECT id, final_text, word_count FROM dictations", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    let store = VoxaStore::open(&path, None).unwrap();
    let conn = store.connection().unwrap();
    let after: (i64, String, i64) = conn
        .query_row("SELECT id, final_text, word_count FROM dictations", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(before, after);
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 2);
}

// ------------------------------------------------------------- performance

/// Pseudo-random text from a fixed vocabulary with a skewed distribution, so
/// the synthetic database has a realistic shape: a few very frequent words, a
/// long tail, many unique phrases.
fn synthetic_text(seed: u64, words: &[String]) -> String {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let length = 20 + next() % 60;
    let mut out = String::new();
    for i in 0..length {
        // skew: square the uniform draw so low indexes are far more common
        let r = (next() % 1000) as f64 / 1000.0;
        let idx = ((r * r * r) * words.len() as f64) as usize;
        let word = &words[idx.min(words.len() - 1)];
        if i == 0 {
            let mut chars = word.chars();
            out.extend(chars.next().map(|c| c.to_uppercase().collect::<String>()));
            out.push_str(chars.as_str());
        } else {
            out.push_str(word);
        }
        out.push(if next() % 11 == 0 { '.' } else if next() % 7 == 0 { ',' } else { ' ' });
        if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out
}

fn vocabulary_for_tests(size: usize) -> Vec<String> {
    let seeds = ["ну", "короче", "как", "бы", "промпт", "агент", "модель", "сделай", "так", "чтобы"];
    let mut words: Vec<String> = seeds.iter().map(|s| s.to_string()).collect();
    let letters: Vec<char> = "абвгдежзиклмнопрстуфхцчшэюя".chars().collect();
    let mut n = 0usize;
    while words.len() < size {
        n += 1;
        let mut w = String::new();
        let mut x = n * 2654435761 % 1_000_003;
        for _ in 0..(4 + x % 6) {
            w.push(letters[x % letters.len()]);
            x = x / 3 + n;
        }
        w.push_str(["", "а", "ом", "ы", "ать"][n % 5]);
        words.push(w);
    }
    words
}

/// Records `dictations` synthetic dictations; returns the total time and the
/// average per dictation over the last thousand (the database is full by then).
fn fill(store: &VoxaStore, dictations: usize) -> (std::time::Duration, f64) {
    let words = vocabulary_for_tests(6000);
    let first = day(2024, 11, 1);
    let started = std::time::Instant::now();
    let tail_from = dictations.saturating_sub(1_000);
    let mut tail_started = started;
    for i in 0..dictations {
        if i == tail_from {
            tail_started = std::time::Instant::now();
        }
        let date = first + chrono::Duration::days((i * 700 / dictations) as i64);
        store
            .record(&DictationInput {
                created_at: noon(date) + (i % 20) as i64 * 60,
                final_text: synthetic_text(i as u64, &words),
                ..Default::default()
            })
            .unwrap();
    }
    let tail = (dictations - tail_from).max(1) as f64;
    (
        started.elapsed(),
        tail_started.elapsed().as_secs_f64() * 1000.0 / tail,
    )
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> (T, std::time::Duration) {
    let started = std::time::Instant::now();
    let value = f();
    let elapsed = started.elapsed();
    eprintln!("  {label}: {elapsed:?}");
    (value, elapsed)
}

fn run_all_queries(conn: &Connection, period: &WordsPeriod, label: &str) -> Vec<std::time::Duration> {
    let fillers: Vec<String> = crate::voxa::defaults::default_filler_words();
    let today = day(2026, 10, 1);
    let stem = query::top_words(conn, period, false, 1, &rolled(conn)).unwrap().remove(0).stem;
    vec![
        timed(&format!("{label} top words"), || query::top_words(conn, period, false, 50, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} top words + stop"), || query::top_words(conn, period, true, 50, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} openers"), || query::openers(conn, period, 10).unwrap()).1,
        timed(&format!("{label} phrases"), || query::phrases(conn, period, 15, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} fillers"), || query::fillers(conn, period, &fillers, today, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} new words"), || query::new_words(conn, period, today, false, 30, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} vocabulary"), || query::vocabulary(conn, period, &rolled(conn)).unwrap()).1,
        timed(&format!("{label} word detail"), || query::word_detail(conn, period, &stem, &Utc).unwrap()).1,
    ]
}

/// A small database, run with the normal tests (debug builds index slowly): the queries must not be slow
/// even in a debug build.
#[test]
fn queries_stay_fast_on_a_few_hundred_dictations() {
    let (_dir, store) = open();
    fill(&store, 300);
    let conn = store.connection().unwrap();
    for d in run_all_queries(&conn, &all(), "300 all") {
        assert!(d.as_millis() < 1500, "{d:?}");
    }
}

/// The benchmark of PLAN-2 B.4: 20 000 dictations, every query under 150 ms and
/// one dictation under 5 ms to index. Run it in release mode:
///
///     cargo test --release words::tests::bench -- --ignored --nocapture
#[test]
#[ignore = "benchmark: run with --release -- --ignored --nocapture"]
fn bench_twenty_thousand_dictations() {
    let (_dir, store) = open();
    let (elapsed, per_dictation) = fill(&store, 20_000);
    eprintln!(
        "indexing: {per_dictation:.3} ms per dictation over the last 1000 \
         ({elapsed:?} for 20000)"
    );
    let conn = store.connection().unwrap();
    // what the app does at start: roll the days up, and once a week trim phrases
    let today = day(2026, 10, 1);
    let (_, rolled_in) = timed("roll-up of all days", || {
        index::roll_up(&conn, &index::yesterday(today)).unwrap()
    });
    eprintln!("roll-up took {rolled_in:?}");
    let (removed, took) = timed("weekly phrase cleanup", || {
        index::cleanup_ngrams(&conn, noon(today), today).unwrap()
    });
    eprintln!("cleanup removed {removed} single-use phrases in {took:?}");
    let rows = |t: &str| -> i64 { conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap() };
    eprintln!(
        "rows: form_daily {}, ngram_daily {}, word_total {}, ngram_total {}",
        rows("form_daily"),
        rows("ngram_daily"),
        rows("word_total"),
        rows("ngram_total")
    );
    let size = std::fs::metadata(_dir.path().join(DB_FILE_NAME)).unwrap().len();
    eprintln!("database file: {:.1} MB", size as f64 / 1e6);

    let mut worst = std::time::Duration::ZERO;
    for (label, period) in [
        ("all time", all()),
        ("last 30 days", between(day(2026, 9, 1), day(2026, 10, 1))),
        ("last 7 days", between(day(2026, 9, 24), day(2026, 10, 1))),
    ] {
        for d in run_all_queries(&conn, &period, label) {
            worst = worst.max(d);
        }
    }
    eprintln!("slowest query: {worst:?}");
    assert!(per_dictation < 5.0, "indexing took {per_dictation:.3} ms per dictation");
}

// ------------------------------------------------------------------ roll-up

/// Every all-time answer, to compare before and after rolling days up.
fn all_time_answers(conn: &Connection, today: NaiveDate) -> String {
    let r = rolled(conn);
    let fillers: Vec<String> = ["ну", "короче", "как бы", "типа"].iter().map(|s| s.to_string()).collect();
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
        query::top_words(conn, &all(), true, 50, &r).unwrap(),
        query::vocabulary(conn, &all(), &r).unwrap(),
        query::phrases(conn, &all(), 15, &r).unwrap(),
        query::fillers(conn, &all(), &fillers, today, &r).unwrap(),
        query::new_words(conn, &all(), today, true, 30, &r).unwrap(),
    )
}

fn busy_history(store: &VoxaStore) {
    let texts = [
        "Ну короче сделай так чтобы работало. Как бы потом проверь",
        "Короче давай посмотрим на пайплайн и на промпт",
        "Сделай так чтобы быстрее. Ну как бы да",
        "Промпт для агента получился хороший, как бы типа",
        "Давай посмотрим на агента ещё раз. Короче потом проверь",
        "Сделай так чтобы работало и пайплайн тоже",
    ];
    for (i, text) in texts.iter().enumerate() {
        // two dictations a day, from 2026-10-01
        let date = day(2026, 10, 1 + (i / 2) as u32);
        say(store, date, Some(i as i64 + 1), text);
    }
}

#[test]
fn rolling_days_up_does_not_change_any_all_time_answer() {
    let (_dir, store) = open();
    busy_history(&store);
    let conn = store.connection().unwrap();
    let today = day(2026, 10, 3);
    let before = all_time_answers(&conn, today);
    assert!(before.contains("сделай так чтобы"), "{before}");

    // partly: the first two days rolled, the last one is the tail
    assert!(index::roll_up(&conn, "2026-10-02").unwrap());
    assert_eq!(rolled(&conn), "2026-10-02");
    assert_eq!(all_time_answers(&conn, today), before);

    // fully
    assert!(index::roll_up(&conn, "2026-10-03").unwrap());
    assert_eq!(all_time_answers(&conn, today), before);
    // and the roll-up is not applied twice
    assert!(!index::roll_up(&conn, "2026-10-03").unwrap());
    assert_eq!(all_time_answers(&conn, today), before);
}

#[test]
fn a_dictation_for_a_rolled_day_reaches_the_totals_and_leaves_them_again() {
    let (_dir, store) = open();
    busy_history(&store);
    let conn = store.connection().unwrap();
    index::roll_up(&conn, "2026-10-03").unwrap();
    let today = day(2026, 10, 4);
    let baseline = all_time_answers(&conn, today);

    // late arrival for a day that is already rolled (e.g. a time zone change)
    say(&store, day(2026, 10, 2), Some(100), "Короче сделай так чтобы работало");
    let with_it = all_time_answers(&conn, today);
    assert_ne!(with_it, baseline);
    let words = top(&conn, &all(), true);
    assert!(words.iter().any(|(w, c)| w == "короче" && *c == 4), "{words:?}");

    // the same answers as a database that never rolled anything up
    let (_d2, plain) = open();
    busy_history(&plain);
    say(&plain, day(2026, 10, 2), Some(100), "Короче сделай так чтобы работало");
    let plain_conn = plain.connection().unwrap();
    assert_eq!(with_it, all_time_answers(&plain_conn, today));

    // deleting it takes it out of the totals too
    assert!(store.remove_by_history_id(100).unwrap());
    assert_eq!(all_time_answers(&conn, today), baseline);
}

#[test]
fn deleting_a_rolled_dictation_updates_the_totals() {
    let (_dir, store) = open();
    busy_history(&store);
    let conn = store.connection().unwrap();
    index::roll_up(&conn, "2026-10-03").unwrap();
    let today = day(2026, 10, 4);
    assert!(store.remove_by_history_id(1).unwrap());

    let (_d2, plain) = open();
    busy_history(&plain);
    assert!(plain.remove_by_history_id(1).unwrap());
    let plain_conn = plain.connection().unwrap();
    assert_eq!(all_time_answers(&conn, today), all_time_answers(&plain_conn, today));
}

#[test]
fn new_words_know_what_the_roll_up_has_seen() {
    let (_dir, store) = open();
    say(&store, day(2026, 9, 1), None, "Старый промпт");
    say(&store, day(2026, 10, 5), None, "Новый пайплайн и промпт");
    say(&store, day(2026, 10, 6), None, "Пайплайн снова и промпт");
    let conn = store.connection().unwrap();
    let october = between(day(2026, 10, 1), day(2026, 10, 31));
    let today = day(2026, 10, 31);
    let ask = |conn: &Connection| -> Vec<String> {
        query::new_words(conn, &october, today, true, 30, &rolled(conn))
            .unwrap()
            .into_iter()
            .map(|w| w.word)
            .collect()
    };
    let unrolled = ask(&conn);
    // September rolled up, October still in the tail
    index::roll_up(&conn, "2026-09-30").unwrap();
    assert_eq!(ask(&conn), unrolled);
    // everything rolled
    index::roll_up(&conn, "2026-10-30").unwrap();
    assert_eq!(ask(&conn), unrolled);
    assert!(unrolled.contains(&"пайплайн".to_string()), "{unrolled:?}");
    assert!(!unrolled.contains(&"промпт".to_string()), "{unrolled:?}");
}

#[test]
fn cleanup_works_on_rolled_totals_too() {
    let (_dir, store) = open();
    say(&store, day(2026, 1, 5), None, "уникальная фраза здесь");
    say(&store, day(2026, 1, 5), None, "повторная фраза тут");
    say(&store, day(2026, 1, 6), None, "повторная фраза тут");
    let conn = store.connection().unwrap();
    let today = day(2026, 10, 22);
    index::cleanup_ngrams(&conn, noon(today), today).unwrap();
    let total = |ngram: &str| -> i64 {
        conn.query_row("SELECT COUNT(*) FROM ngram_total WHERE ngram = ?1", [ngram], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(total("уникальная фраза"), 0);
    assert_eq!(total("повторная фраза"), 1);
    // the all-time list still has the repeated phrase and its full count
    let phrases = query::phrases(&conn, &all(), 10, &rolled(&conn)).unwrap();
    assert_eq!(phrases.bigrams[0].text, "повторная фраза");
    assert_eq!(phrases.bigrams[0].count, 2);
}
