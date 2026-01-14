# SayType HTTP API Implementation Plan

> For Claude: REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

Goal: 讓 Handy 桌面應用程式對外提供語音轉文字 HTTP API，供手機端 App 透過區域網路呼叫。

Architecture: 使用 axum 框架建立 HTTP 伺服器，透過 Tauri state 存取 TranscriptionManager 執行轉錄。設定儲存於現有的 tauri-plugin-store。前端使用 React 提供設定介面。

Tech Stack: Rust (axum 0.8, tower-http, hound, base64), React/TypeScript, Tauri 2.x, tauri-plugin-store

---

## Task 1: 新增 Cargo 依賴

Files:
- Modify: `src-tauri/Cargo.toml:76-80`

Step 1: 新增 saytype feature 所需的額外依賴

在 Cargo.toml 的 SayType 區塊加入 `rand` 用於 token 產生：

```toml
# SayType 擴充模組依賴（optional）
axum = { version = "0.8", optional = true }
tower-http = { version = "0.6", features = ["cors"], optional = true }
base64 = { version = "0.22", optional = true }
rand = { version = "0.8", optional = true }
```

Step 2: 更新 feature flags

```toml
[features]
default = []
saytype = ["dep:axum", "dep:tower-http", "dep:base64", "dep:rand"]
```

Step 3: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功，無錯誤

Step 4: Commit

```bash
git add src-tauri/Cargo.toml
git commit -m "feat(saytype): add rand dependency for token generation"
```

---

## Task 2: 實作 config.rs - SayTypeConfig 結構

Files:
- Create: `src-tauri/src/saytype/config.rs`
- Modify: `src-tauri/src/saytype/mod.rs`

Step 1: 建立 config.rs 檔案

```rust
//! SayType API 設定管理

use rand::Rng;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const SAYTYPE_STORE_PATH: &str = "saytype_config.json";

#[derive(Serialize, Deserialize, Clone, Debug, Type)]
pub struct SayTypeConfig {
    /// API 是否啟用
    pub enabled: bool,
    /// 監聽埠號 (預設 8765)
    pub port: u16,
    /// 認證 Token
    pub token: String,
    /// 是否已完成首次設定引導
    pub onboarded: bool,
}

impl Default for SayTypeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8765,
            token: generate_random_token(),
            onboarded: false,
        }
    }
}

/// 產生 32 字元隨機 token
pub fn generate_random_token() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

/// 讀取 SayType 設定，若不存在則建立預設值
pub fn get_saytype_config(app: &AppHandle) -> SayTypeConfig {
    let store = app
        .store(SAYTYPE_STORE_PATH)
        .expect("Failed to initialize saytype store");

    if let Some(config_value) = store.get("config") {
        serde_json::from_value::<SayTypeConfig>(config_value).unwrap_or_else(|_| {
            let default_config = SayTypeConfig::default();
            store.set("config", serde_json::to_value(&default_config).unwrap());
            default_config
        })
    } else {
        let default_config = SayTypeConfig::default();
        store.set("config", serde_json::to_value(&default_config).unwrap());
        default_config
    }
}

/// 寫入 SayType 設定
pub fn write_saytype_config(app: &AppHandle, config: SayTypeConfig) {
    let store = app
        .store(SAYTYPE_STORE_PATH)
        .expect("Failed to initialize saytype store");
    store.set("config", serde_json::to_value(&config).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_random_token() {
        let token = generate_random_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_default_config() {
        let config = SayTypeConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.port, 8765);
        assert_eq!(config.token.len(), 32);
        assert!(!config.onboarded);
    }
}
```

Step 2: 更新 mod.rs 加入 config 模組

```rust
//! SayType 擴充模組
//!
//! 提供 HTTP API 伺服器，讓手機應用程式可以透過區域網路
//! 存取 Desktop 端的語音轉文字功能。

pub mod api_server;
pub mod config;
pub mod handlers;
pub mod types;
```

Step 3: 驗證編譯與測試

Run: `cd src-tauri && cargo test --features saytype config`
Expected: 測試通過

Step 4: Commit

```bash
git add src-tauri/src/saytype/config.rs src-tauri/src/saytype/mod.rs
git commit -m "feat(saytype): add SayTypeConfig with token generation"
```

---

## Task 3: 更新 types.rs - 補充缺少欄位

Files:
- Modify: `src-tauri/src/saytype/types.rs`

Step 1: 更新 types.rs 加入 API 設計中的欄位

```rust
//! SayType API 請求/回應類型定義

use serde::{Deserialize, Serialize};

/// 轉錄請求
#[derive(Deserialize)]
pub struct TranscribeRequest {
    /// Base64 編碼的音訊資料
    pub audio_base64: String,
    /// 音訊格式（wav 或 ogg）
    pub format: Option<String>,
    /// 取樣率（預設 16000）
    pub sample_rate: Option<u32>,
    /// 是否啟用 LLM 潤飾（保留介面，目前不處理）
    pub polish: Option<bool>,
}

/// 轉錄回應
#[derive(Serialize)]
pub struct TranscribeResponse {
    /// 是否成功
    pub success: bool,
    /// 原始轉錄文字
    pub raw_text: String,
    /// 潤飾後的文字（目前與 raw_text 相同）
    pub polished_text: String,
    /// 偵測到的語言
    pub language: String,
    /// 處理時間（毫秒）
    pub processing_time_ms: u64,
}

/// 狀態回應
#[derive(Serialize)]
pub struct StatusResponse {
    /// 伺服器狀態：ready, loading, error
    pub status: String,
    /// 模型是否已載入
    pub model_loaded: bool,
    /// 當前模型名稱
    pub current_model: Option<String>,
    /// 應用程式版本
    pub version: String,
}

/// 錯誤回應
#[derive(Serialize)]
pub struct ErrorResponse {
    /// 錯誤訊息
    pub error: String,
    /// 錯誤代碼
    pub code: String,
}

/// 錯誤代碼常數
pub mod error_codes {
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    pub const INVALID_FORMAT: &str = "INVALID_FORMAT";
    pub const DECODE_ERROR: &str = "DECODE_ERROR";
    pub const MODEL_NOT_LOADED: &str = "MODEL_NOT_LOADED";
    pub const TRANSCRIBE_ERROR: &str = "TRANSCRIBE_ERROR";
}
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/saytype/types.rs
git commit -m "feat(saytype): update types with complete API fields"
```

---

## Task 4: 實作 audio_convert.rs - WAV 解碼

Files:
- Create: `src-tauri/src/saytype/audio_convert.rs`
- Modify: `src-tauri/src/saytype/mod.rs`

Step 1: 建立 audio_convert.rs 檔案（WAV 支援）

```rust
//! 音訊格式轉換
//!
//! 將 Base64 編碼的音訊資料轉換為 16kHz mono f32 samples

use base64::Engine;
use std::io::Cursor;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioConvertError {
    #[error("Base64 decode error: {0}")]
    Base64DecodeError(#[from] base64::DecodeError),
    #[error("WAV decode error: {0}")]
    WavDecodeError(#[from] hound::Error),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Unsupported sample format")]
    UnsupportedSampleFormat,
}

pub struct AudioConvertResult {
    /// 16kHz mono f32 samples (-1.0 ~ 1.0)
    pub samples: Vec<f32>,
    /// 音訊長度（毫秒）
    pub duration_ms: u64,
}

/// 從 Base64 字串轉換為 f32 samples
pub fn convert_from_base64(
    base64_data: &str,
    format: &str,
) -> Result<AudioConvertResult, AudioConvertError> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64_data)?;
    convert_from_bytes(&bytes, format)
}

/// 從原始 bytes 轉換為 f32 samples
pub fn convert_from_bytes(
    bytes: &[u8],
    format: &str,
) -> Result<AudioConvertResult, AudioConvertError> {
    match format.to_lowercase().as_str() {
        "wav" => decode_wav(bytes),
        "ogg" => {
            // OGG/Opus 支援將在後續實作
            Err(AudioConvertError::UnsupportedFormat("ogg (not yet implemented)".to_string()))
        }
        _ => Err(AudioConvertError::UnsupportedFormat(format.to_string())),
    }
}

/// 解碼 WAV 檔案並重採樣至 16kHz mono
fn decode_wav(bytes: &[u8]) -> Result<AudioConvertResult, AudioConvertError> {
    let cursor = Cursor::new(bytes);
    let mut reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();

    // 讀取所有 samples 並轉換為 f32
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max_value = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_value)
                .collect()
        }
        hound::SampleFormat::Float => {
            reader
                .samples::<f32>()
                .filter_map(|s| s.ok())
                .collect()
        }
    };

    // 轉換為 mono（如果是多聲道）
    let mono_samples = if spec.channels > 1 {
        samples
            .chunks(spec.channels as usize)
            .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
            .collect()
    } else {
        samples
    };

    // 重採樣至 16kHz（如果需要）
    let target_sample_rate = 16000;
    let final_samples = if spec.sample_rate != target_sample_rate {
        resample(&mono_samples, spec.sample_rate, target_sample_rate)
    } else {
        mono_samples
    };

    let duration_ms = (final_samples.len() as u64 * 1000) / target_sample_rate as u64;

    Ok(AudioConvertResult {
        samples: final_samples,
        duration_ms,
    })
}

/// 簡單線性重採樣
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;

    (0..output_len)
        .map(|i| {
            let src_idx = i as f64 * ratio;
            let idx = src_idx.floor() as usize;
            let frac = src_idx.fract() as f32;

            if idx + 1 < samples.len() {
                samples[idx] * (1.0 - frac) + samples[idx + 1] * frac
            } else if idx < samples.len() {
                samples[idx]
            } else {
                0.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_format() {
        let result = convert_from_bytes(&[], "mp3");
        assert!(matches!(result, Err(AudioConvertError::UnsupportedFormat(_))));
    }

    #[test]
    fn test_resample() {
        // 48kHz -> 16kHz (3:1 ratio)
        let input: Vec<f32> = (0..48).map(|i| i as f32 / 48.0).collect();
        let output = resample(&input, 48000, 16000);
        assert_eq!(output.len(), 16);
    }
}
```

Step 2: 更新 mod.rs

```rust
//! SayType 擴充模組
//!
//! 提供 HTTP API 伺服器，讓手機應用程式可以透過區域網路
//! 存取 Desktop 端的語音轉文字功能。

pub mod api_server;
pub mod audio_convert;
pub mod config;
pub mod handlers;
pub mod types;
```

Step 3: 驗證編譯與測試

Run: `cd src-tauri && cargo test --features saytype audio_convert`
Expected: 測試通過

Step 4: Commit

```bash
git add src-tauri/src/saytype/audio_convert.rs src-tauri/src/saytype/mod.rs
git commit -m "feat(saytype): add audio_convert with WAV decoding"
```

---

## Task 5: 實作 handlers.rs - status handler

Files:
- Modify: `src-tauri/src/saytype/handlers.rs`

Step 1: 實作 status handler

```rust
//! SayType API 請求處理器

use crate::managers::transcription::TranscriptionManager;
use crate::saytype::types::{ErrorResponse, StatusResponse, error_codes};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use tauri::AppHandle;

/// 應用程式狀態，包含 Tauri AppHandle
#[derive(Clone)]
pub struct AppState {
    pub app_handle: AppHandle,
    pub token: String,
}

/// 驗證 Authorization header
fn verify_token(headers: &HeaderMap, expected_token: &str) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
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
pub async fn status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
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
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/saytype/handlers.rs
git commit -m "feat(saytype): implement status handler with token auth"
```

---

## Task 6: 實作 handlers.rs - transcribe handler

Files:
- Modify: `src-tauri/src/saytype/handlers.rs`

Step 1: 在 handlers.rs 加入 transcribe handler

在 `status` 函數後加入：

```rust
use crate::saytype::audio_convert::convert_from_base64;
use crate::saytype::types::TranscribeRequest;
use crate::saytype::types::TranscribeResponse;

/// POST /api/transcribe - 執行語音轉文字
pub async fn transcribe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<TranscribeRequest>,
) -> impl IntoResponse {
    // 驗證 token
    if let Err(err) = verify_token(&headers, &state.token) {
        return err.into_response();
    }

    let start_time = std::time::Instant::now();

    // 取得音訊格式（預設 wav）
    let format = request.format.as_deref().unwrap_or("wav");

    // 轉換音訊
    let audio_result = match convert_from_base64(&request.audio_base64, format) {
        Ok(result) => result,
        Err(e) => {
            let code = match &e {
                crate::saytype::audio_convert::AudioConvertError::Base64DecodeError(_) => {
                    error_codes::DECODE_ERROR
                }
                _ => error_codes::INVALID_FORMAT,
            };
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                    code: code.to_string(),
                }),
            )
                .into_response();
        }
    };

    // 取得 TranscriptionManager 並執行轉錄
    let transcription_result = {
        let tm = match state.app_handle.try_state::<Arc<TranscriptionManager>>() {
            Some(tm) => tm,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: "Transcription service not available".to_string(),
                        code: error_codes::MODEL_NOT_LOADED.to_string(),
                    }),
                )
                    .into_response();
            }
        };

        if !tm.is_model_loaded() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: "Model not loaded".to_string(),
                    code: error_codes::MODEL_NOT_LOADED.to_string(),
                }),
            )
                .into_response();
        }

        tm.transcribe(audio_result.samples)
    };

    match transcription_result {
        Ok(text) => {
            let processing_time_ms = start_time.elapsed().as_millis() as u64;
            let response = TranscribeResponse {
                success: true,
                raw_text: text.clone(),
                polished_text: text, // 目前不處理潤飾
                language: "auto".to_string(),
                processing_time_ms,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: error_codes::TRANSCRIBE_ERROR.to_string(),
            }),
        )
            .into_response(),
    }
}
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/saytype/handlers.rs
git commit -m "feat(saytype): implement transcribe handler"
```

---

## Task 7: 實作 api_server.rs - axum Router

Files:
- Modify: `src-tauri/src/saytype/api_server.rs`

Step 1: 實作完整的 API Server

```rust
//! SayType HTTP API 伺服器
//!
//! 使用 axum 框架提供 RESTful API

use crate::saytype::config::get_saytype_config;
use crate::saytype::handlers::{status, transcribe, AppState};
use axum::{routing::{get, post}, Router};
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
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/saytype/api_server.rs
git commit -m "feat(saytype): implement axum HTTP server with CORS"
```

---

## Task 8: 實作 Tauri Commands

Files:
- Create: `src-tauri/src/commands/saytype.rs`
- Modify: `src-tauri/src/commands/mod.rs`

Step 1: 建立 saytype commands

```rust
//! SayType Tauri Commands

use crate::saytype::config::{
    generate_random_token, get_saytype_config, write_saytype_config, SayTypeConfig,
};
use tauri::AppHandle;

#[cfg(feature = "saytype")]
#[tauri::command]
#[specta::specta]
pub fn saytype_get_config(app: AppHandle) -> Result<SayTypeConfig, String> {
    Ok(get_saytype_config(&app))
}

#[cfg(feature = "saytype")]
#[tauri::command]
#[specta::specta]
pub fn saytype_set_config(app: AppHandle, config: SayTypeConfig) -> Result<(), String> {
    write_saytype_config(&app, config);
    Ok(())
}

#[cfg(feature = "saytype")]
#[tauri::command]
#[specta::specta]
pub fn saytype_regenerate_token(app: AppHandle) -> Result<String, String> {
    let mut config = get_saytype_config(&app);
    config.token = generate_random_token();
    write_saytype_config(&app, config.clone());
    Ok(config.token)
}

#[cfg(feature = "saytype")]
#[tauri::command]
#[specta::specta]
pub fn saytype_get_local_ip() -> Result<String, String> {
    // 取得本機 IP（優先使用非 loopback 的 IPv4）
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect("8.8.8.8:80").map_err(|e| e.to_string())?;
    let local_addr = socket.local_addr().map_err(|e| e.to_string())?;

    Ok(local_addr.ip().to_string())
}
```

Step 2: 更新 commands/mod.rs

在現有的 `pub mod` 區塊後加入：

```rust
#[cfg(feature = "saytype")]
pub mod saytype;
```

Step 3: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 4: Commit

```bash
git add src-tauri/src/commands/saytype.rs src-tauri/src/commands/mod.rs
git commit -m "feat(saytype): add Tauri commands for config management"
```

---

## Task 9: 註冊 Tauri Commands 到 lib.rs

Files:
- Modify: `src-tauri/src/lib.rs`

Step 1: 在 specta_builder 加入 saytype commands

找到 `let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![` 區塊，在最後加入：

```rust
        #[cfg(feature = "saytype")]
        commands::saytype::saytype_get_config,
        #[cfg(feature = "saytype")]
        commands::saytype::saytype_set_config,
        #[cfg(feature = "saytype")]
        commands::saytype::saytype_regenerate_token,
        #[cfg(feature = "saytype")]
        commands::saytype::saytype_get_local_ip,
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(saytype): register Tauri commands in specta builder"
```

---

## Task 10: 更新 lib.rs 啟動邏輯使用新的 config

Files:
- Modify: `src-tauri/src/lib.rs:219-232`

Step 1: 更新 SayType 啟動區塊使用新的 config 模組

找到現有的 SayType 啟動區塊並替換：

```rust
    // SayType API Server（條件編譯）
    #[cfg(feature = "saytype")]
    {
        use crate::saytype::config::get_saytype_config;

        let app_handle_clone = app_handle.clone();
        let config = get_saytype_config(app_handle);

        if config.enabled {
            tauri::async_runtime::spawn(async move {
                saytype::api_server::start_api_server(app_handle_clone, config.port).await;
            });
        }
    }
```

Step 2: 驗證編譯

Run: `cd src-tauri && cargo check --features saytype`
Expected: 編譯成功

Step 3: Commit

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(saytype): update startup logic to use SayTypeConfig"
```

---

## Task 11: 建立 i18n 翻譯

Files:
- Modify: `src/i18n/locales/en/translation.json`
- Modify: `src/i18n/locales/zh-Hant/translation.json` (若存在)

Step 1: 加入英文翻譯

在 `translation.json` 加入 saytype 區塊：

```json
{
  "saytype": {
    "title": "SayType Remote Input",
    "enable_api": "Enable API Server",
    "connection_info": "Connection Info",
    "server_address": "Server Address",
    "auth_token": "Authentication Token",
    "regenerate_token": "Regenerate Token",
    "port": "Port",
    "port_change_note": "Restart API after changing port",
    "copy": "Copy",
    "show": "Show",
    "hide": "Hide",
    "onboarding": {
      "title": "Enable SayType Remote Input?",
      "description": "SayType allows your phone to use this computer's speech-to-text feature over the local network.",
      "note": "After enabling, devices on the same network can connect via API (requires authentication token).",
      "enable": "Enable SayType",
      "skip": "Not Now"
    }
  }
}
```

Step 2: 驗證 JSON 語法

Run: `cd src && cat i18n/locales/en/translation.json | python3 -m json.tool > /dev/null && echo "Valid JSON"`
Expected: "Valid JSON"

Step 3: Commit

```bash
git add src/i18n/locales/
git commit -m "feat(saytype): add i18n translations"
```

---

## Task 12: 建立 SayTypeSettings.tsx 元件

Files:
- Create: `src/components/settings/SayTypeSettings.tsx`

Step 1: 建立設定頁面元件

```tsx
import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

interface SayTypeConfig {
  enabled: boolean;
  port: number;
  token: string;
  onboarded: boolean;
}

export function SayTypeSettings() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<SayTypeConfig | null>(null);
  const [localIp, setLocalIp] = useState<string>('');
  const [showToken, setShowToken] = useState(false);
  const [portInput, setPortInput] = useState('');

  useEffect(() => {
    loadConfig();
    loadLocalIp();
  }, []);

  async function loadConfig() {
    try {
      const cfg = await invoke<SayTypeConfig>('saytype_get_config');
      setConfig(cfg);
      setPortInput(cfg.port.toString());
    } catch (e) {
      console.error('Failed to load SayType config:', e);
    }
  }

  async function loadLocalIp() {
    try {
      const ip = await invoke<string>('saytype_get_local_ip');
      setLocalIp(ip);
    } catch (e) {
      console.error('Failed to get local IP:', e);
    }
  }

  async function handleToggle(enabled: boolean) {
    if (!config) return;
    const newConfig = { ...config, enabled };
    await invoke('saytype_set_config', { config: newConfig });
    setConfig(newConfig);
  }

  async function handleRegenerateToken() {
    try {
      const newToken = await invoke<string>('saytype_regenerate_token');
      if (config) {
        setConfig({ ...config, token: newToken });
      }
    } catch (e) {
      console.error('Failed to regenerate token:', e);
    }
  }

  async function handlePortChange() {
    if (!config) return;
    const port = parseInt(portInput, 10);
    if (port >= 1024 && port <= 65535) {
      const newConfig = { ...config, port };
      await invoke('saytype_set_config', { config: newConfig });
      setConfig(newConfig);
    }
  }

  async function copyToClipboard(text: string) {
    await writeText(text);
  }

  if (!config) {
    return <div>Loading...</div>;
  }

  const serverUrl = `http://${localIp}:${config.port}`;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <span>{t('saytype.enable_api')}</span>
        <input
          type="checkbox"
          checked={config.enabled}
          onChange={(e) => handleToggle(e.target.checked)}
          className="toggle"
        />
      </div>

      {config.enabled && (
        <div className="space-y-4 pt-4 border-t">
          <h3 className="font-medium">{t('saytype.connection_info')}</h3>

          <div>
            <label className="text-sm text-gray-500">{t('saytype.server_address')}</label>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={serverUrl}
                readOnly
                className="input input-bordered flex-1"
              />
              <button
                onClick={() => copyToClipboard(serverUrl)}
                className="btn btn-sm"
              >
                {t('saytype.copy')}
              </button>
            </div>
          </div>

          <div>
            <label className="text-sm text-gray-500">{t('saytype.auth_token')}</label>
            <div className="flex items-center gap-2">
              <input
                type={showToken ? 'text' : 'password'}
                value={config.token}
                readOnly
                className="input input-bordered flex-1"
              />
              <button
                onClick={() => setShowToken(!showToken)}
                className="btn btn-sm"
              >
                {showToken ? t('saytype.hide') : t('saytype.show')}
              </button>
              <button
                onClick={() => copyToClipboard(config.token)}
                className="btn btn-sm"
              >
                {t('saytype.copy')}
              </button>
            </div>
            <button
              onClick={handleRegenerateToken}
              className="btn btn-sm btn-outline mt-2"
            >
              {t('saytype.regenerate_token')}
            </button>
          </div>

          <div>
            <label className="text-sm text-gray-500">{t('saytype.port')}</label>
            <div className="flex items-center gap-2">
              <input
                type="number"
                value={portInput}
                onChange={(e) => setPortInput(e.target.value)}
                onBlur={handlePortChange}
                min={1024}
                max={65535}
                className="input input-bordered w-24"
              />
              <span className="text-sm text-gray-500">
                {t('saytype.port_change_note')}
              </span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
```

Step 2: 驗證 TypeScript 語法

Run: `cd src && npx tsc --noEmit`
Expected: 無 TypeScript 錯誤（或僅有既有錯誤）

Step 3: Commit

```bash
git add src/components/settings/SayTypeSettings.tsx
git commit -m "feat(saytype): add SayTypeSettings component"
```

---

## Task 13: 整合 SayTypeSettings 到設定頁面

Files:
- Modify: 設定頁面主檔案（需確認實際路徑）

Step 1: 找出設定頁面檔案

Run: `grep -r "Settings" src/components/settings/*.tsx | head -5`

根據搜尋結果，在適當的設定頁面檔案中引入 SayTypeSettings。

Step 2: 引入 SayTypeSettings 元件

在設定頁面加入：

```tsx
import { SayTypeSettings } from './SayTypeSettings';

// 在 JSX 中適當位置加入
<SayTypeSettings />
```

Step 3: 驗證編譯

Run: `bun run build`
Expected: 建置成功

Step 4: Commit

```bash
git add src/components/settings/
git commit -m "feat(saytype): integrate SayTypeSettings into settings page"
```

---

## Task 14: 整合測試

Step 1: 啟動開發模式

Run: `CMAKE_POLICY_VERSION_MINIMUM=3.5 bun run tauri dev -- --features saytype`

Step 2: 手動測試 API

1. 在設定中啟用 SayType API
2. 記下顯示的 token
3. 測試 status endpoint：

```bash
curl -H "Authorization: Bearer <token>" http://localhost:8765/api/status
```

Expected: 回傳 JSON 包含 status, model_loaded, version

Step 3: 測試 transcribe endpoint

準備一個短的 WAV 檔案並執行：

```bash
# 將 test.wav 轉為 base64
BASE64=$(base64 -i test.wav)
curl -X POST -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d "{\"audio_base64\": \"$BASE64\", \"format\": \"wav\"}" \
  http://localhost:8765/api/transcribe
```

Step 4: Commit 整合完成

```bash
git add -A
git commit -m "feat(saytype): complete SayType HTTP API implementation"
```

---

## Summary

實作完成後的檔案結構：

```
src-tauri/src/saytype/
├── mod.rs           # 模組入口
├── api_server.rs    # HTTP 伺服器
├── handlers.rs      # API handlers
├── types.rs         # 請求/回應類型
├── audio_convert.rs # 音訊轉換
└── config.rs        # 設定管理

src-tauri/src/commands/
└── saytype.rs       # Tauri commands

src/components/settings/
└── SayTypeSettings.tsx  # 設定介面

src/i18n/locales/
└── en/translation.json  # 翻譯
```

API Endpoints:
- `GET /api/status` - 查詢伺服器狀態
- `POST /api/transcribe` - 執行語音轉文字
