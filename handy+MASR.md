# MASR — Malayalam ASR Migration Plan

Replace Handy's generic multi-model catalog with a single Malayalam IndicConformer CTC model while keeping Handy's
shortcut / audio / VAD / paste / history / UI flow intact. The new ASR engine is a native Rust ONNX module;
no Python sidecar is used.

---

## Migration Status & Progress Checklist

- [x] Staged Malayalam IndicConformer model files, packaged into `malayalam-indicconformer-ctc.tar.gz` (SHA256: `976753F720CC2CCB68155F150B74ED0BA200F364D0346590A2D9A01868611C4D`)
- [x] Configured target dependencies (`ort` with `directml` and `ndarray` features, `ndarray = "0.17"`) in `Cargo.toml`.
- [x] Implemented native Rust ASR pipeline (`malayalam_asr.rs`): pre-emphasis, framing, STFT, Hann window, Slaney-normalized mel filterbank, ONNX session with DirectML/CPU fallback, CTC decoding.
- [x] Wired ASR engine in backend (`lib.rs`, `managers/model.rs`, `managers/transcription.rs`).
- [x] Configured frontend UI model filters (`Onboarding.tsx`, `ModelsSettings.tsx`, `AccelerationSelector.tsx`).
- [x] Resampled test WAV file `audio_16k.wav` for compatibility with Rust `hound`.
- [x] Verified all 66 backend unit and integration tests successfully pass.
- [x] Run `npm install --ignore-scripts` and verify the TypeScript/Vite frontend builds successfully.
- [ ] Build the production package (`npm run tauri build`).

## Critical Environment Context (For New Chat)

> [!IMPORTANT]
> **Compilation Environment**:
> 1. **Cargo Target Directory**: The C: drive is nearly full (< 1GB). To prevent disk-full errors and bypass Windows path length limits (MAX_PATH), you **must** set `CARGO_TARGET_DIR` to a short path on the D: drive:
>    ```powershell
>    $env:CARGO_TARGET_DIR="D:\t"
>    ```
> 2. **Vulkan SDK**: The Vulkan SDK is installed at `C:\VulkanSDK\1.4.350.0`. You **must** set the `VULKAN_SDK` environment variable when running cargo commands:
>    ```powershell
>    $env:VULKAN_SDK="C:\VulkanSDK\1.4.350.0"
>    ```
> 3. **Run Commands Example**:
>    ```powershell
>    $env:CARGO_TARGET_DIR="D:\t"; $env:VULKAN_SDK="C:\VulkanSDK\1.4.350.0"; cargo check
>    ```
> 4. **No Bun Dependency**:
>    Since `bun` is not installed on the system, do **not** run standard `npm install` or scripts that invoke `bun`. Instead, run:
>    ```powershell
>    npm install --ignore-scripts
>    ```
>    And run development/build commands using `npm` (e.g. `npm run build`, `npm run tauri dev`) instead of `bun`.

---

## Decision Log (from interview)

| # | Decision | Choice |
|---|----------|--------|
| 1 | ORT dependency | Standalone `ort` crate directly in Cargo.toml, independent of `transcribe-rs` internals |
| 2 | Mel preprocessing | Pure Rust using `rustfft` (already a dep) + `ndarray` — exact librosa parity |
| 3 | Model catalog | Keep all existing Handy models in backend; **filter in frontend only** |
| 4 | Engine wiring | Add `EngineType::MalayalamIndicConformerCTC` to enum; extend match arms |
| 5 | Archive format | Convert local `.zip` → `.tar.gz` before hosting; reuse Handy's existing tar.gz extractor |
| 6 | GPU provider | DirectML on Windows (already provided by `ort-directml` feature) |
| 7 | Parity test | Run Rust inference on `audio.wav`, log transcript, visual inspection — no automated string comparison |
| 8 | Local model zip | Keep zip; create a helper script to convert to `.tar.gz` and compute SHA256 |
| 9 | Test strategy | Leave existing Handy tests intact; add new tests only for Malayalam-specific code |
| 10 | Test model files | Copy model files into MASR's Handy app-data `models/` directory to exercise full path flow |
| 11 | Onboarding UX | Show Malayalam model card; user presses Download explicitly |
| 12 | Settings hiding | Frontend filter on `supports_translation`, `supported_languages`, etc. — no Rust changes |

---

## Open Questions

> [!IMPORTANT]
> **Public model URL**: The hosted `.tar.gz` URL does not exist yet. A clearly named placeholder constant
> (`MALAYALAM_MODEL_URL`) will be used. Release packaging is blocked until replaced.
> Once you host the archive, update `MALAYALAM_MODEL_URL` in `model.rs` and re-run SHA256 check.

> [!NOTE]
> **SHA256 of `.tar.gz`**: The plan SHA (`B9F8ED51...`) is for the original `.zip`. After the zip-to-tarball
> conversion script runs, a new SHA256 must be recorded and used in `model.rs`.

> [!NOTE]
> **`ort` version compatibility**: `transcribe-rs 0.3.8` uses its own `ort` internally. Adding a standalone
> `ort 2.0.0-rc.9` (the latest stable rc on crates.io) may result in two ORT copies linked into the binary.
> This is acceptable for correctness but will increase binary size by ~5–10 MB. Monitor for symbol conflicts
> in the linker; if conflicts arise, pin `ort` to the exact version `transcribe-rs` uses.

---

## Proposed Changes

### 1 — New Rust module: `malayalam_asr.rs`

#### [NEW] `src-tauri/src/malayalam_asr.rs`

This is the core new file. Responsibilities:

- **`MalayalamAsr` struct**: holds `ort::Session`, vocab map, blank_id, features_size, sample_rate.
- **`MalayalamAsr::load(model_dir: &Path) -> Result<Self>`**:
  - Reads `config.json` → sample_rate, features_size.
  - Reads `vocab.txt` using the same two-column `token index` or bare-token format as Python.
  - Sets `blank_id = vocab.len()` (= 5632 for IndicConformer).
  - Creates `ort::Session` from `model.onnx` (sibling `.onnx.data` file is auto-loaded by ORT).
  - On Windows, attempts DirectML execution provider; falls back to CPU.
- **`MalayalamAsr::transcribe(audio: &[f32]) -> Result<String>`**:
  1. **Pre-emphasis**: `s[0]; s[i] - 0.97 * s[i-1]` for i ≥ 1.
  2. **Mel spectrogram** (pure Rust, matches librosa exactly):
     - `n_fft=512`, `hop_length=160`, `win_length=400`, centered framing (pad by `n_fft/2` on both ends).
     - Hann window.
     - Power spectrum (magnitude²).
     - Slaney-normalized mel filterbank: `n_mels=80`, `fmin=0`, `fmax=8000`, HTK=false.
     - `log(mel + 1e-9)`.
  3. **Per-band normalization**: mean & ddof=1 std across time axis, `(x - mean) / (std + 1e-9)`.
  4. **Shape**: `[1, 80, T]` as `f32`.
  5. **ONNX run**: inputs `audio_signal: [1, 80, T]`, `length: [T]` (i64).
  6. **CTC decode**: argmax over vocab axis → deduplicate consecutive → remove blank → map ids to tokens → join → replace `▁` with space → normalize whitespace.
- **Unit tests** (in `#[cfg(test)]` block within the file):
  - `test_load_vocab_two_column_format` — synthetic vocab string, verifies index mapping.
  - `test_ctc_decode_dedup_and_blank_removal` — hand-crafted logits tensor.
  - `test_mel_tensor_shape` — 1 second of silence → assert output shape is `[1, 80, 101]`.
  - `test_inference_on_audio_wav` — loads `D:\Downloads\Projects\Asr malayalam\audio.wav`, runs full inference, logs transcript (visual inspection only, no assertion).

---

### 2 — Cargo.toml additions

#### [MODIFY] `src-tauri/Cargo.toml`

```toml
# New entries in [dependencies]
ort = { version = "2.0.0-rc.9", features = ["directml"] }
ndarray = "0.16"

# New entry in [dev-dependencies]  (already has tempfile = "3")
hound = "3.5.1"   # re-export or add if not already accessible in test scope
```

> [!WARNING]
> `ort` must be scoped to Windows-only with `[target.'cfg(windows)'.dependencies]` to avoid
> pulling in an unneeded copy on macOS/Linux builds (which don't need this module at all).
> Use `#[cfg(windows)]` guards around the entire `malayalam_asr` module in `lib.rs`.

---

### 3 — Model catalog addition

#### [MODIFY] `src-tauri/src/managers/model.rs`

Add one new `EngineType` variant and one new model entry.

**In `EngineType` enum:**
```rust
pub enum EngineType {
    // ... existing variants unchanged ...
    MalayalamIndicConformerCTC,   // <-- add
}
```

**In `ModelManager::new()`** — append after the `cohere-int8` entry:

```rust
const MALAYALAM_MODEL_URL: Option<&str> = None; // TODO: replace with hosted .tar.gz URL before release

available_models.insert(
    "malayalam-indicconformer-ctc".to_string(),
    ModelInfo {
        id: "malayalam-indicconformer-ctc".to_string(),
        name: "Malayalam IndicConformer CTC".to_string(),
        description: "Malayalam speech recognition. High accuracy.".to_string(),
        filename: "malayalam-indicconformer-ctc".to_string(),  // directory name after extraction
        url: MALAYALAM_MODEL_URL.map(String::from),
        sha256: Some(
            "TODO_INSERT_TARBALL_SHA256_AFTER_RUNNING_CONVERSION_SCRIPT".to_string(),
        ),
        size_mb: 950,   // approximate; update after tarball is built
        is_downloaded: false,
        is_downloading: false,
        partial_size: 0,
        is_directory: true,
        engine_type: EngineType::MalayalamIndicConformerCTC,
        accuracy_score: 0.85,
        speed_score: 0.60,
        supports_translation: false,
        is_recommended: true,
        supported_languages: vec!["ml".to_string()],
        supports_language_selection: false,
        is_custom: false,
    },
);
```

**Archive normalization note**: The extracted directory must contain exactly `model.onnx`, `model.onnx.data`, `vocab.txt`, `config.json` at its root. The conversion script (see §6) will produce this layout from the zip.

No existing model entries are removed. The `is_recommended: true` field on this model and `is_recommended: false` on all others ensures the frontend onboarding recommends it.

---

### 4 — Transcription wiring

#### [MODIFY] `src-tauri/src/managers/transcription.rs`

**Imports** — add:
```rust
#[cfg(windows)]
use crate::malayalam_asr::MalayalamAsr;
```

**`LoadedEngine` enum** — add variant:
```rust
enum LoadedEngine {
    // ... existing ...
    #[cfg(windows)]
    MalayalamIndicConformerCTC(MalayalamAsr),
}
```

**`load_model()` match arm** — add:
```rust
EngineType::MalayalamIndicConformerCTC => {
    #[cfg(windows)]
    {
        let engine = MalayalamAsr::load(&model_path).map_err(|e| {
            let error_msg = format!("Failed to load Malayalam ASR model {}: {}", model_id, e);
            emit_loading_failed(&error_msg);
            anyhow::anyhow!(error_msg)
        })?;
        LoadedEngine::MalayalamIndicConformerCTC(engine)
    }
    #[cfg(not(windows))]
    {
        return Err(anyhow::anyhow!("Malayalam ASR is only supported on Windows"));
    }
}
```

**`transcribe()` match arm** — add:
```rust
LoadedEngine::MalayalamIndicConformerCTC(asr) => {
    asr.transcribe(audio)
        .map(|text| transcribe_rs::TranscriptionResult { text, ..Default::default() })
        .map_err(|e| anyhow::anyhow!("Malayalam ASR transcription failed: {}", e))
}
```

> [!NOTE]
> `TranscriptionResult` may not implement `Default`. Adjust to construct it with the minimum required fields
> (typically `text` + empty `segments`). Confirm by checking `transcribe-rs` source.

**No changes** to: VAD, AudioRecordingManager, clipboard paste, history, tray lifecycle, filler-word filter, custom-words correction (though custom_words correction will be a no-op since the model is not Whisper — this is correct behavior).

---

### 5 — `lib.rs` module declaration

#### [MODIFY] `src-tauri/src/lib.rs`

Add after the existing `mod` declarations:
```rust
#[cfg(windows)]
mod malayalam_asr;
```

---

### 6 — Archive conversion helper script

#### [NEW] `scripts/make_malayalam_tarball.ps1`

A PowerShell script that:
1. Extracts `indicconformer_ml_ctc_onnx.zip` (located in `D:\Downloads\Projects\Asr malayalam\`).
2. Normalizes the extracted content to contain only `model.onnx`, `model.onnx.data`, `vocab.txt`, `config.json` inside a root directory named `malayalam-indicconformer-ctc/`.
3. Creates `malayalam-indicconformer-ctc.tar.gz` in the MASR project root.
4. Prints the SHA256 of the resulting tarball to stdout.

Run this script once before hosting. Copy the SHA256 output into `model.rs`.

---

### 7 — Frontend changes

#### [MODIFY] `src/` — onboarding model list

Add `i18n` translation key for the new model. No other frontend file changes required because:

- The existing model cards use `model.id` for routing; the new model will appear automatically.
- `is_recommended: true` drives the default selection in onboarding.
- Existing models remain in catalog (backend) but the frontend should filter the model list to show only `supported_languages.includes("ml")` models OR all models where `is_recommended`.

#### Frontend filter strategy (decision #12)
- **Translation toggle**: hide if `!model.supports_translation` (already false for new model).
- **Language selector**: hide if `!model.supports_language_selection` (already false).
- **Whisper accelerator setting**: hide if `model.engine_type !== 'Whisper'` — this requires a `model.engine_type` field to be exposed to the frontend via specta. Check if it already is; if not, add it to the TS bindings.

> [!NOTE]
> Check `src/` for the existing settings panel components and onboarding model selection component to
> confirm where the filter logic should be added. No structural redesign is needed.

---

## Test Plan

### Rust unit tests (in `malayalam_asr.rs`)
- `test_load_vocab_two_column_format` — vocab parsing
- `test_ctc_decode_dedup_and_blank_removal` — CTC decode logic
- `test_mel_tensor_shape` — 1-sec silence → `[1, 80, 101]` shape
- `test_inference_on_audio_wav` — visual log output on `audio.wav`

### Rust integration test (in `model.rs` or separate integration test)
- `test_malayalam_model_path` — confirms `get_model_path("malayalam-indicconformer-ctc")` works once files are in models dir

### Build verification
```powershell
cargo test -p handy             # Run from src-tauri/
bun run build                   # Full Tauri build
```

### Manual acceptance
1. Fresh install → onboarding shows Malayalam model card.
2. User clicks Download → progress events visible in tray / overlay.
3. SHA256 verified → model auto-selected.
4. Press `Ctrl+Alt+Space` → record Malayalam speech → transcript pasted correctly.

---

## Files Summary

| Action | File |
|--------|------|
| **NEW** | `src-tauri/src/malayalam_asr.rs` |
| **NEW** | `scripts/make_malayalam_tarball.ps1` |
| **MODIFY** | `src-tauri/Cargo.toml` — add `ort`, `ndarray` |
| **MODIFY** | `src-tauri/src/lib.rs` — add `mod malayalam_asr` |
| **MODIFY** | `src-tauri/src/managers/model.rs` — add `EngineType::MalayalamIndicConformerCTC`, new model entry |
| **MODIFY** | `src-tauri/src/managers/transcription.rs` — add `LoadedEngine` variant + match arms |
| **MODIFY** | `src/` — i18n key + frontend model filter logic |
