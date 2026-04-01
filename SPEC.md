# Voice identifier correction

Feature specification — adjusted for Handy

---

## Problem

Voice-to-text transcription mangles code identifiers, file paths, and variable names — converting them into common English words or phonetically similar but wrong strings. This makes voice-driven coding with CLI agents unreliable for any task requiring precise names.

Example: saying "open file utils dot rs" might transcribe as "open file utilise dot RS" or "open file utils dot are S", when the codebase contains `src/utils.rs`.

---

## Goal

A thin correction layer built into Handy's post-transcription pipeline. It intercepts transcribed text, detects likely identifier references, and replaces them with fuzzy-matched candidates from the active codebase — without owning the search logic and without breaking the dictation flow for normal prose.

---

## Scope

- **In scope:** post-transcription correction of file paths, variable names, function names, class names, and module names within a configured project directory
- **Out of scope:** speech-to-text itself, custom code search indexing, IDE integration, voice command parsing, full voice control of the OS
- **Target environment:** any terminal or CLI agent (Claude Code, Codex, OpenCode, etc.) on Linux, macOS, and Windows via Handy's paste pipeline

---

## Architecture (Handy-specific)

This feature is not a daemon or a separate process. It lives entirely inside Handy as a post-transcription processing step, inserted after the filler-word removal stage:

```
Audio → VAD → Whisper/Parakeet/… → custom word correction
     → filler word removal → identifier correction ← NEW
     → clipboard / paste
```

The identifier correction step:
1. Reads the transcription text.
2. Detects candidate tokens (see Trigger modes).
3. Fuzzy-matches each candidate against a pre-built symbol index.
4. Silently replaces single high-confidence matches.
5. Shows the Handy picker overlay for ambiguous matches and blocks the pipeline (up to 10 s) for user input.
6. Passes the corrected text forward to Handy's paste pipeline unchanged.

---

## Trigger modes

### Explicit trigger
User speaks one of the following signal words immediately before the identifier:

| Spoken word | Intent |
|-------------|--------|
| `file`      | Next word is a file name or path |
| `symbol`    | Next word is a code symbol (function, class, variable) |
| `function`  | Next word is a function name |
| `class`     | Next word is a class name |
| `method`    | Next word is a method name |
| `variable`  | Next word is a variable name |
| `module`    | Next word is a module or package name |
| `package`   | Next word is a package name |

The trigger word itself is removed from the output.  Example:

> Spoken: `"open file utils"` → corrected: `"open utils.rs"`

### Automatic detection
Tokens are automatically flagged as identifier candidates when **all** of the following hold:

1. Length is 3–40 characters.
2. Not a pure number or punctuation.
3. Not all-uppercase (acronym like HTTP, API).
4. Not in a ~250-word list of common English words.
5. One or more of:
   - Preceded by a code-context word: `open`, `edit`, `rename`, `delete`, `create`, `import`, `require`, `call`, `define`, `implement`, etc.
   - Has camelCase, PascalCase, or snake_case structure.

Normal prose passes through unchanged because every common English word is excluded before fuzzy matching runs.

---

## Symbol index

### What is indexed
- **File paths** — all non-hidden files within the project root, collected with `fd` (or `find` as fallback). Both the full relative path (`src/utils.rs`) and the bare stem (`utils`) are indexed.
- **Code symbols** — function, class, struct, interface, enum, trait, and variable declarations, extracted with `ripgrep` using language-agnostic patterns. Common language keywords are filtered out.

### Index size
- Capped at 15 000 entries to keep memory footprint predictable.
- Symbols shorter than 2 characters are dropped.

### Index lifecycle
- Built in a background thread at startup if a project root is configured.
- Rebuilt on demand via the **Index** button in settings or the `rebuild_identifier_index` Tauri command.
- Not automatically refreshed on file change in this prototype (planned for a future iteration).

### Required tools
- `fd` — fast, `.gitignore`-aware file enumeration. Falls back to `find`.
- `rg` (ripgrep) — symbol extraction. Optional; if missing, only file paths are indexed.
- `fzf` — optional pre-filter for large indices. Falls back to in-process Levenshtein scan if absent.

---

## Correction behaviour

### Matching
Each candidate token is matched against the index using:
1. **fzf pre-filter** (if available) — pipes the index through `fzf --filter=<token> --algo=v2`, returns the top 20 hits.  Fast even on large indices (~5 ms).
2. **Levenshtein scoring** — `normalized_levenshtein` from the `strsim` crate, with the path extension stripped before comparison.
3. **Soundex phonetic boost** — +0.15 score bonus when the Soundex code of the query matches the candidate's stem.

### Decision thresholds

| Score | Action |
|-------|--------|
| ≥ 0.85 and clearly better than the second candidate (gap > 0.15) | **Silent substitution** — no user interaction |
| ≥ configured threshold (default 0.60) | **Picker** — emit `identifier-pick-needed` event; wait up to 10 s |
| < threshold | **Pass through** — original token unchanged |

### Picker overlay
When the backend determines that one or more tokens need user confirmation, it emits a single `identifier-pick-needed` event containing all ambiguous tokens in one batch. The Handy picker overlay appears above the settings window:

- Shows one card per ambiguous token.
- Displays ranked replacement candidates as buttons.
- "Keep original" option available for each token.
- 10-second countdown auto-applies the top candidate per token on expiry.
- User can click **Apply** to confirm all selections at once or **Skip all** to pass everything through unchanged.
- The selected (or default) replacements are sent back to the backend via `confirm_identifier_pick`, unblocking the pipeline.

---

## Settings

All settings are persisted in Handy's store alongside other app settings.

| Setting | Default | Description |
|---------|---------|-------------|
| `identifier_correction_enabled` | `false` | Master toggle |
| `identifier_correction_project_root` | `null` | Absolute path to the project root to index |
| `identifier_correction_threshold` | `0.60` | Minimum match score (0.0–1.0); lower = more aggressive |

Settings are exposed in **Advanced → Transcription** in the Handy settings window.

---

## Implementation files

### Backend (Rust)

| File | Role |
|------|------|
| `src-tauri/src/identifier_correction.rs` | `IdentifierCorrectionManager`, symbol extraction, detection, matching, picker wait |
| `src-tauri/src/commands/identifier_correction.rs` | `confirm_identifier_pick`, `rebuild_identifier_index`, `set_identifier_correction_settings`, `get_identifier_index_size` |
| `src-tauri/src/settings.rs` | 3 new fields on `AppSettings` |
| `src-tauri/src/managers/transcription.rs` | Integration call after `filter_transcription_output` |
| `src-tauri/src/lib.rs` | Manager init, command registration, event registration |

### Frontend (TypeScript/React)

| File | Role |
|------|------|
| `src/components/identifier-picker/IdentifierPicker.tsx` | Picker overlay — listens for `identifier-pick-needed`, shows candidates, sends selection back |
| `src/components/settings/IdentifierCorrectionSettings.tsx` | Settings UI — enable toggle, project root input, rebuild button, threshold slider |
| `src/components/settings/advanced/AdvancedSettings.tsx` | Mounts settings component in the Transcription group |
| `src/App.tsx` | Mounts `<IdentifierPicker />` at root level |
| `src/i18n/locales/en/translation.json` | New translation keys under `identifierPicker` and `settings.advanced.identifierCorrection` |

---

## Platform notes

The correction layer is fully platform-agnostic. It runs inside the Handy process on all platforms and inherits Handy's paste pipeline:

| Platform | Paste | Notes |
|----------|-------|-------|
| Linux | wl-copy / xclip / wtype / xdotool | Same tools Handy already uses |
| macOS | pbcopy / AppleScript / CGEvent | No additional requirements |
| Windows | Win32 clipboard / Enigo | Works in WSL terminals via Handy's Direct mode |

No clipboard monitoring and no OS-level keyboard hooks are added. Correction happens before the text reaches the paste layer.

---

## Non-goals (unchanged from original spec)

- Not a voice control system — does not execute commands, navigate the OS, or replace a keyboard.
- Not a grammar or prose corrector — only corrects identifiers and paths.
- Not a plugin for any specific editor or agent — works at the Handy output level.
- Not a replacement for the custom words feature — that feature serves personal dictionaries; this serves codebase symbols.

---

## Success criteria

- Correct identifier substitution with no user interaction in the common case.
- Picker surfaces within one render cycle (~16 ms) of the backend emitting the event.
- Zero false positives on normal prose — common English words are never flagged.
- Works with all STT models supported by Handy on Linux without tool-specific configuration.
- No detectable latency increase for transcriptions with no identifier candidates.

---

## Future work

- File-change watcher to incrementally update the symbol index without a full rebuild.
- LSP integration for higher-quality symbol extraction (types, parameter names, imports).
- Multi-word correction (e.g., "parse args" → `parse_args`).
- Per-project index persistence so the index survives app restarts.
- Confidence tuning UI with a live preview of what would be corrected on sample text.
