//! CI-only mock for GPU provider commands.
//!
//! Mirrors the public API of `gpu.rs` without depending on `transcribe_rs`.
//! This file is copied over `gpu.rs` during CI tests.

use log::info;
use tauri::AppHandle;

use crate::settings::{self, GpuProvider};

/// Return which GPU providers are available in this (mock) build.
#[tauri::command]
#[specta::specta]
pub fn get_available_gpu_providers() -> Vec<GpuProvider> {
    vec![GpuProvider::Auto, GpuProvider::CpuOnly]
}

/// Apply the persisted GPU provider at startup (mock: validate + log only).
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
    info!("GPU provider set to: {:?} (mock)", startup_settings.gpu_provider);
}

/// Change the GPU provider setting (mock: validate + persist, no model reload).
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

    let mut s = s;
    s.gpu_provider = provider;
    settings::write_settings(&app, s);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::model::EngineType;

    /// Returns true for engine types that use ORT.
    fn is_ort_engine(engine_type: &EngineType) -> bool {
        matches!(
            engine_type,
            EngineType::Parakeet
                | EngineType::Moonshine
                | EngineType::MoonshineStreaming
                | EngineType::SenseVoice
        )
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
    fn available_providers_mock_returns_auto_and_cpu() {
        let providers = get_available_gpu_providers();
        assert_eq!(providers, vec![GpuProvider::Auto, GpuProvider::CpuOnly]);
    }
}
