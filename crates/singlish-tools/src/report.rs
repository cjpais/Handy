//! Scoring rows, console tables and CSV output for the bakeoff.

use crate::mer::{score, Score};

#[derive(Debug, Clone)]
pub struct ClipResult {
    pub clip: String,
    pub reference: String,
    pub raw: String,
    pub cleaned: String,
}

impl ClipResult {
    pub fn before(&self) -> Score {
        score(&self.reference, &self.raw)
    }
    pub fn after(&self) -> Score {
        score(&self.reference, &self.cleaned)
    }
}

/// Corpus-level score: total errors over total reference tokens.
pub fn total(scores: impl Iterator<Item = Score>) -> Score {
    scores.fold(
        Score {
            errors: 0,
            ref_tokens: 0,
        },
        |a, s| Score {
            errors: a.errors + s.errors,
            ref_tokens: a.ref_tokens + s.ref_tokens,
        },
    )
}

fn pct(s: Score) -> String {
    format!("{:.1}%", s.rate() * 100.0)
}

/// A per-engine table: one row per clip plus a TOTAL row.
pub fn format_table(engine: &str, results: &[ClipResult]) -> String {
    let width = results
        .iter()
        .map(|r| r.clip.len())
        .max()
        .unwrap_or(4)
        .max(5);
    let mut out = format!("== {engine} ==\n");
    out += &format!(
        "{:<width$}  {:>6}  {:>10}  {:>10}\n",
        "clip", "tokens", "MER before", "MER after"
    );
    for r in results {
        out += &format!(
            "{:<width$}  {:>6}  {:>10}  {:>10}\n",
            r.clip,
            r.before().ref_tokens,
            pct(r.before()),
            pct(r.after())
        );
    }
    out += &format!(
        "{:<width$}  {:>6}  {:>10}  {:>10}\n",
        "TOTAL",
        total(results.iter().map(|r| r.before())).ref_tokens,
        pct(total(results.iter().map(|r| r.before()))),
        pct(total(results.iter().map(|r| r.after())))
    );
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub const CSV_HEADER: &str =
    "engine,clip,ref_tokens,errors_before,errors_after,mer_before,mer_after,reference,hypothesis_raw,hypothesis_cleaned";

pub fn csv_rows(engine: &str, results: &[ClipResult]) -> Vec<String> {
    let mut rows: Vec<String> = results
        .iter()
        .map(|r| {
            let (b, a) = (r.before(), r.after());
            [
                csv_field(engine),
                csv_field(&r.clip),
                b.ref_tokens.to_string(),
                b.errors.to_string(),
                a.errors.to_string(),
                format!("{:.4}", b.rate()),
                format!("{:.4}", a.rate()),
                csv_field(&r.reference),
                csv_field(&r.raw),
                csv_field(&r.cleaned),
            ]
            .join(",")
        })
        .collect();
    let (b, a) = (
        total(results.iter().map(|r| r.before())),
        total(results.iter().map(|r| r.after())),
    );
    rows.push(format!(
        "{},TOTAL,{},{},{},{:.4},{:.4},,,",
        csv_field(engine),
        b.ref_tokens,
        b.errors,
        a.errors,
        b.rate(),
        a.rate()
    ));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ClipResult> {
        vec![
            ClipResult {
                clip: "clip01".into(),
                reference: "ok lah".into(),
                raw: "ok la".into(),
                cleaned: "ok lah".into(),
            },
            ClipResult {
                clip: "clip02".into(),
                reference: "我们去makan".into(),
                raw: "我们去makan".into(),
                cleaned: "我们去makan".into(),
            },
        ]
    }

    #[test]
    fn total_is_corpus_level() {
        let r = sample();
        // 1 error over 2 + 4 = 6 tokens before; 0 after.
        let b = total(r.iter().map(|x| x.before()));
        assert_eq!((b.errors, b.ref_tokens), (1, 6));
        assert_eq!(total(r.iter().map(|x| x.after())).errors, 0);
    }

    #[test]
    fn table_contains_rates() {
        let t = format_table("sensevoice", &sample());
        assert!(t.contains("== sensevoice =="));
        assert!(t.contains("50.0%"));
        assert!(t.contains("TOTAL"));
        assert!(t.contains("16.7%"));
    }

    #[test]
    fn csv_escapes_commas_and_quotes() {
        let rows = csv_rows(
            "e",
            &[ClipResult {
                clip: "c".into(),
                reference: "a, \"b\"".into(),
                raw: "a".into(),
                cleaned: "a".into(),
            }],
        );
        assert!(rows[0].contains("\"a, \"\"b\"\"\""));
        assert!(rows.last().unwrap().contains("TOTAL"));
    }
}
