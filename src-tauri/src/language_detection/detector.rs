//! The spoken-language detector model, loaded in Handy's process on CPU (like
//! Silero VAD) when first needed and unloaded with the transcription model.

use anyhow::Result;
use log::{debug, info};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, TryLockError};
use std::time::Instant;

use transcribe_cpp::{Backend, LangIdOptions, LangIdSession, Model, ModelOptions};

use super::Identified;
use crate::audio_toolkit::cpu_engine::init_cpu_backends;

#[derive(Default)]
pub struct LanguageDetector {
    // `LangIdSession` is Send but not Sync.
    loaded: Mutex<Option<Loaded>>,
    /// An unload arrived mid-run; the run drops the model when it ends.
    unload_after_run: AtomicBool,
}

struct Loaded {
    path: PathBuf,
    session: LangIdSession,
}

impl Loaded {
    fn open(path: &Path) -> Result<Self> {
        init_cpu_backends()?;
        let started = Instant::now();
        let model = Model::load_with(
            path,
            &ModelOptions {
                backend: Backend::Cpu,
                device: None,
            },
        )
        .map_err(|e| anyhow::anyhow!("Failed to load the language detector: {e}"))?;
        // The session keeps the native model alive on its own.
        let session = model
            .langid_session()
            .map_err(|e| anyhow::anyhow!("Model is not a language detector: {e}"))?;
        info!(
            "Loaded the language detector {} in {} ms",
            path.file_name().unwrap_or_default().to_string_lossy(),
            started.elapsed().as_millis()
        );
        Ok(Self {
            path: path.to_path_buf(),
            session,
        })
    }
}

impl LanguageDetector {
    /// Rank `allowed` (the detector's labels) for 16 kHz mono `speech`,
    /// loading the model at `path` first if it isn't loaded.
    pub fn identify(&self, path: &Path, speech: &[f32], allowed: &[String]) -> Result<Identified> {
        let mut guard = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|l| l.path != path) {
            *guard = None;
        }
        let loaded = match &mut *guard {
            Some(loaded) => loaded,
            slot @ None => slot.insert(Loaded::open(path)?),
        };
        let started = Instant::now();
        let result = loaded.session.run(
            speech,
            &LangIdOptions {
                allowed: Some(allowed.to_vec()),
            },
        );
        if self.unload_after_run.swap(false, Ordering::AcqRel) {
            *guard = None;
            debug!("Language detector unloaded");
        }
        let result = result?;
        Ok(Identified {
            candidates: result
                .candidates
                .into_iter()
                .map(|c| (c.code, c.p))
                .collect(),
            allowed_mass: result.allowed_mass,
            elapsed: started.elapsed(),
        })
    }

    /// Never waits: an unload that lands mid-run is done by the run when it
    /// ends, so a model unload never blocks its caller (e.g. the event loop).
    pub fn unload(&self) {
        let mut loaded = match self.loaded.try_lock() {
            Ok(loaded) => loaded,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                self.unload_after_run.store(true, Ordering::Release);
                return;
            }
        };
        if loaded.take().is_some() {
            debug!("Language detector unloaded");
        }
    }
}
