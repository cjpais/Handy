use serde::Serialize;
use std::fmt::Debug;

/// Sink for core → UI events. Desktop wires this to `tauri::AppHandle::emit`,
/// mobile wires it to a JNI callback or Tauri Mobile emit.
pub trait EventSink: Send + Sync + 'static {
    fn emit_json(&self, event_name: &str, payload: serde_json::Value);
}

pub fn emit<T: Serialize + Debug>(sink: &dyn EventSink, event_name: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(v) => sink.emit_json(event_name, v),
        Err(e) => log::warn!("EventSink emit serialize failed for {event_name}: {e}"),
    }
}
