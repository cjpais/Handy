use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};

pub const EVENT_SESSION_STARTED: &str = "livestt-session-started";
pub const EVENT_PARTIAL: &str = "livestt-partial";
pub const EVENT_FINAL: &str = "livestt-final";
pub const EVENT_ERROR: &str = "livestt-error";
pub const EVENT_SESSION_ENDED: &str = "livestt-session-ended";

pub const LIVESTT_ERROR_AUTH_REQUIRED: &str = "AUTH_REQUIRED";
pub const LIVESTT_ERROR_INVALID_SERVER_URL: &str = "INVALID_SERVER_URL";
pub const LIVESTT_ERROR_INVALID_CONSULTATION_ID: &str = "INVALID_CONSULTATION_ID";
pub const LIVESTT_ERROR_CONNECT_TIMEOUT: &str = "CONNECT_TIMEOUT";
pub const LIVESTT_ERROR_CONNECT_FAILED: &str = "CONNECT_FAILED";
pub const LIVESTT_ERROR_AUDIO_QUEUE_OVERFLOW: &str = "AUDIO_QUEUE_OVERFLOW";
pub const LIVESTT_ERROR_AUDIO_WRITER_FAILED: &str = "AUDIO_WRITER_FAILED";
pub const LIVESTT_ERROR_FINALIZE_TIMEOUT: &str = "FINALIZE_TIMEOUT";
pub const LIVESTT_ERROR_SERVER_ERROR: &str = "SERVER_ERROR";
pub const LIVESTT_ERROR_WEBSOCKET_CLOSED: &str = "WEBSOCKET_CLOSED";
pub const LIVESTT_ERROR_CANCELED: &str = "CANCELED";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveSttEvent {
    SessionStarted {
        session_id: i64,
    },
    Partial {
        session_id: i64,
        text: String,
        is_final: bool,
    },
    Final {
        session_id: i64,
        text: String,
        is_final: bool,
        #[serde(default)]
        start_time: Option<f64>,
        #[serde(default)]
        end_time: Option<f64>,
    },
    Error {
        #[serde(default)]
        session_id: Option<i64>,
        error_code: String,
        error_message: String,
    },
    SessionEnded {
        session_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveSttSessionPayload {
    pub session_id: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveSttTranscriptPayload {
    pub session_id: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveSttErrorPayload {
    pub session_id: Option<i64>,
    pub error_code: String,
    pub error_message: String,
}

pub fn emit_livestt_error(
    app_handle: &AppHandle,
    session_id: Option<i64>,
    error_code: &str,
    error_message: &str,
) {
    if let Err(error) = app_handle.emit(
        EVENT_ERROR,
        LiveSttErrorPayload {
            session_id,
            error_code: error_code.to_string(),
            error_message: error_message.to_string(),
        },
    ) {
        log::warn!("Failed to emit LiveSTT error frontend event: {}", error);
    }
}

pub fn emit_livestt_event(app_handle: &AppHandle, event: &LiveSttEvent) {
    let (name, payload) = frontend_event_payload(event);

    if let Err(error) = app_handle.emit(name, payload) {
        log::warn!("Failed to emit LiveSTT frontend event: {}", error);
    }
}

fn frontend_event_payload(event: &LiveSttEvent) -> (&'static str, serde_json::Value) {
    match event {
        LiveSttEvent::SessionStarted { session_id } => (
            EVENT_SESSION_STARTED,
            json!(LiveSttSessionPayload {
                session_id: *session_id,
            }),
        ),
        LiveSttEvent::Partial {
            session_id, text, ..
        } => (
            EVENT_PARTIAL,
            json!(LiveSttTranscriptPayload {
                session_id: *session_id,
                text: text.clone(),
            }),
        ),
        LiveSttEvent::Final {
            session_id, text, ..
        } => (
            EVENT_FINAL,
            json!(LiveSttTranscriptPayload {
                session_id: *session_id,
                text: text.clone(),
            }),
        ),
        LiveSttEvent::Error {
            session_id,
            error_message,
            ..
        } => (
            EVENT_ERROR,
            json!(LiveSttErrorPayload {
                session_id: *session_id,
                error_code: LIVESTT_ERROR_SERVER_ERROR.to_string(),
                error_message: error_message.clone(),
            }),
        ),
        LiveSttEvent::SessionEnded { session_id } => (
            EVENT_SESSION_ENDED,
            json!(LiveSttSessionPayload {
                session_id: *session_id,
            }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn livestt_deserializes_session_started() {
        let event: LiveSttEvent =
            serde_json::from_str(r#"{"type":"session_started","session_id":123}"#).unwrap();

        assert_eq!(event, LiveSttEvent::SessionStarted { session_id: 123 });
    }

    #[test]
    fn livestt_deserializes_partial() {
        let event: LiveSttEvent = serde_json::from_str(
            r#"{"type":"partial","session_id":123,"text":"hello","is_final":false}"#,
        )
        .unwrap();

        assert_eq!(
            event,
            LiveSttEvent::Partial {
                session_id: 123,
                text: "hello".to_string(),
                is_final: false,
            }
        );
    }

    #[test]
    fn livestt_deserializes_final() {
        let event: LiveSttEvent = serde_json::from_str(
            r#"{"type":"final","session_id":123,"text":"hello","is_final":true,"start_time":0.0,"end_time":2.5}"#,
        )
        .unwrap();

        assert_eq!(
            event,
            LiveSttEvent::Final {
                session_id: 123,
                text: "hello".to_string(),
                is_final: true,
                start_time: Some(0.0),
                end_time: Some(2.5),
            }
        );
    }

    #[test]
    fn livestt_deserializes_error() {
        let event: LiveSttEvent = serde_json::from_str(
            r#"{"type":"error","error_code":"CONNECTION_CLOSED","error_message":"closed"}"#,
        )
        .unwrap();

        assert_eq!(
            event,
            LiveSttEvent::Error {
                session_id: None,
                error_code: "CONNECTION_CLOSED".to_string(),
                error_message: "closed".to_string(),
            }
        );
    }

    #[test]
    fn livestt_deserializes_session_ended() {
        let event: LiveSttEvent =
            serde_json::from_str(r#"{"type":"session_ended","session_id":123}"#).unwrap();

        assert_eq!(event, LiveSttEvent::SessionEnded { session_id: 123 });
    }

    #[test]
    fn livestt_transcript_payload_contains_only_session_and_text() {
        let payload = LiveSttTranscriptPayload {
            session_id: 123,
            text: "hello".to_string(),
        };

        let value = serde_json::to_value(payload).unwrap();

        assert_eq!(value["session_id"], 123);
        assert_eq!(value["text"], "hello");
        assert_eq!(value.as_object().unwrap().len(), 2);
    }

    #[test]
    fn livestt_frontend_event_mapping_preserves_names_and_payload_shapes() {
        let cases = [
            (
                LiveSttEvent::SessionStarted { session_id: 123 },
                EVENT_SESSION_STARTED,
                json!({ "session_id": 123 }),
            ),
            (
                LiveSttEvent::Partial {
                    session_id: 123,
                    text: "hello".to_string(),
                    is_final: false,
                },
                EVENT_PARTIAL,
                json!({ "session_id": 123, "text": "hello" }),
            ),
            (
                LiveSttEvent::Final {
                    session_id: 123,
                    text: "world".to_string(),
                    is_final: true,
                    start_time: None,
                    end_time: None,
                },
                EVENT_FINAL,
                json!({ "session_id": 123, "text": "world" }),
            ),
            (
                LiveSttEvent::Error {
                    session_id: Some(123),
                    error_code: "IGNORED".to_string(),
                    error_message: "server said no".to_string(),
                },
                EVENT_ERROR,
                json!({
                    "session_id": 123,
                    "error_code": LIVESTT_ERROR_SERVER_ERROR,
                    "error_message": "server said no"
                }),
            ),
            (
                LiveSttEvent::SessionEnded { session_id: 123 },
                EVENT_SESSION_ENDED,
                json!({ "session_id": 123 }),
            ),
        ];

        for (event, expected_name, expected_payload) in cases {
            let (name, payload) = frontend_event_payload(&event);
            assert_eq!(name, expected_name);
            assert_eq!(payload, expected_payload);
        }
    }
}
