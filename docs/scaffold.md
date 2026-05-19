# Goldfish scaffold spec

**Status:** proposed (not yet executed)
**Last updated:** 2026-05-19

This file specifies the minimum Goldfish scaffold: file tree, file contents, and the exact edits to upstream-owned files. Reviewed and merged BEFORE any code changes land, so we can spot problems while they're cheap.

## Goals

1. Establish the `goldfish/` directories on both Rust and frontend sides.
2. Wire one trivial backend command (`goldfish_ping`) end-to-end as a smoke test of the seams.
3. Make every touch into upstream-owned files **minimal, contiguous, and marked** so future merge conflicts are trivial.

**Non-goals (deliberately deferred):**

- Product identity (bundle ID, productName, icons) — see [decisions.md](./decisions.md) trigger.
- Frontend route registry / `<GoldfishMount/>` — pick when first UI feature lands.
- i18n namespace — no Goldfish strings to translate yet.
- Disabling the updater — done as a separate step (next, before scaffold execution).

## File tree to create

```
src-tauri/src/goldfish/
├── mod.rs           # module root, re-exports, register_state hook
├── commands.rs      # Tauri commands (initially: goldfish_ping only)
└── README.md        # "what lives here and why"

src/goldfish/
├── index.ts         # placeholder re-export barrel
└── README.md        # "what lives here and why"
```

## Backend file contents

### `src-tauri/src/goldfish/mod.rs`

```rust
//! Goldfish-only code. Anything in this module is not in upstream Handy
//! and should not be PR'd back. See docs/fork-strategy.md.

pub mod commands;

use tauri::AppHandle;

/// Called once from `initialize_core_logic` in `lib.rs` after upstream
/// managers are registered. Currently a no-op; future home for Goldfish
/// Tauri state, event listeners, and background tasks.
pub fn register_state(_app_handle: &AppHandle) {
    log::info!("goldfish: register_state (no-op)");
}
```

### `src-tauri/src/goldfish/commands.rs`

```rust
//! Tauri commands exposed by Goldfish. Listed in `collect_commands![]`
//! in `lib.rs` under the `// === Goldfish ===` marker.

#[tauri::command]
#[specta::specta]
pub fn goldfish_ping() -> String {
    "goldfish: pong".to_string()
}
```

### `src-tauri/src/goldfish/README.md`

Short note: "Goldfish-only Rust code. Engine code stays in `managers/`, `audio_toolkit/`, `transcription_coordinator.rs`. Anything here is non-upstream."

## Frontend file contents

### `src/goldfish/index.ts`

```ts
// Goldfish-only frontend code lives under this directory.
// Nothing exported yet; see docs/fork-strategy.md for the composition
// pattern decision (deferred until first Goldfish UI lands).
export {};
```

### `src/goldfish/README.md`

Short note mirroring the Rust one.

## Edits to upstream-owned files

Exactly **two** files touched, with marker comments so merges land predictably.

### 1. `src-tauri/src/lib.rs`

**Edit A — module declaration** (near the existing `mod ...;` block at the top):

```rust
mod goldfish;  // Goldfish-only; see src/goldfish/ and docs/fork-strategy.md
```

Place it at the END of the existing `mod` block (currently line ~21, after `mod utils;`) so additions stay contiguous and conflicts are easier to resolve.

**Edit B — `collect_commands![]`** (currently lines 326–429):

Append at the very end, just before the closing `])`:

```rust
            // === Goldfish ===
            goldfish::commands::goldfish_ping,
```

The trailing-comma + marker means upstream's additions to the list land above the marker, Goldfish additions land below, and merge conflicts are always clearly attributable.

**Edit C — `initialize_core_logic()`** (currently ends ~line 295):

Append at the end of the function, before the closing `}`:

```rust
    // === Goldfish ===
    goldfish::register_state(app_handle);
```

That's all three lib.rs touchpoints. `collect_events![]` is untouched (no Goldfish events yet); we'll repeat the marker pattern when we add one.

### 2. `src/bindings.ts`

Not edited manually. It will regenerate when `bun run tauri build` (or `dev`) runs after the Rust changes land. Commit the regenerated file in the same commit as the scaffold so reviewers see the wire-up.

## Validation checklist (before committing)

- [ ] `bun run lint` clean.
- [ ] `cargo fmt --check` clean.
- [ ] `bun run tauri build` (or `dev`) succeeds and regenerates `src/bindings.ts` with `goldfishPing` exported.
- [ ] Boot the app; check log output contains `goldfish: register_state (no-op)`.
- [ ] In DevTools console: `await window.__TAURI__.core.invoke('goldfish_ping')` returns `"goldfish: pong"`.

## What this scaffold does NOT do

- Touch [src/App.tsx](../src/App.tsx). No UI yet.
- Touch [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json). Product identity is a separate step.
- Add any feature. The ping command is purely a wire-up smoke test; first real feature comes after the scaffold lands.

## Open questions before execution

1. Is `goldfish_ping` the right smoke-test command name, or should it be `__goldfish_ping` (double underscore prefix to make it visibly "internal")? Recommendation: plain `goldfish_ping` — short, clear, and removable later.
2. Should `src/goldfish/index.ts` be omitted entirely until there's something to export? Tradeoff: adding an empty file makes the seam visible; omitting it avoids a useless file. Recommendation: keep the placeholder + README so the directory exists in git (git doesn't track empty dirs).
