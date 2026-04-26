// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use handy_app_lib::CliArgs;

fn main() {
    let cli_args = CliArgs::parse();

    #[cfg(target_os = "linux")]
    {
        // DMABUF renderer causes crashes on various GPU/display server configurations
        // See: https://github.com/tauri-apps/tauri/issues/9394
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // On Windows, configure WebView2 proxy + loopback bypass via environment variable
    // BEFORE Tauri/WebView2 initialises its process-wide environment.
    // We use WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS (Chromium command-line flags) instead
    // of wry's proxy_url() API because the API-based proxy has no bypass-list support,
    // meaning even http://localhost:1420 (dev) and http://tauri.localhost (prod) would be
    // routed through the proxy—causing a blank main window when the proxy can't handle
    // loopback traffic (e.g. a gateway-style API proxy).
    #[cfg(target_os = "windows")]
    configure_webview2_proxy();

    handy_app_lib::run(cli_args)
}

/// Read proxy_url from the persisted settings file and inject it into
/// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS together with a bypass list for
/// loopback / tauri.localhost addresses.  Must be called before Tauri starts.
#[cfg(target_os = "windows")]
fn configure_webview2_proxy() {
    let Some(proxy_url) = read_proxy_url_from_settings() else {
        return;
    };

    let proxy_args = format!(
        "--proxy-server={proxy_url} --proxy-bypass-list=<local>;localhost;127.0.0.1;tauri.localhost"
    );
    let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
    let combined = if existing.is_empty() {
        proxy_args
    } else {
        format!("{existing} {proxy_args}")
    };
    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", combined);
}

/// Parse settings_store.json without a Tauri AppHandle to extract proxy_url.
/// Mirrors portable::store_path() logic so portable-mode installs work too.
#[cfg(target_os = "windows")]
fn read_proxy_url_from_settings() -> Option<String> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();

    let settings_path = if exe_dir.join("portable").exists() {
        exe_dir.join("Data").join("settings_store.json")
    } else {
        let app_data = std::env::var("APPDATA").ok()?;
        std::path::PathBuf::from(app_data)
            .join("com.pais.handy")
            .join("settings_store.json")
    };

    let content = std::fs::read_to_string(&settings_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    let url = json["settings"]["proxy_url"].as_str()?;
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}
