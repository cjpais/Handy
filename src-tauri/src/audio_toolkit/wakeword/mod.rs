mod detector;

pub use detector::{WakeWordConfig, WakeWordDetector, WAKE_CHUNK_SAMPLES};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub type WakeDetectCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Shared handle between the recorder's consumer thread (which feeds idle
/// frames) and the wake-word manager (which enables/disables detection and
/// swaps models when settings change).
#[derive(Default)]
pub struct WakeWordRuntime {
    enabled: AtomicBool,
    detector: Mutex<Option<WakeWordDetector>>,
    on_detect: Mutex<Option<WakeDetectCallback>>,
}

impl WakeWordRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Feed one idle-time audio frame (16 kHz mono f32). Invokes the detect
    /// callback when the wake word fires. Uses `try_lock` so a concurrent
    /// model swap can never stall the audio capture thread — frames during a
    /// swap are simply skipped.
    pub fn push_frame(&self, samples: &[f32]) {
        if !self.is_enabled() {
            return;
        }
        let Ok(mut guard) = self.detector.try_lock() else {
            return;
        };
        let Some(detector) = guard.as_mut() else {
            return;
        };
        match detector.push_frame(samples) {
            Ok(true) => {
                log::info!("Wake word detected");
                let cb = self.on_detect.lock().unwrap().clone();
                if let Some(cb) = cb {
                    cb();
                }
            }
            Ok(false) => {}
            Err(e) => {
                // Disable rather than spam inference errors 33x/second.
                log::error!("Wake-word inference failed, disabling detection: {e:#}");
                self.enabled.store(false, Ordering::Relaxed);
            }
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if let Ok(mut guard) = self.detector.lock() {
            if let Some(det) = guard.as_mut() {
                det.reset();
            }
        }
    }

    pub fn set_detector(&self, detector: Option<WakeWordDetector>) {
        *self.detector.lock().unwrap() = detector;
    }

    pub fn set_threshold(&self, threshold: f32) {
        if let Some(det) = self.detector.lock().unwrap().as_mut() {
            det.set_threshold(threshold);
        }
    }

    pub fn set_on_detect(&self, cb: WakeDetectCallback) {
        *self.on_detect.lock().unwrap() = Some(cb);
    }

    /// Clear detector context, e.g. when a recording session ends and idle
    /// listening resumes (stale pre-recording audio must not leak into the
    /// next score window).
    pub fn reset_context(&self) {
        if let Ok(mut guard) = self.detector.try_lock() {
            if let Some(det) = guard.as_mut() {
                det.reset();
            }
        }
    }
}
