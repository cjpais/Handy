use log::{debug, error, info};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Qwen3 ASR Engine using MLX framework (macOS only)
#[derive(Clone)]
pub struct Qwen3Engine {
    model_path: Option<String>,
}

/// Parameters for Qwen3 inference
#[derive(Debug, Clone, Serialize)]
pub struct Qwen3InferenceParams {
    pub language: Option<String>,
    pub task: String, // "transcribe" or "translate"
}

impl Default for Qwen3InferenceParams {
    fn default() -> Self {
        Self {
            language: None,
            task: "transcribe".to_string(),
        }
    }
}

/// Result from transcription
#[derive(Debug, Clone)]
pub struct Qwen3TranscriptionResult {
    pub text: String,
    pub language: Option<String>,
}

impl Qwen3Engine {
    pub fn new() -> Self {
        Self { model_path: None }
    }

    pub fn load_model(&mut self, model_path: &Path) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let path_str = model_path.to_string_lossy().to_string();
        info!("Loading Qwen3 model from: {}", path_str);

        // Check MLX is available
        self.check_mlx_available()?;

        // Verify model path exists
        if !model_path.exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Model path does not exist: {}", path_str),
            )));
        }

        self.model_path = Some(path_str);
        info!("Qwen3 model loaded successfully");
        Ok(())
    }

    pub fn load_model_with_params<P: serde::Serialize>(
        &mut self,
        model_path: &Path,
        _params: P,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        self.load_model(model_path)
    }

    pub fn unload_model(&mut self) {
        debug!("Unloading Qwen3 model");
        self.model_path = None;
    }

    pub fn transcribe_samples<P: serde::Serialize>(
        &mut self,
        audio: Vec<f32>,
        params: Option<P>,
    ) -> std::result::Result<Qwen3TranscriptionResult, Box<dyn std::error::Error>> {
        let model_path = self
            .model_path
            .as_ref()
            .ok_or_else(|| Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Model not loaded",
            )))?;

        debug!("Transcribing {} samples with Qwen3", audio.len());

        // Serialize params if provided
        let params_json = match params {
            Some(p) => serde_json::to_string(&p).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };

        // Prepare input data
        let input_data = serde_json::json!({
            "audio": audio,
            "params": params_json,
            "model_path": model_path
        });

        // Spawn Python process with the script
        let script = include_str!("../../resources/scripts/qwen3_asr.py");

        let mut child = Command::new("python3")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        // Write to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input_data.to_string().as_bytes())
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        }

        // Wait for output
        let output = child
            .wait_with_output()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Qwen3 Python error: {}", stderr);
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Qwen3 transcription failed: {}", stderr),
            )));
        }

        // Parse result
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        if let Some(error) = result.get("error") {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Qwen3 error: {}", error),
            )));
        }

        let text = result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let language = result
            .get("language")
            .and_then(|l| l.as_str())
            .map(|s| s.to_string());

        Ok(Qwen3TranscriptionResult { text, language })
    }

    fn check_mlx_available(&self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("python3")
            .args(["-c", "import mlx; print('MLX available')"])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    Ok(())
                } else {
                    Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "MLX not installed. Run: pip install mlx",
                    )))
                }
            }
            Err(e) => Err(Box::new(e)),
        }
    }
}

impl Default for Qwen3Engine {
    fn default() -> Self {
        Self::new()
    }
}
