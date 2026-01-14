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
