use similar::{DiffTag, TextDiff};
use std::ops::Range;
use std::time::{Duration, Instant};

pub struct MergeResult {
    pub text: String,
    pub conflict: bool,
}

struct Edit {
    range: Range<usize>,
    text: String,
}

fn edits(base: &str, changed: &str, deadline: Instant) -> Option<Vec<Edit>> {
    let chars: Vec<char> = changed.chars().collect();
    let started = Instant::now();
    let budget = deadline
        .checked_duration_since(started)?
        .min(Duration::from_millis(20));
    let diff = TextDiff::configure()
        .timeout(budget)
        .diff_chars(base, changed);
    // Similar does not expose whether it approximated. Conservatively request
    // review whenever the configured time budget was consumed.
    if started.elapsed() >= budget {
        return None;
    }
    let original: Vec<char> = base.chars().collect();
    let mut result = Vec::new();
    let (mut old_cursor, mut new_cursor) = (0, 0);
    for op in diff.ops() {
        let (tag, old, new) = op.as_tag_tuple();
        if tag != DiffTag::Equal {
            continue;
        }
        // Several LCS alignments can be equally short. A trailing letter from
        // an existing word must not jump into newly appended speech: Jon ->
        // "John! and more" can otherwise align n with the n in "and", making
        // "hn! a" one conflicting insertion and silently dropping that speech.
        // Move each unchanged anchor to its earliest exact occurrence, keeping
        // anchor order and rebuilding the gaps. Every result still reconstructs
        // `changed` exactly, and both manual and ASR edits use the same rule.
        let mut start = new.start;
        for candidate in new_cursor..=new.start {
            if Instant::now() >= deadline {
                return None;
            }
            if chars[candidate..candidate + old.len()] == original[old.clone()] {
                start = candidate;
                break;
            }
        }
        if old_cursor != old.start || new_cursor != start {
            result.push(Edit {
                range: old_cursor..old.start,
                text: chars[new_cursor..start].iter().collect(),
            });
        }
        old_cursor = old.end;
        new_cursor = start + old.len();
    }
    if old_cursor != original.len() || new_cursor != chars.len() {
        result.push(Edit {
            range: old_cursor..original.len(),
            text: chars[new_cursor..].iter().collect(),
        });
    }
    (Instant::now() < deadline).then_some(result)
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => a.start == b.start,
        (true, false) => b.start < a.start && a.start < b.end,
        (false, true) => a.start < b.start && b.start < a.end,
        (false, false) => a.start < b.end && b.start < a.end,
    }
}

/// Reconcile exact displayed ASR with a draft. Conflicting ASR remains separately
/// available in EditorState; a broad timeout approximation requires review.
#[cfg(test)]
pub fn merge(base: &str, user: &str, remote: &str) -> MergeResult {
    merge_before(
        base,
        user,
        remote,
        Instant::now() + Duration::from_millis(40),
    )
    .unwrap_or_else(|| MergeResult {
        text: user.into(),
        conflict: true,
    })
}

/// All edits in a provenance replay share this deadline. A timed-out diff is
/// not evidence that the unprocessed recognition can safely replace a draft.
pub fn merge_before(
    base: &str,
    user: &str,
    remote: &str,
    deadline: Instant,
) -> Option<MergeResult> {
    if Instant::now() >= deadline {
        return None;
    }
    if user == base {
        return Some(MergeResult {
            text: remote.into(),
            conflict: false,
        });
    }
    if remote == base || user == remote {
        return Some(MergeResult {
            text: user.into(),
            conflict: false,
        });
    }
    // A whole corrected phrase followed by a word boundary is recognition
    // catching up with that correction. Preserve its new speech directly;
    // character LCS may otherwise match a replaced letter to a later word
    // (cafe -> "café encore"). Never use this for a shortened prefix of the
    // original: "old tail" -> "old" must not resurrect the deleted tail.
    if !user.is_empty() && !base.starts_with(user) {
        if let Some(suffix) = remote.strip_prefix(user) {
            let boundary = |c: char| c.is_whitespace() || c.is_ascii_punctuation();
            if suffix.chars().next().is_some_and(boundary)
                || user.chars().next_back().is_some_and(boundary)
            {
                return Some(MergeResult {
                    text: remote.into(),
                    conflict: false,
                });
            }
            // A partial-word match does not establish that recognition adopted
            // the user's correction; retain it rather than splice the rest of
            // a differently recognized word onto it.
            return Some(MergeResult {
                text: user.into(),
                conflict: true,
            });
        }
    }
    if let Some(suffix) = remote.strip_prefix(base) {
        // Recognition can catch up with a manual insertion. Do not duplicate
        // that insertion when it is also the prefix of newly dictated speech.
        if let Some(user_suffix) = user.strip_prefix(base) {
            if suffix.starts_with(user_suffix) {
                return Some(MergeResult {
                    text: remote.into(),
                    conflict: false,
                });
            }
            if user_suffix.starts_with(suffix) {
                return Some(MergeResult {
                    text: user.into(),
                    conflict: false,
                });
            }
        }
        return Some(MergeResult {
            text: format!("{user}{suffix}"),
            conflict: false,
        });
    }
    let mut accepted = edits(base, user, deadline)?;
    let remote_edits = edits(base, remote, deadline)?;
    let mut conflict = false;
    for remote_edit in remote_edits {
        if Instant::now() >= deadline {
            return None;
        }
        if accepted
            .iter()
            .any(|edit| edit.range == remote_edit.range && edit.text == remote_edit.text)
        {
            continue;
        }
        if accepted
            .iter()
            .any(|edit| overlaps(&edit.range, &remote_edit.range))
        {
            conflict = true;
        } else {
            accepted.push(remote_edit);
        }
    }
    accepted.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let base: Vec<char> = base.chars().collect();
    let mut cursor = 0;
    let mut text = String::new();
    for edit in accepted {
        if Instant::now() >= deadline {
            return None;
        }
        text.extend(base[cursor..edit.range.start].iter().copied());
        text.push_str(&edit.text);
        cursor = edit.range.end;
    }
    text.extend(base[cursor..].iter().copied());
    (Instant::now() < deadline).then_some(MergeResult { text, conflict })
}

#[cfg(test)]
mod tests {
    use super::merge;

    #[test]
    fn unicode_edits_use_character_offsets() {
        let result = merge("你好 café", "您好 café", "你好 cafés");
        assert_eq!(result.text, "您好 cafés");
        assert!(!result.conflict);
    }

    #[test]
    fn end_edits_preserve_new_dictation() {
        assert_eq!(merge("old tail", "old", "old tail next").text, "old next");
        assert_eq!(
            merge("old", "old manual", "old next").text,
            "old manual next"
        );
        assert_eq!(merge("old", "", "old next").text, " next");
    }

    #[test]
    fn disjoint_tentative_rewrite_preserves_correction() {
        let result = merge("cat runs slow", "dog runs slow", "cat runs fast");
        assert_eq!(result.text, "dog runs fast");
        assert!(!result.conflict);
    }

    #[test]
    fn overlapping_rewrite_keeps_user_and_flags_review() {
        let result = merge("the cat", "the dog", "the fox");
        assert_eq!(result.text, "the dog");
        assert!(result.conflict);
    }

    #[test]
    fn recognition_agreeing_with_manual_suffix_does_not_duplicate_it() {
        let result = merge("John", "John!", "John! next");
        assert_eq!(result.text, "John! next");
        assert!(!result.conflict);
    }

    #[test]
    fn agreeing_correction_keeps_new_words_with_repeated_original_characters() {
        for (base, user, remote) in [
            ("Jon", "John", "John! and more"),
            ("Jon", "John", "John and another John"),
            ("Jon stays", "John stays", "John! and more stays"),
            ("Ann", "Anne", "Anne and another noun"),
            ("cafe", "café", "café encore"),
            ("cat", "dog", "dog and cat more"),
        ] {
            let result = merge(base, user, remote);
            assert_eq!(result.text, remote, "{base:?} -> {user:?}");
            assert!(!result.conflict);
        }
    }

    #[test]
    fn agreeing_prefix_does_not_resurrect_a_deleted_tail_or_match_midword() {
        assert_eq!(merge("old tail", "old", "old tail next").text, "old next");
        assert_eq!(merge("car", "ca", "carpet").text, "capet");
        let result = merge("cat", "car", "carpet");
        assert_ne!(result.text, "carpet");
        assert!(result.conflict);
    }
}
