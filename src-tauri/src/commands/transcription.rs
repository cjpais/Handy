use crate::managers::transcription::TranscriptionManager;
use crate::settings::{
    build_slng_endpoint, get_settings, write_settings, ModelUnloadTimeout, SecretString,
    TranscriptionProvider, SLNG_DEFAULT_ENDPOINT, SLNG_DEFAULT_LANGUAGE, SLNG_DEFAULT_MODEL,
    SLNG_DEFAULT_PROVIDER, SLNG_DEFAULT_TIMEOUT_SECONDS, SONIOX_DEFAULT_BASE_URL,
    SONIOX_DEFAULT_MODEL, SONIOX_DEFAULT_TIMEOUT_SECONDS,
};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

#[derive(Serialize, Type)]
pub struct ModelLoadStatus {
    is_loaded: bool,
    current_model: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn set_model_unload_timeout(app: AppHandle, timeout: ModelUnloadTimeout) {
    let mut settings = get_settings(&app);
    settings.model_unload_timeout = timeout;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_transcription_provider(app: AppHandle, provider: TranscriptionProvider) {
    let mut settings = get_settings(&app);
    settings.transcription_provider = provider;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_soniox_api_key(app: AppHandle, api_key: String) {
    let mut settings = get_settings(&app);
    settings.soniox_api_key = SecretString::new(api_key);
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_soniox_base_url(app: AppHandle, base_url: String) {
    let mut settings = get_settings(&app);
    let trimmed = base_url.trim();
    settings.soniox_base_url = if trimmed.is_empty() {
        SONIOX_DEFAULT_BASE_URL.to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_soniox_model(app: AppHandle, model: String) {
    let mut settings = get_settings(&app);
    let trimmed = model.trim();
    settings.soniox_model = if trimmed.is_empty() {
        SONIOX_DEFAULT_MODEL.to_string()
    } else {
        trimmed.to_string()
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_soniox_timeout_seconds(app: AppHandle, timeout_seconds: u64) {
    let mut settings = get_settings(&app);
    settings.soniox_timeout_seconds = if timeout_seconds == 0 {
        SONIOX_DEFAULT_TIMEOUT_SECONDS
    } else {
        timeout_seconds
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_soniox_fallback_to_local(app: AppHandle, fallback_to_local: bool) {
    let mut settings = get_settings(&app);
    settings.soniox_fallback_to_local = fallback_to_local;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_api_key(app: AppHandle, api_key: String) {
    let mut settings = get_settings(&app);
    settings.slng_api_key = SecretString::new(api_key);
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_endpoint(app: AppHandle, endpoint: String) {
    let mut settings = get_settings(&app);
    let trimmed = endpoint.trim();
    settings.slng_endpoint = if trimmed.is_empty() {
        SLNG_DEFAULT_ENDPOINT.to_string()
    } else {
        trimmed.to_string()
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_provider(app: AppHandle, provider: String) {
    let mut settings = get_settings(&app);
    let trimmed = provider.trim();
    settings.slng_provider = if trimmed.is_empty() {
        SLNG_DEFAULT_PROVIDER.to_string()
    } else {
        trimmed.trim_matches('/').to_string()
    };
    settings.slng_endpoint = build_slng_endpoint(&settings.slng_provider, &settings.slng_model);
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_model(app: AppHandle, model: String) {
    let mut settings = get_settings(&app);
    let trimmed = model.trim();
    settings.slng_model = if trimmed.is_empty() {
        SLNG_DEFAULT_MODEL.to_string()
    } else {
        trimmed.trim_matches('/').to_string()
    };
    settings.slng_endpoint = build_slng_endpoint(&settings.slng_provider, &settings.slng_model);
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_language(app: AppHandle, language: String) {
    let mut settings = get_settings(&app);
    let trimmed = language.trim();
    settings.slng_language = if trimmed.is_empty() {
        SLNG_DEFAULT_LANGUAGE.to_string()
    } else {
        trimmed.to_string()
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_timeout_seconds(app: AppHandle, timeout_seconds: u64) {
    let mut settings = get_settings(&app);
    settings.slng_timeout_seconds = if timeout_seconds == 0 {
        SLNG_DEFAULT_TIMEOUT_SECONDS
    } else {
        timeout_seconds
    };
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn set_slng_fallback_to_local(app: AppHandle, fallback_to_local: bool) {
    let mut settings = get_settings(&app);
    settings.slng_fallback_to_local = fallback_to_local;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn get_model_load_status(
    transcription_manager: State<TranscriptionManager>,
) -> Result<ModelLoadStatus, String> {
    Ok(ModelLoadStatus {
        is_loaded: transcription_manager.is_model_loaded(),
        current_model: transcription_manager.get_current_model(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn unload_model_manually(
    transcription_manager: State<TranscriptionManager>,
) -> Result<(), String> {
    transcription_manager
        .unload_model()
        .map_err(|e| format!("Failed to unload model: {}", e))
}
