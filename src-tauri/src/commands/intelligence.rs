use crate::intelligence::vocab::{VocabMiner, VocabSuggestion};
use crate::intelligence::{self, IntelligenceError};
use crate::settings::{get_settings, write_settings};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub fn get_vocab_suggestions(app: AppHandle) -> Vec<VocabSuggestion> {
    app.state::<Arc<VocabMiner>>().suggestions()
}

#[tauri::command]
#[specta::specta]
pub fn resolve_vocab_suggestion(app: AppHandle, word: String, accept: bool) -> Result<(), String> {
    app.state::<Arc<VocabMiner>>().resolve(&word, accept)
}

#[tauri::command]
#[specta::specta]
pub fn run_vocab_scan_now(app: AppHandle) {
    app.state::<Arc<VocabMiner>>().maybe_run(true);
}

#[tauri::command]
#[specta::specta]
pub fn change_intelligence_provider_setting(
    app: AppHandle,
    provider_id: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    if settings.post_process_provider(&provider_id).is_none() {
        return Err(format!("Unknown provider '{provider_id}'"));
    }
    settings.intelligence_provider_id = provider_id;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_intelligence_model_setting(app: AppHandle, model: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.intelligence_model = model;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_voice_edit_enabled_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.voice_edit_enabled = enabled;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_voice_edit_window_setting(app: AppHandle, secs: u32) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.voice_edit_window_secs = secs as u64;
    write_settings(&app, settings);
    Ok(())
}

/// Check the configured intelligence provider is reachable; returns the list
/// of available models so the UI can populate its model dropdown. Works even
/// before a model is selected.
#[tauri::command]
#[specta::specta]
pub async fn test_intelligence_connection(app: AppHandle) -> Result<Vec<String>, String> {
    let settings = get_settings(&app);
    let provider = settings
        .post_process_provider(&settings.intelligence_provider_id)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Unknown intelligence provider '{}'",
                settings.intelligence_provider_id
            )
        })?;
    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    let ctx = intelligence::IntelligenceContext {
        provider,
        api_key,
        model: String::new(),
    };
    intelligence::health_check(&ctx).await.map_err(|e| match e {
        IntelligenceError::Unavailable(msg) => {
            format!("Provider unreachable — is it running? ({msg})")
        }
        other => other.to_string(),
    })
}
