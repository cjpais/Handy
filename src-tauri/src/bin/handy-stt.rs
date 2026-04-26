use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use rubato::{FftFixedIn, Resampler};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tar::Archive;
use tokio::sync::mpsc;
use transcribe_rs::{
    engines::{
        parakeet::{
            ParakeetEngine, ParakeetInferenceParams, ParakeetModelParams, TimestampGranularity,
        },
        whisper::{WhisperEngine, WhisperInferenceParams},
    },
    TranscriptionEngine,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EngineType {
    Whisper,
    Parakeet,
}

#[derive(Debug, Clone)]
struct ModelSpec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    filename: &'static str,
    url: Option<&'static str>,
    size_mb: u64,
    is_directory: bool,
    engine_type: EngineType,
}

fn builtin_models() -> Vec<ModelSpec> {
    vec![
        ModelSpec {
            id: "small",
            name: "Whisper Small",
            description: "Fast and fairly accurate.",
            filename: "ggml-small.bin",
            url: Some("https://blob.handy.computer/ggml-small.bin"),
            size_mb: 487,
            is_directory: false,
            engine_type: EngineType::Whisper,
        },
        ModelSpec {
            id: "medium",
            name: "Whisper Medium",
            description: "Good accuracy, medium speed",
            filename: "whisper-medium-q4_1.bin",
            url: Some("https://blob.handy.computer/whisper-medium-q4_1.bin"),
            size_mb: 492,
            is_directory: false,
            engine_type: EngineType::Whisper,
        },
        ModelSpec {
            id: "turbo",
            name: "Whisper Turbo",
            description: "Balanced accuracy and speed.",
            filename: "ggml-large-v3-turbo.bin",
            url: Some("https://blob.handy.computer/ggml-large-v3-turbo.bin"),
            size_mb: 1600,
            is_directory: false,
            engine_type: EngineType::Whisper,
        },
        ModelSpec {
            id: "large",
            name: "Whisper Large",
            description: "Good accuracy, but slow.",
            filename: "ggml-large-v3-q5_0.bin",
            url: Some("https://blob.handy.computer/ggml-large-v3-q5_0.bin"),
            size_mb: 1100,
            is_directory: false,
            engine_type: EngineType::Whisper,
        },
        ModelSpec {
            id: "parakeet-tdt-0.6b-v2",
            name: "Parakeet V2",
            description: "English only. The best model for English speakers.",
            filename: "parakeet-tdt-0.6b-v2-int8",
            url: Some("https://blob.handy.computer/parakeet-v2-int8.tar.gz"),
            size_mb: 473,
            is_directory: true,
            engine_type: EngineType::Parakeet,
        },
        ModelSpec {
            id: "parakeet-tdt-0.6b-v3",
            name: "Parakeet V3",
            description: "Fast and accurate",
            filename: "parakeet-tdt-0.6b-v3-int8",
            url: Some("https://blob.handy.computer/parakeet-v3-int8.tar.gz"),
            size_mb: 478,
            is_directory: true,
            engine_type: EngineType::Parakeet,
        },
    ]
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    id: u64,
    cmd: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Response<T: Serialize> {
    r#type: &'static str,
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Event<T: Serialize> {
    r#type: &'static str,
    event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<T>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelInfo {
    id: String,
    name: String,
    description: String,
    engine_type: EngineType,
    size_mb: u64,
    is_downloaded: bool,
    is_downloading: bool,
    is_directory: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    model_id: String,
    downloaded: u64,
    total: Option<u64>,
    percentage: Option<f64>,
}

enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
}

struct AppState {
    models_dir: PathBuf,
    models: HashMap<String, ModelSpec>,
    engine: Mutex<Option<(String, LoadedEngine)>>,
    is_downloading: Mutex<Option<String>>,

    current_session: Mutex<Option<u64>>,
    session_audio: Mutex<Vec<f32>>,
    session_sample_rate: Mutex<Option<u32>>,
    next_session_id: AtomicU64,

    language: Mutex<Option<String>>, // None => auto
    translate: Mutex<bool>,
}

impl AppState {
    fn new(models_dir: PathBuf) -> Self {
        let models = builtin_models()
            .into_iter()
            .map(|m| (m.id.to_string(), m))
            .collect();

        Self {
            models_dir,
            models,
            engine: Mutex::new(None),
            is_downloading: Mutex::new(None),
            current_session: Mutex::new(None),
            session_audio: Mutex::new(Vec::new()),
            session_sample_rate: Mutex::new(None),
            next_session_id: AtomicU64::new(1),
            language: Mutex::new(None),
            translate: Mutex::new(false),
        }
    }

    fn model_target_path(&self, spec: &ModelSpec) -> PathBuf {
        self.models_dir.join(spec.filename)
    }

    fn model_partial_path(&self, spec: &ModelSpec) -> PathBuf {
        self.models_dir.join(format!("{}.partial", spec.filename))
    }

    fn model_extracting_path(&self, spec: &ModelSpec) -> PathBuf {
        self.models_dir
            .join(format!("{}.extracting", spec.filename))
    }

    fn is_model_downloaded(&self, spec: &ModelSpec) -> bool {
        let p = self.model_target_path(spec);
        if spec.is_directory {
            p.exists() && p.is_dir()
        } else {
            p.exists() && p.is_file()
        }
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let downloading = self.is_downloading.lock().unwrap().clone();
        self.models
            .values()
            .map(|m| ModelInfo {
                id: m.id.to_string(),
                name: m.name.to_string(),
                description: m.description.to_string(),
                engine_type: m.engine_type.clone(),
                size_mb: m.size_mb,
                is_downloaded: self.is_model_downloaded(m),
                is_downloading: downloading.as_deref() == Some(m.id),
                is_directory: m.is_directory,
            })
            .collect()
    }

    fn unload_model(&self) -> Result<()> {
        let mut guard = self.engine.lock().unwrap();
        if let Some((_id, mut loaded_engine)) = guard.take() {
            match &mut loaded_engine {
                LoadedEngine::Whisper(engine) => engine.unload_model(),
                LoadedEngine::Parakeet(engine) => engine.unload_model(),
            };
        }
        Ok(())
    }

    fn load_model(&self, model_id: &str) -> Result<()> {
        let spec = self
            .models
            .get(model_id)
            .ok_or_else(|| anyhow!("unknown model_id: {model_id}"))?
            .clone();

        if !self.is_model_downloaded(&spec) {
            return Err(anyhow!("model not downloaded: {model_id}"));
        }

        let model_path = self.model_target_path(&spec);

        let loaded = match spec.engine_type {
            EngineType::Whisper => {
                let mut engine = WhisperEngine::new();
                engine
                    .load_model(&model_path)
                    .map_err(|e| anyhow!("failed to load whisper model {model_id}: {e}"))?;
                LoadedEngine::Whisper(engine)
            }
            EngineType::Parakeet => {
                let mut engine = ParakeetEngine::new();
                engine
                    .load_model_with_params(&model_path, ParakeetModelParams::int8())
                    .map_err(|e| anyhow!("failed to load parakeet model {model_id}: {e}"))?;
                LoadedEngine::Parakeet(engine)
            }
        };

        *self.engine.lock().unwrap() = Some((model_id.to_string(), loaded));
        Ok(())
    }

    fn transcribe_samples(&self, samples: Vec<f32>, sample_rate: u32) -> Result<String> {
        // Handy 的后端默认管线是 16kHz mono。这里提供内置重采样以提升对接容错。
        let samples_16k = if sample_rate == 16_000 {
            samples
        } else {
            resample_mono(samples, sample_rate as usize, 16_000)?
        };

        let language = self.language.lock().unwrap().clone();
        let translate = *self.translate.lock().unwrap();

        let mut guard = self.engine.lock().unwrap();
        let (_model_id, engine) = guard
            .as_mut()
            .ok_or_else(|| anyhow!("model is not loaded"))?;

        let result = match engine {
            LoadedEngine::Whisper(whisper) => {
                let whisper_language = language.and_then(|lang| {
                    if lang == "auto" {
                        None
                    } else if lang == "zh-Hans" || lang == "zh-Hant" {
                        Some("zh".to_string())
                    } else {
                        Some(lang)
                    }
                });

                let params = WhisperInferenceParams {
                    language: whisper_language,
                    translate,
                    ..Default::default()
                };

                whisper
                    .transcribe_samples(samples_16k, Some(params))
                    .map_err(|e| anyhow!("whisper transcription failed: {e}"))?
            }
            LoadedEngine::Parakeet(parakeet) => {
                let params = ParakeetInferenceParams {
                    timestamp_granularity: TimestampGranularity::Segment,
                    ..Default::default()
                };

                parakeet
                    .transcribe_samples(samples_16k, Some(params))
                    .map_err(|e| anyhow!("parakeet transcription failed: {e}"))?
            }
        };

        Ok(result.text.trim().to_string())
    }

    fn start_session(&self, sample_rate: u32) -> u64 {
        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        *self.current_session.lock().unwrap() = Some(session_id);
        *self.session_sample_rate.lock().unwrap() = Some(sample_rate);
        self.session_audio.lock().unwrap().clear();
        session_id
    }

    fn push_audio_s16le_base64(&self, session_id: u64, b64: &str) -> Result<()> {
        let cur = *self
            .current_session
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| anyhow!("no active session"))?;
        if cur != session_id {
            return Err(anyhow!("session mismatch"));
        }

        let bytes = B64
            .decode(b64)
            .map_err(|e| anyhow!("invalid base64 audio: {e}"))?;
        if bytes.len() % 2 != 0 {
            return Err(anyhow!("invalid pcm_s16le payload length"));
        }

        let mut out = self.session_audio.lock().unwrap();
        out.reserve(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let v = i16::from_le_bytes([chunk[0], chunk[1]]);
            out.push(v as f32 / 32768.0);
        }
        Ok(())
    }

    fn finish_session_transcribe(&self, session_id: u64) -> Result<String> {
        let cur = self
            .current_session
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow!("no active session"))?;
        if cur != session_id {
            return Err(anyhow!("session mismatch"));
        }

        let sample_rate = self
            .session_sample_rate
            .lock()
            .unwrap()
            .take()
            .unwrap_or(16_000);
        let samples = std::mem::take(&mut *self.session_audio.lock().unwrap());

        self.transcribe_samples(samples, sample_rate)
    }
}

fn resample_mono(mut input: Vec<f32>, in_hz: usize, out_hz: usize) -> Result<Vec<f32>> {
    if in_hz == out_hz {
        return Ok(input);
    }

    // 采用固定 chunk（与仓库现有 resampler 思路一致），避免 gcd 推导复杂。
    const CHUNK_IN: usize = 1024;

    let mut resampler =
        FftFixedIn::<f32>::new(in_hz, out_hz, CHUNK_IN, 1, 1).context("create resampler")?;

    let mut out_all: Vec<f32> = Vec::new();

    if input.is_empty() {
        return Ok(out_all);
    }

    // 末尾 padding 到 CHUNK_IN 的整数倍
    let rem = input.len() % CHUNK_IN;
    if rem != 0 {
        input.resize(input.len() + (CHUNK_IN - rem), 0.0);
    }

    for chunk in input.chunks(CHUNK_IN) {
        let out = resampler
            .process(&[chunk], None)
            .map_err(|e| anyhow!("resample failed: {e}"))?;
        out_all.extend_from_slice(&out[0]);
    }

    Ok(out_all)
}

fn json_write_line(stdout: &mut dyn Write, value: &impl Serialize) -> Result<()> {
    let line = serde_json::to_string(value)?;
    stdout.write_all(line.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

fn parse_models_dir() -> PathBuf {
    let mut args = std::env::args().skip(1);
    let mut models_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "--models-dir" {
            if let Some(p) = args.next() {
                models_dir = Some(PathBuf::from(p));
            }
        }
    }

    models_dir
        .or_else(|| std::env::var("HANDY_MODELS_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("models"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let models_dir = parse_models_dir();
    std::fs::create_dir_all(&models_dir).context("create models dir")?;

    let state = Arc::new(AppState::new(models_dir));

    // 统一 stdout 写入：所有 response/event 都走 out_tx，避免多线程写 stdout 导致 JSON 行被打断。
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<serde_json::Value>();

    let stdin = std::io::stdin();

    let writer_task = tokio::spawn(async move {
        let mut stdout = std::io::stdout();
        while let Some(v) = out_rx.recv().await {
            let _ = json_write_line(&mut stdout, &v);
        }
    });

    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let v = Response::<serde_json::Value> {
                    r#type: "response",
                    id: 0,
                    ok: false,
                    result: None,
                    error: Some(format!("invalid request json: {e}")),
                };
                let _ = out_tx.send(serde_json::to_value(v).unwrap());
                continue;
            }
        };

        let resp_value = handle_request(state.clone(), out_tx.clone(), req).await;
        let _ = out_tx.send(resp_value);
    }

    drop(out_tx);
    let _ = writer_task.await;
    Ok(())
}

async fn handle_request(
    state: Arc<AppState>,
    out_tx: mpsc::UnboundedSender<serde_json::Value>,
    req: Request,
) -> serde_json::Value {
    let id = req.id;

    let result: Result<serde_json::Value> = (|| async {
        match req.cmd.as_str() {
            "ping" => Ok(serde_json::json!({"version": 1})),

            "set_language" => {
                let lang = req
                    .args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                *state.language.lock().unwrap() = lang;
                Ok(serde_json::json!({}))
            }

            "set_translate" => {
                let translate = req
                    .args
                    .get("translate")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                *state.translate.lock().unwrap() = translate;
                Ok(serde_json::json!({}))
            }

            "list_models" => {
                let models = state.list_models();
                Ok(serde_json::to_value(models)?)
            }

            "download_model" => {
                let model_id = req
                    .args
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing args.model_id"))?
                    .to_string();

                let spec = state
                    .models
                    .get(&model_id)
                    .ok_or_else(|| anyhow!("unknown model_id: {model_id}"))?
                    .clone();

                let url = spec
                    .url
                    .ok_or_else(|| anyhow!("model has no download url: {model_id}"))?
                    .to_string();

                {
                    let mut downloading = state.is_downloading.lock().unwrap();
                    if downloading.is_some() {
                        return Err(anyhow!("another model is downloading"));
                    }
                    *downloading = Some(model_id.clone());
                }

                let state_cloned = state.clone();
                let out_tx_cloned = out_tx.clone();
                tokio::spawn(async move {
                    let res =
                        download_and_prepare_model(&state_cloned, &spec, &url, out_tx_cloned).await;

                    let mut downloading = state_cloned.is_downloading.lock().unwrap();
                    *downloading = None;

                    if let Err(e) = res {
                        let _ = out_tx.send(serde_json::json!(Event::<serde_json::Value> {
                            r#type: "event",
                            event: "download_failed",
                            payload: Some(serde_json::json!({
                                "model_id": spec.id,
                                "error": e.to_string(),
                            })),
                        }));
                    }
                });

                Ok(serde_json::json!({"started": true}))
            }

            "load_model" => {
                let model_id = req
                    .args
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing args.model_id"))?;
                state.load_model(model_id)?;
                Ok(serde_json::json!({"loaded": true, "model_id": model_id}))
            }

            "unload_model" => {
                state.unload_model()?;
                Ok(serde_json::json!({"unloaded": true}))
            }

            "start_session" => {
                let sample_rate = req
                    .args
                    .get("sample_rate")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(16_000) as u32;
                let session_id = state.start_session(sample_rate);
                Ok(serde_json::json!({"session_id": session_id}))
            }

            "push_audio" => {
                let session_id = req
                    .args
                    .get("session_id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("missing args.session_id"))?;
                let encoding = req
                    .args
                    .get("encoding")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pcm_s16le");
                if encoding != "pcm_s16le" {
                    return Err(anyhow!("unsupported encoding: {encoding}"));
                }
                let audio_b64 = req
                    .args
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing args.data"))?;

                state.push_audio_s16le_base64(session_id, audio_b64)?;
                Ok(serde_json::json!({"pushed": true}))
            }

            "finish_transcribe" => {
                let session_id = req
                    .args
                    .get("session_id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| anyhow!("missing args.session_id"))?;
                let text = state.finish_session_transcribe(session_id)?;
                Ok(serde_json::json!({"text": text}))
            }

            "transcribe_wav" => {
                let wav_path = req
                    .args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing args.path"))?;

                let (samples, sample_rate) = read_wav_mono_f32(Path::new(wav_path))?;
                let text = state.transcribe_samples(samples, sample_rate)?;
                Ok(serde_json::json!({"text": text}))
            }

            _ => Err(anyhow!("unknown cmd: {}", req.cmd)),
        }
    })()
    .await;

    match result {
        Ok(v) => serde_json::to_value(Response {
            r#type: "response",
            id,
            ok: true,
            result: Some(v),
            error: None,
        })
        .unwrap(),
        Err(e) => serde_json::to_value(Response::<serde_json::Value> {
            r#type: "response",
            id,
            ok: false,
            result: None,
            error: Some(e.to_string()),
        })
        .unwrap(),
    }
}

async fn download_and_prepare_model(
    state: &AppState,
    spec: &ModelSpec,
    url: &str,
    out_tx: mpsc::UnboundedSender<serde_json::Value>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await.context("download request")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("download failed http status: {status}"));
    }

    let total = resp.content_length();
    let partial_path = state.model_partial_path(spec);

    // 用 std::fs 以简化依赖（tokio::fs 在 windows 上也 ok，但这里写法更直接）
    let mut file = File::create(&partial_path)
        .with_context(|| format!("create partial file: {}", partial_path.to_string_lossy()))?;

    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream read")?;
        file.write_all(&chunk).context("write partial file")?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed() >= Duration::from_millis(200) {
            emit_progress(&out_tx, spec.id, downloaded, total);
            last_emit = Instant::now();
        }
    }

    emit_progress(&out_tx, spec.id, downloaded, total);

    // 关闭文件句柄，确保后续 rename / read 没有被占用
    drop(file);

    if spec.is_directory {
        // 解压/解包 tar.gz 到 extracting 目录，然后 rename 成最终目录。
        let extracting_path = state.model_extracting_path(spec);
        let final_dir = state.model_target_path(spec);

        if extracting_path.exists() {
            let _ = std::fs::remove_dir_all(&extracting_path);
        }
        std::fs::create_dir_all(&extracting_path).context("create extracting dir")?;

        let partial_path_cloned = partial_path.clone();
        let extracting_path_cloned = extracting_path.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let tar_gz = File::open(&partial_path_cloned).context("open downloaded tar.gz")?;
            let decoder = GzDecoder::new(tar_gz);
            let mut archive = Archive::new(decoder);
            archive
                .unpack(&extracting_path_cloned)
                .context("unpack tar.gz")?;
            Ok(())
        })
        .await
        .context("join extract task")??;

        // 清理旧目录后原子切换
        if final_dir.exists() {
            let _ = std::fs::remove_dir_all(&final_dir);
        }
        std::fs::rename(&extracting_path, &final_dir)
            .with_context(|| format!("rename extracting dir to final: {}", spec.id))?;
        let _ = std::fs::remove_file(&partial_path);
    } else {
        let final_path = state.model_target_path(spec);
        if final_path.exists() {
            let _ = std::fs::remove_file(&final_path);
        }
        std::fs::rename(&partial_path, &final_path)
            .with_context(|| format!("rename partial to final: {}", spec.id))?;
    }

    let _ = out_tx.send(serde_json::json!(Event::<serde_json::Value> {
        r#type: "event",
        event: "download_completed",
        payload: Some(serde_json::json!({
            "model_id": spec.id,
        })),
    }));

    Ok(())
}

fn emit_progress(
    out_tx: &mpsc::UnboundedSender<serde_json::Value>,
    model_id: &str,
    downloaded: u64,
    total: Option<u64>,
) {
    let (percentage, total_opt) = match total {
        Some(t) if t > 0 => (Some(downloaded as f64 / t as f64 * 100.0), Some(t)),
        _ => (None, None),
    };

    let payload = DownloadProgress {
        model_id: model_id.to_string(),
        downloaded,
        total: total_opt,
        percentage,
    };

    let _ = out_tx.send(serde_json::json!(Event {
        r#type: "event",
        event: "download_progress",
        payload: Some(payload),
    }));
}

fn read_wav_mono_f32(path: &Path) -> Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("open wav: {}", path.to_string_lossy()))?;

    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(anyhow!("wav must be mono (channels=1)"));
    }

    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("read float wav samples")?,
        hound::SampleFormat::Int => {
            // 处理常见 16-bit PCM。
            if spec.bits_per_sample != 16 {
                return Err(anyhow!(
                    "unsupported int wav bits_per_sample: {}",
                    spec.bits_per_sample
                ));
            }
            reader
                .samples::<i16>()
                .map(|s| s.map(|v| v as f32 / 32768.0))
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("read int16 wav samples")?
        }
    };

    Ok((samples, sample_rate))
}
