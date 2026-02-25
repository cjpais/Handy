use crate::managers::transcription::TranscriptionManager;
use crate::settings::{ApiNetworkScope, AppSettings};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use log::{error, info, warn};
use reqwest::Url;
use rodio::{Decoder, Source};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::AppHandle;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const HTTP_PORT: u16 = 5500;
const WYOMING_PORT: u16 = 10300;
const REQUIRED_SAMPLE_RATE: u32 = 16_000;

trait Transcriber: Send + Sync {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String>;
}

struct ManagerTranscriber {
    manager: Arc<TranscriptionManager>,
}

impl Transcriber for ManagerTranscriber {
    fn transcribe(&self, samples: Vec<f32>) -> Result<String> {
        self.manager.initiate_model_load();
        self.manager.transcribe(samples)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalApiRuntimeConfig {
    enabled: bool,
    scope: ApiNetworkScope,
    token: Option<String>,
}

impl LocalApiRuntimeConfig {
    fn from_settings(settings: &AppSettings) -> Self {
        Self {
            enabled: settings.local_api_enabled,
            scope: settings.local_api_network_scope,
            token: settings.local_api_token.as_ref().and_then(|token| {
                let trimmed = token.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }),
        }
    }

    fn host(&self) -> &'static str {
        match self.scope {
            ApiNetworkScope::Loopback => "127.0.0.1",
            ApiNetworkScope::LocalNetwork => "0.0.0.0",
        }
    }

    fn http_addr(&self) -> String {
        format!("{}:{}", self.host(), HTTP_PORT)
    }

    fn wyoming_addr(&self) -> String {
        format!("{}:{}", self.host(), WYOMING_PORT)
    }
}

struct ServerWorker {
    shutdown: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

impl ServerWorker {
    fn stop(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Err(err) = self.handle.join() {
            warn!("Failed to join API server thread: {:?}", err);
        }
    }
}

pub struct ApiServerManager {
    transcriber: Arc<dyn Transcriber>,
    current_config: Mutex<Option<LocalApiRuntimeConfig>>,
    http_worker: Mutex<Option<ServerWorker>>,
    wyoming_worker: Mutex<Option<ServerWorker>>,
}

impl ApiServerManager {
    pub fn new(app_handle: &AppHandle, transcription_manager: Arc<TranscriptionManager>) -> Self {
        let transcriber: Arc<dyn Transcriber> = Arc::new(ManagerTranscriber {
            manager: transcription_manager,
        });

        let manager = Self {
            transcriber,
            current_config: Mutex::new(None),
            http_worker: Mutex::new(None),
            wyoming_worker: Mutex::new(None),
        };

        let settings = crate::settings::get_settings(app_handle);
        manager.apply_settings(&settings);

        manager
    }

    pub fn apply_settings(&self, settings: &AppSettings) {
        let next = LocalApiRuntimeConfig::from_settings(settings);
        let mut current = self.current_config.lock().unwrap();

        if current.as_ref() == Some(&next) {
            return;
        }

        self.stop_all_workers();

        if next.enabled {
            self.start_workers(next.clone());
            info!(
                "Local API service enabled ({:?}); HTTP={}, Wyoming={}, auth={}",
                next.scope,
                next.http_addr(),
                next.wyoming_addr(),
                if next.token.is_some() { "on" } else { "off" }
            );
        } else {
            info!("Local API service disabled");
        }

        *current = Some(next);
    }

    fn start_workers(&self, config: LocalApiRuntimeConfig) {
        let http_shutdown = Arc::new(AtomicBool::new(false));
        let wyoming_shutdown = Arc::new(AtomicBool::new(false));

        let http_addr = config.http_addr();
        let wyoming_addr = config.wyoming_addr();
        let transcriber_for_http = self.transcriber.clone();
        let transcriber_for_wyoming = self.transcriber.clone();
        let http_config = config.clone();
        let wyoming_config = config;

        let http_shutdown_clone = http_shutdown.clone();
        let http_handle = thread::spawn(move || {
            run_http_server(
                &http_addr,
                transcriber_for_http,
                http_config,
                http_shutdown_clone,
            );
        });

        let wyoming_shutdown_clone = wyoming_shutdown.clone();
        let wyoming_handle = thread::spawn(move || {
            run_wyoming_server(
                &wyoming_addr,
                transcriber_for_wyoming,
                wyoming_config,
                wyoming_shutdown_clone,
            );
        });

        *self.http_worker.lock().unwrap() = Some(ServerWorker {
            shutdown: http_shutdown,
            handle: http_handle,
        });

        *self.wyoming_worker.lock().unwrap() = Some(ServerWorker {
            shutdown: wyoming_shutdown,
            handle: wyoming_handle,
        });
    }

    fn stop_all_workers(&self) {
        if let Some(worker) = self.http_worker.lock().unwrap().take() {
            worker.stop();
        }
        if let Some(worker) = self.wyoming_worker.lock().unwrap().take() {
            worker.stop();
        }
    }
}

impl Drop for ApiServerManager {
    fn drop(&mut self) {
        self.stop_all_workers();
    }
}

#[derive(Deserialize)]
struct JsonTranscriptionRequest {
    audio_base64: String,
    #[serde(default = "default_encoding")]
    encoding: String,
    sample_rate: Option<u32>,
    response_format: Option<String>,
}

fn default_encoding() -> String {
    "pcm_s16le".to_string()
}

#[derive(Clone, Copy)]
enum HttpResponseFormat {
    Json,
    Text,
    VerboseJson,
}

impl HttpResponseFormat {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("json").to_ascii_lowercase().as_str() {
            "text" => Self::Text,
            "verbose_json" => Self::VerboseJson,
            _ => Self::Json,
        }
    }
}

#[derive(Default)]
struct ParsedHttpTranscriptionRequest {
    samples: Vec<f32>,
    response_format: HttpResponseFormat,
    timestamp_granularities: Vec<String>,
}

impl Default for HttpResponseFormat {
    fn default() -> Self {
        HttpResponseFormat::Json
    }
}

#[derive(Serialize)]
struct HttpTranscriptionResponse {
    text: String,
}

fn run_http_server(
    addr: &str,
    transcriber: Arc<dyn Transcriber>,
    config: LocalApiRuntimeConfig,
    shutdown: Arc<AtomicBool>,
) {
    let server = match Server::http(addr) {
        Ok(server) => {
            info!(
                "Local OpenAI-style STT API listening on http://{}/v1/audio/transcriptions",
                addr
            );
            server
        }
        Err(err) => {
            error!("Failed to start local API server on {}: {}", addr, err);
            return;
        }
    };

    while !shutdown.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(250)) {
            Ok(Some(request)) => handle_http_request(request, &transcriber, &config),
            Ok(None) => continue,
            Err(err) => {
                warn!("Local API server receive error: {}", err);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_http_request(
    mut request: Request,
    transcriber: &Arc<dyn Transcriber>,
    config: &LocalApiRuntimeConfig,
) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let origin = get_header_value(&request, "Origin");

    let cors_origin = match evaluate_cors_origin(config.scope, origin.as_deref()) {
        Ok(value) => value,
        Err(err) => {
            respond_error(request, StatusCode(403), &err.to_string(), None, false);
            return;
        }
    };

    if method == Method::Options {
        respond(
            request,
            StatusCode(204),
            "{}",
            Some("application/json"),
            cors_origin.as_deref(),
            true,
        );
        return;
    }

    if method != Method::Post || url != "/v1/audio/transcriptions" {
        respond(
            request,
            StatusCode(404),
            "{\"error\":\"not found\"}",
            Some("application/json"),
            cors_origin.as_deref(),
            false,
        );
        return;
    }

    if let Err(err) = authorize_http_request(&request, config.token.as_deref()) {
        respond_error(
            request,
            StatusCode(401),
            &err.to_string(),
            cors_origin.as_deref(),
            false,
        );
        return;
    }

    let mut body = Vec::new();
    if let Err(err) = request.as_reader().read_to_end(&mut body) {
        respond_error(
            request,
            StatusCode(400),
            &format!("failed to read body: {}", err),
            cors_origin.as_deref(),
            false,
        );
        return;
    }

    let content_type = get_header_value(&request, "Content-Type").unwrap_or_default();
    let parsed = match parse_http_transcription_request(&body, &content_type) {
        Ok(parsed) => parsed,
        Err(err) => {
            respond_error(
                request,
                StatusCode(400),
                &err.to_string(),
                cors_origin.as_deref(),
                false,
            );
            return;
        }
    };

    let transcript = match transcriber.transcribe(parsed.samples) {
        Ok(text) => text,
        Err(err) => {
            respond_error(
                request,
                StatusCode(500),
                &err.to_string(),
                cors_origin.as_deref(),
                false,
            );
            return;
        }
    };

    let body = match parsed.response_format {
        HttpResponseFormat::Text => transcript.clone(),
        HttpResponseFormat::VerboseJson => {
            let include_segments = parsed
                .timestamp_granularities
                .iter()
                .any(|value| value == "segment");

            if include_segments {
                json!({
                    "task": "transcribe",
                    "language": "unknown",
                    "duration": 0.0,
                    "text": transcript,
                    "segments": [
                        {
                            "id": 0,
                            "start": 0.0,
                            "end": 0.0,
                            "text": transcript,
                            "tokens": []
                        }
                    ]
                })
                .to_string()
            } else {
                json!({
                    "task": "transcribe",
                    "language": "unknown",
                    "duration": 0.0,
                    "text": transcript
                })
                .to_string()
            }
        }
        HttpResponseFormat::Json => {
            serde_json::to_string(&HttpTranscriptionResponse { text: transcript })
                .unwrap_or_else(|_| "{\"text\":\"\"}".to_string())
        }
    };

    let content_type = match parsed.response_format {
        HttpResponseFormat::Text => Some("text/plain; charset=utf-8"),
        _ => Some("application/json"),
    };

    respond(
        request,
        StatusCode(200),
        &body,
        content_type,
        cors_origin.as_deref(),
        false,
    );
}

fn parse_http_transcription_request(
    body: &[u8],
    content_type: &str,
) -> Result<ParsedHttpTranscriptionRequest> {
    if let Some(boundary) = parse_multipart_boundary(content_type) {
        parse_multipart_transcription_request(body, &boundary)
    } else {
        parse_json_transcription_request(body)
    }
}

fn parse_json_transcription_request(body: &[u8]) -> Result<ParsedHttpTranscriptionRequest> {
    let payload: JsonTranscriptionRequest = serde_json::from_slice(body)
        .context("request must be JSON with audio_base64 and optional encoding/sample_rate")?;

    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.audio_base64.as_bytes())
        .context("invalid base64 audio payload")?;

    let samples = decode_samples(&audio_bytes, &payload.encoding, payload.sample_rate)?;

    Ok(ParsedHttpTranscriptionRequest {
        samples,
        response_format: HttpResponseFormat::parse(payload.response_format.as_deref()),
        timestamp_granularities: Vec::new(),
    })
}

#[derive(Default)]
struct MultipartFile {
    data: Vec<u8>,
}

#[derive(Default)]
struct MultipartFormData {
    file: Option<MultipartFile>,
    fields: HashMap<String, Vec<String>>,
}

impl MultipartFormData {
    fn push_field(&mut self, name: String, value: String) {
        self.fields.entry(name).or_default().push(value);
    }

    fn first_value(&self, name: &str) -> Option<&str> {
        self.fields
            .get(name)
            .and_then(|values| values.first().map(String::as_str))
    }
}

fn parse_multipart_transcription_request(
    body: &[u8],
    boundary: &str,
) -> Result<ParsedHttpTranscriptionRequest> {
    let form = parse_multipart_form_data(body, boundary)?;
    let file_data = form
        .file
        .as_ref()
        .ok_or_else(|| anyhow!("multipart request must include a 'file' field"))?;

    let samples = decode_audio_unknown(&file_data.data)?;
    let timestamp_granularities = form
        .fields
        .get("timestamp_granularities[]")
        .cloned()
        .unwrap_or_default();

    Ok(ParsedHttpTranscriptionRequest {
        samples,
        response_format: HttpResponseFormat::parse(form.first_value("response_format")),
        timestamp_granularities,
    })
}

fn parse_multipart_boundary(content_type: &str) -> Option<String> {
    let lowered = content_type.to_ascii_lowercase();
    if !lowered.starts_with("multipart/form-data") {
        return None;
    }

    for part in content_type.split(';').map(str::trim) {
        if let Some(value) = part.strip_prefix("boundary=") {
            let trimmed = value.trim_matches('"').trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

fn parse_multipart_form_data(body: &[u8], boundary: &str) -> Result<MultipartFormData> {
    let delimiter = format!("--{}", boundary).into_bytes();
    let mut marker = Vec::with_capacity(2 + boundary.len());
    marker.extend_from_slice(b"\r\n--");
    marker.extend_from_slice(boundary.as_bytes());

    let mut position = find_subslice_from(body, &delimiter, 0)
        .ok_or_else(|| anyhow!("invalid multipart body: boundary not found"))?;
    let mut form = MultipartFormData::default();

    loop {
        position += delimiter.len();

        if body.get(position..position + 2) == Some(b"--") {
            break;
        }

        if body.get(position..position + 2) != Some(b"\r\n") {
            return Err(anyhow!("invalid multipart body framing"));
        }
        position += 2;

        let header_end = find_subslice_from(body, b"\r\n\r\n", position)
            .ok_or_else(|| anyhow!("invalid multipart part headers"))?;
        let header_bytes = &body[position..header_end];
        let headers = parse_part_headers(header_bytes)?;
        let part_start = header_end + 4;
        let part_end = find_subslice_from(body, &marker, part_start)
            .ok_or_else(|| anyhow!("multipart part missing trailing boundary"))?;
        let part_data = &body[part_start..part_end];

        let disposition = headers
            .get("content-disposition")
            .ok_or_else(|| anyhow!("multipart part missing Content-Disposition header"))?;
        let name = parse_content_disposition_name(disposition)
            .ok_or_else(|| anyhow!("multipart part missing field name"))?;

        if name == "file" {
            form.file = Some(MultipartFile {
                data: part_data.to_vec(),
            });
        } else {
            let value = String::from_utf8(part_data.to_vec())
                .context("multipart text fields must be valid UTF-8")?;
            form.push_field(name, value);
        }

        position = part_end + 2;
    }

    Ok(form)
}

fn parse_part_headers(header_bytes: &[u8]) -> Result<HashMap<String, String>> {
    let mut headers = HashMap::new();
    for line in header_bytes.split(|byte| *byte == b'\n') {
        let line = String::from_utf8(line.to_vec())?.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid multipart part header"))?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    Ok(headers)
}

fn parse_content_disposition_name(value: &str) -> Option<String> {
    for part in value.split(';').map(str::trim) {
        if let Some(name) = part.strip_prefix("name=") {
            return Some(name.trim_matches('"').to_string());
        }
    }
    None
}

fn find_subslice_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }

    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

fn decode_samples(bytes: &[u8], encoding: &str, sample_rate: Option<u32>) -> Result<Vec<f32>> {
    match encoding.to_ascii_lowercase().as_str() {
        "pcm_s16le" => decode_pcm_s16le(bytes, sample_rate),
        "wav" => decode_wav(bytes),
        _ => Err(anyhow!(
            "unsupported encoding '{}'; expected pcm_s16le or wav",
            encoding
        )),
    }
}

fn decode_audio_unknown(bytes: &[u8]) -> Result<Vec<f32>> {
    if let Ok(wav) = decode_wav(bytes) {
        return Ok(wav);
    }

    decode_with_rodio(bytes)
}

fn decode_with_rodio(bytes: &[u8]) -> Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let decoder = Decoder::new(cursor).context("unsupported or invalid audio format")?;
    let input_rate = decoder.sample_rate();
    let channels = usize::from(decoder.channels().max(1));
    let interleaved: Vec<f32> = decoder.collect();

    if interleaved.is_empty() {
        return Err(anyhow!("audio payload is empty"));
    }

    let mono = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|frame| {
                let sum: f32 = frame.iter().copied().sum();
                sum / frame.len() as f32
            })
            .collect()
    };

    if input_rate == REQUIRED_SAMPLE_RATE {
        Ok(mono)
    } else {
        Ok(resample_linear(&mono, input_rate, REQUIRED_SAMPLE_RATE))
    }
}

fn resample_linear(samples: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if samples.is_empty() || in_rate == 0 || out_rate == 0 || in_rate == out_rate {
        return samples.to_vec();
    }

    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);

    for idx in 0..out_len {
        let src = idx as f64 / ratio;
        let left = src.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let frac = (src - left as f64) as f32;
        let value = samples[left] + (samples[right] - samples[left]) * frac;
        out.push(value);
    }

    out
}

fn decode_pcm_s16le(bytes: &[u8], sample_rate: Option<u32>) -> Result<Vec<f32>> {
    if sample_rate.unwrap_or(REQUIRED_SAMPLE_RATE) != REQUIRED_SAMPLE_RATE {
        return Err(anyhow!(
            "unsupported sample_rate (expected {} Hz)",
            REQUIRED_SAMPLE_RATE
        ));
    }

    if bytes.len() % 2 != 0 {
        return Err(anyhow!("pcm_s16le payload must have even byte length"));
    }

    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / i16::MAX as f32)
        .collect())
}

fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut reader = hound::WavReader::new(cursor).context("invalid WAV payload")?;
    let spec = reader.spec();

    if spec.channels != 1 {
        return Err(anyhow!("WAV payload must be mono"));
    }

    if spec.sample_rate != REQUIRED_SAMPLE_RATE {
        return Err(anyhow!("WAV payload must be {} Hz", REQUIRED_SAMPLE_RATE));
    }

    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => Ok(reader
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|sample| sample as f32 / i16::MAX as f32)
            .collect()),
        (hound::SampleFormat::Float, 32) => Ok(reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?),
        _ => Err(anyhow!(
            "unsupported WAV format; expected 16-bit PCM or 32-bit float"
        )),
    }
}

fn get_header_value(request: &Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().to_string())
}

fn authorize_http_request(request: &Request, required_token: Option<&str>) -> Result<()> {
    let Some(required_token) = required_token else {
        return Ok(());
    };

    let auth_header = get_header_value(request, "Authorization")
        .ok_or_else(|| anyhow!("missing Authorization header"))?;
    let provided = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| anyhow!("Authorization header must use Bearer token"))?;

    if provided == required_token {
        Ok(())
    } else {
        Err(anyhow!("invalid API token"))
    }
}

fn evaluate_cors_origin(scope: ApiNetworkScope, origin: Option<&str>) -> Result<Option<String>> {
    let Some(origin) = origin else {
        return Ok(None);
    };

    match scope {
        ApiNetworkScope::Loopback => {
            let parsed = Url::parse(origin).context("invalid Origin header")?;
            let host = parsed.host_str().unwrap_or_default();

            if host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1" {
                Ok(Some(origin.to_string()))
            } else {
                Err(anyhow!(
                    "Origin not allowed in loopback mode (use localhost/127.0.0.1)"
                ))
            }
        }
        ApiNetworkScope::LocalNetwork => Err(anyhow!(
            "browser cross-origin access is disabled in local network mode"
        )),
    }
}

fn respond(
    request: Request,
    status: StatusCode,
    body: &str,
    content_type: Option<&str>,
    cors_origin: Option<&str>,
    is_preflight: bool,
) {
    let mut response = Response::from_string(body.to_string()).with_status_code(status);

    if let Some(content_type) = content_type {
        if let Ok(header) = Header::from_bytes("Content-Type", content_type) {
            response.add_header(header);
        }
    }

    if let Some(origin) = cors_origin {
        if let Ok(header) = Header::from_bytes("Access-Control-Allow-Origin", origin) {
            response.add_header(header);
        }
        if let Ok(header) = Header::from_bytes("Vary", "Origin") {
            response.add_header(header);
        }
        if let Ok(header) = Header::from_bytes(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization",
        ) {
            response.add_header(header);
        }
        if let Ok(header) = Header::from_bytes("Access-Control-Allow-Methods", "POST, OPTIONS") {
            response.add_header(header);
        }
        if is_preflight {
            if let Ok(header) = Header::from_bytes("Access-Control-Max-Age", "600") {
                response.add_header(header);
            }
        }
    }

    if let Err(err) = request.respond(response) {
        warn!("Failed to send local API response: {}", err);
    }
}

fn respond_error(
    request: Request,
    status: StatusCode,
    message: &str,
    cors_origin: Option<&str>,
    is_preflight: bool,
) {
    let body = json!({
        "error": {
            "code": status.0,
            "message": message
        }
    })
    .to_string();

    respond(
        request,
        status,
        &body,
        Some("application/json"),
        cors_origin,
        is_preflight,
    );
}

#[derive(Debug, Deserialize)]
struct WyomingHeaderIn {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    payload_length: Option<usize>,
}

#[derive(Serialize)]
struct WyomingHeaderOut {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_length: Option<usize>,
}

fn run_wyoming_server(
    addr: &str,
    transcriber: Arc<dyn Transcriber>,
    config: LocalApiRuntimeConfig,
    shutdown: Arc<AtomicBool>,
) {
    let listener = match TcpListener::bind(addr) {
        Ok(listener) => {
            if let Err(err) = listener.set_nonblocking(true) {
                error!("Failed to set Wyoming listener non-blocking mode: {}", err);
                return;
            }
            info!("Wyoming-compatible ASR server listening on {}", addr);
            listener
        }
        Err(err) => {
            error!("Failed to bind Wyoming server on {}: {}", addr, err);
            return;
        }
    };

    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if let Err(err) = stream.set_nonblocking(false) {
                    warn!("Failed to set Wyoming stream blocking mode: {}", err);
                    continue;
                }
                let transcriber = transcriber.clone();
                let required_token = config.token.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_wyoming_connection(stream, transcriber, required_token)
                    {
                        warn!("Wyoming connection {} ended with error: {}", peer, err);
                    }
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => {
                warn!("Wyoming accept error: {}", err);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_wyoming_connection(
    stream: TcpStream,
    transcriber: Arc<dyn Transcriber>,
    required_token: Option<String>,
) -> Result<()> {
    let reader_stream = stream.try_clone().context("failed to clone TCP stream")?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = BufWriter::new(stream);

    let mut audio_buffer: Vec<u8> = Vec::new();
    let mut sample_rate: u32 = REQUIRED_SAMPLE_RATE;
    let mut authenticated = required_token.is_none();

    loop {
        let event = match read_wyoming_event(&mut reader)? {
            Some(event) => event,
            None => break,
        };

        match event.event_type.as_str() {
            "describe" => {
                let info_payload = json!({
                    "asr": [{
                        "name": "handy",
                        "description": "Handy local ASR",
                        "attribution": { "name": "Handy" }
                    }],
                    "auth_required": required_token.is_some()
                });
                write_wyoming_event(&mut writer, "info", Some(info_payload), None)?;
            }
            "auth" => {
                let provided = event.data.get("token").and_then(Value::as_str);
                if provided == required_token.as_deref() {
                    authenticated = true;
                    write_wyoming_event(
                        &mut writer,
                        "auth-ok",
                        Some(json!({ "status": "ok" })),
                        None,
                    )?;
                } else {
                    write_wyoming_event(
                        &mut writer,
                        "error",
                        Some(json!({ "message": "invalid token" })),
                        None,
                    )?;
                    break;
                }
            }
            "audio-start" | "audio-chunk" | "audio-stop" | "transcribe" => {
                if !authenticated {
                    write_wyoming_event(
                        &mut writer,
                        "error",
                        Some(json!({ "message": "unauthorized: send auth event first" })),
                        None,
                    )?;
                    break;
                }

                match event.event_type.as_str() {
                    "audio-start" => {
                        audio_buffer.clear();
                        sample_rate = event
                            .data
                            .get("rate")
                            .and_then(Value::as_u64)
                            .map(|rate| rate as u32)
                            .unwrap_or(REQUIRED_SAMPLE_RATE);
                    }
                    "audio-chunk" => {
                        if let Some(payload) =
                            event_payload_bytes(event.payload_length, &mut reader)?
                        {
                            audio_buffer.extend_from_slice(&payload);
                        }
                    }
                    "audio-stop" => {
                        let samples = decode_pcm_s16le(&audio_buffer, Some(sample_rate))?;
                        let transcript = transcriber.transcribe(samples)?;
                        write_wyoming_event(
                            &mut writer,
                            "transcript",
                            Some(json!({ "text": transcript })),
                            None,
                        )?;
                        audio_buffer.clear();
                    }
                    _ => {}
                }
            }
            _ => {
                write_wyoming_event(
                    &mut writer,
                    "error",
                    Some(json!({
                        "message": format!("unsupported event type '{}'", event.event_type)
                    })),
                    None,
                )?;
            }
        }
    }

    Ok(())
}

fn read_wyoming_event<R: BufRead>(reader: &mut R) -> Result<Option<WyomingHeaderIn>> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        return Ok(None);
    }

    let header: WyomingHeaderIn =
        serde_json::from_str(line.trim_end()).context("invalid Wyoming JSON header")?;
    Ok(Some(header))
}

fn event_payload_bytes<R: Read>(
    payload_length: Option<usize>,
    reader: &mut R,
) -> Result<Option<Vec<u8>>> {
    let Some(payload_length) = payload_length else {
        return Ok(None);
    };

    let mut payload = vec![0_u8; payload_length];
    reader
        .read_exact(&mut payload)
        .context("failed to read Wyoming payload bytes")?;
    Ok(Some(payload))
}

fn write_wyoming_event<W: Write>(
    writer: &mut W,
    event_type: &str,
    data: Option<Value>,
    payload: Option<&[u8]>,
) -> Result<()> {
    let header = WyomingHeaderOut {
        event_type: event_type.to_string(),
        data,
        payload_length: payload.map(|bytes| bytes.len()),
    };

    let mut line = serde_json::to_string(&header)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;

    if let Some(payload) = payload {
        writer.write_all(payload)?;
    }

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use std::io::Read;
    use std::net::TcpStream;

    struct MockTranscriber;

    impl Transcriber for MockTranscriber {
        fn transcribe(&self, _samples: Vec<f32>) -> Result<String> {
            Ok("mock transcript".to_string())
        }
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    fn spawn_http_server(
        config: LocalApiRuntimeConfig,
    ) -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
        let port = free_port();
        let addr = format!("127.0.0.1:{}", port);
        let addr_for_thread = addr.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        let transcriber: Arc<dyn Transcriber> = Arc::new(MockTranscriber);

        let handle = thread::spawn(move || {
            run_http_server(&addr_for_thread, transcriber, config, shutdown_clone);
        });

        thread::sleep(Duration::from_millis(120));
        (addr, shutdown, handle)
    }

    fn spawn_wyoming_server(
        config: LocalApiRuntimeConfig,
    ) -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
        let port = free_port();
        let addr = format!("127.0.0.1:{}", port);
        let addr_for_thread = addr.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        let transcriber: Arc<dyn Transcriber> = Arc::new(MockTranscriber);

        let handle = thread::spawn(move || {
            run_wyoming_server(&addr_for_thread, transcriber, config, shutdown_clone);
        });

        thread::sleep(Duration::from_millis(120));
        (addr, shutdown, handle)
    }

    fn stop_server(shutdown: Arc<AtomicBool>, handle: thread::JoinHandle<()>) {
        shutdown.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }

    fn send_http(addr: &str, raw_request: &[u8]) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(raw_request).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn test_wav_bytes() -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: REQUIRED_SAMPLE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for _ in 0..320 {
                writer.write_sample::<i16>(0).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn http_json_request_returns_transcript() {
        let config = LocalApiRuntimeConfig {
            enabled: true,
            scope: ApiNetworkScope::Loopback,
            token: None,
        };
        let (addr, shutdown, handle) = spawn_http_server(config);

        let pcm = vec![0_u8; 640];
        let body = json!({
            "audio_base64": STANDARD.encode(pcm),
            "encoding": "pcm_s16le",
            "sample_rate": 16000
        })
        .to_string();

        let request = format!(
            "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            addr,
            body.len(),
            body
        );

        let response = send_http(&addr, request.as_bytes());
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("mock transcript"));

        stop_server(shutdown, handle);
    }

    #[test]
    fn http_multipart_request_returns_transcript() {
        let config = LocalApiRuntimeConfig {
            enabled: true,
            scope: ApiNetworkScope::Loopback,
            token: None,
        };
        let (addr, shutdown, handle) = spawn_http_server(config);

        let boundary = "----handyBoundary";
        let wav = test_wav_bytes();
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"sample.wav\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
        body.extend_from_slice(&wav);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"response_format\"\r\n\r\n");
        body.extend_from_slice(b"json\r\n");
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let request_head = format!(
            "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: {}\r\nContent-Type: multipart/form-data; boundary={}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            addr,
            boundary,
            body.len()
        );

        let mut raw = request_head.into_bytes();
        raw.extend_from_slice(&body);

        let response = send_http(&addr, &raw);
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("mock transcript"));

        stop_server(shutdown, handle);
    }

    #[test]
    fn http_auth_token_is_enforced() {
        let config = LocalApiRuntimeConfig {
            enabled: true,
            scope: ApiNetworkScope::Loopback,
            token: Some("secret-token".to_string()),
        };
        let (addr, shutdown, handle) = spawn_http_server(config);

        let pcm = vec![0_u8; 640];
        let body = json!({
            "audio_base64": STANDARD.encode(pcm),
            "encoding": "pcm_s16le",
            "sample_rate": 16000
        })
        .to_string();

        let unauthorized = format!(
            "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            addr,
            body.len(),
            body
        );

        let unauthorized_response = send_http(&addr, unauthorized.as_bytes());
        assert!(unauthorized_response.starts_with("HTTP/1.1 401"));

        let authorized = format!(
            "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer secret-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            addr,
            body.len(),
            body
        );

        let authorized_response = send_http(&addr, authorized.as_bytes());
        assert!(authorized_response.starts_with("HTTP/1.1 200"));

        stop_server(shutdown, handle);
    }

    #[test]
    fn wyoming_transcript_flow_works() {
        let config = LocalApiRuntimeConfig {
            enabled: true,
            scope: ApiNetworkScope::Loopback,
            token: None,
        };
        let (addr, shutdown, handle) = spawn_wyoming_server(config);

        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());

        stream.write_all(b"{\"type\":\"describe\"}\n").unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("\"type\":\"info\""));

        stream
            .write_all(b"{\"type\":\"audio-start\",\"data\":{\"rate\":16000}}\n")
            .unwrap();

        let payload = vec![0_u8; 640];
        stream
            .write_all(
                format!(
                    "{{\"type\":\"audio-chunk\",\"payload_length\":{}}}\n",
                    payload.len()
                )
                .as_bytes(),
            )
            .unwrap();
        stream.write_all(&payload).unwrap();
        stream.write_all(b"{\"type\":\"audio-stop\"}\n").unwrap();

        let mut saw_transcript = false;
        for _ in 0..3 {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.contains("\"type\":\"transcript\"") && line.contains("mock transcript") {
                saw_transcript = true;
                break;
            }
            if line.is_empty() {
                break;
            }
        }
        assert!(saw_transcript);

        stop_server(shutdown, handle);
    }
}
