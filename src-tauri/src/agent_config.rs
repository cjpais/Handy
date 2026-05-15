use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_PATH: &str = "agent_environment_store.json";

const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const OPENAI_REALTIME_MODEL: &str = "OPENAI_REALTIME_MODEL";
const GOOGLE_OAUTH_CLIENT_ID: &str = "GOOGLE_OAUTH_CLIENT_ID";
const GOOGLE_OAUTH_CLIENT_SECRET: &str = "GOOGLE_OAUTH_CLIENT_SECRET";
pub const AGENT_OWNER_NAME: &str = "AGENT_OWNER_NAME";
pub const NOTION_LEADS_TABLE_TARGET: &str = "NOTION_LEADS_TABLE_TARGET";
pub const NOTION_DEALS_TABLE_TARGET: &str = "NOTION_DEALS_TABLE_TARGET";
pub const NOTION_TASKS_TABLE_TARGET: &str = "NOTION_TASKS_TABLE_TARGET";
pub const NOTION_COMPANIES_TABLE_TARGET: &str = "NOTION_COMPANIES_TABLE_TARGET";
pub const NOTION_CONTACTS_TABLE_TARGET: &str = "NOTION_CONTACTS_TABLE_TARGET";
const DEFAULT_NOTION_LEADS_TABLE_TARGET: &str = "";
const DEFAULT_NOTION_DEALS_TABLE_TARGET: &str =
    "https://www.notion.so/2ec7d10de3d5806cba69ed65faba75fa?v=2ec7d10de3d580e796fb000c54fe562d";
const DEFAULT_NOTION_TASKS_TABLE_TARGET: &str =
    "https://www.notion.so/2077d10de3d580a8a551d13058db209b";
const DEFAULT_NOTION_COMPANIES_TABLE_TARGET: &str =
    "https://www.notion.so/2817d10de3d580938613dc3cf1269dba";
const DEFAULT_NOTION_CONTACTS_TABLE_TARGET: &str =
    "https://www.notion.so/2ef7d10de3d580f1bb6fc3155f0121ce";
const DEFAULT_AGENT_OWNER_NAME: &str = "Jason Walkow";

#[derive(Clone, Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnvironment {
    pub openai_api_key_saved: bool,
    pub openai_realtime_model: String,
    pub google_oauth_client_id: String,
    pub google_oauth_client_secret_saved: bool,
    pub agent_owner_name: String,
    pub notion_leads_table_target: String,
    pub notion_deals_table_target: String,
    pub notion_tasks_table_target: String,
    pub notion_companies_table_target: String,
    pub notion_contacts_table_target: String,
}

fn read_values(app: &AppHandle) -> Result<HashMap<String, String>, String> {
    let store = app
        .store(crate::portable::store_path(STORE_PATH))
        .map_err(|error| format!("Failed to open agent environment store: {}", error))?;

    Ok(store
        .get("values")
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default())
}

fn write_values(app: &AppHandle, values: HashMap<String, String>) -> Result<(), String> {
    let store = app
        .store(crate::portable::store_path(STORE_PATH))
        .map_err(|error| format!("Failed to open agent environment store: {}", error))?;

    store.set(
        "values",
        serde_json::to_value(values)
            .map_err(|error| format!("Failed to serialize agent environment: {}", error))?,
    );
    Ok(())
}

pub fn get_config_value(app: &AppHandle, key: &str) -> Option<String> {
    read_values(app)
        .ok()
        .and_then(|values| values.get(key).cloned())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| match key {
            NOTION_LEADS_TABLE_TARGET => {
                if DEFAULT_NOTION_LEADS_TABLE_TARGET.is_empty() {
                    None
                } else {
                    Some(DEFAULT_NOTION_LEADS_TABLE_TARGET.to_string())
                }
            }
            NOTION_DEALS_TABLE_TARGET => Some(DEFAULT_NOTION_DEALS_TABLE_TARGET.to_string()),
            NOTION_TASKS_TABLE_TARGET => Some(DEFAULT_NOTION_TASKS_TABLE_TARGET.to_string()),
            NOTION_COMPANIES_TABLE_TARGET => {
                Some(DEFAULT_NOTION_COMPANIES_TABLE_TARGET.to_string())
            }
            NOTION_CONTACTS_TABLE_TARGET => Some(DEFAULT_NOTION_CONTACTS_TABLE_TARGET.to_string()),
            AGENT_OWNER_NAME => Some(DEFAULT_AGENT_OWNER_NAME.to_string()),
            _ => None,
        })
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_environment(app: AppHandle) -> Result<AgentEnvironment, String> {
    let values = read_values(&app)?;

    Ok(AgentEnvironment {
        openai_api_key_saved: get_config_value(&app, OPENAI_API_KEY).is_some(),
        openai_realtime_model: values
            .get(OPENAI_REALTIME_MODEL)
            .cloned()
            .or_else(|| std::env::var(OPENAI_REALTIME_MODEL).ok())
            .unwrap_or_else(|| "gpt-realtime".to_string()),
        google_oauth_client_id: values
            .get(GOOGLE_OAUTH_CLIENT_ID)
            .cloned()
            .or_else(|| std::env::var(GOOGLE_OAUTH_CLIENT_ID).ok())
            .unwrap_or_default(),
        google_oauth_client_secret_saved: get_config_value(&app, GOOGLE_OAUTH_CLIENT_SECRET)
            .is_some(),
        agent_owner_name: values
            .get(AGENT_OWNER_NAME)
            .cloned()
            .or_else(|| std::env::var(AGENT_OWNER_NAME).ok())
            .unwrap_or_else(|| DEFAULT_AGENT_OWNER_NAME.to_string()),
        notion_leads_table_target: values
            .get(NOTION_LEADS_TABLE_TARGET)
            .cloned()
            .or_else(|| std::env::var(NOTION_LEADS_TABLE_TARGET).ok())
            .unwrap_or_else(|| DEFAULT_NOTION_LEADS_TABLE_TARGET.to_string()),
        notion_deals_table_target: values
            .get(NOTION_DEALS_TABLE_TARGET)
            .cloned()
            .or_else(|| std::env::var(NOTION_DEALS_TABLE_TARGET).ok())
            .unwrap_or_else(|| DEFAULT_NOTION_DEALS_TABLE_TARGET.to_string()),
        notion_tasks_table_target: values
            .get(NOTION_TASKS_TABLE_TARGET)
            .cloned()
            .or_else(|| std::env::var(NOTION_TASKS_TABLE_TARGET).ok())
            .unwrap_or_else(|| DEFAULT_NOTION_TASKS_TABLE_TARGET.to_string()),
        notion_companies_table_target: values
            .get(NOTION_COMPANIES_TABLE_TARGET)
            .cloned()
            .or_else(|| std::env::var(NOTION_COMPANIES_TABLE_TARGET).ok())
            .unwrap_or_else(|| DEFAULT_NOTION_COMPANIES_TABLE_TARGET.to_string()),
        notion_contacts_table_target: values
            .get(NOTION_CONTACTS_TABLE_TARGET)
            .cloned()
            .or_else(|| std::env::var(NOTION_CONTACTS_TABLE_TARGET).ok())
            .unwrap_or_else(|| DEFAULT_NOTION_CONTACTS_TABLE_TARGET.to_string()),
    })
}

#[tauri::command]
#[specta::specta]
pub fn update_agent_environment_value(
    app: AppHandle,
    key: String,
    value: String,
) -> Result<AgentEnvironment, String> {
    match key.as_str() {
        OPENAI_API_KEY
        | OPENAI_REALTIME_MODEL
        | GOOGLE_OAUTH_CLIENT_ID
        | GOOGLE_OAUTH_CLIENT_SECRET
        | AGENT_OWNER_NAME
        | NOTION_LEADS_TABLE_TARGET
        | NOTION_DEALS_TABLE_TARGET
        | NOTION_TASKS_TABLE_TARGET
        | NOTION_COMPANIES_TABLE_TARGET
        | NOTION_CONTACTS_TABLE_TARGET => {}
        _ => return Err(format!("Unsupported agent environment key: {}", key)),
    }

    let mut values = read_values(&app)?;
    if value.trim().is_empty() {
        values.remove(&key);
    } else {
        values.insert(key, value);
    }
    write_values(&app, values)?;
    get_agent_environment(app)
}
