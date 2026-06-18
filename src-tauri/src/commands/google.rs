use crate::managers::google_api::GoogleApi;
use crate::managers::google_oauth::GoogleOAuth;
use crate::managers::meeting_assistant::{
    GoogleIntegrationStatus, MeetingAssistantManager, MeetingPromptPayload,
};
use crate::settings::{get_settings, write_settings, GoogleFeature};
use tauri::{AppHandle, Manager};

fn meeting_assistant(app: &AppHandle) -> tauri::State<'_, std::sync::Arc<MeetingAssistantManager>> {
    app.state::<std::sync::Arc<MeetingAssistantManager>>()
}

#[tauri::command]
#[specta::specta]
pub async fn connect_google_features(
    app: AppHandle,
    features: Vec<GoogleFeature>,
) -> Result<String, String> {
    let token_response = GoogleOAuth::start_oauth_flow(&features)
        .await
        .map_err(|e| format!("OAuth failed: {}", e))?;

    let refresh_token = token_response.refresh_token.ok_or_else(|| {
        "No refresh token received from Google. Disconnect first if you previously connected."
            .to_string()
    })?;

    let mut settings = get_settings(&app);
    for feature in features {
        match feature {
            GoogleFeature::GmailTasks => {
                settings.google_auth_tokens.gmail_tasks_refresh_token = Some(refresh_token.clone());
                settings.google_oauth_token = Some(refresh_token.clone());
            }
            GoogleFeature::Calendar => {
                settings.google_auth_tokens.calendar_refresh_token = Some(refresh_token.clone());
            }
        }
    }
    write_settings(&app, settings);
    Ok("success".to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_google_integration_status(app: AppHandle) -> GoogleIntegrationStatus {
    meeting_assistant(&app).google_status()
}

#[tauri::command]
#[specta::specta]
pub fn disconnect_google_feature(app: AppHandle, feature: GoogleFeature) -> Result<(), String> {
    let mut settings = get_settings(&app);
    match feature {
        GoogleFeature::GmailTasks => {
            settings.google_auth_tokens.gmail_tasks_refresh_token = None;
            settings.google_oauth_token =
                settings.google_auth_tokens.calendar_refresh_token.clone();
        }
        GoogleFeature::Calendar => {
            settings.google_auth_tokens.calendar_refresh_token = None;
            settings.meeting_calendar_prompts_enabled = false;
            settings.google_oauth_token = settings
                .google_auth_tokens
                .gmail_tasks_refresh_token
                .clone();
        }
    }
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn start_meeting_recording_from_prompt(app: AppHandle) -> Result<(), String> {
    crate::overlay::hide_meeting_prompt_window(&app);
    meeting_assistant(&app).clear_active_prompt();
    crate::signal_handle::send_transcription_input(&app, "meeting", "Meeting Assistant");
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_meeting_prompt(app: AppHandle, payload: MeetingPromptPayload) -> Result<(), String> {
    crate::overlay::hide_meeting_prompt_window(&app);
    meeting_assistant(&app).dismiss_prompt(&payload);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn close_meeting_prompt(app: AppHandle) -> Result<(), String> {
    crate::overlay::hide_meeting_prompt_window(&app);
    meeting_assistant(&app).clear_active_prompt();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_meeting_calendar_prompts_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.meeting_calendar_prompts_enabled = enabled;
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
        .google_auth_tokens
        .gmail_tasks_refresh_token
        .as_ref()
        .ok_or("Not authenticated with Google Gmail/Tasks")?;

    let token_response = GoogleOAuth::refresh_token(refresh_token, GoogleFeature::GmailTasks)
        .await
        .map_err(|e| format!("Failed to refresh Google token: {}", e))?;

    let access_token = &token_response.access_token;
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

    for item in action_items {
        GoogleApi::create_task(
            access_token,
            &item,
            Some(&format!("From meeting on {}", date)),
        )
        .await
        .map_err(|e| format!("Failed to create task: {}", e))?;
    }

    Ok(())
}
