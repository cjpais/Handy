use crate::settings::{get_settings, write_settings, RegexFilter};
use tauri::AppHandle;
use uuid::Uuid;

#[tauri::command]
pub fn get_regex_filters(app: AppHandle) -> Result<Vec<RegexFilter>, String> {
    let settings = get_settings(&app);
    Ok(settings.regex_filters)
}

#[tauri::command]
pub fn add_regex_filter(
    app: AppHandle,
    name: String,
    pattern: String,
    replacement: String,
) -> Result<RegexFilter, String> {
    let mut settings = get_settings(&app);
    
    // Validate regex pattern
    if let Err(e) = regex::Regex::new(&pattern) {
        return Err(format!("Invalid regex pattern: {}", e));
    }
    
    let filter = RegexFilter::new(
        Uuid::new_v4().to_string(),
        name,
        pattern,
        replacement,
    );
    
    settings.regex_filters.push(filter.clone());
    write_settings(&app, settings);
    
    Ok(filter)
}

#[tauri::command]
pub fn update_regex_filter(
    app: AppHandle,
    id: String,
    name: String,
    pattern: String,
    replacement: String,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    
    // Validate regex pattern
    if let Err(e) = regex::Regex::new(&pattern) {
        return Err(format!("Invalid regex pattern: {}", e));
    }
    
    if let Some(filter) = settings.regex_filters.iter_mut().find(|f| f.id == id) {
        filter.name = name;
        filter.pattern = pattern;
        filter.replacement = replacement;
        filter.enabled = enabled;
        
        write_settings(&app, settings);
        Ok(())
    } else {
        Err("Regex filter not found".to_string())
    }
}

#[tauri::command]
pub fn delete_regex_filter(app: AppHandle, id: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    
    let initial_len = settings.regex_filters.len();
    settings.regex_filters.retain(|f| f.id != id);
    
    if settings.regex_filters.len() < initial_len {
        write_settings(&app, settings);
        Ok(())
    } else {
        Err("Regex filter not found".to_string())
    }
}

#[tauri::command]
pub fn toggle_regex_filter(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    
    if let Some(filter) = settings.regex_filters.iter_mut().find(|f| f.id == id) {
        filter.enabled = enabled;
        write_settings(&app, settings);
        Ok(())
    } else {
        Err("Regex filter not found".to_string())
    }
}