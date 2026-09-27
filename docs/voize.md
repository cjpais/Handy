# Voize

A private fork of [Handy](https://github.com/cjpais/Handy), the offline
speech-to-text desktop app, being adapted into a **fully local English voice
conversation agent** while keeping ordinary dictation as a separate mode.

Upstream Handy is a Tauri 2 desktop app (Rust backend + React/TypeScript
frontend) that transcribes speech entirely on-device and pastes the text into
whatever application has focus. See [the upstream README](../README.md) for
feature documentation, [BUILD.md](../BUILD.md) for platform build prerequisites,
and [CONTRIBUTING.md](../CONTRIBUTING.md) before making a contribution.

> **Status:** the conversation pipeline is **planned, not implemented**. The code
> base today is upstream Handy 0.9.7 with local context and documentation files
> added. See [`context/masterplan.md`](../context/masterplan.md) for the roadmap.

---

## Documentation map

Read in roughly this order when getting oriented.

| Document | What it covers |
| --- | --- |
| [README.md](../README.md) | Upstream feature overview, installation, CLI flags, troubleshooting |
| [BUILD.md](../BUILD.md) | Platform-specific build setup and native prerequisites |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | Contribution workflow, code style, commit conventions |
| [CONTRIBUTING_TRANSLATIONS.md](../CONTRIBUTING_TRANSLATIONS.md) | How to add or update a UI translation |
| [AGENTS.md](../AGENTS.md) | Instructions for AI coding assistants (partly stale — see below) |
| [`codebase-v1.md`](../context/references/codebase-v1.md) | **Local-only.** Deep architecture and navigation reference with line-level detail |
| [`CONTEXT.md`](../CONTEXT.md) | **Local-only.** Session memory protocol, project invariants, validation ladder |
| [`masterplan.md`](../context/masterplan.md) | **Local-only.** Mission, proposed architecture, milestones M0–M4 |
| [`progress.md`](../context/progress.md) | **Local-only.** What has actually been verified or completed |
| [`hardpoint.md`](../context/hardpoint.md) | **Local-only.** Confirmed traps with causes and safe workarounds |

`CONTEXT.md` and `context/` are deliberately gitignored — they are local working
memory and are **not** part of the published repository. Do not force-add them.
`docs/` (this directory) is intended to be committed.

> **Note on `AGENTS.md`:** it predates parts of the current code. Its model list
> (Whisper Small/Medium/Turbo/Large) is outdated — model selection is
> catalog-driven with 69 entries — and its CLI table omits the headless mode.
> `codebase-v1.md` lists the corrections. When the two disagree, the source wins.

---

## Folder guides

Every significant directory has its own `README.md` describing what it does, the
conventions to follow, and the traps to avoid. Browse them directly, or see the
[index in `docs/README.md`](README.md).

| Folder | Purpose |
| --- | --- |
| [`src/`](../src/README.md) | React/TypeScript frontend (two entry points: main window + overlay) |
| [`src-tauri/`](../src-tauri/README.md) | Rust backend, Tauri config, native resources and packaging |
| [`src-tauri/src/`](../src-tauri/src/README.md) | Backend source: managers, pipeline, commands, platform code |
| [`scripts/`](../scripts/README.md) | Repository tooling and CI data checks |
| [`tests/`](../tests/README.md) | Playwright specs |
| [`.github/`](../.github/README.md) | CI workflows and the mandatory issue/PR templates |
| [`src-tauri/resources/`](../src-tauri/resources/README.md) | Files bundled into the app at build time |
| [`src-tauri/capabilities/`](../src-tauri/capabilities/README.md) | Tauri permission grants per window |
| [`src-tauri/transcribe-libs/`](../src-tauri/transcribe-libs/README.md) | Gitignored, build-staged ggml/transcribe libraries bundled beside the executable |
| [`src-tauri/swift/`](../src-tauri/swift/README.md) | Apple Intelligence bridge (macOS aarch64 only) |
| [`src-tauri/nsis/`](../src-tauri/nsis/README.md) | Windows installer template |
| [`nix/`](../nix/README.md) | NixOS / Home Manager packaging |
| [`public/`](../public/README.md) | Static assets copied verbatim into the frontend build |

---

## Quick start

```bash
bun install                                  # also regenerates .nix/bun.nix if needed

# Required once: the VAD model is not committed
mkdir -p src-tauri/resources/models
curl -o src-tauri/resources/models/silero_vad_v4.onnx \
  https://blob.handy.computer/silero_vad_v4.onnx

bun run tauri dev                            # full native app (needs Rust + platform build tools)
```

`bun run dev` starts only Vite on port 1420. It **cannot** exercise the
microphone, transcription or any other native behaviour — use `bun run tauri dev`
for that.

See [`CONTEXT.md`](../CONTEXT.md) for the full validation ladder and
[`environment.md`](../context/references/environment.md) for the verified state of
this machine's toolchain.

---

## Project invariants

These constrain any change to the conversation feature. Full detail in
[`CONTEXT.md`](../CONTEXT.md).

- Conversation audio and text stay **local**; loopback endpoints only, with no
  cloud fallback. Existing post-processing providers *can* reach remote services
  and must not be reused blindly for conversation.
- Conversation output must **not** be pasted into the active application.
- Ordinary dictation, cancellation and model lifecycle behaviour must be preserved.
- Half-duplex before streaming and barge-in.
- Never commit credentials, transcripts, or machine-specific personal paths.

---

## License

MIT. See [LICENSE](../LICENSE) for details and retain the upstream copyright notice.

This fork has **not** yet been rebranded for distribution: the bundle still uses
upstream's `com.pais.handy` identifier, `Handy` product name, updater endpoint and
signing configuration. Auditing all of those is part of milestone M4.
