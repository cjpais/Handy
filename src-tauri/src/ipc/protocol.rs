//! JSON-RPC 2.0 message types and the stable error taxonomy for the local IPC
//! API. Framing is newline-delimited JSON (one message per line, UTF-8). See
//! docs/ipc.md for the full protocol contract.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol version spoken by this server. Bump only on breaking changes;
/// additive fields/methods do not require a bump.
pub const PROTOCOL_VERSION: u64 = 1;

/// An incoming JSON-RPC request. `id` is absent for notifications, which per
/// spec receive no response.
#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Stable, machine-readable failure kinds. The numeric code and the kebab-case
/// kind string are both part of the public contract; messages are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    // JSON-RPC standard codes.
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    // Handy application codes (1000+).
    Busy,
    SetupRequired,
    ModelNotInstalled,
    ModelLoadFailed,
    UnsupportedAudio,
    FileNotFound,
    Internal,
    ProtocolMismatch,
}

impl ErrorKind {
    pub fn code(self) -> i64 {
        match self {
            ErrorKind::ParseError => -32700,
            ErrorKind::InvalidRequest => -32600,
            ErrorKind::MethodNotFound => -32601,
            ErrorKind::InvalidParams => -32602,
            ErrorKind::Busy => 1000,
            ErrorKind::SetupRequired => 1001,
            ErrorKind::ModelNotInstalled => 1002,
            ErrorKind::ModelLoadFailed => 1003,
            ErrorKind::UnsupportedAudio => 1004,
            ErrorKind::FileNotFound => 1005,
            ErrorKind::Internal => 1006,
            ErrorKind::ProtocolMismatch => 1007,
        }
    }

    pub fn kind_str(self) -> &'static str {
        match self {
            ErrorKind::ParseError => "parse-error",
            ErrorKind::InvalidRequest => "invalid-request",
            ErrorKind::MethodNotFound => "method-not-found",
            ErrorKind::InvalidParams => "invalid-params",
            ErrorKind::Busy => "busy",
            ErrorKind::SetupRequired => "setup-required",
            ErrorKind::ModelNotInstalled => "model-not-installed",
            ErrorKind::ModelLoadFailed => "model-load-failed",
            ErrorKind::UnsupportedAudio => "unsupported-audio",
            ErrorKind::FileNotFound => "file-not-found",
            ErrorKind::Internal => "internal-error",
            ErrorKind::ProtocolMismatch => "protocol-mismatch",
        }
    }
}

/// A service-level failure carried back to the client as a JSON-RPC error
/// object with `data.kind` set to the stable kind string.
#[derive(Debug)]
pub struct RpcError {
    pub kind: ErrorKind,
    pub message: String,
}

impl RpcError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: i64,
    message: &'a str,
    data: ErrorData<'a>,
}

#[derive(Serialize)]
struct ErrorData<'a> {
    kind: &'a str,
}

/// Serialize a success response for `id`.
pub fn success_response(id: &Value, result: Value) -> String {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// Serialize an error response for `id` (JSON `null` id for unparseable
/// requests, per spec).
pub fn error_response(id: &Value, error: &RpcError) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": ErrorBody {
            code: error.kind.code(),
            message: &error.message,
            data: ErrorData { kind: error.kind.kind_str() },
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_with_id_and_params() {
        let req: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":7,"method":"status","params":{}}"#)
                .unwrap();
        assert_eq!(req.method, "status");
        assert_eq!(req.id, Some(Value::from(7)));
        assert!(req.params.is_some());
    }

    #[test]
    fn parses_notification_without_id() {
        let req: Request = serde_json::from_str(r#"{"jsonrpc":"2.0","method":"status"}"#).unwrap();
        assert!(req.id.is_none());
        assert!(req.params.is_none());
    }

    #[test]
    fn error_response_carries_stable_kind() {
        let err = RpcError::new(ErrorKind::Busy, "another transcription is in progress");
        let raw = error_response(&Value::from(3), &err);
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["error"]["code"], 1000);
        assert_eq!(v["error"]["data"]["kind"], "busy");
        assert_eq!(v["id"], 3);
    }

    #[test]
    fn success_response_round_trips() {
        let raw = success_response(&Value::from("abc"), serde_json::json!({ "text": "hi" }));
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["result"]["text"], "hi");
        assert_eq!(v["id"], "abc");
        assert_eq!(v["jsonrpc"], "2.0");
    }
}
