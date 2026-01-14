//! SayType HTTP API 伺服器
//!
//! 使用 axum 框架提供 RESTful API

use crate::saytype::config::get_saytype_config;
use crate::saytype::handlers::{status, transcribe, AppState};
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tauri::AppHandle;
use tower_http::cors::{Any, CorsLayer};

/// 啟動 SayType API 伺服器
pub async fn start_api_server(app_handle: AppHandle, port: u16) {
    let config = get_saytype_config(&app_handle);

    let state = Arc::new(AppState {
        app_handle: app_handle.clone(),
        token: config.token.clone(),
    });

    // CORS 設定：允許所有來源（區域網路使用）
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/transcribe", post(transcribe))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    log::info!("SayType API listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("Failed to bind SayType API server: {}", e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        log::error!("SayType API server error: {}", e);
    }
}
