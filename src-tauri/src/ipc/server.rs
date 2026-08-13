//! Local socket server: accepts connections, reads newline-delimited JSON-RPC
//! requests, and dispatches them to the service layer. Thread-per-connection;
//! request handling within a connection is sequential, so a client that wants
//! concurrent calls opens multiple connections (the app-wide engine gate still
//! serializes actual inference).

use std::io::{BufRead, BufReader, Read, Write};
use std::thread;

use interprocess::local_socket::{Listener, Stream};
use log::{debug, warn};
use serde_json::Value;
use tauri::AppHandle;

use super::protocol::{self, ErrorKind, Request, RpcError};
use super::service;

/// Hard cap on a single request line. Requests carry paths and small options,
/// not audio payloads, so this is generous.
const MAX_LINE_BYTES: u64 = 256 * 1024;

pub fn run(app: AppHandle, listener: Listener) {
    use interprocess::local_socket::traits::ListenerExt as _;
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let app = app.clone();
                thread::Builder::new()
                    .name("ipc-conn".into())
                    .spawn(move || handle_connection(app, stream))
                    .ok();
            }
            Err(e) => {
                warn!("IPC accept failed: {e}");
            }
        }
    }
}

fn handle_connection(app: AppHandle, stream: Stream) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let n = match (&mut reader).take(MAX_LINE_BYTES).read_line(&mut line) {
            Ok(0) => break, // client disconnected
            Ok(n) => n,
            Err(e) => {
                debug!("IPC read error: {e}");
                break;
            }
        };
        // A line that fills the cap without a terminating newline is either a
        // runaway payload or garbage; there is no way to resync, so drop the
        // connection after telling the client why.
        if n as u64 >= MAX_LINE_BYTES && !line.ends_with('\n') {
            let err = RpcError::new(ErrorKind::InvalidRequest, "request line too long");
            write_line(&mut reader, &protocol::error_response(&Value::Null, &err));
            break;
        }
        if line.trim().is_empty() {
            continue;
        }

        if let Some(response) = handle_line(&app, &line) {
            if !write_line(&mut reader, &response) {
                break;
            }
        }
    }
}

/// Parse and dispatch one request line. Returns `None` for notifications
/// (requests without an id), which receive no response per JSON-RPC 2.0.
fn handle_line(app: &AppHandle, line: &str) -> Option<String> {
    let raw: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            let err = RpcError::new(ErrorKind::ParseError, format!("invalid JSON: {e}"));
            return Some(protocol::error_response(&Value::Null, &err));
        }
    };
    // Preserve the id (if any) for error responses even when the request
    // doesn't match the expected shape.
    let id = raw.get("id").cloned();
    let req: Request = match serde_json::from_value(raw) {
        Ok(r) => r,
        Err(e) => {
            let err = RpcError::new(ErrorKind::InvalidRequest, format!("invalid request: {e}"));
            return Some(protocol::error_response(
                id.as_ref().unwrap_or(&Value::Null),
                &err,
            ));
        }
    };

    let result = dispatch(app, &req);
    let id = req.id?; // notification: execute but do not respond
    Some(match result {
        Ok(value) => protocol::success_response(&id, value),
        Err(err) => protocol::error_response(&id, &err),
    })
}

fn dispatch(app: &AppHandle, req: &Request) -> Result<Value, RpcError> {
    match req.method.as_str() {
        "initialize" => service::initialize(app, req.params.as_ref()),
        "status" => Ok(service::status(app)),
        "models.list" => service::models_list(app),
        "transcribe.file" => service::transcribe_file(app, req.params.as_ref()),
        other => Err(RpcError::new(
            ErrorKind::MethodNotFound,
            format!("unknown method '{other}'"),
        )),
    }
}

fn write_line(reader: &mut BufReader<Stream>, response: &str) -> bool {
    let stream = reader.get_mut();
    if let Err(e) = stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
    {
        debug!("IPC write error: {e}");
        return false;
    }
    true
}
