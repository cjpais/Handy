use log::{debug, info, warn};
use rodio::OutputStreamBuilder;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;

/// Piper TTS model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiperConfig {
    pub audio: AudioConfig,
    pub phoneme_type: Option<String>,
    pub phoneme_id_map: Option<std::collections::HashMap<String, Vec<i64>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub sample_rate: u32,
}

impl Default for PiperConfig {
    fn default() -> Self {
        Self {
            audio: AudioConfig { sample_rate: 22050 },
            phoneme_type: None,
            phoneme_id_map: None,
        }
    }
}

/// Local TTS placeholder
/// Note: Full Piper TTS implementation requires espeak-ng for phonemization
/// This is a placeholder that can be extended later
pub struct LocalTts {
    config: PiperConfig,
    model_path: Option<String>,
    is_loaded: bool,
}

impl LocalTts {
    pub fn new() -> Self {
        Self {
            config: PiperConfig::default(),
            model_path: None,
            is_loaded: false,
        }
    }

    pub fn load_model(&mut self, model_path: &Path) -> Result<(), String> {
        let path_str = model_path.to_string_lossy().to_string();

        // Don't reload if same model
        if self.model_path.as_ref() == Some(&path_str) && self.is_loaded {
            debug!("TTS model already loaded: {}", path_str);
            return Ok(());
        }

        info!("Loading TTS model from: {}", path_str);

        // Check if model file exists
        if !model_path.exists() {
            return Err(format!("TTS model file not found: {}", path_str));
        }

        // Load config if exists
        let config_path = model_path.with_extension("onnx.json");
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match serde_json::from_str::<PiperConfig>(&content) {
                    Ok(config) => {
                        self.config = config;
                        debug!("Loaded TTS config: sample_rate={}", self.config.audio.sample_rate);
                    }
                    Err(e) => {
                        warn!("Failed to parse TTS config: {}, using defaults", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read TTS config: {}, using defaults", e);
                }
            }
        }

        // Note: Full ONNX model loading would require proper phonemization
        // For now, we just mark as loaded and use system TTS as fallback
        self.model_path = Some(path_str.clone());
        self.is_loaded = true;

        info!("TTS model loaded (stub): {}", path_str);
        Ok(())
    }

    pub fn unload_model(&mut self) {
        if self.is_loaded {
            info!("Unloading TTS model");
            self.is_loaded = false;
            self.model_path = None;
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.is_loaded
    }

    /// Synthesize speech from text
    /// Note: This is a placeholder - full implementation needs espeak-ng phonemization
    pub fn synthesize(&self, text: &str) -> Result<Vec<f32>, String> {
        if !self.is_loaded {
            return Err("No TTS model loaded".to_string());
        }

        // Placeholder: Generate silence for the duration that would be spoken
        // In a full implementation, this would:
        // 1. Convert text to phonemes using espeak-ng
        // 2. Convert phonemes to IDs using the model's phoneme_id_map
        // 3. Run ONNX inference
        // 4. Return the audio samples

        warn!("TTS synthesis is using placeholder (silence) - full Piper implementation pending");

        // Generate ~1 second of silence per 10 characters as placeholder
        let sample_rate = self.config.audio.sample_rate;
        let duration_seconds = (text.len() as f32 / 10.0).max(0.5);
        let num_samples = (sample_rate as f32 * duration_seconds) as usize;

        Ok(vec![0.0f32; num_samples])
    }

    /// Get sample rate for the loaded model
    pub fn sample_rate(&self) -> u32 {
        self.config.audio.sample_rate
    }
}

impl Default for LocalTts {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper for LocalTts
pub struct LocalTtsManager {
    tts: Mutex<LocalTts>,
}

impl LocalTtsManager {
    pub fn new() -> Self {
        Self {
            tts: Mutex::new(LocalTts::new()),
        }
    }

    pub fn load_model(&self, model_path: &Path) -> Result<(), String> {
        let mut tts_guard = self.tts.lock().unwrap();
        tts_guard.load_model(model_path)
    }

    pub fn unload_model(&self) {
        let mut tts_guard = self.tts.lock().unwrap();
        tts_guard.unload_model();
    }

    pub fn is_loaded(&self) -> bool {
        let tts_guard = self.tts.lock().unwrap();
        tts_guard.is_loaded()
    }

    pub fn synthesize(&self, text: &str) -> Result<Vec<f32>, String> {
        let tts_guard = self.tts.lock().unwrap();
        tts_guard.synthesize(text)
    }

    pub fn sample_rate(&self) -> u32 {
        let tts_guard = self.tts.lock().unwrap();
        tts_guard.sample_rate()
    }

    /// Synthesize and play audio
    pub fn speak(&self, text: &str, volume: f32) -> Result<(), String> {
        let samples = self.synthesize(text)?;
        let sample_rate = self.sample_rate();

        // If we only have silence (placeholder), log warning and return
        if samples.iter().all(|&s| s.abs() < 0.001) {
            warn!("TTS produced silence - Piper model integration pending");
            // For now, just return success - in production this would use system TTS
            return Ok(());
        }

        // Convert to WAV in memory
        let wav_data = self.samples_to_wav(&samples, sample_rate)?;

        // Play using rodio
        let stream_builder = OutputStreamBuilder::from_default_device()
            .map_err(|e| format!("Failed to get audio output: {}", e))?;

        let stream_handle = stream_builder
            .open_stream()
            .map_err(|e| format!("Failed to open audio stream: {}", e))?;

        let mixer = stream_handle.mixer();
        let cursor = Cursor::new(wav_data);

        let sink = rodio::play(mixer, cursor)
            .map_err(|e| format!("Failed to play audio: {}", e))?;

        sink.set_volume(volume);
        sink.sleep_until_end();

        Ok(())
    }

    fn samples_to_wav(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
        let mut wav_data = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut wav_data);

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        for &sample in samples {
            let sample_i16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer
                .write_sample(sample_i16)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

        Ok(wav_data)
    }
}

impl Default for LocalTtsManager {
    fn default() -> Self {
        Self::new()
    }
}
