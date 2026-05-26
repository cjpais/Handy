//! handy-platform: trait definitions for platform-specific capabilities.
//!
//! Concrete implementations live in:
//! - `src-tauri/` (desktop: cpal, enigo, OS clipboard)
//! - `src-mobile/` + `crates/handy-mobile/` (Android: Oboe, IME, JNI)

pub mod audio;
pub mod event_sink;
pub mod storage;
pub mod text_output;

pub use audio::{AudioCapture, AudioConfig, AudioFrame};
pub use event_sink::EventSink;
pub use storage::AppStorage;
pub use text_output::{OutputMode, TextOutput};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    struct CapturingSink(Mutex<Vec<(String, serde_json::Value)>>);

    impl EventSink for CapturingSink {
        fn emit_json(&self, event_name: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().push((event_name.to_string(), payload));
        }
    }

    #[test]
    fn event_sink_round_trip() {
        let sink = CapturingSink(Mutex::new(Vec::new()));
        event_sink::emit(&sink, "test/event", &json!({"k": 1}));
        let captured = sink.0.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "test/event");
        assert_eq!(captured[0].1, json!({"k": 1}));
    }
}
