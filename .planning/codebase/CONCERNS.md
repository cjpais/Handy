# Codebase Concerns

**Analysis Date:** 2026-03-28

---

## Tech Debt

**Model registry hardcoded in Rust source:**
- Issue: All downloadable model definitions (names, URLs, SHA256s, descriptions, scores) are hardcoded in a giant `HashMap::insert` block rather than read from a JSON/TOML config file.
- Files: `src-tauri/src/managers/model.rs:124` — `// TODO this should be read from a JSON file or something..`
- Impact: Adding or updating a model requires a full Rust recompile and new release. URLs and checksums are buried in code, increasing risk of stale/incorrect values going unnoticed.
- Fix approach: Extract model registry to `src-tauri/resources/models.json`; load at startup via `serde_json`.
- Severity: **Medium**

**Pervasive `.unwrap()` on Mutex locks in managers:**
- Issue: Nearly every `Mutex::lock()` call in `src-tauri/src/managers/audio.rs` uses `.unwrap()` (lines 200, 227, 247, 249, 267, 276, 287, 326, 337, 339, 341, 353, 357, 369, 376, 380, 390, 392, 408, 417, 436, 449, 452, 476, 489, 493, 496). A poisoned mutex (panic in another thread holding the lock) will crash the entire app.
- Files: `src-tauri/src/managers/audio.rs`, `src-tauri/src/managers/model.rs:78,83`
- Impact: Any panic anywhere in a lock-holding code path causes an unrecoverable crash on next lock acquisition.
- Fix approach: Replace `.unwrap()` with `.unwrap_or_else(|e| e.into_inner())` for poison recovery, or structured error propagation.
- Severity: **Medium**

**`.unwrap()` in settings serialization path:**
- Issue: `serde_json::to_value(&settings).unwrap()` is called at least 8 times in `src-tauri/src/settings.rs` (lines 822, 831, 837, 842, 856, 861, 866, 877). Settings are a well-known struct so failure is unlikely, but any future addition of a non-serializable field will panic silently.
- Files: `src-tauri/src/settings.rs`
- Fix approach: Propagate the error with `?` or log and fall back gracefully.
- Severity: **Low**

**Unchecked shortcut binding lookup:**
- Issue: `src-tauri/src/settings.rs:889` uses `bindings.get(id).unwrap()` — panics if an unknown shortcut ID is passed.
- Files: `src-tauri/src/settings.rs:889`
- Fix approach: Return a `Result` or `Option` and handle the missing-key case.
- Severity: **Low**

---

## Known Issues

**Portable mode migration is fire-and-forget:**
- Issue: `src-tauri/src/portable.rs:29-31` silently upgrades a legacy empty portable marker. If the write fails (read-only filesystem, permissions issue), the upgrade is silently skipped via `let _ = ...` and the user may be unaware their portable mode marker was not upgraded.
- Files: `src-tauri/src/portable.rs:25-31`
- Impact: On next launch without the `Data/` dir present alongside the empty marker, portable mode will not activate, causing data to be written to `%APPDATA%` unexpectedly.
- Fix approach: Log an explicit warning if the marker write fails; surface in UI or startup log.
- Severity: **Medium**

**Signal handling is Unix-only:**
- Issue: `src-tauri/src/lib.rs:174` wraps `Signals::new(&[SIGUSR1, SIGUSR2]).unwrap()` in a `#[cfg(unix)]` block, but the `.unwrap()` will panic if signal registration fails (e.g., signal already registered by another library).
- Files: `src-tauri/src/lib.rs:174`
- Fix approach: Use `.expect()` with a descriptive message or propagate as an error.
- Severity: **Low**

**Resampler can silently produce wrong output:**
- Issue: `src-tauri/src/audio_toolkit/audio/resampler.rs:54` uses `.unwrap()` in the resampling pipeline. A misconfiguration or unexpected sample format will panic mid-recording.
- Files: `src-tauri/src/audio_toolkit/audio/resampler.rs:54`
- Severity: **Medium**

---

## Security Concerns

**Unsafe FFI blocks for Apple Intelligence:**
- Issue: `src-tauri/src/apple_intelligence.rs` contains multiple `unsafe` blocks (lines 20, 41, 49, 55, 61, 70) interfacing with a native C/ObjC LLM API. Raw pointer manipulation (`&*response_ptr`) and manual memory management (`free_apple_llm_response`) are used without null-pointer checks before dereference.
- Files: `src-tauri/src/apple_intelligence.rs:41-70`
- Impact: A null response pointer from the native API would cause a segfault/undefined behavior.
- Current mitigation: None observed — no null check before `&*response_ptr` on line 49.
- Recommendation: Add explicit null check on `response_ptr` before dereferencing; wrap in a safe abstraction function.
- Severity: **High** (macOS aarch64 only)

**Unsafe Windows overlay topmost via raw HWND:**
- Issue: `src-tauri/src/overlay.rs:119` retrieves a raw `HWND` from the webview window for Windows-specific `SetWindowPos` calls inside an `unsafe` block.
- Files: `src-tauri/src/overlay.rs:109-120`
- Impact: Incorrect HWND usage can lead to undefined behavior or silent failures; currently low risk as it calls standard Win32 APIs.
- Severity: **Low**

**Unsafe audio device initialization:**
- Issue: `src-tauri/src/managers/audio.rs:23` contains an `unsafe` block. Context needed to assess scope, but unsafe in a device-initialization path can cause issues on device hot-plug or unexpected states.
- Files: `src-tauri/src/managers/audio.rs:23`
- Severity: **Low** (needs further audit)

**Model downloads over HTTPS without intermediate verification:**
- Issue: Models are downloaded from `blob.handy.computer` and SHA256 is verified post-download (visible in model registry). However, there is no certificate pinning. A compromised CDN or MITM could serve a modified model file. SHA256 check provides integrity but only after download completes.
- Files: `src-tauri/src/managers/model.rs`
- Severity: **Low** (SHA256 check is present; risk is primarily against active MITM)

---

## Platform Quirks

**Linux overlay: Wayland + KDE disabled:**
- Issue: `src-tauri/src/overlay.rs:82-84` explicitly skips GTK layer shell initialization on Wayland+KDE, falling back to a regular always-on-top window. The overlay will not behave as a true overlay on KDE Wayland.
- Files: `src-tauri/src/overlay.rs:71-100`
- Impact: Users on KDE Wayland get a degraded overlay experience (may appear behind other windows).
- Severity: **Medium** (platform-specific, documented in CLAUDE.md as "overlay disabled by default on Linux")

**Linux overlay: not created by default:**
- Issue: `src-tauri/src/overlay.rs:223` wraps overlay creation in `#[cfg(not(target_os = "linux"))]`, meaning Linux users have no overlay at all unless GTK layer shell is available.
- Files: `src-tauri/src/overlay.rs:223-229`
- Severity: **Low** (known limitation)

**macOS: activation policy must be toggled manually:**
- Issue: `src-tauri/src/lib.rs:98-100` sets `ActivationPolicy::Regular` on macOS only when showing the main window. If the window is hidden (e.g., `--start-hidden`), the app stays as an accessory/background app, which is intentional but fragile — any new window-show code path that misses this call will leave the app with wrong activation policy.
- Files: `src-tauri/src/lib.rs:98-100`
- Severity: **Low**

**Portable mode is Windows-only in practice:**
- Issue: The portable mode feature (`src-tauri/src/portable.rs`) is generic but the use-case (storing data next to the exe) is primarily relevant on Windows (Scoop installs). On macOS/Linux, exe path conventions differ and writing alongside the binary in `/Applications` or `/usr/bin` would fail.
- Files: `src-tauri/src/portable.rs`
- Impact: Not a bug, but the feature is untested/not useful on non-Windows platforms.
- Severity: **Low**

---

## Missing Coverage

**No tests for AudioRecordingManager or TranscriptionManager:**
- What's not tested: The core recording state machine (`src-tauri/src/managers/audio.rs`), VAD pipeline, and transcription pipeline (`src-tauri/src/managers/transcription.rs`). These are the most complex and most failure-prone parts of the app.
- Files: `src-tauri/src/managers/audio.rs`, `src-tauri/src/managers/transcription.rs`
- Risk: State machine bugs (e.g., recording stuck in wrong state, mute not restored) go undetected until user reports.
- Priority: **High**

**No tests for settings migration logic:**
- What's not tested: `src-tauri/src/settings.rs` migration paths that upgrade old settings schemas. Regressions would silently reset user settings.
- Files: `src-tauri/src/settings.rs:820-870`
- Risk: A settings schema change that breaks migration causes all user settings to reset on upgrade, with no error surfaced.
- Priority: **High**

**No frontend tests (zero .test.* files detected):**
- What's not tested: All React components, hooks (`src/hooks/`), Zustand stores (`src/stores/`), and i18n wiring.
- Files: `src/` (entire frontend)
- Risk: UI regressions, broken settings forms, incorrect state updates go undetected.
- Priority: **Medium**

**Portable mode: only `is_valid_portable_marker` is unit-tested:**
- What's not tested: The migration branch (empty marker + existing `Data/` dir triggers upgrade), the `app_data_dir`/`app_log_dir`/`store_path` portable overrides.
- Files: `src-tauri/src/portable.rs`
- Risk: Migration path could regress silently; directory resolution could return wrong paths.
- Priority: **Medium**

**No integration tests for CLI flags:**
- What's not tested: `--toggle-transcription`, `--toggle-post-process`, `--cancel` single-instance message passing.
- Files: `src-tauri/src/cli.rs`, `src-tauri/src/signal_handle.rs`
- Risk: CLI remote-control flags could silently break across Tauri or plugin upgrades.
- Priority: **Low**

---

*Concerns audit: 2026-03-28*
