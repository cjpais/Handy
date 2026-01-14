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
