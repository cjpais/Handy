//! Inline diff shown after a transcription result.
//!
//! Only lines that changed are returned. Within a changed line, shared words
//! stay plain, deletions are marked, and insertions are marked. Identical
//! text, an empty side, or a paste with no "before" text produces nothing.

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
    let old_lines = split_lines(before);
    let new_lines = split_lines(after);
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
                    deleted.push(old_lines[old_index]);
                    old_index += 1;
                }
                Edit::Insert => {
                    inserted.push(new_lines[new_index]);
                    new_index += 1;
                }
                Edit::Keep => break,
            }
            op_index += 1;
        }

        let paired = deleted.len().min(inserted.len());
        for pair in 0..paired {
            let spans = word_diff(deleted[pair], inserted[pair]);
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

fn word_diff(before: &str, after: &str) -> Vec<DiffSpan> {
    let old_tokens = tokens(before);
    let new_tokens = tokens(after);
    let ops = edits(&old_tokens, &new_tokens);
    let mut spans = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;
    for op in ops {
        match op {
            Edit::Keep => {
                push_span(&mut spans, DiffSpanKind::Same, old_tokens[old_index]);
                old_index += 1;
                new_index += 1;
            }
            Edit::Delete => {
                push_span(&mut spans, DiffSpanKind::Delete, old_tokens[old_index]);
                old_index += 1;
            }
            Edit::Insert => {
                push_span(&mut spans, DiffSpanKind::Insert, new_tokens[new_index]);
                new_index += 1;
            }
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

fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

fn tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    let mut in_space = None;
    while let Some((index, ch)) = chars.next() {
        let space = ch.is_whitespace();
        if in_space != Some(space) {
            if index > start {
                tokens.push(&text[start..index]);
            }
            start = index;
            in_space = Some(space);
        }
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
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
            vec![("same", "hello"), ("delete", " um")]
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
