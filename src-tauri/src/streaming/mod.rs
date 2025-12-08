//! Streaming transcription module for eager output while recording.
//!
//! This module provides real-time transcription output during recording,
//! triggering intermediate results at natural pause points (speech pauses).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      StreamingController                            │
//! │                                                                     │
//! │  ┌───────────────┐     ┌─────────────────┐     ┌───────────────┐   │
//! │  │ PauseDetector │────▶│ Chunked Transcr │────▶│ TextReplacer  │   │
//! │  │ (VAD-based)   │     │ (batch windows) │     │ (backspace)   │   │
//! │  └───────────────┘     └─────────────────┘     └───────────────┘   │
//! │         ▲                                              │           │
//! │         │                                              ▼           │
//! │  ┌──────────────────────────────────────────────────────────────┐ │
//! │  │                    Audio Buffer                              │ │
//! │  │  [accumulated samples since recording start]                 │ │
//! │  └──────────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```

mod controller;
mod manager;
mod pause_detector;
mod text_replacer;

pub use controller::{StreamingConfig, StreamingController, StreamingEvent, StreamingState};
pub use manager::StreamingManager;
pub use pause_detector::PauseDetector;
pub use text_replacer::TextReplacer;
