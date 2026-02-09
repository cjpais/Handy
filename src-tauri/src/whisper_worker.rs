use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use transcribe_rs::engines::whisper::{WhisperEngine, WhisperInferenceParams, WhisperModelParams};
use transcribe_rs::TranscriptionEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperRuntimeMode {
    Gpu,
    Cpu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperWorkerInferenceParams {
    pub language: Option<String>,
    pub translate: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WhisperWorkerRequest {
    LoadModel {
        model_path: String,
        use_gpu: bool,
    },
    Transcribe {
        audio: Vec<f32>,
        params: WhisperWorkerInferenceParams,
    },
    UnloadModel,
}

#[derive(Debug, Serialize, Deserialize)]
struct WhisperWorkerResponse {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

pub struct WhisperWorkerClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    runtime_mode: WhisperRuntimeMode,
}

impl WhisperWorkerClient {
    pub fn spawn_for_model(model_path: &PathBuf, runtime_mode: WhisperRuntimeMode) -> Result<Self> {
        let current_exe = std::env::current_exe()?;
        let mut child = Command::new(current_exe)
            .arg("--whisper-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("whisper worker stdin is unavailable"))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("whisper worker stdout is unavailable"))?;

        let mut client = Self {
            child,
            stdin: BufWriter::new(child_stdin),
            stdout: BufReader::new(child_stdout),
            runtime_mode,
        };

        client.send_request(WhisperWorkerRequest::LoadModel {
            model_path: model_path.to_string_lossy().to_string(),
            use_gpu: runtime_mode == WhisperRuntimeMode::Gpu,
        })?;

        Ok(client)
    }

    pub fn runtime_mode(&self) -> WhisperRuntimeMode {
        self.runtime_mode
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    pub fn transcribe(
        &mut self,
        audio: Vec<f32>,
        params: WhisperWorkerInferenceParams,
    ) -> Result<String> {
        let response = self.send_request(WhisperWorkerRequest::Transcribe { audio, params })?;
        response
            .text
            .ok_or_else(|| anyhow!("whisper worker returned empty result"))
    }

    pub fn unload(&mut self) -> Result<()> {
        let _ = self.send_request(WhisperWorkerRequest::UnloadModel)?;
        Ok(())
    }

    pub fn terminate(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn send_request(&mut self, request: WhisperWorkerRequest) -> Result<WhisperWorkerResponse> {
        let payload = serde_json::to_string(&request)?;
        self.stdin.write_all(payload.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;

        let mut response_line = String::new();
        let read = self.stdout.read_line(&mut response_line)?;
        if read == 0 {
            return Err(anyhow!("whisper worker process exited"));
        }

        let response: WhisperWorkerResponse = serde_json::from_str(response_line.trim())?;
        if response.ok {
            Ok(response)
        } else {
            Err(anyhow!(
                "{}",
                response
                    .error
                    .unwrap_or_else(|| "whisper worker request failed".to_string())
            ))
        }
    }
}

impl Drop for WhisperWorkerClient {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn run_worker_process() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    let mut engine: Option<WhisperEngine> = None;

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }

        let request: WhisperWorkerRequest = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                write_response(
                    &mut writer,
                    WhisperWorkerResponse {
                        ok: false,
                        text: None,
                        error: Some(format!("invalid request: {}", e)),
                    },
                )?;
                continue;
            }
        };

        let response = match request {
            WhisperWorkerRequest::LoadModel {
                model_path,
                use_gpu,
            } => {
                let mut whisper = WhisperEngine::new();
                match whisper.load_model_with_params(
                    PathBuf::from(model_path).as_path(),
                    WhisperModelParams { use_gpu },
                ) {
                    Ok(_) => {
                        engine = Some(whisper);
                        WhisperWorkerResponse {
                            ok: true,
                            text: None,
                            error: None,
                        }
                    }
                    Err(e) => WhisperWorkerResponse {
                        ok: false,
                        text: None,
                        error: Some(format!("failed to load whisper model: {}", e)),
                    },
                }
            }
            WhisperWorkerRequest::Transcribe { audio, params } => {
                match engine.as_mut() {
                    Some(engine_ref) => {
                        let inference_params = WhisperInferenceParams {
                            language: params.language,
                            translate: params.translate,
                            ..Default::default()
                        };

                        match engine_ref.transcribe_samples(audio, Some(inference_params)) {
                            Ok(result) => WhisperWorkerResponse {
                                ok: true,
                                text: Some(result.text),
                                error: None,
                            },
                            Err(e) => WhisperWorkerResponse {
                                ok: false,
                                text: None,
                                error: Some(format!("whisper transcription failed: {}", e)),
                            },
                        }
                    }
                    None => WhisperWorkerResponse {
                        ok: false,
                        text: None,
                        error: Some("model is not loaded".to_string()),
                    },
                }
            }
            WhisperWorkerRequest::UnloadModel => {
                if let Some(engine_ref) = engine.as_mut() {
                    engine_ref.unload_model();
                }
                engine = None;
                WhisperWorkerResponse {
                    ok: true,
                    text: None,
                    error: None,
                }
            }
        };

        write_response(&mut writer, response)?;
    }

    Ok(())
}

fn write_response(
    writer: &mut BufWriter<std::io::StdoutLock<'_>>,
    response: WhisperWorkerResponse,
) -> Result<()> {
    let payload = serde_json::to_string(&response)?;
    writer.write_all(payload.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
