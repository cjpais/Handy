use crate::audio_toolkit::decode_audio_file;
use crate::clipboard::paste;
use crate::managers::history::HistoryManager;
use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, AppSettings};
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::{debug, error, info};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

const MAX_FILE_SIZE_BYTES: u64 = 64 * 1024 * 1024; // 64 MB

const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "flac", "ogg", "aac"];

#[derive(serde::Serialize, Clone)]
struct FileTranscriptionEvent {
    file_path: String,
    transcription_text: String,
}

#[derive(serde::Serialize, Clone)]
struct FileTranscriptionErrorEvent {
    file_path: String,
    error: String,
}

/// Apply LLM-based post-processing to transcription text
async fn maybe_post_process_transcription(
    settings: &AppSettings,
    transcription: &str,
) -> Option<String> {
    if !settings.post_process_enabled {
        return None;
    }

    let provider = match settings.active_post_process_provider().cloned() {
        Some(provider) => provider,
        None => {
            debug!("Post-processing enabled but no provider is selected");
            return None;
        }
    };

    let model = settings
        .post_process_models
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    if model.trim().is_empty() {
        debug!(
            "Post-processing skipped because provider '{}' has no model configured",
            provider.id
        );
        return None;
    }

    let selected_prompt_id = match &settings.post_process_selected_prompt_id {
        Some(id) => id.clone(),
        None => {
            debug!("Post-processing skipped because no prompt is selected");
            return None;
        }
    };

    let prompt = match settings
        .post_process_prompts
        .iter()
        .find(|prompt| prompt.id == selected_prompt_id)
    {
        Some(prompt) => prompt.prompt.clone(),
        None => {
            debug!(
                "Post-processing skipped because prompt '{}' was not found",
                selected_prompt_id
            );
            return None;
        }
    };

    if prompt.trim().is_empty() {
        debug!("Post-processing skipped because the selected prompt is empty");
        return None;
    }

    let api_key = settings
        .post_process_api_keys
        .get(&provider.id)
        .cloned()
        .unwrap_or_default();

    debug!(
        "Starting LLM post-processing with provider '{}' (model: {})",
        provider.id, model
    );

    let processed_prompt = prompt.replace("${output}", transcription);
    debug!("Processed prompt length: {} chars", processed_prompt.len());

    let client = match crate::llm_client::create_client(&provider, api_key) {
        Ok(client) => client,
        Err(e) => {
            error!("Failed to create LLM client: {}", e);
            return None;
        }
    };

    let message = match ChatCompletionRequestUserMessageArgs::default()
        .content(processed_prompt)
        .build()
    {
        Ok(msg) => ChatCompletionRequestMessage::User(msg),
        Err(e) => {
            error!("Failed to build chat message: {}", e);
            return None;
        }
    };

    let request = match CreateChatCompletionRequestArgs::default()
        .model(&model)
        .messages(vec![message])
        .build()
    {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to build chat completion request: {}", e);
            return None;
        }
    };

    match client.chat().create(request).await {
        Ok(response) => {
            if let Some(choice) = response.choices.first() {
                if let Some(content) = &choice.message.content {
                    debug!(
                        "LLM post-processing succeeded for provider '{}'. Output length: {} chars",
                        provider.id,
                        content.len()
                    );
                    return Some(content.clone());
                }
            }
            error!("LLM API response has no content");
            None
        }
        Err(e) => {
            error!(
                "LLM post-processing failed for provider '{}': {}",
                provider.id, e
            );
            None
        }
    }
}

/// Convert between Simplified and Traditional Chinese using OpenCC
async fn maybe_convert_chinese_variant(
    settings: &AppSettings,
    transcription: &str,
) -> Option<String> {
    let is_simplified = settings.selected_language == "zh-Hans";
    let is_traditional = settings.selected_language == "zh-Hant";

    if !is_simplified && !is_traditional {
        debug!("Language not Chinese; skipping variant conversion");
        return None;
    }

    debug!(
        "Starting Chinese variant conversion for: {}",
        settings.selected_language
    );

    let config = if is_simplified {
        BuiltinConfig::Tw2sp
    } else {
        BuiltinConfig::S2twp
    };

    match OpenCC::from_config(config) {
        Ok(converter) => {
            let converted = converter.convert(transcription);
            debug!("Chinese variant conversion completed");
            Some(converted)
        }
        Err(e) => {
            error!("Failed to initialize OpenCC converter: {}", e);
            None
        }
    }
}

#[tauri::command]
pub fn transcribe_file(app: AppHandle, file_path: String) -> Result<(), String> {
    log::info!("Received transcription request for file: {}", file_path);

    // Validate file exists
    let path = Path::new(&file_path);
    if !path.exists() {
        let error = format!("File does not exist: {}", file_path);
        log::error!("{}", error);
        let _ = app.emit(
            "file-transcription-failed",
            FileTranscriptionErrorEvent {
                file_path: file_path.clone(),
                error: error.clone(),
            },
        );
        return Err(error);
    }

    // Validate file is readable and get metadata
    let metadata = std::fs::metadata(path).map_err(|e| {
        let error = format!("Cannot read file metadata: {}", e);
        log::error!("{}", error);
        let _ = app.emit(
            "file-transcription-failed",
            FileTranscriptionErrorEvent {
                file_path: file_path.clone(),
                error: error.clone(),
            },
        );
        error
    })?;

    // Validate it's a file, not a directory
    if !metadata.is_file() {
        let error = format!("Path is not a file: {}", file_path);
        log::error!("{}", error);
        let _ = app.emit(
            "file-transcription-failed",
            FileTranscriptionErrorEvent {
                file_path: file_path.clone(),
                error: error.clone(),
            },
        );
        return Err(error);
    }

    // Validate file size
    let file_size = metadata.len();
    if file_size > MAX_FILE_SIZE_BYTES {
        let error = format!(
            "File size ({} MB) exceeds maximum allowed size ({} MB)",
            file_size / (1024 * 1024),
            MAX_FILE_SIZE_BYTES / (1024 * 1024)
        );
        log::error!("{}", error);
        let _ = app.emit(
            "file-transcription-failed",
            FileTranscriptionErrorEvent {
                file_path: file_path.clone(),
                error: error.clone(),
            },
        );
        return Err(error);
    }

    // Validate file extension
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    match extension {
        Some(ext) if SUPPORTED_AUDIO_EXTENSIONS.contains(&ext.as_str()) => {
            log::info!("File validation passed for: {}", file_path);
        }
        Some(ext) => {
            let error = format!(
                "Unsupported file extension: .{}. Supported formats: {}",
                ext,
                SUPPORTED_AUDIO_EXTENSIONS.join(", ")
            );
            log::error!("{}", error);
            let _ = app.emit(
                "file-transcription-failed",
                FileTranscriptionErrorEvent {
                    file_path: file_path.clone(),
                    error: error.clone(),
                },
            );
            return Err(error);
        }
        None => {
            let error = "File has no extension".to_string();
            log::error!("{}", error);
            let _ = app.emit(
                "file-transcription-failed",
                FileTranscriptionErrorEvent {
                    file_path: file_path.clone(),
                    error: error.clone(),
                },
            );
            return Err(error);
        }
    }

    // Validation complete - emit started event
    info!("Starting transcription for file: {}", file_path);
    let _ = app.emit(
        "file-transcription-started",
        FileTranscriptionEvent {
            file_path: file_path.clone(),
            transcription_text: String::new(),
        },
    );

    // Spawn async task to handle decode -> transcribe -> post-process -> save -> paste
    let file_path_clone = file_path.clone();
    tauri::async_runtime::spawn(async move {
        let result = process_file_transcription(&app, &file_path_clone).await;
        
        match result {
            Ok(transcription_text) => {
                info!("File transcription completed successfully: {}", file_path_clone);
                let _ = app.emit(
                    "file-transcription-completed",
                    FileTranscriptionEvent {
                        file_path: file_path_clone,
                        transcription_text,
                    },
                );
            }
            Err(e) => {
                error!("File transcription failed: {}", e);
                let _ = app.emit(
                    "file-transcription-failed",
                    FileTranscriptionErrorEvent {
                        file_path: file_path_clone,
                        error: e,
                    },
                );
            }
        }
    });

    Ok(())
}

/// Process the file transcription pipeline: decode -> transcribe -> post-process -> save -> paste
async fn process_file_transcription(app: &AppHandle, file_path: &str) -> Result<String, String> {
    info!("Decoding audio file: {}", file_path);
    let audio_samples = decode_audio_file(file_path)
        .await
        .map_err(|e| format!("Failed to decode audio file: {}", e))?;
    
    info!("Decoded {} audio samples", audio_samples.len());

    let tm = app.state::<Arc<TranscriptionManager>>();
    
    if !tm.is_model_loaded() {
        info!("Model not loaded, initiating load");
        tm.initiate_model_load();
        
        let max_wait = std::time::Duration::from_secs(120);
        let start = std::time::Instant::now();
        
        while !tm.is_model_loaded() && start.elapsed() < max_wait {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        
        if !tm.is_model_loaded() {
            return Err("Model failed to load within timeout period".to_string());
        }
        
        info!("Model loaded successfully");
    }
    
    info!("Starting transcription");
    let transcription = tm
        .transcribe(audio_samples.clone())
        .map_err(|e| format!("Transcription failed: {}", e))?;

    if transcription.is_empty() {
        return Err("Transcription result is empty".to_string());
    }

    info!("Transcription completed: '{}'", transcription);

    let settings = get_settings(app);
    let mut final_text = transcription.clone();
    let mut post_processed_text: Option<String> = None;
    let mut post_process_prompt: Option<String> = None;

    if let Some(converted_text) = maybe_convert_chinese_variant(&settings, &transcription).await {
        final_text = converted_text.clone();
        post_processed_text = Some(converted_text);
    } else if let Some(processed_text) = maybe_post_process_transcription(&settings, &transcription).await {
        final_text = processed_text.clone();
        post_processed_text = Some(processed_text);

        if let Some(prompt_id) = &settings.post_process_selected_prompt_id {
            if let Some(prompt) = settings
                .post_process_prompts
                .iter()
                .find(|p| &p.id == prompt_id)
            {
                post_process_prompt = Some(prompt.prompt.clone());
            }
        }
    }

    let hm = app.state::<Arc<HistoryManager>>();
    info!("Saving transcription to history");
    if let Err(e) = hm
        .save_transcription(
            audio_samples,
            transcription.clone(),
            post_processed_text,
            post_process_prompt,
            Some(file_path.to_string()),
        )
        .await
    {
        error!("Failed to save transcription to history: {}", e);
    }

    let final_text_clone = final_text.clone();
    let app_clone = app.clone();
    app.run_on_main_thread(move || {
        match paste(final_text_clone, app_clone) {
            Ok(()) => info!("Text pasted successfully"),
            Err(e) => error!("Failed to paste transcription: {}", e),
        }
    })
    .map_err(|e| format!("Failed to run paste on main thread: {:?}", e))?;

    Ok(final_text)
}
