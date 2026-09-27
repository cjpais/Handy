# `tests/` — Playwright specs

Browser-level frontend tests, run with [Playwright](https://playwright.dev/) using
`playwright.config.ts` at the repository root.

```bash
bun run test:playwright        # headless, starts Vite automatically
bun run test:playwright:ui     # interactive UI mode
```

The config starts `bunx vite dev` on **port 1420** and runs the specs against it.
Requires installed dependencies and browser assets
(`bunx playwright install` if Chromium is missing).

---

## Read this before adding a test

These tests run against **plain Vite in a browser**, with **no Tauri runtime**.
That means:

- `window.__TAURI__` / IPC does not exist, so **no `commands.*` call can work**.
- There is no backend, no microphone, no model, no filesystem, no tray, no overlay
  window and no global shortcuts.
- The app's main window will not render as it does natively: `App.tsx` returns
  `null` until `commands.getAppSettings()` resolves, which never happens here.

So these tests **cannot verify application behaviour**. They are smoke tests for
build and serving correctness only.

> **Do not claim native behaviour from these tests.** Microphone capture,
> transcription, paste, overlay rendering and model handling must be validated in
> `bun run tauri dev` on real hardware. See the validation ladder in
> [`../CONTEXT.md`](../CONTEXT.md).

---

## Current contents

`app.spec.ts` — two tests: that `/` returns HTTP 200, and that the served HTML
contains `<html>` and `<body>`. That is the entire suite.

Because the dev server is already started by the config, these effectively assert
"Vite builds and serves the entry point". They are a guard against a broken Vite
config or a missing `index.html`, not a guard against regressions in the app.

---

## Where other tests live

Frontend *unit* logic is not tested here, and there is no vitest/jest runner. The
existing test-like files are plain `bun` scripts using `node:assert`:

| File | How to run |
| --- | --- |
| `src/lib/utils/keyboard.test.ts` | `bun run test:keyboard` (wired in `package.json`) |
| `src/components/settings/history/clipboard.test.ts` | `bun <path>` — **no npm script** |
| `src/components/update-checker/portableInstaller.test.ts` | `bun <path>` — **no npm script** |

The last two have no runner wired up, so they do not run in CI. If you add
frontend logic worth testing, prefer extending that pattern and wiring a script,
rather than assuming a test framework exists.

Backend tests are Rust unit tests distributed through the modules
(`cargo test --manifest-path src-tauri/Cargo.toml`, ~225 tests). CI runs Rust tests
via `.github/workflows/test.yml` and these Playwright specs via
`.github/workflows/playwright.yml`.

---

## Notes

- CI triggers Playwright on `pull_request` and `workflow_dispatch` only — **not**
  on push to `main`.
- If you need real end-to-end coverage of the app, the practical path is a Tauri
  driver (e.g. `tauri-driver` / WebDriver) rather than Playwright against Vite.
  Nothing like that exists in this repository today.
- Keep new specs honest: a test that asserts a page loaded is fine and useful, but
  name and document it as a smoke test rather than implying feature coverage.
