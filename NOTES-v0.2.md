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
