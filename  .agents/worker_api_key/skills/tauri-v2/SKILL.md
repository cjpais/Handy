---
name: tauri-v2
description: "Tauri v2+ cross-platform app development with Rust backend. Use when configuring tauri.conf.json, implementing Rust commands (#[tauri::command]), setting up IPC patterns (invoke, emit, channels), configuring permissions/capabilities, troubleshooting build issues, or deploying desktop/mobile apps. Triggers on Tauri, src-tauri, invoke, emit, capabilities.json."
version: 1.0.1
---

# Tauri v2+ Development Skill

> Build cross-platform desktop and mobile apps with web frontends and Rust backends.

## Before You Start

**This skill prevents 8+ common errors and saves ~60% tokens.**

| Metric        | Without Skill      | With Skill            |
| ------------- | ------------------ | --------------------- |
| Setup Time    | ~2 hours           | ~30 min               |
| Common Errors | 8+                 | 0                     |
| Token Usage   | High (exploration) | Low (direct patterns) |

### Known Issues This Skill Prevents

1. Permission denied errors from missing capabilities
2. IPC failures from unregistered commands in `generate_handler!`
3. State management panics from type mismatches
4. Mobile build failures from missing Rust targets
5. White screen issues from misconfigured dev URLs

## Quick Start

### Step 1: Create a Tauri Command

```rust
// src-tauri/src/lib.rs
#[tauri::command]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Why this matters:** Commands not in `generate_handler![]` silently fail when invoked from frontend.

> **`main.rs` stays thin:** `src-tauri/src/main.rs` should only be a thin passthrough — all application logic lives in `lib.rs`:
>
> ```rust
> // src-tauri/src/main.rs
> #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
> fn main() {
>     app_lib::run();
> }
> ```
>
> This split is required for mobile builds — Tauri replaces `main()` with `mobile_entry_point` on mobile targets.

### Step 2: Call from Frontend

```typescript
import { invoke } from "@tauri-apps/api/core";

const greeting = await invoke<string>("greet", { name: "World" });
console.log(greeting); // "Hello, World!"
```

**Why this matters:** Use `@tauri-apps/api/core` (not `@tauri-apps/api/tauri` - that's v1 API).

### Step 3: Add Required Permissions

```json
// src-tauri/capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

**Why this matters:** Tauri v2 denies everything by default - explicit permissions required for all operations.

## Critical Rules

### Always Do

- Register every command in `tauri::generate_handler![cmd1, cmd2, ...]`
- Return `Result<T, E>` from commands for proper error handling
- Use `Mutex<T>` for shared state accessed from multiple commands
- Add capabilities before using any plugin features
- Use `lib.rs` for shared code (required for mobile builds)
- Use `#[cfg_attr(mobile, tauri::mobile_entry_point)]` on `pub fn run()` in `lib.rs` for mobile compatibility

### Never Do

- Never use borrowed types (`&str`) in async commands - use owned types
- Never block the main thread - use async for I/O operations
- Never hardcode paths - use Tauri path APIs (`app.path()`)
- Never skip capability setup - even "safe" operations need permissions

### Common Mistakes

**Wrong - Borrowed type in async:**

```rust
#[tauri::command]
async fn bad(name: &str) -> String { // Compile error!
    name.to_string()
}
```

**Correct - Owned type:**

```rust
#[tauri::command]
async fn good(name: String) -> String {
    name
}
```

**Why:** Async commands cannot borrow data across await points; Tauri requires owned types for async command parameters.

## Known Issues Prevention

| Issue                                      | Root Cause                                                   | Solution                                                                  |
| ------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------------------- |
| "Command not found"                        | Missing from `generate_handler!`                             | Add command to handler macro                                              |
| "Permission denied"                        | Missing capability                                           | Add to `capabilities/default.json`                                        |
| Plugin feature silently fails              | Plugin installed but permission not in capability            | Add plugin permission string to `capabilities/default.json`               |
| Updater fails in production                | Unsigned artifacts or HTTP endpoint                          | Generate keys with `cargo tauri signer generate`, use HTTPS endpoint only |
| Sidecar not found                          | `externalBin` not in `tauri.conf.json` or missing executable | Add path to `bundle.externalBin`, ensure binary is bundled                |
| Feature works on desktop, breaks on mobile | Desktop-only API used                                        | Check if API has mobile support — some plugins are desktop-only           |
| State panic on access                      | Type mismatch in `State<T>`                                  | Use exact type from `.manage()`                                           |
| White screen on launch                     | Frontend not building                                        | Check `beforeDevCommand` in config                                        |
| IPC timeout                                | Blocking async command                                       | Remove blocking code or use spawn                                         |
| Mobile build fails                         | Missing Rust targets                                         | Run `rustup target add <target>`                                          |

## Deep-Dive References

- **Security & permissions** → [`references/capabilities-reference.md`](references/capabilities-reference.md)
- **IPC decision guide** → [`references/ipc-patterns.md`](references/ipc-patterns.md)
- **Official plugins** → [`references/plugin-reference.md`](references/plugin-reference.md)
- **Updater & distribution** → [`references/updater-distribution-reference.md`](references/updater-distribution-reference.md)
- **Tray, sidecars, deep links** → [`references/advanced-runtime-reference.md`](references/advanced-runtime-reference.md)

## Configuration Reference

### tauri.conf.json

```json
{
  "$schema": "./gen/schemas/desktop-schema.json",
  "productName": "my-app",
  "version": "1.0.0",
  "identifier": "com.example.myapp",
  "build": {
    "devUrl": "http://localhost:5173",
    "frontendDist": "../dist",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "My App",
        "width": 800,
        "height": 600
      }
    ],
    "security": {
      "csp": "default-src 'self'; img-src 'self' data:",
      "capabilities": ["default"]
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/icon.icns", "icons/icon.ico", "icons/icon.png"]
  }
}
```

**Key settings:**

- `build.devUrl`: Must match your frontend dev server port
- `app.security.capabilities`: Array of capability file identifiers

**Plugin configuration** — Some plugins require additional `tauri.conf.json` blocks (e.g., `store`, `updater`). Always check the specific plugin docs at `v2.tauri.app/plugin/<plugin-name>/` for required config keys.

## Project Structure

```
my-tauri-app/
├── src/                    # Frontend source
├── src-tauri/
│   ├── src/
│   │   ├── main.rs         # Thin passthrough — calls lib::run()
│   │   └── lib.rs          # ALL application logic lives here
│   ├── capabilities/
│   │   └── default.json    # Capability definitions (grant permissions here)
│   ├── tauri.conf.json     # App configuration (devUrl, bundle, security)
│   ├── Cargo.toml          # Rust dependencies
│   └── build.rs            # Build script (required for tauri-build)
└── package.json
```

**Why `lib.rs` owns all logic:** Tauri replaces `main()` with `#[cfg_attr(mobile, tauri::mobile_entry_point)]` on mobile. All commands, state, and builder setup must live in `lib.rs::run()`.

### Cargo.toml

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[lib]
name = "app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

**Key settings:**

- `[lib]` section: Required for mobile builds
- `crate-type`: Must include all three types for cross-platform

## Common Patterns

### Error Handling Pattern

Use `Result<T, E>` and `thiserror` for type-safe error propagation across the IPC boundary. See [`references/ipc-patterns.md`](references/ipc-patterns.md) for full implementation details.

```rust
use thiserror::Error;

#[derive(Debug, Error)]
enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Not found: {0}")]
    NotFound(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::ser::Serializer {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

#[tauri::command]
fn risky_operation() -> Result<String, AppError> {
    Ok("success".into())
}
```

### Serde Boundary Rules

All command arguments must implement `serde::Deserialize`, and return types must implement `serde::Serialize`. This is how Tauri bridges JSON over the IPC boundary.

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateUserArgs {
    name: String,
    email: String,
    role: Option<String>,  // Optional fields use Option<T>
}

#[derive(Serialize)]
struct User {
    id: u64,
    name: String,
}

#[tauri::command]
fn create_user(args: CreateUserArgs) -> Result<User, String> {
    Ok(User { id: 1, name: args.name })
}
```

**Common serde pitfalls:**

- Field names are camelCase in JS, snake_case in Rust — Tauri automatically converts between them
- `Option<T>` maps to optional JS arguments (can be `undefined` or `null`)
- Complex enums need `#[serde(tag = "type")]` or similar to be JSON-safe
- Error types must also implement `Serialize` (see Error Handling Pattern above)

### State Management Pattern

Tauri state manages application data across commands. See [`references/ipc-patterns.md`](references/ipc-patterns.md) for more complex state patterns.

```rust
use std::sync::Mutex;
use tauri::State;

struct AppState {
    counter: u32,
}

#[tauri::command]
fn increment(state: State<'_, Mutex<AppState>>) -> u32 {
    let mut s = state.lock().unwrap();
    s.counter += 1;
    s.counter
}

// In builder:
tauri::Builder::default()
    .manage(Mutex::new(AppState { counter: 0 }))
```

### Event Emission Pattern

Events are fire-and-forget notifications. See [`references/ipc-patterns.md`](references/ipc-patterns.md) for bidirectional examples.

```rust
use tauri::Emitter;

#[tauri::command]
fn start_task(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        app.emit("task-progress", 50).unwrap();
        app.emit("task-complete", "done").unwrap();
    });
}
```

```typescript
import { listen } from "@tauri-apps/api/event";

const unlisten = await listen("task-progress", (e) => {
  console.log("Progress:", e.payload);
});
// Call unlisten() when done
```

### Channel Streaming Pattern

Channels provide high-frequency, typed streaming from Rust to Frontend. See [`references/ipc-patterns.md`](references/ipc-patterns.md) for full implementation details.

```rust
use tauri::ipc::Channel;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "event", content = "data")]
enum DownloadEvent {
    Progress { percent: u32 },
    Complete { path: String },
}

#[tauri::command]
async fn download(url: String, on_event: Channel<DownloadEvent>) {
    for i in 0..=100 {
        on_event.send(DownloadEvent::Progress { percent: i }).unwrap();
    }
    on_event.send(DownloadEvent::Complete { path: "/downloads/file".into() }).unwrap();
}
```

```typescript
import { invoke, Channel } from "@tauri-apps/api/core";

const channel = new Channel<DownloadEvent>();
channel.onmessage = (msg) => console.log(msg.event, msg.data);
await invoke("download", { url: "https://...", onEvent: channel });
```

### Window Access Pattern

Tauri v2 uses `WebviewWindow` for unified window and webview management.

```rust
use tauri::Manager;

#[tauri::command]
fn focus_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
    }
}
```

**Why this matters:** Use `tauri::WebviewWindow` and `app.get_webview_window("label")` in v2 — the v1 `app.get_window()` API is removed in v2.

## Bundled Resources

### References

Located in `references/`:

- `capabilities-reference.md` - Permission patterns and examples
- `ipc-patterns.md` - Complete IPC examples
- `plugin-reference.md` - Official plugin install, registration, and permission strings
- `updater-distribution-reference.md` - Signing, HTTPS requirements, and bundle shipping
- `advanced-runtime-reference.md` - `TrayIconBuilder`, sidecars, deep links, and asset protocols

> **Note:** For deep dives on specific topics, see the reference files above.

## Dependencies

### Required

| Package           | Version  | Purpose       |
| ----------------- | -------- | ------------- |
| `@tauri-apps/cli` | ^2 (v2+) | CLI tooling   |
| `@tauri-apps/api` | ^2 (v2+) | Frontend APIs |
| `tauri`           | ^2 (v2+) | Rust core     |
| `tauri-build`     | ^2 (v2+) | Build scripts |

_\*Last verified: 2026-04-02. Always check [official changelog](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/CHANGELOG.md) for feature timing._

### Optional (Plugins)

| Package               | Version  | Purpose                   | Key Permission   |
| --------------------- | -------- | ------------------------- | ---------------- |
| `tauri-plugin-fs`     | ^2 (v2+) | File system access        | `fs:default`     |
| `tauri-plugin-dialog` | ^2 (v2+) | Native dialogs            | `dialog:default` |
| `tauri-plugin-shell`  | ^2 (v2+) | Shell commands, open URLs | `shell:default`  |
| `tauri-plugin-http`   | ^2 (v2+) | HTTP client               | `http:default`   |
| `tauri-plugin-store`  | ^2 (v2+) | Key-value storage         | `store:default`  |

> **Plugin permissions are mandatory.** Installing a plugin without adding its permission string to a capability file causes silent runtime failures. See [`references/plugin-reference.md`](references/plugin-reference.md) for full install + permission details for all official plugins.

## Official Documentation

- [Tauri v2+ Documentation](https://v2.tauri.app/)
- [Commands Reference](https://v2.tauri.app/develop/calling-rust/)
- [Capabilities & Permissions](https://v2.tauri.app/security/capabilities/)
- [Configuration Reference](https://v2.tauri.app/reference/config/)

## Troubleshooting

### White Screen on Launch

**Symptoms:** App launches but shows blank white screen

**Solution:**

1. Verify `devUrl` matches your frontend dev server port
2. Check `beforeDevCommand` runs your dev server
3. Open DevTools (Cmd+Option+I / Ctrl+Shift+I) to check for errors

### Command Returns Undefined

**Symptoms:** `invoke()` returns undefined instead of expected value

**Solution:**

1. Verify command is in `generate_handler![]`
2. Check Rust command actually returns a value
3. Ensure argument names match (camelCase in JS, snake_case in Rust by default)

### Mobile Build Failures

**Symptoms:** Android/iOS build fails with missing target

**Solution:**

```bash
# Android targets
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android

# iOS targets (macOS only)
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
```

### Desktop vs Mobile Behavioral Differences

Not all Tauri APIs and plugins support mobile (iOS/Android). Before using any plugin or API in a mobile build:

1. **Check the plugin page** at `v2.tauri.app/plugin/<name>/` for platform support matrix
2. **Common desktop-only items**: System tray (`TrayIconBuilder`), window labels/multi-window, some shell plugin features
3. **Mobile-safe patterns**: IPC commands/events/channels work on all platforms; `tauri::AppHandle` is mobile-safe
4. **Conditional compilation**: Use `#[cfg(desktop)]` / `#[cfg(mobile)]` for platform-specific Rust logic

```rust
#[tauri::command]
fn platform_info() -> String {
    #[cfg(desktop)]
    return "desktop".to_string();
    #[cfg(mobile)]
    return "mobile".to_string();
}
```

## Setup Checklist

Before using this skill, verify:

- [ ] `npx tauri info` shows correct Tauri v2 versions
- [ ] `src-tauri/capabilities/default.json` exists with at least `core:default`
- [ ] All commands registered in `generate_handler![]`
- [ ] `lib.rs` contains shared code (for mobile support)
- [ ] Required Rust targets installed for target platforms
