//! SayType 擴充模組
//!
//! 提供 HTTP API 伺服器，讓手機應用程式可以透過區域網路
//! 存取 Desktop 端的語音轉文字功能。

pub mod api_server;
pub mod audio_convert;
pub mod config;
pub mod handlers;
pub mod types;

// TODO: 後續實作
// pub mod llm_polish;
