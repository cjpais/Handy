//! LLM Sidecar Process
//!
//! This is a separate process that handles LLM inference to avoid GGML symbol
//! conflicts with whisper-rs in the main application.
//!
//! Communication is via JSON over stdin/stdout:
//! - Requests are JSON objects on stdin (one per line)
//! - Responses are JSON objects on stdout (one per line)

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "load")]
    Load { model_path: String },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "chat")]
    Chat {
        system_prompt: String,
        user_message: String,
        max_tokens: u32,
    },
    #[serde(rename = "generate")]
    Generate { prompt: String, max_tokens: u32 },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum Response {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "result")]
    Result { text: String },
    #[serde(rename = "status")]
    Status { loaded: bool, model_path: Option<String> },
}

struct LlmState {
    backend: Arc<LlamaBackend>,
    model: Option<Arc<LlamaModel>>,
    model_path: Option<String>,
}

impl LlmState {
    fn new() -> Result<Self, String> {
        let backend = LlamaBackend::init()
            .map_err(|e| format!("Failed to init llama backend: {}", e))?;

        Ok(Self {
            backend: Arc::new(backend),
            model: None,
            model_path: None,
        })
    }

    fn load_model(&mut self, model_path: &str) -> Result<(), String> {
        let path = Path::new(model_path);

        if !path.exists() {
            return Err(format!("Model file does not exist: {}", model_path));
        }

        // Don't reload if same model
        if self.model_path.as_deref() == Some(model_path) && self.model.is_some() {
            return Ok(());
        }

        // Unload existing model first
        self.model = None;
        self.model_path = None;

        // Use default params - Metal will be enabled if feature is on
        let model_params = LlamaModelParams::default();

        let model = LlamaModel::load_from_file(&self.backend, path, &model_params)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        self.model = Some(Arc::new(model));
        self.model_path = Some(model_path.to_string());

        Ok(())
    }

    fn unload_model(&mut self) {
        self.model = None;
        self.model_path = None;
    }

    fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        self.generate_internal(prompt, max_tokens, true)
    }

    fn generate_internal(&self, prompt: &str, max_tokens: u32, add_bos: bool) -> Result<String, String> {
        let model = self.model.as_ref()
            .ok_or_else(|| "No model loaded".to_string())?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(4096))
            .with_n_batch(512);

        let mut ctx = model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| format!("Failed to create context: {}", e))?;

        // Tokenize the prompt - don't add BOS if the prompt already has one (like chat format)
        let bos_setting = if add_bos {
            llama_cpp_2::model::AddBos::Always
        } else {
            llama_cpp_2::model::AddBos::Never
        };
        let tokens = model
            .str_to_token(prompt, bos_setting)
            .map_err(|e| format!("Failed to tokenize: {}", e))?;

        // Create batch for initial prompt
        let mut batch = LlamaBatch::new(tokens.len(), 1);

        // Add tokens to batch
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == tokens.len() - 1)
                .map_err(|e| format!("Failed to add token to batch: {}", e))?;
        }

        // Decode the prompt
        ctx.decode(&mut batch)
            .map_err(|e| format!("Failed to decode prompt: {}", e))?;

        // Create a sampler chain with parameters tuned for varied, creative responses
        // Order matters: penalties first, then temperature, then top-p, then final selection
        let mut sampler = LlamaSampler::chain_simple([
            // Repetition penalties - strong settings to prevent repetitive phrases
            // penalty_last_n=128: consider last 128 tokens (covers more context)
            // penalty_repeat=1.3: repeated tokens are 1.3x less likely (stronger penalty)
            // penalty_freq=0.1: slight frequency penalty for commonly used tokens
            // penalty_present=0.1: slight penalty for tokens already present
            LlamaSampler::penalties(128, 1.3, 0.1, 0.1),
            // Temperature for creativity (0.9 = more varied and creative)
            LlamaSampler::temp(0.9),
            // Top-p (nucleus) sampling - consider tokens in top 92% probability mass
            LlamaSampler::top_p(0.92, 1),
            // Final token selection with time-based seed for variation
            LlamaSampler::dist(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u32)
                .unwrap_or(42)),
        ]);

        // Generate tokens
        let mut output_tokens = Vec::new();
        let mut n_cur = tokens.len();

        for _ in 0..max_tokens {
            // Sample the next token
            let new_token_id = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(new_token_id);

            // Check for end of generation
            if model.is_eog_token(new_token_id) {
                break;
            }

            output_tokens.push(new_token_id);

            // Prepare next batch
            batch.clear();
            batch
                .add(new_token_id, n_cur as i32, &[0], true)
                .map_err(|e| format!("Failed to add token: {}", e))?;

            n_cur += 1;

            // Decode next token
            ctx.decode(&mut batch)
                .map_err(|e| format!("Failed to decode: {}", e))?;
        }

        // Convert tokens to string
        let mut output = String::new();
        for token in output_tokens {
            match model.token_to_str(token, llama_cpp_2::model::Special::Tokenize) {
                Ok(piece) => output.push_str(&piece),
                Err(e) => {
                    // Log but don't fail on UTF-8 conversion errors (can happen with partial tokens)
                    log::warn!("Token conversion warning: {}", e);
                }
            }
        }

        Ok(output.trim().to_string())
    }

    fn chat(&self, system_prompt: &str, user_message: &str, max_tokens: u32) -> Result<String, String> {
        // Detect model type from path to use correct chat format
        let model_path = self.model_path.as_deref().unwrap_or("");
        let model_lower = model_path.to_lowercase();

        log::info!("Chat request - model: {}", model_path);
        log::debug!("System prompt: {}", system_prompt);
        log::debug!("User message: {}", user_message);

        let (prompt, add_bos) = if model_lower.contains("mistral") || model_lower.contains("mixtral") {
            // Mistral/Mixtral format: [INST] ... [/INST]
            let prompt = format!(
                "<s>[INST] {}\n\n{} [/INST]",
                system_prompt, user_message
            );
            (prompt, false) // <s> is BOS
        } else if model_lower.contains("dolphin") || model_lower.contains("chatml") {
            // ChatML format (used by Dolphin and many fine-tunes)
            let prompt = format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_prompt, user_message
            );
            (prompt, true)
        } else if model_lower.contains("qwen") {
            // Qwen uses ChatML-like format
            let prompt = format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_prompt, user_message
            );
            (prompt, true)
        } else if model_lower.contains("gpt-oss") || model_lower.contains("heretic") {
            // GPT-oss / OpenAI Harmony format (specific models only)
            let prompt = format!(
                "<|start|>developer<|message|>\n{}\n<|end|>\n<|start|>user<|message|>\n{}\n<|end|>\n<|start|>assistant<|channel|>final<|message|>\n",
                system_prompt, user_message
            );
            (prompt, true)
        } else if model_lower.contains("openai-") || model_lower.contains("neoplus") || model_lower.contains("neo-") || model_lower.contains("brainstorm") || model_lower.contains("brains") || model_lower.contains("uncensored") {
            // DavidAU's OpenAI/NEO models and other uncensored models use ChatML format
            log::info!("Using ChatML format for OpenAI/NEO/uncensored model");
            let prompt = format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                system_prompt, user_message
            );
            (prompt, true)
        } else {
            // Default: Llama 3 format
            // Note: <|begin_of_text|> is the BOS token, so we don't add another BOS during tokenization
            let prompt = format!(
                "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                system_prompt, user_message
            );
            (prompt, false)
        };

        log::info!("Using prompt template, length: {} chars", prompt.len());
        self.generate_internal(&prompt, max_tokens, add_bos)
    }
}

fn send_response(response: &Response) {
    let json = serde_json::to_string(response).unwrap();
    println!("{}", json);
    io::stdout().flush().unwrap();
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    log::info!("LLM sidecar starting...");

    let mut state = match LlmState::new() {
        Ok(s) => {
            log::info!("LLM backend initialized");
            s
        }
        Err(e) => {
            send_response(&Response::Error {
                message: format!("Failed to initialize: {}", e),
            });
            std::process::exit(1);
        }
    };

    // Signal ready
    send_response(&Response::Ok {
        message: "LLM sidecar ready".to_string(),
    });

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log::error!("Failed to read stdin: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                send_response(&Response::Error {
                    message: format!("Invalid JSON: {}", e),
                });
                continue;
            }
        };

        match request {
            Request::Load { model_path } => {
                log::info!("Loading model: {}", model_path);
                match state.load_model(&model_path) {
                    Ok(()) => {
                        log::info!("Model loaded successfully");
                        send_response(&Response::Ok {
                            message: "Model loaded".to_string(),
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to load model: {}", e);
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Unload => {
                log::info!("Unloading model");
                state.unload_model();
                send_response(&Response::Ok {
                    message: "Model unloaded".to_string(),
                });
            }
            Request::Chat {
                system_prompt,
                user_message,
                max_tokens,
            } => {
                log::info!("Chat request");
                match state.chat(&system_prompt, &user_message, max_tokens) {
                    Ok(text) => {
                        send_response(&Response::Result { text });
                    }
                    Err(e) => {
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Generate { prompt, max_tokens } => {
                log::info!("Generate request");
                match state.generate(&prompt, max_tokens) {
                    Ok(text) => {
                        send_response(&Response::Result { text });
                    }
                    Err(e) => {
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Status => {
                send_response(&Response::Status {
                    loaded: state.is_loaded(),
                    model_path: state.model_path.clone(),
                });
            }
            Request::Shutdown => {
                log::info!("Shutdown requested");
                send_response(&Response::Ok {
                    message: "Shutting down".to_string(),
                });
                break;
            }
        }
    }

    log::info!("LLM sidecar exiting");
}
