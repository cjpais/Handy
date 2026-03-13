# Dev Log

## 2026-02-21

### Summary
Added Linux OCR support for `${OCR}` / `${ocr}` prompt variables using Tesseract.

### What Changed and Why
- Added `src-tauri/src/linux_ocr.rs`.
  - Captures active window pixels from X11 and performs OCR via `tesseract`.
  - Adds fallback to root window capture when active window capture fails.
  - Returns OCR text to the existing prompt-template expansion pipeline.
- Updated `src-tauri/src/actions.rs`.
  - Wired Linux into `fetch_ocr_template_value`.
  - Kept existing macOS/Windows behavior unchanged.
  - Fixed unit tests to call the current prompt-template helper function.
- Updated `src-tauri/src/lib.rs` and `src-tauri/Cargo.toml`.
  - Registered Linux OCR module.
  - Added Linux `x11` crate dependency.
- Updated Linux build/package config.
  - `.github/workflows/build.yml`: install `tesseract-ocr` on Ubuntu build jobs.
  - `src-tauri/tauri.conf.json`: add Debian dependency on `tesseract-ocr`.

### Verification
- `bun run build` passed on Ubuntu 24.04 ARM64 VM.
- `cd src-tauri && cargo test` passed (`42 passed, 0 failed`).
- `bun run tauri build --bundles appimage,deb,rpm --target aarch64-unknown-linux-gnu --config '{"bundle":{"createUpdaterArtifacts":false}}'` passed.

### Gotchas / Follow-up
- Linux OCR implementation currently targets X11 capture path.
  - Wayland-only sessions may not provide active-window capture through this path.
- Tauri still emits pre-existing bundler warnings about `__TAURI_BUNDLE_TYPE`; no functional change made for that in this task.

### Follow-up Update
Implemented Linux OCR backend selection for Wayland fallback screenshot tools.

### What Changed and Why
- Updated `src-tauri/src/linux_ocr.rs`.
  - Added startup-time backend detection (`XDG_SESSION_TYPE`, desktop environment, command availability).
  - Added cached capture strategy selection:
    - X11/unknown session -> existing X11 capture path.
    - Wayland session -> `gnome-screenshot` or `spectacle` fallback.
  - Added Wayland screenshot capture execution:
    - GNOME: `gnome-screenshot -f <temp.png>`
    - KDE fallback: `spectacle -b -n -o <temp.png>`
  - Added startup warning messages when Wayland has no supported screenshot command.
  - Added unit tests for session parsing, desktop parsing, and Wayland tool selection logic.
- Updated `src-tauri/src/lib.rs`.
  - Initialize Linux OCR backend strategy during app startup (`initialize_core_logic`).

### Verification
- Local:
  - `bun run build` passed.
  - `cd src-tauri && cargo test` blocked by existing macOS Swift Apple Intelligence toolchain issue (`FoundationModelsMacros` unavailable).
- Ubuntu VM:
  - `cd /home/evren/code/Handy/src-tauri && cargo test` passed (`46 passed, 0 failed`).

### Gotchas / Follow-up
- Wayland fallback currently relies on desktop screenshot CLI tools rather than xdg-desktop-portal.
  - This is intentionally pragmatic and should be treated as an interim compatibility path.
- GNOME exposes screenshot capabilities over D-Bus (`org.gnome.Shell.Screenshot`), including window-specific methods.
  - A future Linux improvement could replace CLI screenshot calls with compositor API integration for better active-window behavior and fewer external tool assumptions.
