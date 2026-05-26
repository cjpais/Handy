//! Re-export shim. Real implementation lives in `handy_core::text`.
//! Keep this file so we have a place to add desktop-only middleware later.
pub use handy_core::text::{apply_custom_words, filter_transcription_output};
