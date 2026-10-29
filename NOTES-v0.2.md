# Handy v0.2 (Singlish / Chinglish) — working notes

Branch: `v0.2-singlish` (never pushed to `main`).

## Phase 0 — Recon

### (a) Inference library and engines

The brief assumed `transcribe-rs`. This fork does **not** use it. All models run
through **`transcribe-cpp` 0.4.0** (GGUF/ggml; `src-tauri/Cargo.toml`), plus
`handy-recorder` 0.1 for capture and `earshot` for VAD. There is no ONNX path any
more (`RetiredOnnxModel` entries in `managers/model.rs` map old ONNX ids to GGUF
replacements, e.g. `sense-voice-int8` -> `handy-computer/SenseVoiceSmall-gguf`).

Models are declared in `src-tauri/src/catalog/catalog.json` (schema 2) and listed
in the UI from there. Families present include: whisper (small / medium /
large-v3 / turbo), parakeet (several), **sensevoice (`SenseVoiceSmall`)**,
**Qwen3-ASR (`Qwen3-ASR-0.6B`, `Qwen3-ASR-1.7B`)**, canary, cohere, moonshine,
gigaam, granite, voxtral, Fun-ASR, and others.

Consequences for the plan:

- **Phase 1:** SenseVoice is already selectable (catalog id `SenseVoiceSmall`,
  family `sensevoice`, Q8_0 default, languages zh/yue/en/ja/ko). No engine work and
  no `transcribe-rs` bump is needed.
- **Phase 5:** Qwen3-ASR already exists as a catalog model through `transcribe-cpp`,
  so a separate candle-based engine behind a `qwen3asr` feature is redundant.
  Decision: skip Phase 5 as specified and use the existing Qwen3-ASR models in the
  bakeoff instead.

### (b) Custom words

Yes. `AppSettings.custom_words: Vec<String>` (`settings.rs`), plus
`word_correction_threshold`.

- `audio_toolkit/text.rs::apply_custom_words` does fuzzy (n-gram, Soundex, edit
  distance) correction toward the user's words.
- For models that support vocabulary prompting (`Feature::Vocabulary`), the words
  are passed to the engine as a hint instead (`vocabulary_for_model` in
  `managers/transcription.rs`), and fuzzy correction is skipped
  (`custom_words_already_prompted`).

This is a *spelling-biasing* feature (list of target words), not a from->to
replacement table. The Singlish layer is therefore additive and does not replace it.

### (c) Post-transcription hooks

`managers/transcription.rs::post_process_transcription_text` is the single
pipeline, in order: Chinese script conversion (`chinese_script.rs`, OpenCC) ->
custom words -> filler word removal -> `normalize_transcription_output`. After that,
`actions.rs` optionally runs LLM post-processing (`post_process_transcription`)
before paste. Special tokens such as SenseVoice's `<|en|>`, `<|HAPPY|>`, `<|woitn|>`
are CONTROL-typed in the GGUF tokenizer and are dropped inside transcribe-cpp; the
Rust side has no tag handling of its own.

### (d) GitHub Actions

| Workflow | Trigger | Does |
| --- | --- | --- |
| `build.yml` | `workflow_call` | Reusable. Checkout, bun, Rust, per-OS deps, optional signing, `tauri-action`, uploads artifacts (macOS: `.dmg` + `.app`). |
| `build-test.yml` | manual | Matrix over macOS arm64/x64, Linux, Windows via `build.yml`, signing on. |
| `main-build.yml` | push to `main` | Same via `build.yml`. |
| `pr-test-build.yml` | manual | PR build via `build.yml`. |
| `release.yml` | release | Signed release builds. |
| `test.yml`, `code-quality.yml`, `nix-check.yml`, `playwright.yml` | manual / `main` | Tests, lint, nix, e2e. |

`build.yml` reads secrets only when `sign-binaries` is true, so calling it with
`sign-binaries: false` gives an unsigned build with no secrets.

## Phase 1 — SenseVoice

Already in Handy (see Phase 0), so no engine work and no `transcribe-rs`/`transcribe-cpp`
bump. Changes:

- Catalog label changed to **"SenseVoice (Chinese + English)"** (`catalog.json`, id
  `SenseVoiceSmall` unchanged so downloads and settings keep working). If you merge
  upstream and `catalog.json` conflicts, re-apply this one-line rename.
- "int8": the GGUF catalog has no int8 file. The default quant is **Q8_0**, the
  closest equivalent (252 MB).
- Tag stripping: transcribe-cpp drops CONTROL tokens (`<|zh|>`, `<|HAPPY|>`,
  `<|Speech|>`, `<|withitn|>`) unless `keep_special_tags` is set, and Handy never sets
  it. As a safety net, `singlish_tools::strip_asr_tags` also runs first in
  `post_process_transcription_text` (unit-tested).
- Simplified output: Handy's existing **Chinese script** setting (Settings > Advanced)
  converts Mandarin output via OpenCC. Set it to *Simplified*. Its default follows the
  system locale, so I did not change it.

## Phase 2 — Singlish vocabulary layer

- Crate: `crates/singlish-tools` (no Tauri dependency, own `[workspace]`).
  `cd crates/singlish-tools && cargo test` runs 42 tests (19 for the vocabulary layer).
- Hook: `src-tauri/src/singlish.rs` + one call in
  `managers/transcription.rs::post_process_transcription_text`, right after custom
  words and before filler removal. Runs after the transcript is final, before paste.
- File: `<app data dir>/singlish_vocabulary.json`, created from
  `crates/singlish-tools/src/default_vocabulary.json` on first run. On macOS that is
  `~/Library/Application Support/com.pais.handy/`. It is re-read when its modification
  time changes, so edits apply to the next transcription without a restart. A file
  with invalid JSON is ignored (bundled default is used) and a warning is logged.
- Settings > Advanced > Transcription: **Singlish vocabulary** toggle (default ON,
  `singlish_vocabulary_enabled`) and an **Open file** button.
- Integration with custom words: they are separate and stack. Custom words
  (`settings.custom_words`) bias spelling toward a list of target words (prompted into
  the model when supported, else fuzzy-matched). The Singlish file is an explicit
  from->to table plus casing fixes. Custom words run first. Nothing was duplicated or
  replaced.
- Matching rules: case-insensitive; whole word only (a word is a run of non-CJK
  letters/digits/apostrophes); multi-word `from` phrases need plain whitespace between
  words; Chinese characters are never part of a match but a Latin word next to Chinese
  still matches (`好好la` -> `好好lah`). Replacements keep a leading capital
  (`La` -> `Lah`). `protected_terms` always win over replacements inside their span.
- **Deliberate deviation:** matches written ALL-CAPS with 2+ letters (`LA`, `LAR`) are
  treated as acronyms and left alone by *replacements*, so "I live in LA" is not
  turned into "lah". Protected terms are unaffected.
- Conservative seed: no mapping from a common English word (`low`, `may`, `shook`,
  `chop`, `sir`, `lore`, `lay`, `le`, `lo` are all deliberately absent; tests cover
  these). `steady` and `makan` etc. only have mis-hearing sources mapped; the
  canonical word itself is not forced into a casing. `la`/`lar` -> `lah` are in the
  seed as you specified; remove them from your file if "la" hurts your dictation.
- Seed protected terms: Funding Societies, Worksplice, Singlish, Singapore, PayNow,
  GrabPay, MRT, HDB, CPF. Edit freely.
- The mis-hearing lists are educated guesses, not measured. Use `bakeoff` (below)
  on your recordings, read `hypothesis_raw` in the CSV, and add what you actually see.

## Phase 3 — Evaluation CLI (`bakeoff`)

`crates/singlish-tools/src/bin/bakeoff.rs`. It loads GGUF models through
`transcribe-cpp` (pinned `=0.4.0`, same as the app). Because that compiles ggml, the
binary sits behind a Cargo feature (`engines`; `metal` implies it), so plain
`cargo test` in the crate stays fast and never builds it.

- Metric: Mixed Error Rate. English words are split on whitespace, lower-cased, with
  edge punctuation stripped; every Chinese/Japanese/Korean character is one token;
  Chinese punctuation is ignored. Errors are token-level Levenshtein (sub + del + ins)
  over reference token count. Per-engine TOTAL is corpus-level (total errors / total
  reference tokens), not a mean of clip rates.
- "Before" = raw engine text (tags stripped). "After" = same text through the
  Singlish layer using your own `singlish_vocabulary.json` if found, else the bundled
  default (`--vocab` overrides).
- Audio is resampled to 16 kHz mono (windowed-sinc) from any WAV (16/24/32-bit int or
  float, any channel count).
- Engines: `parakeet`, `sensevoice`, `qwen3asr` map to the downloaded GGUF files in
  Handy's models folder (matched by file name, Q8_0 preferred). If several match
  (there are many Parakeet variants) it prints the one it picked; override with
  `--model parakeet=/path/to/model.gguf`. A missing model skips that engine and the
  others still run. Note Parakeet Unified is English-only, so expect it to do badly
  on the Chinese parts: that is a true result for your use case.
- Not applied in the bakeoff: Chinese Simplified/Traditional conversion, filler-word
  removal, custom words. Reference transcripts should therefore be in Simplified
  characters.

### Running it on your Mac with your own recordings

1. In the Handy v0.2 app, download the three models (Settings > Models): a Parakeet
   model, **SenseVoice (Chinese + English)**, and **Qwen3-ASR 0.6B**.
2. Make a folder, e.g. `~/clips`, with pairs: `clip01.wav` + `clip01.txt`,
   `clip02.wav` + `clip02.txt`, ... Record real speech in your own style (QuickTime or
   Voice Memos exports work once converted to WAV, e.g.
   `afconvert -f WAVE -d LEI16 in.m4a clip01.wav`). The `.txt` holds exactly what you
   said, with `lah`/`leh` spelled as you want them, Chinese in Simplified characters.
   Aim for 30+ clips with a mix of English, Singlish, and Chinese-English.
3. Install Rust (`brew install rustup && rustup-init`), then:

```bash
git clone https://github.com/Kai-0501/Handy.git && cd Handy && git checkout v0.2-singlish
cd crates/singlish-tools
cargo run --release --features metal --bin bakeoff -- --audio-dir ~/clips --engines parakeet,sensevoice,qwen3asr
```

Output: one table per engine on stdout (per-clip and TOTAL, MER before and after the
Singlish layer) and `bakeoff-results.csv` in the current directory (includes the
reference and raw/cleaned hypotheses for every clip). Run `cargo run ... -- --help`
for options. If auto-detection picks the wrong model, add
`--model sensevoice=~/Library/Application\ Support/com.pais.handy/models/<file>.gguf`
(use the full path, `~` is expanded by your shell only when unquoted).
