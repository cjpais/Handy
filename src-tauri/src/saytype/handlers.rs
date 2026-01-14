//! SayType API 請求處理器

use crate::managers::transcription::TranscriptionManager;
use crate::saytype::types::{error_codes, ErrorResponse, StatusResponse};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// 應用程式狀態，包含 Tauri AppHandle
#[derive(Clone)]
pub struct AppState {
    pub app_handle: AppHandle,
    pub token: String,
}

/// 驗證 Authorization header
fn verify_token(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

    if token != expected_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid token".to_string(),
                code: error_codes::UNAUTHORIZED.to_string(),
            }),
        ));
    }

    Ok(())
}

/// GET /api/status - 取得伺服器狀態
pub async fn status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    // 驗證 token
    if let Err(err) = verify_token(&headers, &state.token) {
        return err.into_response();
    }

    // 取得 TranscriptionManager 狀態
    let (model_loaded, current_model) = {
        if let Some(tm) = state.app_handle.try_state::<Arc<TranscriptionManager>>() {
            (tm.is_model_loaded(), tm.get_current_model())
        } else {
            (false, None)
        }
    };

    let status = if model_loaded { "ready" } else { "loading" };

    let response = StatusResponse {
        status: status.to_string(),
        model_loaded,
        current_model,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    (StatusCode::OK, Json(response)).into_response()
}
