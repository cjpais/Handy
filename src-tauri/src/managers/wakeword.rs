use anyhow::{anyhow, Context, Result};
use ort::{inputs, session::Session, value::Tensor};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::Manager;

const FRAME_SAMPLES: usize = 1280; // target chunk size for inference
const MEL_ROWS: usize = 76;
const MEL_COLS: usize = 32;
const EMBED_DIMS: usize = 96;

pub struct WakeWordManager {
    app_handle: tauri::AppHandle,
    threshold: f32,
    // Model sessions
    melspec: Session,
    embed: Session,
    wake: Session,
    // Streaming buffers
    mel_buf: VecDeque<[f32; MEL_COLS]>,
    feat_buf: VecDeque<[f32; EMBED_DIMS]>,
    wake_window: usize,
    audio_tail: Vec<i16>,
    // Sample aggregation buffer (i16)
    i16_buf: Vec<i16>,
    frames_seen: usize,
    verbose: bool,
    running: bool,
}
impl WakeWordManager {
    pub fn new(app_handle: &tauri::AppHandle, verbose: bool, threshold: f32) -> Result<Self> {
        // Resolve models with robust fallbacks for dev vs bundled paths
        fn try_resolve(app: &tauri::AppHandle, rel: &str) -> Option<PathBuf> {
            // Attempt resolution via the Tauri Resource base
            if let Ok(p) = app
                .path()
                .resolve(rel, tauri::path::BaseDirectory::Resource)
            {
                if p.exists() {
                    return Some(p);
                }
            }
            // Fallback to current working directory (useful in dev)
            if let Ok(cwd) = std::env::current_dir() {
                let p = cwd.join(rel);
                if p.exists() {
                    return Some(p);
                }
            }
            None
        }

        // Candidate relative paths to handle both dev and bundled layouts
        let candidates = [
            (
                "melspectrogram.onnx",
                [
                    "resources/models/melspectrogram.onnx",
                    "models/melspectrogram.onnx",
                    "src-tauri/resources/models/melspectrogram.onnx",
                ],
            ),
            (
                "embedding_model.onnx",
                [
                    "resources/models/embedding_model.onnx",
                    "models/embedding_model.onnx",
                    "src-tauri/resources/models/embedding_model.onnx",
                ],
            ),
            (
                "hey_mycroft_v0.1.onnx",
                [
                    "resources/models/hey_mycroft_v0.1.onnx",
                    "models/hey_mycroft_v0.1.onnx",
                    "src-tauri/resources/models/hey_mycroft_v0.1.onnx",
                ],
            ),
        ];

        let mut melspec_path: Option<PathBuf> = None;
        let mut embed_path: Option<PathBuf> = None;
        let mut wake_path: Option<PathBuf> = None;

        for (name, paths) in candidates.iter() {
            let mut found: Option<PathBuf> = None;
            for rel in paths {
                if let Some(p) = try_resolve(app_handle, rel) {
                    found = Some(p);
                    break;
                }
            }
            match *name {
                "melspectrogram.onnx" => melspec_path = found,
                "embedding_model.onnx" => embed_path = found,
                "hey_mycroft_v0.1.onnx" => wake_path = found,
                _ => {}
            }
        }

        let (melspec_path, embed_path, wake_path) = (melspec_path, embed_path, wake_path);
        if melspec_path.is_none() || embed_path.is_none() || wake_path.is_none() {
            return Err(anyhow!(
                "Wake-word models not found in resources/models (melspectrogram.onnx, embedding_model.onnx, hey_mycroft_v0.1.onnx)"
            ));
        }
        let melspec_path = melspec_path.unwrap();
        let embed_path = embed_path.unwrap();
        let wake_path = wake_path.unwrap();

        log::info!(
            "Wake-word: resolved models -> melspec={:?}, embed={:?}, wake={:?}",
            melspec_path,
            embed_path,
            wake_path
        );

        let melspec = Session::builder()?
            .with_intra_threads(1)?
            .with_inter_threads(1)?
            .commit_from_file(melspec_path)?;
        let embed = Session::builder()?
            .with_intra_threads(1)?
            .with_inter_threads(1)?
            .commit_from_file(embed_path)?;
        let wake = Session::builder()?
            .with_intra_threads(1)?
            .with_inter_threads(1)?
            .commit_from_file(wake_path)?;

        log::info!("Wake-word: ONNX sessions created");

        // Initialize buffers
        let mut mel_buf = VecDeque::with_capacity(MEL_ROWS);
        for _ in 0..MEL_ROWS {
            mel_buf.push_back([0.0; MEL_COLS]);
        }

        let mut feat_buf = VecDeque::new();
        // Seed with zeros to avoid random triggers on startup
        for _ in 0..41 {
            feat_buf.push_back([0.0f32; EMBED_DIMS]);
        }

        Ok(Self {
            app_handle: app_handle.clone(),
            threshold,
            melspec,
            embed,
            wake,
            mel_buf,
            feat_buf,
            wake_window: 16,
            audio_tail: vec![0i16; 480],
            i16_buf: Vec::with_capacity(FRAME_SAMPLES * 2),
            frames_seen: 0,
            verbose,
            running: false,
        })
    }

    pub fn start(&mut self) {
        self.running = true;
        // Reset counters and streaming buffers so early frames are ignored and
        // previous context doesn't leak into a new detection window.
        self.frames_seen = 0;
        self.i16_buf.clear();
        // Reinitialize mel buffer with zeros
        self.mel_buf.clear();
        for _ in 0..MEL_ROWS {
            self.mel_buf.push_back([0.0; MEL_COLS]);
        }
        // Re-seed features buffer with zeros to dampen initial activations
        self.feat_buf.clear();
        for _ in 0..41 {
            self.feat_buf.push_back([0.0f32; EMBED_DIMS]);
        }
        // Clear audio tail
        self.audio_tail.clear();
        self.audio_tail.extend_from_slice(&[0i16; 480]);
        log::info!("Wake-word: started (threshold={})", self.threshold);
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.i16_buf.clear();
        log::info!("Wake-word: stopped");
    }

    /// Accept 30ms speech frames (f32, -1..1), aggregate to 1280-sample chunks and run inference.
    pub fn on_speech_frame(&mut self, frame: &[f32]) -> Result<()> {
        log::debug!("Calling on_speech_frame on wakeword");
        if !self.running {
            return Ok(());
        }

        log::info!("Wake-word: speech frame received len={}", frame.len());

        // Convert f32 to i16 and append
        self.i16_buf.reserve(frame.len());
        for &x in frame {
            let s = (x.clamp(-1.0, 1.0) * 32767.0) as i16;
            self.i16_buf.push(s);
        }

        // Process as many full chunks as available
        while self.i16_buf.len() >= FRAME_SAMPLES {
            let chunk: Vec<i16> = self.i16_buf.drain(0..FRAME_SAMPLES).collect();
            let p = match self.predict(&chunk) {
                Ok(val) => val,
                Err(e) => {
                    log::error!("Wake-word: predict error: {}", e);
                    0.0
                }
            };
            if self.verbose {
                log::debug!("wake p={:.6}", p);
            }
            log::debug!("Wake-word: p={:.4} (frame={})", p, self.frames_seen);
            if self.frames_seen >= 5 && p > self.threshold {
                // Immediately inactivate wake-word before any recording to avoid interference
                self.stop();
                log::debug!("Ordering: wake-word stopped; scheduling start/stop actions");

                // Start a short recording window (5 seconds) via transcription action
                // Run actions asynchronously to avoid blocking the UI thread.
                let app_start = self.app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    super::super::actions::start_transcription_via_wakeword(&app_start);
                });

                let app_stop = self.app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    super::super::actions::stop_transcription_via_wakeword(&app_stop);
                });
            }
        }

        Ok(())
    }

    fn predict(&mut self, frame_i16: &[i16]) -> Result<f32> {
        log::info!(
            "Wake-word: starting prediction (frame_i16.len={})",
            frame_i16.len()
        );
        if frame_i16.len() != FRAME_SAMPLES {
            return Err(anyhow!("frame must be {} samples", FRAME_SAMPLES));
        }

        // Build streaming window: last 480 samples + this frame (1280) => up to 1760
        let mut window_i16: Vec<i16> = Vec::with_capacity(480 + FRAME_SAMPLES);
        window_i16.extend_from_slice(&self.audio_tail);
        window_i16.extend_from_slice(frame_i16);
        if window_i16.len() > FRAME_SAMPLES + 480 {
            let start = window_i16.len() - (FRAME_SAMPLES + 480);
            window_i16.drain(0..start);
        }
        // Update tail for next call: last 480 samples of current window
        if window_i16.len() >= 480 {
            let start_tail = window_i16.len() - 480;
            self.audio_tail.clear();
            self.audio_tail.extend_from_slice(&window_i16[start_tail..]);
        }

        // Convert to f32 and create tensor (1, len)
        let x: Vec<f32> = window_i16.iter().map(|&s| s as f32).collect();
        let x_len = x.len();
        let input_tensor = Tensor::from_array(([1usize, x_len], x.into_boxed_slice()))?;
        let mel_in_name = self.melspec.inputs[0].name.clone();
        let outputs = match self.melspec.run(inputs! { &*mel_in_name => input_tensor }) {
            Ok(o) => o,
            Err(e) => {
                log::error!("Wake-word: melspec run error: {}", e);
                return Err(anyhow!("melspec run error: {}", e));
            }
        };
        let mel_arr = outputs[0]
            .try_extract_array::<f32>()
            .context("extract mel array")?;

        // Normalize: spec/10 + 2 and append all rows to mel buffer.
        let shape = mel_arr.shape().to_vec();
        match shape.as_slice() {
            // [32] => single row
            [mel_cols_const] if *mel_cols_const == MEL_COLS => {
                let mut row = [0.0f32; MEL_COLS];
                for (i, v) in mel_arr.iter().take(MEL_COLS).enumerate() {
                    row[i] = *v / 10.0 + 2.0;
                }
                self.mel_buf.push_back(row);
            }
            // [rows, 32]
            [rows, mel_cols_const] if *mel_cols_const == MEL_COLS => {
                let rows = *rows;
                let data = mel_arr.iter().copied().collect::<Vec<f32>>();
                for r in 0..rows {
                    let mut row = [0.0f32; MEL_COLS];
                    let offs = r * MEL_COLS;
                    for c in 0..MEL_COLS {
                        row[c] = data[offs + c] / 10.0 + 2.0;
                    }
                    self.mel_buf.push_back(row);
                }
            }
            // [1, rows, 32]
            [1, rows, mel_cols_const] if *mel_cols_const == MEL_COLS => {
                let rows = *rows;
                let data = mel_arr.iter().copied().collect::<Vec<f32>>();
                for r in 0..rows {
                    let mut row = [0.0f32; MEL_COLS];
                    let offs = r * MEL_COLS;
                    for c in 0..MEL_COLS {
                        row[c] = data[offs + c] / 10.0 + 2.0;
                    }
                    self.mel_buf.push_back(row);
                }
            }
            // [1, 1, rows, 32]
            [1, 1, rows, mel_cols_const] if *mel_cols_const == MEL_COLS => {
                let rows = *rows;
                let data = mel_arr.iter().copied().collect::<Vec<f32>>();
                for r in 0..rows {
                    let mut row = [0.0f32; MEL_COLS];
                    let offs = r * MEL_COLS;
                    for c in 0..MEL_COLS {
                        row[c] = data[offs + c] / 10.0 + 2.0;
                    }
                    self.mel_buf.push_back(row);
                }
            }
            _ => {
                // Fallback: take last 32 values as one row
                let flat = mel_arr.iter().copied().collect::<Vec<f32>>();
                let mut row = [0.0f32; MEL_COLS];
                for i in 0..MEL_COLS {
                    let v = if flat.len() >= MEL_COLS {
                        flat[flat.len() - MEL_COLS + i]
                    } else {
                        0.0
                    };
                    row[i] = v / 10.0 + 2.0;
                }
                self.mel_buf.push_back(row);
            }
        }
        while self.mel_buf.len() > MEL_ROWS {
            self.mel_buf.pop_front();
        }

        // Prepare (1, 76, 32, 1)
        let mut emb_input = Vec::with_capacity(MEL_ROWS * MEL_COLS);
        for r in self
            .mel_buf
            .iter()
            .rev()
            .take(MEL_ROWS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            emb_input.extend_from_slice(r);
        }
        let emb_tensor = Tensor::from_array((
            [1usize, MEL_ROWS, MEL_COLS, 1usize],
            emb_input.into_boxed_slice(),
        ))?;

        let emb_in_name = self.embed.inputs[0].name.clone();
        let emb_out = match self.embed.run(inputs! { &*emb_in_name => emb_tensor }) {
            Ok(o) => o,
            Err(e) => {
                log::error!("Wake-word: embed run error: {}", e);
                return Err(anyhow!("embed run error: {}", e));
            }
        };
        let emb_arr = emb_out[0]
            .try_extract_array::<f32>()
            .context("extract embedding")?;
        let flat_emb = emb_arr.iter().copied().collect::<Vec<f32>>();
        let mut emb_vec = [0.0f32; EMBED_DIMS];
        for i in 0..EMBED_DIMS {
            emb_vec[i] = if flat_emb.len() > i { flat_emb[i] } else { 0.0 };
        }
        self.feat_buf.push_back(emb_vec);
        while self.feat_buf.len() > 512 {
            self.feat_buf.pop_front();
        }

        // Select window
        let t = self.wake_window.min(self.feat_buf.len());
        let start = self.feat_buf.len() - t;
        let mut wake_input = Vec::with_capacity(t * EMBED_DIMS);
        for v in self.feat_buf.iter().skip(start) {
            wake_input.extend_from_slice(v);
        }
        let wake_tensor =
            Tensor::from_array(([1usize, t, EMBED_DIMS], wake_input.into_boxed_slice()))?;
        let in_name = self.wake.inputs[0].name.clone();
        let wake_out = match self.wake.run(inputs! { &*in_name => wake_tensor }) {
            Ok(o) => o,
            Err(e) => {
                log::error!("Wake-word: wake run error: {}", e);
                return Err(anyhow!("wake run error: {}", e));
            }
        };
        let prob = wake_out[0]
            .try_extract_array::<f32>()
            .context("extract wake prob")?;
        let first = prob
            .iter()
            .copied()
            .next()
            .ok_or_else(|| anyhow!("empty wake output"))?;
        self.frames_seen = self.frames_seen.saturating_add(1);
        Ok(first)
    }
}

pub type WakeWordManagerHandle = Arc<Mutex<WakeWordManager>>;

#[cfg(test)]
mod tests {
    use super::*;
    use hound;

    // Helper to read mono PCM samples from a WAV file as i16.
    fn read_wav_i16(path: &std::path::Path) -> Result<Vec<i16>> {
        let mut reader = hound::WavReader::open(path).map_err(|e| anyhow!("open wav: {}", e))?;
        let spec = reader.spec();
        if spec.channels != 1 {
            return Err(anyhow!("expected mono wav, got {} channels", spec.channels));
        }
        // Prefer int samples; fallback to f32
        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Int => reader.samples::<i16>().map(|s| s.unwrap_or(0)).collect(),
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .map(|s| {
                    let x = s.unwrap_or(0.0).clamp(-1.0, 1.0);
                    (x * 32767.0) as i16
                })
                .collect(),
        };
        Ok(samples)
    }

    #[test]
    fn wakeword_prob_exceeds_threshold_on_test_wav() -> Result<()> {
        // Build a minimal Tauri app to get an AppHandle for resource resolution and event emits.
        let app = tauri::Builder::default()
            .build(tauri::generate_context!())
            .map_err(|e| anyhow!("build tauri app: {}", e))?;
        let handle = app.handle();

        // Initialize manager (verbose off to keep test logs tidy).
        let mut mgr = WakeWordManager::new(&handle, false, 0.5)?;

        // Load test audio from resources.
        let wav_path = std::path::Path::new("resources/hey_mycroft_test.wav");
        let mut samples = read_wav_i16(wav_path)?;

        // Stream through the model in FRAME_SAMPLES chunks, track max probability.
        let mut max_p = 0.0f32;
        while samples.len() >= FRAME_SAMPLES {
            let chunk: Vec<i16> = samples.drain(0..FRAME_SAMPLES).collect();
            let p = mgr.predict(&chunk)?;
            if p > max_p {
                max_p = p;
            }
        }

        // Basic assertion: expect a noticeable activation on the wake-word audio.
        // Threshold chosen conservatively to avoid flakiness across platforms.
        assert!(max_p > 0.40, "max wake-word prob too low: {:.3}", max_p);
        Ok(())
    }
}
