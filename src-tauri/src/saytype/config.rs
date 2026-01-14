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
        assert!(token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
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
