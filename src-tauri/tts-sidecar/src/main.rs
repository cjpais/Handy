//! TTS Sidecar Process
//!
//! This is a separate process that handles TTS synthesis using piper-rs to avoid
//! ort version conflicts with vad-rs in the main application.
//!
//! Communication is via JSON over stdin/stdout:
//! - Requests are JSON objects on stdin (one per line)
//! - Responses are JSON objects on stdout (one per line)

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use cpal::traits::{DeviceTrait, HostTrait};
use piper_rs::synth::PiperSpeechSynthesizer;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "load")]
    Load { model_path: String },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "speak")]
    Speak {
        text: String,
        #[serde(default)]
        output_device: Option<String>,
        #[serde(default = "default_volume")]
        volume: f32,
    },
    #[serde(rename = "synthesize")]
    Synthesize { text: String },
    #[serde(rename = "list_devices")]
    ListDevices,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "shutdown")]
    Shutdown,
}

fn default_volume() -> f32 {
    1.0
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum Response {
    #[serde(rename = "ok")]
    Ok { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "devices")]
    Devices { devices: Vec<String> },
    #[serde(rename = "status")]
    Status {
        loaded: bool,
        model_path: Option<String>,
    },
    #[serde(rename = "audio")]
    Audio {
        /// Base64 encoded f32 PCM samples
        audio_base64: String,
        sample_rate: u32,
    },
}

struct TtsState {
    synth: Option<Arc<PiperSpeechSynthesizer>>,
    model_path: Option<String>,
    sample_rate: u32,
}

impl TtsState {
    fn new() -> Self {
        Self {
            synth: None,
            model_path: None,
            sample_rate: 22050, // Piper default
        }
    }

    fn load_model(&mut self, model_path: &str) -> Result<(), String> {
        // model_path should be the path to the .onnx.json config file
        // If user provides .onnx path, try to find the corresponding .json
        let config_path = if model_path.ends_with(".onnx") {
            format!("{}.json", model_path)
        } else if model_path.ends_with(".onnx.json") {
            model_path.to_string()
        } else {
            // Assume it's a base path, try adding .onnx.json
            format!("{}.onnx.json", model_path)
        };

        let path = Path::new(&config_path);

        if !path.exists() {
            return Err(format!("Model config file does not exist: {}", config_path));
        }

        // Don't reload if same model
        if self.model_path.as_deref() == Some(model_path) && self.synth.is_some() {
            return Ok(());
        }

        // Unload existing model first
        self.synth = None;
        self.model_path = None;

        // Load the Piper model from config
        let model = piper_rs::from_config_path(path)
            .map_err(|e| format!("Failed to load Piper model config: {}", e))?;

        let synth = PiperSpeechSynthesizer::new(model)
            .map_err(|e| format!("Failed to create Piper synthesizer: {}", e))?;

        // Get sample rate from the model config if available
        // Piper models typically use 22050 Hz
        self.sample_rate = 22050;

        self.synth = Some(Arc::new(synth));
        self.model_path = Some(model_path.to_string());

        Ok(())
    }

    fn unload_model(&mut self) {
        self.synth = None;
        self.model_path = None;
    }

    fn is_loaded(&self) -> bool {
        self.synth.is_some()
    }

    fn speak(
        &self,
        text: &str,
        output_device: Option<&str>,
        volume: f32,
    ) -> Result<(), String> {
        let samples = self.synthesize_samples(text)?;
        // Play the audio
        self.play_audio(&samples, self.sample_rate, output_device, volume)
    }

    /// Synthesize audio and return raw f32 samples
    fn synthesize_samples(&self, text: &str) -> Result<Vec<f32>, String> {
        let synth = self
            .synth
            .as_ref()
            .ok_or_else(|| "No TTS model loaded".to_string())?;

        // Synthesize audio using parallel synthesis
        let audio_results = synth
            .synthesize_parallel(text.to_string(), None)
            .map_err(|e| format!("Failed to synthesize: {}", e))?;

        // Collect all samples from the parallel results
        let mut samples: Vec<f32> = Vec::new();
        for result in audio_results {
            let chunk = result.map_err(|e| format!("Failed to get audio chunk: {}", e))?;
            samples.extend(chunk.into_vec());
        }

        Ok(samples)
    }

    /// Synthesize audio and return as base64-encoded bytes
    fn synthesize(&self, text: &str) -> Result<(String, u32), String> {
        let samples = self.synthesize_samples(text)?;

        // Convert f32 samples to bytes
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|&s| s.to_le_bytes())
            .collect();

        // Encode as base64
        let audio_base64 = BASE64.encode(&bytes);

        Ok((audio_base64, self.sample_rate))
    }

    fn play_audio(
        &self,
        samples: &[f32],
        sample_rate: u32,
        output_device: Option<&str>,
        volume: f32,
    ) -> Result<(), String> {
        // Get the output stream for the specified device or default
        let (_stream, stream_handle) = if let Some(device_name) = output_device {
            // Try to find the specified device
            let host = cpal::default_host();
            let device = host
                .output_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .find(|d| {
                    d.name()
                        .map(|n| n.contains(device_name) || device_name.contains(&n))
                        .unwrap_or(false)
                });

            if let Some(dev) = device {
                log::info!("Using output device: {:?}", dev.name());
                OutputStream::try_from_device(&dev)
                    .map_err(|e| format!("Failed to open device '{}': {}", device_name, e))?
            } else {
                log::warn!(
                    "Device '{}' not found, using default",
                    device_name
                );
                OutputStream::try_default()
                    .map_err(|e| format!("Failed to open default device: {}", e))?
            }
        } else {
            OutputStream::try_default()
                .map_err(|e| format!("Failed to open default device: {}", e))?
        };

        // Create a sink for playback
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| format!("Failed to create sink: {}", e))?;

        // Set volume
        sink.set_volume(volume);

        // Create a source from the samples using rodio's SamplesBuffer
        // Piper outputs mono audio at the specified sample rate
        let buf = SamplesBuffer::new(1, sample_rate, samples.to_vec());

        // Play and wait for completion
        sink.append(buf);
        sink.sleep_until_end();

        Ok(())
    }
}

fn list_output_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| format!("Failed to enumerate devices: {}", e))?;

    let names: Vec<String> = devices
        .filter_map(|d| d.name().ok())
        .collect();

    Ok(names)
}

fn send_response(response: &Response) {
    let json = serde_json::to_string(response).unwrap();
    println!("{}", json);
    io::stdout().flush().unwrap();
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    log::info!("TTS sidecar starting...");

    let mut state = TtsState::new();

    // Signal ready
    send_response(&Response::Ok {
        message: "TTS sidecar ready".to_string(),
    });

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log::error!("Failed to read stdin: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                send_response(&Response::Error {
                    message: format!("Invalid JSON: {}", e),
                });
                continue;
            }
        };

        match request {
            Request::Load { model_path } => {
                log::info!("Loading TTS model: {}", model_path);
                match state.load_model(&model_path) {
                    Ok(()) => {
                        log::info!("TTS model loaded successfully");
                        send_response(&Response::Ok {
                            message: "Model loaded".to_string(),
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to load TTS model: {}", e);
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Unload => {
                log::info!("Unloading TTS model");
                state.unload_model();
                send_response(&Response::Ok {
                    message: "Model unloaded".to_string(),
                });
            }
            Request::Speak {
                text,
                output_device,
                volume,
            } => {
                log::info!("Speak request: {} chars", text.len());
                match state.speak(&text, output_device.as_deref(), volume) {
                    Ok(()) => {
                        send_response(&Response::Ok {
                            message: "Speech complete".to_string(),
                        });
                    }
                    Err(e) => {
                        log::error!("Speech failed: {}", e);
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Synthesize { text } => {
                log::debug!("Synthesize request: {} chars", text.len());
                match state.synthesize(&text) {
                    Ok((audio_base64, sample_rate)) => {
                        send_response(&Response::Audio {
                            audio_base64,
                            sample_rate,
                        });
                    }
                    Err(e) => {
                        log::error!("Synthesis failed: {}", e);
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::ListDevices => {
                match list_output_devices() {
                    Ok(devices) => {
                        send_response(&Response::Devices { devices });
                    }
                    Err(e) => {
                        send_response(&Response::Error { message: e });
                    }
                }
            }
            Request::Status => {
                send_response(&Response::Status {
                    loaded: state.is_loaded(),
                    model_path: state.model_path.clone(),
                });
            }
            Request::Shutdown => {
                log::info!("Shutdown requested");
                send_response(&Response::Ok {
                    message: "Shutting down".to_string(),
                });
                break;
            }
        }
    }

    log::info!("TTS sidecar exiting");
}
