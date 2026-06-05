use std::collections::HashMap;
use std::path::Path;
use ndarray::{Array1, Array3};
use rustfft::{FftPlanner, num_complex::Complex};

pub struct MalayalamAsr {
    session: ort::session::Session,
    vocab: HashMap<usize, String>,
    blank_id: usize,
    features_size: usize,
    sample_rate: usize,
}

impl MalayalamAsr {
    pub fn load(model_dir: &Path) -> anyhow::Result<Self> {
        let config_path = model_dir.join("config.json");
        let mut sample_rate = 16000;
        let mut features_size = 80;
        
        if config_path.exists() {
            if let Ok(config_str) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(&config_str) {
                    if let Some(sr) = config.get("sample_rate").and_then(|v| v.as_u64()) {
                        sample_rate = sr as usize;
                    }
                    if let Some(fs) = config.get("features_size").and_then(|v| v.as_u64()) {
                        features_size = fs as usize;
                    }
                }
            }
        }
        
        let vocab_path = model_dir.join("vocab.txt");
        let vocab = load_vocab(&vocab_path)?;
        let blank_id = vocab.len();
        
        let model_path = model_dir.join("model.onnx");
        if !model_path.exists() {
            return Err(anyhow::anyhow!("ONNX model file not found at {:?}", model_path));
        }

        // Initialize ONNX Session with DirectML and CPU fallback
        let session = match ort::session::Session::builder() {
            Ok(builder) => {
                let mut builder = match builder.with_execution_providers([ort::ep::DirectML::default().build()]) {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("Failed to configure DirectML: {}, falling back to CPU", e);
                        ort::session::Session::builder()?
                    }
                };
                builder.commit_from_file(&model_path)?
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to create session builder: {}", e));
            }
        };

        Ok(Self {
            session,
            vocab,
            blank_id,
            features_size,
            sample_rate,
        })
    }

    pub fn transcribe(&mut self, audio: &[f32]) -> anyhow::Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        // 1. Pre-emphasis
        let signal_pe = pre_emphasis(audio);

        // 2. Mel Spectrogram calculation
        let n_fft = 512;
        let hop_length = 160;
        let win_length = 400;
        
        // Centered STFT padding by n_fft/2 (256)
        let padded = reflect_pad(&signal_pe, n_fft / 2);
        let num_frames = 1 + (padded.len() - n_fft) / hop_length;
        
        let window = hann_window(win_length);
        
        // Setup FFT planner
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n_fft);
        
        let pad_offset = (n_fft - win_length) / 2; // (512 - 400) / 2 = 56
        let n_bins = n_fft / 2 + 1;
        
        let mut spectrogram = Vec::with_capacity(num_frames);
        for t in 0..num_frames {
            let frame_start = t * hop_length;
            let mut buffer = vec![Complex { re: 0.0, im: 0.0 }; n_fft];
            
            // Extract centered windowed frame and place it in buffer
            for i in 0..win_length {
                let sample = padded[frame_start + pad_offset + i];
                buffer[pad_offset + i] = Complex {
                    re: sample * window[i],
                    im: 0.0,
                };
            }
            
            fft.process(&mut buffer);
            
            // Compute power spectrum (magnitude squared) for the first n_bins
            let mut frame_power = Vec::with_capacity(n_bins);
            for k in 0..n_bins {
                let mag_sq = buffer[k].re * buffer[k].re + buffer[k].im * buffer[k].im;
                frame_power.push(mag_sq);
            }
            spectrogram.push(frame_power);
        }

        // Construct Slaney-normalized mel filterbank
        let filterbank = mel_filterbank(
            self.sample_rate as f32,
            n_fft,
            self.features_size,
            0.0,
            8000.0,
        );

        // Project onto Mel frequency bands
        let mut mel_spec = vec![vec![0.0f32; num_frames]; self.features_size];
        for m in 0..self.features_size {
            for t in 0..num_frames {
                let mut sum = 0.0f32;
                for k in 0..n_bins {
                    sum += filterbank[m][k] * spectrogram[t][k];
                }
                // Log scale with 1e-9 floor
                mel_spec[m][t] = (sum + 1e-9).ln();
            }
        }

        // Per-feature (band) normalization across time (ddof=1)
        for m in 0..self.features_size {
            let mut sum = 0.0f32;
            for t in 0..num_frames {
                sum += mel_spec[m][t];
            }
            let mean = sum / num_frames as f32;
            
            let mut var_sum = 0.0f32;
            for t in 0..num_frames {
                let diff = mel_spec[m][t] - mean;
                var_sum += diff * diff;
            }
            let variance = if num_frames > 1 {
                var_sum / (num_frames - 1) as f32
            } else {
                0.0f32
            };
            let std = variance.sqrt();
            
            for t in 0..num_frames {
                mel_spec[m][t] = (mel_spec[m][t] - mean) / (std + 1e-9);
            }
        }

        // Shape inputs for ONNX Runtime: [1, 80, T]
        let mut input_array = Array3::<f32>::zeros((1, self.features_size, num_frames));
        for m in 0..self.features_size {
            for t in 0..num_frames {
                input_array[[0, m, t]] = mel_spec[m][t];
            }
        }

        let length_array = Array1::<i64>::from_vec(vec![num_frames as i64]);

        let input_value = ort::value::Value::from_array(input_array)?;
        let length_value = ort::value::Value::from_array(length_array)?;

        // Run model inference
        let outputs = self.session.run(ort::inputs![
            "audio_signal" => input_value,
            "length" => length_value,
        ])?;

        let output_value = outputs
            .values()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No outputs returned from the session"))?;
            
        let logits_view = output_value.try_extract_array::<f32>()?;
        let shape = logits_view.shape();
        if shape.len() != 3 {
            return Err(anyhow::anyhow!("Expected 3D logits shape, found {:?}", shape));
        }

        let dim1 = shape[1];
        let dim2 = shape[2];
        let vocab_size = self.vocab.len();

        let mut transpose = false;
        if dim1 == vocab_size || dim1 == vocab_size + 1 {
            transpose = true;
        } else if dim2 == vocab_size || dim2 == vocab_size + 1 {
            transpose = false;
        } else if dim1 != dim2 {
            transpose = true;
        }

        let t_len = if transpose { dim2 } else { dim1 };
        let v_len = if transpose { dim1 } else { dim2 };

        let mut logits_2d = vec![vec![0.0f32; v_len]; t_len];
        if transpose {
            for t in 0..t_len {
                for v in 0..v_len {
                    logits_2d[t][v] = *logits_view
                        .get([0, v, t])
                        .ok_or_else(|| anyhow::anyhow!("Logits index out of bounds"))?;
                }
            }
        } else {
            for t in 0..t_len {
                for v in 0..v_len {
                    logits_2d[t][v] = *logits_view
                        .get([0, t, v])
                        .ok_or_else(|| anyhow::anyhow!("Logits index out of bounds"))?;
                }
            }
        }

        // CTC Argmax Decoding
        let mut ids = Vec::with_capacity(t_len);
        for t in 0..t_len {
            let mut max_val = f32::NEG_INFINITY;
            let mut max_idx = 0;
            for v in 0..v_len {
                let val = logits_2d[t][v];
                if val > max_val {
                    max_val = val;
                    max_idx = v;
                }
            }
            ids.push(max_idx);
        }

        let mut decoded_tokens = Vec::new();
        let mut prev = -1i32;
        for idx in ids {
            let idx_i32 = idx as i32;
            if idx_i32 != prev {
                if idx < self.vocab.len() && idx != self.blank_id {
                    if let Some(token) = self.vocab.get(&idx) {
                        decoded_tokens.push(token.clone());
                    }
                }
            }
            prev = idx_i32;
        }

        let joined = decoded_tokens.join("");
        let replaced = joined.replace('\u{2581}', " ").replace('▁', " ");
        let normalized = replaced.split_whitespace().collect::<Vec<&str>>().join(" ");

        Ok(normalized)
    }
}

fn load_vocab(vocab_path: &Path) -> anyhow::Result<HashMap<usize, String>> {
    let file = std::fs::File::open(vocab_path)?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;

    let mut vocab = HashMap::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim_end_matches('\r').trim_end_matches('\n');
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 2 && parts[1].chars().all(|c| c.is_ascii_digit()) {
            let token = parts[0].to_string();
            let index = parts[1].parse::<usize>()?;
            vocab.insert(index, token);
        } else if parts.len() >= 2 && parts[parts.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            let index = parts[parts.len() - 1].parse::<usize>()?;
            let token = parts[..parts.len() - 1].join(" ");
            vocab.insert(index, token);
        } else {
            vocab.insert(idx, trimmed.to_string());
        }
    }
    Ok(vocab)
}

fn pre_emphasis(signal: &[f32]) -> Vec<f32> {
    if signal.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(signal.len());
    out.push(signal[0]);
    for i in 1..signal.len() {
        out.push(signal[i] - 0.97 * signal[i - 1]);
    }
    out
}

fn reflect_pad(x: &[f32], p: usize) -> Vec<f32> {
    let l = x.len();
    let mut padded = vec![0.0; l + 2 * p];
    padded[p..(p + l)].copy_from_slice(x);
    for i in 0..p {
        padded[p - 1 - i] = x[i + 1];
    }
    for i in 0..p {
        padded[p + l + i] = x[l - 2 - i];
    }
    padded
}

fn hann_window(n: usize) -> Vec<f32> {
    let mut w = Vec::with_capacity(n);
    use std::f32::consts::PI;
    for i in 0..n {
        let val = 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos());
        w.push(val);
    }
    w
}

fn hz_to_mel(f: f32) -> f32 {
    let f_min = 0.0f32;
    let f_sp = 200.0f32 / 3.0f32;
    let min_log_hz = 1000.0f32;
    let min_log_mel = (min_log_hz - f_min) / f_sp;
    let logstep = 6.4f32.ln() / 27.0f32;

    if f < min_log_hz {
        (f - f_min) / f_sp
    } else {
        min_log_mel + (f / min_log_hz).ln() / logstep
    }
}

fn mel_to_hz(mel: f32) -> f32 {
    let f_min = 0.0f32;
    let f_sp = 200.0f32 / 3.0f32;
    let min_log_hz = 1000.0f32;
    let min_log_mel = (min_log_hz - f_min) / f_sp;
    let logstep = 6.4f32.ln() / 27.0f32;

    if mel < min_log_mel {
        f_min + f_sp * mel
    } else {
        min_log_hz * (logstep * (mel - min_log_mel)).exp()
    }
}

fn mel_frequencies(n_mels: usize, fmin: f32, fmax: f32) -> Vec<f32> {
    let min_mel = hz_to_mel(fmin);
    let max_mel = hz_to_mel(fmax);

    let mut mels = Vec::with_capacity(n_mels);
    for i in 0..n_mels {
        let fraction = i as f32 / (n_mels - 1) as f32;
        let mel = min_mel + fraction * (max_mel - min_mel);
        mels.push(mel_to_hz(mel));
    }
    mels
}

fn fft_frequencies(sr: f32, n_fft: usize) -> Vec<f32> {
    let n_bins = n_fft / 2 + 1;
    let mut freqs = Vec::with_capacity(n_bins);
    for k in 0..n_bins {
        freqs.push(k as f32 * sr / n_fft as f32);
    }
    freqs
}

fn mel_filterbank(sr: f32, n_fft: usize, n_mels: usize, fmin: f32, fmax: f32) -> Vec<Vec<f32>> {
    let mel_f = mel_frequencies(n_mels + 2, fmin, fmax);
    let fftfreqs = fft_frequencies(sr, n_fft);
    let n_bins = n_fft / 2 + 1;

    let mut fdiff = Vec::with_capacity(n_mels + 1);
    for i in 0..=n_mels {
        fdiff.push(mel_f[i + 1] - mel_f[i]);
    }

    let mut weights = vec![vec![0.0f32; n_bins]; n_mels];

    for i in 0..n_mels {
        let lower_center = mel_f[i];
        let _center = mel_f[i + 1];
        let upper_center = mel_f[i + 2];

        let fdiff_lower = fdiff[i];
        let fdiff_upper = fdiff[i + 1];

        for j in 0..n_bins {
            let f = fftfreqs[j];

            let lower = (f - lower_center) / fdiff_lower;
            let upper = (upper_center - f) / fdiff_upper;

            let w = 0.0f32.max(lower.min(upper));
            weights[i][j] = w;
        }
    }

    // Apply Slaney area normalization
    for i in 0..n_mels {
        let enorm = 2.0 / (mel_f[i + 2] - mel_f[i]);
        for j in 0..n_bins {
            weights[i][j] *= enorm;
        }
    }

    weights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_vocab_two_column_format() {
        let temp_dir = tempfile::tempdir().unwrap();
        let vocab_path = temp_dir.path().join("vocab.txt");
        let vocab_content = "token_a 0\ntoken_b 1\ntoken_c 2\n";
        std::fs::write(&vocab_path, vocab_content).unwrap();

        let vocab = load_vocab(&vocab_path).unwrap();
        assert_eq!(vocab.len(), 3);
        assert_eq!(vocab.get(&0).unwrap(), "token_a");
        assert_eq!(vocab.get(&1).unwrap(), "token_b");
        assert_eq!(vocab.get(&2).unwrap(), "token_c");
    }

    #[test]
    fn test_ctc_decode_dedup_and_blank_removal() {
        // We'll verify the helper parts
        let vocab: HashMap<usize, String> = [
            (0, "a".to_string()),
            (1, "b".to_string()),
            (2, "c".to_string()),
        ]
        .into_iter()
        .collect();
        let blank_id = 3;

        // Simulate argmax output IDs: [0, 0, 3, 1, 1, 2, 3] -> "abc"
        let ids = vec![0, 0, 3, 1, 1, 2, 3];
        let mut decoded_tokens = Vec::new();
        let mut prev = -1i32;
        for idx in ids {
            let idx_i32 = idx as i32;
            if idx_i32 != prev {
                if idx < vocab.len() && idx != blank_id {
                    if let Some(token) = vocab.get(&idx) {
                        decoded_tokens.push(token.clone());
                    }
                }
            }
            prev = idx_i32;
        }
        let joined = decoded_tokens.join("");
        assert_eq!(joined, "abc");
    }

    #[test]
    fn test_mel_tensor_shape() {
        // 1 second of silence at 16000Hz = 16000 samples
        let silence = vec![0.0f32; 16000];
        let signal_pe = pre_emphasis(&silence);
        
        let n_fft = 512;
        let hop_length = 160;
        
        let padded = reflect_pad(&signal_pe, n_fft / 2);
        let num_frames = 1 + (padded.len() - n_fft) / hop_length;
        
        // Assert that 16000 samples yields exactly 101 frames (which matches [1, 80, 101] shape)
        assert_eq!(num_frames, 101);
    }

    #[test]
    fn test_inference_on_audio_wav() {
        let wav_path = Path::new("d:\\Downloads\\Projects\\Asr malayalam\\audio_16k.wav");
        if !wav_path.exists() {
            println!("Skipping integration test: audio.wav not found at {:?}", wav_path);
            return;
        }

        let model_dir = Path::new("d:\\Downloads\\Projects\\Asr malayalam\\model");
        if !model_dir.exists() {
            println!("Skipping integration test: model dir not found at {:?}", model_dir);
            return;
        }

        let mut asr = MalayalamAsr::load(model_dir).unwrap();
        
        // Load WAV file using hound
        let mut reader = hound::WavReader::open(wav_path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000);
        assert_eq!(spec.channels, 1);
        
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => {
                let max_val = 2.0f32.powi(spec.bits_per_sample as i32 - 1);
                reader.samples::<i32>().map(|s| s.unwrap() as f32 / max_val).collect()
            }
        };

        let transcript = asr.transcribe(&samples).unwrap();
        println!("Integration Test Transcription Result:\n{}", transcript);
        assert!(!transcript.is_empty());
    }
}
