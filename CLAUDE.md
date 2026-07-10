# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Canonical guidance

**[AGENTS.md](AGENTS.md) is the single source of truth** for this repo — read it first. It covers development commands, backend/frontend architecture, the manager + command-event patterns, the audio → VAD → transcription pipeline, i18n rules, CLI flags, the single-instance model, and platform notes. Do not duplicate that content here; edit AGENTS.md instead. [BUILD.md](BUILD.md) has platform-specific build setup and troubleshooting.

## Quick reference

```bash
bun install                       # JS deps
bun run tauri dev                 # run full app (Rust + Vite)
bun run lint && bun run format:check   # before committing
bunx tsc --noEmit                 # frontend type check
```

Required before first dev run — download the VAD model (backend fails to init without it):

```bash
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

There is no frontend/JS test suite here — `bun run test:playwright` drives the app end-to-end via Playwright (see `playwright.config.ts`, `tests/`). Rust tests: `cd src-tauri && cargo test`.

## Windows gotchas (learned running this repo)

These are not in BUILD.md's happy path and cost real time to rediscover:

- **`bun install` can leave transitive packages as empty dirs** (seen with `debug`/`ms`, pulled in by `micromark` → `react-markdown`), causing Vite to fail with `Could not resolve "debug"`. `bun install --force` alone does **not** fix it — the corruption is in bun's global cache. Fix: `bun pm cache rm`, delete the empty dirs under `node_modules/`, then `bun install --force`.
- **260-char path limit (`MSB3491`)** hits the Vulkan shader build when the repo lives under a deep path. Build with a short target dir: `export CARGO_TARGET_DIR="C:/h"` before `bun run tauri dev` / `tauri build`. (Alternative: enable Windows long paths — see BUILD.md troubleshooting.)
- The first `tauri dev` fetches a **tauri fork** (`github.com/cjpais/tauri.git`) and compiles whisper/onnx/Vulkan from source — the initial build is long (tens of minutes); subsequent builds are incremental.
