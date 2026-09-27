# `scripts/` — repository tooling and CI data checks

Standalone scripts that validate or generate repository data. Three are wired into
`package.json`, one runs automatically on install, and two are maintainer tools for
the model catalog. None of them are part of the shipped application.

---

## Checks wired into `package.json`

| Script | Command | What it validates |
| --- | --- | --- |
| `check-translations.ts` | `bun run check:translations` | Every locale is key-complete against `en` |
| `check-model-language-coverage.ts` | `bun run check:model-languages` | Every catalog language maps to exactly one frontend intent |
| `check-nix-deps.ts` | *(postinstall)* `bun scripts/check-nix-deps.ts` | Keeps `.nix/bun.nix` in sync with `bun.lock` |

### `check-translations.ts`

Compares every directory under `src/i18n/locales/` against the `en` reference,
walking nested keys, and reports **missing** and **extra** keys per language with
colourised output. Exits non-zero on failure, so it is safe to use as a gate.

Add a UI string to `en/translation.json` and this will tell you which of the other
26 locales are now incomplete. See
[`../CONTRIBUTING_TRANSLATIONS.md`](../CONTRIBUTING_TRANSLATIONS.md) for the
translation workflow — note that missing keys in non-English locales are expected
during normal development, so read the output rather than assuming any failure is
yours.

### `check-model-language-coverage.ts`

Asserts that every language code in `src-tauri/src/catalog/catalog.json` maps to
**exactly one** entry in `MODEL_CAPABILITY_LANGUAGES`
(`src/lib/constants/languages.ts`). Zero matches means a model language is
unreachable from the UI; multiple matches means the picker is ambiguous.

It also pins the Norwegian alias: intent `no` must remain equivalent to model code
`nb`. That assertion lives here deliberately, next to the catalog coverage check,
so that adding a separate picker entry cannot silently break language continuity
when a user switches between model families.

Run this after editing the catalog or the frontend language constants.

### `check-nix-deps.ts`

Keeps `.nix/bun.nix` (per-package Nix `fetchurl` expressions) in sync with
`bun.lock` via `bun2nix`. It hashes `bun.lock`, compares against the stored hash in
`.nix/bun-lock-hash`, and regenerates only when they differ — so the common case
costs about 2 ms.

Behaviour worth knowing:

- **Skips entirely on Windows** (exit 0). `bun2nix` is Nix-only and hangs on
  Windows, so a Windows contributor sees nothing.
- **Exits 0 even if `bun2nix` fails**, deliberately, so a broken Nix toolchain
  cannot block `bun install` for non-Nix developers. CI validates `bun.nix`
  independently.
- When it regenerates anything, commit `bun.lock`, `.nix/bun.nix` **and**
  `.nix/bun-lock-hash` together.

---

## Maintainer tools

Both are Python scripts using inline PEP 723 metadata, run with
[`uv`](https://docs.astral.sh/uv/) — no virtualenv setup required.

| Script | Purpose |
| --- | --- |
| `gen_catalog.py` | Generates `src-tauri/src/catalog/catalog.json` |
| `mirror_models.py` | Mirrors the catalog's GGUF files to S3-compatible storage |

### `gen_catalog.py`

```bash
HF_TOKEN=$(hf auth token) uv run gen_catalog.py [out_path]
```

Merges three sources into the committed catalog:

1. Hugging Face model-card `transcribe_cpp` blocks — **canonical** for capabilities
   and benchmarks.
2. A small **range-read of each GGUF header** — display labels only.
3. Local curation inside the script — the recommended set and editorial descriptions.

The output is committed and `include_str!`'d into the Rust binary
(`src-tauri/src/catalog/mod.rs`), so **regenerating it changes the app**. Bump
`CATALOG_VERSION` when the schema changes. After regenerating, run
`bun run check:model-languages`.

### `mirror_models.py`

Mirrors catalog GGUFs to Cloudflare R2 under keys
`{repo_id}/{revision}/{filename}` — the same three values that form the HF resolve
URL, so the app's mirror template is plain substitution. A revision pins content,
making keys immutable.

- **Default is a dry run**: prints the plan and writes nothing.
- `--execute` performs it. Downloads and uploads are pipelined; Ctrl-C is safe at
  any point, since a key only appears once its multipart upload completes.
- The bucket is the only state: each run HEADs every expected key and uploads what
  is missing, so runs are idempotent and machine-independent.
- Every file is hash-verified against the catalog's `sha256` between download and
  upload. That verification is what lets the app trust bare key existence.
- Mirrors are treated as **untrusted** by design — `gen_catalog.py` records a
  per-file `sha256` so listing a mirror only affects availability, never integrity.

Requires credentials for the R2 bucket; without them it runs offline and read-only.

---

## `ci/`

`ci/stage-transcribe-libs.sh` — stages transcribe-cpp's dynamic backend libraries
(`libtranscribe.so*`, `libggml*.so*`) into a packaging destination such as an
AppImage's `usr/lib`.

```bash
scripts/ci/stage-transcribe-libs.sh <src-lib-dir> <dest-dir>
```

It copies with `-L` so SONAME symlinks become real files, then **fails loudly** if
`libtranscribe.so` or the `libggml-cpu*` modules are missing. The CPU check matters:
a package without a CPU backend has no usable compute device on machines lacking a
GPU backend, which is exactly the crash this dynamic-backends posture exists to
avoid. Used by the Linux release path; the Windows equivalent is the committed DLL
set in [`../src-tauri/transcribe-libs/`](../src-tauri/transcribe-libs/README.md).

---

## Conventions

- Scripts are run through **Bun** (TypeScript) or **uv** (Python). Do not add a
  `node`/`ts-node` dependency, and do not require a Python virtualenv.
- A check script exits non-zero on failure and prints an actionable per-item
  message. Keep output copy-pasteable — these run in CI logs.
- `check-nix-deps.ts` is the one deliberate exception to "fail loudly"; see above.
- Generated artifacts (`catalog.json`, `.nix/bun.nix`) are committed. If a script
  regenerates one, commit it in the same change.
