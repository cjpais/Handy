use anyhow::{anyhow, Context, Result};
use ndarray::{Array2, Array3, Array4};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

/// openWakeWord audio frontend constants. The models are trained on 16 kHz
/// audio processed in 80 ms chunks; with look-back context each chunk yields
/// 8 melspectrogram frames (10 ms hop), the embedding backbone consumes a
/// 76-frame window, and the classifier head consumes a 16-embedding window
/// (~1.28 s of context).
pub const WAKE_CHUNK_SAMPLES: usize = 1280; // 80 ms @ 16 kHz
const MEL_BINS: usize = 32;
const MEL_WINDOW: usize = 76;
const EMBEDDING_DIM: usize = 96;
const EMBEDDING_WINDOW: usize = 16;
/// Look-back context prepended when computing a chunk's melspectrogram
/// (3 × 160-sample hops, matching openWakeWord's streaming implementation).
/// Without it, chunk-boundary frames are computed against a truncated window
/// and the effective mel frame rate is wrong — the official v0.1 heads
/// tolerate that, but newer community-trained models do not.
const MEL_LOOKBACK_SAMPLES: usize = 160 * 3;
/// Raw-sample history retained for the look-back window.
const RAW_BUFFER_LEN: usize = WAKE_CHUNK_SAMPLES + MEL_LOOKBACK_SAMPLES;

#[derive(Debug, Clone, Copy)]
pub struct WakeWordConfig {
    /// Detection score (0..1) at or above which a chunk counts as a hit.
    pub threshold: f32,
    /// Consecutive hit chunks required to trigger.
    pub trigger_chunks: u32,
    /// Quiet period after a trigger during which detection is suppressed.
    pub refractory: Duration,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            trigger_chunks: 1,
            refractory: Duration::from_secs(2),
        }
    }
}

/// Three-stage openWakeWord pipeline (melspectrogram -> shared embedding ->
/// per-word classifier head) running on `ort`.
pub struct WakeWordDetector {
    melspec: Session,
    embedding: Session,
    head: Session,
    // I/O names read from model metadata: head input names vary between the
    // pretrained models ("x.1" for hey_jarvis, "onnx::Flatten_0" for others)
    // and Colab-trained custom heads.
    melspec_input: String,
    melspec_output: String,
    embedding_input: String,
    embedding_output: String,
    head_input: String,
    head_output: String,
    sample_buf: Vec<f32>,
    /// Trailing raw samples (chunk + look-back) for melspec context.
    raw_history: VecDeque<f32>,
    mel_frames: VecDeque<[f32; MEL_BINS]>,
    embeddings: VecDeque<[f32; EMBEDDING_DIM]>,
    config: WakeWordConfig,
    consecutive_hits: u32,
    refractory_until: Option<Instant>,
}

fn create_session(path: &Path) -> Result<Session> {
    // The builder methods' intermediate error type is not Send+Sync; convert
    // to ort::Error explicitly (same workaround as the in-tree vad-rs fork).
    Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| -> ort::Error { e.into() })?
        .with_intra_threads(1)
        .map_err(|e| -> ort::Error { e.into() })?
        .with_inter_threads(1)
        .map_err(|e| -> ort::Error { e.into() })?
        .commit_from_file(path)
        .with_context(|| format!("failed to load ONNX model at {}", path.display()))
}

fn single_io_names(session: &Session, what: &str) -> Result<(String, String)> {
    let input = session
        .inputs()
        .first()
        .ok_or_else(|| anyhow!("{what} model has no inputs"))?
        .name()
        .to_string();
    let output = session
        .outputs()
        .first()
        .ok_or_else(|| anyhow!("{what} model has no outputs"))?
        .name()
        .to_string();
    Ok((input, output))
}

impl WakeWordDetector {
    pub fn new(
        melspec_path: &Path,
        embedding_path: &Path,
        head_path: &Path,
        config: WakeWordConfig,
    ) -> Result<Self> {
        let melspec = create_session(melspec_path)?;
        let embedding = create_session(embedding_path)?;
        let head = create_session(head_path)?;

        let (melspec_input, melspec_output) = single_io_names(&melspec, "melspectrogram")?;
        let (embedding_input, embedding_output) = single_io_names(&embedding, "embedding")?;
        let (head_input, head_output) = single_io_names(&head, "wake-word head")?;

        log::info!(
            "Wake-word detector loaded: melspec[{}->{}] embedding[{}->{}] head[{}->{}] ({})",
            melspec_input,
            melspec_output,
            embedding_input,
            embedding_output,
            head_input,
            head_output,
            head_path.display()
        );

        Ok(Self {
            melspec,
            embedding,
            head,
            melspec_input,
            melspec_output,
            embedding_input,
            embedding_output,
            head_input,
            head_output,
            sample_buf: Vec::with_capacity(WAKE_CHUNK_SAMPLES * 2),
            raw_history: VecDeque::with_capacity(RAW_BUFFER_LEN),
            mel_frames: VecDeque::with_capacity(MEL_WINDOW + 16),
            embeddings: VecDeque::with_capacity(EMBEDDING_WINDOW + 1),
            config,
            consecutive_hits: 0,
            refractory_until: None,
        })
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.config.threshold = threshold.clamp(0.01, 0.99);
    }

    /// Feed 16 kHz mono f32 samples (-1..1, any length; typically the
    /// recorder's 30 ms/480-sample frames). Returns `true` exactly once per
    /// detection; the detector then clears its context and enters refractory.
    pub fn push_frame(&mut self, samples: &[f32]) -> Result<bool> {
        self.sample_buf.extend_from_slice(samples);

        let mut detected = false;
        while self.sample_buf.len() >= WAKE_CHUNK_SAMPLES {
            let chunk: Vec<f32> = self.sample_buf.drain(..WAKE_CHUNK_SAMPLES).collect();
            let score = self.process_chunk(&chunk)?;

            if let Some(until) = self.refractory_until {
                if Instant::now() < until {
                    self.consecutive_hits = 0;
                    continue;
                }
                self.refractory_until = None;
            }

            if score >= self.config.threshold {
                self.consecutive_hits += 1;
                if self.consecutive_hits >= self.config.trigger_chunks {
                    detected = true;
                    self.trigger_reset();
                }
            } else {
                self.consecutive_hits = 0;
            }
        }
        Ok(detected)
    }

    /// Clear all rolling context (e.g. when idle listening resumes after a
    /// recording session, so stale audio can't influence the next score).
    pub fn reset(&mut self) {
        self.sample_buf.clear();
        self.raw_history.clear();
        self.mel_frames.clear();
        self.embeddings.clear();
        self.consecutive_hits = 0;
    }

    fn trigger_reset(&mut self) {
        self.reset();
        self.refractory_until = Some(Instant::now() + self.config.refractory);
    }

    fn process_chunk(&mut self, chunk: &[f32]) -> Result<f32> {
        // Maintain a raw-sample history so each chunk's melspectrogram is
        // computed with look-back context (openWakeWord streaming semantics):
        // melspec over the trailing chunk + 480 samples, all resulting frames
        // appended — yielding the true 10 ms-hop frame rate without boundary
        // artifacts.
        self.raw_history.extend(chunk.iter().copied());
        while self.raw_history.len() > RAW_BUFFER_LEN {
            self.raw_history.pop_front();
        }

        // openWakeWord models are trained on 16-bit PCM-range floats.
        let scaled: Vec<f32> = self.raw_history.iter().map(|s| s * 32767.0).collect();

        // Stage 1: melspectrogram — input [1, N] -> [1, 1, frames, 32].
        let mel_in = Array2::from_shape_vec((1, scaled.len()), scaled)?;
        let mel_data: Vec<f32> = {
            let outputs = self.melspec.run(vec![(
                Cow::Owned(self.melspec_input.clone()),
                Value::from_array(mel_in)?.into_dyn(),
            )])?;
            let (mel_shape, mel_data) = outputs
                .get(self.melspec_output.as_str())
                .ok_or_else(|| anyhow!("melspectrogram output missing"))?
                .try_extract_tensor::<f32>()?;
            let mel_cols = *mel_shape
                .last()
                .ok_or_else(|| anyhow!("melspectrogram output has no shape"))?
                as usize;
            if mel_cols != MEL_BINS {
                return Err(anyhow!(
                    "unexpected melspectrogram output shape {mel_shape:?} (want last dim {MEL_BINS})"
                ));
            }
            mel_data.to_vec()
        };
        for frame in mel_data.chunks_exact(MEL_BINS) {
            let mut arr = [0f32; MEL_BINS];
            for (dst, src) in arr.iter_mut().zip(frame) {
                // openWakeWord's fixed melspectrogram transform.
                *dst = src / 10.0 + 2.0;
            }
            self.mel_frames.push_back(arr);
        }
        while self.mel_frames.len() > MEL_WINDOW {
            self.mel_frames.pop_front();
        }
        if self.mel_frames.len() < MEL_WINDOW {
            return Ok(0.0);
        }

        // Stage 2: embedding — input [1, 76, 32, 1] -> [1, 1, 1, 96].
        let mut emb_in = Array4::<f32>::zeros((1, MEL_WINDOW, MEL_BINS, 1));
        for (i, frame) in self.mel_frames.iter().enumerate() {
            for (j, v) in frame.iter().enumerate() {
                emb_in[[0, i, j, 0]] = *v;
            }
        }
        let emb = {
            let outputs = self.embedding.run(vec![(
                Cow::Owned(self.embedding_input.clone()),
                Value::from_array(emb_in)?.into_dyn(),
            )])?;
            let (emb_shape, emb_data) = outputs
                .get(self.embedding_output.as_str())
                .ok_or_else(|| anyhow!("embedding output missing"))?
                .try_extract_tensor::<f32>()?;
            if emb_data.len() != EMBEDDING_DIM {
                return Err(anyhow!(
                    "unexpected embedding output shape {emb_shape:?} (want {EMBEDDING_DIM} values)"
                ));
            }
            let mut emb = [0f32; EMBEDDING_DIM];
            emb.copy_from_slice(emb_data);
            emb
        };
        self.embeddings.push_back(emb);
        while self.embeddings.len() > EMBEDDING_WINDOW {
            self.embeddings.pop_front();
        }
        if self.embeddings.len() < EMBEDDING_WINDOW {
            return Ok(0.0);
        }

        // Stage 3: classifier head — input [1, 16, 96] -> [1, 1] sigmoid score.
        let mut head_in = Array3::<f32>::zeros((1, EMBEDDING_WINDOW, EMBEDDING_DIM));
        for (i, emb) in self.embeddings.iter().enumerate() {
            for (j, v) in emb.iter().enumerate() {
                head_in[[0, i, j]] = *v;
            }
        }
        let outputs = self.head.run(vec![(
            Cow::Owned(self.head_input.clone()),
            Value::from_array(head_in)?.into_dyn(),
        )])?;
        let (_, score_data) = outputs
            .get(self.head_output.as_str())
            .ok_or_else(|| anyhow!("wake-word head output missing"))?
            .try_extract_tensor::<f32>()?;
        Ok(score_data.first().copied().unwrap_or(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn models_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/models/wakeword")
    }

    fn make_detector_for(head: &str) -> WakeWordDetector {
        WakeWordDetector::new(
            &models_dir().join("melspectrogram.onnx"),
            &models_dir().join("embedding_model.onnx"),
            &models_dir().join(head),
            WakeWordConfig::default(),
        )
        .expect("bundled wake-word models should load")
    }

    fn make_detector() -> WakeWordDetector {
        make_detector_for("hey_jarvis_v0.1.onnx")
    }

    /// Feed a wav fixture (with leading/trailing silence for window warm-up
    /// and flush) and count detections.
    fn detections_for_fixture(det: &mut WakeWordDetector, fixture: &str) -> usize {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/audio_toolkit/wakeword/testdata")
            .join(fixture);
        let mut reader = hound::WavReader::open(wav_path).expect("fixture wav should open");
        assert_eq!(reader.spec().sample_rate, 16000);
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();

        let silence = vec![0f32; 16000];
        let mut detections = 0;
        for frame in silence
            .iter()
            .chain(samples.iter())
            .chain(silence.iter())
            .copied()
            .collect::<Vec<f32>>()
            .chunks(480)
        {
            if det.push_frame(frame).expect("inference should succeed") {
                detections += 1;
            }
        }
        detections
    }

    #[test]
    fn silence_never_triggers() {
        let mut det = make_detector();
        let frame = [0f32; 480];
        // ~6 s of silence: enough to fill every rolling window several times.
        for _ in 0..200 {
            let fired = det.push_frame(&frame).expect("inference should succeed");
            assert!(!fired, "silence must not trigger the wake word");
        }
    }

    #[test]
    fn tts_hey_jarvis_triggers_once() {
        let mut det = make_detector();
        assert_eq!(
            detections_for_fixture(&mut det, "hey_jarvis_tts.wav"),
            1,
            "synthetic 'hey jarvis' should trigger exactly once (refractory suppresses echoes)"
        );
    }

    #[test]
    fn jarvis_head_triggers_on_bare_jarvis_and_hey_jarvis() {
        // Community jarvis_v2 model is trained on both phrasings. Fresh
        // detectors per fixture: the 2 s post-trigger refractory would
        // otherwise suppress the second detection in a fast-running test.
        let mut det = make_detector_for("jarvis_v2.onnx");
        assert_eq!(
            detections_for_fixture(&mut det, "jarvis_tts.wav"),
            1,
            "synthetic bare 'jarvis' should trigger the jarvis_v2 head"
        );
        let mut det = make_detector_for("jarvis_v2.onnx");
        assert_eq!(
            detections_for_fixture(&mut det, "hey_jarvis_tts.wav"),
            1,
            "synthetic 'hey jarvis' should also trigger the jarvis_v2 head"
        );
    }

    #[test]
    fn jarvis_head_ignores_silence() {
        let mut det = make_detector_for("jarvis_v2.onnx");
        let frame = [0f32; 480];
        for _ in 0..200 {
            assert!(!det.push_frame(&frame).expect("inference should succeed"));
        }
    }

    #[test]
    fn rechunking_handles_odd_frame_sizes() {
        let mut det = make_detector();
        // 480-sample frames don't divide 1280 evenly; ensure the internal
        // buffer never desyncs or errors across many pushes.
        let frame = [0.01f32; 480];
        for _ in 0..50 {
            det.push_frame(&frame).expect("inference should succeed");
        }
        assert!(det.sample_buf.len() < WAKE_CHUNK_SAMPLES);
    }
}
