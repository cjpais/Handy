//! Drafts a reply to the chat that is open on screen.
//!
//! The reader takes a screenshot and asks a vision model to read it, so it works
//! on any chat page in any browser or app. Nothing is sent to the chat: the reply
//! is typed into the focused input like a transcript, and the person presses send.
//!
//! Several free providers are tried in order. When one fails (its daily limit is
//! used up, its key is missing, its model was withdrawn), the next one is tried.

use base64::Engine;
use log::{info, warn};
use serde_json::{json, Value};
use std::process::Command;
use std::time::Duration;

/// Reasoning models spend part of this on thinking before they write the reply.
const MAX_REPLY_TOKENS: u32 = 2000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest side of the screenshot sent to the model. A full Retina capture
/// is several MB; this keeps chat text readable at a fraction of the size.
#[cfg(target_os = "macos")]
const SCREENSHOT_MAX_SIDE_PX: u32 = 1600;

/// How many of the latest messages the reply must answer.
const MESSAGES_TO_READ: u32 = 3;

/// A provider with an OpenAI-compatible chat completions endpoint and a free tier.
struct Provider {
    name: &'static str,
    url: &'static str,
    model: &'static str,
    /// Keychain entry with the person's key. Add it with:
    /// `security add-generic-password -s <service> -a handy -w`
    keychain_service: &'static str,
    env_var: &'static str,
}

/// Tried in this order. Free-tier model IDs change: a withdrawn model fails like
/// a used-up limit does, and the next provider is tried.
const PROVIDERS: [Provider; 3] = [
    Provider {
        name: "Groq",
        url: "https://api.groq.com/openai/v1/chat/completions",
        model: "qwen/qwen3.8-27b",
        keychain_service: "handy-groq-key",
        env_var: "GROQ_API_KEY",
    },
    Provider {
        name: "OpenRouter",
        url: "https://openrouter.ai/api/v1/chat/completions",
        model: "openrouter/free",
        keychain_service: "handy-openrouter-key",
        env_var: "OPENROUTER_API_KEY",
    },
    Provider {
        name: "Gemini",
        url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        model: "gemini-3.5-flash-lite",
        keychain_service: "handy-gemini-key",
        env_var: "GEMINI_API_KEY",
    },
];

/// Written for any chat app, so the model can tell the open conversation from
/// the rest of the screen and the user's own messages from the other side's.
///
/// Internal notes may inform the reply but never appear in it: a live test
/// replied "only available on our Pro plan, and I'm unable to offer refunds",
/// both taken from a note the customer could not see.
fn instructions() -> String {
    format!(
        "The image is a screenshot of a conversation in a chat app. It can be any app: \
         WhatsApp, Slack, Messenger, a support inbox such as Gleap or Intercom, or email. \
         The user of this tool is about to reply. Write the reply the user should send next.\n\
         \n\
         How to read the screen:\n\
         - Find the open conversation. Ignore lists of other chats, menus and sidebars. \
           Use a profile or details panel only to understand who the other side is.\n\
         - The user's own messages are usually on the right, in a coloured bubble (green in \
           WhatsApp), or marked \"You\". The other side's messages are usually on the left.\n\
         - Support tools can show internal notes, often as yellow boxes or labelled \"Note\" \
           or \"Internal\". The other side never sees them.\n\
         \n\
         Rules:\n\
         - Read the last {MESSAGES_TO_READ} messages that the other side can see. Reply to the \
           newest message from the other side.\n\
         - Internal notes are private to the team. Follow what a note tells the team to do or \
           avoid, but never quote it, repeat it or hint at it, never mention that a note exists, \
           and never tell the other side a fact that appears only in a note.\n\
         - Match the language, length and tone of the user's earlier messages. A chat reply \
           is short, like a text message. Write an email only when the conversation is an \
           email: then greet by first name and sign off the way the user's earlier replies do.\n\
         - Answer only what the messages ask. Do not invent facts, prices, dates, links or \
           promises. If the messages do not give enough to answer, ask one short question.\n\
         - Write plain text. Chat boxes show Markdown symbols as they are, so use no \
           asterisks, no headings and no bullet symbols.\n\
         - Reply with the message text only. No greeting label, no quotes, no explanation."
    )
}

/// The chat completions request body for one screenshot.
fn build_request(model: &str, jpeg_base64: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": MAX_REPLY_TOKENS,
        "messages": [
            { "role": "system", "content": instructions() },
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/jpeg;base64,{jpeg_base64}") }
                    },
                    { "type": "text", "text": "Write the reply." }
                ]
            }
        ]
    })
}

/// Removes `<think>…</think>` blocks that some reasoning models put in the
/// reply text. An unclosed block (the model ran out of tokens while thinking)
/// removes everything after it, so thinking is never typed.
fn strip_thinking(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The reply text of a chat completions response.
fn parse_reply(response: &Value) -> Result<String, String> {
    if let Some(message) = response.pointer("/error/message").and_then(Value::as_str) {
        return Err(message.to_string());
    }
    let content = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = strip_thinking(content);
    let text = text.trim();
    if text.is_empty() {
        return Err("no reply text".to_string());
    }
    Ok(text.to_string())
}

/// Sends one request and returns the reply text. Any HTTP error, including a
/// used-up limit (429) or no credits (402), is an `Err`.
fn request_reply(url: &str, key: &str, body: &Value) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(url)
        .header("Authorization", &format!("Bearer {key}"))
        .send_json(body)
        .map_err(|e| format!("could not connect: {e}"))?;
    let status = response.status().as_u16();
    let json: Value = response.body_mut().read_json().unwrap_or(Value::Null);
    let reply = parse_reply(&json);
    match (status, reply) {
        (200..=299, reply) => reply,
        (status, Err(message)) if json != Value::Null => Err(format!("HTTP {status}: {message}")),
        (status, _) => Err(format!("HTTP {status}")),
    }
}

/// One provider to try, with its key if the person saved one.
struct Attempt<'a> {
    name: &'a str,
    url: &'a str,
    model: &'a str,
    key: Option<String>,
}

/// Tries each provider in order and returns the first reply. Fails only when
/// all of them fail, with each one's reason.
fn draft_with_fallback(attempts: &[Attempt], jpeg_base64: &str) -> Result<String, String> {
    let mut failures = Vec::new();
    for attempt in attempts {
        let Some(key) = &attempt.key else {
            failures.push(format!("{}: no key saved", attempt.name));
            continue;
        };
        let body = build_request(attempt.model, jpeg_base64);
        match request_reply(attempt.url, key, &body) {
            Ok(reply) => {
                info!("Drafted the reply with {}", attempt.name);
                return Ok(reply);
            }
            Err(e) => {
                warn!(
                    "{} could not draft the reply, trying the next provider: {e}",
                    attempt.name
                );
                failures.push(format!("{}: {e}", attempt.name));
            }
        }
    }
    Err(format!(
        "No provider drafted a reply. {}",
        failures.join("; ")
    ))
}

fn saved_key(provider: &Provider) -> Option<String> {
    if let Ok(key) = std::env::var(provider.env_var) {
        if !key.trim().is_empty() {
            return Some(key.trim().to_string());
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                provider.keychain_service,
                "-w",
            ])
            .output()
            .ok()?;
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if output.status.success() && !key.is_empty() {
            return Some(key);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn capture_screen_jpeg() -> Result<Vec<u8>, String> {
    let path = std::env::temp_dir().join(format!("handy-reply-{}.jpg", std::process::id()));
    let result = (|| {
        // -x: no shutter sound. Captures the main display.
        let captured = Command::new("screencapture")
            .args(["-x", "-t", "jpg"])
            .arg(&path)
            .status()
            .map_err(|e| format!("Could not run screencapture: {e}"))?;
        if !captured.success() {
            return Err("screencapture failed. Allow Screen Recording for Handy.".to_string());
        }
        let resized = Command::new("sips")
            .args(["-Z", &SCREENSHOT_MAX_SIDE_PX.to_string()])
            .arg(&path)
            .output()
            .map_err(|e| format!("Could not run sips: {e}"))?;
        if !resized.status.success() {
            return Err("Could not shrink the screenshot".to_string());
        }
        let bytes =
            std::fs::read(&path).map_err(|e| format!("Could not read the screenshot: {e}"))?;
        if bytes.is_empty() {
            return Err("The screenshot is empty. Allow Screen Recording for Handy.".to_string());
        }
        Ok(bytes)
    })();
    // The screenshot can hold customer details: never leave it behind.
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(not(target_os = "macos"))]
fn capture_screen_jpeg() -> Result<Vec<u8>, String> {
    Err("Reply drafting reads the screen and is only available on macOS".to_string())
}

/// Reads the chat on screen and returns the reply to type.
pub fn draft_reply_from_screen() -> Result<String, String> {
    let attempts: Vec<Attempt> = PROVIDERS
        .iter()
        .map(|p| Attempt {
            name: p.name,
            url: p.url,
            model: p.model,
            key: saved_key(p),
        })
        .collect();
    if attempts.iter().all(|a| a.key.is_none()) {
        return Err(
            "No provider key saved. Save one with: security add-generic-password -s handy-groq-key -a handy -w"
                .to_string(),
        );
    }
    let jpeg = capture_screen_jpeg()?;
    draft_with_fallback(
        &attempts,
        &base64::engine::general_purpose::STANDARD.encode(jpeg),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_carries_the_screenshot_as_an_image_part() {
        let body = build_request("some-model", "AAAA");
        assert_eq!(body["model"], "some-model");
        assert_eq!(body["messages"][0]["role"], "system");
        let image = &body["messages"][1]["content"][0];
        assert_eq!(image["type"], "image_url");
        assert_eq!(image["image_url"]["url"], "data:image/jpeg;base64,AAAA");
    }

    #[test]
    fn the_instructions_fit_any_chat_keep_notes_private_and_ask_for_three_messages() {
        let text = instructions();
        assert!(text.contains("It can be any app: WhatsApp"));
        assert!(text.contains("last 3 messages"));
        assert!(text.contains("Internal notes are private to the team"));
        assert!(text.contains("never quote it, repeat it or hint at it"));
        assert!(text.contains("a fact that appears only in a note"));
    }

    #[test]
    fn the_reply_is_the_trimmed_message_content() {
        let response =
            json!({ "choices": [{ "message": { "content": "  Hi Tarek,\nThanks.  " } }] });
        assert_eq!(parse_reply(&response).unwrap(), "Hi Tarek,\nThanks.");
    }

    #[test]
    fn thinking_is_never_typed() {
        let response = json!({ "choices": [{ "message": {
            "content": "<think>The customer wants a refund.</think>\nHi Tarek, I have sent it."
        } }] });
        assert_eq!(parse_reply(&response).unwrap(), "Hi Tarek, I have sent it.");
    }

    #[test]
    fn a_reply_cut_off_while_thinking_is_an_error() {
        let response =
            json!({ "choices": [{ "message": { "content": "<think>The customer wa" } }] });
        assert!(parse_reply(&response).is_err());
    }

    #[test]
    fn an_api_error_is_reported_and_never_typed() {
        let response = json!({ "error": { "message": "Rate limit reached" } });
        assert!(parse_reply(&response)
            .unwrap_err()
            .contains("Rate limit reached"));
    }

    #[test]
    fn an_empty_response_is_an_error() {
        assert!(parse_reply(&json!({ "choices": [] })).is_err());
        assert!(parse_reply(&json!({ "choices": [{ "message": { "content": "  " } }] })).is_err());
    }

    /// Serves one canned HTTP response on localhost and returns the request it got.
    fn serve_once(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                received.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&received).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length = text[..split]
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if received.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&received).to_string()
        });
        (url, handle)
    }

    const OK_BODY: &str = r#"{"choices":[{"message":{"content":"Hi Tarek, thanks."}}]}"#;

    #[test]
    fn the_http_call_sends_the_key_and_the_screenshot_and_returns_the_reply() {
        let (url, server) = serve_once("200 OK", OK_BODY);
        let reply = request_reply(&url, "test-key", &build_request("m", "QUJD")).unwrap();
        let request = server.join().unwrap();
        assert_eq!(reply, "Hi Tarek, thanks.");
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer test-key"));
        assert!(request.contains("data:image/jpeg;base64,QUJD"));
    }

    #[test]
    fn a_used_up_limit_switches_to_the_next_provider() {
        let (limited_url, limited) = serve_once(
            "429 Too Many Requests",
            r#"{"error":{"message":"Rate limit reached for requests per day"}}"#,
        );
        let (ok_url, ok) = serve_once("200 OK", OK_BODY);
        let attempts = [
            Attempt {
                name: "First",
                url: &limited_url,
                model: "m",
                key: Some("k1".into()),
            },
            Attempt {
                name: "Second",
                url: &ok_url,
                model: "m",
                key: Some("k2".into()),
            },
        ];
        assert_eq!(
            draft_with_fallback(&attempts, "QUJD").unwrap(),
            "Hi Tarek, thanks."
        );
        limited.join().unwrap();
        assert!(ok.join().unwrap().to_lowercase().contains("bearer k2"));
    }

    #[test]
    fn a_provider_without_a_key_is_skipped() {
        let (ok_url, ok) = serve_once("200 OK", OK_BODY);
        let attempts = [
            Attempt {
                name: "NoKey",
                url: "http://127.0.0.1:9/unused",
                model: "m",
                key: None,
            },
            Attempt {
                name: "Second",
                url: &ok_url,
                model: "m",
                key: Some("k2".into()),
            },
        ];
        assert_eq!(
            draft_with_fallback(&attempts, "QUJD").unwrap(),
            "Hi Tarek, thanks."
        );
        ok.join().unwrap();
    }

    #[test]
    fn when_every_provider_fails_each_reason_is_reported_and_nothing_is_typed() {
        let (limited_url, limited) = serve_once(
            "429 Too Many Requests",
            r#"{"error":{"message":"Rate limit reached"}}"#,
        );
        let (broke_url, broke) = serve_once(
            "402 Payment Required",
            r#"{"error":{"code":402,"message":"Insufficient credits"}}"#,
        );
        let attempts = [
            Attempt {
                name: "Groq",
                url: &limited_url,
                model: "m",
                key: Some("k1".into()),
            },
            Attempt {
                name: "OpenRouter",
                url: &broke_url,
                model: "m",
                key: Some("k2".into()),
            },
            Attempt {
                name: "Gemini",
                url: "unused",
                model: "m",
                key: None,
            },
        ];
        let error = draft_with_fallback(&attempts, "QUJD").unwrap_err();
        limited.join().unwrap();
        broke.join().unwrap();
        assert!(error.contains("Groq: HTTP 429: Rate limit reached"));
        assert!(error.contains("OpenRouter: HTTP 402: Insufficient credits"));
        assert!(error.contains("Gemini: no key saved"));
    }
}
