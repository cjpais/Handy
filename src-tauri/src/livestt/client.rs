use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::Message;
use url::{form_urlencoded, Url};

use super::events::{LiveSttEvent, LIVESTT_ERROR_WEBSOCKET_CLOSED};

#[derive(Debug, Clone)]
pub struct LiveSttConfig {
    pub server_url: String,
    pub access_token: String,
    pub consultation_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct LiveSttState {
    pub session_id: Option<i64>,
    pub final_text: String,
    pub pending_partial: Option<String>,
    pub current_text: String,
    ended: bool,
    failed_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveSttResult {
    pub session_id: Option<i64>,
    pub final_text: String,
    pub pending_partial: Option<String>,
    pub current_text: String,
}

const LIVESTT_WS_MESSAGE_QUEUE_CAPACITY: usize = 32;

pub struct LiveSttClient {
    writer_tx: mpsc::Sender<Message>,
    state: Arc<Mutex<LiveSttState>>,
    notify: Arc<Notify>,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
}

impl LiveSttClient {
    pub async fn connect(
        config: LiveSttConfig,
        event_tx: mpsc::Sender<LiveSttEvent>,
    ) -> Result<Self, String> {
        let websocket_url = build_websocket_url(&config)?;
        let (socket, _) = tokio_tungstenite::connect_async(websocket_url.as_str())
            .await
            .map_err(|e| {
                format!(
                    "LiveSTT WebSocket connection failed: {}",
                    redact_access_token(&e.to_string(), &config.access_token)
                )
            })?;

        let (mut writer, mut reader) = socket.split();
        let (writer_tx, mut writer_rx) =
            mpsc::channel::<Message>(LIVESTT_WS_MESSAGE_QUEUE_CAPACITY);
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());

        let writer_task = tokio::spawn(async move {
            while let Some(message) = writer_rx.recv().await {
                let is_close = matches!(message, Message::Close(_));
                if writer.send(message).await.is_err() {
                    break;
                }
                if is_close {
                    break;
                }
            }
        });

        let reader_state = state.clone();
        let reader_notify = notify.clone();
        let reader_event_tx = event_tx.clone();
        let reader_task = tokio::spawn(async move {
            while let Some(message) = reader.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        if let Err(error) = handle_text_frame(
                            &reader_state,
                            &reader_notify,
                            &reader_event_tx,
                            &text,
                        )
                        .await
                        {
                            mark_failed(&reader_state, &reader_notify, &reader_event_tx, error)
                                .await;
                            break;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        handle_close_frame(
                            &reader_state,
                            &reader_notify,
                            &reader_event_tx,
                            frame.as_ref(),
                        )
                        .await;
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        mark_failed(
                            &reader_state,
                            &reader_notify,
                            &reader_event_tx,
                            format!("LiveSTT WebSocket read failed: {}", error),
                        )
                        .await;
                        break;
                    }
                }
            }
        });

        Ok(Self {
            writer_tx,
            state,
            notify,
            reader_task,
            writer_task,
        })
    }

    pub async fn send_audio_chunk(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.send_message(Message::Binary(bytes.into())).await
    }

    pub async fn stop_record(&self) -> Result<(), String> {
        self.send_message(Message::Text(serialize_stop_record_command()?.into()))
            .await
    }

    pub async fn wait_session_ended(&self, timeout: Duration) -> Result<LiveSttResult, String> {
        let wait = async {
            loop {
                if let Some(result) = self.result_if_finished().await? {
                    return Ok(result);
                }

                self.notify.notified().await;
            }
        };

        tokio::time::timeout(timeout, wait)
            .await
            .map_err(|_| "Timed out waiting for LiveSTT session to end".to_string())?
    }

    pub async fn current_result(&self) -> LiveSttResult {
        let state = self.state.lock().await;

        LiveSttResult {
            session_id: state.session_id,
            final_text: state.final_text.clone(),
            pending_partial: state.pending_partial.clone(),
            current_text: state.current_text.clone(),
        }
    }

    pub async fn close_transport(&self) -> Result<(), String> {
        self.send_message(Message::Close(None)).await
    }

    pub fn request_close_transport(&self) {
        let _ = self.writer_tx.try_send(Message::Close(None));
    }

    async fn send_message(&self, message: Message) -> Result<(), String> {
        if let Some(error) = self.failure_error().await {
            return Err(error);
        }

        self.writer_tx
            .send(message)
            .await
            .map_err(|_| "LiveSTT WebSocket writer is closed".to_string())
    }

    pub async fn failure_error(&self) -> Option<String> {
        self.state.lock().await.failed_error.clone()
    }

    async fn result_if_finished(&self) -> Result<Option<LiveSttResult>, String> {
        let state = self.state.lock().await;

        if let Some(error) = &state.failed_error {
            return Err(error.clone());
        }

        if !state.ended {
            return Ok(None);
        }

        Ok(Some(LiveSttResult {
            session_id: state.session_id,
            final_text: state.final_text.clone(),
            pending_partial: state.pending_partial.clone(),
            current_text: state.current_text.clone(),
        }))
    }
}

impl Drop for LiveSttClient {
    fn drop(&mut self) {
        self.request_close_transport();
        self.reader_task.abort();
        self.writer_task.abort();
    }
}

pub fn build_websocket_url(config: &LiveSttConfig) -> Result<Url, String> {
    if config.access_token.trim().is_empty() {
        return Err("LiveSTT access token is required".to_string());
    }

    let mut url = Url::parse(config.server_url.trim())
        .map_err(|e| format!("Invalid LiveSTT server URL: {}", e))?;

    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(format!(
                "Unsupported LiveSTT server URL scheme '{}'; expected http or https",
                other
            ));
        }
    };

    url.set_scheme(scheme)
        .map_err(|_| "Failed to set LiveSTT WebSocket URL scheme".to_string())?;
    url.set_path("api/ws/live-transcription");
    url.set_query(None);

    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("token", &config.access_token);
        pairs.append_pair("audio_format", "pcm");

        if let Some(consultation_id) = config.consultation_id {
            pairs.append_pair("consultation_id", &consultation_id.to_string());
        }
    }

    Ok(url)
}

pub fn serialize_stop_record_command() -> Result<String, String> {
    serde_json::to_string(&serde_json::json!({ "type": "stop_record" }))
        .map_err(|e| format!("Failed to serialize LiveSTT stop command: {}", e))
}

fn redact_access_token(message: &str, access_token: &str) -> String {
    let token = access_token.trim();
    if token.is_empty() {
        return message.to_string();
    }

    let encoded_token: String = form_urlencoded::byte_serialize(token.as_bytes()).collect();

    message
        .replace(token, "[REDACTED]")
        .replace(&encoded_token, "[REDACTED]")
}

pub fn is_livestt_auth_close_code(code: u16, reason: &str) -> bool {
    matches!(code, 4001 | 4401 | 4403)
        || (code == 1008 && contains_explicit_auth_failure_marker(reason))
}

pub fn is_livestt_auth_protocol_error(error_code: &str, error_message: &str) -> bool {
    let code = error_code.to_lowercase();
    matches!(
        code.as_str(),
        "token_expired" | "auth_expired" | "unauthorized" | "forbidden" | "auth_required"
    ) || contains_explicit_auth_failure_marker(error_message)
}

pub fn contains_explicit_auth_failure_marker(value: &str) -> bool {
    let normalized = value.to_lowercase();
    normalized.contains("token expired")
        || normalized.contains("jwt expired")
        || normalized.contains("access token expired")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
}

async fn handle_text_frame(
    state: &Arc<Mutex<LiveSttState>>,
    notify: &Arc<Notify>,
    event_tx: &mpsc::Sender<LiveSttEvent>,
    text: &str,
) -> Result<(), String> {
    let event: LiveSttEvent =
        serde_json::from_str(text).map_err(|e| format!("Failed to parse LiveSTT event: {}", e))?;

    apply_event_and_forward(state, notify, event_tx, event).await;

    Ok(())
}

async fn apply_event_and_forward(
    state: &Arc<Mutex<LiveSttState>>,
    notify: &Arc<Notify>,
    event_tx: &mpsc::Sender<LiveSttEvent>,
    event: LiveSttEvent,
) {
    apply_event(state, notify, event.clone()).await;
    try_forward_event(event_tx, event);
}

async fn apply_event(state: &Arc<Mutex<LiveSttState>>, notify: &Arc<Notify>, event: LiveSttEvent) {
    let should_notify;
    {
        let mut state = state.lock().await;
        should_notify = apply_event_to_state(&mut state, event);
    }

    if should_notify {
        notify.notify_waiters();
    }
}

fn append_transcript_chunk(target: &mut String, chunk: &str) {
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return;
    }

    if target.trim().is_empty() {
        target.clear();
        target.push_str(chunk);
        return;
    }

    let last_char = target.chars().last();
    let first_char = chunk.chars().next();

    let should_insert_space = last_char.map(|c| !c.is_whitespace()).unwrap_or(false)
        && first_char
            .map(|c| {
                !c.is_whitespace()
                    && !matches!(c, '.' | ',' | '!' | '?' | ':' | ';' | ')' | ']' | '}')
            })
            .unwrap_or(false);

    if should_insert_space {
        target.push(' ');
    }

    target.push_str(chunk);
}

fn merge_final_text(final_text: &mut String, next_final: &str) {
    let next_final = next_final.trim();
    if next_final.is_empty() {
        return;
    }

    let existing = final_text.trim();

    // Supports both server contracts:
    // 1. Incremental final chunks: "hello" then "world" => append.
    // 2. Cumulative final text: "hello" then "hello world" => replace with full value.
    if existing.is_empty() || next_final.starts_with(existing) {
        final_text.clear();
        final_text.push_str(next_final);
        return;
    }

    append_transcript_chunk(final_text, next_final);
}

fn compose_current_text(final_text: &str, pending_partial: Option<&str>) -> String {
    let mut current = final_text.trim().to_string();

    if let Some(partial) = pending_partial
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        append_transcript_chunk(&mut current, partial);
    }

    current
}

fn apply_event_to_state(state: &mut LiveSttState, event: LiveSttEvent) -> bool {
    match event {
        LiveSttEvent::SessionStarted { session_id } => {
            state.session_id = Some(session_id);
            state.final_text.clear();
            state.pending_partial = None;
            state.current_text.clear();
            state.ended = false;
            state.failed_error = None;
            true
        }
        LiveSttEvent::Partial { text, .. } => {
            let trimmed = text.trim().to_string();

            state.pending_partial = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
            state.current_text =
                compose_current_text(&state.final_text, state.pending_partial.as_deref());
            false
        }
        LiveSttEvent::Final { text, .. } => {
            merge_final_text(&mut state.final_text, &text);
            state.pending_partial = None;
            state.current_text = state.final_text.clone();
            false
        }
        LiveSttEvent::Error {
            error_code,
            error_message,
            ..
        } => {
            state.failed_error = if is_livestt_auth_protocol_error(&error_code, &error_message) {
                Some(format!("LiveSTT WebSocket auth expired: {}", error_code))
            } else {
                Some(format!(
                    "LiveSTT protocol error {}: {}",
                    error_code, error_message
                ))
            };
            true
        }
        LiveSttEvent::SessionEnded { session_id } => {
            state.session_id = Some(session_id);
            state.ended = true;
            true
        }
    }
}

async fn handle_close_frame(
    state: &Arc<Mutex<LiveSttState>>,
    notify: &Arc<Notify>,
    event_tx: &mpsc::Sender<LiveSttEvent>,
    frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>,
) {
    if state.lock().await.ended {
        return;
    }

    let Some(frame) = frame else {
        mark_failed(
            state,
            notify,
            event_tx,
            "LiveSTT WebSocket closed before session ended".to_string(),
        )
        .await;
        return;
    };

    let code = close_code_u16(frame.code);
    let reason = frame.reason.to_string();

    if is_livestt_auth_close_code(code, &reason) {
        mark_failed(
            state,
            notify,
            event_tx,
            format!("LiveSTT WebSocket auth expired: code={}", code),
        )
        .await;
        return;
    }

    mark_failed(
        state,
        notify,
        event_tx,
        format!(
            "LiveSTT WebSocket closed before session ended: code={}",
            code
        ),
    )
    .await;
}

fn close_code_u16(code: CloseCode) -> u16 {
    code.into()
}

async fn mark_failed(
    state: &Arc<Mutex<LiveSttState>>,
    notify: &Arc<Notify>,
    event_tx: &mpsc::Sender<LiveSttEvent>,
    error: String,
) {
    let error_message = error.clone();

    {
        let mut state = state.lock().await;
        state.failed_error = Some(error);
    }

    notify.notify_waiters();
    try_forward_event(
        event_tx,
        LiveSttEvent::Error {
            session_id: None,
            error_code: LIVESTT_ERROR_WEBSOCKET_CLOSED.to_string(),
            error_message,
        },
    );
}

fn try_forward_event(event_tx: &mpsc::Sender<LiveSttEvent>, event: LiveSttEvent) {
    match event_tx.try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            log::warn!("LiveSTT event queue is full; dropping frontend event");
        }
        Err(TrySendError::Closed(_)) => {
            log::debug!("LiveSTT event receiver dropped; skipping frontend event");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    use tokio_tungstenite::tungstenite::Utf8Bytes;

    fn test_config() -> LiveSttConfig {
        LiveSttConfig {
            server_url: "https://grandedoc-server-98243818959.europe-west6.run.app".to_string(),
            access_token: "token value".to_string(),
            consultation_id: None,
        }
    }

    #[test]
    fn livestt_url_builder_converts_https_to_wss() {
        let url = build_websocket_url(&test_config()).unwrap();

        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.path(), "/api/ws/live-transcription");
    }

    #[test]
    fn livestt_url_builder_includes_audio_format() {
        let url = build_websocket_url(&test_config()).unwrap();

        assert!(url.query().unwrap().contains("audio_format=pcm"));
    }

    #[test]
    fn livestt_url_builder_includes_optional_consultation_id() {
        let mut config = test_config();
        config.consultation_id = Some(42);

        let url = build_websocket_url(&config).unwrap();

        assert!(url.query().unwrap().contains("consultation_id=42"));
    }

    #[test]
    fn livestt_url_builder_percent_encodes_token() {
        let url = build_websocket_url(&test_config()).unwrap();

        assert!(url.query().unwrap().contains("token=token+value"));
    }

    #[test]
    fn livestt_partial_updates_current_text_without_finalizing() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Partial {
                session_id: 1,
                text: "hello wor".to_string(),
                is_final: false,
            },
        );

        assert_eq!(state.current_text, "hello wor");
        assert_eq!(state.final_text, "");
        assert_eq!(state.pending_partial.as_deref(), Some("hello wor"));
    }

    #[test]
    fn livestt_final_replaces_text() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello world".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        assert_eq!(state.final_text, "hello world");
        assert_eq!(state.current_text, "hello world");
        assert_eq!(state.pending_partial, None);
    }

    #[test]
    fn livestt_incremental_final_chunks_are_accumulated() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "first".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "second".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        assert_eq!(state.final_text, "first second");
        assert_eq!(state.current_text, "first second");
    }

    #[test]
    fn livestt_final_chunks_insert_whitespace_when_needed() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "world".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        assert_eq!(state.final_text, "hello world");
    }

    #[test]
    fn livestt_punctuation_final_chunk_does_not_insert_extra_space() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: ".".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        assert_eq!(state.final_text, "hello.");
    }

    #[test]
    fn livestt_cumulative_final_text_replaces_existing_accumulation() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello world".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        assert_eq!(state.final_text, "hello world");
        assert_eq!(state.current_text, "hello world");
    }

    #[test]
    fn livestt_partial_after_final_does_not_change_final_text() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Final {
                session_id: 1,
                text: "stable".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Partial {
                session_id: 1,
                text: "preview".to_string(),
                is_final: false,
            },
        );

        assert_eq!(state.final_text, "stable");
        assert_eq!(state.current_text, "stable preview");
        assert_eq!(state.pending_partial.as_deref(), Some("preview"));
    }

    #[test]
    fn livestt_empty_partial_clears_preview() {
        let mut state = LiveSttState::default();

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Partial {
                session_id: 1,
                text: "hello".to_string(),
                is_final: false,
            },
        );

        apply_event_to_state(
            &mut state,
            LiveSttEvent::Partial {
                session_id: 1,
                text: " ".to_string(),
                is_final: false,
            },
        );

        assert_eq!(state.pending_partial, None);
        assert_eq!(state.current_text, "");
    }

    #[test]
    fn livestt_session_ended_marks_ended_and_requests_notification() {
        let mut state = LiveSttState::default();

        let should_notify =
            apply_event_to_state(&mut state, LiveSttEvent::SessionEnded { session_id: 7 });

        assert!(should_notify);
        assert_eq!(state.session_id, Some(7));
        assert!(state.ended);
    }

    #[tokio::test]
    async fn livestt_session_ended_returns_accumulated_final_text() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (writer_tx, _writer_rx) = mpsc::channel(1);
        let client = LiveSttClient {
            writer_tx,
            state: state.clone(),
            notify: notify.clone(),
            reader_task: tokio::spawn(async {}),
            writer_task: tokio::spawn(async {}),
        };

        apply_event(
            &state,
            &notify,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        )
        .await;
        apply_event(
            &state,
            &notify,
            LiveSttEvent::Final {
                session_id: 1,
                text: "world".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        )
        .await;
        apply_event(
            &state,
            &notify,
            LiveSttEvent::SessionEnded { session_id: 1 },
        )
        .await;

        let result = client
            .wait_session_ended(Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(result.final_text, "hello world");
    }

    #[test]
    fn livestt_stop_record_command_serializes_to_json() {
        let command = serialize_stop_record_command().unwrap();

        assert_eq!(command, r#"{"type":"stop_record"}"#);
    }

    #[test]
    fn livestt_connection_error_redacts_raw_token() {
        let message = "failed for token=secret-token";

        let redacted = redact_access_token(message, "secret-token");

        assert_eq!(redacted, "failed for token=[REDACTED]");
    }

    #[test]
    fn livestt_connection_error_redacts_encoded_token() {
        let message = "failed for token=token+value";

        let redacted = redact_access_token(message, "token value");

        assert_eq!(redacted, "failed for token=[REDACTED]");
    }

    #[test]
    fn livestt_auth_close_code_detects_auth_expiration() {
        assert!(is_livestt_auth_close_code(4001, ""));
        assert!(is_livestt_auth_close_code(4401, ""));
        assert!(is_livestt_auth_close_code(4403, ""));
        assert!(is_livestt_auth_close_code(1008, "token expired"));
        assert!(is_livestt_auth_close_code(1008, "jwt expired"));
        assert!(is_livestt_auth_close_code(1008, "access token expired"));
        assert!(is_livestt_auth_close_code(1008, "unauthorized"));
        assert!(is_livestt_auth_close_code(1008, "forbidden"));
    }

    #[test]
    fn livestt_auth_close_code_ignores_normal_close() {
        assert!(!is_livestt_auth_close_code(1000, ""));
        assert!(!is_livestt_auth_close_code(1008, "message too large"));
        assert!(!is_livestt_auth_close_code(
            1008,
            "authentication backend overloaded"
        ));
    }

    #[test]
    fn livestt_auth_protocol_error_detects_common_codes() {
        assert!(is_livestt_auth_protocol_error("TOKEN_EXPIRED", ""));
        assert!(is_livestt_auth_protocol_error("AUTH_EXPIRED", ""));
        assert!(is_livestt_auth_protocol_error("UNAUTHORIZED", ""));
        assert!(is_livestt_auth_protocol_error("FORBIDDEN", ""));
        assert!(is_livestt_auth_protocol_error("AUTH_REQUIRED", ""));
        assert!(is_livestt_auth_protocol_error(
            "SERVER_ERROR",
            "token expired"
        ));
    }

    #[test]
    fn livestt_auth_protocol_error_ignores_unrelated_errors() {
        assert!(!is_livestt_auth_protocol_error(
            "SERVER_ERROR",
            "backend overloaded"
        ));
        assert!(!is_livestt_auth_protocol_error(
            "SERVER_ERROR",
            "authentication backend overloaded"
        ));
    }

    fn close_frame(code: u16, reason: &'static str) -> CloseFrame {
        CloseFrame {
            code: CloseCode::from(code),
            reason: Utf8Bytes::from_static(reason),
        }
    }

    #[tokio::test]
    async fn livestt_close_1000_before_session_ended_marks_failed() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, _event_rx) = mpsc::channel(1);
        let frame = close_frame(1000, "");

        handle_close_frame(&state, &notify, &event_tx, Some(&frame)).await;

        assert_eq!(
            state.lock().await.failed_error.as_deref(),
            Some("LiveSTT WebSocket closed before session ended: code=1000")
        );
    }

    #[tokio::test]
    async fn livestt_close_without_frame_before_session_ended_marks_failed() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, _event_rx) = mpsc::channel(1);

        handle_close_frame(&state, &notify, &event_tx, None).await;

        assert_eq!(
            state.lock().await.failed_error.as_deref(),
            Some("LiveSTT WebSocket closed before session ended")
        );
    }

    #[tokio::test]
    async fn livestt_close_1000_after_session_ended_does_not_fail() {
        let state = Arc::new(Mutex::new(LiveSttState {
            ended: true,
            ..LiveSttState::default()
        }));
        let notify = Arc::new(Notify::new());
        let (event_tx, _event_rx) = mpsc::channel(1);
        let frame = close_frame(1000, "");

        handle_close_frame(&state, &notify, &event_tx, Some(&frame)).await;

        assert_eq!(state.lock().await.failed_error, None);
    }

    #[tokio::test]
    async fn livestt_close_4001_before_ended_marks_auth_expired() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, _event_rx) = mpsc::channel(1);
        let frame = close_frame(4001, "");

        handle_close_frame(&state, &notify, &event_tx, Some(&frame)).await;

        assert_eq!(
            state.lock().await.failed_error.as_deref(),
            Some("LiveSTT WebSocket auth expired: code=4001")
        );
    }

    #[tokio::test]
    async fn livestt_close_1008_auth_backend_overloaded_is_not_auth_expired() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, _event_rx) = mpsc::channel(1);
        let frame = close_frame(1008, "authentication backend overloaded");

        handle_close_frame(&state, &notify, &event_tx, Some(&frame)).await;

        assert_eq!(
            state.lock().await.failed_error.as_deref(),
            Some("LiveSTT WebSocket closed before session ended: code=1008")
        );
    }

    #[tokio::test]
    async fn livestt_apply_event_forwards_protocol_event() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, mut event_rx) = mpsc::channel(1);
        let event = LiveSttEvent::Partial {
            session_id: 1,
            text: "hello wor".to_string(),
            is_final: false,
        };

        apply_event_and_forward(&state, &notify, &event_tx, event.clone()).await;

        assert_eq!(event_rx.recv().await, Some(event));

        let state = state.lock().await;
        assert_eq!(state.current_text, "hello wor");
        assert_eq!(state.pending_partial.as_deref(), Some("hello wor"));
    }

    #[tokio::test]
    async fn livestt_apply_event_updates_state_when_event_receiver_dropped() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, event_rx) = mpsc::channel(1);
        drop(event_rx);

        apply_event_and_forward(
            &state,
            &notify,
            &event_tx,
            LiveSttEvent::Final {
                session_id: 1,
                text: "hello world".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.final_text, "hello world");
        assert_eq!(state.current_text, "hello world");
        assert_eq!(state.pending_partial, None);
    }

    #[tokio::test]
    async fn livestt_apply_event_updates_state_when_event_queue_is_full() {
        let state = Arc::new(Mutex::new(LiveSttState::default()));
        let notify = Arc::new(Notify::new());
        let (event_tx, mut event_rx) = mpsc::channel(1);

        event_tx
            .try_send(LiveSttEvent::Partial {
                session_id: 1,
                text: "queued".to_string(),
                is_final: false,
            })
            .unwrap();

        apply_event_and_forward(
            &state,
            &notify,
            &event_tx,
            LiveSttEvent::Final {
                session_id: 1,
                text: "latest".to_string(),
                is_final: true,
                start_time: None,
                end_time: None,
            },
        )
        .await;

        let state = state.lock().await;
        assert_eq!(state.final_text, "latest");
        assert_eq!(state.current_text, "latest");
        drop(state);

        assert_eq!(
            event_rx.recv().await,
            Some(LiveSttEvent::Partial {
                session_id: 1,
                text: "queued".to_string(),
                is_final: false,
            })
        );
        assert!(event_rx.try_recv().is_err());
    }
}
