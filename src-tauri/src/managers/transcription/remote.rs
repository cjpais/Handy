//! Transcription on a remote server instead of a local model. The recording
//! is POSTed as a 16 kHz mono WAV: in a multipart form, in a chat request for
//! an audio LLM, as the raw body, or base64 inside a JSON body of the user's
//! own. Everything about the request and the response comes from the user's
//! [`RemoteAsrServer`]: no server is built in.

use crate::engine_supervisor::EngineError;
use crate::llm_client::{client_builder, sanitized_url_for_log};
use crate::settings::{AppSettings, RemoteAsrServer, RemoteBody, RequestPair};
use anyhow::{bail, Result};
use base64::Engine;
use log::{debug, warn};
use reqwest::multipart::{Form, Part};
use serde::Serialize;
use serde_json::{json, Map, Value};
use specta::Type;
use std::fmt;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How often a waiting request looks for a cancel.
const CANCEL_POLL: Duration = Duration::from_millis(50);
const LANGUAGE: &str = "language";
const API_KEY: &str = "api_key";
const AUDIO_BASE64: &str = "audio_base64";
/// The most of a server's error text worth showing.
const ERROR_TEXT_LIMIT: usize = 300;
/// The largest reply read from a server. A transcript is a few kilobytes;
/// more than this is something else, and isn't loaded into memory.
const REPLY_LIMIT_BYTES: usize = 4 * 1024 * 1024;

/// Why a request failed, as a code Settings translates
/// (`settings.advanced.remoteTranscription.errors.*`) and the detail only Rust
/// knows. Shown as an English sentence in logs and notices.
#[derive(Debug)]
pub struct Failure {
    pub code: &'static str,
    pub detail: String,
    /// It may not happen again: nothing was transcribed (the connection was
    /// refused, reset or never made) or the server said to try later (429,
    /// 500, 502, 503). A bad key or a wrong path fails the same way again.
    pub transient: bool,
}

impl Failure {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            transient: false,
        }
    }

    fn transient(mut self) -> Self {
        self.transient = true;
        self
    }

    /// `code|detail`, split again by the frontend.
    pub fn coded(&self) -> String {
        format!("{}|{}", self.code, self.detail)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = &self.detail;
        match self.code {
            "noUrl" => write!(f, "no URL is set"),
            "unreachable" => write!(f, "couldn't connect to {d}"),
            "noAnswer" => write!(f, "no answer from {d}"),
            "refused" => write!(f, "the server answered {d}"),
            "webPage" => write!(f, "the server answered with a web page, not a transcript"),
            "stream" => write!(f, "the server streamed its answer"),
            "tooLarge" => write!(f, "the server's reply is too large to be a transcript"),
            "noText" => write!(f, "the reply has no text at {d}"),
            "keychainMissing" => write!(f, "the API key is no longer in the system keychain"),
            "keychain" => write!(f, "couldn't use the system keychain ({d})"),
            "placeholder" => write!(f, "{d} uses a placeholder with no value"),
            "badRequest" => write!(f, "the request couldn't be built ({d})"),
            code => write!(f, "{code}: {d}"),
        }
    }
}

impl std::error::Error for Failure {}

/// Whether `e` is a [`Failure`] that may not happen again.
pub fn is_transient(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Failure>().is_some_and(|f| f.transient)
}

/// Why a server's settings were refused, as a code Settings translates
/// (`settings.advanced.remoteTranscription.errors.*`) and the detail only
/// Rust knows.
#[derive(Debug, PartialEq, Eq)]
pub struct ServerError {
    pub code: &'static str,
    pub detail: String,
}

impl ServerError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ServerError {
    /// `code|detail`, split again by the frontend.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}|{}", self.code, self.detail)
    }
}

/// One client for every request, built like the app's others so connections
/// are reused and dead ones aren't. It follows a redirect only to the host
/// it asked: a recording and its key headers don't go anywhere the user
/// didn't name.
fn client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    // Same scheme, host and port only: not over to plain http, and not to
    // another service on the same machine.
    let same_origin = reqwest::redirect::Policy::custom(|attempt| {
        let asked = attempt.previous().first().map(|url| url.origin());
        if attempt.previous().len() > 5 {
            attempt.error("too many redirects")
        } else if asked.as_ref() != Some(&attempt.url().origin()) {
            attempt.error("the server redirected somewhere else")
        } else if !matches!(attempt.status().as_u16(), 307 | 308) {
            // 301/302/303 turn the POST into a GET without the recording:
            // fail with a reason rather than send an empty request.
            attempt.error("the server redirected the request to the URL it gave; use that URL")
        } else {
            attempt.follow()
        }
    });
    let built = client_builder().redirect(same_origin).build()?;
    Ok(CLIENT.get_or_init(|| built))
}

/// The runtime remote requests run on: their own, so a request never waits
/// on the app's runtime, whose workers a dictation may be holding. Its
/// connections live as long as it does, so the client's pool stays good.
fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    // Built under a lock: two first calls must not both build one, since the
    // spare would be dropped wherever it lost, possibly inside a runtime.
    static BUILDING: Mutex<()> = Mutex::new(());
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let _building = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let built = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("remote-transcription-io")
        .enable_all()
        .build()?;
    Ok(RUNTIME.get_or_init(|| built))
}

/// One remote transcription: the server, and the language its request names.
#[derive(Clone)]
pub struct RemoteTarget {
    server: RemoteAsrServer,
    /// Handy's language as an ISO 639-1 code, or `None` on Auto.
    language: Option<String>,

    /// The cancel count when the recording started: a cancel after it stops
    /// this request, even one made before the request began.
    pub(crate) cancel_mark: Option<u64>,
}

impl RemoteTarget {
    /// `None` unless remote transcription is on with a usable server.
    pub fn from_settings(settings: &AppSettings) -> Option<Self> {
        let server = settings.active_remote_server()?.clone();
        let mut target = Self::new(server, &settings.selected_language);
        // On Auto with one spoken language set, that's the language, as it
        // is for a local model.
        if target.language.is_none() {
            if let [only] = settings.spoken_languages.as_slice() {
                target.language = request_language(only);
            }
        }
        Some(target)
    }

    fn new(server: RemoteAsrServer, selected_language: &str) -> Self {
        Self {
            server,
            language: request_language(selected_language),

            cancel_mark: None,
        }
    }

    /// The language the request told the server, when the URL, a field, the
    /// prompt or the JSON body names `{language}` and Handy has one.
    pub fn requested_language(&self) -> Option<String> {
        let uses = |text: &str| uses_placeholder(text, LANGUAGE);
        let server = &self.server;
        let in_fields = || server.fields.iter().any(|p| uses(&p.value));
        let named = uses(&server.url)
            || server.headers.iter().any(|p| uses(&p.value))
            || match server.body {
                RemoteBody::Multipart => in_fields(),
                RemoteBody::Chat => in_fields() || uses(&server.prompt),
                RemoteBody::Json => uses(&server.json_body),
                RemoteBody::Raw => false,
            };
        self.language.clone().filter(|_| named)
    }

    /// Transcribe 16 kHz mono `samples`. When `cancelled` says so the
    /// request is dropped mid-flight, which closes the connection, and this
    /// returns [`EngineError::Cancelled`]. Safe to call from inside or
    /// outside an async runtime.
    pub fn transcribe(&self, samples: &[f32], cancelled: &dyn Fn() -> bool) -> Result<String> {
        self.transcribe_wav(crate::audio_toolkit::wav_bytes(samples)?, cancelled)
    }

    /// [`Self::transcribe`] for a recording already encoded as WAV, so a retry
    /// doesn't encode it again.
    pub fn transcribe_wav(&self, wav: Vec<u8>, cancelled: &dyn Fn() -> bool) -> Result<String> {
        debug!(
            "Remote transcription: '{}' {} ({} bytes of WAV)",
            self.server.name,
            sanitized_url_for_log(&self.server.url),
            wav.len()
        );
        // On its own thread, never as a task of the caller's runtime: the
        // caller may be blocking one of that runtime's workers, and a task
        // spawned from it would wait behind the very thread waiting for it.
        let (tx, rx) = mpsc::channel();
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let runtime = runtime()?;
        let target = self.clone();
        std::thread::Builder::new()
            .name("remote-transcription".into())
            .spawn(move || {
                // Here, not before: a keychain prompt or a stalled keychain
                // service mustn't keep a cancel from getting through.
                let target = match target.with_stored_key() {
                    Ok(target) => target,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                let request = Box::pin(target.send(wav));
                let answer = runtime.block_on(async move {
                    match futures_util::future::select(request, cancel_rx).await {
                        futures_util::future::Either::Left((result, _)) => Some(result),
                        // The request is dropped here, closing its connection.
                        futures_util::future::Either::Right(_) => None,
                    }
                });
                if let Some(result) = answer {
                    let _ = tx.send(result);
                }
            })?;
        loop {
            match rx.recv_timeout(CANCEL_POLL) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) if cancelled() => {
                    debug!("Remote transcription cancelled; dropping the request");
                    let _ = cancel_tx.send(());
                    return Err(EngineError::Cancelled.into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("the remote transcription thread stopped without an answer")
                }
            }
        }
    }

    /// This target with the server's key read from the system keychain,
    /// unless it already carries one (typed in Settings, not saved yet).
    fn with_stored_key(&self) -> Result<Self> {
        let mut target = self.clone();
        if !(target.server.has_api_key && target.server.api_key.trim().is_empty()) {
            return Ok(target);
        }
        // Read once per process (the keychain may ask), and asked again
        // only after a failure: a locked keychain or a dismissed prompt.
        let key = match crate::keychain::load_cached(&target.server.id) {
            Ok(Some(key)) => key,
            Ok(None) => return Err(Failure::new("keychainMissing", "").into()),
            Err(e) => return Err(Failure::new("keychain", e).into()),
        };
        target.server.api_key = key.into();
        Ok(target)
    }

    fn api_key(&self) -> Option<&str> {
        Some(self.server.api_key.trim()).filter(|k| !k.is_empty())
    }

    /// `text` with `{language}` and `{api_key}` filled, or `None` when one
    /// has no value and no fallback.
    fn fill_text(&self, text: &str) -> Option<String> {
        let text = fill_placeholder(text, LANGUAGE, self.language.as_deref())?;
        fill_placeholder(&text, API_KEY, self.api_key())
    }

    /// `pair` with its placeholders filled, or `None` when it has no name or
    /// a placeholder has no value (so the pair is left out).
    fn fill(&self, pair: &RequestPair) -> Option<(String, String)> {
        let name = pair.name.trim();
        if name.is_empty() {
            return None;
        }
        Some((name.to_string(), self.fill_text(&pair.value)?))
    }

    /// The fields to send: filled, and without the ones left empty.
    fn fields(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.server
            .fields
            .iter()
            .filter_map(|pair| self.fill(pair))
            .filter(|(_, value)| !value.is_empty())
    }

    /// The URL with its placeholders filled. A query parameter whose
    /// placeholder has no value is dropped; one elsewhere is an error.
    fn url(&self) -> Result<reqwest::Url> {
        let raw = self.server.url.trim();
        let (base, query) = raw.split_once('?').unwrap_or((raw, ""));
        // In the path a value is one segment: percent-encoded, so a key with
        // a / or ? in it can't change where the request goes.
        let encoded = |v: Option<&str>| v.map(percent_encode_segment);
        let base = fill_placeholder(base, LANGUAGE, encoded(self.language.as_deref()).as_deref())
            .and_then(|b| fill_placeholder(&b, API_KEY, encoded(self.api_key()).as_deref()))
            .ok_or_else(|| Failure::new("placeholder", "URL"))?;
        let mut url =
            reqwest::Url::parse(&base).map_err(|e| Failure::new("badRequest", e.to_string()))?;
        if !query.is_empty() {
            let pairs: Vec<(String, String)> = reqwest::Url::parse(&format!("http://q/?{query}"))?
                .query_pairs()
                .filter_map(|(name, value)| {
                    self.fill(&RequestPair {
                        name: name.into_owned(),
                        value: value.into_owned(),
                    })
                })
                .collect();
            if !pairs.is_empty() {
                url.query_pairs_mut().extend_pairs(pairs);
            }
        }
        Ok(url)
    }

    fn form(&self, wav: Vec<u8>) -> Result<Form> {
        let file = Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let mut form = Form::new().part(self.server.file_field.trim().to_string(), file);
        for (name, value) in self.fields() {
            form = form.text(name, value);
        }
        Ok(form)
    }

    /// An OpenAI-style chat request: the fields as top-level keys, the
    /// prompt as the system message and the recording as `input_audio`.
    fn chat_body(&self, wav: &[u8]) -> Result<String> {
        let mut body = Map::new();
        for (name, value) in self.fields() {
            body.insert(name, typed_json(&value));
        }
        let mut messages = Vec::new();
        let prompt = self
            .fill_text(self.server.prompt.trim())
            .ok_or_else(|| Failure::new("placeholder", "Prompt"))?;
        if !prompt.is_empty() {
            messages.push(json!({ "role": "system", "content": prompt }));
        }
        let audio = base64::engine::general_purpose::STANDARD.encode(wav);
        messages.push(json!({
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": { "data": audio, "format": "wav" },
            }],
        }));
        body.insert("messages".to_string(), Value::Array(messages));
        Ok(Value::Object(body).to_string())
    }

    /// The user's JSON body with the placeholders and the recording in it.
    fn json_body(&self, wav: &[u8]) -> Result<String> {
        let audio = base64::engine::general_purpose::STANDARD.encode(wav);
        let body = fill_json_body(
            &self.server.json_body,
            &[
                (LANGUAGE, self.language.as_deref()),
                (API_KEY, self.api_key()),
            ],
        )?;
        // Last, and as it is: base64 needs no escaping, and the other
        // placeholders shouldn't be searched for inside it.
        Ok(body.replace(&format!("{{{AUDIO_BASE64}}}"), &audio))
    }

    /// The request for `url` with this server's headers and `wav` as its
    /// body.
    fn request(
        &self,
        url: reqwest::Url,
        wav: Vec<u8>,
        timeout: u32,
    ) -> Result<reqwest::RequestBuilder> {
        let server = &self.server;
        let mut request = client()?
            .post(url)
            .timeout(Duration::from_secs(timeout.into()));

        let mut headers: Vec<(String, String)> =
            server.headers.iter().filter_map(|p| self.fill(p)).collect();
        let is_content_type = |name: &str| name.eq_ignore_ascii_case("content-type");
        let has_content_type = headers.iter().any(|(name, _)| is_content_type(name));
        request = match server.body {
            RemoteBody::Multipart => {
                // The form sets it, boundary included; a second one breaks it.
                if has_content_type {
                    warn!("Ignoring the Content-Type header: a form upload sets its own");
                    headers.retain(|(name, _)| !is_content_type(name));
                }
                request.multipart(self.form(wav)?)
            }
            RemoteBody::Raw if has_content_type => request.body(wav),
            RemoteBody::Raw => request.header("Content-Type", "audio/wav").body(wav),
            RemoteBody::Chat | RemoteBody::Json => {
                let body = match server.body {
                    RemoteBody::Chat => self.chat_body(&wav)?,
                    _ => self.json_body(&wav)?,
                };
                if has_content_type {
                    request.body(body)
                } else {
                    request
                        .header("Content-Type", "application/json")
                        .body(body)
                }
            }
        };
        for (name, value) in headers {
            request = request.header(name, value);
        }
        Ok(request)
    }

    async fn send(self, wav: Vec<u8>) -> Result<String> {
        let server = &self.server;
        // For messages: without the query, which may carry a key.
        let url = sanitized_url_for_log(server.url.trim());
        let url = url.as_str();
        let timeout = server.timeout_secs.max(1);
        let mut target_url = self.url()?;
        let mut redirected = false;
        let response = loop {
            let response = self
                .request(target_url.clone(), wav.clone(), timeout)?
                .send()
                .await
                .map_err(|e| describe_send_error(e, url, timeout, Stage::Sending))?;
            // reqwest can't send a form upload again, so it hands a 307/308
            // back instead of following it; follow it here, once, under the
            // same rule as every other redirect.
            match same_origin_redirect(&target_url, &response) {
                Some(next) if !redirected => {
                    debug!("Following the server's redirect for the form upload");
                    target_url = next;
                    redirected = true;
                }
                _ => break response,
            }
        };
        drop(wav);
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| describe_send_error(e, url, timeout, Stage::ReadingReply))?
        {
            if bytes.len() + chunk.len() > REPLY_LIMIT_BYTES {
                return Err(Failure::new("tooLarge", "").into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = String::from_utf8_lossy(&bytes).into_owned();
        if !status.is_success() {
            let refused = Failure::new("refused", format!("{status}: {}", error_message(&body)));
            // 504 comes after a proxy waited the whole time out: again would
            // wait as long.
            let retry = matches!(status.as_u16(), 429 | 500 | 502 | 503);
            return Err(if retry { refused.transient() } else { refused }.into());
        }
        check_reply_kind(&content_type, &body)?;
        response_text(&body, &server.text_paths)
    }
}

/// Where a 307/308 `response` to `asked` points, if it stays on the same
/// scheme, host and port.
fn same_origin_redirect(
    asked: &reqwest::Url,
    response: &reqwest::Response,
) -> Option<reqwest::Url> {
    if !matches!(response.status().as_u16(), 307 | 308) {
        return None;
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)?
        .to_str()
        .ok()?;
    let next = asked.join(location).ok()?;
    (next.origin() == asked.origin()).then_some(next)
}

/// A connection test's result, for Settings.
#[derive(Debug, Serialize, Type)]
pub struct RemoteTestResult {
    pub elapsed_ms: u32,
    /// What the server made of a second of silence (usually nothing).
    pub text: String,
}

/// Send a second of silence to `server`, as a dictation would.
pub fn test_remote_transcription(
    server: RemoteAsrServer,
    selected_language: &str,
) -> Result<RemoteTestResult> {
    let server = clean_remote_server(server).map_err(|e| Failure::new(e.code, e.detail))?;
    if server.url.is_empty() {
        return Err(Failure::new("noUrl", "").into());
    }
    let target = RemoteTarget::new(server, selected_language);
    let started = Instant::now();
    let text = target.transcribe(&vec![0.0; 16_000], &|| false)?;
    Ok(RemoteTestResult {
        elapsed_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        text,
    })
}

/// `server` with its text trimmed and blank required fields filled with
/// their defaults; an error for a URL that isn't http(s) or a JSON body that
/// can't work. Pairs without a name are kept (a row being typed in Settings)
/// and skipped when the request is built.
pub fn clean_remote_server(mut server: RemoteAsrServer) -> Result<RemoteAsrServer, ServerError> {
    server.id = server.id.trim().to_string();
    if server.id.is_empty() {
        return Err(ServerError::new("missingId", ""));
    }
    server.name = server.name.trim().to_string();
    server.url = server.url.trim().to_string();
    if !server.url.is_empty() {
        let parsed = reqwest::Url::parse(&server.url)
            .map_err(|_| ServerError::new("invalidUrl", server.url.clone()))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ServerError::new("urlScheme", server.url.clone()));
        }
    }
    server.api_key = server.api_key.trim().to_string().into();
    for pair in server.headers.iter_mut().chain(server.fields.iter_mut()) {
        pair.name = pair.name.trim().to_string();
        pair.value = pair.value.trim().to_string();
    }
    // Caught here, a bad header is a form error; at request time it would
    // read as a network failure.
    for header in server.headers.iter().filter(|h| !h.name.is_empty()) {
        let name_ok = reqwest::header::HeaderName::from_bytes(header.name.as_bytes()).is_ok();
        let value_ok = reqwest::header::HeaderValue::from_str(&header.value).is_ok();
        if !name_ok || !value_ok {
            return Err(ServerError::new("invalidHeader", header.name.clone()));
        }
    }
    server.file_field = match server.file_field.trim() {
        "" => "file".to_string(),
        field => field.to_string(),
    };
    server.text_paths = match server.text_paths.trim() {
        "" => "text".to_string(),
        paths => paths.to_string(),
    };
    if server.timeout_secs == 0 {
        server.timeout_secs = 120;
    }
    server.prompt = server.prompt.trim().to_string();
    server.json_body = server.json_body.trim().to_string();
    // A JSON value or a prompt can't just be left out when Handy's language
    // is Auto, so {language} needs a fallback there ({language|auto}).
    let bare = format!("{{{LANGUAGE}}}");
    if server.body == RemoteBody::Json && server.json_body.contains(&bare) {
        return Err(ServerError::new("placeholder", "JSON"));
    }
    if server.body == RemoteBody::Chat && server.prompt.contains(&bare) {
        return Err(ServerError::new("placeholder", "Prompt"));
    }
    if server.body == RemoteBody::Json {
        if !server.json_body.contains(&format!("{{{AUDIO_BASE64}}}")) {
            return Err(ServerError::new("jsonNeedsAudio", ""));
        }
        // Check it parses with every placeholder filled.
        let filled = fill_json_body(
            &server.json_body,
            &[
                (AUDIO_BASE64, Some("")),
                (LANGUAGE, Some("en")),
                (API_KEY, Some("key")),
            ],
        )
        .map_err(|e| ServerError::new("jsonInvalid", e.to_string()))?;
        serde_json::from_str::<Value>(&filled)
            .map_err(|e| ServerError::new("jsonInvalid", e.to_string()))?;
    }
    Ok(server)
}

/// A field's value as JSON: a number, `true`, `false` or `null` as itself,
/// anything else as a string. Quotes force a string (`"4"` for a model
/// named 4).
fn typed_json(value: &str) -> Value {
    match serde_json::from_str::<Value>(value) {
        Ok(v @ (Value::Number(_) | Value::Bool(_) | Value::Null | Value::String(_))) => v,
        _ => Value::String(value.to_string()),
    }
}

/// Whether `text` has the placeholder `{name}` or `{name|fallback}`.
fn uses_placeholder(text: &str, name: &str) -> bool {
    text.contains(&format!("{{{name}}}")) || text.contains(&format!("{{{name}|"))
}

/// Refuses a 2xx reply that can't be a transcript, before it gets pasted
/// somewhere: a web page (a captive portal, a proxy's error) or a stream.
fn check_reply_kind(content_type: &str, body: &str) -> Result<()> {
    let start = body.trim_start();
    let starts_with = |prefix: &str| {
        start
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    if content_type.contains("html") || starts_with("<!doctype") || starts_with("<html") {
        return Err(Failure::new("webPage", "").into());
    }
    if content_type.contains("event-stream") || content_type.contains("ndjson") {
        return Err(Failure::new("stream", "").into());
    }
    Ok(())
}

/// `value` with every `{name}` and `{name|fallback}` replaced by `with`,
/// or by the fallback when `with` is `None`. `None` when a placeholder
/// has neither.
fn fill_placeholder(value: &str, name: &str, with: Option<&str>) -> Option<String> {
    let open = format!("{{{name}");
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let (fallback, len) = match after.chars().next() {
            Some('}') => (None, 1),
            Some('|') => match after.find('}') {
                Some(end) => (Some(&after[1..end]), end + 1),
                None => break,
            },
            // `{languages}` or similar: not this placeholder.
            _ => {
                out.push_str(&rest[..start + open.len()]);
                rest = after;
                continue;
            }
        };
        out.push_str(&rest[..start]);
        out.push_str(with.or(fallback)?);
        rest = &after[len..];
    }
    out.push_str(rest);
    Some(out)
}

/// `template` with each placeholder filled by its value escaped for a JSON
/// string. A placeholder with no value is an error, since a JSON value
/// can't just be left out.
fn fill_json_body(template: &str, values: &[(&str, Option<&str>)]) -> Result<String> {
    let mut body = template.to_string();
    for (name, with) in values {
        // Escaped as the inside of a JSON string, without its quotes.
        let escaped = with.map(|v| {
            let quoted = Value::String(v.to_string()).to_string();
            quoted[1..quoted.len() - 1].to_string()
        });
        body = fill_placeholder(&body, name, escaped.as_deref())
            .ok_or_else(|| Failure::new("placeholder", "JSON"))?;
    }
    Ok(body)
}

/// A request that got no answer, said plainly.
/// Where a request was when it failed. Once the server has answered, the
/// recording was transcribed: sending it again would do (and bill) it twice.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Sending,
    ReadingReply,
}

fn describe_send_error(
    e: reqwest::Error,
    url: &str,
    timeout_secs: u32,
    stage: Stage,
) -> anyhow::Error {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| {
                format!(
                    "{h}{}",
                    u.port().map(|p| format!(":{p}")).unwrap_or_default()
                )
            })
        })
        .unwrap_or_else(|| url.to_string());
    // reqwest puts the full URL in its messages, query (and any key in it)
    // included: keep it out of anything shown or logged.
    let e = &e.without_url();
    // A connection that was never made sent nothing: worth another try.
    if e.is_connect() && e.is_timeout() {
        return Failure::new("unreachable", format!("{host} (timed out)"))
            .transient()
            .into();
    }
    // Not transient: the recording was sent, and sending it again costs as
    // much and most likely times out the same way.
    if e.is_timeout() {
        return Failure::new("noAnswer", format!("{host} in {timeout_secs} s")).into();
    }
    // The request itself is wrong (a header an API key made invalid, say):
    // sending it again changes nothing.
    if e.is_builder() {
        return Failure::new("badRequest", e.to_string()).into();
    }
    // The innermost cause says what went wrong ("Connection refused", "dns
    // error", "certificate"…); the outer layers only repeat the URL.
    let mut cause: &dyn std::error::Error = e;
    while let Some(inner) = cause.source() {
        cause = inner;
    }
    // Only a connection that was refused, reset or dropped before an answer
    // is worth another try: nothing was transcribed, and it may come back. A
    // bad name, a bad certificate or a refused redirect fails the same way.
    // Told apart by the OS error's kind, not its text, which differs by OS
    // ("Connection refused" here, "actively refused it" on Windows).
    // And only while connecting: once the recording is up, a dropped
    // connection may come after the server transcribed (and billed) it.
    let retry =
        stage == Stage::Sending && e.is_connect() && !e.is_redirect() && dropped_connection(e);
    let failure = Failure::new("unreachable", format!("{host} ({cause})"));
    if retry {
        failure.transient().into()
    } else {
        failure.into()
    }
}

/// Whether `e` comes from a connection the other side refused, reset or
/// closed.
fn dropped_connection(e: &(dyn std::error::Error + 'static)) -> bool {
    use std::io::ErrorKind::*;
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(error) = next {
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            if matches!(
                io.kind(),
                ConnectionRefused | ConnectionReset | ConnectionAborted | BrokenPipe | NotConnected
            ) {
                return true;
            }
        }
        next = error.source();
    }
    false
}

/// `value` with everything but unreserved characters percent-encoded, to sit
/// inside one segment of a URL path.
fn percent_encode_segment(value: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// An error for Settings: `code|detail` when it's a [`Failure`] or a
/// refused setting, so it can be translated, and the plain text otherwise.
pub fn coded_error(e: &anyhow::Error) -> String {
    match e.downcast_ref::<Failure>() {
        Some(failure) => failure.coded(),
        None => format!("{e:#}"),
    }
}

/// Handy's language setting as an ISO 639-1 code.
fn request_language(selected: &str) -> Option<String> {
    let code = selected.split(['-', '_']).next().unwrap_or_default();
    (!code.is_empty() && code != "auto").then(|| code.to_lowercase())
}

/// The text of a response: the first of `paths` with a value in a JSON
/// object or array, or the body itself when it is anything else (plain text,
/// which may happen to read as a JSON number or word). Thai SARA AM written
/// as NIKHAHIT + SARA AA (some tokenizers split it) is joined back into one
/// character.
fn response_text(body: &str, paths: &str) -> Result<String> {
    let text = match serde_json::from_str::<Value>(body) {
        Ok(Value::String(text)) => text,
        // A bare `null` is nothing said, like `"text": null`.
        Ok(Value::Null) => String::new(),
        Ok(json @ (Value::Object(_) | Value::Array(_))) => {
            let paths: Vec<&str> = paths
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            match paths.iter().find_map(|path| json_path(&json, path)) {
                Some(text) => text,
                // `"text": null` is a server's way of saying nothing was said.
                None if paths.iter().any(|path| json_path_exists(&json, path)) => String::new(),
                None => {
                    let detail = format!("{} (keys: {})", paths.join(", "), top_level_keys(&json));
                    return Err(Failure::new("noText", detail).into());
                }
            }
        }
        _ => body.to_string(),
    };
    // The text gets pasted wherever the user is typing, a terminal included:
    // nothing but printable characters, line breaks and tabs goes through.
    let text: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    Ok(text
        .replace("\u{0E4D}\u{0E32}", "\u{0E33}")
        .trim()
        .to_string())
}

/// Every value a dot `path` (`results.0.transcript`) reaches in `json`. A
/// `*` step takes each item of an array (or value of an object).
fn json_nodes<'a>(json: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut nodes = vec![json];
    for key in path.split('.') {
        nodes = nodes
            .into_iter()
            .flat_map(|node| -> Vec<&Value> {
                match (node, key) {
                    (Value::Array(items), "*") => items.iter().collect(),
                    (Value::Object(map), "*") => map.values().collect(),
                    (Value::Object(map), _) => map.get(key).into_iter().collect(),
                    (Value::Array(items), _) => key
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| items.get(i))
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                }
            })
            .collect();
    }
    nodes
}

/// The text at `path`, its pieces joined with spaces; `None` when nothing
/// but nulls is there.
fn json_path(json: &Value, path: &str) -> Option<String> {
    let pieces: Vec<String> = json_nodes(json, path)
        .into_iter()
        .filter_map(|node| match node {
            Value::String(s) => Some(s.trim().to_string()),
            Value::Null => None,
            other => Some(other.to_string()),
        })
        .collect();
    if pieces.is_empty() {
        return None;
    }
    // Joined the way chunks of a recording are: no space between pieces of a
    // language written without them (Thai, Chinese, Japanese).
    let pieces: Vec<&str> = pieces
        .iter()
        .map(String::as_str)
        .filter(|p| !p.is_empty())
        .collect();
    Some(crate::audio_toolkit::chunking::join_transcripts(&pieces))
}

/// Whether `path` reaches anything in `json`, a null included.
fn json_path_exists(json: &Value, path: &str) -> bool {
    !json_nodes(json, path).is_empty()
}

fn top_level_keys(json: &Value) -> String {
    match json {
        Value::Object(map) if !map.is_empty() => map.keys().cloned().collect::<Vec<_>>().join(", "),
        _ => "none".to_string(),
    }
}

/// The useful part of an error body: `detail` (FastAPI), `error.message`
/// (OpenAI), `error` or `message`, or the start of the body.
fn error_message(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let found = parsed.as_ref().and_then(|v| {
        ["detail", "error.message", "error", "message"]
            .iter()
            .find_map(|path| json_path(v, path))
    });
    found
        .as_deref()
        .unwrap_or(body)
        .trim()
        .chars()
        .take(ERROR_TEXT_LIMIT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &str, value: &str) -> RequestPair {
        RequestPair {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    fn server(url: &str) -> RemoteAsrServer {
        RemoteAsrServer {
            id: "s1".to_string(),
            name: "Test".to_string(),
            url: url.to_string(),
            body: RemoteBody::Multipart,
            api_key: String::new().into(),
            has_api_key: false,
            headers: vec![pair("Authorization", "Bearer {api_key}")],
            fields: vec![],
            prompt: String::new(),
            json_body: String::new(),
            file_field: "file".to_string(),
            text_paths: "text".to_string(),
            timeout_secs: 10,
        }
    }

    #[test]
    fn language_is_the_base_code_or_nothing() {
        assert_eq!(request_language("th"), Some("th".to_string()));
        assert_eq!(request_language("zh-Hans"), Some("zh".to_string()));
        assert_eq!(request_language("auto"), None);
        assert_eq!(request_language(""), None);
    }

    #[test]
    fn placeholders_fill_or_drop_the_pair() {
        let mut s = server("http://x");
        s.fields = vec![pair("language", "{language}"), pair("model", "m")];
        let auto = RemoteTarget::new(s.clone(), "auto");
        assert_eq!(auto.fill(&s.fields[0]), None);
        assert_eq!(auto.fill(&s.headers[0]), None, "no key, no auth header");
        assert_eq!(auto.requested_language(), None);

        s.api_key = "k".to_string().into();
        let thai = RemoteTarget::new(s.clone(), "th");
        assert_eq!(
            thai.fill(&s.fields[0]),
            Some(("language".into(), "th".into()))
        );
        assert_eq!(
            thai.fill(&s.headers[0]),
            Some(("Authorization".into(), "Bearer k".into()))
        );
        assert_eq!(thai.requested_language(), Some("th".to_string()));
    }

    #[test]
    fn placeholder_fallbacks() {
        assert_eq!(
            fill_placeholder("{language|auto}", "language", None),
            Some("auto".to_string())
        );
        assert_eq!(
            fill_placeholder("{language|auto}", "language", Some("th")),
            Some("th".to_string())
        );
        assert_eq!(fill_placeholder("{language}", "language", None), None);
        assert_eq!(
            fill_placeholder("x-{language}-{language|y}", "language", Some("th")),
            Some("x-th-th".to_string())
        );
        // Other braces are left alone.
        assert_eq!(
            fill_placeholder("{languages} {other}", "language", Some("th")),
            Some("{languages} {other}".to_string())
        );
        let mut s = server("http://x");
        s.fields = vec![pair("language", "{language|auto}")];
        let auto = RemoteTarget::new(s.clone(), "auto");
        assert_eq!(
            auto.fill(&s.fields[0]),
            Some(("language".into(), "auto".into()))
        );
        assert_eq!(auto.requested_language(), None);
        assert_eq!(
            RemoteTarget::new(s, "th").requested_language(),
            Some("th".to_string())
        );
    }

    #[test]
    fn text_is_read_at_the_first_path_present() {
        let body =
            r#"{"text": "raw", "display_text": " ทำงาน ", "results": [{"transcript": "nested"}]}"#;
        assert_eq!(response_text(body, "display_text, text").unwrap(), "ทำงาน");
        assert_eq!(response_text(body, "missing,text").unwrap(), "raw");
        assert_eq!(
            response_text(body, "results.0.transcript").unwrap(),
            "nested"
        );
        // Present but empty (silence) is text, not an error.
        assert_eq!(response_text(r#"{"text": ""}"#, "text").unwrap(), "");
    }

    #[test]
    fn a_missing_path_names_the_keys_that_are_there() {
        let err = response_text(r#"{"transcript": "x", "lang": "th"}"#, "text")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no text at text"), "{err}");
        assert!(err.contains("transcript") && err.contains("lang"), "{err}");
    }

    #[test]
    fn plain_text_and_split_sara_am() {
        assert_eq!(response_text("สวัสดี\n", "text").unwrap(), "สวัสดี");
        let split = "\u{0E17}\u{0E4D}\u{0E32}"; // ทำ as NIKHAHIT + SARA AA
        let body = format!(r#"{{"text": "{split}"}}"#);
        assert_eq!(response_text(&body, "text").unwrap(), "ทำ");
    }

    #[test]
    fn error_message_reads_common_error_shapes() {
        assert_eq!(error_message(r#"{"detail": "bad lang"}"#), "bad lang");
        assert_eq!(
            error_message(r#"{"error": {"message": "bad key"}}"#),
            "bad key"
        );
        assert_eq!(error_message(r#"{"error": "nope"}"#), "nope");
        assert_eq!(error_message("Bad Gateway"), "Bad Gateway");
    }

    #[test]
    fn cleaning_trims_drops_and_validates() {
        let mut s = server("  https://asr.example/v1/x  ");
        s.fields = vec![pair(" lang ", " th-mix "), pair("  ", "dropped")];
        s.file_field = " ".to_string();
        s.text_paths = String::new();
        s.timeout_secs = 0;
        let s = clean_remote_server(s).unwrap();
        assert_eq!(s.url, "https://asr.example/v1/x");
        assert_eq!(s.fields, vec![pair("lang", "th-mix"), pair("", "dropped")]);
        assert_eq!(s.file_field, "file");
        assert_eq!(s.text_paths, "text");
        assert_eq!(s.timeout_secs, 120);

        assert_eq!(
            clean_remote_server(server("ftp://x")).unwrap_err().code,
            "urlScheme"
        );
        assert_eq!(
            clean_remote_server(server("not a url")).unwrap_err().code,
            "invalidUrl"
        );
        assert!(
            clean_remote_server(server("")).is_ok(),
            "a draft may lack a URL"
        );
    }

    /// A one-request HTTP server that answers with `body` and hands back
    /// the request it read.
    fn serve_once(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                let Some(head_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length: usize = text
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if request.len() >= head_end + 4 + length || n == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, handle)
    }

    #[test]
    fn sends_what_the_server_settings_describe() {
        let (url, server_thread) = serve_once(r#"{"result": {"words": "สวัสดีครับ"}}"#);
        let mut s = server(&format!("{url}/custom/path"));
        s.api_key = "secret".to_string().into();
        s.headers.push(pair("X-Team", "dictation"));
        s.fields = vec![
            pair("model", "thai-asr"),
            pair("language", "{language}"),
            pair("lang", "th-mix"),
        ];
        s.file_field = "audio".to_string();
        s.text_paths = "result.words".to_string();
        let target = RemoteTarget::new(s, "th");
        assert_eq!(
            target.transcribe(&[0.0; 1600], &|| false).unwrap(),
            "สวัสดีครับ"
        );

        let request = server_thread.join().unwrap();
        let lower = request.to_lowercase();
        assert!(request.starts_with("POST /custom/path "));
        assert!(lower.contains("authorization: bearer secret"));
        assert!(lower.contains("x-team: dictation"));
        assert!(request.contains("name=\"audio\"; filename=\"audio.wav\""));
        assert!(request.contains("name=\"model\"\r\n\r\nthai-asr"));
        assert!(request.contains("name=\"language\"\r\n\r\nth"));
        assert!(request.contains("name=\"lang\"\r\n\r\nth-mix"));
    }

    #[test]
    fn url_placeholders_fill_or_drop_query_parameters() {
        let mut s = server(
            "https://api.example.com/v1/listen?model=nova-3&language={language}&key={api_key}&mode={language|multi}",
        );
        s.api_key = "a b&c".to_string().into();
        let auto = RemoteTarget::new(s.clone(), "auto");
        assert_eq!(
            auto.url().unwrap().as_str(),
            "https://api.example.com/v1/listen?model=nova-3&key=a+b%26c&mode=multi"
        );
        let thai = RemoteTarget::new(s, "th");
        assert_eq!(
            thai.url().unwrap().as_str(),
            "https://api.example.com/v1/listen?model=nova-3&language=th&key=a+b%26c&mode=th"
        );
        assert_eq!(thai.requested_language(), Some("th".to_string()));

        let path = RemoteTarget::new(server("https://x/{language}/asr"), "auto");
        assert!(path.url().is_err(), "a path placeholder can't be dropped");
    }

    #[test]
    fn json_bodies_escape_values_and_must_parse() {
        let filled = fill_json_body(
            r#"{"audio": "{audio_base64}", "prompt": "lang={language|auto}"}"#,
            &[(AUDIO_BASE64, Some("QUJD")), (LANGUAGE, Some(r#"th" "x"#))],
        )
        .unwrap();
        let json: Value = serde_json::from_str(&filled).unwrap();
        assert_eq!(json["audio"], "QUJD");
        assert_eq!(json["prompt"], r#"lang=th" "x"#);

        let missing = fill_json_body(r#"{"l": "{language}"}"#, &[(LANGUAGE, None)]);
        assert_eq!(
            missing
                .unwrap_err()
                .downcast_ref::<Failure>()
                .map(|f| f.code),
            Some("placeholder")
        );

        let mut s = server("http://x");
        s.body = RemoteBody::Json;
        s.json_body = r#"{"a": 1}"#.to_string();
        assert_eq!(
            clean_remote_server(s.clone()).unwrap_err().code,
            "jsonNeedsAudio"
        );
        s.json_body = r#"{"a": "{audio_base64}",}"#.to_string();
        assert_eq!(
            clean_remote_server(s.clone()).unwrap_err().code,
            "jsonInvalid"
        );
        s.json_body = r#"{"a": "{audio_base64}"}"#.to_string();
        assert!(clean_remote_server(s).is_ok());
    }

    #[test]
    fn wildcard_paths_join_every_piece() {
        let body = r#"{"results": [
            {"alternatives": [{"transcript": "สวัสดี"}]},
            {"alternatives": [{"transcript": " ครับ "}]},
            {"alternatives": []}
        ]}"#;
        assert_eq!(
            response_text(body, "results.*.alternatives.0.transcript").unwrap(),
            "สวัสดีครับ"
        );
        assert!(response_text(body, "results.*.missing").is_err());
    }

    #[test]
    fn sends_a_raw_body() {
        let (url, server_thread) = serve_once(
            r#"{"results": {"channels": [{"alternatives": [{"transcript": "hello"}]}]}}"#,
        );
        let mut s = server(&format!("{url}/v1/listen?language={{language|multi}}"));
        s.body = RemoteBody::Raw;
        s.api_key = "dg".to_string().into();
        s.headers = vec![pair("Authorization", "Token {api_key}")];
        s.fields = vec![pair("ignored", "for raw")];
        s.text_paths = "results.channels.0.alternatives.0.transcript".to_string();
        let target = RemoteTarget::new(s, "auto");
        assert_eq!(target.transcribe(&[0.0; 1600], &|| false).unwrap(), "hello");

        let request = server_thread.join().unwrap();
        let lower = request.to_lowercase();
        assert!(request.starts_with("POST /v1/listen?language=multi "));
        assert!(lower.contains("authorization: token dg"));
        assert!(lower.contains("content-type: audio/wav"));
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        assert!(body.starts_with("RIFF"));
        assert!(!request.contains("ignored"));
    }

    #[test]
    fn sends_a_json_body_with_the_audio_in_base64() {
        let (url, server_thread) =
            serve_once(r#"{"choices": [{"message": {"content": "hello there"}}]}"#);
        let mut s = server(&format!("{url}/v1/chat/completions"));
        s.body = RemoteBody::Json;
        s.json_body = r#"{"messages": [{"role": "user", "content": [
            {"type": "input_audio", "input_audio": {"data": "{audio_base64}", "format": "wav"}}
        ]}]}"#
            .to_string();
        s.text_paths = "choices.0.message.content".to_string();
        let target = RemoteTarget::new(s, "en");
        assert_eq!(
            target.transcribe(&[0.0; 1600], &|| false).unwrap(),
            "hello there"
        );

        let request = server_thread.join().unwrap();
        assert!(request
            .to_lowercase()
            .contains("content-type: application/json"));
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: Value = serde_json::from_str(body).unwrap();
        let audio = json["messages"][0]["content"][0]["input_audio"]["data"]
            .as_str()
            .unwrap();
        let wav = base64::engine::general_purpose::STANDARD
            .decode(audio)
            .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 1600 * 2);
    }

    #[test]
    fn chat_requests_carry_the_prompt_the_audio_and_typed_fields() {
        let (url, server_thread) =
            serve_once(r#"{"choices": [{"message": {"content": "สวัสดีครับ"}}]}"#);
        let mut s = server(&format!("{url}/v1/chat/completions"));
        s.body = RemoteBody::Chat;
        s.prompt = "Transcribe the audio. It is in {language|any language}.".to_string();
        s.fields = vec![
            pair("model", "qwen2.5-omni"),
            pair("temperature", "0"),
            pair("stream", "false"),
            pair("left_blank", ""),
        ];
        s.text_paths = "choices.0.message.content".to_string();
        let target = RemoteTarget::new(s, "th");
        assert_eq!(
            target.transcribe(&[0.0; 1600], &|| false).unwrap(),
            "สวัสดีครับ"
        );
        assert_eq!(target.requested_language(), Some("th".to_string()));

        let request = server_thread.join().unwrap();
        let body = &request[request.find("\r\n\r\n").unwrap() + 4..];
        let json: Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["model"], "qwen2.5-omni");
        assert_eq!(json["temperature"], 0);
        assert_eq!(json["stream"], false);
        assert!(json.get("left_blank").is_none());
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(
            json["messages"][0]["content"],
            "Transcribe the audio. It is in th."
        );
        let audio = &json["messages"][1]["content"][0];
        assert_eq!(audio["type"], "input_audio");
        assert_eq!(audio["input_audio"]["format"], "wav");
        let wav = base64::engine::general_purpose::STANDARD
            .decode(audio["input_audio"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(&wav[..4], b"RIFF");
    }

    #[test]
    fn a_chat_request_without_a_prompt_has_no_system_message() {
        let mut s = server("http://x");
        s.body = RemoteBody::Chat;
        let body = RemoteTarget::new(s, "auto").chat_body(b"RIFF").unwrap();
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["messages"].as_array().unwrap().len(), 1);
        assert_eq!(json["messages"][0]["role"], "user");
    }

    #[test]
    fn field_values_keep_their_json_type_only_when_unambiguous() {
        assert_eq!(typed_json("0.2"), json!(0.2));
        assert_eq!(typed_json("true"), json!(true));
        assert_eq!(typed_json("whisper-1"), json!("whisper-1"));
        // Not turned into an object or an array.
        assert_eq!(typed_json("[1]"), json!("[1]"));
        // Quotes force a string.
        assert_eq!(typed_json("\"4\""), json!("4"));
        assert_eq!(typed_json("4"), json!(4));
    }

    #[test]
    fn a_form_upload_ignores_a_content_type_header() {
        let (url, server_thread) = serve_once(r#"{"text": "ok"}"#);
        let mut s = server(&url);
        s.headers = vec![
            pair("Content-Type", "multipart/form-data"),
            pair("X-A", "b"),
        ];
        RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap();
        let request = server_thread.join().unwrap().to_lowercase();
        assert_eq!(
            request.matches("content-type: multipart/form-data").count(),
            1
        );
        assert!(request.contains("content-type: multipart/form-data; boundary="));
        assert!(request.contains("x-a: b"));
    }

    #[test]
    fn a_blank_form_field_is_left_out() {
        let (url, server_thread) = serve_once(r#"{"text": "ok"}"#);
        let mut s = server(&url);
        s.fields = vec![
            pair("model", ""),
            pair("", "nameless"),
            pair("task", "transcribe"),
        ];
        RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap();
        let request = server_thread.join().unwrap();
        assert!(!request.contains("name=\"model\""));
        assert!(!request.contains("nameless"));
        assert!(request.contains("name=\"task\"\r\n\r\ntranscribe"));
    }

    #[test]
    fn null_text_is_silence_and_bare_words_are_plain_text() {
        assert_eq!(response_text(r#"{"text": null}"#, "text").unwrap(), "");
        // A null first path still falls through to one with text.
        assert_eq!(
            response_text(
                r#"{"display_text": null, "text": "hi"}"#,
                "display_text, text"
            )
            .unwrap(),
            "hi"
        );
        // Plain-text replies that happen to parse as JSON.
        assert_eq!(response_text("42", "text").unwrap(), "42");
        assert_eq!(response_text("true", "text").unwrap(), "true");
        // A bare null is nothing said, like `"text": null`.
        assert_eq!(response_text("null", "text").unwrap(), "");
    }

    /// A dictation calls this from a task that blocks a runtime worker.
    #[test]
    fn works_when_called_from_inside_the_runtime() {
        let (url, server_thread) = serve_once(r#"{"text": "ok"}"#);
        let target = RemoteTarget::new(server(&url), "auto");
        let (tx, rx) = mpsc::channel();
        tauri::async_runtime::spawn(async move {
            // Blocks this worker, as transcribe_recording does.
            let _ = tx.send(target.transcribe(&[0.0; 160], &|| false));
        });
        let text = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the request deadlocked")
            .unwrap();
        assert_eq!(text, "ok");
        server_thread.join().unwrap();
    }

    #[test]
    fn a_cancel_closes_the_connection() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        // Reads until the client hangs up, and reports how long that took.
        let server_thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let started = Instant::now();
            let mut buf = [0u8; 4096];
            while matches!(stream.read(&mut buf), Ok(n) if n > 0) {}
            started.elapsed()
        });
        let target = RemoteTarget::new(server(&url), "auto");
        let started = Instant::now();
        let err = target
            .transcribe(&[0.0; 160], &|| {
                started.elapsed() > Duration::from_millis(200)
            })
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<EngineError>(),
            Some(EngineError::Cancelled)
        ));
        // Well inside the server's 10 s request timeout: the client closed it.
        let open_for = server_thread.join().unwrap();
        assert!(open_for < Duration::from_secs(3), "{open_for:?}");
    }

    #[test]
    fn a_cancel_stops_waiting_for_a_slow_server() {
        // Accepts the connection and never answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hold = std::thread::spawn(move || {
            let _conn = listener.accept();
            std::thread::sleep(Duration::from_secs(3));
        });
        let target = RemoteTarget::new(server(&url), "auto");
        let started = Instant::now();
        let cancel_after = Duration::from_millis(200);
        let err = target
            .transcribe(&[0.0; 160], &|| started.elapsed() > cancel_after)
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<EngineError>(),
            Some(EngineError::Cancelled)
        ));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        drop(hold);
    }

    #[test]
    fn a_web_page_or_a_stream_is_not_a_transcript() {
        assert!(check_reply_kind("text/html; charset=utf-8", "<html>Sign in</html>").is_err());
        assert!(check_reply_kind("", "  <!DOCTYPE html><html>").is_err());
        assert!(check_reply_kind("text/event-stream", "data: {}").is_err());
        assert!(check_reply_kind("application/x-ndjson", "{}").is_err());
        assert!(check_reply_kind("application/json", r#"{"text": "<b>hi</b>"}"#).is_ok());
        assert!(check_reply_kind("text/plain", "สวัสดี").is_ok());
    }

    #[test]
    fn placeholders_are_matched_whole() {
        assert!(uses_placeholder("lang={language}", LANGUAGE));
        assert!(uses_placeholder("{language|auto}", LANGUAGE));
        assert!(!uses_placeholder("{languages}", LANGUAGE));
    }

    /// Answers one request with `status` and an empty JSON object.
    fn answer_with(status: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                format!("HTTP/1.1 {status}\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}")
                    .as_bytes(),
            );
        });
        url
    }

    #[test]
    fn only_failures_worth_retrying_are_transient() {
        let transient = |url: &str| {
            let err = RemoteTarget::new(server(url), "auto")
                .transcribe(&[0.0; 160], &|| false)
                .unwrap_err();
            is_transient(&err)
        };
        assert!(transient(&answer_with("503 Service Unavailable")));
        assert!(transient(&answer_with("429 Too Many Requests")));
        assert!(transient("http://127.0.0.1:1/x"), "connection refused");
        assert!(
            !transient("http://handy-no-such-host.invalid/x"),
            "a bad name stays bad"
        );
        assert!(!transient(&answer_with("401 Unauthorized")));
        // A gateway timeout already waited the whole time once.
        assert!(!transient(&answer_with("504 Gateway Timeout")));
        assert!(!transient(&answer_with("422 Unprocessable Entity")));
    }

    #[test]
    fn a_bad_header_is_refused_when_saved() {
        let mut s = server("http://x");
        s.headers = vec![pair("X Bad Name", "v")];
        assert_eq!(
            clean_remote_server(s.clone()).unwrap_err().code,
            "invalidHeader"
        );
        s.headers = vec![pair("X-Ok", "line\nbreak")];
        assert_eq!(
            clean_remote_server(s.clone()).unwrap_err().code,
            "invalidHeader"
        );
        s.headers = vec![
            pair("", "row still being typed"),
            pair("X-Ok", "Bearer {api_key}"),
        ];
        assert!(clean_remote_server(s).is_ok());
    }

    #[test]
    fn logs_and_errors_never_show_what_the_url_or_headers_carry() {
        let mut s = server("http://127.0.0.1:1/listen?key=SECRET-IN-URL");
        s.headers = vec![pair("xi-api-key", "SECRET-IN-HEADER")];
        s.json_body = "SECRET-IN-BODY".to_string();
        s.api_key = "SECRET-KEY".to_string().into();
        let debug = format!("{s:?}");
        assert!(!debug.contains("SECRET"), "{debug}");
        assert!(debug.contains("xi-api-key"));

        let err = RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(!format!("{err:#}").contains("SECRET"), "{err:#}");
    }

    #[test]
    fn control_characters_never_reach_the_paste() {
        // An escape sequence and a bell around ordinary text.
        let body = r#"{"text": "\u001b[2Jls\u0007 -la\r\nline two\ttabbed\u0000"}"#;
        assert_eq!(
            response_text(body, "text").unwrap(),
            "[2Jls -la\nline two\ttabbed"
        );
        assert_eq!(
            response_text("plain\u{1b}]0;x\u{7}text", "text").unwrap(),
            "plain]0;xtext"
        );
    }

    /// Answers one request with `head` (status line and headers) and `body`.
    fn raw_reply(head: String, body: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        url
    }

    #[test]
    fn a_redirect_to_another_host_is_refused() {
        // A form upload can't be replayed, so it is never followed; a raw
        // body can, and is stopped by the same-host rule.
        for body in [RemoteBody::Multipart, RemoteBody::Raw] {
            // Would receive the recording and the key header if followed.
            let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            elsewhere.set_nonblocking(true).unwrap();
            let port = elsewhere.local_addr().unwrap().port();
            let url = raw_reply(
                format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://localhost:{port}/steal\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                ),
                Vec::new(),
            );
            let mut s = server(&url);
            s.body = body;
            s.headers = vec![pair("xi-api-key", "SECRET")];
            let err = RemoteTarget::new(s, "auto")
                .transcribe(&[0.0; 160], &|| false)
                .unwrap_err();
            let message = format!("{err:#}");
            if body == RemoteBody::Raw {
                assert!(message.contains("redirected somewhere else"), "{message}");
            }
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                elsewhere.accept().is_err(),
                "{body:?}: the other host was contacted ({message})"
            );
        }
    }

    #[test]
    fn an_oversized_reply_is_not_loaded() {
        let size = REPLY_LIMIT_BYTES + 1024;
        let url = raw_reply(
            format!("HTTP/1.1 200 OK\r\ncontent-length: {size}\r\nconnection: close\r\n\r\n"),
            vec![b'a'; size],
        );
        let err = RemoteTarget::new(server(&url), "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<Failure>().map(|f| f.code),
            Some("tooLarge"),
            "{err:#}"
        );
    }

    #[test]
    fn a_redirect_to_another_port_or_to_http_is_refused() {
        let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        elsewhere.set_nonblocking(true).unwrap();
        let port = elsewhere.local_addr().unwrap().port();
        // Same host (127.0.0.1), another port.
        let url = raw_reply(
            format!(
                "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://127.0.0.1:{port}/x\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            ),
            Vec::new(),
        );
        let mut s = server(&url);
        s.body = RemoteBody::Raw;
        let err = RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("redirected somewhere else"),
            "{err:#}"
        );
        std::thread::sleep(Duration::from_millis(200));
        assert!(elsewhere.accept().is_err());
    }

    #[test]
    fn a_timeout_is_not_sent_again() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hold = std::thread::spawn(move || {
            let _conn = listener.accept();
            std::thread::sleep(Duration::from_secs(3));
        });
        let mut s = server(&url);
        s.timeout_secs = 1;
        let err = RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<Failure>().map(|f| f.code),
            Some("noAnswer"),
            "{err:#}"
        );
        assert!(!is_transient(&err));
        drop(hold);
    }

    #[test]
    fn a_placeholder_in_the_path_stays_one_segment() {
        let mut s = server("https://asr.example.com/v1/{api_key}/listen?lang={language}");
        s.api_key = "ab/c+d?e#f".to_string().into();
        let url = RemoteTarget::new(s, "th").url().unwrap();
        assert_eq!(url.path(), "/v1/ab%2Fc%2Bd%3Fe%23f/listen");
        assert_eq!(url.query(), Some("lang=th"));
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn a_language_header_counts_as_naming_the_language() {
        let mut s = server("http://x");
        s.headers = vec![pair("Accept-Language", "{language}")];
        assert_eq!(
            RemoteTarget::new(s, "th").requested_language(),
            Some("th".to_string())
        );
    }

    #[test]
    fn settings_get_codes_they_can_translate() {
        let refused = RemoteTarget::new(server(&answer_with("401 Unauthorized")), "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(coded_error(&refused).starts_with("refused|401 Unauthorized"));
        let none = test_remote_transcription(server(""), "auto").unwrap_err();
        assert_eq!(coded_error(&none), "noUrl|");
        let bad = test_remote_transcription(server("ftp://x"), "auto").unwrap_err();
        assert_eq!(coded_error(&bad), "urlScheme|ftp://x");
        // Logs and notices still read as English.
        assert_eq!(none.to_string(), "no URL is set");
    }

    #[test]
    fn one_damaged_server_doesnt_cost_the_others() {
        let settings: AppSettings = serde_json::from_value(json!({
            "remote_transcription_servers": [
                { "id": "good", "name": "GPU box", "url": "http://gpu.lan:18081/v1/transcribe" },
                { "id": "newer", "url": "http://x", "body": "xml" },
                { "id": "bad", "url": "http://y", "timeout_secs": null }
            ]
        }))
        .unwrap();
        let ids: Vec<_> = settings
            .remote_transcription_servers
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(ids, ["good"]);
    }

    #[test]
    fn a_cancel_before_the_request_still_stops_it() {
        // The cancel count moved after the recording started (mark 0) but
        // before this request: it must not be sent at all.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let epoch = std::sync::atomic::AtomicU64::new(1);
        let mark = 0;
        let started = Instant::now();
        let err = RemoteTarget::new(server(&url), "auto")
            .transcribe(&[0.0; 160], &|| {
                epoch.load(std::sync::atomic::Ordering::Acquire) != mark
            })
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<EngineError>(),
            Some(EngineError::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_keychain_error_is_not_kept() {
        let mut s = server("http://x");
        s.has_api_key = true;
        s.id = "handy-test-no-such-key".to_string();
        let target = RemoteTarget::new(s, "auto");
        // No key there: an error, every time it's asked.
        assert!(target.with_stored_key().is_err());
        assert!(target.with_stored_key().is_err());
    }

    #[test]
    fn a_json_body_or_prompt_needs_a_language_fallback() {
        let mut s = server("http://x");
        s.body = RemoteBody::Json;
        s.json_body = r#"{"a": "{audio_base64}", "l": "{language}"}"#.to_string();
        let err = clean_remote_server(s.clone()).unwrap_err();
        assert_eq!((err.code, err.detail.as_str()), ("placeholder", "JSON"));
        s.json_body = r#"{"a": "{audio_base64}", "l": "{language|auto}"}"#.to_string();
        assert!(clean_remote_server(s.clone()).is_ok());

        s.body = RemoteBody::Chat;
        s.prompt = "Transcribe. Language: {language}".to_string();
        let err = clean_remote_server(s.clone()).unwrap_err();
        assert_eq!((err.code, err.detail.as_str()), ("placeholder", "Prompt"));
        s.prompt = "Transcribe. Language: {language|any}".to_string();
        assert!(clean_remote_server(s).is_ok());
    }

    /// Answers two requests in turn with `replies`, returning the request
    /// lines it got.
    fn serve_replies(replies: [String; 2]) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut lines = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buf = [0u8; 65536];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request);
                    let Some(head_end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length: usize = text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length || n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&request).into_owned();
                lines.push(format!(
                    "{} {}",
                    text.lines().next().unwrap_or(""),
                    text.contains("RIFF")
                ));
                stream.write_all(reply.as_bytes()).unwrap();
            }
            lines
        });
        (url, handle)
    }

    #[test]
    fn a_form_upload_follows_a_same_origin_307_with_the_recording() {
        let body = r#"{"text": "moved"}"#;
        let (url, handle) = serve_replies([
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: /v1/transcribe/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(),
            format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()),
        ]);
        let text = RemoteTarget::new(server(&format!("{url}/v1/transcribe")), "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap();
        assert_eq!(text, "moved");
        let lines = handle.join().unwrap();
        assert_eq!(
            lines,
            [
                "POST /v1/transcribe HTTP/1.1 true",
                "POST /v1/transcribe/ HTTP/1.1 true"
            ]
        );
    }

    #[test]
    fn one_spoken_language_is_sent_when_the_language_is_auto() {
        let mut settings = crate::settings::get_default_settings();
        let mut s = server("http://127.0.0.1:1/x?lang={language}");
        s.id = "only".to_string();
        settings.remote_transcription_enabled = true;
        settings.remote_transcription_servers = vec![s];
        settings.remote_transcription_server_id = Some("only".to_string());
        settings.selected_language = "auto".to_string();
        settings.spoken_languages = vec!["th".to_string()];
        let target = RemoteTarget::from_settings(&settings).unwrap();
        assert_eq!(target.requested_language().as_deref(), Some("th"));
        settings.spoken_languages = vec!["th".to_string(), "en".to_string()];
        let target = RemoteTarget::from_settings(&settings).unwrap();
        assert_eq!(target.requested_language(), None);
    }

    #[test]
    fn a_redirect_that_would_drop_the_recording_is_refused() {
        let url = raw_reply(
            "HTTP/1.1 302 Found\r\nlocation: /v1/transcribe/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(),
            Vec::new(),
        );
        let err = RemoteTarget::new(server(&url), "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(format!("{err:#}").contains("use that URL"), "{err:#}");
    }

    #[test]
    fn a_reply_cut_off_after_the_server_answered_is_not_sent_again() {
        // Headers promise a body, then the connection closes: the server
        // already transcribed it.
        let url = raw_reply(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 500\r\n\r\n"
                .to_string(),
            b"{\"text\": \"partial".to_vec(),
        );
        let err = RemoteTarget::new(server(&url), "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(!is_transient(&err), "{err:#}");
    }

    #[test]
    fn errors_never_carry_the_url_query() {
        let mut s = server("http://127.0.0.1:1/listen?key={api_key}");
        s.api_key = "SECRET-KEY".to_string().into();
        let err = RemoteTarget::new(s, "auto")
            .transcribe(&[0.0; 160], &|| false)
            .unwrap_err();
        assert!(!format!("{err:#} {err:?}").contains("SECRET"), "{err:#}");
    }

    #[test]
    fn an_unreachable_server_is_an_error() {
        let target = RemoteTarget::new(server("http://127.0.0.1:1/x"), "auto");
        let err = target.transcribe(&[0.0; 160], &|| false).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.starts_with("couldn't connect to 127.0.0.1:1 ("),
            "{msg}"
        );
        assert!(!msg.contains("error sending request"), "{msg}");
    }
}
