use crate::settings::{get_settings, write_settings, PolishRule};
use tauri::AppHandle;
use uuid::Uuid;

#[tauri::command]
pub fn get_polish_rules(app: AppHandle) -> Vec<PolishRule> {
    let settings = get_settings(&app);
    settings.polish_rules
}

#[tauri::command]
pub fn add_polish_rule(
    app: AppHandle,
    name: String,
    api_url: String,
    api_key: String,
    model: String,
    prompt: String,
) -> Result<PolishRule, String> {
    // Input validation
    if name.trim().is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if api_url.trim().is_empty() {
        return Err("API URL cannot be empty".to_string());
    }
    if api_key.trim().is_empty() {
        return Err("API key cannot be empty".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model cannot be empty".to_string());
    }
    if prompt.trim().is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }

    let mut settings = get_settings(&app);
    let id = Uuid::new_v4().to_string();
    
    let new_rule = PolishRule::new(
        id.clone(),
        name.trim().to_string(),
        api_url.trim().to_string(),
        api_key.trim().to_string(),
        model.trim().to_string(),
        prompt.trim().to_string(),
    );
    
    settings.polish_rules.push(new_rule.clone());
    write_settings(&app, settings);
    
    Ok(new_rule)
}

#[tauri::command]
pub fn update_polish_rule(
    app: AppHandle,
    id: String,
    name: String,
    api_url: String,
    api_key: String,
    model: String,
    prompt: String,
    enabled: bool,
) -> Result<PolishRule, String> {
    // Input validation
    if name.trim().is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if api_url.trim().is_empty() {
        return Err("API URL cannot be empty".to_string());
    }
    if api_key.trim().is_empty() {
        return Err("API key cannot be empty".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model cannot be empty".to_string());
    }
    if prompt.trim().is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }

    let mut settings = get_settings(&app);
    
    if let Some(rule) = settings.polish_rules.iter_mut().find(|r| r.id == id) {
        rule.name = name.trim().to_string();
        rule.api_url = api_url.trim().to_string();
        rule.api_key = api_key.trim().to_string();
        rule.model = model.trim().to_string();
        rule.prompt = prompt.trim().to_string();
        rule.enabled = enabled;
        
        let result = rule.clone();
        write_settings(&app, settings);
        Ok(result)
    } else {
        Err("Polish rule not found".to_string())
    }
}

#[tauri::command]
pub fn delete_polish_rule(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    
    let initial_len = settings.polish_rules.len();
    settings.polish_rules.retain(|rule| rule.id != id);
    
    if settings.polish_rules.len() < initial_len {
        write_settings(&app, settings);
        Ok(())
    } else {
        Err("Polish rule not found".to_string())
    }
}

#[tauri::command]
pub fn toggle_polish_rule(app: AppHandle, id: String, enabled: bool) -> Result<PolishRule, String> {
    let mut settings = get_settings(&app);
    
    if let Some(rule) = settings.polish_rules.iter_mut().find(|r| r.id == id) {
        rule.enabled = enabled;
        let result = rule.clone();
        write_settings(&app, settings);
        Ok(result)
    } else {
        Err("Polish rule not found".to_string())
    }
}