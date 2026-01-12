//! Memory Sidecar - Semantic memory storage for Discord conversations
//!
//! Provides embedding generation and vector search over stdin/stdout JSON IPC.
//! Follows the same pattern as llm-sidecar and tts-sidecar.

mod embeddings;
mod vector_store;

#[allow(unused_imports)]
use anyhow::Result;
use embeddings::{EmbeddingModel, EmbeddingModelInfo};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use tokio::runtime::Runtime;
use uuid::Uuid;
use vector_store::{MemoryEntry, VectorStore};

/// Request types from the main app
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "store")]
    Store {
        user_id: String,
        content: String,
        is_bot: bool,
    },
    #[serde(rename = "query")]
    Query {
        user_id: String,
        text: String,
        limit: usize,
    },
    #[serde(rename = "query_all")]
    QueryAll { text: String, limit: usize },
    #[serde(rename = "browse_recent")]
    BrowseRecent {
        limit: usize,
        user_id: Option<String>,
        is_bot: Option<bool>,
    },
    #[serde(rename = "list_users")]
    ListUsers,
    #[serde(rename = "count")]
    Count,
    #[serde(rename = "clear_all")]
    ClearAll,
    #[serde(rename = "cleanup")]
    Cleanup { ttl_days: u32 },
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "list_models")]
    ListModels,
    #[serde(rename = "load_model")]
    LoadModel { model_id: String },
    #[serde(rename = "get_current_model")]
    GetCurrentModel,
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// Embedding model info for API responses
#[derive(Debug, Serialize)]
struct EmbeddingModelInfoResponse {
    id: String,
    name: String,
    description: String,
    dimension: usize,
    size_mb: u32,
    is_downloaded: bool,
    is_loaded: bool,
}

/// User info with memory count
#[derive(Debug, Serialize)]
struct UserInfo {
    user_id: String,
    memory_count: usize,
}

/// Response types to the main app
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum Response {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "stored")]
    Stored { id: String },
    #[serde(rename = "results")]
    Results { messages: Vec<MemoryEntry> },
    #[serde(rename = "users")]
    Users { users: Vec<UserInfo> },
    #[serde(rename = "count")]
    Count { total: usize },
    #[serde(rename = "cleared")]
    Cleared { deleted: u32 },
    #[serde(rename = "cleanup_done")]
    CleanupDone { deleted: u32 },
    #[serde(rename = "status")]
    Status {
        ready: bool,
        model_loaded: bool,
        current_model_id: Option<String>,
    },
    #[serde(rename = "models")]
    Models {
        models: Vec<EmbeddingModelInfoResponse>,
    },
    #[serde(rename = "current_model")]
    CurrentModel { model: Option<EmbeddingModelInfoResponse> },
    #[serde(rename = "model_loaded")]
    ModelLoaded { model_id: String },
    #[serde(rename = "error")]
    Error { message: String },
}

fn send_response(response: &Response) {
    let json = serde_json::to_string(response).unwrap();
    println!("{}", json);
    io::stdout().flush().unwrap();
}

fn get_data_dir() -> PathBuf {
    // Use standard app data directory
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("onichan_memory")
}

fn model_info_to_response(info: &EmbeddingModelInfo, current_model_id: Option<&str>) -> EmbeddingModelInfoResponse {
    EmbeddingModelInfoResponse {
        id: info.id.clone(),
        name: info.name.clone(),
        description: info.description.clone(),
        dimension: info.dimension,
        size_mb: info.size_mb,
        is_downloaded: EmbeddingModel::is_downloaded(&info.id),
        is_loaded: current_model_id == Some(&info.id),
    }
}

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    info!("Memory sidecar starting...");

    // Create tokio runtime for async operations
    let rt = Runtime::new().expect("Failed to create tokio runtime");

    // Initialize embedding model with default
    let mut model: Option<EmbeddingModel> = match EmbeddingModel::load_default() {
        Ok(m) => {
            info!("Default embedding model loaded successfully");
            Some(m)
        }
        Err(e) => {
            warn!("Failed to load default embedding model: {}. Continuing without model.", e);
            None
        }
    };

    // Initialize vector store
    let db_path = get_data_dir().join("lancedb");
    let mut store = match rt.block_on(VectorStore::open(&db_path)) {
        Ok(s) => {
            info!("Vector store opened at: {:?}", db_path);
            s
        }
        Err(e) => {
            error!("Failed to open vector store: {}", e);
            send_response(&Response::Error {
                message: format!("Failed to open vector store: {}", e),
            });
            return;
        }
    };

    // Send ready message
    send_response(&Response::Ok {
        message: "Memory sidecar ready".to_string(),
    });

    // Process requests
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to read stdin: {}", e);
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
                    message: format!("Invalid request: {}", e),
                });
                continue;
            }
        };

        match request {
            Request::Store {
                user_id,
                content,
                is_bot,
            } => {
                let current_model = match &mut model {
                    Some(m) => m,
                    None => {
                        send_response(&Response::Error {
                            message: "No embedding model loaded".to_string(),
                        });
                        continue;
                    }
                };

                let id = Uuid::new_v4().to_string();

                // Generate embedding
                let embedding = match current_model.embed(&content) {
                    Ok(e) => e,
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Embedding failed: {}", e),
                        });
                        continue;
                    }
                };

                // Store in vector database
                match rt.block_on(store.store(&id, &user_id, &content, &embedding, is_bot)) {
                    Ok(_) => {
                        info!("Stored memory {} for user {}", id, user_id);
                        send_response(&Response::Stored { id });
                    }
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Storage failed: {}", e),
                        });
                    }
                }
            }

            Request::Query {
                user_id,
                text,
                limit,
            } => {
                let current_model = match &mut model {
                    Some(m) => m,
                    None => {
                        send_response(&Response::Error {
                            message: "No embedding model loaded".to_string(),
                        });
                        continue;
                    }
                };

                // Generate embedding for query
                let embedding = match current_model.embed(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Embedding failed: {}", e),
                        });
                        continue;
                    }
                };

                // Search vector database
                match rt.block_on(store.query(&user_id, &embedding, limit)) {
                    Ok(messages) => {
                        info!(
                            "Query for user {} returned {} results",
                            user_id,
                            messages.len()
                        );
                        send_response(&Response::Results { messages });
                    }
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Query failed: {}", e),
                        });
                    }
                }
            }

            Request::QueryAll { text, limit } => {
                let current_model = match &mut model {
                    Some(m) => m,
                    None => {
                        send_response(&Response::Error {
                            message: "No embedding model loaded".to_string(),
                        });
                        continue;
                    }
                };

                // Generate embedding for query
                let embedding = match current_model.embed(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Embedding failed: {}", e),
                        });
                        continue;
                    }
                };

                // Search vector database (all users)
                match rt.block_on(store.query_all(&embedding, limit)) {
                    Ok(messages) => {
                        info!("Query all returned {} results", messages.len());
                        send_response(&Response::Results { messages });
                    }
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Query failed: {}", e),
                        });
                    }
                }
            }

            Request::BrowseRecent { limit, user_id, is_bot } => {
                // Browse recent memories without semantic search
                match rt.block_on(store.browse_recent(limit, user_id.as_deref(), is_bot)) {
                    Ok(messages) => {
                        info!("Browse recent returned {} results", messages.len());
                        send_response(&Response::Results { messages });
                    }
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("Browse failed: {}", e),
                        });
                    }
                }
            }

            Request::ListUsers => {
                match rt.block_on(store.list_users()) {
                    Ok(user_data) => {
                        let users: Vec<UserInfo> = user_data
                            .into_iter()
                            .map(|(user_id, memory_count)| UserInfo { user_id, memory_count })
                            .collect();
                        info!("Listed {} unique users", users.len());
                        send_response(&Response::Users { users });
                    }
                    Err(e) => {
                        send_response(&Response::Error {
                            message: format!("List users failed: {}", e),
                        });
                    }
                }
            }

            Request::Count => match rt.block_on(store.count_all()) {
                Ok(total) => {
                    info!("Total memories: {}", total);
                    send_response(&Response::Count { total });
                }
                Err(e) => {
                    send_response(&Response::Error {
                        message: format!("Count failed: {}", e),
                    });
                }
            },

            Request::ClearAll => match rt.block_on(store.clear_all()) {
                Ok(deleted) => {
                    info!("Cleared {} memories", deleted);
                    send_response(&Response::Cleared { deleted });
                }
                Err(e) => {
                    send_response(&Response::Error {
                        message: format!("Clear failed: {}", e),
                    });
                }
            },

            Request::Cleanup { ttl_days } => match rt.block_on(store.cleanup(ttl_days)) {
                Ok(deleted) => {
                    info!("Cleanup deleted {} old memories", deleted);
                    send_response(&Response::CleanupDone { deleted });
                }
                Err(e) => {
                    send_response(&Response::Error {
                        message: format!("Cleanup failed: {}", e),
                    });
                }
            },

            Request::Status => {
                let current_model_id = model.as_ref().map(|m| m.info().id.clone());
                send_response(&Response::Status {
                    ready: true,
                    model_loaded: model.is_some(),
                    current_model_id,
                });
            }

            Request::ListModels => {
                let current_model_id = model.as_ref().map(|m| m.info().id.as_str());
                let models: Vec<EmbeddingModelInfoResponse> = EmbeddingModelInfo::available_models()
                    .iter()
                    .map(|info| model_info_to_response(info, current_model_id))
                    .collect();
                send_response(&Response::Models { models });
            }

            Request::LoadModel { model_id } => {
                info!("Loading embedding model: {}", model_id);
                match EmbeddingModel::load(&model_id) {
                    Ok(new_model) => {
                        info!("Successfully loaded model: {}", model_id);
                        model = Some(new_model);
                        send_response(&Response::ModelLoaded { model_id });
                    }
                    Err(e) => {
                        error!("Failed to load model {}: {}", model_id, e);
                        send_response(&Response::Error {
                            message: format!("Failed to load model: {}", e),
                        });
                    }
                }
            }

            Request::GetCurrentModel => {
                let current_model_info = model.as_ref().map(|m| {
                    model_info_to_response(m.info(), Some(&m.info().id))
                });
                send_response(&Response::CurrentModel { model: current_model_info });
            }

            Request::Shutdown => {
                info!("Shutdown requested");
                send_response(&Response::Ok {
                    message: "Shutting down".to_string(),
                });
                break;
            }
        }
    }

    info!("Memory sidecar exiting");
}
