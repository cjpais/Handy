//! Session tracker for Handy transcription sessions.
//!
//! A *session* is the full lifecycle of a single hotkey press → text output
//! event: recording starts, recording stops, transcription begins,
//! transcription completes, text is pasted. The `SessionTracker` assigns each
//! session a unique `SessionId` and tracks its phase, allowing all structured
//! log events to be correlated.
//!
//! The tracker also maintains an in-memory ring buffer of the last
//! `SESSION_HISTORY_CAP` session summaries, queryable from the frontend via
//! a Tauri command (Phase 2).

use crate::logging::{self, AppEvent, SessionId};
use log::{debug, info};
use parking_lot::Mutex;
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tauri::Manager;

/// Maximum number of session summaries kept in the ring buffer.
const SESSION_HISTORY_CAP: usize = 200;

/// Phases a session can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Variants used selectively; some reserved for future lifecycle stages
pub enum SessionPhase {
    Recording,
    Transcribing,
    PostProcessing,
    Pasting,
    Done,
    Failed,
}

/// Summary of a completed (or failed) session — stored in the ring buffer
/// for the frontend to query.
#[derive(Debug, Clone, Serialize, Type)]
pub struct SessionSummary {
    pub id: SessionId,
    pub started_at_ms: u64,
    /// Total wall-clock duration in milliseconds (start → end).
    pub duration_ms: Option<u64>,
    pub success: bool,
    pub model_id: Option<String>,
    pub text_length: Option<usize>,
    pub had_post_processing: bool,
    /// JSON-encoded phase durations for convenience (avoids HashMap specta issues).
    pub phases_json: Option<String>,
    /// Errors encountered during the session.
    pub errors: Vec<String>,
}

/// Active (in-progress) session state.
struct ActiveSession {
    id: SessionId,
    started_at: Instant,
    /// Epoch milliseconds of session start, for stable serialisation.
    started_at_ms: u64,
    /// Name of the microphone device, captured at recording start.
    _mic_device: String,
    /// Which ASR model is being used (set when transcription starts).
    model_id: Option<String>,
    /// Current phase of the session.
    phase: SessionPhase,
    /// When the current phase started.
    phase_started_at: Instant,
    /// Accumulated phase durations.
    phases: HashMap<String, u64>,
    /// Errors collected during this session.
    errors: Vec<String>,
    /// Whether post-processing was requested.
    post_process: bool,
    /// Length of the final transcription text (set on completion).
    text_length: Option<usize>,
}

/// Manages session lifecycle and the ring buffer of past sessions.
pub struct SessionTracker {
    current: Mutex<Option<ActiveSession>>,
    history: Mutex<Vec<SessionSummary>>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(None),
            history: Mutex::new(Vec::with_capacity(SESSION_HISTORY_CAP)),
        }
    }

    // ── Session lifecycle ──────────────────────────────────────────

    /// Start a new session. Called when the user presses the hotkey.
    pub fn start_session(&self, mic_device: &str, always_on: bool) -> SessionId {
        let sid = logging::new_session_id();

        // Finalise any previous session that wasn't explicitly ended (guard
        // against leaked sessions from crashes).
        if let Some(prev) = self.current.lock().take() {
            log::warn!(
                "Session {} was still active when new session {} started; finalising as failed",
                prev.id,
                sid
            );
            self.finalise_session(prev, false);
        }

        let now = Instant::now();
        let started_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let session = ActiveSession {
            id: sid.clone(),
            started_at: now,
            started_at_ms,
            _mic_device: mic_device.to_string(),
            model_id: None,
            phase: SessionPhase::Recording,
            phase_started_at: now,
            phases: HashMap::new(),
            errors: Vec::new(),
            post_process: false,
            text_length: None,
        };

        logging::emit(AppEvent::RecordingStarted {
            sid: sid.clone(),
            mic: mic_device.to_string(),
            always_on,
        });

        *self.current.lock() = Some(session);
        debug!("Session {} started (mic: {})", sid, mic_device);
        sid
    }

    /// Mark the transition from Recording → Transcribing.
    /// Called after recording stops and samples are retrieved.
    pub fn advance_to_transcribing(
        &self,
        sid: &SessionId,
        model_id: &str,
        sample_count: usize,
        recording_duration_ms: u64,
    ) {
        let mut guard = self.current.lock();
        if let Some(ref mut session) = *guard {
            if session.id != *sid {
                log::warn!(
                    "Session ID mismatch in advance_to_transcribing: expected {}, got {}",
                    session.id,
                    sid
                );
                return;
            }

            let recording_ms = session.phase_started_at.elapsed().as_millis() as u64;
            session.phases.insert("recording".to_string(), recording_ms);
            session.phase = SessionPhase::Transcribing;
            session.phase_started_at = Instant::now();
            session.model_id = Some(model_id.to_string());

            logging::emit(AppEvent::RecordingStopped {
                sid: sid.clone(),
                sample_count,
                duration_ms: recording_duration_ms,
            });
            logging::emit(AppEvent::TranscriptionStarted {
                sid: sid.clone(),
                model_id: model_id.to_string(),
            });
        }
    }

    /// Mark the transition from Transcribing → PostProcessing (or Done).
    /// Called when transcription completes (success or failure).
    pub fn advance_to_post_processing(
        &self,
        sid: &SessionId,
        text_length: usize,
        transcription_duration_ms: u64,
    ) {
        let mut guard = self.current.lock();
        if let Some(ref mut session) = *guard {
            if session.id != *sid {
                return;
            }

            let transcribing_ms = session.phase_started_at.elapsed().as_millis() as u64;
            session
                .phases
                .insert("transcribing".to_string(), transcribing_ms);
            session.phase = SessionPhase::PostProcessing;
            session.phase_started_at = Instant::now();
            session.text_length = Some(text_length);

            if let Some(ref model_id) = session.model_id {
                logging::emit(AppEvent::TranscriptionCompleted {
                    sid: sid.clone(),
                    model_id: model_id.clone(),
                    text_length,
                    duration_ms: transcription_duration_ms,
                });
            }
        }
    }

    /// Mark the session as done (text pasted successfully).
    pub fn finish_session(&self, sid: &SessionId, paste_duration_ms: u64) {
        let session = self.current.lock().take();
        if let Some(session) = session {
            if session.id != *sid {
                // Put it back
                *self.current.lock() = Some(session);
                return;
            }
            logging::emit(AppEvent::PasteSucceeded {
                sid: sid.clone(),
                duration_ms: paste_duration_ms,
            });
            self.finalise_session(session, true);
        }
    }

    /// Mark the session as failed — e.g. transcription error or paste failure.
    pub fn fail_session(&self, sid: &SessionId, error: &str) {
        let session = self.current.lock().take();
        if let Some(mut session) = session {
            if session.id != *sid {
                // Put it back — different session
                *self.current.lock() = Some(session);
                return;
            }
            session.errors.push(error.to_string());

            // Emit the appropriate failure event based on phase
            match session.phase {
                SessionPhase::Recording => {
                    logging::emit(AppEvent::RecordingFailed {
                        sid: sid.clone(),
                        error: error.to_string(),
                        error_type: "recording".to_string(),
                    });
                }
                SessionPhase::Transcribing => {
                    let model_id = session.model_id.clone().unwrap_or_default();
                    logging::emit(AppEvent::TranscriptionFailed {
                        sid: sid.clone(),
                        model_id,
                        error: error.to_string(),
                        duration_ms: session.phase_started_at.elapsed().as_millis() as u64,
                    });
                }
                SessionPhase::Pasting => {
                    logging::emit(AppEvent::PasteFailed {
                        sid: sid.clone(),
                        error: error.to_string(),
                    });
                }
                _ => {}
            }

            self.finalise_session(session, false);
        }
    }

    /// Record a post-processing failure.
    #[allow(dead_code)]
    pub fn post_process_failed(&self, sid: &SessionId, provider: &str, error: &str) {
        let mut guard = self.current.lock();
        if let Some(ref mut session) = *guard {
            if session.id != *sid {
                return;
            }
            session.errors.push(error.to_string());
            logging::emit(AppEvent::PostProcessFailed {
                sid: sid.clone(),
                provider: provider.to_string(),
                error: error.to_string(),
            });
        }
    }

    /// Mark that post-processing was requested for this session.
    #[allow(dead_code)]
    pub fn set_post_process(&self, sid: &SessionId, provider: &str) {
        let mut guard = self.current.lock();
        if let Some(ref mut session) = *guard {
            if session.id != *sid {
                return;
            }
            session.post_process = true;
            logging::emit(AppEvent::PostProcessStarted {
                sid: sid.clone(),
                provider: provider.to_string(),
            });
        }
    }

    /// Record a post-processing completion.
    #[allow(dead_code)]
    pub fn post_process_completed(&self, sid: &SessionId, provider: &str, duration_ms: u64) {
        let mut guard = self.current.lock();
        if let Some(ref mut session) = *guard {
            if session.id != *sid {
                return;
            }
            session
                .phases
                .insert("post_processing".to_string(), duration_ms);

            logging::emit(AppEvent::PostProcessCompleted {
                sid: sid.clone(),
                provider: provider.to_string(),
                duration_ms,
            });
        }
    }

    // ── Query ──────────────────────────────────────────────────────

    /// Get the last N session summaries.
    pub fn get_recent_sessions(&self, limit: usize) -> Vec<SessionSummary> {
        let history = self.history.lock();
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get the session ID of the currently active session, if any.
    /// Useful for correlating events in the async transcription pipeline
    /// without needing to pass the ID through every function.
    pub fn current_session_id(&self) -> Option<SessionId> {
        let guard = self.current.lock();
        guard.as_ref().map(|s| s.id.clone())
    }

    // ── Internal ───────────────────────────────────────────────────

    /// Convert an ActiveSession into a SessionSummary and add it to the
    /// history ring buffer.
    fn finalise_session(&self, session: ActiveSession, success: bool) {
        let duration_ms = session.started_at.elapsed().as_millis() as u64;

        let phases_json = if session.phases.is_empty() {
            None
        } else {
            serde_json::to_string(&session.phases).ok()
        };

        let summary = SessionSummary {
            id: session.id.clone(),
            started_at_ms: session.started_at_ms,
            duration_ms: Some(duration_ms),
            success,
            model_id: session.model_id,
            text_length: session.text_length,
            had_post_processing: session.post_process,
            phases_json,
            errors: session.errors,
        };

        info!(
            "Session {} completed (success={}, duration={}ms)",
            summary.id, summary.success, duration_ms
        );

        let mut history = self.history.lock();
        if history.len() >= SESSION_HISTORY_CAP {
            history.remove(0);
        }
        history.push(summary);
    }
}

/// Tauri command to get recent session histories.
#[tauri::command]
#[specta::specta]
pub fn get_session_history(
    app: tauri::AppHandle,
    limit: Option<u32>,
) -> Result<Vec<SessionSummary>, String> {
    let tracker = app.state::<Arc<SessionTracker>>();
    let limit = limit.unwrap_or(20) as usize;
    Ok(tracker.get_recent_sessions(limit))
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tracker_has_no_current_session() {
        let tracker = SessionTracker::new();
        assert!(tracker.current_session_id().is_none());
    }

    #[test]
    fn new_tracker_has_empty_history() {
        let tracker = SessionTracker::new();
        let recent = tracker.get_recent_sessions(10);
        assert!(recent.is_empty());
    }

    #[test]
    fn start_session_returns_nonempty_id() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("Built-in Microphone", false);
        assert!(!sid.is_empty());
        assert!(sid.starts_with("s-"));
    }

    #[test]
    fn start_session_sets_current() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        assert_eq!(tracker.current_session_id(), Some(sid));
    }

    #[test]
    fn finish_session_clears_current() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.finish_session(&sid, 10);
        assert!(tracker.current_session_id().is_none());
    }

    #[test]
    fn finish_session_adds_to_history() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.finish_session(&sid, 5);

        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, sid);
        assert!(recent[0].success);
        assert!(recent[0].duration_ms.is_some());
    }

    #[test]
    fn fail_session_adds_to_history_as_failure() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.fail_session(&sid, "transcription failed");

        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent.len(), 1);
        assert!(!recent[0].success);
        assert_eq!(recent[0].errors, vec!["transcription failed"]);
    }

    #[test]
    fn fail_session_clears_current() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.fail_session(&sid, "error");
        assert!(tracker.current_session_id().is_none());
    }

    #[test]
    fn start_session_finalises_leaked_previous_session() {
        let tracker = SessionTracker::new();
        let _sid1 = tracker.start_session("mic-1", false);
        let sid2 = tracker.start_session("mic-2", false);

        // Previous session should appear as failed in history
        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent.len(), 1);
        assert!(!recent[0].success); // leaked session finalised as failed
        assert_eq!(tracker.current_session_id(), Some(sid2));
    }

    #[test]
    fn advance_to_transcribing_sets_model() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.advance_to_transcribing(&sid, "whisper-turbo", 16000, 1000);

        let recent = tracker.get_recent_sessions(10);
        // Session is still active, not in history yet
        assert!(recent.is_empty());
    }

    #[test]
    fn advance_to_post_processing_records_text_length() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.advance_to_transcribing(&sid, "whisper-turbo", 16000, 1000);
        tracker.advance_to_post_processing(&sid, 42, 500);

        // Finish and check
        tracker.finish_session(&sid, 10);
        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].text_length, Some(42));
        assert!(recent[0].success);
    }

    #[test]
    fn session_with_post_process_records_flag() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.set_post_process(&sid, "openai");
        tracker.finish_session(&sid, 10);

        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent.len(), 1);
        assert!(recent[0].had_post_processing);
    }

    #[test]
    fn wrong_session_id_is_ignored() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);

        let wrong_id = "s-wrong-id".to_string();
        tracker.finish_session(&wrong_id, 10);

        // Session should still be active
        assert_eq!(tracker.current_session_id(), Some(sid));

        // No history entries
        assert!(tracker.get_recent_sessions(10).is_empty());
    }

    #[test]
    fn history_respects_limit() {
        let tracker = SessionTracker::new();
        for i in 0..5 {
            let sid = tracker.start_session(&format!("mic-{}", i), false);
            tracker.finish_session(&sid, 1);
        }

        let all = tracker.get_recent_sessions(10);
        assert_eq!(all.len(), 5);

        let limited = tracker.get_recent_sessions(3);
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn history_is_most_recent_first() {
        let tracker = SessionTracker::new();
        let mut ids = Vec::new();
        for _ in 0..3 {
            let sid = tracker.start_session("mic", false);
            ids.push(sid.clone());
            tracker.finish_session(&sid, 1);
        }

        let recent = tracker.get_recent_sessions(10);
        // Most recent first
        assert_eq!(recent[0].id, ids[2]);
        assert_eq!(recent[1].id, ids[1]);
        assert_eq!(recent[2].id, ids[0]);
    }

    #[test]
    fn fail_session_adds_error_to_errors_vec() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.fail_session(&sid, "model load failed");

        let recent = tracker.get_recent_sessions(10);
        assert_eq!(recent[0].errors, vec!["model load failed"]);
    }

    #[test]
    fn session_phase_serializes_to_snake_case() {
        // Verify the serde rename works
        let phase = SessionPhase::Recording;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"recording\"");

        let phase = SessionPhase::Transcribing;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"transcribing\"");

        let phase = SessionPhase::PostProcessing;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"post_processing\"");

        let phase = SessionPhase::Done;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"done\"");

        let phase = SessionPhase::Failed;
        let json = serde_json::to_string(&phase).unwrap();
        assert_eq!(json, "\"failed\"");
    }

    #[test]
    fn session_summary_serializes() {
        let tracker = SessionTracker::new();
        let sid = tracker.start_session("mic-1", false);
        tracker.finish_session(&sid, 10);

        let summary = &tracker.get_recent_sessions(1)[0];
        let json = serde_json::to_string(summary).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"success\":true"));
    }
}