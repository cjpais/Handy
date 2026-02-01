use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::Manager;

#[derive(Debug, Serialize)]
struct SidecarRequest {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    model_loaded: Option<bool>,
    #[serde(default)]
    status: Option<String>,
}

pub struct QwenAsrManager {
    process: Mutex<Option<SidecarProcess>>,
    sidecar_script_path: PathBuf,
    model_loaded: Mutex<bool>,
}

struct SidecarProcess {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

/// Path to the self-contained venv for Qwen ASR.
fn venv_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("com.handy.app").join("qwen-asr-venv")
}

/// Python binary inside the venv.
fn venv_python() -> PathBuf {
    venv_dir().join("bin").join("python3")
}

/// Expanded PATH that includes common macOS binary locations (for finding uv/python).
fn expanded_path() -> String {
    let extra = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    if let Ok(existing) = std::env::var("PATH") {
        format!("{}:{}", extra, existing)
    } else {
        extra.to_string()
    }
}

/// Resolve the full path to `uv`, checking common locations.
fn resolve_uv() -> Option<String> {
    let candidates = [
        "/opt/homebrew/bin/uv",
        "/usr/local/bin/uv",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    // Try via shell login to pick up cargo/brew paths
    if let Ok(output) = Command::new("/bin/zsh")
        .args(["-l", "-c", "which uv"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }
    None
}

impl QwenAsrManager {
    pub fn new(app: &tauri::App) -> Result<Self> {
        let sidecar_script_path = app
            .path()
            .resolve(
                "resources/sidecar/qwen_asr_sidecar.py",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| anyhow::anyhow!("Failed to resolve sidecar path: {}", e))?;

        Ok(Self {
            process: Mutex::new(None),
            sidecar_script_path,
            model_loaded: Mutex::new(false),
        })
    }

    /// Check if the self-contained venv with mlx-audio is ready.
    pub fn check_prerequisites() -> Result<PrerequisiteStatus> {
        let python = venv_python();

        // Check if venv python exists
        if !python.exists() {
            // Check if uv is available for installation
            if resolve_uv().is_none() {
                return Ok(PrerequisiteStatus {
                    available: false,
                    message: "uv is not installed. Install it first: brew install uv".to_string(),
                });
            }
            return Ok(PrerequisiteStatus {
                available: false,
                message: "Qwen ASR environment not set up yet.".to_string(),
            });
        }

        // Check mlx-audio is importable in the venv
        let mlx_audio_ok = Command::new(&python)
            .args(["-c", "import mlx_audio; print('ok')"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !mlx_audio_ok {
            return Ok(PrerequisiteStatus {
                available: false,
                message: "mlx-audio is not installed in the Qwen ASR environment.".to_string(),
            });
        }

        Ok(PrerequisiteStatus {
            available: true,
            message: "Ready".to_string(),
        })
    }

    /// Create a self-contained venv and install mlx-audio into it using uv.
    pub fn install_mlx_audio() -> Result<String> {
        println!("QwenASR: install_mlx_audio called");
        let uv = resolve_uv()
            .ok_or_else(|| anyhow::anyhow!("uv is not installed. Install it first: brew install uv"))?;
        println!("QwenASR: resolved uv at: {}", uv);

        let venv = venv_dir();
        let path_env = expanded_path();

        // Create venv if it doesn't exist
        if !venv.join("bin").join("python3").exists() {
            println!("QwenASR: creating venv at {:?}", venv);
            let output = Command::new(&uv)
                .args(["venv", "--python", "3.11"])
                .arg(&venv)
                .env("PATH", &path_env)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("QwenASR: venv creation failed: {}", stderr);
                return Err(anyhow::anyhow!("Failed to create venv: {}", stderr));
            }
            println!("QwenASR: venv created successfully");
        }

        // Install mlx-audio into the venv
        let output = Command::new(&uv)
            .args(["pip", "install", "-U", "mlx-audio @ git+https://github.com/Blaizzy/mlx-audio.git"])
            .arg("--python")
            .arg(venv.join("bin").join("python3"))
            .env("PATH", &path_env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            println!("QwenASR: mlx-audio installed successfully");
            Ok(format!("{}\n{}", stdout, stderr))
        } else {
            println!("QwenASR: uv pip install failed: {}\n{}", stdout, stderr);
            Err(anyhow::anyhow!(
                "uv pip install failed: {}\n{}",
                stdout,
                stderr
            ))
        }
    }

    fn start_sidecar(&self) -> Result<()> {
        let mut process_guard = self.process.lock().unwrap();

        // Kill existing process if any
        if let Some(mut proc) = process_guard.take() {
            let _ = proc.child.kill();
        }

        let script_path = self.sidecar_script_path.to_str().ok_or_else(|| {
            anyhow::anyhow!("Invalid sidecar script path")
        })?;

        let python = venv_python();
        if !python.exists() {
            return Err(anyhow::anyhow!(
                "Qwen ASR venv not found. Run setup first."
            ));
        }

        let mut child = Command::new(&python)
            .arg(script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to capture sidecar stdin")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            anyhow::anyhow!("Failed to capture sidecar stdout")
        })?;

        let mut sidecar = SidecarProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Wait for ready signal
        let response = read_response(&mut sidecar.stdout)?;
        if !response.ok {
            return Err(anyhow::anyhow!(
                "Sidecar failed to start: {}",
                response.error.unwrap_or_default()
            ));
        }

        *process_guard = Some(sidecar);
        Ok(())
    }

    fn send_command(&self, request: &SidecarRequest) -> Result<SidecarResponse> {
        let mut process_guard = self.process.lock().unwrap();
        let sidecar = process_guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Sidecar process not running"))?;

        let json = serde_json::to_string(request)?;
        writeln!(sidecar.stdin, "{}", json)?;
        sidecar.stdin.flush()?;

        read_response(&mut sidecar.stdout)
    }

    /// Start the sidecar and load the model.
    pub fn load_model(&self) -> Result<()> {
        println!("QwenAsrManager: Starting sidecar and loading model...");

        // Start sidecar if not running
        {
            let process_guard = self.process.lock().unwrap();
            if process_guard.is_none() {
                drop(process_guard);
                self.start_sidecar()?;
            }
        }

        let response = self.send_command(&SidecarRequest {
            command: "load_model".to_string(),
            audio_path: None,
            language: None,
        })?;

        if response.ok {
            let mut loaded = self.model_loaded.lock().unwrap();
            *loaded = true;
            println!("QwenAsrManager: Model loaded successfully");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failed to load Qwen3-ASR model: {}",
                response.error.unwrap_or_default()
            ))
        }
    }

    /// Transcribe audio from a WAV file path.
    pub fn transcribe_file(&self, audio_path: &str, language: Option<&str>) -> Result<String> {
        let response = self.send_command(&SidecarRequest {
            command: "transcribe".to_string(),
            audio_path: Some(audio_path.to_string()),
            language: language.map(|s| s.to_string()),
        })?;

        if response.ok {
            Ok(response.text.unwrap_or_default())
        } else {
            Err(anyhow::anyhow!(
                "Transcription failed: {}",
                response.error.unwrap_or_default()
            ))
        }
    }

    /// Transcribe audio from f32 samples (16kHz mono).
    /// Writes a temporary WAV file, transcribes, then cleans up.
    pub fn transcribe(&self, audio: &[f32], language: Option<&str>) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        // Write audio to a temporary WAV file
        let tmp_path = std::env::temp_dir().join("handy_qwen_asr_tmp.wav");
        let tmp_path_str = tmp_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid temp path"))?;

        write_wav(tmp_path_str, audio, 16000)?;

        let result = self.transcribe_file(tmp_path_str, language);

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_path);

        result
    }

    pub fn is_model_loaded(&self) -> bool {
        *self.model_loaded.lock().unwrap()
    }

    /// Shutdown the sidecar process.
    pub fn shutdown(&self) {
        let mut process_guard = self.process.lock().unwrap();
        if let Some(mut proc) = process_guard.take() {
            let _ = writeln!(proc.stdin, r#"{{"command":"shutdown"}}"#);
            let _ = proc.stdin.flush();
            // Give it a moment to exit gracefully
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = proc.child.kill();
        }
        let mut loaded = self.model_loaded.lock().unwrap();
        *loaded = false;
    }
}

impl Drop for QwenAsrManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn read_response(reader: &mut BufReader<std::process::ChildStdout>) -> Result<SidecarResponse> {
    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.is_empty() {
        return Err(anyhow::anyhow!("Sidecar process closed unexpectedly"));
    }

    serde_json::from_str(&line)
        .map_err(|e| anyhow::anyhow!("Failed to parse sidecar response: {} (raw: {})", e, line.trim()))
}

fn write_wav(path: &str, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        let s = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrerequisiteStatus {
    pub available: bool,
    pub message: String,
}
