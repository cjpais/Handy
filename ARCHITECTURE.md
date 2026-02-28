# Handy Architecture Notes

## OCR Context Providers

The `${OCR}` / `${ocr}` prompt template variable is resolved in `src-tauri/src/actions.rs` through platform-specific OCR providers.

- macOS: `src-tauri/src/macos_ocr.rs` and Swift bridge files under `src-tauri/swift/`.
- Windows: `src-tauri/src/windows_ocr.rs` using Windows Media OCR APIs.
- Linux: `src-tauri/src/linux_ocr.rs` using backend selection at startup:
  - X11 session: X11 active-window capture + `tesseract` CLI OCR.
  - Wayland session: desktop screenshot CLI fallback (`gnome-screenshot` or `spectacle`) + `tesseract`.

## Linux OCR Flow

1. On app startup (`initialize_core_logic`), Linux OCR detects:
   - Session type (`XDG_SESSION_TYPE`)
   - Desktop environment (`XDG_CURRENT_DESKTOP` / `DESKTOP_SESSION`)
   - Available Wayland screenshot tools (`gnome-screenshot`, `spectacle`)
2. Capture strategy is selected once and cached:
   - X11/unknown session -> X11 capture path.
   - Wayland -> screenshot command fallback path (desktop-preferred tool first).
3. Capture pipeline:
   - X11 path: resolve active window (`_NET_ACTIVE_WINDOW`), capture via `XGetImage`, encode temporary PPM.
   - Wayland path: execute screenshot tool to a temporary PNG.
4. Execute `tesseract <image> stdout --psm 6`.
5. Return OCR text to `actions.rs`, where OCR text is truncated to `MAX_OCR_TEXT_CHARS` before prompt expansion.

## Packaging and Build Requirements

- Linux build workflow installs `tesseract-ocr` in `.github/workflows/build.yml`.
- Debian bundle metadata includes `tesseract-ocr` dependency in `src-tauri/tauri.conf.json`.
