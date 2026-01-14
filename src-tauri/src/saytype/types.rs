//! SayType API 請求/回應類型定義

use serde::{Deserialize, Serialize};

/// 轉錄請求
#[derive(Deserialize)]
pub struct TranscribeRequest {
    /// Base64 編碼的音訊資料
    pub audio_base64: String,
    /// 音訊格式（wav, mp3, ogg 等）
    pub format: Option<String>,
    /// 是否啟用 LLM 潤飾
    pub polish: Option<bool>,
}

/// 轉錄回應
#[derive(Serialize)]
pub struct TranscribeResponse {
    /// 是否成功
    pub success: bool,
    /// 原始轉錄文字
    pub raw_text: String,
    /// 潤飾後的文字（若未啟用則與 raw_text 相同）
    pub polished_text: String,
    /// 處理時間（毫秒）
    pub processing_time_ms: u64,
}

/// 狀態回應
#[derive(Serialize)]
pub struct StatusResponse {
    /// 伺服器狀態
    pub status: String,
    /// 模型是否已載入
    pub model_loaded: bool,
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
