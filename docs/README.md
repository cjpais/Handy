# Fork-local documentation

**This directory is not part of upstream Handy.** It exists so that this fork's
architecture notes and folder guides are easy to find from the repository root,
without turning every source directory into an upstream-facing file.

Upstream-facing documentation lives at the repository root:
[README.md](../README.md), [BUILD.md](../BUILD.md),
[CONTRIBUTING.md](../CONTRIBUTING.md), [AGENTS.md](../AGENTS.md).

---

## Contents

| File | What it covers |
| --- | --- |
| [`voize.md`](voize.md) | **Start here.** What this fork is, the documentation map, current status, quick start, project invariants |

## Folder guides

Each significant directory in the repository has a `README.md` describing what it
does, the conventions to follow, and the traps to avoid. They are discoverable by
browsing, and indexed here for convenience.

| Folder | Purpose |
| --- | --- |
| [`../src/`](../src/README.md) | React/TypeScript frontend — two entry points (main window + overlay) |
| [`../src-tauri/`](../src-tauri/README.md) | Rust backend, Tauri config, native resources and packaging |
| [`../src-tauri/src/`](../src-tauri/src/README.md) | Backend source: managers, pipeline, commands, platform code |
| [`../scripts/`](../scripts/README.md) | Repository tooling and CI data checks |
| [`../tests/`](../tests/README.md) | Playwright specs and what they can/cannot verify |
| [`../.github/`](../.github/README.md) | CI workflows and the mandatory issue/PR templates |
| [`../src-tauri/resources/`](../src-tauri/resources/README.md) | Files bundled into the app at build time |
| [`../src-tauri/capabilities/`](../src-tauri/capabilities/README.md) | Tauri permission grants per window |
| [`../src-tauri/transcribe-libs/`](../src-tauri/transcribe-libs/README.md) | Gitignored, build-staged ggml/transcribe libraries bundled beside the executable |
| [`../src-tauri/swift/`](../src-tauri/swift/README.md) | Apple Intelligence bridge (macOS aarch64 only) |
| [`../src-tauri/nsis/`](../src-tauri/nsis/README.md) | Windows installer template |
| [`../nix/`](../nix/README.md) | NixOS / Home Manager packaging |
| [`../public/`](../public/README.md) | Static assets copied verbatim into the frontend build |

Generated or vendored directories deliberately have no README: `node_modules/`,
`dist/`, `src-tauri/target/`, `src-tauri/gen/`, `src-tauri/icons/`,
`sponsor-images/`.

---

## Relationship to `context/`

`context/` and `docs/` serve different audiences and should not be merged:

- **`docs/`** (here) — durable architecture and navigation documentation written
  for any reader. Stable, describes how the code works.
- **[`../context/`](../context/)** — local session memory. Contains the session
  protocol, the roadmap, the progress log, confirmed traps, and
  [`codebase-v1.md`](../context/references/codebase-v1.md), the deep architecture
  reference. Written for future work sessions, includes unverified state and
  machine-specific notes.

`CONTEXT.md` and `context/` are gitignored and must not be force-added. `docs/`
is intended to be committed.

When the two disagree about how the code works, prefer
[`codebase-v1.md`](../context/references/codebase-v1.md), which carries line-level
references, and treat these folder guides as the summary.

---

## Maintenance

These guides describe the code as it is, not as it was or as it is planned to be.
If you change a folder's structure or conventions, update its README in the same
change. If a claim here contradicts the source, the source wins — fix the README
rather than trusting it.

Folder guides are written to be verifiable: prefer naming real files, types and
commands over prose. Do not document planned work as though it exists.
