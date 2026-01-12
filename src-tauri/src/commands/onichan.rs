use crate::local_llm::LocalLlmManager;
use crate::local_tts::LocalTtsManager;
use crate::onichan::{ConversationMessage, OnichanManager, OnichanMode};
use crate::onichan_conversation::OnichanConversationManager;
use crate::onichan_models::{OnichanModelInfo, OnichanModelManager};
use crate::settings::get_settings;
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub fn onichan_enable(manager: State<'_, Arc<OnichanManager>>) {
    manager.enable();
}

#[tauri::command]
#[specta::specta]
pub fn onichan_disable(manager: State<'_, Arc<OnichanManager>>) {
    manager.disable();
}

#[tauri::command]
#[specta::specta]
pub fn onichan_is_active(manager: State<'_, Arc<OnichanManager>>) -> bool {
    manager.is_active()
}

#[tauri::command]
#[specta::specta]
pub fn onichan_get_mode(manager: State<'_, Arc<OnichanManager>>) -> OnichanMode {
    manager.get_mode()
}

#[tauri::command]
#[specta::specta]
pub fn onichan_set_mode(manager: State<'_, Arc<OnichanManager>>, mode: OnichanMode) {
    manager.set_mode(mode);
}

#[tauri::command]
#[specta::specta]
pub async fn onichan_process_input(
    manager: State<'_, Arc<OnichanManager>>,
    text: String,
) -> Result<String, String> {
    manager.process_input(text).await
}

#[tauri::command]
#[specta::specta]
pub async fn onichan_speak(
    manager: State<'_, Arc<OnichanManager>>,
    text: String,
) -> Result<(), String> {
    manager.speak(&text).await
}

#[tauri::command]
#[specta::specta]
pub fn onichan_clear_history(manager: State<'_, Arc<OnichanManager>>) {
    manager.clear_history();
}

#[tauri::command]
#[specta::specta]
pub fn onichan_get_history(manager: State<'_, Arc<OnichanManager>>) -> Vec<ConversationMessage> {
    manager.get_history()
}

// Model management commands

#[tauri::command]
#[specta::specta]
pub fn get_onichan_models(manager: State<'_, Arc<OnichanModelManager>>) -> Vec<OnichanModelInfo> {
    manager.get_available_models()
}

#[tauri::command]
#[specta::specta]
pub fn get_onichan_llm_models(
    manager: State<'_, Arc<OnichanModelManager>>,
) -> Vec<OnichanModelInfo> {
    manager.get_llm_models()
}

#[tauri::command]
#[specta::specta]
pub fn get_onichan_tts_models(
    manager: State<'_, Arc<OnichanModelManager>>,
) -> Vec<OnichanModelInfo> {
    manager.get_tts_models()
}

#[tauri::command]
#[specta::specta]
pub async fn download_onichan_model(
    manager: State<'_, Arc<OnichanModelManager>>,
    model_id: String,
) -> Result<(), String> {
    manager
        .download_model(&model_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_onichan_model(
    manager: State<'_, Arc<OnichanModelManager>>,
    model_id: String,
) -> Result<(), String> {
    manager.delete_model(&model_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn load_local_llm(
    model_manager: State<'_, Arc<OnichanModelManager>>,
    llm_manager: State<'_, Arc<LocalLlmManager>>,
    model_id: String,
) -> Result<(), String> {
    log::info!("load_local_llm command called with model_id: {}", model_id);

    let model_path = model_manager.get_model_path(&model_id).map_err(|e| {
        log::error!("Failed to get model path: {}", e);
        e.to_string()
    })?;

    log::info!("Model path resolved to: {:?}", model_path);

    // Load model synchronously (blocking) - tokio spawn_blocking causes crashes with llama-cpp
    log::info!("Loading model synchronously...");
    let result = llm_manager.load_model(&model_path);

    match &result {
        Ok(()) => log::info!("Model loaded successfully via command"),
        Err(e) => log::error!("Model load failed: {}", e),
    }

    result
}

#[tauri::command]
#[specta::specta]
pub fn unload_local_llm(llm_manager: State<'_, Arc<LocalLlmManager>>) {
    llm_manager.unload_model();
}

#[tauri::command]
#[specta::specta]
pub fn is_local_llm_loaded(llm_manager: State<'_, Arc<LocalLlmManager>>) -> bool {
    llm_manager.is_loaded()
}

#[tauri::command]
#[specta::specta]
pub fn get_local_llm_model_name(llm_manager: State<'_, Arc<LocalLlmManager>>) -> Option<String> {
    llm_manager.get_loaded_model_name()
}

#[tauri::command]
#[specta::specta]
pub fn local_llm_chat(
    llm_manager: State<'_, Arc<LocalLlmManager>>,
    system_prompt: String,
    user_message: String,
    max_tokens: u32,
) -> Result<String, String> {
    llm_manager.chat(&system_prompt, &user_message, max_tokens)
}

#[tauri::command]
#[specta::specta]
pub fn load_local_tts(
    model_manager: State<'_, Arc<OnichanModelManager>>,
    tts_manager: State<'_, Arc<LocalTtsManager>>,
    model_id: String,
) -> Result<(), String> {
    let model_path = model_manager
        .get_model_path(&model_id)
        .map_err(|e| e.to_string())?;
    tts_manager.load_model(&model_path)
}

#[tauri::command]
#[specta::specta]
pub fn unload_local_tts(tts_manager: State<'_, Arc<LocalTtsManager>>) {
    tts_manager.unload_model();
}

#[tauri::command]
#[specta::specta]
pub fn is_local_tts_loaded(tts_manager: State<'_, Arc<LocalTtsManager>>) -> bool {
    tts_manager.is_loaded()
}

#[tauri::command]
#[specta::specta]
pub fn local_tts_speak(
    app: AppHandle,
    tts_manager: State<'_, Arc<LocalTtsManager>>,
    text: String,
) -> Result<(), String> {
    let settings = get_settings(&app);
    let volume = settings.audio_feedback_volume;
    // Set the output device from settings before speaking
    tts_manager.set_output_device(settings.selected_output_device.clone());
    tts_manager.speak(&text, volume)
}

// Conversation mode commands

#[tauri::command]
#[specta::specta]
pub fn onichan_start_conversation(
    conversation_manager: State<'_, Arc<OnichanConversationManager>>,
) -> Result<(), String> {
    conversation_manager.start()
}

#[tauri::command]
#[specta::specta]
pub fn onichan_stop_conversation(conversation_manager: State<'_, Arc<OnichanConversationManager>>) {
    conversation_manager.stop();
}

#[tauri::command]
#[specta::specta]
pub fn onichan_is_conversation_running(
    conversation_manager: State<'_, Arc<OnichanConversationManager>>,
) -> bool {
    conversation_manager.is_running()
}
