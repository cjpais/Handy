// CI-only mock TranscriptionManager - avoids whisper/Vulkan dependencies.
// This file is copied over transcription.rs during CI tests.
// Existing tests don't exercise transcription, so this is safe.

use crate::managers::model::ModelManager;
use crate::settings::AppSettings;
use anyhow::Result;
use serde::Serialize;
use specta::Type;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;

#[derive(Clone, Debug, Serialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub error: Option<String>,
}

/// RAII guard that is a no-op in the mock — mirrors the real `LoadingGuard`.
pub struct LoadingGuard;

#[derive(Clone)]
pub struct TranscriptionManager {
    #[allow(dead_code)]
    app_handle: AppHandle,
    dictation_active: Arc<Mutex<bool>>,
}

impl TranscriptionManager {
    pub fn new(app_handle: &AppHandle, _model_manager: Arc<ModelManager>) -> Result<Self> {
        Ok(Self {
            app_handle: app_handle.clone(),
            dictation_active: Arc::new(Mutex::new(false)),
        })
    }

    pub fn is_model_loaded(&self) -> bool {
        false
    }

    pub fn try_start_loading(&self) -> Option<LoadingGuard> {
        Some(LoadingGuard)
    }

    pub fn unload_model(&self) -> Result<()> {
        Ok(())
    }

    pub fn maybe_unload_immediately(&self, _context: &str) {}

    pub fn load_model(&self, _model_id: &str) -> Result<()> {
        Ok(())
    }

    pub fn initiate_model_load(&self) {}

    pub fn get_current_model(&self) -> Option<String> {
        None
    }

    pub fn set_dictation_active(&self, active: bool) {
        *self.dictation_active.lock().unwrap() = active;
    }

    pub fn is_dictation_active(&self) -> bool {
        *self.dictation_active.lock().unwrap()
    }

    pub fn wait_for_dictation_idle_for(&self, _timeout: Duration) -> bool {
        !self.is_dictation_active()
    }

    pub fn transcribe(&self, _audio: Vec<f32>) -> Result<String> {
        Ok(String::new())
    }

    pub fn transcribe_with_settings(
        &self,
        audio: Vec<f32>,
        _settings: AppSettings,
    ) -> Result<String> {
        self.transcribe(audio)
    }
}

/// No-op in CI mock.
pub fn apply_accelerator_settings(_app: &tauri::AppHandle) {}

#[derive(Serialize, Clone, Debug, Type)]
pub struct GpuDeviceOption {
    pub id: i32,
    pub name: String,
    pub total_vram_mb: usize,
}

#[derive(Serialize, Clone, Debug, Type)]
pub struct AvailableAccelerators {
    pub whisper: Vec<String>,
    pub ort: Vec<String>,
    pub gpu_devices: Vec<GpuDeviceOption>,
}

/// Returns empty lists in CI mock.
pub fn get_available_accelerators() -> AvailableAccelerators {
    AvailableAccelerators {
        whisper: vec![],
        ort: vec![],
        gpu_devices: vec![],
    }
}
