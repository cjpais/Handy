use anyhow::{Context, Result};
use log::{debug, info};
use ndarray::Array2;
use ort::session::Session;
use ort::value::TensorRef;
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// GigaAM v3 e2e_ctc mel spectrogram parameters (16 kHz audio)
const N_FFT: usize = 320;
const HOP_LENGTH: usize = 160;
const WIN_LENGTH: usize = 320;
const N_MELS: usize = 64;
const F_MIN: f32 = 0.0;
const F_MAX: f32 = 8000.0; // Nyquist for 16 kHz

/// GigaAM v3 e2e_ctc BPE vocabulary (257 tokens: 0–255 subwords + 256 blank).
///
/// Includes Russian subwords, punctuation, Latin characters, digits,
/// and currency symbols. The `▁` prefix denotes a word boundary (space).
const VOCAB: &[&str] = &[
    "<unk>",    // 0
    "▁",        // 1: word boundary / space
    ".",        // 2
    "е",        // 3
    "а",        // 4
    "с",        // 5
    "и",        // 6
    ",",        // 7
    "о",        // 8
    "т",        // 9
    "н",        // 10
    "м",        // 11
    "у",        // 12
    "й",        // 13
    "л",        // 14
    "я",        // 15
    "в",        // 16
    "д",        // 17
    "з",        // 18
    "к",        // 19
    "но",       // 20
    "▁с",       // 21
    "ы",        // 22
    "г",        // 23
    "▁в",       // 24
    "б",        // 25
    "р",        // 26
    "п",        // 27
    "то",       // 28
    "ть",       // 29
    "ра",       // 30
    "▁по",      // 31
    "ка",       // 32
    "ш",        // 33
    "ни",       // 34
    "ли",       // 35
    "на",       // 36
    "го",       // 37
    "х",        // 38
    "ро",       // 39
    "ва",       // 40
    "▁на",      // 41
    "ю",        // 42
    "ко",       // 43
    "ль",       // 44
    "те",       // 45
    "?",        // 46
    "ч",        // 47
    "ж",        // 48
    "во",       // 49
    "ла",       // 50
    "ре",       // 51
    "да",       // 52
    "▁и",       // 53
    "ло",       // 54
    "ст",       // 55
    "-",        // 56
    "ё",        // 57
    "▁не",      // 58
    "ле",       // 59
    "ри",       // 60
    "де",       // 61
    "та",       // 62
    "ны",       // 63
    "▁В",       // 64
    "▁С",       // 65
    "ь",        // 66
    "ки",       // 67
    "ер",       // 68
    "▁о",       // 69
    "ви",       // 70
    "ти",       // 71
    "ма",       // 72
    "▁за",      // 73
    "▁А",       // 74
    "▁Т",       // 75
    "▁у",       // 76
    "же",       // 77
    "э",        // 78
    "▁М",       // 79
    "ц",        // 80
    "ди",       // 81
    "не",       // 82
    "ру",       // 83
    "че",       // 84
    "ф",        // 85
    "ве",       // 86
    "▁Д",       // 87
    "бо",       // 88
    "▁К",       // 89
    "щ",        // 90
    "▁О",       // 91
    "ми",       // 92
    "▁что",     // 93
    "▁«",       // 94
    "»",        // 95
    "ся",       // 96
    "▁По",      // 97
    "▁про",     // 98
    "e",        // 99
    "a",        // 100
    "ку",       // 101
    "ну",       // 102
    "▁это",     // 103
    "мо",       // 104
    "жи",       // 105
    "▁ко",      // 106
    "▁П",       // 107
    "▁И",       // 108
    "ча",       // 109
    "му",       // 110
    "0",        // 111
    "ты",       // 112
    "ста",      // 113
    "сь",       // 114
    "▁как",     // 115
    "o",        // 116
    "▁мо",      // 117
    "i",        // 118
    "до",       // 119
    "ля",       // 120
    "▁до",      // 121
    "▁от",      // 122
    "У",        // 123
    "Б",        // 124
    "ры",       // 125
    "чи",       // 126
    "ци",       // 127
    "▁бы",      // 128
    "▁Включи",  // 129
    "па",       // 130
    "ключ",     // 131
    "по",       // 132
    "ду",       // 133
    "▁при",     // 134
    "\u{2014}", // 135: em dash —
    "Л",        // 136
    "n",        // 137
    "Р",        // 138
    "сто",      // 139
    "r",        // 140
    "▁так",     // 141
    "сти",      // 142
    "Г",        // 143
    "▁На",      // 144
    "Н",        // 145
    "▁об",      // 146
    "▁мне",     // 147
    "l",        // 148
    "Я",        // 149
    "t",        // 150
    "1",        // 151
    "▁За",      // 152
    "s",        // 153
    "Э",        // 154
    "Ч",        // 155
    "Е",        // 156
    "▁есть",    // 157
    "ень",      // 158
    "▁Ну",      // 159
    "2",        // 160
    "▁Сбер",    // 161
    "вер",      // 162
    "▁вот",     // 163
    "ение",     // 164
    "смотр",    // 165
    "В",        // 166
    "▁раз",     // 167
    "Ф",        // 168
    "▁пере",    // 169
    "ешь",      // 170
    "▁тебя",    // 171
    "u",        // 172
    "3",        // 173
    "5",        // 174
    "d",        // 175
    "y",        // 176
    "Х",        // 177
    "4",        // 178
    "З",        // 179
    "S",        // 180
    "С",        // 181
    "h",        // 182
    "c",        // 183
    "m",        // 184
    "9",        // 185
    ":",        // 186
    "8",        // 187
    "6",        // 188
    "7",        // 189
    "M",        // 190
    "B",        // 191
    "П",        // 192
    "D",        // 193
    "T",        // 194
    "!",        // 195
    "k",        // 196
    "g",        // 197
    "О",        // 198
    "C",        // 199
    "Ш",        // 200
    "М",        // 201
    "A",        // 202
    "p",        // 203
    "Ю",        // 204
    "P",        // 205
    "Т",        // 206
    "К",        // 207
    "А",        // 208
    "L",        // 209
    "b",        // 210
    "Д",        // 211
    "ъ",        // 212
    "H",        // 213
    "%",        // 214
    "F",        // 215
    "v",        // 216
    "V",        // 217
    "R",        // 218
    "O",        // 219
    "I",        // 220
    "И",        // 221
    "N",        // 222
    "Ж",        // 223
    "\"",       // 224
    "K",        // 225
    "G",        // 226
    "Ц",        // 227
    "f",        // 228
    "w",        // 229
    "E",        // 230
    "₽",        // 231
    "W",        // 232
    "J",        // 233
    "x",        // 234
    "z",        // 235
    "'",        // 236
    "U",        // 237
    "Y",        // 238
    "&",        // 239
    "Z",        // 240
    "X",        // 241
    "+",        // 242
    "/",        // 243
    "Щ",        // 244
    ";",        // 245
    "j",        // 246
    "Й",        // 247
    "q",        // 248
    "Q",        // 249
    "°",        // 250
    "Ё",        // 251
    "Ы",        // 252
    "€",        // 253
    "$",        // 254
    "«",        // 255
];
const BLANK_ID: usize = 256; // <blk> token index

/// ONNX-based engine for GigaAM v3 e2e_ctc Russian speech recognition.
///
/// Implements mel spectrogram feature extraction and CTC greedy decoding
/// for the GigaAM v3 end-to-end model from salute-developers/GigaAM.
/// Supports punctuation, Latin characters, and digits.
///
/// Note: This engine does not implement the `TranscriptionEngine` trait from
/// `transcribe-rs` because that trait is defined in an external crate. Instead,
/// it provides a compatible API and is integrated via `LoadedEngine` enum in
/// `transcription.rs`.
pub struct GigaAMEngine {
    session: Option<Session>,
    loaded_model_path: Option<PathBuf>,
    mel_filterbank: Array2<f32>,
    hann_window: Vec<f32>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl Default for GigaAMEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GigaAMEngine {
    fn drop(&mut self) {
        self.unload_model();
    }
}

impl GigaAMEngine {
    /// Create a new GigaAM engine with precomputed mel filterbank, window, and FFT plan.
    pub fn new() -> Self {
        let window: Vec<f32> = (0..WIN_LENGTH)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / WIN_LENGTH as f32).cos()))
            .collect();
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(N_FFT);

        Self {
            session: None,
            loaded_model_path: None,
            mel_filterbank: compute_mel_filterbank(N_MELS, N_FFT, 16000, F_MIN, F_MAX),
            hann_window: window,
            fft,
        }
    }

    /// Load a GigaAM ONNX model from the given path.
    pub fn load_model(&mut self, model_path: &Path) -> Result<()> {
        info!("Loading GigaAM model from {:?}", model_path);
        let session = Session::builder()
            .context("Failed to create ONNX session builder")?
            .commit_from_file(model_path)
            .context("Failed to load GigaAM ONNX model")?;
        self.session = Some(session);
        self.loaded_model_path = Some(model_path.to_path_buf());
        info!("GigaAM model loaded successfully");
        Ok(())
    }

    /// Unload the current model, freeing memory.
    pub fn unload_model(&mut self) {
        self.session = None;
        self.loaded_model_path = None;
        debug!("GigaAM model unloaded");
    }

    /// Transcribe audio samples (16 kHz mono f32) to text.
    pub fn transcribe(&mut self, audio: Vec<f32>) -> Result<String> {
        if audio.len() < N_FFT {
            return Ok(String::new());
        }

        // Compute mel spectrogram before borrowing session mutably
        let mel = self.compute_mel_spectrogram(&audio);
        let time_steps = mel.shape()[1];

        debug!(
            "Mel spectrogram shape: [{}, {}]",
            mel.shape()[0],
            mel.shape()[1]
        );

        // Prepare input tensors: features [1, n_mels, time], feature_lengths [1]
        let features = mel.insert_axis(ndarray::Axis(0)); // [1, 64, T]
        let features_dyn = features.into_dyn();
        let feature_lengths = ndarray::arr1(&[time_steps as i64]).into_dyn();

        // Run ONNX inference
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("GigaAM model not loaded"))?;
        let inputs = ort::inputs! {
            "features" => TensorRef::from_array_view(features_dyn.view())?,
            "feature_lengths" => TensorRef::from_array_view(feature_lengths.view())?,
        };
        let outputs = session.run(inputs)?;

        // Extract log_probs output [1, T', vocab_size]
        let log_probs = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract log_probs tensor")?;
        let log_probs = log_probs.to_owned().into_dimensionality::<ndarray::Ix3>()?;
        debug!("Log probs shape: {:?}", log_probs.shape());

        // CTC greedy decode
        let text = ctc_greedy_decode(&log_probs);
        Ok(text)
    }

    /// Compute log-mel spectrogram from raw audio samples.
    ///
    /// Uses Hanning window, no center padding, and HTK mel filterbank
    /// matching the GigaAM v3 preprocessing pipeline.
    fn compute_mel_spectrogram(&self, audio: &[f32]) -> Array2<f32> {
        let n_frames = (audio.len() - N_FFT) / HOP_LENGTH + 1;
        let freq_bins = N_FFT / 2 + 1;

        // Compute STFT power spectrogram
        let mut power_spec = Array2::<f32>::zeros((freq_bins, n_frames));

        for frame_idx in 0..n_frames {
            let start = frame_idx * HOP_LENGTH;
            let mut fft_buf: Vec<Complex<f32>> = (0..N_FFT)
                .map(|i| Complex::new(audio[start + i] * self.hann_window[i], 0.0))
                .collect();

            self.fft.process(&mut fft_buf);

            for (bin, val) in fft_buf.iter().enumerate().take(freq_bins) {
                power_spec[[bin, frame_idx]] = val.norm_sqr();
            }
        }

        // Apply mel filterbank: mel = filterbank @ power_spec → [n_mels, n_frames]
        let mel = self.mel_filterbank.dot(&power_spec);

        // Log scaling: clamp then ln (GigaAM SpecScaler)
        mel.mapv(|v| v.clamp(1e-9, 1e9).ln())
    }
}

/// CTC greedy decoding: argmax → collapse consecutive → remove blanks → map BPE tokens to text.
fn ctc_greedy_decode(log_probs: &ndarray::Array3<f32>) -> String {
    let time_steps = log_probs.shape()[1];
    let vocab_size = log_probs.shape()[2];

    let mut token_ids: Vec<usize> = Vec::with_capacity(time_steps);

    for t in 0..time_steps {
        let mut best_id = 0;
        let mut best_val = f32::NEG_INFINITY;
        for v in 0..vocab_size {
            let val = log_probs[[0, t, v]];
            if val > best_val {
                best_val = val;
                best_id = v;
            }
        }
        token_ids.push(best_id);
    }

    // Collapse consecutive duplicates and remove blanks
    let mut result = String::new();
    let mut prev_id: Option<usize> = None;

    for &id in &token_ids {
        if Some(id) == prev_id {
            prev_id = Some(id);
            continue;
        }
        prev_id = Some(id);

        if id == BLANK_ID || id >= VOCAB.len() {
            continue;
        }

        let token = VOCAB[id];

        // Skip <unk> tokens
        if token == "<unk>" {
            continue;
        }

        // SentencePiece ▁ prefix denotes word boundary (space)
        if let Some(stripped) = token.strip_prefix('▁') {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(stripped);
        } else {
            result.push_str(token);
        }
    }

    result.trim().to_string()
}

/// Compute mel filterbank matrix [n_mels, n_fft/2+1] using HTK formula.
fn compute_mel_filterbank(
    n_mels: usize,
    n_fft: usize,
    sample_rate: u32,
    f_min: f32,
    f_max: f32,
) -> Array2<f32> {
    let n_freqs = n_fft / 2 + 1;

    let hz_to_mel = |f: f32| -> f32 { 2595.0 * (1.0 + f / 700.0).log10() };
    let mel_to_hz = |m: f32| -> f32 { 700.0 * (10.0f32.powf(m / 2595.0) - 1.0) };

    let mel_min = hz_to_mel(f_min);
    let mel_max = hz_to_mel(f_max);

    // n_mels + 2 equally spaced points in mel scale
    let mel_points: Vec<f32> = (0..=n_mels + 1)
        .map(|i| mel_min + (mel_max - mel_min) * i as f32 / (n_mels + 1) as f32)
        .collect();

    let hz_points: Vec<f32> = mel_points.iter().map(|&m| mel_to_hz(m)).collect();

    // Convert Hz to FFT bin indices
    let bin_points: Vec<f32> = hz_points
        .iter()
        .map(|&f| f * n_fft as f32 / sample_rate as f32)
        .collect();

    let mut filterbank = Array2::<f32>::zeros((n_mels, n_freqs));

    for m in 0..n_mels {
        let f_left = bin_points[m];
        let f_center = bin_points[m + 1];
        let f_right = bin_points[m + 2];

        for k in 0..n_freqs {
            let freq = k as f32;
            if freq >= f_left && freq <= f_center {
                let denom = f_center - f_left;
                if denom > 0.0 {
                    filterbank[[m, k]] = (freq - f_left) / denom;
                }
            } else if freq > f_center && freq <= f_right {
                let denom = f_right - f_center;
                if denom > 0.0 {
                    filterbank[[m, k]] = (f_right - freq) / denom;
                }
            }
        }
    }

    filterbank
}
