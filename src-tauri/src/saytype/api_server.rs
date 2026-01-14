//! SayType HTTP API 伺服器
//!
//! 使用 axum 框架提供 RESTful API

use tauri::AppHandle;

/// 啟動 SayType API 伺服器
///
/// # Arguments
/// * `app_handle` - Tauri 應用程式句柄，用於存取 managers
/// * `port` - 伺服器監聽埠號
pub async fn start_api_server(_app_handle: AppHandle, port: u16) {
    log::info!("SayType API 伺服器將於 port {} 啟動", port);

    // TODO: 實作 axum 伺服器
    // 1. 建立 Router 並註冊路由
    // 2. 設定 CORS 中介層
    // 3. 將 AppHandle 注入為 Extension
    // 4. 綁定並啟動伺服器

    // 路由規劃：
    // GET  /api/status     - 取得伺服器狀態
    // POST /api/transcribe - 執行語音轉文字
    // GET  /api/models     - 列出可用模型
}
