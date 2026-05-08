use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::settings;

use super::audio::f32_samples_to_pcm_i16_le;
use super::auth::{ensure_fresh_livestt_access_token, force_refresh_livestt_access_token};
use super::client::{
    contains_explicit_auth_failure_marker, LiveSttClient, LiveSttConfig, LiveSttResult,
};
use super::events::{
    emit_livestt_event, LiveSttEvent, LIVESTT_ERROR_AUDIO_QUEUE_OVERFLOW,
    LIVESTT_ERROR_AUDIO_WRITER_FAILED, LIVESTT_ERROR_AUTH_REQUIRED, LIVESTT_ERROR_CANCELED,
    LIVESTT_ERROR_CONNECT_FAILED, LIVESTT_ERROR_CONNECT_TIMEOUT, LIVESTT_ERROR_FINALIZE_TIMEOUT,
    LIVESTT_ERROR_INVALID_CONSULTATION_ID, LIVESTT_ERROR_INVALID_SERVER_URL,
    LIVESTT_ERROR_WEBSOCKET_CLOSED,
};
use super::replay::LiveSttReplayBuffer;

pub type LiveSttAudioSender = LiveSttAudioSink;

pub const LIVESTT_AUDIO_QUEUE_CAPACITY: usize = 16;
const LIVESTT_EVENT_QUEUE_CAPACITY: usize = 32;
pub const LIVESTT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const LIVESTT_AUDIO_OVERFLOW_ERROR: &str = "LiveSTT audio stream fell behind; please try again";
const WRITER_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const LIVESTT_MAX_RECONNECTS_PER_SESSION: usize = 2;

#[derive(Clone)]
pub struct LiveSttAudioSink {
    sender: mpsc::Sender<Vec<f32>>,
    overflowed: Arc<AtomicBool>,
}

impl LiveSttAudioSink {
    fn new(sender: mpsc::Sender<Vec<f32>>, overflowed: Arc<AtomicBool>) -> Self {
        Self { sender, overflowed }
    }

    pub fn try_send_chunk(&self, chunk: Vec<f32>) {
        match self.sender.try_send(chunk) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                let first = !self.overflowed.swap(true, Ordering::Relaxed);
                if first {
                    log::warn!("LiveSTT audio queue overflowed; session will be discarded");
                } else {
                    log::debug!("LiveSTT audio queue still overflowed; dropping chunk");
                }
            }
            Err(TrySendError::Closed(_)) => {
                log::debug!("LiveSTT audio queue closed; dropping chunk");
            }
        }
    }

    #[cfg(test)]
    fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Relaxed)
    }
}

pub struct LiveSttSessionManager {
    slot: Mutex<SessionSlot>,
    finalizing: Mutex<Option<FinalizingLiveSttSession>>,
    start_generation: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
pub struct LiveSttStartReservation {
    generation: u64,
}

enum SessionSlot {
    Idle,
    Starting(u64),
    Active(ActiveLiveSttSession),
}

struct FinalizingLiveSttSession {
    context: Arc<LiveSttSessionContext>,
    cancel_tx: watch::Sender<bool>,
}

impl Default for LiveSttSessionManager {
    fn default() -> Self {
        Self {
            slot: Mutex::new(SessionSlot::Idle),
            finalizing: Mutex::new(None),
            start_generation: AtomicU64::new(0),
        }
    }
}

impl Default for SessionSlot {
    fn default() -> Self {
        SessionSlot::Idle
    }
}

struct ActiveLiveSttSession {
    binding_id: String,
    context: Arc<LiveSttSessionContext>,
    audio_tx: Option<LiveSttAudioSender>,
    audio_overflowed: Arc<AtomicBool>,
    writer_task: JoinHandle<Result<(), String>>,
    finalize_timeout: Duration,
}

struct LiveSttSessionContext {
    app_handle: AppHandle,
    server_url: String,
    consultation_id: Option<i64>,
    prompt: Option<String>,
    event_tx: mpsc::Sender<LiveSttEvent>,
    client: Mutex<Arc<LiveSttClient>>,
    replay_buffer: Mutex<LiveSttReplayBuffer>,
    reconnect_attempts: AtomicU64,
}

impl LiveSttSessionContext {
    fn new(
        app_handle: AppHandle,
        server_url: String,
        consultation_id: Option<i64>,
        prompt: Option<String>,
        event_tx: mpsc::Sender<LiveSttEvent>,
        client: Arc<LiveSttClient>,
    ) -> Self {
        Self {
            app_handle,
            server_url,
            consultation_id,
            prompt,
            event_tx,
            client: Mutex::new(client),
            replay_buffer: Mutex::new(LiveSttReplayBuffer::default()),
            reconnect_attempts: AtomicU64::new(0),
        }
    }

    fn current_client(&self) -> Arc<LiveSttClient> {
        self.client
            .lock()
            .expect("LiveSTT session context poisoned")
            .clone()
    }

    fn replace_client(&self, client: Arc<LiveSttClient>) {
        let old_client = std::mem::replace(
            &mut *self
                .client
                .lock()
                .expect("LiveSTT session context poisoned"),
            client,
        );
        old_client.request_close_transport();
    }

    async fn close_current_transport(&self) {
        let client = self.current_client();
        if client.close_transport().await.is_err() {
            client.request_close_transport();
        }
    }

    fn request_close_transport(&self) {
        self.current_client().request_close_transport();
    }

    fn append_replay_chunk(&self, bytes: Vec<u8>) {
        self.replay_buffer
            .lock()
            .expect("LiveSTT replay buffer poisoned")
            .append_pcm_chunk(bytes);
    }

    fn replay_snapshot(&self) -> Vec<Vec<u8>> {
        self.replay_buffer
            .lock()
            .expect("LiveSTT replay buffer poisoned")
            .snapshot_chunks()
    }

    fn reserve_reconnect(&self) -> Result<(), String> {
        let previous = self.reconnect_attempts.fetch_add(1, Ordering::Relaxed);
        if previous < LIVESTT_MAX_RECONNECTS_PER_SESSION as u64 {
            Ok(())
        } else {
            Err("LiveSTT auth reconnect retry limit reached".to_string())
        }
    }
}

impl LiveSttSessionManager {
    pub fn reserve_start(&self) -> Result<LiveSttStartReservation, String> {
        self.reserve_starting()
            .map(|generation| LiveSttStartReservation { generation })
    }

    pub async fn start_reserved_session(
        &self,
        app_handle: AppHandle,
        binding_id: String,
        reservation: LiveSttStartReservation,
    ) -> Result<LiveSttAudioSender, String> {
        let generation = reservation.generation;
        let result = self
            .start_session_inner(app_handle, binding_id, generation)
            .await;
        if result.is_err() {
            self.clear_starting(generation);
        }

        result
    }

    async fn start_session_inner(
        &self,
        app_handle: AppHandle,
        binding_id: String,
        generation: u64,
    ) -> Result<LiveSttAudioSender, String> {
        let app_settings = settings::get_settings(&app_handle);
        let finalize_timeout = Duration::from_millis(app_settings.livestt_finalize_timeout_ms);
        let server_url =
            settings::validate_livestt_server_url_required(&app_settings.livestt_server_url)?;
        let consultation_id =
            parse_consultation_id(app_settings.livestt_consultation_id.as_deref())?;
        let prompt = settings::normalize_livestt_prompt(app_settings.livestt_prompt.as_deref())?;

        let (event_tx, event_rx) = mpsc::channel::<LiveSttEvent>(LIVESTT_EVENT_QUEUE_CAPACITY);

        let client = connect_initial_livestt_client(
            &app_handle,
            server_url.clone(),
            consultation_id,
            prompt.clone(),
            event_tx.clone(),
        )
        .await?;
        spawn_livestt_event_bridge(app_handle.clone(), event_rx);
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>(LIVESTT_AUDIO_QUEUE_CAPACITY);
        let audio_overflowed = Arc::new(AtomicBool::new(false));
        let context = Arc::new(LiveSttSessionContext::new(
            app_handle,
            server_url,
            consultation_id,
            prompt,
            event_tx,
            client,
        ));
        let writer_task = spawn_audio_writer(context.clone(), audio_rx);
        let sender = LiveSttAudioSink::new(audio_tx, audio_overflowed.clone());

        let active = ActiveLiveSttSession {
            binding_id,
            context,
            audio_tx: Some(sender.clone()),
            audio_overflowed,
            writer_task,
            finalize_timeout,
        };

        self.activate(active, generation)?;
        Ok(sender)
    }

    pub async fn stop_session(&self, binding_id: &str) -> Result<LiveSttResult, String> {
        let active = self.take_active_for_stop(binding_id)?;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        self.set_finalizing(active.context.clone(), cancel_tx)?;

        let result = stop_active_session(active, cancel_rx).await;
        self.clear_finalizing();

        result
    }

    pub fn cancel_session(&self) -> Result<(), String> {
        log::info!("LiveSTT session cancellation requested");

        if let Some(finalizing) = self.take_finalizing() {
            let _ = finalizing.cancel_tx.send(true);
            finalizing.context.request_close_transport();
            log::info!("LiveSTT WebSocket transport closed due to cancel");
            log::info!("Active LiveSTT session canceled");
            return Ok(());
        }

        let active = {
            let mut slot = self.slot.lock().expect("LiveSTT session manager poisoned");
            match std::mem::replace(&mut *slot, SessionSlot::Idle) {
                SessionSlot::Idle => return Ok(()),
                SessionSlot::Starting(_) => {
                    log::info!("LiveSTT session start canceled while connecting");
                    return Ok(());
                }
                SessionSlot::Active(active) => active,
            }
        };

        active.context.request_close_transport();
        log::info!("LiveSTT WebSocket transport closed due to cancel");
        drop(active.audio_tx);
        active.writer_task.abort();
        log::info!("Active LiveSTT session canceled");

        Ok(())
    }

    pub fn is_active(&self) -> bool {
        let slot_active = !matches!(
            &*self.slot.lock().expect("LiveSTT session manager poisoned"),
            SessionSlot::Idle
        );

        let finalizing_active = self
            .finalizing
            .lock()
            .expect("LiveSTT session manager poisoned")
            .is_some();

        slot_active || finalizing_active
    }

    pub fn is_starting(&self) -> bool {
        matches!(
            &*self.slot.lock().expect("LiveSTT session manager poisoned"),
            SessionSlot::Starting(_)
        )
    }

    fn reserve_starting(&self) -> Result<u64, String> {
        let mut slot = self.slot.lock().expect("LiveSTT session manager poisoned");

        if matches!(&*slot, SessionSlot::Idle) {
            let generation = self.start_generation.fetch_add(1, Ordering::Relaxed) + 1;
            *slot = SessionSlot::Starting(generation);
            Ok(generation)
        } else {
            Err("A LiveSTT session is already active".to_string())
        }
    }

    fn clear_starting(&self, generation: u64) {
        let mut slot = self.slot.lock().expect("LiveSTT session manager poisoned");

        if matches!(&*slot, SessionSlot::Starting(current) if *current == generation) {
            *slot = SessionSlot::Idle;
        }
    }

    fn activate(&self, active: ActiveLiveSttSession, generation: u64) -> Result<(), String> {
        let mut slot = self.slot.lock().expect("LiveSTT session manager poisoned");

        if matches!(&*slot, SessionSlot::Starting(current) if *current == generation) {
            *slot = SessionSlot::Active(active);
            Ok(())
        } else if matches!(&*slot, SessionSlot::Idle) {
            active.context.request_close_transport();
            active.writer_task.abort();
            log::info!("LiveSTT WebSocket transport closed after canceled start");
            Err("LiveSTT session start was canceled".to_string())
        } else {
            active.context.request_close_transport();
            active.writer_task.abort();
            Err("A LiveSTT session is already active".to_string())
        }
    }

    fn take_active_for_stop(&self, binding_id: &str) -> Result<ActiveLiveSttSession, String> {
        let mut slot = self.slot.lock().expect("LiveSTT session manager poisoned");

        match &*slot {
            SessionSlot::Idle => return Err("No active LiveSTT session".to_string()),
            SessionSlot::Starting(_) => return Err("LiveSTT session is still starting".to_string()),
            SessionSlot::Active(active) if active.binding_id != binding_id => {
                return Err("LiveSTT session binding does not match active recording".to_string());
            }
            SessionSlot::Active(_) => {}
        }

        match std::mem::replace(&mut *slot, SessionSlot::Idle) {
            SessionSlot::Active(active) => Ok(active),
            _ => unreachable!("slot was checked above"),
        }
    }

    fn set_finalizing(
        &self,
        context: Arc<LiveSttSessionContext>,
        cancel_tx: watch::Sender<bool>,
    ) -> Result<(), String> {
        let mut finalizing = self
            .finalizing
            .lock()
            .expect("LiveSTT session manager poisoned");

        if finalizing.is_some() {
            return Err("A LiveSTT session is already finalizing".to_string());
        }

        *finalizing = Some(FinalizingLiveSttSession { context, cancel_tx });
        Ok(())
    }

    fn take_finalizing(&self) -> Option<FinalizingLiveSttSession> {
        self.finalizing
            .lock()
            .expect("LiveSTT session manager poisoned")
            .take()
    }

    fn clear_finalizing(&self) {
        self.finalizing
            .lock()
            .expect("LiveSTT session manager poisoned")
            .take();
    }
}

fn spawn_livestt_event_bridge(app_handle: AppHandle, mut event_rx: mpsc::Receiver<LiveSttEvent>) {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            emit_livestt_event(&app_handle, &event);
        }
    });
}

async fn connect_initial_livestt_client(
    app_handle: &AppHandle,
    server_url: String,
    consultation_id: Option<i64>,
    prompt: Option<String>,
    event_tx: mpsc::Sender<LiveSttEvent>,
) -> Result<Arc<LiveSttClient>, String> {
    let token = ensure_fresh_livestt_access_token(app_handle).await?;
    match connect_livestt_client(
        server_url.clone(),
        consultation_id,
        prompt.clone(),
        token,
        event_tx.clone(),
    )
    .await
    {
        Ok(client) => Ok(client),
        Err(error)
            if is_livestt_auth_connect_error(&error) || is_livestt_auth_runtime_error(&error) =>
        {
            log::warn!("LiveSTT WebSocket auth failed; attempting token refresh once");
            let token = force_refresh_livestt_access_token(app_handle).await?;
            connect_livestt_client(server_url, consultation_id, prompt, token, event_tx).await
        }
        Err(error) => Err(error),
    }
}

async fn connect_livestt_client(
    server_url: String,
    consultation_id: Option<i64>,
    prompt: Option<String>,
    access_token: String,
    event_tx: mpsc::Sender<LiveSttEvent>,
) -> Result<Arc<LiveSttClient>, String> {
    let config = LiveSttConfig {
        server_url,
        access_token,
        consultation_id,
        prompt,
    };

    tokio::time::timeout(
        LIVESTT_CONNECT_TIMEOUT,
        LiveSttClient::connect(config, event_tx),
    )
    .await
    .map_err(|_| "Timed out connecting to LiveSTT server".to_string())?
    .map(Arc::new)
}

fn spawn_audio_writer(
    context: Arc<LiveSttSessionContext>,
    mut audio_rx: mpsc::Receiver<Vec<f32>>,
) -> JoinHandle<Result<(), String>> {
    tokio::spawn(async move {
        while let Some(samples) = audio_rx.recv().await {
            let bytes = f32_samples_to_pcm_i16_le(&samples);
            context.append_replay_chunk(bytes.clone());

            let client = context.current_client();
            match client.send_audio_chunk(bytes).await {
                Ok(()) => {}
                Err(error) if is_livestt_auth_runtime_error(&error) => {
                    log::warn!(
                        "LiveSTT auth expired while recording; refreshing token and reconnecting"
                    );
                    let client = reconnect_with_refresh_and_replay(&context).await?;
                    if let Some(error) = client.failure_error().await {
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
        }

        Ok(())
    })
}

async fn stop_active_session(
    mut active: ActiveLiveSttSession,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<LiveSttResult, String> {
    active.audio_tx.take();

    tokio::select! {
        result = wait_for_writer_drain(&mut active.writer_task) => {
            if let Err(error) = result {
                active.context.close_current_transport().await;
                return Err(error);
            }
        }
        _ = cancel_rx.changed() => {
            active.writer_task.abort();
            active.context.close_current_transport().await;
            return Err("LiveSTT session canceled".to_string());
        }
    }

    if *cancel_rx.borrow() {
        active.context.close_current_transport().await;
        return Err("LiveSTT session canceled".to_string());
    }

    if let Err(error) = ensure_no_audio_overflow(active.audio_overflowed.load(Ordering::Relaxed)) {
        active.context.close_current_transport().await;
        return Err(error);
    }

    let mut client = active.context.current_client();
    if let Err(error) = client.stop_record().await {
        if is_livestt_auth_runtime_error(&error) {
            log::warn!(
                "LiveSTT auth expired during finalization; refreshing token and reconnecting"
            );
            client = reconnect_with_refresh_and_replay_or_cancel(&active.context, &mut cancel_rx)
                .await?;
            client.stop_record().await?;
        } else {
            let _ = client.close_transport().await;
            return Err(error);
        }
    }

    let wait_result = tokio::select! {
        result = client.wait_session_ended(active.finalize_timeout) => result,
        _ = cancel_rx.changed() => {
            active.context.close_current_transport().await;
            return Err("LiveSTT session canceled".to_string());
        }
    };

    let result = match wait_result {
        Ok(result) => result,
        Err(error) if error.contains("Timed out waiting for LiveSTT session to end") => {
            let current = client.current_result().await;
            resolve_timeout_result(&current)?
        }
        Err(error) if is_livestt_auth_runtime_error(&error) => {
            log::warn!("LiveSTT auth expired while waiting for final result; refreshing token and reconnecting");
            client = reconnect_with_refresh_and_replay_or_cancel(&active.context, &mut cancel_rx)
                .await?;
            client.stop_record().await?;
            tokio::select! {
                result = client.wait_session_ended(active.finalize_timeout) => result,
                _ = cancel_rx.changed() => {
                    active.context.close_current_transport().await;
                    return Err("LiveSTT session canceled".to_string());
                }
            }?
        }
        Err(error) => {
            let _ = client.close_transport().await;
            return Err(error);
        }
    };

    if let Err(error) = ensure_no_audio_overflow(active.audio_overflowed.load(Ordering::Relaxed)) {
        active.context.close_current_transport().await;
        return Err(error);
    }

    active.context.close_current_transport().await;

    Ok(result)
}

async fn reconnect_with_refresh_and_replay_or_cancel(
    context: &Arc<LiveSttSessionContext>,
    cancel_rx: &mut watch::Receiver<bool>,
) -> Result<Arc<LiveSttClient>, String> {
    tokio::select! {
        result = reconnect_with_refresh_and_replay(context) => result,
        _ = cancel_rx.changed() => {
            context.close_current_transport().await;
            Err("LiveSTT session canceled".to_string())
        }
    }
}

async fn reconnect_with_refresh_and_replay(
    context: &Arc<LiveSttSessionContext>,
) -> Result<Arc<LiveSttClient>, String> {
    context.reserve_reconnect()?;

    let access_token = force_refresh_livestt_access_token(&context.app_handle).await?;
    let client = connect_livestt_client(
        context.server_url.clone(),
        context.consultation_id,
        context.prompt.clone(),
        access_token,
        context.event_tx.clone(),
    )
    .await?;

    let chunks = context.replay_snapshot();
    context.replace_client(client.clone());

    for chunk in chunks {
        if let Err(error) = client.send_audio_chunk(chunk).await {
            context.close_current_transport().await;
            return Err(error);
        }
    }

    Ok(client)
}

async fn wait_for_writer_drain(
    writer_task: &mut JoinHandle<Result<(), String>>,
) -> Result<(), String> {
    match tokio::time::timeout(WRITER_DRAIN_TIMEOUT, &mut *writer_task).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(error)) => Err(format!("LiveSTT audio writer task failed: {}", error)),
        Err(_) => {
            writer_task.abort();
            Err("Timed out waiting for LiveSTT audio writer to drain".to_string())
        }
    }
}

fn parse_consultation_id(value: Option<&str>) -> Result<Option<i64>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    value
        .parse::<i64>()
        .map(Some)
        .map_err(|_| "LiveSTT consultation ID must be an integer".to_string())
}

fn resolve_timeout_result(current: &LiveSttResult) -> Result<LiveSttResult, String> {
    if !current.final_text.trim().is_empty() {
        log::warn!("LiveSTT finalization timed out; using accumulated final transcript");
        return Ok(current.clone());
    }

    Err("Timed out waiting for LiveSTT final transcript".to_string())
}

fn ensure_no_audio_overflow(overflowed: bool) -> Result<(), String> {
    if overflowed {
        Err(LIVESTT_AUDIO_OVERFLOW_ERROR.to_string())
    } else {
        Ok(())
    }
}

pub fn classify_livestt_error(error: &str) -> &'static str {
    if error.contains("login is required")
        || error.contains("login expired")
        || error.contains("access token is required")
    {
        LIVESTT_ERROR_AUTH_REQUIRED
    } else if error.contains("token refresh failed with status 401")
        || error.contains("token refresh failed with status 403")
    {
        LIVESTT_ERROR_AUTH_REQUIRED
    } else if error.contains("server URL") {
        LIVESTT_ERROR_INVALID_SERVER_URL
    } else if error.contains("consultation ID") {
        LIVESTT_ERROR_INVALID_CONSULTATION_ID
    } else if error.contains("Timed out connecting to LiveSTT server") {
        LIVESTT_ERROR_CONNECT_TIMEOUT
    } else if error.contains("WebSocket connection failed") {
        LIVESTT_ERROR_CONNECT_FAILED
    } else if error.contains(LIVESTT_AUDIO_OVERFLOW_ERROR) {
        LIVESTT_ERROR_AUDIO_QUEUE_OVERFLOW
    } else if error.contains("audio writer") {
        LIVESTT_ERROR_AUDIO_WRITER_FAILED
    } else if error.contains("Timed out waiting for LiveSTT") {
        LIVESTT_ERROR_FINALIZE_TIMEOUT
    } else if error.contains("canceled") {
        LIVESTT_ERROR_CANCELED
    } else {
        LIVESTT_ERROR_WEBSOCKET_CLOSED
    }
}

pub fn is_livestt_auth_connect_error(error: &str) -> bool {
    let normalized = error.to_lowercase();
    normalized.contains("401")
        || normalized.contains("403")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
}

fn is_livestt_auth_runtime_error(error: &str) -> bool {
    let normalized = error.to_lowercase();
    normalized.contains("websocket auth expired")
        || normalized.contains("token_expired")
        || normalized.contains("auth_expired")
        || normalized.contains("auth_required")
        || contains_explicit_auth_failure_marker(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(
        final_text: &str,
        pending_partial: Option<&str>,
        current_text: &str,
    ) -> LiveSttResult {
        LiveSttResult {
            session_id: Some(1),
            final_text: final_text.to_string(),
            pending_partial: pending_partial.map(ToString::to_string),
            current_text: current_text.to_string(),
        }
    }

    #[test]
    fn livestt_session_manager_reserves_only_one_session() {
        let manager = LiveSttSessionManager::default();

        let generation = manager.reserve_starting().unwrap();
        assert!(manager.reserve_starting().is_err());
        assert!(manager.is_active());

        manager.clear_starting(generation);

        assert!(!manager.is_active());
    }

    #[test]
    fn livestt_cancel_clears_starting_session() {
        let manager = LiveSttSessionManager::default();

        manager.reserve_starting().unwrap();
        assert!(manager.is_active());

        manager.cancel_session().unwrap();

        assert!(!manager.is_active());
        assert!(manager.reserve_starting().is_ok());
    }

    #[test]
    fn livestt_clear_starting_ignores_old_generation() {
        let manager = LiveSttSessionManager::default();

        let old_generation = manager.reserve_starting().unwrap();
        manager.cancel_session().unwrap();
        let new_generation = manager.reserve_starting().unwrap();

        manager.clear_starting(old_generation);

        assert!(manager.is_active());
        assert!(manager.reserve_starting().is_err());
        manager.clear_starting(new_generation);
        assert!(!manager.is_active());
    }

    #[test]
    fn livestt_parse_consultation_id_accepts_empty() {
        assert_eq!(parse_consultation_id(None).unwrap(), None);
        assert_eq!(parse_consultation_id(Some("  ")).unwrap(), None);
    }

    #[test]
    fn livestt_parse_consultation_id_accepts_integer() {
        assert_eq!(parse_consultation_id(Some("42")).unwrap(), Some(42));
    }

    #[test]
    fn livestt_parse_consultation_id_rejects_non_integer() {
        assert!(parse_consultation_id(Some("abc")).is_err());
    }

    #[test]
    fn livestt_timeout_uses_final_text_when_available() {
        let current = result("final", Some("preview"), "preview");

        let result = resolve_timeout_result(&current).unwrap();

        assert_eq!(result.final_text, "final");
        assert_eq!(result.current_text, "preview");
        assert_eq!(result.pending_partial.as_deref(), Some("preview"));
    }

    #[test]
    fn livestt_timeout_uses_accumulated_final_text_when_available() {
        let current = result(
            "hello world",
            Some("ignored preview"),
            "hello world ignored preview",
        );

        let result = resolve_timeout_result(&current).unwrap();

        assert_eq!(result.final_text, "hello world");
    }

    #[test]
    fn livestt_timeout_rejects_current_text() {
        let current = result("", Some("partial"), "partial");

        assert!(resolve_timeout_result(&current).is_err());
    }

    #[test]
    fn livestt_timeout_rejects_empty_final_and_current_text() {
        let current = result("", None, "");

        assert!(resolve_timeout_result(&current).is_err());
    }

    #[test]
    fn livestt_audio_sink_sets_overflow_on_full_queue() {
        let (sender, _receiver) = mpsc::channel::<Vec<f32>>(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        let sink = LiveSttAudioSink::new(sender, overflowed);

        sink.try_send_chunk(vec![0.0]);
        sink.try_send_chunk(vec![1.0]);

        assert!(sink.overflowed());
    }

    #[test]
    fn livestt_audio_sink_does_not_overflow_on_closed_queue() {
        let (sender, receiver) = mpsc::channel::<Vec<f32>>(1);
        drop(receiver);
        let overflowed = Arc::new(AtomicBool::new(false));
        let sink = LiveSttAudioSink::new(sender, overflowed);

        sink.try_send_chunk(vec![0.0]);

        assert!(!sink.overflowed());
    }

    #[test]
    fn livestt_overflow_prevents_successful_result() {
        let error = ensure_no_audio_overflow(true).unwrap_err();

        assert_eq!(error, LIVESTT_AUDIO_OVERFLOW_ERROR);
        assert!(ensure_no_audio_overflow(false).is_ok());
    }

    #[test]
    fn livestt_auth_connect_error_detects_auth_failures() {
        assert!(is_livestt_auth_connect_error("HTTP error 401"));
        assert!(is_livestt_auth_connect_error("401 Unauthorized"));
        assert!(is_livestt_auth_connect_error("403 Forbidden"));
        assert!(is_livestt_auth_connect_error(
            "LiveSTT WebSocket connection failed: HTTP error 401"
        ));
    }

    #[test]
    fn livestt_auth_connect_error_ignores_network_failures() {
        assert!(!is_livestt_auth_connect_error("connection refused"));
        assert!(!is_livestt_auth_connect_error("dns error"));
        assert!(!is_livestt_auth_connect_error(
            "Timed out connecting to LiveSTT server"
        ));
    }

    #[test]
    fn livestt_auth_runtime_error_ignores_generic_auth_words() {
        assert!(!is_livestt_auth_runtime_error(
            "authentication backend overloaded"
        ));
        assert!(!is_livestt_auth_runtime_error("backend overloaded"));
        assert!(is_livestt_auth_runtime_error("token expired"));
        assert!(is_livestt_auth_runtime_error("jwt expired"));
        assert!(is_livestt_auth_runtime_error("access token expired"));
        assert!(is_livestt_auth_runtime_error("unauthorized"));
        assert!(is_livestt_auth_runtime_error("forbidden"));
    }

    #[test]
    fn livestt_classify_login_expired_as_auth_required() {
        assert_eq!(
            classify_livestt_error("LiveSTT login expired; please log in again"),
            LIVESTT_ERROR_AUTH_REQUIRED
        );
    }
}
