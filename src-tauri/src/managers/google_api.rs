use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct GoogleApi;

impl GoogleApi {
    pub async fn send_email(
        access_token: &str,
        recipients: Vec<String>,
        subject: &str,
        body: &str,
    ) -> Result<()> {
        let client = reqwest::Client::new();

        // Construct RFC 2822 email
        let to = recipients.join(", ");
        let email_content = format!(
            "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}",
            to, subject, body
        );

        let encoded_email = URL_SAFE_NO_PAD.encode(email_content.as_bytes());

        let url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";

        let response = client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "raw": encoded_email
            }))
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

        let url = "https://tasks.googleapis.com/tasks/v1/lists/@default/tasks";

        let mut body = json!({
            "title": title,
        });

        if let Some(n) = notes {
            body["notes"] = json!(n);
        }

        let response = client
            .post(url)
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
}
