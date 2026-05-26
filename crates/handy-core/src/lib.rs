//! handy-core: platform-agnostic core for the Handy speech-to-text app.
//!
//! This crate intentionally does NOT depend on `tauri`, `cpal`, `enigo`, `rdev`,
//! `gtk-*`, or any GUI/platform-specific library. Platform capabilities are
//! injected through traits defined in the `handy-platform` crate.

pub mod audio;
pub mod text;
pub mod vad;

pub use text::{apply_custom_words, filter_transcription_output};
pub use vad::{SileroVad, VoiceActivityDetector};

#[cfg(test)]
mod sanity_check {
    #[test]
    fn it_links() {
        assert_eq!(2 + 2, 4);
    }
}
