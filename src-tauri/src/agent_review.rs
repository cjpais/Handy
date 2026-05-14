use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const RELATION_SELECTION_ERROR_PREFIX: &str = "AGENT_RELATION_SELECTION:";

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentReviewStatus {
    Pending,
    Approved,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentReviewRequest {
    pub id: String,
    pub title: String,
    pub action_name: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub status: AgentReviewStatus,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub resolution_json: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolOverlay {
    pub id: String,
    pub title: String,
    pub tool_name: String,
    pub result_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationCandidate {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRelationSelection {
    pub property_name: String,
    pub query: String,
    pub message: String,
    pub candidates: Vec<AgentRelationCandidate>,
}

#[derive(Default)]
pub struct AgentReviewManager {
    pending: Mutex<Option<AgentReviewRequest>>,
    tool_overlay: Mutex<Option<AgentToolOverlay>>,
}

impl AgentReviewManager {
    fn get(&self) -> Option<AgentReviewRequest> {
        self.pending
            .lock()
            .expect("agent review lock poisoned")
            .clone()
    }

    fn set(&self, request: AgentReviewRequest) {
        *self.pending.lock().expect("agent review lock poisoned") = Some(request);
    }

    fn clear(&self) {
        *self.pending.lock().expect("agent review lock poisoned") = None;
    }

    fn get_tool_overlay(&self) -> Option<AgentToolOverlay> {
        self.tool_overlay
            .lock()
            .expect("agent tool overlay lock poisoned")
            .clone()
    }

    fn set_tool_overlay(&self, overlay: AgentToolOverlay) {
        *self
            .tool_overlay
            .lock()
            .expect("agent tool overlay lock poisoned") = Some(overlay);
    }

    fn clear_tool_overlay(&self) {
        *self
            .tool_overlay
            .lock()
            .expect("agent tool overlay lock poisoned") = None;
    }
}

fn emit_review(app: &AppHandle, request: &AgentReviewRequest) {
    if let Err(error) = app.emit("agent-review-updated", request) {
        log::warn!("Failed to emit agent-review-updated: {}", error);
    }
}

fn emit_tool_overlay(app: &AppHandle, overlay: Option<AgentToolOverlay>) {
    if let Err(error) = app.emit("agent-tool-overlay-updated", overlay) {
        log::warn!("Failed to emit agent-tool-overlay-updated: {}", error);
    }
}

fn tool_overlay_title(tool_name: &str) -> String {
    match tool_name {
        "notion_search" => "Notion results",
        "gmail_search" => "Email results",
        "calendar_list_events" => "Calendar",
        "calendar_check_availability" => "Availability",
        "granola_search_notes" => "Granola notes",
        "gmail_create_draft" => "Email draft",
        _ => "Tool result",
    }
    .to_string()
}

pub fn relation_selection_error(selection: AgentRelationSelection) -> String {
    format!(
        "{}{}",
        RELATION_SELECTION_ERROR_PREFIX,
        serde_json::to_string(&selection).unwrap_or_else(|_| "{}".to_string())
    )
}

fn relation_selection_from_error(error: &str) -> Option<String> {
    error
        .strip_prefix(RELATION_SELECTION_ERROR_PREFIX)
        .map(|json| json.to_string())
}

pub fn show_agent_tool_overlay(app: AppHandle, tool_name: &str, result_json: String) {
    if matches!(tool_name, "notion_create_page") {
        return;
    }

    let manager = app.state::<AgentReviewManager>();
    let overlay = AgentToolOverlay {
        id: format!("tool-{}", chrono::Utc::now().timestamp_millis()),
        title: tool_overlay_title(tool_name),
        tool_name: tool_name.to_string(),
        result_json,
    };
    manager.set_tool_overlay(overlay.clone());
    emit_tool_overlay(&app, Some(overlay));
    crate::utils::show_agent_review_overlay(&app);
}

fn build_notion_page_arguments(
    arguments: &Value,
    page_type: &str,
    table_target: Option<String>,
) -> Value {
    let mut fields = arguments.clone();
    if let Some(field_object) = fields.as_object_mut() {
        field_object
            .entry("ownerName")
            .or_insert_with(|| Value::String("Jason Walkow".to_string()));
    }

    let primary_name = fields
        .get("dealName")
        .and_then(Value::as_str)
        .or_else(|| fields.get("company").and_then(Value::as_str))
        .or_else(|| fields.get("accountName").and_then(Value::as_str))
        .or_else(|| fields.get("contactName").and_then(Value::as_str))
        .unwrap_or("Untitled");

    let title = format!("{}: {}", page_type, primary_name);
    let content = serde_json::to_string_pretty(&fields).unwrap_or_else(|_| fields.to_string());

    let mut payload = json!({
        "pages": [
            {
                "properties": {
                    "title": title
                },
                "content": content
            }
        ]
    });

    if let Some(table_target) = table_target.filter(|value| !value.trim().is_empty()) {
        payload["parent"] = json!({
            "type": "data_source_id",
            "data_source_id": table_target.trim()
        });
    }

    payload
}

fn string_field<'a>(fields: &'a Value, key: &str) -> Option<&'a str> {
    fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn amount_number(value: &str) -> Option<Value> {
    let numeric = value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    numeric
        .parse::<f64>()
        .ok()
        .filter(|amount| amount.is_finite())
        .map(Value::from)
}

fn build_notion_deal_arguments(arguments: &Value, table_target: String) -> Value {
    let mut fields = arguments.clone();
    if let Some(field_object) = fields.as_object_mut() {
        field_object
            .entry("ownerName")
            .or_insert_with(|| Value::String("Jason Walkow".to_string()));
    }

    let title = string_field(&fields, "dealName")
        .or_else(|| string_field(&fields, "company"))
        .unwrap_or("Untitled Deal");
    let mut properties = serde_json::Map::new();
    properties.insert("Name".to_string(), Value::String(title.to_string()));

    if let Some(company) = string_field(&fields, "company") {
        properties.insert("Client".to_string(), Value::String(company.to_string()));
    }
    if let Some(contact_name) = string_field(&fields, "contactName") {
        properties.insert(
            "Contacts".to_string(),
            Value::String(contact_name.to_string()),
        );
    }
    if let Some(owner_name) = string_field(&fields, "ownerName") {
        properties.insert(
            "Relationship Owner".to_string(),
            Value::String(owner_name.to_string()),
        );
    }
    if let Some(stage) = string_field(&fields, "stage") {
        properties.insert("Stage".to_string(), Value::String(stage.to_string()));
    }
    if let Some(amount) = string_field(&fields, "amount") {
        properties.insert(
            "Deal Amount".to_string(),
            amount_number(amount).unwrap_or_else(|| Value::String(amount.to_string())),
        );
    }

    json!({
        "pages": [
            {
                "properties": Value::Object(properties),
                "content": serde_json::to_string_pretty(&fields).unwrap_or_else(|_| fields.to_string())
            }
        ],
        "parent": {
            "type": "data_source_id",
            "data_source_id": table_target.trim()
        }
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_review(app: AppHandle) -> Result<Option<AgentReviewRequest>, String> {
    Ok(app.state::<AgentReviewManager>().get())
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_tool_overlay(app: AppHandle) -> Result<Option<AgentToolOverlay>, String> {
    Ok(app.state::<AgentReviewManager>().get_tool_overlay())
}

#[tauri::command]
#[specta::specta]
pub fn clear_agent_tool_overlay(app: AppHandle) -> Result<(), String> {
    let manager = app.state::<AgentReviewManager>();
    manager.clear_tool_overlay();
    emit_tool_overlay(&app, None);
    if manager.get().is_none() {
        crate::utils::hide_agent_review_overlay(&app);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn propose_notion_lead(
    app: AppHandle,
    arguments_json: String,
) -> Result<AgentReviewRequest, String> {
    let arguments: Value = serde_json::from_str(&arguments_json)
        .map_err(|error| format!("Invalid Notion lead JSON: {}", error))?;
    let table_target =
        crate::agent_config::get_config_value(&app, crate::agent_config::NOTION_LEADS_TABLE_TARGET)
            .ok_or_else(|| "Allowed Notion table is not configured: Leads".to_string())?;
    let request = AgentReviewRequest {
        id: format!("review-{}", chrono::Utc::now().timestamp_millis()),
        title: "Create Notion lead".to_string(),
        action_name: "notion_lead".to_string(),
        tool_name: "notion_create_page".to_string(),
        arguments_json: build_notion_page_arguments(&arguments, "Lead", Some(table_target))
            .to_string(),
        status: AgentReviewStatus::Pending,
        result_json: None,
        error: None,
        resolution_json: None,
    };

    app.state::<AgentReviewManager>().set(request.clone());
    app.state::<AgentReviewManager>().clear_tool_overlay();
    emit_tool_overlay(&app, None);
    emit_review(&app, &request);
    crate::utils::show_agent_review_overlay(&app);
    Ok(request)
}

#[tauri::command]
#[specta::specta]
pub fn propose_notion_deal(
    app: AppHandle,
    arguments_json: String,
) -> Result<AgentReviewRequest, String> {
    let arguments: Value = serde_json::from_str(&arguments_json)
        .map_err(|error| format!("Invalid Notion deal JSON: {}", error))?;
    let table_target =
        crate::agent_config::get_config_value(&app, crate::agent_config::NOTION_DEALS_TABLE_TARGET)
            .ok_or_else(|| "Allowed Notion table is not configured: Deals".to_string())?;
    let request = AgentReviewRequest {
        id: format!("review-{}", chrono::Utc::now().timestamp_millis()),
        title: "Create Notion deal".to_string(),
        action_name: "notion_deal".to_string(),
        tool_name: "notion_create_page".to_string(),
        arguments_json: build_notion_deal_arguments(&arguments, table_target).to_string(),
        status: AgentReviewStatus::Pending,
        result_json: None,
        error: None,
        resolution_json: None,
    };

    app.state::<AgentReviewManager>().set(request.clone());
    app.state::<AgentReviewManager>().clear_tool_overlay();
    emit_tool_overlay(&app, None);
    emit_review(&app, &request);
    crate::utils::show_agent_review_overlay(&app);
    Ok(request)
}

#[tauri::command]
#[specta::specta]
pub fn cancel_agent_review(app: AppHandle) -> Result<AgentReviewRequest, String> {
    let manager = app.state::<AgentReviewManager>();
    let mut request = manager
        .get()
        .ok_or_else(|| "No pending agent review".to_string())?;
    request.status = AgentReviewStatus::Cancelled;
    manager.clear();
    emit_review(&app, &request);
    if manager.get_tool_overlay().is_none() {
        crate::utils::hide_agent_review_overlay(&app);
    }
    Ok(request)
}

#[tauri::command]
#[specta::specta]
pub fn select_agent_review_relation(
    app: AppHandle,
    property_name: String,
    url: String,
) -> Result<AgentReviewRequest, String> {
    let url = url.trim().to_string();
    if !url.starts_with("https://www.notion.so/") {
        return Err("Relation selection must be a Notion page URL".to_string());
    }

    let manager = app.state::<AgentReviewManager>();
    let mut request = manager
        .get()
        .ok_or_else(|| "No pending agent review".to_string())?;
    let mut arguments: Value = serde_json::from_str(&request.arguments_json)
        .map_err(|error| format!("Invalid review arguments JSON: {}", error))?;

    let properties = arguments
        .get_mut("pages")
        .and_then(Value::as_array_mut)
        .and_then(|pages| pages.first_mut())
        .and_then(|page| page.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "Review does not include editable Notion properties".to_string())?;
    properties.insert(property_name, Value::String(url));

    request.arguments_json = arguments.to_string();
    request.status = AgentReviewStatus::Pending;
    request.error = None;
    request.resolution_json = None;
    manager.set(request.clone());
    emit_review(&app, &request);
    crate::utils::show_agent_review_overlay(&app);
    Ok(request)
}

#[tauri::command]
#[specta::specta]
pub async fn approve_agent_review(app: AppHandle) -> Result<AgentReviewRequest, String> {
    let manager = app.state::<AgentReviewManager>();
    let mut request = manager
        .get()
        .ok_or_else(|| "No pending agent review".to_string())?;
    let result = crate::agent_connections::run_agent_connection_tool(
        app.clone(),
        request.tool_name.clone(),
        request.arguments_json.clone(),
    )
    .await;

    match result {
        Ok(result_json) => {
            request.status = AgentReviewStatus::Approved;
            request.result_json = Some(result_json);
            request.error = None;
            request.resolution_json = None;
            manager.clear();
            if manager.get_tool_overlay().is_none() {
                crate::utils::hide_agent_review_overlay(&app);
            }
        }
        Err(error) => {
            if let Some(resolution_json) = relation_selection_from_error(&error) {
                request.status = AgentReviewStatus::Pending;
                request.error = None;
                request.resolution_json = Some(resolution_json);
            } else {
                request.status = AgentReviewStatus::Failed;
                request.error = Some(error);
                request.resolution_json = None;
            }
            manager.set(request.clone());
            crate::utils::show_agent_review_overlay(&app);
        }
    }

    emit_review(&app, &request);
    Ok(request)
}
