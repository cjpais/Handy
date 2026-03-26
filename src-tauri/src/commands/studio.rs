use crate::managers::studio::{StartStudioJobConfig, StudioHomeData, StudioJob, StudioManager};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn prepare_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    file_path: String,
) -> Result<StudioJob, String> {
    studio_manager
        .prepare_job(&file_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn start_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
    config: StartStudioJobConfig,
) -> Result<(), String> {
    studio_manager
        .start_job(&job_id, config)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
) -> Result<(), String> {
    studio_manager
        .cancel_job(&job_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
) -> Result<Option<StudioJob>, String> {
    studio_manager
        .get_job(&job_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn list_studio_jobs(
    studio_manager: State<'_, Arc<StudioManager>>,
) -> Result<StudioHomeData, String> {
    studio_manager
        .list_jobs()
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
) -> Result<(), String> {
    studio_manager
        .delete_job(&job_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn open_studio_output_folder(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
) -> Result<(), String> {
    studio_manager
        .open_output_folder(&job_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn retry_studio_job(
    studio_manager: State<'_, Arc<StudioManager>>,
    job_id: String,
) -> Result<(), String> {
    studio_manager
        .retry_job(&job_id)
        .map_err(|error| error.to_string())
}
