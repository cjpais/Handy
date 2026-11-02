//! Inline diff shown after a transcription result.
//!
//! Only lines that changed are returned. Whitespace runs are collapsed to a
//! single space before comparing, and highlights cover whole words. Shared
//! words stay plain. A line whose words all changed is one deletion segment
//! and one insertion segment, not a highlight per word. Identical text, an
//! empty side, or a paste with no "before" text produces nothing.

use serde::Serialize;

/// How long the result diff stays up before the overlay hides itself.
pub const DIFF_OVERLAY_HIDE_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffSpanKind {
    Same,
    Insert,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffSpan {
    pub kind: DiffSpanKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub spans: Vec<DiffSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiffOverlay {
    pub lines: Vec<DiffLine>,
}

/// Choose the two texts to compare, then diff them.
///
/// Context scribe compares the captured selection with the model reply.
/// Regular post-processing compares the transcript with the processed text.
/// Anything else (plain paste, a missing side, identical text) is `None`.
pub fn result_overlay(
    context_scribe: bool,
    selection: &str,
    transcript: &str,
    processed: Option<&str>,
) -> Option<DiffOverlay> {
    let (before, after) = if context_scribe {
        if selection.is_empty() {
            return None;
        }
        (selection, processed?)
    } else {
        let after = processed?;
        if transcript.is_empty() {
            return None;
        }
        (transcript, after)
    };
    if after.is_empty() || before == after {
        return None;
    }
    inline_diff(before, after)
}

pub fn inline_diff(before: &str, after: &str) -> Option<DiffOverlay> {
    if before == after {
        return None;
    }
    let lines = changed_lines(before, after);
    if lines.is_empty() {
        None
    } else {
        Some(DiffOverlay { lines })
    }
}

fn changed_lines(before: &str, after: &str) -> Vec<DiffLine> {
    let old_lines = normalized_lines(before);
    let new_lines = normalized_lines(after);
    let ops = edits(&old_lines, &new_lines);

    let mut lines = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;
    let mut op_index = 0;
    while op_index < ops.len() {
        if ops[op_index] == Edit::Keep {
            old_index += 1;
            new_index += 1;
            op_index += 1;
            continue;
        }

        let mut deleted = Vec::new();
        let mut inserted = Vec::new();
        while op_index < ops.len() && ops[op_index] != Edit::Keep {
            match ops[op_index] {
                Edit::Delete => {
                    deleted.push(old_lines[old_index].as_str());
                    old_index += 1;
                }
                Edit::Insert => {
                    inserted.push(new_lines[new_index].as_str());
                    new_index += 1;
                }
                Edit::Keep => break,
            }
            op_index += 1;
        }

        let paired = deleted.len().min(inserted.len());
        for pair in 0..paired {
            let spans = word_diff(&deleted[pair], &inserted[pair]);
            if spans.iter().any(|span| span.kind != DiffSpanKind::Same) {
                lines.push(DiffLine { spans });
            }
        }
        for line in &deleted[paired..] {
            lines.push(whole_line(DiffSpanKind::Delete, line));
        }
        for line in &inserted[paired..] {
            lines.push(whole_line(DiffSpanKind::Insert, line));
        }
    }
    lines
}

fn whole_line(kind: DiffSpanKind, text: &str) -> DiffLine {
    DiffLine {
        spans: vec![DiffSpan {
            kind,
            text: visible_line(text),
        }],
    }
}

fn visible_line(text: &str) -> String {
    if text.is_empty() {
        " ".to_string()
    } else {
        text.to_string()
    }
}

/// Word diff of one already-normalized line.
///
/// Tokens are whitespace-separated words, so a one-character edit highlights
/// the whole word. Spaces are separators, not tokens that can match: when no
/// word is shared, the line is a single delete span plus a single insert span.
fn word_diff(before: &str, after: &str) -> Vec<DiffSpan> {
    let old_words: Vec<&str> = before.split_whitespace().collect();
    let new_words: Vec<&str> = after.split_whitespace().collect();
    let ops = edits(&old_words, &new_words);

    let mut spans = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;
    let mut op_index = 0;
    while op_index < ops.len() {
        if ops[op_index] == Edit::Keep {
            let mut words = Vec::new();
            while op_index < ops.len() && ops[op_index] == Edit::Keep {
                words.push(old_words[old_index]);
                old_index += 1;
                new_index += 1;
                op_index += 1;
            }
            let mut text = words.join(" ");
            // A separator before this shared run belongs to the plain text,
            // including when the run follows an insertion that consumed no
            // old words.
            if !spans.is_empty() {
                text.insert(0, ' ');
            }
            if op_index < ops.len() {
                text.push(' ');
            }
            push_span(&mut spans, DiffSpanKind::Same, &text);
            continue;
        }

        let mut deleted = Vec::new();
        let mut inserted = Vec::new();
        while op_index < ops.len() && ops[op_index] != Edit::Keep {
            match ops[op_index] {
                Edit::Delete => {
                    deleted.push(old_words[old_index]);
                    old_index += 1;
                }
                Edit::Insert => {
                    inserted.push(new_words[new_index]);
                    new_index += 1;
                }
                Edit::Keep => break,
            }
            op_index += 1;
        }
        if !deleted.is_empty() {
            push_span(&mut spans, DiffSpanKind::Delete, &deleted.join(" "));
        }
        if !inserted.is_empty() {
            push_span(&mut spans, DiffSpanKind::Insert, &inserted.join(" "));
        }
    }
    spans
}

fn push_span(spans: &mut Vec<DiffSpan>, kind: DiffSpanKind, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.kind == kind {
            last.text.push_str(text);
            return;
        }
    }
    spans.push(DiffSpan {
        kind,
        text: text.to_string(),
    });
}

/// Split on newlines and collapse each line's whitespace to single spaces.
fn normalized_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Keep,
    Delete,
    Insert,
}

/// Longest-common-subsequence edit script. Huge inputs fall back to
/// "delete everything, then insert everything" so a long selection cannot
/// stall the paste on an n×m table.
fn edits<T: Eq>(before: &[T], after: &[T]) -> Vec<Edit> {
    let n = before.len();
    let m = after.len();
    const MAX_CELLS: usize = 200_000;
    if n.saturating_mul(m) > MAX_CELLS {
        return std::iter::repeat(Edit::Delete)
            .take(n)
            .chain(std::iter::repeat(Edit::Insert).take(m))
            .collect();
    }

    let mut score = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in 1..=n {
        for j in 1..=m {
            score[at(i, j)] = if before[i - 1] == after[j - 1] {
                score[at(i - 1, j - 1)] + 1
            } else {
                score[at(i - 1, j)].max(score[at(i, j - 1)])
            };
        }
    }

    let mut ops = Vec::new();
    let mut i = n;
    let mut j = m;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && before[i - 1] == after[j - 1] {
            ops.push(Edit::Keep);
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || score[at(i, j - 1)] >= score[at(i - 1, j)]) {
            ops.push(Edit::Insert);
            j -= 1;
        } else {
            ops.push(Edit::Delete);
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &DiffLine) -> Vec<(&str, &str)> {
        line.spans
            .iter()
            .map(|span| {
                let kind = match span.kind {
                    DiffSpanKind::Same => "same",
                    DiffSpanKind::Insert => "insert",
                    DiffSpanKind::Delete => "delete",
                };
                (kind, span.text.as_str())
            })
            .collect()
    }

    #[test]
    fn identical_text_has_no_overlay() {
        assert!(inline_diff("same text", "same text").is_none());
    }

    #[test]
    fn unchanged_lines_are_omitted() {
        let diff =
            inline_diff("keep\nchange me\nalso keep", "keep\nchanged me\nalso keep").unwrap();
        assert_eq!(diff.lines.len(), 1);
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![("delete", "change"), ("insert", "changed"), ("same", " me"),]
        );
    }

    #[test]
    fn word_replacement_keeps_the_shared_words() {
        let diff = inline_diff("The quick brown fox", "The slow brown fox").unwrap();
        assert_eq!(diff.lines.len(), 1);
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![
                ("same", "The "),
                ("delete", "quick"),
                ("insert", "slow"),
                ("same", " brown fox"),
            ]
        );
    }

    #[test]
    fn added_and_removed_lines_are_whole_line_spans() {
        let replaced = inline_diff("keep\ngone", "keep\nnew").unwrap();
        assert_eq!(replaced.lines.len(), 1);
        assert_eq!(
            kinds(&replaced.lines[0]),
            vec![("delete", "gone"), ("insert", "new")]
        );

        let removed = inline_diff("keep\ngone", "keep").unwrap();
        assert_eq!(kinds(&removed.lines[0]), vec![("delete", "gone")]);

        let added = inline_diff("keep", "keep\nextra").unwrap();
        assert_eq!(added.lines.len(), 1);
        assert_eq!(kinds(&added.lines[0]), vec![("insert", "extra")]);
    }

    #[test]
    fn context_scribe_diffs_selection_against_the_model() {
        let diff = result_overlay(
            true,
            "the selected paragraph",
            "make it shorter",
            Some("shorter paragraph"),
        )
        .unwrap();
        let flat: String = diff.lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert!(flat.contains("selected"));
        assert!(flat.contains("shorter"));
        assert!(!flat.contains("make it"));
    }

    #[test]
    fn post_process_diffs_the_transcript_against_processed_text() {
        let diff = result_overlay(false, "ignored selection", "hello um", Some("hello")).unwrap();
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![("same", "hello "), ("delete", "um")]
        );
    }

    #[test]
    fn whole_line_replacement_is_one_segment() {
        let diff = inline_diff("alpha beta gamma", "one two three").unwrap();
        assert_eq!(diff.lines.len(), 1);
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![("delete", "alpha beta gamma"), ("insert", "one two three"),]
        );
    }

    #[test]
    fn whitespace_runs_collapse_before_compare() {
        assert!(inline_diff("foo  bar", "foo bar").is_none());
        assert!(inline_diff("foo\tbar", "foo bar").is_none());

        let diff = inline_diff("The  quick   brown fox", "The slow brown fox").unwrap();
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![
                ("same", "The "),
                ("delete", "quick"),
                ("insert", "slow"),
                ("same", " brown fox"),
            ]
        );
    }

    #[test]
    fn shared_words_keep_a_separator_around_the_change() {
        let inserted = inline_diff("fox", "the fox").unwrap();
        assert_eq!(
            kinds(&inserted.lines[0]),
            vec![("insert", "the"), ("same", " fox")]
        );

        let removed = inline_diff("the fox", "fox").unwrap();
        assert_eq!(
            kinds(&removed.lines[0]),
            vec![("delete", "the"), ("same", " fox")]
        );

        let middle = inline_diff("alpha beta gamma delta", "alpha one two delta").unwrap();
        assert_eq!(
            kinds(&middle.lines[0]),
            vec![
                ("same", "alpha "),
                ("delete", "beta gamma"),
                ("insert", "one two"),
                ("same", " delta"),
            ]
        );
    }

    #[test]
    fn a_change_inside_a_word_highlights_the_whole_word() {
        let diff = inline_diff("change me", "changed me").unwrap();
        assert_eq!(
            kinds(&diff.lines[0]),
            vec![("delete", "change"), ("insert", "changed"), ("same", " me"),]
        );
    }

    #[test]
    fn missing_before_or_after_shows_nothing() {
        assert!(result_overlay(true, "", "spoken", Some("rewritten")).is_none());
        assert!(result_overlay(false, "", "spoken", None).is_none());
        assert!(result_overlay(false, "", "", Some("processed")).is_none());
        assert!(result_overlay(true, "selection", "spoken", None).is_none());
        assert!(result_overlay(true, "selection", "spoken", Some("")).is_none());
        assert!(result_overlay(false, "", "same", Some("same")).is_none());
    }
}
