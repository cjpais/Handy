use super::merge::{merge_before, MergeResult};
use serde::Serialize;
use specta::Type;
use std::time::{Duration, Instant};

const REPLAY_BUDGET: Duration = Duration::from_millis(40);

/// The before/after pair describes only an explicit user change, never a new
/// recognition snapshot. Keeping the pair retains intent when ASR agrees with
/// a correction temporarily, and when the user later undoes that correction.
struct ManualEdit {
    before: String,
    after: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum OverlayEditPhase {
    #[default]
    Inactive,
    Recording,
    Finalizing,
    Review,
    Pasting,
}

/// Private draft and exact visible ASR stay in memory; never derive Debug.
#[derive(Default)]
pub struct EditorState {
    pub session_id: u64,
    pub phase: OverlayEditPhase,
    pub text: String,
    pub asr_text: String,
    pub revision: u64,
    pub user_revision: u64,
    pub conflict: bool,
    pub paste_requested: bool,
    manual_edits: Vec<ManualEdit>,
    replay_blocked: bool,
}

impl EditorState {
    pub fn start_session(&mut self, id: u64) -> bool {
        if self.active() || id == 0 || id <= self.session_id {
            return false;
        }
        *self = Self {
            session_id: id,
            ..Self::default()
        };
        true
    }

    /// Cache or reconcile the exact visible preview, including tentative speech.
    pub fn observe_preview(&mut self, id: u64, text: String) -> bool {
        if id == 0 || id != self.session_id {
            return false;
        }
        match self.phase {
            OverlayEditPhase::Inactive => self.asr_text = text,
            OverlayEditPhase::Recording | OverlayEditPhase::Finalizing => self.merge_asr(text),
            _ => return false,
        }
        self.bump_revision();
        true
    }

    pub fn request(&mut self, id: u64, is_recording: bool) -> Result<(), String> {
        self.require_session(id)?;
        if self.active() {
            return Err("Editor is already active".into());
        }
        self.text.clone_from(&self.asr_text);
        self.phase = if is_recording {
            OverlayEditPhase::Recording
        } else {
            OverlayEditPhase::Finalizing
        };
        self.bump_revision();
        Ok(())
    }

    pub fn request_stop(&mut self, id: u64, paste_requested: bool) -> Result<(), String> {
        self.require_session(id)?;
        if self.phase != OverlayEditPhase::Recording {
            return Err("Editor is not recording".into());
        }
        self.phase = OverlayEditPhase::Finalizing;
        self.paste_requested = paste_requested;
        self.bump_revision();
        Ok(())
    }

    /// Client revision and server snapshot sequence are deliberately independent.
    pub fn update(
        &mut self,
        id: u64,
        user_revision: u64,
        base_asr: String,
        draft: String,
    ) -> Result<bool, String> {
        self.require_session(id)?;
        if !self.editable() || user_revision <= self.user_revision {
            return Ok(false);
        }
        self.accept_draft(user_revision, &base_asr, &draft);
        self.bump_revision();
        Ok(true)
    }

    pub fn finalize(&mut self, id: u64, final_asr: String) -> bool {
        if id == 0 || id != self.session_id || self.phase != OverlayEditPhase::Finalizing {
            return false;
        }
        self.merge_asr(final_asr);
        self.phase = OverlayEditPhase::Review;
        self.bump_revision();
        true
    }

    /// Explicit review submission confirms conflicts, including an unchanged draft.
    pub fn begin_paste(
        &mut self,
        id: u64,
        user_revision: u64,
        base_asr: String,
        draft: String,
    ) -> Result<(), String> {
        self.require_session(id)?;
        if self.phase != OverlayEditPhase::Review {
            return Err("Editor is not ready to paste".into());
        }
        if user_revision < self.user_revision {
            return Err("Editor draft revision is stale".into());
        }
        self.accept_draft(user_revision, &base_asr, &draft);
        self.conflict = false;
        self.phase = OverlayEditPhase::Pasting;
        self.bump_revision();
        Ok(())
    }

    pub fn paste_failed(&mut self, id: u64) -> bool {
        if id == 0 || id != self.session_id || self.phase != OverlayEditPhase::Pasting {
            return false;
        }
        self.phase = OverlayEditPhase::Review;
        self.paste_requested = false;
        self.bump_revision();
        true
    }

    pub fn complete(&mut self, id: u64) -> bool {
        if id == 0 || id != self.session_id || self.phase != OverlayEditPhase::Pasting {
            return false;
        }
        self.cancel();
        true
    }

    pub fn cancel(&mut self) {
        self.phase = OverlayEditPhase::Inactive;
        self.text.clear();
        self.asr_text.clear();
        self.manual_edits.clear();
        self.replay_blocked = false;
        self.conflict = false;
        self.paste_requested = false;
        self.bump_revision();
    }

    pub fn active(&self) -> bool {
        self.phase != OverlayEditPhase::Inactive
    }

    fn editable(&self) -> bool {
        self.phase == OverlayEditPhase::Recording
            || self.phase == OverlayEditPhase::Review
            || (self.phase == OverlayEditPhase::Finalizing && !self.paste_requested)
    }

    fn accept_draft(&mut self, revision: u64, base_asr: &str, draft: &str) {
        let deadline = Instant::now() + REPLAY_BUDGET;
        // The client holds its ASR baseline while an acknowledgement is pending.
        // Replaying previous edits at that baseline reconstructs the draft it
        // edited, even when cumulative updates arrive out of order. Deriving a
        // delta against raw ASR instead would record prior corrections twice.
        let previous = (!self.replay_blocked)
            .then(|| self.replay(base_asr, deadline))
            .flatten();
        if let Some(previous) = previous {
            if previous.text != draft {
                self.manual_edits.push(ManualEdit {
                    before: previous.text,
                    after: draft.into(),
                });
            }
            if let Some(merged) = self.replay(&self.asr_text, deadline) {
                self.text = merged.text;
                self.conflict |= merged.conflict;
            } else {
                self.retain_for_review(draft);
            }
        } else {
            self.retain_for_review(draft);
        }
        self.user_revision = revision;
    }

    fn merge_asr(&mut self, asr: String) {
        self.merge_asr_before(asr, Instant::now() + REPLAY_BUDGET);
    }

    fn merge_asr_before(&mut self, asr: String, deadline: Instant) {
        if !self.replay_blocked {
            if let Some(merged) = self.replay(&asr, deadline) {
                self.text = merged.text;
                self.conflict |= merged.conflict;
            } else {
                // Keep the complete draft, not a partly replayed approximation.
                // The latest full recognition remains available beside it.
                self.replay_blocked = true;
                self.conflict = true;
            }
        }
        self.asr_text = asr;
    }

    fn replay(&self, asr: &str, deadline: Instant) -> Option<MergeResult> {
        let mut result = MergeResult {
            text: asr.into(),
            conflict: false,
        };
        for edit in &self.manual_edits {
            let next = merge_before(&edit.before, &edit.after, &result.text, deadline)?;
            result.text = next.text;
            result.conflict |= next.conflict;
        }
        (Instant::now() < deadline).then_some(result)
    }

    fn retain_for_review(&mut self, draft: &str) {
        self.text = draft.into();
        self.conflict = true;
        self.replay_blocked = true;
    }

    fn require_session(&self, id: u64) -> Result<(), String> {
        if id == 0 || id != self.session_id {
            return Err("Editor session is stale".into());
        }
        Ok(())
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{EditorState, OverlayEditPhase};
    use std::time::{Duration, Instant};

    #[test]
    fn final_automatic_expansion_preserves_manual_name_correction() {
        let base = "my email belongs to Jon";
        let mut state = recording_text(base);
        state
            .update(1, 1, base.into(), "my email belongs to John".into())
            .unwrap();
        state.request_stop(1, false).unwrap();
        assert!(state.finalize(1, "me@example.com belongs to Jon.".into()));
        assert_eq!(state.text, "me@example.com belongs to John.");
    }

    fn recording() -> EditorState {
        recording_text("the cat")
    }

    fn recording_text(text: &str) -> EditorState {
        let mut state = EditorState::default();
        assert!(state.start_session(1));
        assert!(state.observe_preview(1, text.into()));
        assert!(state.request(1, true).is_ok());
        state
    }

    #[test]
    fn edit_midstream_retains_new_dictation_and_final_tail() {
        let mut state = recording();
        assert!(state
            .update(1, 1, "the cat".into(), "the dog".into())
            .unwrap());
        assert!(state.observe_preview(1, "the cat runs".into()));
        assert_eq!(state.text, "the dog runs");
        assert!(state.request_stop(1, true).is_ok());
        assert!(state.observe_preview(1, "the cat runs home".into()));
        assert!(state.finalize(1, "the cat runs home.".into()));
        assert_eq!(state.text, "the dog runs home.");
        assert!(!state.observe_preview(1, "late".into()));
        assert!(state.paste_requested && !state.conflict);
    }

    #[test]
    fn deletion_survives_subsequent_asr_updates() {
        let mut state = recording();
        assert!(state.update(1, 1, "the cat".into(), "cat".into()).unwrap());
        assert!(state.observe_preview(1, "the cat runs".into()));
        assert!(state.observe_preview(1, "the cat runs fast".into()));
        assert_eq!(state.text, "cat runs fast");
    }

    #[test]
    fn conflict_recovery_requires_explicit_review_submission() {
        let mut state = recording();
        assert!(state
            .update(1, 1, "the cat".into(), "the dog".into())
            .unwrap());
        assert!(state.observe_preview(1, "the fox".into()));
        assert_eq!(state.text, "the dog");
        assert_eq!(state.asr_text, "the fox");
        assert!(state
            .update(1, 2, "the fox".into(), "the dog!".into())
            .unwrap());
        assert!(state.conflict);
        assert!(state.request_stop(1, true).is_ok());
        assert!(state.finalize(1, "the fox".into()));
        assert!(state.conflict && state.paste_requested);
        assert!(state
            .begin_paste(1, 2, "the fox".into(), "the dog!".into())
            .is_ok());
        assert!(!state.conflict);
    }

    #[test]
    fn old_sessions_and_client_revisions_cannot_mutate_draft() {
        let mut state = recording();
        assert!(state.update(1, 2, "the cat".into(), "new".into()).unwrap());
        assert!(!state.update(1, 1, "the cat".into(), "old".into()).unwrap());
        assert!(!state.start_session(2));
        state.cancel();
        assert!(state.start_session(2));
        assert!(state.request(2, true).is_ok());
        assert!(state.update(1, 99, "".into(), "stale".into()).is_err());
        assert!(!state.finalize(1, "stale".into()));
        assert!(state.text.is_empty());
        assert_eq!(state.user_revision, 0);
    }

    #[test]
    fn failed_paste_preserves_submission_against_queued_updates() {
        let mut state = recording();
        assert!(state.request_stop(1, true).is_ok());
        assert!(state.finalize(1, "the cat".into()));
        assert!(state
            .begin_paste(1, 3, "the cat".into(), "latest".into())
            .is_ok());
        assert!(!state
            .update(1, 4, "the cat".into(), "during paste".into())
            .unwrap());
        assert!(state.paste_failed(1));
        assert!(!state
            .update(1, 2, "the cat".into(), "delayed".into())
            .unwrap());
        assert_eq!(state.text, "latest");
        assert!(state.phase == OverlayEditPhase::Review);
    }

    #[test]
    fn blank_draft_valid_and_cancellation_clears_private_state() {
        let mut state = recording();
        assert!(state.request_stop(1, false).is_ok());
        assert!(state.finalize(1, "the cat".into()));
        assert!(state
            .begin_paste(1, 1, "the cat".into(), String::new())
            .is_ok());
        assert!(state.complete(1));
        assert!(state.text.is_empty() && state.asr_text.is_empty());
        assert!(!state.paste_requested && !state.conflict && !state.active());
        assert!(!state.start_session(1));
    }

    #[test]
    fn correction_survives_recognition_agreeing_then_reverting() {
        let mut state = recording_text("Jon");
        assert!(state.update(1, 1, "Jon".into(), "John".into()).unwrap());
        assert!(state.observe_preview(1, "John".into()));
        assert_eq!(state.text, "John");
        assert!(state.request_stop(1, false).is_ok());
        assert!(state.finalize(1, "Jon".into()));
        assert_eq!(state.text, "John");
    }

    #[test]
    fn deletion_survives_recognition_agreeing_then_reverting() {
        let mut state = recording_text("the cat");
        assert!(state.update(1, 1, "the cat".into(), "cat".into()).unwrap());
        assert!(state.observe_preview(1, "cat".into()));
        assert!(state.observe_preview(1, "the cat runs".into()));
        assert_eq!(state.text, "cat runs");
    }

    #[test]
    fn further_edit_and_explicit_undo_survive_recognition_reversion() {
        let mut state = recording_text("Jon");
        assert!(state.update(1, 1, "Jon".into(), "John".into()).unwrap());
        assert!(state.observe_preview(1, "John".into()));
        assert!(state.update(1, 2, "John".into(), "John!".into()).unwrap());
        assert!(state.observe_preview(1, "Jon".into()));
        assert_eq!(state.text, "John!");
        assert!(state.update(1, 3, "Jon".into(), "Jon!".into()).unwrap());
        assert!(state.observe_preview(1, "John! and more".into()));
        assert_eq!(state.text, "Jon! and more");
        assert!(state.observe_preview(1, "Jon and more speech".into()));
        assert_eq!(state.text, "Jon! and more speech");
    }

    #[test]
    fn agreeing_correction_and_new_speech_survive_later_reversion_variants() {
        for (original, corrected, suffix) in [
            ("Jon", "John", " and another John"),
            ("cafe", "café", " encore"),
            ("cat", "dog", " and cat more"),
        ] {
            let mut state = recording_text(original);
            assert!(state
                .update(1, 1, original.into(), corrected.into())
                .unwrap());
            assert!(state.observe_preview(1, format!("{corrected}{suffix}")));
            assert_eq!(state.text, format!("{corrected}{suffix}"));
            assert!(state.observe_preview(1, format!("{original}{suffix} now")));
            assert_eq!(state.text, format!("{corrected}{suffix} now"));
        }
    }

    #[test]
    fn user_undo_to_original_word_is_still_explicit_intent() {
        let mut state = recording_text("Jon");
        assert!(state.update(1, 1, "Jon".into(), "John".into()).unwrap());
        assert!(state.update(1, 2, "Jon".into(), "Jon".into()).unwrap());
        assert!(state.observe_preview(1, "John goes home".into()));
        assert_eq!(state.text, "Jon goes home");
    }

    #[test]
    fn pending_cumulative_updates_do_not_duplicate_edits_or_new_speech() {
        let mut state = recording_text("Jon");
        assert!(state.update(1, 1, "Jon".into(), "John".into()).unwrap());
        assert!(state.observe_preview(1, "John goes".into()));
        // Both edits were typed before the client received an acknowledgement;
        // they still carry its original ASR baseline, without the new speech.
        assert!(state.update(1, 3, "Jon".into(), "John!?".into()).unwrap());
        assert!(!state.update(1, 2, "Jon".into(), "John!".into()).unwrap());
        assert_eq!(state.text, "John!? goes");
        assert!(state.observe_preview(1, "Jon goes home".into()));
        assert_eq!(state.text, "John!? goes home");
        // After acknowledgement the user sees the speech too. It must not turn
        // into a second manual insertion when another ASR suffix arrives.
        assert!(state
            .update(1, 4, "Jon goes home".into(), "John!? goes HOME".into())
            .unwrap());
        assert!(state.observe_preview(1, "Jon goes home now".into()));
        assert_eq!(state.text, "John!? goes HOME now");
    }

    #[test]
    fn exhausted_replay_preserves_full_draft_and_exposes_latest_recognition() {
        let mut state = recording_text("Jon");
        assert!(state.update(1, 1, "Jon".into(), "John".into()).unwrap());
        state.merge_asr_before(
            "Jon and more speech".into(),
            Instant::now() - Duration::from_millis(1),
        );
        assert_eq!(state.text, "John");
        assert_eq!(state.asr_text, "Jon and more speech");
        assert!(state.conflict);
        assert!(state
            .update(
                1,
                2,
                "Jon and more speech".into(),
                "My reviewed draft".into()
            )
            .unwrap());
        assert!(state.observe_preview(1, "Another recognition".into()));
        assert_eq!(state.text, "My reviewed draft");
        assert_eq!(state.asr_text, "Another recognition");
        state.cancel();
        assert!(state.manual_edits.is_empty());
        assert!(!state.replay_blocked);
    }
}
