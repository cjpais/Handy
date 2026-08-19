//! Reference client for Handy's local IPC API (see docs/ipc.md).
//!
//! Run while Handy is running:
//!
//! ```sh
//! cargo run --example ipc_client -- status
//! cargo run --example ipc_client -- models
//! cargo run --example ipc_client -- transcribe /absolute/path/to/audio.wav
//! ```
//!
//! This is deliberately small and dependency-light, because that is the point:
//! integrating against Handy is "connect to a local socket, write a JSON line,
//! read a JSON line". Any language can do this — see docs/ipc.md for a Python
//! version of the same client.
//!
//! Set `HANDY_SOCK` to override the socket path (e.g. for portable installs).

use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::Stream;
use serde_json::{json, Value};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (method, params) = match args.first().map(String::as_str) {
        Some("status") => ("status", json!({})),
        Some("models") => ("models.list", json!({})),
        Some("transcribe") => match args.get(1) {
            Some(path) => ("transcribe.file", json!({ "path": path })),
            None => return usage(),
        },
        _ => return usage(),
    };

    let mut conn = match connect() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    // Optional version handshake: fails fast (protocol-mismatch) if this
    // client and the running Handy ever diverge on a breaking change.
    let init = json!({ "protocol_version": 1, "client": "handy-ipc-client-example" });
    if let Err(e) = call(&mut conn, 1, "initialize", init) {
        return report(e);
    }

    match call(&mut conn, 2, method, params) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            0
        }
        Err(e) => report(e),
    }
}

fn usage() -> i32 {
    eprintln!("usage: ipc_client <status | models | transcribe /absolute/path.wav>");
    2
}

enum CallError {
    /// Server answered with a JSON-RPC error: (stable kind, human message).
    Rpc(String, String),
    /// Connection-level failure.
    Transport(String),
}

fn report(e: CallError) -> i32 {
    match e {
        CallError::Rpc(kind, message) => {
            eprintln!("error [{kind}]: {message}");
            // `kind` is the stable field to branch on — see the error table
            // in docs/ipc.md. `busy` is the one retryable case.
            if kind == "busy" {
                eprintln!("hint: Handy is dictating or transcribing; retry shortly.");
            }
        }
        CallError::Transport(message) => eprintln!("error: {message}"),
    }
    1
}

/// Send one request and read its response. Framing is one JSON object per
/// `\n`-terminated line in each direction.
fn call(
    conn: &mut BufReader<Stream>,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, CallError> {
    let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let stream = conn.get_mut();
    stream
        .write_all(request.to_string().as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|e| CallError::Transport(format!("write failed: {e}")))?;

    let mut line = String::new();
    conn.read_line(&mut line)
        .map_err(|e| CallError::Transport(format!("read failed: {e}")))?;
    let response: Value = serde_json::from_str(&line)
        .map_err(|e| CallError::Transport(format!("invalid response: {e}")))?;

    if let Some(error) = response.get("error") {
        let kind = error["data"]["kind"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let message = error["message"].as_str().unwrap_or("").to_string();
        return Err(CallError::Rpc(kind, message));
    }
    Ok(response["result"].clone())
}

#[cfg(unix)]
fn connect() -> Result<BufReader<Stream>, String> {
    use interprocess::local_socket::{traits::Stream as _, GenericFilePath, ToFsName};

    let path = socket_path();
    let name = path
        .clone()
        .to_fs_name::<GenericFilePath>()
        .map_err(|e| format!("bad socket path: {e}"))?;
    let stream = Stream::connect(name).map_err(|e| {
        format!(
            "cannot connect to {} ({e}) — is Handy running?",
            path.display()
        )
    })?;
    Ok(BufReader::new(stream))
}

#[cfg(windows)]
fn connect() -> Result<BufReader<Stream>, String> {
    use interprocess::local_socket::{traits::Stream as _, GenericNamespaced, ToNsName};

    let name = "com.pais.handy.ipc"
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| format!("bad pipe name: {e}"))?;
    let stream = Stream::connect(name).map_err(|e| {
        format!("cannot connect to \\\\.\\pipe\\com.pais.handy.ipc ({e}) — is Handy running?")
    })?;
    Ok(BufReader::new(stream))
}

/// Default per-platform socket path (Handy's app data dir); `HANDY_SOCK`
/// overrides it. Portable installs keep the socket in their Data directory.
#[cfg(unix)]
fn socket_path() -> std::path::PathBuf {
    use std::path::PathBuf;

    if let Ok(explicit) = std::env::var("HANDY_SOCK") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(home).join("Library/Application Support/com.pais.handy/handy.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join(".local/share"));
        data_home.join("com.pais.handy/handy.sock")
    }
}
