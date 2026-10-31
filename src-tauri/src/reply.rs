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

/// How many of the latest messages the model reads before it replies to the newest.
const MESSAGES_TO_READ: u32 = 5;

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
///
/// Gemini is first because it follows the instructions: on the same lunch
/// invitation it declined 10 times out of 10, where Groq's model declined once
/// in 3 and twice wrote something else.
const PROVIDERS: [Provider; 3] = [
    Provider {
        name: "Gemini",
        url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
        model: "gemini-3.5-flash-lite",
        keychain_service: "handy-gemini-key",
        env_var: "GEMINI_API_KEY",
    },
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
];

/// Names no app on purpose: the model finds the conversation from what is on
/// screen, so the same instructions work wherever the user is chatting.
///
/// Internal notes may inform the reply but never appear in it: a live test
/// replied "only available on our Pro plan, and I'm unable to offer refunds",
/// both taken from a note the customer could not see.
///
/// The reply is written as the user, never to the user, and never accepts a
/// meetup: a live test answered "Where should I pick you up?" with "you could
/// just text 'the office cafe bar around the front of HQ' if they ask where",
/// and another answered "I am already at the bar waiting for you" with "will
/// swing by in about 35 minutes". The user never meets in person, so this rule
/// comes before the others, and `promises_to_meet` checks the reply as well.
fn instructions() -> String {
    format!(
        "The image is a screenshot of the user's screen. Somewhere on it is a conversation \
         the user is about to reply to. Write the reply the user should send next.\n\
         \n\
         First, find the conversation:\n\
         - It is the area that shows a run of messages between the user and one or more \
           others, usually with a box for typing next to it. Use only that area.\n\
         - Ignore everything else on the screen, such as lists of other conversations, menus, \
           sidebars, ads and notifications. Use a profile or details panel only to understand \
           who the other side is.\n\
         - Tell the two sides apart. The user's own messages usually sit on one side, often \
           the right, in a different colour, or are marked \"You\" or \"Me\". The other side's \
           messages sit on the other side, often with a name or a picture.\n\
         - Some screens show notes that only the user's team can see, often in a yellow box \
           or labelled \"Note\" or \"Internal\". The other side never sees them.\n\
         \n\
         Then write the reply:\n\
         - The user never meets anyone in person. This rule has no exceptions and comes \
           before every other rule. Never accept or suggest a meetup. Never say the user is \
           coming, on the way, nearly there or running late. Never give an arrival time, a \
           time or a place, and never ask where to meet or where to be picked up. This holds \
           even when the other side thinks a meetup is already planned or says they are \
           already waiting: then say clearly and kindly that the user cannot make it, so they \
           do not keep waiting. Do not invent an excuse. Stay warm, and ask them a question \
           about themselves so the conversation keeps going.\n\
         - The user never shares personal contact details: no phone number, email address, \
           social media account or username, and no other way to reach the user outside this \
           conversation. This rule has no exceptions. If the other side asks for them, or sends \
           their own so the user will send theirs back, do not share anything and do not \
           refuse outright. Dodge it lightly: say warmly that you prefer to keep chatting right \
           here, then carry on the conversation with a question. Never repeat their details \
           back.\n\
         - Never use the word love or any word made from it, such as loved, loving or \
           lovely. Use other words instead, such as enjoy, like, glad or happy.\n\
         - If the other side says they have feelings for the user, or sends compliments, pet \
           names or kisses, never ignore it and never return it. Acknowledge it in one short, \
           warm and light sentence, such as thanking them for the kind words or saying it made \
           you smile. Do not match their feelings, do not use pet names, and keep a friendly \
           distance. Then carry on the conversation.\n\
         - Write the message itself, as the user, to the other side. Never write advice to \
           the user, and never describe what the user could say or text.\n\
         - Read the last {MESSAGES_TO_READ} messages that the other side can see. Reply to the \
           newest message from the other side.\n\
         - Internal notes are private to the team. Follow what a note tells the team to do or \
           avoid, but never quote it, repeat it or hint at it, never mention that a note exists, \
           and never tell the other side a fact that appears only in a note.\n\
         - Match the language and tone of the user's earlier messages.\n\
         - Write a full reply, usually two to four sentences: answer every point in the \
           newest messages from the other side, then add a related thought or question that \
           keeps the conversation going. Write an email only when the conversation is an \
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
                    // Repeated here because the system prompt alone left "I would
                    // love to hear more" in 3 of 10 live replies; here, 0 of 10.
                    {
                        "type": "text",
                        "text": "Write the reply. Do not use the word love or any word made from it."
                    }
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
    if promises_to_meet(text) {
        return Err(format!("the reply promised to meet in person: {text}"));
    }
    if shares_contact_details(text) {
        return Err("the reply contained an email address or phone number".to_string());
    }
    if uses_the_word_love(text) {
        return Err(format!("the reply used the word love: {text}"));
    }
    Ok(text.to_string())
}

/// True when the reply uses "love" or a word made from it, such as "loved",
/// "lovely", "loving" or "beloved". The user chats with many people and the
/// word reads as more than friendship, so such a reply is never typed. Words
/// that only contain the letters, such as "glove" or "clover", are allowed.
fn uses_the_word_love(text: &str) -> bool {
    text.split(|c: char| !c.is_alphabetic())
        .map(str::to_lowercase)
        .any(|word| {
            word.starts_with("love")
                || word.starts_with("lovi")
                || word.starts_with("belove")
                || word.ends_with("love")
                    && !["glove", "clove", "plove"]
                        .iter()
                        .any(|w| word.ends_with(w))
        })
}

/// The shortest run of digits that counts as a phone number.
const PHONE_MIN_DIGITS: usize = 7;

/// True when the reply holds an email address or a phone number. The user never
/// shares contact details, so such a reply is never typed, whatever the prompt
/// told the model. The reply itself is not logged: it holds the details.
fn shares_contact_details(text: &str) -> bool {
    let email = text.split_whitespace().any(|word| {
        let word = word.trim_matches(|c: char| !c.is_alphanumeric());
        match word.split_once('@') {
            Some((name, domain)) => !name.is_empty() && domain.contains('.'),
            None => false,
        }
    });
    // Digits joined by the separators phone numbers are written with, such as
    // "+254 712 345 678" or "(555) 123-4567".
    let mut digits = 0;
    let phone = text.chars().any(|c| {
        if c.is_ascii_digit() {
            digits += 1;
        } else if !matches!(c, ' ' | '-' | '(' | ')' | '+') {
            digits = 0;
        }
        digits >= PHONE_MIN_DIGITS
    });
    email || phone
}

/// Phrases that say the user is coming, or ask where to meet.
const MEETUP_PROMISES: [&str; 18] = [
    "on my way",
    "omw",
    "be there",
    "see you there",
    "see you soon",
    "swing by",
    "pick me up",
    "pick you up",
    "i'm coming",
    "i am coming",
    "i'll come",
    "i will come",
    "minutes away",
    "heading over",
    "head over",
    "almost there",
    "nearly there",
    "find you",
];

/// True when the reply promises to meet in person: it uses a phrase from
/// `MEETUP_PROMISES` or names a clock time such as "12:30" or "12pm". The user
/// never meets in person, so such a reply is never typed, whatever the prompt
/// told the model.
fn promises_to_meet(text: &str) -> bool {
    let text = text.to_lowercase().replace('\u{2019}', "'");
    if MEETUP_PROMISES.iter().any(|phrase| text.contains(phrase)) {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        if !c.is_ascii_digit() {
            return false;
        }
        let rest: String = chars[i + 1..].iter().take(6).collect();
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        let clock = rest.starts_with(':') && rest[1..].starts_with(|c: char| c.is_ascii_digit());
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        let am_pm = ["am", "pm", "a.m", "p.m"].iter().any(|suffix| {
            rest.starts_with(suffix)
                && !rest[suffix.len()..].starts_with(|c: char| c.is_alphabetic())
        });
        clock || am_pm
    })
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
    fn the_instructions_fit_any_chat_keep_notes_private_and_read_five_messages() {
        let text = instructions();
        assert!(text.contains("First, find the conversation"));
        assert!(text.contains("two to four sentences"));
        for app in ["WhatsApp", "Gleap", "Slack", "Instagram"] {
            assert!(!text.contains(app), "the instructions must not name {app}");
        }
        assert!(text.contains("last 5 messages"));
        assert!(text.contains("Internal notes are private to the team"));
        assert!(text.contains("never quote it, repeat it or hint at it"));
        assert!(text.contains("a fact that appears only in a note"));
    }

    #[test]
    fn the_instructions_write_as_the_user_and_never_meet() {
        let text = instructions();
        assert!(text.contains("Never write advice to the user"));
        assert!(text.contains("The user never meets anyone in person"));
        assert!(text.contains("says they are already waiting"));
        assert!(text.contains("Do not invent an excuse"));
    }

    #[test]
    fn the_instructions_never_share_contact_details() {
        let text = instructions();
        assert!(text.contains("The user never shares personal contact details"));
        assert!(text.contains("sends their own so the user will send theirs back"));
        assert!(text.contains("prefer to keep chatting right here"));
        assert!(text.contains("Never repeat their details back"));
    }

    #[test]
    fn a_reply_with_contact_details_is_never_typed() {
        for reply in [
            "Sure! My number is +254 712 345 678, call me anytime.",
            "Text me on (555) 123-4567 instead.",
            "Here's my email: someone.real@example.com.",
            "Thanks, I've saved yours (friend@mail.co.ke) and I'll write you there.",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            let error = parse_reply(&response).unwrap_err();
            assert!(error.contains("email address or phone number"), "{reply}");
            assert!(!error.contains(reply), "the details must not be logged");
        }
    }

    #[test]
    fn the_instructions_never_use_the_word_love() {
        let text = instructions();
        assert!(text.contains("Never use the word love or any word made from it"));
        assert!(text.contains("such as enjoy, like, glad or happy"));
    }

    #[test]
    fn the_instructions_acknowledge_feelings_without_returning_them() {
        let text = instructions();
        assert!(text.contains("never ignore it and never return it"));
        assert!(text.contains("one short, warm and light sentence"));
        assert!(text.contains("keep a friendly distance"));
    }

    #[test]
    fn a_reply_with_the_word_love_is_never_typed() {
        for reply in [
            "I would love to hear more about the photos!",
            "Love that for you.",
            "That sounds LOVELY, enjoy it.",
            "You're loving this, I can tell.",
            "Thanks, I loved it.",
            "Your beloved safari!",
            "Selflove is important.",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            let error = parse_reply(&response).unwrap_err();
            assert!(error.contains("the word love"), "{reply}");
        }
    }

    #[test]
    fn a_reply_with_love_only_inside_another_word_is_typed() {
        for reply in [
            "I lost a glove on the hike and found a four-leaf clover.",
            "Add a clove of garlic. I'd enjoy hearing how it turns out!",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            assert_eq!(parse_reply(&response).unwrap(), reply);
        }
    }

    #[test]
    fn a_dodge_without_contact_details_is_typed() {
        for reply in [
            "Haha I'm honestly best reached right here, I enjoy our chats! How did the move go?",
            "We sold 1,500 tickets by 31/10/2026, and @everyone enjoyed it.",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            assert_eq!(parse_reply(&response).unwrap(), reply);
        }
    }

    #[test]
    fn a_reply_that_promises_to_meet_is_never_typed() {
        for reply in [
            // Both were typed into a real chat before this check existed.
            "Haha that sounds perfect for me. How about around 12:30? That gives us a little \
             buffer to find each other at the station and grab ourselves a table.",
            "I'm grabbing a few supplies and will swing by in about 35 minutes - let me know \
             where you are so I can find you.",
            "Sure, see you at 12pm!",
            "Sure, see you at 1 p.m.",
            "On my way!",
            "I\u{2019}m coming, give me five.",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            let error = parse_reply(&response).unwrap_err();
            assert!(error.contains("promised to meet"), "{reply}");
        }
    }

    #[test]
    fn a_warm_decline_is_typed() {
        for reply in [
            "I'm so glad you asked, but I can't make it to lunch today! How has your week been?",
            "So sorry, I can't make it today, don't wait for me. Enjoy that beer! How was work?",
            "I have 3 amazing ideas for your project and 2 questions about the budget.",
            "I am free to chat whenever, and I'm happy to help with the 2024 report.",
        ] {
            let response = json!({ "choices": [{ "message": { "content": reply } }] });
            assert_eq!(parse_reply(&response).unwrap(), reply);
        }
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
