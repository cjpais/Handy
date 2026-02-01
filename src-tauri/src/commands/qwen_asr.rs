use crate::managers::model::ModelManager;
use crate::managers::qwen_asr::{PrerequisiteStatus, QwenAsrManager};
use std::sync::Arc;
use tauri::State;

/// Check if python3 and mlx-audio are installed.
#[tauri::command]
pub async fn check_qwen_asr_prerequisites() -> Result<PrerequisiteStatus, String> {
    QwenAsrManager::check_prerequisites().map_err(|e| e.to_string())
}

/// Install mlx-audio via pip.
#[tauri::command]
pub async fn install_qwen_asr_dependencies() -> Result<String, String> {
    QwenAsrManager::install_mlx_audio().map_err(|e| e.to_string())
}

/// Setup Qwen3-ASR: check prerequisites, then mark the model as ready.
/// The actual model download from HuggingFace happens on first load via the sidecar.
#[tauri::command]
pub async fn setup_qwen_asr(
    model_manager: State<'_, Arc<ModelManager>>,
) -> Result<(), String> {
    // Check prerequisites first
    let status = QwenAsrManager::check_prerequisites().map_err(|e| e.to_string())?;

    if !status.available {
        return Err(status.message);
    }

    // Mark qwen3-asr as ready in model manager
    model_manager.set_qwen_asr_ready(true);

    Ok(())
}
