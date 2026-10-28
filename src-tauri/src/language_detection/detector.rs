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
    /// An unload is pending: it found a run holding the model, so that run
    /// drops it once it lets go of the lock.
    unload_requested: AtomicBool,
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
        let result = self.run(path, speech, allowed);
        // Checked only after the lock is released: an unload that found it
        // held posted its request first, so it can't be missed here.
        if self.unload_requested.load(Ordering::SeqCst) {
            self.unload();
        }
        result
    }

    fn run(&self, path: &Path, speech: &[f32], allowed: &[String]) -> Result<Identified> {
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
        )?;
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
        // Posted before trying the lock, so a run that releases it in between
        // still sees the request.
        self.unload_requested.store(true, Ordering::SeqCst);
        let mut loaded = match self.loaded.try_lock() {
            Ok(loaded) => loaded,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        self.unload_requested.store(false, Ordering::SeqCst);
        if loaded.take().is_some() {
            debug!("Language detector unloaded");
        }
    }
}
