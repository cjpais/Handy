//! ASR Benchmarking CLI — MalayalamAsr + Whisper Turbo/Large
//!
//! Usage:
//!   cargo run --bin benchmark -- --model <malayalam|whisper-turbo|whisper-large> \
//!     [--model-dir <path>] [--whisper-model-path <path>] \
//!     --benchmark-dir <path> --transcripts <path> [--use-gpu] [--language ml]

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use hound::WavReader;
use rubato::{FftFixedIn, Resampler};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use transcribe_rs::whisper_cpp::{WhisperEngine, WhisperInferenceParams, WhisperLoadParams};

use handy_app_lib::malayalam_asr::MalayalamAsr;

// ─── Model selector ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, ValueEnum)]
enum ModelChoice {
    Malayalam,
    WhisperTurbo,
    WhisperLarge,
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "benchmark", about = "Benchmark ASR models on the Malayalam dataset")]
struct Cli {
    /// Which model to benchmark
    #[arg(long)]
    model: ModelChoice,

    /// Path to the MalayalamAsr model directory (model.onnx, vocab.txt, config.json)
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// Explicit path to a Whisper .bin file (auto-detected from Handy app data if omitted)
    #[arg(long)]
    whisper_model_path: Option<PathBuf>,

    /// Language hint passed to Whisper (e.g. "ml", "en", "auto").
    /// Defaults to "auto" (Whisper auto-detect) — passing "ml" can cause
    /// repetition loops on Malayalam Unicode output with some whisper.cpp builds.
    #[arg(long, default_value = "auto")]
    language: String,

    /// Benchmark directory containing single_speaker/ and multi_speaker/
    #[arg(long)]
    benchmark_dir: PathBuf,

    /// Path to transcripts.json
    #[arg(long)]
    transcripts: PathBuf,

    /// Use GPU (CUDA/Vulkan) for inference
    #[arg(long, default_value_t = false)]
    use_gpu: bool,

    /// Number of CPU threads for Whisper decoding (0 = whisper default = min(4, cores))
    #[arg(long, default_value_t = 0)]
    n_threads: i32,

    /// Limit inference to the first N clips (0 = no limit; useful for quick CPU tests)
    #[arg(long, default_value_t = 0)]
    max_clips: usize,
}

// ─── Unified engine abstraction ───────────────────────────────────────────────

enum BenchmarkEngine {
    Malayalam(MalayalamAsr),
    Whisper { engine: WhisperEngine, language: Option<String>, n_threads: i32 },
}

impl BenchmarkEngine {
    fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        match self {
            BenchmarkEngine::Malayalam(asr) => asr.transcribe(samples),
            BenchmarkEngine::Whisper { engine, language, n_threads } => {
                let params = WhisperInferenceParams {
                    language: language.clone(),
                    translate: false,
                    print_special: false,
                    print_progress: false,
                    print_realtime: false,
                    print_timestamps: false,
                    suppress_blank: true,
                    suppress_non_speech_tokens: true,
                    no_speech_thold: 0.2,
                    n_threads: *n_threads,
                    initial_prompt: None,
                };
                let result = engine
                    .transcribe_with(samples, &params)
                    .map_err(|e| anyhow::anyhow!("Whisper transcription failed: {}", e))?;
                Ok(result.text)
            }
        }
    }
}

// ─── Transcript DB ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TranscriptEntry {
    clean_text: String,
    is_multi_speaker: bool,
    is_code_switched: bool,
}

type TranscriptDb = HashMap<String, TranscriptEntry>;

// ─── Per-clip result ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ClipResult {
    stem: String,
    is_multi_speaker: bool,
    is_code_switched: bool,
    /// "clean" | "noisy" | "heavy_noisy"
    noise_label: String,
    audio_secs: f64,
    latency_secs: f64,
    wer: f64,
    cer: f64,
    subs: usize,
    ins: usize,
    dels: usize,
    peak_rss_mb: f64,
    cpu_pct: f32,
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load transcripts
    let transcript_str = std::fs::read_to_string(&cli.transcripts)
        .context("Failed to read transcripts.json")?;
    let db: TranscriptDb =
        serde_json::from_str(&transcript_str).context("Failed to parse transcripts.json")?;

    // Resolve language hint (None = Whisper auto-detect)
    let whisper_lang: Option<String> = match cli.language.as_str() {
        "auto" => None,
        l => Some(l.to_string()),
    };

    // Load model
    let device_label = if cli.use_gpu { "GPU" } else { "CPU" };
    let mut engine = load_engine(&cli, whisper_lang)?;
    let model_label = format!("{} — {}", model_display_name(&cli.model), device_label);
    eprintln!("[benchmark] Model loaded: {}", model_label);

    // Discover WAV files
    let mut wavs = collect_wavs(&cli.benchmark_dir)?;
    if cli.max_clips > 0 {
        // Keep only clean files up to max_clips, plus their noisy counterparts
        let clean: Vec<PathBuf> = wavs.iter()
            .filter(|p| !p.to_string_lossy().to_lowercase().contains("noisy"))
            .take(cli.max_clips)
            .cloned().collect();
        let clean_stems: std::collections::HashSet<String> =
            clean.iter().map(|p| stem_of(p)).collect();
        wavs.retain(|p| {
            let s = stem_of(p);
            clean_stems.contains(&s)
        });
        eprintln!("[benchmark] max_clips={} → {} WAV files after filtering.", cli.max_clips, wavs.len());
    } else {
        eprintln!("[benchmark] Found {} WAV files.", wavs.len());
    }

    // sysinfo
    let pid = Pid::from_u32(std::process::id());
    let refresh_kind = RefreshKind::nothing().with_processes(ProcessRefreshKind::everything());
    let mut sys = System::new_with_specifics(refresh_kind);

    let mut results: Vec<ClipResult> = Vec::new();

    // ── Pass 1: clean + existing noisy ────────────────────────────────────────
    for wav_path in &wavs {
        let stem = stem_of(wav_path);
        let entry = match db.get(&stem) {
            Some(e) => e,
            None => {
                eprintln!("[benchmark] WARNING: no transcript for '{}', skipping.", stem);
                continue;
            }
        };
        let path_lc = wav_path.to_string_lossy().to_lowercase();
        let noise_label = if path_lc.contains("noisy") { "noisy" } else { "clean" }.to_string();
        let (samples, audio_secs) = load_wav_16k(wav_path)?;

        match run_clip(
            &mut engine, &mut sys, pid, refresh_kind,
            &stem, entry.is_multi_speaker, entry.is_code_switched,
            noise_label, &samples, audio_secs, &entry.clean_text,
        ) {
            Ok(r)  => results.push(r),
            Err(e) => eprintln!("[benchmark] WARNING: clip '{}' failed: {}", stem, e),
        }
    }

    // ── Pass 2: heavy noise (σ=0.20) generated in-memory from clean ───────────
    eprintln!("[benchmark] Generating heavy-noisy variants (σ=0.20) …");
    let clean_wavs: Vec<&PathBuf> = wavs.iter()
        .filter(|p| !p.to_string_lossy().to_lowercase().contains("noisy"))
        .collect();

    for wav_path in &clean_wavs {
        let stem = stem_of(wav_path);
        let entry = match db.get(&stem) { Some(e) => e, None => continue };
        let (samples, audio_secs) = load_wav_16k(wav_path)?;
        let heavy = apply_gaussian_noise(&samples, 0.20);

        match run_clip(
            &mut engine, &mut sys, pid, refresh_kind,
            &stem, entry.is_multi_speaker, entry.is_code_switched,
            "heavy_noisy".to_string(), &heavy, audio_secs, &entry.clean_text,
        ) {
            Ok(r)  => results.push(r),
            Err(e) => eprintln!("[benchmark] WARNING: heavy-noisy clip '{}' failed: {}", stem, e),
        }
    }

    // ── Pass 3: combined long clips ───────────────────────────────────────────
    eprintln!("[benchmark] Running combined long-clip tests …");
    let combined_groups: &[(&str, bool, &str)] = &[
        ("Combined: Mal-only Clean",  false, "clean"),
        ("Combined: CS Clean",        true,  "clean"),
        ("Combined: Mal-only Noisy",  false, "noisy"),
        ("Combined: CS Noisy",        true,  "noisy"),
        ("Combined: Mal-only Heavy",  false, "heavy_noisy"),
        ("Combined: CS Heavy",        true,  "heavy_noisy"),
    ];

    let mut combined_results: Vec<CombinedResult> = Vec::new();

    for &(label, is_cs, noise) in combined_groups {
        let matching: Vec<&ClipResult> = results.iter()
            .filter(|r| !r.is_multi_speaker && r.is_code_switched == is_cs && r.noise_label == noise)
            .collect();
        if matching.is_empty() { continue; }

        let mut combined_samples: Vec<f32> = Vec::new();
        let mut combined_refs: Vec<String> = Vec::new();
        let mut total_audio = 0.0f64;

        for clip in &matching {
            let clean_wav = wavs.iter().find(|p| {
                stem_of(p) == clip.stem && !p.to_string_lossy().to_lowercase().contains("noisy")
            });
            if let Some(cw) = clean_wav {
                if let Ok((mut samps, dur)) = load_wav_16k(cw) {
                    match noise {
                        "noisy" => {
                            if let Some(nw) = wavs.iter().find(|p| {
                                stem_of(p) == clip.stem
                                    && p.to_string_lossy().to_lowercase().contains("noisy")
                            }) {
                                if let Ok((ns, nd)) = load_wav_16k(nw) { samps = ns; total_audio += nd; }
                            } else { total_audio += dur; }
                        }
                        "heavy_noisy" => { samps = apply_gaussian_noise(&samps, 0.20); total_audio += dur; }
                        _ => { total_audio += dur; }
                    }
                    combined_samples.extend_from_slice(&samps);
                    if let Some(e) = db.get(&clip.stem) {
                        combined_refs.push(normalize_text(&e.clean_text));
                    }
                }
            }
        }

        let combined_ref = combined_refs.join(" ");
        sys.refresh_specifics(refresh_kind);
        let rss_before = sys.process(pid).map(|p| p.memory()).unwrap_or(0);

        let t0 = Instant::now();
        let hypothesis = match engine.transcribe(&combined_samples) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[benchmark] WARNING: combined clip '{}' failed: {}", label, e);
                continue;
            }
        };
        let latency = t0.elapsed().as_secs_f64();

        sys.refresh_specifics(refresh_kind);
        let rss_after = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let peak_rss_mb = rss_after.max(rss_before) as f64 / 1024.0 / 1024.0;

        let hyp_n = normalize_text(&hypothesis);
        let ref_words: Vec<&str> = combined_ref.split_whitespace().collect();
        let hyp_words: Vec<&str> = hyp_n.split_whitespace().collect();
        let (s, i, d) = edit_ops(&ref_words, &hyp_words);
        let wer = if ref_words.is_empty() { 0.0 } else { (s + i + d) as f64 / ref_words.len() as f64 };

        let ref_chars = char_tokens(&combined_ref);
        let hyp_chars = char_tokens(&hyp_n);
        let rcr: Vec<&str> = ref_chars.iter().map(|s| s.as_str()).collect();
        let hcr: Vec<&str> = hyp_chars.iter().map(|s| s.as_str()).collect();
        let (cs2, ci2, cd2) = edit_ops(&rcr, &hcr);
        let cer = if ref_chars.is_empty() { 0.0 } else { (cs2 + ci2 + cd2) as f64 / ref_chars.len() as f64 };

        combined_results.push(CombinedResult {
            label: label.to_string(),
            clip_count: matching.len(),
            audio_secs: total_audio,
            latency_secs: latency,
            wer, cer,
            subs: s, ins: i, dels: d,
            peak_rss_mb,
        });
    }

    print_report(&results, &combined_results, &model_label);
    Ok(())
}

// ─── Engine loading ───────────────────────────────────────────────────────────

fn load_engine(cli: &Cli, whisper_lang: Option<String>) -> Result<BenchmarkEngine> {
    match &cli.model {
        ModelChoice::Malayalam => {
            let dir = cli.model_dir.as_ref()
                .context("--model-dir is required for the 'malayalam' model")?;
            eprintln!("[benchmark] Loading MalayalamAsr from {:?} …", dir);
            let asr = if cli.use_gpu {
                MalayalamAsr::load_gpu(dir)?
            } else {
                MalayalamAsr::load(dir)?
            };
            Ok(BenchmarkEngine::Malayalam(asr))
        }

        ModelChoice::WhisperTurbo | ModelChoice::WhisperLarge => {
            let bin_path = resolve_whisper_path(cli)?;
            eprintln!("[benchmark] Loading Whisper from {:?} (GPU={}) …", bin_path, cli.use_gpu);
            let params = WhisperLoadParams {
                use_gpu: cli.use_gpu,
                flash_attn: true,
                gpu_device: transcribe_rs::accel::GPU_DEVICE_AUTO,
            };
            let engine = WhisperEngine::load_with_params(&bin_path, params)
                .map_err(|e| anyhow::anyhow!("Failed to load Whisper model: {}", e))?;
            Ok(BenchmarkEngine::Whisper { engine, language: whisper_lang, n_threads: cli.n_threads })
        }
    }
}

fn resolve_whisper_path(cli: &Cli) -> Result<PathBuf> {
    if let Some(p) = &cli.whisper_model_path {
        return Ok(p.clone());
    }
    let handy_models = dirs_next_appdata().join("com.pais.handy").join("models");
    let filename = match &cli.model {
        ModelChoice::WhisperTurbo  => "ggml-large-v3-turbo.bin",
        ModelChoice::WhisperLarge  => "ggml-large-v3-q5_0.bin",
        ModelChoice::Malayalam     => unreachable!(),
    };
    let path = handy_models.join(filename);
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Whisper model not found at {:?}. Use --whisper-model-path to specify the path.",
            path
        ));
    }
    Ok(path)
}

fn dirs_next_appdata() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_sys_appdata())
}

fn dirs_sys_appdata() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
        .join("AppData").join("Roaming")
}

fn model_display_name(m: &ModelChoice) -> &'static str {
    match m {
        ModelChoice::Malayalam    => "MalayalamAsr (IndicConformer CTC)",
        ModelChoice::WhisperTurbo => "Whisper Turbo (large-v3-turbo)",
        ModelChoice::WhisperLarge => "Whisper Large (large-v3-q5_0)",
    }
}

// ─── Single-clip inference ────────────────────────────────────────────────────

fn run_clip(
    engine: &mut BenchmarkEngine,
    sys: &mut System,
    pid: Pid,
    refresh_kind: RefreshKind,
    stem: &str,
    is_multi_speaker: bool,
    is_code_switched: bool,
    noise_label: String,
    samples: &[f32],
    audio_secs: f64,
    ground_truth: &str,
) -> Result<ClipResult> {
    sys.refresh_specifics(refresh_kind);
    let rss_before = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
    let cpu_before = sys.process(pid).map(|p| p.cpu_usage()).unwrap_or(0.0);

    let t0 = Instant::now();
    let hypothesis = engine.transcribe(samples)
        .with_context(|| format!("Transcription failed for {}", stem))?;
    let latency_secs = t0.elapsed().as_secs_f64();

    sys.refresh_specifics(refresh_kind);
    let rss_after = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
    let cpu_after = sys.process(pid).map(|p| p.cpu_usage()).unwrap_or(0.0);

    let peak_rss_mb = rss_after.max(rss_before) as f64 / 1024.0 / 1024.0;
    let cpu_pct = (cpu_before + cpu_after) / 2.0;

    let reference = normalize_text(ground_truth);
    let hyp_n = normalize_text(&hypothesis);

    // WER
    let ref_words: Vec<&str> = reference.split_whitespace().collect();
    let hyp_words: Vec<&str> = hyp_n.split_whitespace().collect();
    let (s, i, d) = edit_ops(&ref_words, &hyp_words);
    let wer = if ref_words.is_empty() { 0.0 } else { (s + i + d) as f64 / ref_words.len() as f64 };

    // CER
    let ref_chars = char_tokens(&reference);
    let hyp_chars = char_tokens(&hyp_n);
    let rcr: Vec<&str> = ref_chars.iter().map(|s| s.as_str()).collect();
    let hcr: Vec<&str> = hyp_chars.iter().map(|s| s.as_str()).collect();
    let (cs2, ci2, cd2) = edit_ops(&rcr, &hcr);
    let cer = if ref_chars.is_empty() { 0.0 } else { (cs2 + ci2 + cd2) as f64 / ref_chars.len() as f64 };

    Ok(ClipResult {
        stem: stem.to_string(),
        is_multi_speaker, is_code_switched, noise_label,
        audio_secs, latency_secs,
        wer, cer,
        subs: s, ins: i, dels: d,
        peak_rss_mb, cpu_pct,
    })
}

// ─── WAV Loading + Resampling ─────────────────────────────────────────────────

fn load_wav_16k(path: &Path) -> Result<(Vec<f32>, f64)> {
    let mut reader = WavReader::open(path)
        .with_context(|| format!("Cannot open {:?}", path))?;
    let spec = reader.spec();

    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let max = 2.0f32.powi(spec.bits_per_sample as i32 - 1);
            reader.samples::<i32>().map(|s| s.unwrap() as f32 / max).collect()
        }
    };

    let mono: Vec<f32> = if spec.channels == 1 { raw } else {
        raw.chunks(spec.channels as usize)
            .map(|ch| ch.iter().sum::<f32>() / ch.len() as f32)
            .collect()
    };

    let target = 16000usize;
    let in_sr = spec.sample_rate as usize;
    let samples = if in_sr == target { mono } else { resample(&mono, in_sr, target)? };
    let secs = samples.len() as f64 / target as f64;
    Ok((samples, secs))
}

fn resample(input: &[f32], in_sr: usize, out_sr: usize) -> Result<Vec<f32>> {
    let chunk = 1024usize;
    let mut r = FftFixedIn::<f32>::new(in_sr, out_sr, chunk, 1, 1)?;
    let mut out = Vec::new();
    let mut src = input;
    while src.len() >= chunk {
        let res = r.process(&[&src[..chunk]], None)?;
        out.extend_from_slice(&res[0]);
        src = &src[chunk..];
    }
    if !src.is_empty() {
        let mut pad = src.to_vec();
        pad.resize(chunk, 0.0);
        let res = r.process(&[&pad], None)?;
        out.extend_from_slice(&res[0]);
    }
    Ok(out)
}

// ─── Noise ────────────────────────────────────────────────────────────────────

fn apply_gaussian_noise(samples: &[f32], level: f32) -> Vec<f32> {
    use std::f32::consts::PI;
    let n = samples.len().max(1);
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < samples.len() {
        let u1 = (i as f32 + 1.0) / (n as f32 + 1.0);
        let u2 = ((i * 6791 + 1337) % n) as f32 / n as f32 + 1e-9;
        let mag = (-2.0 * u1.ln()).sqrt() * level;
        let z0 = mag * (2.0 * PI * u2).cos();
        let z1 = mag * (2.0 * PI * u2).sin();
        out.push((samples[i] + z0).clamp(-1.0, 1.0));
        i += 1;
        if i < samples.len() { out.push((samples[i] + z1).clamp(-1.0, 1.0)); i += 1; }
    }
    out
}

// ─── WAV Discovery ────────────────────────────────────────────────────────────

fn collect_wavs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    visit_dir(dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn visit_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() { visit_dir(&p, out)?; }
        else if p.extension().and_then(|e| e.to_str()) == Some("wav") { out.push(p); }
    }
    Ok(())
}

fn stem_of(p: &Path) -> String {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
}

// ─── Text Normalization ───────────────────────────────────────────────────────

fn normalize_text(t: &str) -> String {
    t.replace('\n', " ")
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn char_tokens(s: &str) -> Vec<String> {
    s.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_string()).collect()
}

// ─── Levenshtein edit ops ─────────────────────────────────────────────────────

/// Returns (substitutions, insertions, deletions)
fn edit_ops<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> (usize, usize, usize) {
    let n = reference.len();
    let m = hypothesis.len();
    let mut dp = vec![vec![(0usize, 0usize, 0usize, 0usize); m + 1]; n + 1];
    for i in 0..=n { dp[i][0] = (i, 0, 0, i); }
    for j in 0..=m { dp[0][j] = (j, 0, j, 0); }
    for i in 1..=n {
        for j in 1..=m {
            if reference[i - 1] == hypothesis[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                let (cs, ss, is_, ds) = dp[i - 1][j - 1]; // sub
                let (cd, sd, id_, dd) = dp[i - 1][j];     // del
                let (ci, si, ii, di) = dp[i][j - 1];      // ins
                if cs + 1 <= cd + 1 && cs + 1 <= ci + 1 {
                    dp[i][j] = (cs + 1, ss + 1, is_, ds);
                } else if cd + 1 <= ci + 1 {
                    dp[i][j] = (cd + 1, sd, id_, dd + 1);
                } else {
                    dp[i][j] = (ci + 1, si, ii + 1, di);
                }
            }
        }
    }
    let (_, s, i, d) = dp[n][m];
    (s, i, d)
}

// ─── Report structures ────────────────────────────────────────────────────────

struct CombinedResult {
    label: String,
    clip_count: usize,
    audio_secs: f64,
    latency_secs: f64,
    wer: f64,
    cer: f64,
    subs: usize,
    ins: usize,
    dels: usize,
    peak_rss_mb: f64,
}

#[derive(Default)]
struct GroupStats {
    label: String,
    count: usize,
    total_audio: f64,
    total_latency: f64,
    total_wer: f64,
    total_cer: f64,
    total_s: usize,
    total_i: usize,
    total_d: usize,
    peak_rss: f64,
    total_cpu: f64,
}

impl GroupStats {
    fn new(label: &str) -> Self { Self { label: label.into(), ..Default::default() } }

    fn add(&mut self, r: &ClipResult) {
        self.count += 1;
        self.total_audio   += r.audio_secs;
        self.total_latency += r.latency_secs;
        self.total_wer     += r.wer;
        self.total_cer     += r.cer;
        self.total_s       += r.subs;
        self.total_i       += r.ins;
        self.total_d       += r.dels;
        self.peak_rss       = self.peak_rss.max(r.peak_rss_mb);
        self.total_cpu     += r.cpu_pct as f64;
    }

    fn rtf(&self)      -> f64 { if self.total_audio == 0.0 { 0.0 } else { self.total_latency / self.total_audio } }
    fn avg_lat(&self)  -> f64 { if self.count == 0 { 0.0 } else { self.total_latency * 1000.0 / self.count as f64 } }
    fn avg_wer(&self)  -> f64 { if self.count == 0 { 0.0 } else { self.total_wer / self.count as f64 } }
    fn avg_cer(&self)  -> f64 { if self.count == 0 { 0.0 } else { self.total_cer / self.count as f64 } }
    fn avg_cpu(&self)  -> f64 { if self.count == 0 { 0.0 } else { self.total_cpu / self.count as f64 } }
}

fn print_group_table(groups: &[GroupStats]) {
    println!("| Group | N | WER | CER | S | I | D | RTF | Avg Lat(ms) | Peak RAM(MB) | Avg CPU% |");
    println!("|-------|---|-----|-----|---|---|---|-----|-------------|--------------|----------|");
    for g in groups {
        println!("| {} | {} | {:.3} | {:.3} | {} | {} | {} | {:.3} | {:.0} | {:.1} | {:.1} |",
            g.label, g.count,
            g.avg_wer(), g.avg_cer(),
            g.total_s, g.total_i, g.total_d,
            g.rtf(), g.avg_lat(), g.peak_rss, g.avg_cpu());
    }
    println!();
}

// ─── Full Markdown Report ─────────────────────────────────────────────────────

fn print_report(results: &[ClipResult], combined: &[CombinedResult], model_label: &str) {
    println!("# ASR Benchmark Report — {}\n", model_label);

    // ── Per-clip detail ───────────────────────────────────────────────────────
    println!("## Per-Clip Results\n");
    println!("| Stem | Multi | CS | Noise | WER | CER | S | I | D | Lat(ms) | Audio(s) | RAM(MB) | CPU% |");
    println!("|------|-------|----|-------|-----|-----|---|---|---|---------|----------|---------|------|");
    for r in results {
        println!("| {} | {} | {} | {} | {:.2} | {:.2} | {} | {} | {} | {:.0} | {:.2} | {:.1} | {:.1} |",
            r.stem,
            if r.is_multi_speaker { "✓" } else { "" },
            if r.is_code_switched { "✓" } else { "" },
            r.noise_label,
            r.wer, r.cer, r.subs, r.ins, r.dels,
            r.latency_secs * 1000.0, r.audio_secs, r.peak_rss_mb, r.cpu_pct);
    }
    println!();

    // ── Overall Dataset ───────────────────────────────────────────────────────
    println!("## Overall Dataset\n");
    let mut overall = GroupStats::new("Overall");
    for r in results { overall.add(r); }
    print_group_table(&[overall]);

    // ── By Noise Level ────────────────────────────────────────────────────────
    println!("## Clean vs. Noisy\n");
    let mut g_clean = GroupStats::new("Clean");
    let mut g_noisy = GroupStats::new("Noisy (σ=0.03)");
    let mut g_heavy = GroupStats::new("Heavy Noisy (σ=0.20)");
    for r in results {
        match r.noise_label.as_str() {
            "clean"       => g_clean.add(r),
            "noisy"       => g_noisy.add(r),
            "heavy_noisy" => g_heavy.add(r),
            _ => {}
        }
    }
    print_group_table(&[g_clean, g_noisy, g_heavy]);

    // ── Single vs. Multi Speaker ──────────────────────────────────────────────
    println!("## Single Speaker vs. Multi Speaker (clean only)\n");
    let mut g_single = GroupStats::new("Single Speaker");
    let mut g_multi  = GroupStats::new("Multi Speaker");
    for r in results.iter().filter(|r| r.noise_label == "clean") {
        if r.is_multi_speaker { g_multi.add(r); } else { g_single.add(r); }
    }
    print_group_table(&[g_single, g_multi]);

    // ── Pure Malayalam vs. Code-Switched ─────────────────────────────────────
    println!("## Pure Malayalam vs. Code-Switched (clean only)\n");
    let mut g_mal = GroupStats::new("Pure Malayalam");
    let mut g_cs  = GroupStats::new("Code-Switched");
    for r in results.iter().filter(|r| r.noise_label == "clean") {
        if r.is_code_switched { g_cs.add(r); } else { g_mal.add(r); }
    }
    print_group_table(&[g_mal, g_cs]);

    // ── Noise Robustness Gap ──────────────────────────────────────────────────
    println!("## Noise Robustness Gap\n");
    println!("| Stem | WER_clean | WER_noisy | ΔWER | WER_heavy | ΔheavyWER | CER_clean | CER_noisy | ΔCER |");
    println!("|------|-----------|-----------|------|-----------|-----------|-----------|-----------|------|");

    let mut by_stem: HashMap<&str, [Option<(f64, f64)>; 3]> = HashMap::new();
    for r in results {
        let slot = match r.noise_label.as_str() {
            "clean"       => 0,
            "noisy"       => 1,
            "heavy_noisy" => 2,
            _             => continue,
        };
        by_stem.entry(&r.stem).or_insert([None; 3])[slot] = Some((r.wer, r.cer));
    }

    let mut stems: Vec<&&str> = by_stem.keys().collect();
    stems.sort();

    let mut sum_dw1 = 0.0f64; let mut sum_dw2 = 0.0f64;
    let mut sum_dc1 = 0.0f64; let mut cnt = 0usize;

    for stem in &stems {
        let v = &by_stem[*stem];
        let (cw, cc) = v[0].unwrap_or((f64::NAN, f64::NAN));
        let (nw, nc) = v[1].unwrap_or((f64::NAN, f64::NAN));
        let (hw, _)  = v[2].unwrap_or((f64::NAN, f64::NAN));
        let dw1 = nw - cw;
        let dw2 = hw - cw;
        let dc1 = nc - cc;
        if !dw1.is_nan() && !dw2.is_nan() {
            sum_dw1 += dw1; sum_dw2 += dw2; sum_dc1 += dc1; cnt += 1;
        }
        println!("| {} | {:.3} | {:.3} | **{:+.3}** | {:.3} | **{:+.3}** | {:.3} | {:.3} | **{:+.3}** |",
            stem,
            na(cw), na(nw), na(dw1),
            na(hw), na(dw2),
            na(cc), na(nc), na(dc1));
    }
    if cnt > 0 {
        println!("| **Avg** | — | — | **{:+.3}** | — | **{:+.3}** | — | — | **{:+.3}** |",
            sum_dw1 / cnt as f64,
            sum_dw2 / cnt as f64,
            sum_dc1 / cnt as f64);
    }
    println!();

    // ── Code-Switching Performance Gap ────────────────────────────────────────
    println!("## Code-Switching Performance Gap\n");
    println!("| Noise Level | WER Mal-only | WER Code-Switched | ΔWER | CER Mal-only | CER Code-Switched | ΔCER |");
    println!("|-------------|-------------|-------------------|------|-------------|-------------------|------|");
    for noise in ["clean", "noisy", "heavy_noisy"] {
        let mal_wer: Vec<f64> = results.iter()
            .filter(|r| !r.is_code_switched && !r.is_multi_speaker && r.noise_label == noise)
            .map(|r| r.wer).collect();
        let cs_wer: Vec<f64> = results.iter()
            .filter(|r| r.is_code_switched && r.noise_label == noise)
            .map(|r| r.wer).collect();
        let mal_cer: Vec<f64> = results.iter()
            .filter(|r| !r.is_code_switched && !r.is_multi_speaker && r.noise_label == noise)
            .map(|r| r.cer).collect();
        let cs_cer: Vec<f64> = results.iter()
            .filter(|r| r.is_code_switched && r.noise_label == noise)
            .map(|r| r.cer).collect();

        let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
        let mw = avg(&mal_wer); let cw = avg(&cs_wer);
        let mc = avg(&mal_cer); let cc = avg(&cs_cer);
        println!("| {} | {:.3} | {:.3} | **{:+.3}** | {:.3} | {:.3} | **{:+.3}** |",
            noise, mw, cw, cw - mw, mc, cc, cc - mc);
    }
    println!();

    // ── Combined Long-Clip Results ────────────────────────────────────────────
    println!("## Combined Long-Clip Results\n");
    println!("| Group | Clips | Audio(s) | WER | CER | S | I | D | RTF | Lat(ms) | RAM(MB) |");
    println!("|-------|-------|----------|-----|-----|---|---|---|-----|---------|---------|");
    for c in combined {
        let rtf = if c.audio_secs == 0.0 { 0.0 } else { c.latency_secs / c.audio_secs };
        println!("| {} | {} | {:.1} | {:.3} | {:.3} | {} | {} | {} | {:.3} | {:.0} | {:.1} |",
            c.label, c.clip_count, c.audio_secs,
            c.wer, c.cer, c.subs, c.ins, c.dels,
            rtf, c.latency_secs * 1000.0, c.peak_rss_mb);
    }
    println!();

    // ── Summary Scorecard ─────────────────────────────────────────────────────
    println!("## Summary Scorecard\n");
    let clean_clips: Vec<&ClipResult> = results.iter().filter(|r| r.noise_label == "clean").collect();
    let total_lat: f64 = clean_clips.iter().map(|r| r.latency_secs).sum();
    let total_dur: f64 = clean_clips.iter().map(|r| r.audio_secs).sum();
    let avg_wer: f64   = clean_clips.iter().map(|r| r.wer).sum::<f64>() / clean_clips.len().max(1) as f64;
    let avg_cer: f64   = clean_clips.iter().map(|r| r.cer).sum::<f64>() / clean_clips.len().max(1) as f64;
    let avg_lat: f64   = total_lat * 1000.0 / clean_clips.len().max(1) as f64;
    let rtf_clean      = if total_dur == 0.0 { 0.0 } else { total_lat / total_dur };
    let peak_rss: f64  = results.iter().map(|r| r.peak_rss_mb).fold(0.0f64, f64::max);

    println!("| Metric | Value |");
    println!("|--------|-------|");
    println!("| Overall WER (clean) | {:.3} ({:.1}%) |", avg_wer, avg_wer * 100.0);
    println!("| Overall CER (clean) | {:.3} ({:.1}%) |", avg_cer, avg_cer * 100.0);
    println!("| RTF (clean clips) | {:.3} | ", rtf_clean);
    println!("| Avg Latency (clean) | {:.0} ms |", avg_lat);
    println!("| Peak RAM | {:.1} MB |", peak_rss);
    println!();
}

/// Format f64 as "N/A" when NaN
fn na(v: f64) -> String {
    if v.is_nan() { "N/A".to_string() } else { format!("{:.3}", v) }
}
