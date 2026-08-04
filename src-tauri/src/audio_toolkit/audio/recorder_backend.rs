//! `Recorder` — the thin seam that lets `AudioRecordingManager` drive either the
//! native PipeWire backend (Linux) or the cpal/ALSA backend (everywhere) through
//! one type with the SAME method surface (`open`/`start`/`stop`/`close`).
//!
//! Selection strategy (Linux): try PipeWire at `open()`; if its
//! connection/stream setup fails (no session, etc.), fall back to the existing
//! cpal path and log it. Both backends are built up front from the SAME shared
//! VAD + callbacks (VAD lives behind `Arc<Mutex<..>>`, so only one ONNX session
//! exists), but only the selected one is ever opened at a time.
//!
//! Non-Linux builds compile the cpal backend ONLY — no PipeWire code, no
//! behavioural change. cpal remains fully compiled and working on Linux too.

use super::recorder::{AudioFrameCallback, LevelCallback, VadConfig};
use super::{AudioRecorder, VadPolicy};

#[cfg(target_os = "linux")]
use super::pipewire_recorder::PipeWireRecorder;

/// Which backend is currently open (Linux only — non-Linux is always cpal).
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// Nothing open yet, or closed.
    None,
    PipeWire,
    Cpal,
}

pub struct Recorder {
    /// Always present: the fallback and the only backend off Linux.
    cpal: AudioRecorder,
    #[cfg(target_os = "linux")]
    pipewire: PipeWireRecorder,
    #[cfg(target_os = "linux")]
    active: Backend,
}

impl Recorder {
    /// Build both backends from shared parts. See `AudioRecorder::from_parts`.
    pub(crate) fn from_parts(
        vad: Option<VadConfig>,
        level_cb: Option<LevelCallback>,
        audio_cb: Option<AudioFrameCallback>,
    ) -> Self {
        #[cfg(target_os = "linux")]
        {
            // Share one VAD engine + callbacks across both backends (all cheap
            // to clone: VAD is Arc<Mutex<..>>, callbacks are Arc). Only one
            // backend is opened at a time, so they never run concurrently.
            let cpal =
                AudioRecorder::from_parts(vad.clone(), level_cb.clone(), audio_cb.clone());
            let pipewire = PipeWireRecorder::from_parts(vad, level_cb, audio_cb);
            Recorder {
                cpal,
                pipewire,
                active: Backend::None,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Recorder {
                cpal: AudioRecorder::from_parts(vad, level_cb, audio_cb),
            }
        }
    }

    /// Open the microphone. On Linux, prefer native PipeWire and fall back to
    /// cpal/ALSA if PipeWire setup fails. The cpal-resolved `device` is used for
    /// the cpal path; the PipeWire path currently ignores it and captures the
    /// default source (see TODO).
    pub fn open(
        &mut self,
        device: Option<cpal::Device>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        {
            // TODO(pipewire device selection): translate a user-selected mic to
            // a PipeWire `node.name` and pass it here instead of `None`. For the
            // MVP we capture the default source, which reproduces today's cpal
            // "default" behaviour. PipeWire-native device enumeration/selection
            // (the de-cpal refactor) would slot in at this call site and in
            // `device.rs`/`managers/audio.rs`.
            match self.pipewire.open(None) {
                Ok(()) => {
                    self.active = Backend::PipeWire;
                    log::info!("Microphone capture using native PipeWire backend");
                    return Ok(());
                }
                Err(e) => {
                    log::warn!(
                        "PipeWire capture unavailable ({e}); falling back to cpal/ALSA backend"
                    );
                }
            }
            self.cpal.open(device)?;
            self.active = Backend::Cpal;
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.cpal.open(device)
        }
    }

    pub fn start(&self, vad_policy: VadPolicy) -> Result<(), Box<dyn std::error::Error>> {
        // Expression-block-per-cfg idiom (mirrors `get_cpal_host`): exactly one
        // block survives cfg-stripping and becomes the tail expression.
        #[cfg(target_os = "linux")]
        {
            match self.active {
                Backend::PipeWire => self.pipewire.start(vad_policy),
                _ => self.cpal.start(vad_policy),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.cpal.start(vad_policy)
        }
    }

    pub fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        {
            match self.active {
                Backend::PipeWire => self.pipewire.stop(),
                _ => self.cpal.stop(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.cpal.stop()
        }
    }

    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        {
            let result = match self.active {
                Backend::PipeWire => self.pipewire.close(),
                _ => self.cpal.close(),
            };
            self.active = Backend::None;
            result
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.cpal.close()
        }
    }
}
