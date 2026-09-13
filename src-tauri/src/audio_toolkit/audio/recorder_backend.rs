//! `Recorder` — the thin seam that lets `AudioRecordingManager` drive either the
//! native PipeWire backend (Linux) or the cpal/ALSA backend (everywhere) through
//! one type with the SAME method surface (`open`/`start`/`stop`/`close`).
//!
//! Selection strategy (Linux) is driven by the caller's `CaptureBackend`
//! preference, resolved once by `resolve_capture_backend`:
//!   * `None` ("Auto"): prefer PipeWire; if its connection/stream setup fails
//!     (no session, etc.), fall back to the cpal/ALSA path and log it.
//!   * `Some(PipeWire)`: pin PipeWire. A setup failure is a HARD error — never
//!     a silent fallback, so the app always knows which backend is active.
//!   * `Some(Cpal)`: pin cpal/ALSA and never touch PipeWire.
//!
//! Both backends are built up front from the SAME shared VAD + callbacks (VAD
//! lives behind `Arc<Mutex<..>>`, so only one ONNX session exists), but only the
//! selected one is ever opened at a time.
//!
//! Non-Linux builds compile the cpal backend ONLY — no PipeWire code, no
//! behavioural change. cpal remains fully compiled and working on Linux too.

use std::sync::mpsc;

use super::recorder::{AudioFrameCallback, LevelCallback, VadConfig};
use super::{AudioRecorder, VadPolicy};

#[cfg(target_os = "linux")]
use super::pipewire_recorder::PipeWireRecorder;

/// A concrete capture backend. Deliberately has no "auto" variant: this is the
/// RESOLVED backend, so every consumer (device enumeration in particular) reads
/// what is actually in use rather than re-deriving the preference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureBackend {
    /// Native PipeWire capture (Linux only).
    PipeWire,
    /// cpal — the ALSA host on Linux, the platform default elsewhere.
    Cpal,
}

impl CaptureBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureBackend::PipeWire => "pipewire",
            CaptureBackend::Cpal => "alsa",
        }
    }
}

/// Resolve a backend preference to the backend that will actually be used.
///
/// `preferred == None` means "Auto": prefer PipeWire when a session is
/// reachable, else cpal/ALSA. An explicit preference is returned unchanged —
/// including `PipeWire` on a host without it, so `open` can fail loudly instead
/// of reporting a backend it is not going to use.
pub fn resolve_capture_backend(preferred: Option<CaptureBackend>) -> CaptureBackend {
    match preferred {
        Some(backend) => backend,
        #[cfg(target_os = "linux")]
        None => {
            if super::pipewire_recorder::check_pipewire_available() {
                CaptureBackend::PipeWire
            } else {
                CaptureBackend::Cpal
            }
        }
        #[cfg(not(target_os = "linux"))]
        None => CaptureBackend::Cpal,
    }
}

pub struct Recorder {
    /// Always present: the fallback and the only backend off Linux.
    cpal: AudioRecorder,
    #[cfg(target_os = "linux")]
    pipewire: PipeWireRecorder,
    /// Which backend is currently open, `None` when nothing is.
    active: Option<CaptureBackend>,
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
            let cpal = AudioRecorder::from_parts(vad.clone(), level_cb.clone(), audio_cb.clone());
            let pipewire = PipeWireRecorder::from_parts(vad, level_cb, audio_cb);
            Recorder {
                cpal,
                pipewire,
                active: None,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Recorder {
                cpal: AudioRecorder::from_parts(vad, level_cb, audio_cb),
                active: None,
            }
        }
    }

    /// Open the microphone on the requested backend.
    ///
    /// `backend` is the already-resolved choice (see `resolve_capture_backend`)
    /// — the seam does not re-derive it, so the backend the app reports is the
    /// backend it opens. `device` is the cpal-resolved device for the cpal path;
    /// the PipeWire path captures the graph's default source.
    ///
    /// A PipeWire failure falls back to cpal only when the caller allows it
    /// (`allow_fallback`, i.e. the "Auto" preference). With the backend pinned
    /// to PipeWire the error propagates, so the user sees that their explicit
    /// choice is unavailable instead of silently recording through ALSA.
    pub fn open(
        &mut self,
        backend: CaptureBackend,
        device: Option<cpal::Device>,
        allow_fallback: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        {
            if backend == CaptureBackend::PipeWire {
                match self.pipewire.open(None) {
                    Ok(()) => {
                        self.active = Some(CaptureBackend::PipeWire);
                        log::info!("Microphone capture using native PipeWire backend");
                        return Ok(());
                    }
                    Err(e) if allow_fallback => {
                        log::warn!(
                            "PipeWire capture unavailable ({e}); falling back to cpal/ALSA backend"
                        );
                    }
                    Err(e) => {
                        // Pinned to PipeWire: surface the failure instead of
                        // quietly recording through a backend the user did not pick.
                        self.active = None;
                        return Err(e);
                    }
                }
            }
            self.cpal.open(device)?;
            self.active = Some(CaptureBackend::Cpal);
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            // cpal is the only backend off Linux; the PipeWire-specific inputs
            // are inert there.
            let _ = (backend, allow_fallback);
            self.cpal.open(device)?;
            self.active = Some(CaptureBackend::Cpal);
            Ok(())
        }
    }

    /// The backend currently open, or `None` when the microphone is closed.
    pub fn active_backend(&self) -> Option<CaptureBackend> {
        self.active
    }

    /// Begin capturing. Returns the one-shot receiver that fires after the first
    /// microphone chunk is processed (the shared `run_consumer` sends it), so the
    /// manager can build its `RecordingReadiness` uniformly across backends.
    pub fn start(
        &self,
        vad_policy: VadPolicy,
    ) -> Result<mpsc::Receiver<()>, Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        if self.active == Some(CaptureBackend::PipeWire) {
            return self.pipewire.start(vad_policy);
        }
        self.cpal.start(vad_policy)
    }

    pub fn stop(&self) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        if self.active == Some(CaptureBackend::PipeWire) {
            return self.pipewire.stop();
        }
        self.cpal.stop()
    }

    pub fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(target_os = "linux")]
        if self.active == Some(CaptureBackend::PipeWire) {
            self.active = None;
            return self.pipewire.close();
        }
        self.active = None;
        self.cpal.close()
    }

    /// Pin capture to a single input channel. Applied to the cpal backend — the
    /// only one that selects a channel. The PipeWire backend targets a whole
    /// source node and downmixes all of its channels to mono.
    pub fn set_selected_channel(&mut self, channel: Option<u16>) {
        self.cpal.set_selected_channel(channel);
    }

    /// Whether the open stream has died and must be rebuilt before the next
    /// recording. Delegates to the active backend; both mirror the same
    /// stream-error flag semantics.
    pub fn needs_reopen(&self) -> bool {
        #[cfg(target_os = "linux")]
        if self.active == Some(CaptureBackend::PipeWire) {
            return self.pipewire.needs_reopen();
        }
        self.cpal.needs_reopen()
    }
}
