//! GPU execution provider Tauri commands.

use std::sync::Arc;

use log::info;
use tauri::{AppHandle, Manager};

use crate::managers::model::{EngineType, ModelManager};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{self, GpuProvider};

impl From<GpuProvider> for transcribe_rs::GpuProvider {
    fn from(p: GpuProvider) -> Self {
        match p {
            GpuProvider::Auto => transcribe_rs::GpuProvider::Auto,
            GpuProvider::CpuOnly => transcribe_rs::GpuProvider::CpuOnly,
            GpuProvider::DirectMl => transcribe_rs::GpuProvider::DirectMl,
            GpuProvider::Cuda => transcribe_rs::GpuProvider::Cuda,
            GpuProvider::CoreMl => transcribe_rs::GpuProvider::CoreMl,
            GpuProvider::WebGpu => transcribe_rs::GpuProvider::WebGpu,
        }
    }
}

impl From<transcribe_rs::GpuProvider> for GpuProvider {
    fn from(p: transcribe_rs::GpuProvider) -> Self {
        match p {
            transcribe_rs::GpuProvider::Auto => GpuProvider::Auto,
            transcribe_rs::GpuProvider::CpuOnly => GpuProvider::CpuOnly,
            transcribe_rs::GpuProvider::DirectMl => GpuProvider::DirectMl,
            transcribe_rs::GpuProvider::Cuda => GpuProvider::Cuda,
            transcribe_rs::GpuProvider::CoreMl => GpuProvider::CoreMl,
            transcribe_rs::GpuProvider::WebGpu => GpuProvider::WebGpu,
        }
    }
}

/// Return which GPU providers are available in this build.
#[tauri::command]
#[specta::specta]
pub fn get_available_gpu_providers() -> Vec<GpuProvider> {
    transcribe_rs::available_providers()
        .into_iter()
        .map(GpuProvider::from)
        .collect()
}

/// Returns true for engine types that use ORT (and thus respect the
/// GpuProvider setting).  Whisper uses whisper.cpp — reloading it on
/// provider change is a no-op waste of time.
fn is_ort_engine(engine_type: &EngineType) -> bool {
    matches!(
        engine_type,
        EngineType::Parakeet
            | EngineType::Moonshine
            | EngineType::MoonshineStreaming
            | EngineType::SenseVoice
    )
}

/// Apply the persisted GPU provider at startup.
///
/// If the persisted value isn't available in this build (e.g. "directml"
/// saved by a previous build that had DirectML compiled in), reset to Auto.
pub fn apply_startup_gpu_provider(app_handle: &AppHandle) {
    let mut startup_settings = settings::get_settings(app_handle);
    let available = get_available_gpu_providers();
    if !available.contains(&startup_settings.gpu_provider) {
        log::warn!(
            "Persisted GPU provider {:?} is not available in this build, resetting to Auto",
            startup_settings.gpu_provider
        );
        startup_settings.gpu_provider = GpuProvider::Auto;
        settings::write_settings(app_handle, startup_settings.clone());
    }
    let gpu_provider: transcribe_rs::GpuProvider = startup_settings.gpu_provider.into();
    transcribe_rs::set_gpu_provider(gpu_provider);
    info!("GPU provider set to: {:?}", gpu_provider);
}

/// Change the GPU provider setting, update the global, and reload models.
///
/// Validates feasibility before mutating any state: checks whether a model
/// reload would be blocked (busy transcription) before touching the global
/// atomic or persisted settings.
#[tauri::command]
#[specta::specta]
pub async fn change_gpu_provider_setting(
    app: AppHandle,
    provider: GpuProvider,
) -> Result<(), String> {
    let s = settings::get_settings(&app);
    if s.gpu_provider == provider {
        return Ok(());
    }
    let previous = s.gpu_provider;

    // --- Validation phase: no mutation yet ---

    // Check if a model reload would be needed and whether it's feasible
    let reload_model_id: Option<String> = if let Some(tm) =
        app.try_state::<Arc<TranscriptionManager>>()
    {
        if let Some(current_model_id) = tm.get_current_model() {
            // Reject if a transcription is currently in flight.
            // During transcription the engine is taken out of the mutex,
            // so is_model_loaded() returns false even though current_model_id is set.
            if !tm.is_model_loaded() {
                return Err(
                    "Cannot change GPU provider while a model is loading or transcription is in progress. Try again when idle."
                        .to_string(),
                );
            }

            // Only reload ORT-based engines (Whisper uses whisper.cpp)
            let needs_reload = app
                .try_state::<Arc<ModelManager>>()
                .and_then(|mm| mm.get_model_info(&current_model_id))
                .map(|info| is_ort_engine(&info.engine_type))
                .unwrap_or(false);

            if needs_reload {
                Some(current_model_id)
            } else {
                info!(
                    "Skipping batch model reload: '{}' is not ORT-based",
                    current_model_id
                );
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // --- Mutation phase: validated, safe to proceed ---

    transcribe_rs::set_gpu_provider(provider.into());
    info!("GPU provider changed to: {:?}", provider);

    let mut s = s;
    s.gpu_provider = provider;
    settings::write_settings(&app, s);

    // Reload model if needed
    if let Some(model_id) = reload_model_id {
        if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
            info!("Reloading batch model '{}' for new GPU provider", model_id);
            if let Err(e) = tm.unload_model() {
                log::warn!("Failed to unload batch model: {}", e);
            }
            if let Err(e) = tm.load_model(&model_id) {
                // Rollback: restore previous provider on reload failure
                log::warn!("Failed to reload batch model, rolling back provider: {}", e);
                transcribe_rs::set_gpu_provider(previous.into());
                let mut reverted = settings::get_settings(&app);
                reverted.gpu_provider = previous;
                settings::write_settings(&app, reverted);
                return Err(format!("Failed to reload batch model: {}", e));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::EngineType;

    #[test]
    fn gpu_provider_from_round_trip() {
        let variants = [
            GpuProvider::Auto,
            GpuProvider::CpuOnly,
            GpuProvider::DirectMl,
            GpuProvider::Cuda,
            GpuProvider::CoreMl,
            GpuProvider::WebGpu,
        ];
        for v in variants {
            let tr: transcribe_rs::GpuProvider = v.into();
            let back: GpuProvider = tr.into();
            assert_eq!(back, v);
        }
    }

    #[test]
    fn is_ort_engine_classification() {
        assert!(!is_ort_engine(&EngineType::Whisper));
        assert!(is_ort_engine(&EngineType::Parakeet));
        assert!(is_ort_engine(&EngineType::Moonshine));
        assert!(is_ort_engine(&EngineType::MoonshineStreaming));
        assert!(is_ort_engine(&EngineType::SenseVoice));
    }

    #[test]
    fn available_providers_includes_auto_and_cpu() {
        let providers = get_available_gpu_providers();
        assert!(providers.contains(&GpuProvider::Auto));
        assert!(providers.contains(&GpuProvider::CpuOnly));
        assert!(providers.len() >= 2);
    }
}
