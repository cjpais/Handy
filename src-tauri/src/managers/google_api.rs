use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct GoogleApi;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CalendarEventsResponse {
    #[serde(default)]
    pub items: Vec<CalendarEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: Option<String>,
    pub summary: Option<String>,
    pub html_link: Option<String>,
    pub hangout_link: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start: CalendarDateTime,
    pub end: Option<CalendarDateTime>,
    pub conference_data: Option<ConferenceData>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDateTime {
    pub date_time: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceData {
    #[serde(default)]
    pub entry_points: Vec<ConferenceEntryPoint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceEntryPoint {
    pub uri: Option<String>,
}

impl GoogleApi {
    pub async fn send_email(
        access_token: &str,
        recipients: Vec<String>,
        subject: &str,
        body: &str,
    ) -> Result<()> {
        let client = reqwest::Client::new();
        let to = recipients.join(", ");
        let email_content = format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
            to, subject, body
        );

        let encoded_email = URL_SAFE_NO_PAD.encode(email_content.as_bytes());
        let response = client
            .post("https://gmail.googleapis.com/gmail/v1/users/me/messages/send")
            .bearer_auth(access_token)
            .json(&json!({ "raw": encoded_email }))
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to send email: {}", error_text));
        }

        Ok(())
    }

    pub async fn create_task(access_token: &str, title: &str, notes: Option<&str>) -> Result<()> {
        let client = reqwest::Client::new();
        let mut body = json!({ "title": title });
        if let Some(n) = notes {
            body["notes"] = json!(n);
        }

        let response = client
            .post("https://tasks.googleapis.com/tasks/v1/lists/@default/tasks")
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to create task: {}", error_text));
        }

        Ok(())
    }

    pub async fn list_upcoming_events(
        access_token: &str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
            .bearer_auth(access_token)
            .query(&[
                ("timeMin", time_min.to_rfc3339()),
                ("timeMax", time_max.to_rfc3339()),
                ("singleEvents", "true".to_string()),
                ("orderBy", "startTime".to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Failed to fetch calendar events: {}", error_text));
        }

        Ok(response.json::<CalendarEventsResponse>().await?.items)
    }
}
