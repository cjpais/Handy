//! Compare ASR engines on your own recordings, before and after the Singlish
//! replacement layer.
//!
//!     cargo run --release --features metal --bin bakeoff -- \
//!         --audio-dir ~/clips --engines parakeet,sensevoice,qwen3asr
//!
//! Each `clipNN.wav` needs a `clipNN.txt` reference transcript beside it.

use singlish_tools::audio::load_wav_16k_mono;
use singlish_tools::report::{csv_rows, format_table, ClipResult, CSV_HEADER};
use singlish_tools::{apply, strip_asr_tags, Vocabulary};
use std::path::{Path, PathBuf};
use transcribe_cpp::{Feature, Model, RunOptions};

const USAGE: &str = "usage: bakeoff --audio-dir <dir> --engines parakeet,sensevoice[,qwen3asr]
       [--models-dir <dir>] [--model <engine>=<path.gguf>]... [--vocab <file.json>]
       [--vocab-hints] [--csv <file>]

  --models-dir   where Handy keeps downloaded models (default: Handy's app data dir)
  --model        use this exact .gguf for an engine instead of auto-detecting
  --vocab        vocabulary JSON (default: your Handy singlish_vocabulary.json, else bundled)
  --vocab-hints  also pass vocabulary terms to engines that accept them (changes the raw baseline)
  --csv          output path (default: bakeoff-results.csv)";

struct Args {
    audio_dir: PathBuf,
    engines: Vec<String>,
    models_dir: PathBuf,
    overrides: Vec<(String, PathBuf)>,
    vocab: Option<PathBuf>,
    vocab_hints: bool,
    csv: PathBuf,
}

fn default_models_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/com.pais.handy/models")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("com.pais.handy/models")
    } else {
        home.join(".local/share/com.pais.handy/models")
    }
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let (mut audio_dir, mut engines, mut models_dir) = (None, None, None);
    let (mut overrides, mut vocab, mut vocab_hints) = (Vec::new(), None, false);
    let mut csv = PathBuf::from("bakeoff-results.csv");
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--audio-dir" => audio_dir = Some(PathBuf::from(value("--audio-dir")?)),
            "--engines" => {
                engines = Some(
                    value("--engines")?
                        .split(',')
                        .map(|e| e.trim().to_lowercase())
                        .filter(|e| !e.is_empty())
                        .collect::<Vec<_>>(),
                )
            }
            "--models-dir" => models_dir = Some(PathBuf::from(value("--models-dir")?)),
            "--model" => {
                let v = value("--model")?;
                let (e, p) = v.split_once('=').ok_or("--model expects <engine>=<path>")?;
                overrides.push((e.to_lowercase(), PathBuf::from(p)));
            }
            "--vocab" => vocab = Some(PathBuf::from(value("--vocab")?)),
            "--vocab-hints" => vocab_hints = true,
            "--csv" => csv = PathBuf::from(value("--csv")?),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        audio_dir: audio_dir.ok_or("--audio-dir is required")?,
        engines: engines.ok_or("--engines is required")?,
        models_dir: models_dir.unwrap_or_else(default_models_dir),
        overrides,
        vocab,
        vocab_hints,
        csv,
    })
}

/// File-name fragments that identify an engine's GGUF.
fn patterns(engine: &str) -> Result<&'static [&'static str], String> {
    match engine {
        "parakeet" => Ok(&["parakeet"]),
        "sensevoice" => Ok(&["sensevoice", "sense-voice"]),
        "qwen3asr" => Ok(&["qwen3-asr", "qwen3asr"]),
        other => Err(format!(
            "unknown engine '{other}' (use parakeet, sensevoice, qwen3asr)"
        )),
    }
}

fn find_ggufs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_ggufs(&path, out);
        } else if path.extension().is_some_and(|e| e == "gguf") {
            out.push(path);
        }
    }
}

fn locate_model(args: &Args, engine: &str) -> Result<PathBuf, String> {
    if let Some((_, p)) = args.overrides.iter().find(|(e, _)| e == engine) {
        return Ok(p.clone());
    }
    let pats = patterns(engine)?;
    let mut all = Vec::new();
    find_ggufs(&args.models_dir, &mut all);
    let mut found: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            pats.iter().any(|pat| name.contains(pat))
        })
        .collect();
    // Prefer Q8_0, then shorter names (the base variant), then alphabetical.
    found.sort_by_key(|p| {
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        (!name.contains("q8_0"), name.len(), name)
    });
    if found.len() > 1 {
        eprintln!(
            "note: {engine}: {} candidate models; using {}",
            found.len(),
            found[0].display()
        );
        eprintln!("      pass --model {engine}=<path> to pick another");
    }
    found.into_iter().next().ok_or_else(|| {
        format!(
            "no {engine} .gguf found under {} (download it in Handy first, or pass --model {engine}=<path>)",
            args.models_dir.display()
        )
    })
}

fn load_clips(dir: &Path) -> Result<Vec<(String, Vec<f32>, String)>, String> {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();
    let mut clips = Vec::new();
    for wav in wavs {
        let txt = wav.with_extension("txt");
        let name = wav
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let Ok(reference) = std::fs::read_to_string(&txt) else {
            eprintln!("skip {name}: no {}", txt.display());
            continue;
        };
        clips.push((name, load_wav_16k_mono(&wav)?, reference.trim().to_string()));
    }
    if clips.is_empty() {
        return Err(format!(
            "no clipNN.wav + clipNN.txt pairs in {}",
            dir.display()
        ));
    }
    Ok(clips)
}

fn load_vocab(args: &Args) -> Result<Vocabulary, String> {
    let path = args.vocab.clone().or_else(|| {
        let p = args.models_dir.parent()?.join("singlish_vocabulary.json");
        p.exists().then_some(p)
    });
    match path {
        Some(p) => {
            eprintln!("vocabulary: {}", p.display());
            let json = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            Vocabulary::from_json(&json).map_err(|e| format!("{}: {e}", p.display()))
        }
        None => {
            eprintln!("vocabulary: bundled default");
            Ok(Vocabulary::bundled())
        }
    }
}

fn run_engine(
    args: &Args,
    engine: &str,
    clips: &[(String, Vec<f32>, String)],
    vocab: &Vocabulary,
) -> Result<Vec<ClipResult>, String> {
    let path = locate_model(args, engine)?;
    eprintln!("{engine}: loading {}", path.display());
    let model = Model::load(&path).map_err(|e| format!("load {}: {e}", path.display()))?;
    let mut session = model.session().map_err(|e| e.to_string())?;
    let mut options = RunOptions::default();
    if args.vocab_hints && model.supports(Feature::Vocabulary) {
        options.vocabulary = vocab
            .protected_terms
            .iter()
            .cloned()
            .chain(vocab.replacements.iter().map(|r| r.to.clone()))
            .collect();
    }
    let mut results = Vec::new();
    for (name, pcm, reference) in clips {
        eprintln!("{engine}: {name}");
        let result = session
            .run(pcm, &options)
            .map_err(|e| format!("{name}: {e}"))?;
        let raw = strip_asr_tags(result.text.trim());
        let cleaned = apply(&raw, vocab);
        results.push(ClipResult {
            clip: name.clone(),
            reference: reference.clone(),
            raw,
            cleaned,
        });
    }
    Ok(results)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(if e.is_empty() { 0 } else { 2 });
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    transcribe_cpp::init_logging();
    // Registers Metal/CPU backends; a failure here still allows CPU runs.
    if let Err(e) = transcribe_cpp::init_backends_default() {
        eprintln!("warning: backend init: {e}");
    }
    let clips = load_clips(&args.audio_dir)?;
    let vocab = load_vocab(args)?;
    eprintln!("{} clips\n", clips.len());

    let mut csv = vec![CSV_HEADER.to_string()];
    for engine in &args.engines {
        match run_engine(args, engine, &clips, &vocab) {
            Ok(results) => {
                println!("{}", format_table(engine, &results));
                csv.extend(csv_rows(engine, &results));
            }
            // One missing model should not throw away the other engines' results.
            Err(e) => eprintln!("{engine}: skipped: {e}\n"),
        }
    }
    std::fs::write(&args.csv, csv.join("\n") + "\n")
        .map_err(|e| format!("{}: {e}", args.csv.display()))?;
    println!("wrote {}", args.csv.display());
    Ok(())
}
