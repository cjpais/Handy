# `src-tauri/resources/` — bundled application resources

Files copied into the built application and resolved at runtime via Tauri's
`BaseDirectory::Resource`. Declared in `tauri.conf.json` as
`"resources": ["resources/**/*"]`, and remapped next to the executable on Windows
by `tauri.windows.conf.json`.

Anything added here is shipped inside the installer, so keep it small. Reference
these files in Rust through `app.path().resolve(path, BaseDirectory::Resource)`.

---

## Contents

| Path | Purpose |
| --- | --- |
| `default_settings.json` | Baseline settings shipped with the app |
| `models/silero_vad_v4.onnx` | **Required.** Silero VAD model used for voice activity detection |
| `models/gigaam_vocab.txt` | Vocabulary file for the GigaAM engine |
| `handy.png`, `handy_warning.png` | App artwork used in UI prompts |
| `recording.png`, `transcribing.png` | Tray/overlay state artwork |
| `tray_idle*.png`, `tray_recording*.png`, `tray_transcribing*.png` | Tray icons, each in light and `_dark` variants plus a `_warning` idle variant |
| `marimba_start.wav`, `marimba_stop.wav` | "Marimba" audio feedback theme |
| `pop_start.wav`, `pop_stop.wav` | "Pop" audio feedback theme |

Tray icon selection is theme-aware and driven by `tray.rs`
(`get_icon_path(theme, TrayIconState, warning)`). If you add a state or a theme,
update both the icon set and that function — a missing file panics at startup
because the icon is loaded with `.unwrap()` during tray construction.

---

## The VAD model must be downloaded before running

`models/silero_vad_v4.onnx` is **not committed**. A fresh clone cannot record
audio until it exists:

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx \
  https://blob.handy.computer/silero_vad_v4.onnx
```

Unlike the speech-to-text models — which download at runtime into the app data
directory and are managed through the UI — this one is a **build-time** resource,
because VAD is needed before any model is selected.

`models/gigaam_vocab.txt` **is** committed; do not delete it.

---

## Audio feedback themes

`audio_feedback.rs` plays these through `rodio`. The theme is a user setting
(`SoundTheme`: `marimba | pop | custom`). Selecting `custom` makes the app look
for `custom_start.wav` / `custom_stop.wav` in the **app data directory**, not here
(`commands::audio::check_custom_sounds`), so users can supply their own without
modifying the bundle.

These WAVs are short feedback chimes only. There is no TTS or general audio-output
pipeline in this codebase.

---

## Notes

- Files here are subject to the project license review before distribution. The
  Silero model and any model assets carry their own licenses — see
  `context/masterplan.md` (milestone M4) before shipping Voize.
- Runtime-loaded models (69 catalog entries) do **not** live here. They download
  into `<app_data>/models`, or `Data/models` in portable mode.
- Do not place user-editable defaults here: resources are replaced on upgrade.
