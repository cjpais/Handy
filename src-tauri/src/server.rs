use crate::managers::transcription::TranscriptionManager;
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use log::{error, info};
use serde::Serialize;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

pub struct ServerState {
    pub transcription_manager: Arc<TranscriptionManager>,
}

#[derive(Clone)]
pub struct AppState {
    pub state: Arc<ServerState>,
}

#[derive(Debug, Serialize)]
struct Segment {
    id: u32,
    seek: u32,
    start: f64,
    end: f64,
    text: String,
    tokens: Vec<u32>,
    temperature: f32,
    avg_logprob: f32,
    compression_ratio: f32,
    no_speech_prob: f32,
}

#[derive(Debug, Serialize)]
pub struct GroqTranscriptionResponse {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    segments: Option<Vec<Segment>>,
    // We could add words here if we support word-level timestamps in the future
}

pub struct ApiServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub port: u16,
}

impl ApiServer {
    pub fn new(port: u16) -> Self {
        Self {
            shutdown_tx: None,
            port,
        }
    }

    pub fn start(&mut self, tm: Arc<TranscriptionManager>) {
        let port = self.port;
        let (tx, rx) = oneshot::channel();
        self.shutdown_tx = Some(tx);

        let state = AppState {
            state: Arc::new(ServerState {
                transcription_manager: tm,
            }),
        };

        let app = Router::new()
            .route("/v1/audio/transcriptions", post(transcribe_audio))
            .with_state(state);

        tauri::async_runtime::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            info!("Starting local API server on {}", addr);

            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    if let Err(e) = axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            rx.await.ok();
                        })
                        .await
                    {
                        error!("Server error: {}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to bind server to {}: {}", addr, e);
                }
            }
            info!("Local API server on port {} stopped", port);
        });
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn transcribe_audio(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_bytes = None;
    let mut _model = "whisper-large-v3-turbo".to_string();
    let mut response_format = "json".to_string();
    let mut timestamp_granularities = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        if name == "file" {
            if let Ok(bytes) = field.bytes().await {
                file_bytes = Some(bytes);
            }
        } else if name == "model" {
            if let Ok(text) = field.text().await {
                _model = text;
            }
        } else if name == "response_format" {
            if let Ok(text) = field.text().await {
                response_format = text;
            }
        } else if name == "timestamp_granularities[]" || name == "timestamp_granularities" {
            if let Ok(text) = field.text().await {
                timestamp_granularities.push(text);
            }
        }
    }

    if let Some(bytes) = file_bytes {
        // Decode audio
        let cursor = Cursor::new(bytes);
        let decoder = match rodio::Decoder::new(cursor) {
            Ok(d) => d,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("Failed to decode audio: {}", e)})),
                );
            }
        };

        // Resample and collect samples
        let samples: Vec<f32> = match crate::audio_toolkit::resample_audio(decoder) {
            Ok(s) => s,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Failed to resample audio: {}", e)})),
                );
            }
        };

        // Transcribe
        let verbose = response_format == "verbose_json";
        state.state.transcription_manager.initiate_model_load();
        match state.state.transcription_manager.transcribe_internal(
            samples,
            verbose,
            timestamp_granularities.contains(&"segment".to_string()),
        ) {
            Ok(result) => {
                let segments = if verbose {
                    Some(
                        result
                            .segments
                            .into_iter()
                            .map(|s| Segment {
                                id: s.id,
                                seek: s.seek,
                                start: s.start,
                                end: s.end,
                                text: s.text,
                                tokens: s.tokens,
                                temperature: s.temperature,
                                avg_logprob: s.avg_logprob,
                                compression_ratio: s.compression_ratio,
                                no_speech_prob: s.no_speech_prob,
                            })
                            .collect(),
                    )
                } else {
                    None
                };

                let response = GroqTranscriptionResponse {
                    text: result.text,
                    segments,
                };

                return (
                    StatusCode::OK,
                    Json(serde_json::to_value(response).unwrap()),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Transcription failed: {}", e)})),
                );
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "No file provided"})),
    )
}
