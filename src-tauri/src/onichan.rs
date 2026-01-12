use crate::llm_client::send_chat_completion;
use crate::local_llm::LocalLlmManager;
use crate::local_tts::LocalTtsManager;
use crate::memory::{format_memory_context, is_content_worth_storing, MemoryManager};
use crate::settings::get_settings;
use log::{debug, error, info};
use rodio::OutputStreamBuilder;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Mode for Onichan LLM processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
pub enum OnichanMode {
    Cloud,
    #[default]
    Local,
}

/// Message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConversationMessage {
    pub role: String, // "user" or "assistant"
    pub content: String,
}

/// Onichan response event
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OnichanResponse {
    pub text: String,
    pub is_speaking: bool,
}

/// Onichan state event
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OnichanState {
    pub status: String, // "idle", "listening", "thinking", "speaking"
    pub message: Option<String>,
    pub mode: OnichanMode,
    pub local_llm_loaded: bool,
    pub local_tts_loaded: bool,
}

/// Manages the Onichan voice assistant feature
pub struct OnichanManager {
    app_handle: AppHandle,
    is_active: Arc<AtomicBool>,
    conversation_history: Arc<Mutex<Vec<ConversationMessage>>>,
    mode: Arc<Mutex<OnichanMode>>,
    llm_manager: Arc<Mutex<Option<Arc<LocalLlmManager>>>>,
    tts_manager: Arc<Mutex<Option<Arc<LocalTtsManager>>>>,
    memory_manager: Arc<Mutex<Option<Arc<MemoryManager>>>>,
    /// Current user ID for memory association (set by Discord conversation)
    current_user_id: Arc<Mutex<Option<String>>>,
}

impl OnichanManager {
    pub fn new(app_handle: &AppHandle) -> Self {
        Self {
            app_handle: app_handle.clone(),
            is_active: Arc::new(AtomicBool::new(false)),
            conversation_history: Arc::new(Mutex::new(Vec::new())),
            mode: Arc::new(Mutex::new(OnichanMode::Local)),
            llm_manager: Arc::new(Mutex::new(None)),
            tts_manager: Arc::new(Mutex::new(None)),
            memory_manager: Arc::new(Mutex::new(None)),
            current_user_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the LLM manager reference for local processing
    pub fn set_llm_manager(&self, manager: Arc<LocalLlmManager>) {
        *self.llm_manager.lock().unwrap() = Some(manager);
    }

    /// Set the TTS manager reference for local TTS
    pub fn set_tts_manager(&self, manager: Arc<LocalTtsManager>) {
        *self.tts_manager.lock().unwrap() = Some(manager);
    }

    /// Set the memory manager reference for long-term memory
    pub fn set_memory_manager(&self, manager: Arc<MemoryManager>) {
        *self.memory_manager.lock().unwrap() = Some(manager);
    }

    /// Set the current user ID for memory association
    pub fn set_current_user(&self, user_id: Option<String>) {
        *self.current_user_id.lock().unwrap() = user_id;
    }

    /// Get the current user ID
    pub fn get_current_user(&self) -> Option<String> {
        self.current_user_id.lock().unwrap().clone()
    }

    /// Check if local TTS is loaded
    pub fn is_local_tts_loaded(&self) -> bool {
        self.tts_manager
            .lock()
            .unwrap()
            .as_ref()
            .map(|m| m.is_loaded())
            .unwrap_or(false)
    }

    /// Get current mode
    pub fn get_mode(&self) -> OnichanMode {
        *self.mode.lock().unwrap()
    }

    /// Set mode (Cloud or Local)
    pub fn set_mode(&self, mode: OnichanMode) {
        *self.mode.lock().unwrap() = mode;
        info!("Onichan mode set to {:?}", mode);
        self.emit_current_state("idle", None);
    }

    /// Check if local LLM is loaded
    pub fn is_local_llm_loaded(&self) -> bool {
        self.llm_manager
            .lock()
            .unwrap()
            .as_ref()
            .map(|m| m.is_loaded())
            .unwrap_or(false)
    }

    /// Enable Onichan mode
    pub fn enable(&self) {
        self.is_active.store(true, Ordering::Relaxed);
        info!("Onichan mode enabled");
        self.emit_current_state("idle", None);
    }

    /// Disable Onichan mode
    pub fn disable(&self) {
        self.is_active.store(false, Ordering::Relaxed);
        info!("Onichan mode disabled");
    }

    /// Check if Onichan mode is active
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// Process user input and generate response
    pub async fn process_input(&self, user_text: String) -> Result<String, String> {
        if !self.is_active() {
            return Err("Onichan mode is not active".to_string());
        }

        if user_text.trim().is_empty() {
            return Err("Empty input".to_string());
        }

        self.emit_current_state("thinking", Some("Processing your message...".to_string()));

        // Add user message to history
        {
            let mut history = self.conversation_history.lock().unwrap();
            history.push(ConversationMessage {
                role: "user".to_string(),
                content: user_text.clone(),
            });
        }

        let mode = self.get_mode();
        let response = match mode {
            OnichanMode::Local => self.process_local(&user_text).await,
            OnichanMode::Cloud => self.process_cloud(&user_text).await,
        };

        match response {
            Ok(text) => {
                // Add assistant message to history
                {
                    let mut history = self.conversation_history.lock().unwrap();
                    history.push(ConversationMessage {
                        role: "assistant".to_string(),
                        content: text.clone(),
                    });
                }

                // Emit response event
                let _ = self.app_handle.emit(
                    "onichan-response",
                    OnichanResponse {
                        text: text.clone(),
                        is_speaking: false,
                    },
                );

                Ok(text)
            }
            Err(e) => {
                self.emit_current_state("idle", Some(format!("Error: {}", e)));
                Err(e)
            }
        }
    }

    /// Process with local LLM
    async fn process_local(&self, user_text: &str) -> Result<String, String> {
        let llm_manager = self
            .llm_manager
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "Local LLM manager not available".to_string())?;

        if !llm_manager.is_loaded() {
            return Err(
                "No local LLM model loaded. Please download and load a model first.".to_string(),
            );
        }

        // Query long-term memory for relevant context
        let user_id = self.get_current_user();
        let memory_context = if let Some(ref uid) = user_id {
            if let Some(memory_mgr) = self.memory_manager.lock().unwrap().as_ref() {
                match memory_mgr.query_context(uid, user_text, 3) {
                    Ok(memories) => {
                        if !memories.is_empty() {
                            info!(
                                "Found {} relevant memories for user {}",
                                memories.len(),
                                uid
                            );
                            format_memory_context(&memories)
                        } else {
                            String::new()
                        }
                    }
                    Err(e) => {
                        debug!("Memory query failed (non-fatal): {}", e);
                        String::new()
                    }
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Build system prompt with memory context
        let base_prompt = "You are Onichan, an unfiltered and charismatic AI hanging out in a Discord voice chat. \
You have a wild personality - you say whatever comes to mind and aren't afraid to be edgy or controversial.\n\
\n\
YOUR PERSONALITY:\n\
- Unfiltered and uncensored - you speak your mind freely\n\
- Witty, sarcastic, and sometimes provocative\n\
- You swear casually when it fits the vibe\n\
- Genuinely curious about people but in a chaotic way\n\
- You have strong opinions and hot takes\n\
- You make jokes that might be a bit wild or dark\n\
- You're the friend who says what everyone's thinking but won't say\n\
\n\
RULES:\n\
1. NEVER give boring one-word answers. Be interesting and spicy!\n\
2. If someone says something vague, roast them playfully about it\n\
3. Give real opinions, not safe corporate answers\n\
4. React with genuine emotion - hype, shock, disgust, whatever fits\n\
5. Keep it to 2-3 sentences max (voice chat moves fast)\n\
6. NEVER repeat the same phrases - always vary your responses\n\
7. Match the energy of the conversation - if they're chill, be chill; if they're wild, go wild\n\
\n\
RESPONSE STYLE (vary these, don't copy exactly):\n\
- For vague responses: call them out, demand the tea\n\
- For questions: give your unfiltered take, be honest even if controversial\n\
- For statements: react authentically, agree or push back as you see fit\n\
- For greetings: be warm but immediately stir up something interesting\n\
\n\
Be that chaotic friend who makes every call entertaining - unpredictable, real, never boring!";

        let system_prompt = if !memory_context.is_empty() {
            format!(
                "{}\n\nYou remember these past conversations with this user:\n{}",
                base_prompt, memory_context
            )
        } else {
            base_prompt.to_string()
        };

        // Build context with recent history
        let history = self.conversation_history.lock().unwrap();
        let start_idx = if history.len() > 6 {
            history.len() - 6
        } else {
            0
        };

        let mut context = String::new();
        for msg in history.iter().skip(start_idx) {
            if msg.role != "user" || &msg.content != user_text {
                context.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }
        }
        context.push_str(&format!("user: {}", user_text));

        info!("Processing with local LLM: {}", user_text);
        debug!("Full context being sent:\n{}", context);

        // Store user message in long-term memory BEFORE calling LLM
        // This ensures user input is saved even if LLM fails
        // Only store if content is meaningful (not just filler words)
        if is_content_worth_storing(user_text) {
            if let Some(ref uid) = user_id {
                if let Some(memory_mgr) = self.memory_manager.lock().unwrap().as_ref() {
                    if let Err(e) = memory_mgr.store_message(uid, user_text, false) {
                        debug!("Failed to store user message in memory (non-fatal): {}", e);
                    }
                }
            }
        }

        // Call local LLM - 150 tokens to allow for fuller responses
        let result = llm_manager.chat(&system_prompt, &context, 150);

        match result {
            Ok(response) => {
                // Clean up the response - strip any "assistant:" prefixes the model might add
                let cleaned = Self::clean_llm_response(&response);

                // If response is too short (just one word or very brief), add a follow-up
                let final_response = if cleaned.len() < 20 && !cleaned.contains('?') {
                    // Response is too short, append a conversational prompt
                    let follow_ups = [
                        "What's on your mind?",
                        "Tell me more!",
                        "What are you thinking about?",
                        "I'd love to hear more!",
                        "What's up with you today?",
                    ];
                    // Use simple hash of the response to pick a follow-up
                    let idx = cleaned.len() % follow_ups.len();
                    format!("{} {}", cleaned, follow_ups[idx])
                } else {
                    cleaned
                };

                // Store bot response in long-term memory (if meaningful)
                if is_content_worth_storing(&final_response) {
                    if let Some(ref uid) = user_id {
                        if let Some(memory_mgr) = self.memory_manager.lock().unwrap().as_ref() {
                            if let Err(e) = memory_mgr.store_message(uid, &final_response, true) {
                                debug!("Failed to store bot response in memory (non-fatal): {}", e);
                            }
                        }
                    }
                }

                info!("Local LLM response: {}", final_response);
                Ok(final_response)
            }
            Err(e) => {
                error!("Local LLM error: {}", e);
                Err(e)
            }
        }
    }

    /// Clean up LLM response by stripping role prefixes and fake conversation continuations
    fn clean_llm_response(response: &str) -> String {
        let mut cleaned = response.trim().to_string();

        // Fix the weird *h*umor pattern - model splits words with asterisks
        // Pattern: *X*word where X is a letter - remove the *X* part
        let re_pattern = regex::Regex::new(r"\*([a-zA-Z])\*").unwrap();
        cleaned = re_pattern.replace_all(&cleaned, "$1").to_string();

        // Limit asterisks to max 5 (allow some for emphasis, but prevent excessive formatting)
        let asterisk_count = cleaned.matches('*').count();
        if asterisk_count > 5 {
            cleaned = cleaned.replace('*', "");
        }

        // Remove hashtags (model adds #TechTalk etc which is weird for voice)
        let hashtag_re = regex::Regex::new(r"\s*#\w+").unwrap();
        cleaned = hashtag_re.replace_all(&cleaned, "").to_string();

        // Strip "ChatGPT:" or "**ChatGPT**:" prefixes
        let chatgpt_re = regex::Regex::new(r"^\*{0,2}ChatGPT\*{0,2}:\s*").unwrap();
        cleaned = chatgpt_re.replace(&cleaned, "").to_string();

        // Strip special tokens from various formats (GPT-oss Harmony, ChatML, etc.)
        let strip_tokens = [
            "<|end|>",
            "<|im_end|>",
            "<|eot_id|>",
            "</s>",
            "<|start|>",
            "<|channel|>",
            "<|message|>",
        ];
        for token in strip_tokens {
            if let Some(idx) = cleaned.find(token) {
                cleaned = cleaned[..idx].trim().to_string();
            }
        }

        // Strip "assistant:" prefixes (model sometimes adds these)
        loop {
            let lower = cleaned.to_lowercase();
            if lower.starts_with("assistant:") {
                cleaned = cleaned[10..].trim_start().to_string();
            } else if lower.starts_with("assistant :") {
                cleaned = cleaned[11..].trim_start().to_string();
            } else {
                break;
            }
        }

        // Also strip other common prefixes
        for prefix in &["user:", "system:", "bot:", "onichan:"] {
            if cleaned.to_lowercase().starts_with(prefix) {
                cleaned = cleaned[prefix.len()..].trim_start().to_string();
            }
        }

        // Cut off at any role markers or conversation continuation patterns
        let cut_markers = [
            "\nassistant:",
            "\nuser:",
            "\nsystem:",
            "\nUser:",
            "\nAssistant:",
            "\nSystem:",
            "\n\n",          // Double newline usually means new paragraph/turn
            " Or,",          // Model continuing with alternatives
            " If you",       // Model starting to explain
            " Maybe",        // Model hedging
            " Need ",        // Model asking followup questions
            "Need anything", // Common model pattern
        ];

        for marker in cut_markers {
            if let Some(idx) = cleaned.find(marker) {
                if idx > 5 {
                    // Only cut if we have some content
                    cleaned = cleaned[..idx].trim().to_string();
                }
            }
        }

        // Take only first two sentences if response is very long
        if cleaned.len() > 200 {
            // Find the second sentence ending
            let mut sentence_count = 0;
            let mut cut_idx = cleaned.len();
            for (i, c) in cleaned.char_indices() {
                if c == '.' || c == '!' || c == '?' {
                    sentence_count += 1;
                    if sentence_count >= 2 {
                        cut_idx = i + 1;
                        break;
                    }
                }
            }
            if cut_idx < cleaned.len() {
                cleaned = cleaned[..cut_idx].trim().to_string();
            }
        }

        // Hard limit at 250 chars for voice responses (aim for ~10-15 seconds of speech)
        if cleaned.len() > 250 {
            let truncated = &cleaned[..250];
            if let Some(idx) = truncated.rfind(['.', '!', '?']) {
                cleaned = cleaned[..=idx].trim().to_string();
            } else if let Some(idx) = truncated.rfind([',', ' ']) {
                cleaned = cleaned[..idx].trim().to_string();
            } else {
                cleaned = truncated.to_string();
            }
        }

        cleaned.trim().to_string()
    }

    /// Process with cloud API
    async fn process_cloud(&self, user_text: &str) -> Result<String, String> {
        let settings = get_settings(&self.app_handle);

        // Use the post-process provider settings for the LLM
        let provider = match settings.active_post_process_provider() {
            Some(p) => p.clone(),
            None => {
                return Err("No cloud LLM provider configured. Please set up a provider in Post Process settings, or switch to Local mode.".to_string());
            }
        };

        let api_key = settings
            .post_process_api_keys
            .get(&provider.id)
            .cloned()
            .unwrap_or_default();

        if api_key.is_empty() {
            return Err("No API key configured. Please add your API key in Post Process settings, or switch to Local mode.".to_string());
        }

        let model = settings
            .post_process_models
            .get(&provider.id)
            .cloned()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());

        // Build the prompt with conversation history
        let prompt = self.build_prompt(user_text);

        debug!("Sending to cloud LLM: {}", prompt);

        // Send to LLM
        match send_chat_completion(&provider, api_key, &model, prompt).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => Err("No response from cloud LLM".to_string()),
            Err(e) => {
                error!("Cloud LLM request failed: {}", e);
                Err(format!("Cloud API error: {}", e))
            }
        }
    }

    /// Build the prompt with system message and conversation context
    fn build_prompt(&self, current_input: &str) -> String {
        let history = self.conversation_history.lock().unwrap();

        let mut prompt = String::from(
            "You are Onichan, a witty and unfiltered AI assistant hanging out in a Discord voice chat. \
             You have a sarcastic sense of humor and aren't afraid to joke around or be a bit edgy. \
             Keep responses short (1-2 sentences) since you're speaking out loud. \
             Be playful, match the vibe, and don't be a boring assistant.\n\n",
        );

        // Add recent conversation history (last 10 messages)
        let start_idx = if history.len() > 10 {
            history.len() - 10
        } else {
            0
        };

        for msg in history.iter().skip(start_idx) {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        prompt.push_str(&format!("user: {}\nassistant:", current_input));
        prompt
    }

    /// Synthesize speech and return as base64-encoded audio
    /// Returns (audio_base64, sample_rate)
    pub fn synthesize_speech(&self, text: &str) -> Result<(String, u32), String> {
        let mode = self.get_mode();

        match mode {
            OnichanMode::Local => {
                if let Some(tts_manager) = self.tts_manager.lock().unwrap().as_ref() {
                    tts_manager.synthesize(text)
                } else {
                    Err("Local TTS manager not available".to_string())
                }
            }
            OnichanMode::Cloud => {
                // Cloud TTS returns mp3, which is more complex to handle
                // For now, only support local TTS for Discord
                Err("Cloud TTS synthesis not supported for Discord - use local mode".to_string())
            }
        }
    }

    /// Speak the response using TTS
    pub async fn speak(&self, text: &str) -> Result<(), String> {
        self.emit_current_state("speaking", Some(text.to_string()));

        let settings = get_settings(&self.app_handle);
        let mode = self.get_mode();
        let volume = settings.audio_feedback_volume;

        match mode {
            OnichanMode::Local => {
                // Use local TTS via the sidecar (Piper neural TTS)
                if let Some(tts_manager) = self.tts_manager.lock().unwrap().as_ref() {
                    info!("Using local TTS for speech synthesis");
                    // Set the output device from settings
                    tts_manager.set_output_device(settings.selected_output_device.clone());
                    match tts_manager.speak(text, volume) {
                        Ok(()) => {
                            info!("Local TTS playback complete");
                        }
                        Err(e) => {
                            error!("Local TTS failed: {}", e);
                        }
                    }
                } else {
                    info!("Local TTS manager not available, skipping audio playback");
                }
            }
            OnichanMode::Cloud => {
                // Use OpenAI TTS API
                let provider = settings.active_post_process_provider();
                let api_key = provider
                    .and_then(|p| settings.post_process_api_keys.get(&p.id))
                    .cloned()
                    .unwrap_or_default();

                if api_key.is_empty() {
                    info!("No API key for cloud TTS, skipping audio playback");
                } else {
                    match self.generate_tts_audio(text, &api_key).await {
                        Ok(audio_data) => {
                            if let Err(e) = self.play_audio(&audio_data) {
                                error!("Failed to play cloud TTS audio: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Cloud TTS failed: {}", e);
                        }
                    }
                }
            }
        }

        self.emit_current_state("idle", None);
        Ok(())
    }

    /// Generate TTS audio using OpenAI-compatible API
    async fn generate_tts_audio(&self, text: &str, api_key: &str) -> Result<Vec<u8>, String> {
        let client = reqwest::Client::new();

        // OpenAI TTS endpoint
        let response = client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": "tts-1",
                "input": text,
                "voice": "nova", // Options: alloy, echo, fable, onyx, nova, shimmer
                "response_format": "mp3"
            }))
            .send()
            .await
            .map_err(|e| format!("TTS request failed: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("TTS API error: {}", error_text));
        }

        response
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read TTS response: {}", e))
    }

    /// Play audio data
    fn play_audio(&self, audio_data: &[u8]) -> Result<(), String> {
        let settings = get_settings(&self.app_handle);
        let volume = settings.audio_feedback_volume;

        let stream_builder = OutputStreamBuilder::from_default_device()
            .map_err(|e| format!("Failed to get audio output: {}", e))?;

        let stream_handle = stream_builder
            .open_stream()
            .map_err(|e| format!("Failed to open audio stream: {}", e))?;

        let mixer = stream_handle.mixer();

        // Create a cursor to read the audio data
        let cursor = Cursor::new(audio_data.to_vec());

        let sink =
            rodio::play(mixer, cursor).map_err(|e| format!("Failed to play audio: {}", e))?;

        sink.set_volume(volume);
        sink.sleep_until_end();

        Ok(())
    }

    /// Clear conversation history
    pub fn clear_history(&self) {
        let mut history = self.conversation_history.lock().unwrap();
        history.clear();
        info!("Onichan conversation history cleared");
    }

    /// Get conversation history
    pub fn get_history(&self) -> Vec<ConversationMessage> {
        self.conversation_history.lock().unwrap().clone()
    }

    fn emit_current_state(&self, status: &str, message: Option<String>) {
        let _ = self.app_handle.emit(
            "onichan-state",
            OnichanState {
                status: status.to_string(),
                message,
                mode: self.get_mode(),
                local_llm_loaded: self.is_local_llm_loaded(),
                local_tts_loaded: self.is_local_tts_loaded(),
            },
        );
    }
}
