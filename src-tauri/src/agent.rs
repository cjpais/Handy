use chrono::Local;
use reqwest::multipart;
use serde::Serialize;
use serde_json::json;
use specta::Type;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionStatus {
    Idle,
    Running,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResult {
    pub tool_name: String,
    pub output: String,
}

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSnapshot {
    pub status: AgentSessionStatus,
    pub last_tool_result: Option<AgentToolResult>,
}

#[derive(Default)]
pub struct AgentManager {
    state: Mutex<AgentState>,
}

#[derive(Default)]
struct AgentState {
    running: bool,
    last_tool_result: Option<AgentToolResult>,
}

impl AgentManager {
    fn snapshot(&self) -> AgentSessionSnapshot {
        let state = self.state.lock().expect("agent state lock poisoned");
        AgentSessionSnapshot {
            status: if state.running {
                AgentSessionStatus::Running
            } else {
                AgentSessionStatus::Idle
            },
            last_tool_result: state.last_tool_result.clone(),
        }
    }

    fn set_running(&self, running: bool) -> AgentSessionSnapshot {
        let mut state = self.state.lock().expect("agent state lock poisoned");
        state.running = running;
        AgentSessionSnapshot {
            status: if state.running {
                AgentSessionStatus::Running
            } else {
                AgentSessionStatus::Idle
            },
            last_tool_result: state.last_tool_result.clone(),
        }
    }

    fn toggle_running(&self) -> AgentSessionSnapshot {
        let mut state = self.state.lock().expect("agent state lock poisoned");
        state.running = !state.running;
        AgentSessionSnapshot {
            status: if state.running {
                AgentSessionStatus::Running
            } else {
                AgentSessionStatus::Idle
            },
            last_tool_result: state.last_tool_result.clone(),
        }
    }

    fn set_tool_result(&self, result: AgentToolResult) -> AgentSessionSnapshot {
        let mut state = self.state.lock().expect("agent state lock poisoned");
        state.last_tool_result = Some(result);
        AgentSessionSnapshot {
            status: if state.running {
                AgentSessionStatus::Running
            } else {
                AgentSessionStatus::Idle
            },
            last_tool_result: state.last_tool_result.clone(),
        }
    }
}

fn emit_agent_state(app: &AppHandle, snapshot: &AgentSessionSnapshot) {
    if let Err(error) = app.emit("agent-session-changed", snapshot) {
        log::warn!("Failed to emit agent-session-changed event: {}", error);
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_session(app: AppHandle) -> Result<AgentSessionSnapshot, String> {
    let manager = app.state::<AgentManager>();
    Ok(manager.snapshot())
}

#[tauri::command]
#[specta::specta]
pub fn start_agent_session(app: AppHandle) -> Result<AgentSessionSnapshot, String> {
    let manager = app.state::<AgentManager>();
    let snapshot = manager.set_running(true);
    emit_agent_state(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn stop_agent_session(app: AppHandle) -> Result<AgentSessionSnapshot, String> {
    let manager = app.state::<AgentManager>();
    let snapshot = manager.set_running(false);
    emit_agent_state(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn toggle_agent_session(app: AppHandle) -> Result<AgentSessionSnapshot, String> {
    let manager = app.state::<AgentManager>();
    let snapshot = manager.toggle_running();
    emit_agent_state(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub async fn create_agent_realtime_call(app: AppHandle, sdp: String) -> Result<String, String> {
    let api_key = crate::agent_config::get_config_value(&app, "OPENAI_API_KEY")
        .ok_or_else(|| "OPENAI_API_KEY is required to start the voice agent".to_string())?;
    let model = crate::agent_config::get_config_value(&app, "OPENAI_REALTIME_MODEL")
        .unwrap_or_else(|| "gpt-realtime".to_string());

    let session = json!({
        "type": "realtime",
        "model": model,
        "output_modalities": ["audio"],
        "audio": {
            "output": {
                "voice": "marin"
            }
        }
    });

    let form = multipart::Form::new()
        .text("sdp", sdp)
        .text("session", session.to_string());

    let response = reqwest::Client::new()
        .post("https://api.openai.com/v1/realtime/calls")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("Failed to create Realtime call: {}", error))?;

    let status = response.status();
    let answer_sdp = response
        .text()
        .await
        .map_err(|error| format!("Failed to read Realtime call response: {}", error))?;

    if !status.is_success() {
        return Err(format!(
            "Realtime call failed with status {}: {}",
            status, answer_sdp
        ));
    }

    Ok(answer_sdp)
}

#[tauri::command]
#[specta::specta]
pub fn log_agent_runtime_event(message: String) {
    log::info!("Agent runtime: {}", message);
}

#[tauri::command]
#[specta::specta]
pub fn run_agent_test_tool(app: AppHandle) -> Result<AgentSessionSnapshot, String> {
    let manager = app.state::<AgentManager>();
    let result = AgentToolResult {
        tool_name: "get_current_time".to_string(),
        output: Local::now().to_rfc3339(),
    };
    let snapshot = manager.set_tool_result(result);
    emit_agent_state(&app, &snapshot);
    Ok(snapshot)
}
