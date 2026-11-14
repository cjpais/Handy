//! Air-gapped speech-to-text.
//!
//! When `stt_http.enabled` is set in settings, a finished recording is sent to
//! an OpenAI-compatible `/audio/transcriptions` endpoint instead of the local
//! transcribe-cpp engine. Works with vLLM, whisper.cpp's server, LM Studio and
//! any other server that speaks the same request format.
//!
//! Disabled by default: the local pipeline is untouched unless the user opts in.

use std::time::Duration;

use crate::managers::transcription::Transcription;
use crate::settings::SttHttpSettings;

/// 16 kHz mono f32 samples -> 16-bit PCM WAV, in memory.
fn wav_bytes(pcm: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Longest recording this backend will send. Guards against a stuck
/// push-to-talk key turning into a multi-hundred-megabyte upload.
const MAX_AUDIO_SECONDS: f64 = 600.0;
/// Largest response body accepted from the endpoint.
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Only http/https with a host is a legitimate endpoint; anything else is a
/// typo or a policy violation, and it must fail before any audio is read.
fn validate_url(base_url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(base_url.trim())
        .map_err(|e| format!("Endpoint URL is not valid: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("Endpoint URL must be http or https, got {other}")),
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err("Endpoint URL has no host".to_string());
    }
    Ok(parsed.to_string())
}

/// Accepts `http://host:port/v1`, the full endpoint, or a bare host; adds
/// `/audio/transcriptions` only when it is not already there.
fn transcriptions_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/audio/transcriptions") {
        base.to_string()
    } else {
        format!("{base}/audio/transcriptions")
    }
}

/// `{"text": "..."}` is the documented shape; plain text is accepted too,
/// because some local servers answer with the transcript alone.
fn parse_transcript(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("text")
                .and_then(|text| text.as_str())
                .map(|text| text.to_string())
        })
        .unwrap_or_else(|| body.trim().to_string())
}

/// Sends `pcm` to the configured backend and returns its text.
pub async fn transcribe(
    pcm: &[f32],
    sample_rate: u32,
    config: &SttHttpSettings,
) -> Result<Transcription, String> {
    if config.base_url.trim().is_empty() {
        return Err("STT endpoint URL is empty".to_string());
    }
    if pcm.is_empty() {
        return Err("No audio to transcribe".to_string());
    }
    let seconds = pcm.len() as f64 / sample_rate as f64;
    if seconds > MAX_AUDIO_SECONDS {
        return Err(format!(
            "Recording is {seconds:.0} s long; the limit for the external backend is {MAX_AUDIO_SECONDS:.0} s"
        ));
    }

    let url = transcriptions_url(&validate_url(&config.base_url)?);
    let file = reqwest::multipart::Part::bytes(wav_bytes(pcm, sample_rate))
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Could not build the audio part: {e}"))?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", file)
        .text("response_format", "json");
    if !config.model.trim().is_empty() {
        form = form.text("model", config.model.trim().to_string());
    }
    if let Some(language) = config
        .language
        .as_ref()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
    {
        form = form.text("language", language.to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs.max(5)))
        .build()
        .map_err(|e| format!("Could not create the HTTP client: {e}"))?;

    let mut request = client.post(&url).multipart(form);
    if !config.api_key.trim().is_empty() {
        request = request.bearer_auth(config.api_key.trim());
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("STT request to {url} failed: {e}"))?;
    let status = response.status();
    if let Some(length) = response.content_length() {
        if length as usize > MAX_RESPONSE_BYTES {
            return Err(format!(
                "STT backend response is {length} bytes; refusing to read more than {MAX_RESPONSE_BYTES}"
            ));
        }
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("Could not read the STT response: {e}"))?;

    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        return Err(format!("STT backend returned {status}: {snippet}"));
    }

    Ok(Transcription {
        text: parse_transcript(&body),
        language: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_well_formed() {
        let wav = wav_bytes(&[0.0, 0.5, -0.5], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 3 * 2);
        let declared = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(declared, 6);
    }

    #[test]
    fn parses_the_documented_json_shape() {
        assert_eq!(parse_transcript(r#"{"text":"hello"}"#), "hello");
        assert_eq!(parse_transcript(r#"{"text":""}"#), "");
    }

    #[test]
    fn accepts_plain_text_and_tolerates_junk() {
        assert_eq!(parse_transcript("  plain text\n"), "plain text");
        assert_eq!(parse_transcript(r#"{"error":"boom"}"#), r#"{"error":"boom"}"#);
    }

    #[test]
    fn rejects_non_http_endpoints() {
        assert!(validate_url("ftp://host/x").is_err());
        assert!(validate_url("not-a-url").is_err());
        assert!(validate_url("http://127.0.0.1:8000/v1").is_ok());
    }

    #[test]
    fn url_keeps_an_explicit_endpoint() {
        assert_eq!(
            transcriptions_url("http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/v1/audio/transcriptions"
        );
        assert_eq!(
            transcriptions_url("http://127.0.0.1:8080/v1/audio/transcriptions/"),
            "http://127.0.0.1:8080/v1/audio/transcriptions"
        );
    }

    /// End-to-end through the real HTTP client: a local socket plays the
    /// backend, so this catches multipart, headers and response parsing.
    #[test]
    fn posts_wav_and_reads_the_transcript() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        fn headers_end(buf: &[u8]) -> Option<usize> {
            buf.windows(4).position(|w| w == b"\r\n\r\n")
        }

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut chunk = [0u8; 8192];
            let mut end = None;
            loop {
                let read = stream.read(&mut chunk).expect("read headers");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(pos) = headers_end(&request) {
                    end = Some(pos);
                    break;
                }
            }
            let pos = end.expect("headers");
            let head = String::from_utf8_lossy(&request[..pos]).to_string();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            while request.len() < pos + 4 + length {
                let read = stream.read(&mut chunk).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let saw_wav = request.windows(4).any(|w| w == b"RIFF");
            let saw_model = String::from_utf8_lossy(&request).contains("whisper-1");
            let body = r#"{"text":"ok"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).expect("write");
            (saw_wav, saw_model)
        });

        let config = SttHttpSettings {
            enabled: true,
            base_url: format!("http://{addr}/v1"),
            model: "whisper-1".to_string(),
            ..SttHttpSettings::default()
        };
        let pcm = vec![0.1f32; 16_000];
        let result = tauri::async_runtime::block_on(transcribe(&pcm, 16_000, &config));
        let (saw_wav, saw_model) = server.join().expect("server");
        assert!(saw_wav, "the request must carry a WAV body");
        assert!(saw_model, "the request must name the model");
        assert_eq!(result.expect("transcribe").text, "ok");
    }
}
