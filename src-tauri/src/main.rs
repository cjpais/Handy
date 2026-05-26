// Prevents additional console window on Windows, including `tauri dev`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::Parser;
use handy_app_lib::CliArgs;

fn main() {
    install_crash_logger();

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

    // Force WebView2 to keep its user-data folder under an ASCII-only path.
    // Without this, WebView2 derives the folder from the exe path, which on
    // installs like "G:\美声智能\" gets re-encoded through the ANSI code page
    // and fails to open — surfaces as Windows Event ID 1005 and an instant
    // launch crash with no Rust panic logged.
    #[cfg(target_os = "windows")]
    force_ascii_webview2_user_data_folder();

    log_startup_marker();

    #[cfg(target_os = "windows")]
    warn_non_ascii_paths();

    handy_app_lib::run(cli_args)
}

/// Pin WebView2's user data folder to `%LOCALAPPDATA%\com.pais.handy\EBWebView`
/// (ASCII), bypassing the default derived from the exe path. This avoids the
/// "Windows cannot access the file" 1005 crash users see when the install dir
/// contains non-ASCII characters like Chinese.
///
/// Skipped in portable mode — there the WebView2 folder is already pinned to
/// the portable Data dir via `WebviewWindowBuilder::data_directory()`.
#[cfg(target_os = "windows")]
fn force_ascii_webview2_user_data_folder() {
    // Respect a user-provided override.
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some() {
        return;
    }

    // Portable install: lib.rs sets the data_directory on the window builder.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("portable").exists() {
                return;
            }
        }
    }

    let Ok(local) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let folder = std::path::PathBuf::from(&local)
        .join("com.pais.handy")
        .join("EBWebView");
    if let Err(e) = std::fs::create_dir_all(&folder) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        append_crash_log(&format!(
            "[{ts}] WEBVIEW2_DATA_FOLDER_CREATE_FAIL path={folder:?} error={e}"
        ));
        return;
    }
    let folder_str = folder.to_string_lossy().to_string();
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &folder_str);

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    append_crash_log(&format!(
        "[{ts}] WEBVIEW2_DATA_FOLDER pinned to {folder_str}"
    ));
}

/// Resolve a writable directory for the early crash log. Tries portable mode first,
/// then %APPDATA%\com.pais.handy, then std::env::temp_dir() as a last resort.
/// Returns None only if even temp_dir() is unwritable (extremely unlikely).
fn crash_log_dir() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("portable").exists() {
                let dir = parent.join("Data");
                if std::fs::create_dir_all(&dir).is_ok() {
                    return Some(dir);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("com.pais.handy");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir);
        }
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::PathBuf::from(home).join(".handy");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir);
        }
    }

    let tmp = std::env::temp_dir().join("com.pais.handy");
    if std::fs::create_dir_all(&tmp).is_ok() {
        return Some(tmp);
    }

    None
}

fn crash_log_path() -> Option<std::path::PathBuf> {
    crash_log_dir().map(|d| d.join("crash.log"))
}

fn append_crash_log(line: &str) {
    let Some(path) = crash_log_path() else {
        eprintln!("{line}");
        return;
    };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
        if !line.ends_with('\n') {
            let _ = f.write_all(b"\n");
        }
    } else {
        eprintln!("{line}");
    }
}

/// Install a panic hook that writes to a file users can send back when the app
/// crashes before Tauri's logger is up. Runs before anything else in main().
///
/// On Windows the hook also calls `std::process::exit(1)` from the main thread
/// so the named pipe held by `tauri_plugin_single_instance` is released
/// immediately. Without this a panicked first instance can leave its IPC
/// channel half-open; the next launch is detected as a "second instance" and
/// silently quits while the user sees nothing — matching the reported
/// "opens once, then never again" symptom.
fn install_crash_logger() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let line = format!("[{ts}] PANIC thread={thread} at {location}: {payload}");
        append_crash_log(&line);
        default_hook(info);

        // Force the whole process to exit on Windows. Without this, a panic
        // on a worker thread leaves the main thread (and the single-instance
        // pipe) alive but useless — next launch is treated as a second
        // instance and disappears silently.
        #[cfg(target_os = "windows")]
        std::process::exit(1);
    }));
}

/// Warn if APPDATA or the exe path contains non-ASCII characters. Some C/C++
/// dependencies (whisper.cpp, certain ONNX Runtime paths) use ANSI fopen()
/// internally and silently fail on non-ASCII paths, producing what looks like
/// a clean crash on launch. We can't actually move the data dir, but we can
/// surface the condition in crash.log so support can correlate it later.
#[cfg(target_os = "windows")]
fn warn_non_ascii_paths() {
    let mut findings: Vec<String> = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_ascii() {
            findings.push(format!("APPDATA contains non-ASCII chars: {appdata}"));
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.is_ascii() {
            findings.push(format!("LOCALAPPDATA contains non-ASCII chars: {local}"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            if !s.is_ascii() {
                findings.push(format!("Executable path contains non-ASCII chars: {s}"));
            }
        }
    }

    for f in &findings {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        append_crash_log(&format!("[{ts}] WARN_NON_ASCII_PATH {f}"));
    }
}

fn log_startup_marker() {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let pid = std::process::id();
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "<unknown>".to_string());
    append_crash_log(&format!(
        "[{ts}] STARTUP pid={pid} version={} exe={exe}",
        env!("CARGO_PKG_VERSION")
    ));
}

/// Read proxy_url from the persisted settings file and inject it into
/// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS together with a bypass list for
/// loopback / tauri.localhost addresses.  Must be called before Tauri starts.
#[cfg(target_os = "windows")]
fn configure_webview2_proxy() {
    let Some(proxy_url) = read_proxy_url_from_settings() else {
        return;
    };

    // Validate URL shape BEFORE handing it to WebView2's command line. An
    // invalid value like "127.0.0.1:7890" (no scheme) or "http:/badhost"
    // makes Chromium refuse to launch the renderer — which on customer
    // machines looks like an unconditional startup crash.
    if !is_valid_proxy_url(&proxy_url) {
        append_crash_log(&format!(
            "[{}] PROXY_URL_INVALID rejecting persisted proxy_url={:?}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            proxy_url
        ));
        eprintln!("[proxy] ignoring invalid persisted proxy_url: {proxy_url:?}");
        return;
    }

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

/// Minimal but strict proxy URL validation: must parse, must have an http/https/
/// socks scheme, must have a non-empty host. We avoid pulling in the `url` crate
/// here to keep main.rs's startup path dependency-free.
#[cfg(target_os = "windows")]
fn is_valid_proxy_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Disallow shell-injection / argument-splitting characters that would
    // corrupt the --proxy-server= command-line flag.
    if trimmed.chars().any(|c| c == ' ' || c == '"' || c == '\'' || c.is_control()) {
        return false;
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return false;
    };
    let scheme_ok = matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "socks4" | "socks5" | "socks5h"
    );
    if !scheme_ok {
        return false;
    }
    // After scheme://, the next path/query/fragment separator marks the end
    // of the authority. The authority itself must contain a non-empty host
    // (i.e. something before a possible :port).
    let authority = rest
        .split(|c: char| c == '/' || c == '?' || c == '#')
        .next()
        .unwrap_or("");
    let host = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
    let host_only = host.split(':').next().unwrap_or("");
    !host_only.is_empty()
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
