use crate::managers::google_api::GoogleApi;
use crate::managers::google_oauth::GoogleOAuth;
use crate::settings::{get_settings, write_settings};
use tauri::AppHandle;

#[tauri::command]
#[specta::specta]
pub async fn start_google_oauth(app: AppHandle) -> Result<String, String> {
    match GoogleOAuth::start_oauth_flow().await {
        Ok(token_response) => {
            let mut settings = get_settings(&app);
            if let Some(refresh_token) = token_response.refresh_token {
                settings.google_oauth_token = Some(refresh_token);
                write_settings(&app, settings);
                Ok("success".to_string())
            } else {
                Err("No refresh token received from Google. If you were already connected, please disconnect first.".to_string())
            }
        }
        Err(e) => Err(format!("OAuth failed: {}", e)),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_google_auth_status(app: AppHandle) -> bool {
    let settings = get_settings(&app);
    settings.google_oauth_token.is_some()
}

#[tauri::command]
#[specta::specta]
pub fn disconnect_google_auth(app: AppHandle) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.google_oauth_token = None;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn send_meeting_follow_up(
    app: AppHandle,
    recipients: Vec<String>,
    summary: String,
    action_items: Vec<String>,
) -> Result<(), String> {
    let settings = get_settings(&app);
    let refresh_token = settings
        .google_oauth_token
        .as_ref()
        .ok_or("Not authenticated with Google")?;

    // Refresh token to get access token
    let token_response = GoogleOAuth::refresh_token(refresh_token)
        .await
        .map_err(|e| format!("Failed to refresh Google token: {}", e))?;

    let access_token = &token_response.access_token;

    // Send Email
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let subject = format!("Meeting Notes: {}", date);
    
    let mut body = format!("## Summary\n{}\n\n", summary);
    if !action_items.is_empty() {
        body.push_str("## Action Items\n");
        for item in &action_items {
            body.push_str(&format!("✅ {}\n", item));
        }
    }

    GoogleApi::send_email(access_token, recipients, &subject, &body)
        .await
        .map_err(|e| format!("Failed to send email: {}", e))?;

    // Create Tasks
    for item in action_items {
        GoogleApi::create_task(access_token, &item, Some(&format!("From meeting on {}", date)))
            .await
            .map_err(|e| format!("Failed to create task: {}", e))?;
    }

    Ok(())
}
