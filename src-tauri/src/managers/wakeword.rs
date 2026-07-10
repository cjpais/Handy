use crate::audio_toolkit::wakeword::{WakeWordConfig, WakeWordDetector, WakeWordRuntime};
use crate::settings::get_settings;
use anyhow::{anyhow, Result};
use log::{error, info};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// The `hotkey_string`/source tag identifying wake-word-initiated triggers
/// throughout the pipeline (coordinator stage tracking, action VAD policy).
pub const WAKE_SOURCE: &str = "wakeword";

const MELSPEC_RESOURCE: &str = "resources/models/wakeword/melspectrogram.onnx";
const EMBEDDING_RESOURCE: &str = "resources/models/wakeword/embedding_model.onnx";

/// Owns the [`WakeWordRuntime`] shared with the audio recorder and translates
/// settings changes into detector state (enable/disable, model swaps,
/// threshold updates).
pub struct WakeWordManager {
    app: AppHandle,
    runtime: Arc<WakeWordRuntime>,
}

impl WakeWordManager {
    pub fn new(app: AppHandle) -> Self {
        let runtime = Arc::new(WakeWordRuntime::new());
        runtime.set_on_detect({
            let app = app.clone();
            Arc::new(move || {
                // Same path as the CLI/signal triggers: toggle semantics, and
                // the coordinator ignores it while recording/processing. Only
                // pushes onto the coordinator channel, so it is safe to call
                // from the recorder's consumer thread.
                crate::signal_handle::send_transcription_input(&app, "transcribe", WAKE_SOURCE);
            })
        });
        Self { app, runtime }
    }

    pub fn runtime(&self) -> Arc<WakeWordRuntime> {
        Arc::clone(&self.runtime)
    }

    /// Re-read wake-word settings and (re)build the detector accordingly.
    /// Model loading happens on a spawned thread so settings commands return
    /// immediately; detection stays disabled until the load finishes.
    pub fn apply_settings(&self) {
        let settings = get_settings(&self.app);
        if !settings.wake_word_enabled {
            self.runtime.set_enabled(false);
            self.runtime.set_detector(None);
            info!("Wake word disabled");
            return;
        }

        let app = self.app.clone();
        let runtime = Arc::clone(&self.runtime);
        std::thread::spawn(move || {
            let settings = get_settings(&app);
            match build_detector(&app, &settings) {
                Ok(detector) => {
                    runtime.set_detector(Some(detector));
                    runtime.set_enabled(true);
                    info!(
                        "Wake word enabled: model={:?} threshold={}",
                        settings.wake_word_model, settings.wake_word_threshold
                    );
                }
                Err(e) => {
                    runtime.set_enabled(false);
                    runtime.set_detector(None);
                    error!("Failed to load wake-word model: {e:#}");
                }
            }
        });
    }

    /// Cheap runtime update that doesn't reload the ONNX sessions.
    pub fn apply_threshold(&self, threshold: f32) {
        self.runtime.set_threshold(threshold);
    }

    /// Validate that a custom head loads as part of the openWakeWord pipeline
    /// (used by the settings command before persisting a custom model path).
    pub fn validate_custom_model(&self, head_path: &str) -> Result<()> {
        let (melspec, embedding) = resolve_shared_model_paths(&self.app)?;
        WakeWordDetector::new(
            &melspec,
            &embedding,
            PathBuf::from(head_path).as_path(),
            WakeWordConfig::default(),
        )
        .map(|_| ())
    }
}

fn resolve_shared_model_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf)> {
    let resolve = |resource: &str| -> Result<PathBuf> {
        app.path()
            .resolve(resource, tauri::path::BaseDirectory::Resource)
            .map_err(|e| anyhow!("failed to resolve bundled resource {resource}: {e}"))
    };
    Ok((resolve(MELSPEC_RESOURCE)?, resolve(EMBEDDING_RESOURCE)?))
}

fn build_detector(
    app: &AppHandle,
    settings: &crate::settings::AppSettings,
) -> Result<WakeWordDetector> {
    let (melspec, embedding) = resolve_shared_model_paths(app)?;

    let head: PathBuf = match settings.wake_word_model.bundled_head_path() {
        Some(resource) => app
            .path()
            .resolve(resource, tauri::path::BaseDirectory::Resource)
            .map_err(|e| anyhow!("failed to resolve bundled resource {resource}: {e}"))?,
        None => {
            let path = settings
                .wake_word_custom_model_path
                .as_deref()
                .ok_or_else(|| anyhow!("wake word set to Custom but no model path configured"))?;
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(anyhow!(
                    "custom wake-word model not found at {}",
                    path.display()
                ));
            }
            path
        }
    };

    let config = WakeWordConfig {
        threshold: settings.wake_word_threshold,
        ..WakeWordConfig::default()
    };
    WakeWordDetector::new(&melspec, &embedding, &head, config)
}

/// `AlwaysOn` when either the user asked for it or the wake word needs the
/// stream open to listen; the mic-mode consumers use this instead of reading
/// `always_on_microphone` directly.
pub fn effective_microphone_mode(
    settings: &crate::settings::AppSettings,
) -> crate::managers::audio::MicrophoneMode {
    if settings.always_on_microphone || settings.wake_word_enabled {
        crate::managers::audio::MicrophoneMode::AlwaysOn
    } else {
        crate::managers::audio::MicrophoneMode::OnDemand
    }
}
