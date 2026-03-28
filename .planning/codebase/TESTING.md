# Testing Patterns

**Analysis Date:** 2026-03-28

## Test Framework

**Frontend (E2E):**
- Framework: Playwright `@playwright/test` ^1.58.0
- Config: `playwright.config.ts` (project root)
- Browser: Chromium (Desktop Chrome profile)
- Base URL: `http://localhost:1420` (Vite dev server)

**Backend (Rust unit tests):**
- Framework: Rust's built-in `#[cfg(test)]` / `#[test]` — no external crate needed
- No Rust test files detected in `src-tauri/src/` — unit tests not yet written

**No unit/component test framework for frontend** — no Vitest, Jest, or React Testing Library is configured.

**Run Commands:**
```bash
bun run test:playwright          # Run all Playwright E2E tests (headless)
bun run test:playwright:ui       # Run with Playwright UI explorer
bun run check:translations       # Validate i18n locale completeness
cargo test                       # Run Rust unit tests (from src-tauri/)
```

## Test File Organization

**Playwright tests:**
- Location: `tests/` directory at project root
- Naming: `*.spec.ts` pattern
- Current tests: `tests/app.spec.ts` (2 smoke tests)

**Rust tests:**
- Convention: inline `#[cfg(test)]` module at bottom of each `.rs` file (standard Rust pattern)
- No test modules currently present in `src-tauri/src/`

## Test Structure

**Playwright suite pattern:**
```typescript
import { test, expect } from "@playwright/test";

test.describe("Handy App", () => {
  test("dev server responds", async ({ page }) => {
    const response = await page.goto("/");
    expect(response?.status()).toBe(200);
  });

  test("page has html structure", async ({ page }) => {
    await page.goto("/");
    const html = await page.content();
    expect(html).toContain("<html");
  });
});
```

## Playwright Configuration

Config file: `playwright.config.ts`

Key settings:
- `testDir: "./tests"` — all tests live in `tests/`
- `fullyParallel: true` — tests run in parallel
- `retries: 2` on CI, `0` locally
- `workers: 1` on CI (serial), unlimited locally
- `reporter: "html"` — generates HTML report
- `trace: "on-first-retry"` — trace captured on retry

**Web server setup:**
```typescript
webServer: {
  command: "bunx vite dev",
  url: "http://localhost:1420",
  reuseExistingServer: !process.env.CI,  // reuses running server locally
  timeout: 30000,
}
```

The Playwright config launches the Vite dev server automatically. Tests run against the frontend in isolation — they do NOT spin up the Tauri binary or Rust backend.

## Test Coverage

**Current coverage: Minimal**

Only 2 smoke tests exist (`tests/app.spec.ts`):
1. Verify dev server responds with HTTP 200
2. Verify page has basic HTML structure (`<html>`, `<body>`)

**No tests for:**
- Component behavior (no unit/component test framework)
- Settings logic (`src/stores/settingsStore.ts`, `src/hooks/useSettings.ts`)
- i18n translation completeness (checked by `check:translations` script, not a test runner)
- Tauri commands and Rust business logic (`src-tauri/src/managers/`)
- Audio pipeline (`src-tauri/src/audio_toolkit/`)
- VAD processing, transcription pipeline
- History storage (`src-tauri/src/managers/history.rs`)

## Translation Validation

Not a test framework test — a standalone script:

```bash
bun run check:translations
# → scripts/check-translations.ts
```

Validates that all locale files (`es`, `fr`, `vi`, ...) have all keys present in the English reference (`src/i18n/locales/en/translation.json`). Run this whenever adding new i18n keys.

## CI Behavior

Playwright config detects `process.env.CI`:
- `forbidOnly: !!process.env.CI` — prevents `.only` tests from being committed
- `retries: 2` on CI
- `workers: 1` on CI (no parallelism)
- Dev server is always started fresh on CI (`reuseExistingServer: false`)

## Adding New Tests

**New Playwright E2E test:**
- File: `tests/{feature}.spec.ts`
- Pattern: `test.describe` block + individual `test()` calls
- The Vite dev server starts automatically — navigate with `page.goto("/")`
- Note: Tauri APIs (`invoke`, `listen`) are NOT available in Playwright tests (no Tauri runtime). Test only UI rendered from the Vite dev server.

**New Rust unit test (when adding):**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // arrange
        // act
        // assert
    }
}
```
Place the `mod tests` block at the bottom of the relevant `.rs` file. Run with `cargo test` from `src-tauri/`.

## Coverage Gaps (Priority)

| Area | Risk | Priority |
|------|------|----------|
| `src/stores/settingsStore.ts` | Settings mutation logic untested | High |
| `src-tauri/src/managers/transcription.rs` | Core pipeline untested | High |
| `src-tauri/src/portable.rs` | Portable mode detection logic | High |
| `src-tauri/src/settings.rs` | Serde migration / deserialization | Medium |
| `src/i18n/` | Translation completeness | Low (script covers this) |
| Component rendering | No component test framework present | Medium |

---

*Testing analysis: 2026-03-28*
