//! Drafts a reply to the chat that is open on screen.
//!
//! The reader takes a screenshot and asks Claude to read it, so it works on any
//! chat page in any browser or app. Nothing is sent to the chat: the reply is
//! typed into the focused input like a transcript, and the person presses send.

use base64::Engine;
use serde_json::{json, Value};
use std::process::Command;
use std::time::Duration;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-sonnet-5-5";
const MAX_REPLY_TOKENS: u32 = 700;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Keychain entry holding the person's own Anthropic API key. Add it with:
/// `security add-generic-password -s handy-anthropic-key -a handy -w`
const KEYCHAIN_SERVICE: &str = "handy-anthropic-key";

/// How many of the latest messages the reply must answer.
const MESSAGES_TO_READ: u32 = 3;

fn instructions() -> String {
    format!(
        "The image is a screenshot of a customer support conversation that a support agent is \
         working on. Read the last {MESSAGES_TO_READ} messages that the customer can see. \
         Write the reply the agent should send next.\n\
         \n\
         Rules:\n\
         - Internal notes are not part of the conversation. They are often shaded yellow or \
           labelled as a note. Never use them, quote them or mention them.\n\
         - Ignore menus, sidebars and ticket details. They are not messages.\n\
         - Match the language and the tone of the agent's earlier replies.\n\
         - Answer only what the messages ask. Do not invent facts, prices, dates or promises. \
           If the messages do not give enough to answer, ask one short question.\n\
         - Reply with the message text only. No greeting label, no quotes, no explanation."
    )
}

/// The Messages API request body for one screenshot.
fn build_request(png_base64: &str) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_REPLY_TOKENS,
        "system": instructions(),
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": png_base64,
                    }
                },
                { "type": "text", "text": "Write the reply." }
            ]
        }]
    })
}

/// Joins the text blocks of a Messages API response.
fn parse_reply(response: &Value) -> Result<String, String> {
    if let Some(message) = response.pointer("/error/message").and_then(Value::as_str) {
        return Err(format!("The model returned an error: {message}"));
    }
    let text: String = response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return Err("The model returned no text".to_string());
    }
    Ok(text.to_string())
}

#[cfg(target_os = "macos")]
fn capture_screen_png() -> Result<Vec<u8>, String> {
    let path = std::env::temp_dir().join(format!("handy-reply-{}.png", std::process::id()));
    // -x: no shutter sound. Captures the main display.
    let status = Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .arg(&path)
        .status()
        .map_err(|e| format!("Could not run screencapture: {e}"))?;
    let bytes = std::fs::read(&path);
    // The screenshot can hold customer details: never leave it behind.
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return Err("screencapture failed. Allow Screen Recording for Handy.".to_string());
    }
    let bytes = bytes.map_err(|e| format!("Could not read the screenshot: {e}"))?;
    if bytes.is_empty() {
        return Err("The screenshot is empty. Allow Screen Recording for Handy.".to_string());
    }
    Ok(bytes)
}

#[cfg(not(target_os = "macos"))]
fn capture_screen_png() -> Result<Vec<u8>, String> {
    Err("Reply drafting reads the screen and is only available on macOS".to_string())
}

fn api_key() -> Result<String, String> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key.trim().to_string());
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
            .output()
            .map_err(|e| format!("Could not run security: {e}"))?;
        if output.status.success() {
            let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !key.is_empty() {
                return Ok(key);
            }
        }
    }
    Err(format!(
        "No Anthropic API key. Save one with: security add-generic-password -s {KEYCHAIN_SERVICE} -a handy -w"
    ))
}

/// Reads the chat on screen and returns the reply to type.
pub fn draft_reply_from_screen() -> Result<String, String> {
    let key = api_key()?;
    let png = capture_screen_png()?;
    let body = build_request(&base64::engine::general_purpose::STANDARD.encode(png));

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(API_URL)
        .header("x-api-key", &key)
        .header("anthropic-version", API_VERSION)
        .send_json(&body)
        .map_err(|e| format!("Could not reach the model: {e}"))?;
    let json: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("Unreadable model response: {e}"))?;
    parse_reply(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_carries_the_screenshot_as_an_image_block() {
        let body = build_request("AAAA");
        let image = &body["messages"][0]["content"][0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["source"]["media_type"], "image/png");
        assert_eq!(image["source"]["data"], "AAAA");
    }

    #[test]
    fn the_instructions_exclude_internal_notes_and_ask_for_three_messages() {
        let text = instructions();
        assert!(text.contains("last 3 messages"));
        assert!(text.contains("Internal notes are not part of the conversation"));
        assert!(text.contains("Never use them"));
    }

    #[test]
    fn the_reply_is_the_trimmed_text_of_the_response() {
        let response = json!({
            "content": [
                { "type": "text", "text": "  Hi Tarek,\n" },
                { "type": "text", "text": "Thanks for waiting.  " }
            ]
        });
        assert_eq!(
            parse_reply(&response).unwrap(),
            "Hi Tarek,\nThanks for waiting."
        );
    }

    #[test]
    fn non_text_blocks_are_not_typed() {
        let response = json!({
            "content": [
                { "type": "thinking", "thinking": "private" },
                { "type": "text", "text": "Hello" }
            ]
        });
        assert_eq!(parse_reply(&response).unwrap(), "Hello");
    }

    #[test]
    fn an_api_error_is_reported_and_never_typed() {
        let response =
            json!({ "error": { "type": "authentication_error", "message": "invalid x-api-key" } });
        let error = parse_reply(&response).unwrap_err();
        assert!(error.contains("invalid x-api-key"));
    }

    #[test]
    fn an_empty_response_is_an_error() {
        assert!(parse_reply(&json!({ "content": [] })).is_err());
        assert!(parse_reply(&json!({ "content": [{ "type": "text", "text": "  " }] })).is_err());
    }
}
