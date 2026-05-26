# Handy 移动端 - 阶段 0+1 实施计划 (技术 spike + Rust core 抽取)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把当前桌面端深度耦合 Tauri 的 Rust 业务逻辑(转录、VAD、模型管理、历史、文本后处理)抽取到独立 `handy-core` crate,并验证它能交叉编译到 `aarch64-linux-android`。本计划完成后,桌面端行为零回归,且为后续移动端工程铺好了 Rust 复用层。

**Architecture:** 引入 Cargo workspace + 4 个新 crate (`handy-core`、`handy-platform`、稍后用的 `handy-mobile`、暂不创建的 `src-mobile`)。`handy-core` 不依赖 `tauri::*` / `cpal` / `enigo`,通过 `handy-platform` trait 接收平台能力注入。桌面端在 `src-tauri/` 内提供 trait 实现并把 manager 委托给 core,保持现有 Tauri command 签名不变以保证前端 `bindings.ts` 不需要重新生成。

**Tech Stack:** Rust 1.83+ stable, Cargo workspace (resolver = 2), `transcribe-rs` (vendored), `vad-rs`, `rusqlite` (bundled), `tokio`, `async-trait`, `cargo-nextest` (测试)。GitHub Actions Linux runner 用 `cross` 做 Android 交叉编译。

**关联设计文档:** [docs/superpowers/specs/2026-05-26-handy-mobile-design.md](../specs/2026-05-26-handy-mobile-design.md)

---

## 文件结构总览

新建:
- `Cargo.toml` (根) — workspace manifest
- `crates/handy-core/Cargo.toml` + `src/lib.rs`
- `crates/handy-core/src/text/mod.rs` (从 `src-tauri/src/audio_toolkit/text.rs` 移动)
- `crates/handy-core/src/audio/mod.rs` (从 `src-tauri/src/audio_toolkit/audio/{resampler,utils,visualizer,constants}.rs` 移动 — 不含 cpal/recorder)
- `crates/handy-core/src/vad/mod.rs` (从 `src-tauri/src/audio_toolkit/vad/` 整体移动)
- `crates/handy-core/src/model/mod.rs` (从 `src-tauri/src/managers/model.rs` 抽取纯逻辑)
- `crates/handy-core/src/history/mod.rs` (从 `src-tauri/src/managers/history.rs` 整体移动)
- `crates/handy-core/src/settings/mod.rs` (从 `src-tauri/src/settings.rs` 抽取共享 schema)
- `crates/handy-core/src/transcription/{mod.rs,engine.rs,events.rs}` (从 `src-tauri/src/managers/transcription.rs` 抽取,去 tauri 化)
- `crates/handy-platform/Cargo.toml` + `src/{lib,audio,text_output,storage,notification,event_sink}.rs`
- `.github/workflows/mobile-ci.yml` (GitHub Actions: 交叉编译 + 单元测试)
- `docs/mobile/README.md` (入口占位)

修改:
- `src-tauri/Cargo.toml` — 改为 workspace member,依赖 `handy-core` 和 `handy-platform`
- `src-tauri/src/audio_toolkit/mod.rs` — 移除已迁出的子模块,re-export `handy_core` 中对应符号保持上层调用不变
- `src-tauri/src/managers/{transcription,model,history,mod}.rs` — manager 改为 core 服务的薄封装
- `src-tauri/src/settings.rs` — 桌面专属字段保留本地,共享字段委托给 `handy_core::settings`
- `src-tauri/src/lib.rs` — 把 trait 实现注入到 core 服务

不修改 (本计划范围内):
- `src/` 前端任何文件 — 通过保持 Tauri command 签名一致保证 `bindings.ts` 无需重新生成
- `src-tauri/src/commands/` — 命令实现保持不变,内部委托给新 core 服务
- `src-tauri/src/audio_toolkit/audio/{recorder.rs,device.rs,ai_mouse_mic.rs}` — cpal 相关代码留在桌面端

---

## 阶段 0: 技术 Spike (验证 Android 交叉编译)

### 任务 0: 在 GitHub Actions 上验证 `transcribe-rs` 能交叉编译到 Android target

**Files:**
- Create: `.github/workflows/mobile-ci.yml`

**先决条件检查:**

- [ ] **Step 0.1: 创建临时验证分支**

Run:
```powershell
git checkout -b mobile/phase-0-spike
```

- [ ] **Step 0.2: 创建最小 GHA workflow**

Create `.github/workflows/mobile-ci.yml`:

```yaml
name: Mobile CI

on:
  push:
    branches: [main, mobile/**]
  pull_request:
    branches: [main]
  workflow_dispatch:

jobs:
  android-cross-compile-spike:
    name: Android Cross-Compile Spike
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android

      - name: Install Android NDK
        uses: nttld/setup-ndk@v1
        id: setup-ndk
        with:
          ndk-version: r26d
          local-cache: true

      - name: Configure cargo for NDK
        run: |
          mkdir -p ~/.cargo
          cat >> ~/.cargo/config.toml <<EOF
          [target.aarch64-linux-android]
          linker = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"
          ar     = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
          EOF

      - name: Cache cargo build
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: android-spike
          workspaces: src-tauri

      - name: Cross-compile src-tauri to aarch64-linux-android (spike, expected to fail or warn)
        working-directory: src-tauri
        env:
          CC_aarch64-linux-android: ${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
          AR_aarch64-linux-android: ${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar
          ANDROID_NDK_HOME: ${{ steps.setup-ndk.outputs.ndk-path }}
        # 仅检查能不能链接到 whisper.cpp / onnxruntime;失败时打印日志但不阻塞,本任务只是侦察
        run: cargo check --target aarch64-linux-android --lib --no-default-features --features "" 2>&1 | tee spike.log || true

      - name: Upload spike log
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: android-spike-log
          path: src-tauri/spike.log
```

- [ ] **Step 0.3: Commit & push,触发 GHA**

```powershell
git add .github/workflows/mobile-ci.yml
git commit -m "ci: add Android cross-compile spike workflow"
git push -u origin mobile/phase-0-spike
```

- [ ] **Step 0.4: 等 GHA 跑完,下载 `android-spike-log` artifact**

Expected: spike.log 文件存在。结果有两种可能,都不阻塞后续:
- **A. cargo check 通过**: 极好,说明 transcribe-rs 在 Android 上能直接交叉编译
- **B. 失败**: 把 spike.log 中的错误信息记录到 `docs/mobile/spike-findings.md` (新建),分类错误源 (whisper-rs/cmake/onnxruntime/cpal/gtk),作为后续阶段需要 cfg-guard 或 vendor 补丁的依据

- [ ] **Step 0.5: 记录 spike 结论**

Create `docs/mobile/spike-findings.md`:

```markdown
# Android Cross-Compile Spike 记录 (Phase 0)

## 执行环境
- GHA ubuntu-latest, NDK r26d, target aarch64-linux-android24
- 命令: `cargo check --target aarch64-linux-android --lib`
- 时间: <填入 GHA 运行时间戳>

## 结果
- [ ] cargo check 通过
- [ ] cargo check 失败,错误源 (勾选所有适用):
  - [ ] cpal (`cpal` 不支持 Android,需在 `handy-core` 中隔离)
  - [ ] gtk-layer-shell / gtk (Linux 桌面专属)
  - [ ] whisper-rs cmake (whisper.cpp 编译失败)
  - [ ] onnxruntime-sys (Parakeet 引擎依赖)
  - [ ] rdev / enigo (输入模拟)
  - [ ] tauri-plugin-* (部分 plugin 已 cfg-guard,但可能还有遗漏)
  - [ ] 其他: ___

## 关键错误片段
\`\`\`
<粘贴 spike.log 中前 50 行关键错误>
\`\`\`

## 后续阶段对策
- 阶段 1 抽 `handy-core` 时确保隔离: <根据上面勾选项填具体策略>
- 阶段 2/3 在 src-mobile/ 中需要的额外 cfg-guard: <填具体>
```

填写完成后:
```powershell
git add docs/mobile/spike-findings.md
git commit -m "docs(mobile): record phase 0 cross-compile spike findings"
git push
```

**本任务完成判据**: GHA 跑完一次 (无论成功失败),`spike-findings.md` 有内容、已 push。

---

## 阶段 1: 引入 Cargo Workspace

### 任务 1: 创建 Cargo workspace 根 manifest

**Files:**
- Create: `Cargo.toml` (在仓库根目录)
- Modify: `src-tauri/Cargo.toml` (调整为 workspace member)

- [ ] **Step 1.1: 切回 main 分支,从最新 main 开新分支**

```powershell
git checkout main
git pull
git checkout -b mobile/phase-1-workspace
```

- [ ] **Step 1.2: 创建根 Cargo.toml**

Create `Cargo.toml` (仓库根):

```toml
[workspace]
members  = ["src-tauri"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT"
authors = ["cjpais", "唐海文"]

[workspace.dependencies]
# 共享版本号,后续 crate 引用 workspace = true
anyhow            = "1"
log               = "0.4"
serde             = { version = "1", features = ["derive"] }
serde_json        = "1"
tokio             = { version = "1.43", features = ["sync", "rt", "rt-multi-thread", "macros"] }
async-trait       = "0.1"
thiserror         = "2"
chrono            = { version = "0.4", features = ["serde"] }
regex             = "1"
rusqlite          = { version = "0.37", features = ["bundled"] }
rusqlite_migration = "2.3"
reqwest           = { version = "0.12", features = ["json", "stream"] }
futures-util      = "0.3"
hound             = "3.5.1"
rubato            = "0.16.2"
rustfft           = "6.4.0"
strsim            = "0.11.0"
natural           = "0.5.0"
once_cell         = "1"
transcribe-rs     = { path = "src-tauri/vendor/transcribe-rs", features = ["whisper", "onnx"] }
vad-rs            = { git = "https://github.com/cjpais/vad-rs", default-features = false }
ferrous-opencc    = "0.2.3"

# 桌面专属 (不被 handy-core 引用),但放 workspace 统一版本
tauri             = { version = "2.10.2", default-features = false }
specta            = "=2.0.0-rc.22"
tauri-specta      = { version = "=2.0.0-rc.21", default-features = false }

[patch.crates-io]
# 沿用 src-tauri 现有 patch
tauri-runtime     = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-runtime-wry = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-utils       = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
transcribe-rs     = { path = "src-tauri/vendor/transcribe-rs" }

[profile.dev]
incremental = true

[profile.release]
lto              = true
codegen-units    = 1
strip            = true
panic            = "unwind"
```

- [ ] **Step 1.3: 移除 src-tauri/Cargo.toml 中已上提到 workspace 的 [patch.crates-io] 与 [profile.*]**

Modify `src-tauri/Cargo.toml`:

删除文件中的以下两段 (它们现在在根 Cargo.toml 里):

```toml
[patch.crates-io]
tauri-runtime = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-runtime-wry = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-utils = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
transcribe-rs = { path = "vendor/transcribe-rs" }
```

```toml
[profile.dev]
incremental = true

[profile.release]
lto = true
codegen-units = 1
strip = true
panic = "unwind"
```

保留 `[package]`、`[lib]`、`[build-dependencies]`、`[dependencies]` 等其他内容不变。

- [ ] **Step 1.4: 验证桌面端仍能编译**

```powershell
cd f:\@Haiwen\MSZN\Handy
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS — 输出 `Finished \`dev\` profile [unoptimized + debuginfo] target(s)` (可能有 warning,无 error)。
如果失败: 看是否有版本号在 src-tauri/Cargo.toml 与根 Cargo.toml 冲突;调和后重试。

- [ ] **Step 1.5: 跑现有测试确认零回归**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部测试通过 (与改动前一致)。

- [ ] **Step 1.6: Commit**

```powershell
git add Cargo.toml src-tauri/Cargo.toml
git commit -m "build: introduce cargo workspace, hoist patch & profile to root"
```

---

### 任务 2: 创建空的 `handy-core` crate 并加入 workspace

**Files:**
- Create: `crates/handy-core/Cargo.toml`
- Create: `crates/handy-core/src/lib.rs`
- Modify: `Cargo.toml` (根) — 加入新 member

- [ ] **Step 2.1: 创建目录与最小 lib**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src" -Force
```

Create `crates/handy-core/Cargo.toml`:

```toml
[package]
name        = "handy-core"
version     = "0.1.0"
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "Platform-agnostic core for Handy: transcription, VAD, model, history, settings."

[lib]
name = "handy_core"
path = "src/lib.rs"

[dependencies]
anyhow      = { workspace = true }
log         = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
thiserror   = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

Create `crates/handy-core/src/lib.rs`:

```rust
//! handy-core: platform-agnostic core for the Handy speech-to-text app.
//!
//! This crate intentionally does NOT depend on `tauri`, `cpal`, `enigo`, `rdev`,
//! `gtk-*`, or any GUI/platform-specific library. Platform capabilities are
//! injected through traits defined in the `handy-platform` crate.

#[cfg(test)]
mod sanity_check {
    #[test]
    fn it_links() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 2.2: 把 handy-core 加入 workspace members**

Modify `Cargo.toml` (根):

```toml
[workspace]
members  = ["src-tauri", "crates/handy-core"]
resolver = "2"
```

- [ ] **Step 2.3: 验证 workspace 解析与构建**

```powershell
cargo check -p handy-core
```

Expected: PASS — `Finished \`dev\` profile`。

```powershell
cargo test -p handy-core --lib
```

Expected: `test sanity_check::it_links ... ok`,1 passed。

- [ ] **Step 2.4: 验证桌面端不受影响**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 2.5: Commit**

```powershell
git add crates/handy-core Cargo.toml
git commit -m "build(handy-core): add empty handy-core crate to workspace"
```

---

### 任务 3: 创建 `handy-platform` crate (trait 定义)

**Files:**
- Create: `crates/handy-platform/Cargo.toml`
- Create: `crates/handy-platform/src/lib.rs`
- Create: `crates/handy-platform/src/audio.rs`
- Create: `crates/handy-platform/src/text_output.rs`
- Create: `crates/handy-platform/src/storage.rs`
- Create: `crates/handy-platform/src/event_sink.rs`
- Modify: `Cargo.toml` (根) — 加入新 member

- [ ] **Step 3.1: 写第一个 trait 的失败测试**

```powershell
New-Item -ItemType Directory -Path "crates/handy-platform/src" -Force
```

Create `crates/handy-platform/Cargo.toml`:

```toml
[package]
name        = "handy-platform"
version     = "0.1.0"
edition.workspace = true
license.workspace = true
authors.workspace = true
description = "Platform abstraction traits used by handy-core and implemented by host apps."

[lib]
name = "handy_platform"
path = "src/lib.rs"

[dependencies]
anyhow      = { workspace = true }
async-trait = { workspace = true }
serde       = { workspace = true }
tokio       = { workspace = true }
thiserror   = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
```

Create `crates/handy-platform/src/lib.rs`:

```rust
//! handy-platform: trait definitions for platform-specific capabilities.
//!
//! Concrete implementations live in:
//! - `src-tauri/` (desktop: cpal, enigo, OS clipboard)
//! - `src-mobile/` + `crates/handy-mobile/` (Android: Oboe, IME, JNI)

pub mod audio;
pub mod event_sink;
pub mod storage;
pub mod text_output;

pub use audio::{AudioCapture, AudioConfig, AudioFrame};
pub use event_sink::EventSink;
pub use storage::AppStorage;
pub use text_output::{OutputMode, TextOutput};
```

Create `crates/handy-platform/src/audio.rs`:

```rust
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug)]
pub struct AudioConfig {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub frame_size: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate_hz: 16_000,
            channels: 1,
            frame_size: 512,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub timestamp_ms: u64,
}

#[async_trait]
pub trait AudioCapture: Send + Sync {
    async fn start(&mut self, config: AudioConfig) -> Result<mpsc::Receiver<AudioFrame>>;
    async fn stop(&mut self) -> Result<()>;
    fn is_capturing(&self) -> bool;
}
```

Create `crates/handy-platform/src/text_output.rs`:

```rust
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputMode {
    Clipboard,
    Typed,
    ImeCommit,
}

#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn deliver(&self, text: &str, mode: OutputMode) -> Result<()>;
}
```

Create `crates/handy-platform/src/storage.rs`:

```rust
use std::path::PathBuf;

pub trait AppStorage: Send + Sync {
    fn models_dir(&self) -> PathBuf;
    fn db_path(&self) -> PathBuf;
    fn settings_path(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
}
```

Create `crates/handy-platform/src/event_sink.rs`:

```rust
use serde::Serialize;
use std::fmt::Debug;

/// Sink for core → UI events. Desktop wires this to `tauri::AppHandle::emit`,
/// mobile wires it to a JNI callback or Tauri Mobile emit.
pub trait EventSink: Send + Sync + 'static {
    fn emit_json(&self, event_name: &str, payload: serde_json::Value);
}

pub fn emit<T: Serialize + Debug>(sink: &dyn EventSink, event_name: &str, payload: &T) {
    match serde_json::to_value(payload) {
        Ok(v) => sink.emit_json(event_name, v),
        Err(e) => log::warn!("EventSink emit serialize failed for {event_name}: {e}"),
    }
}
```

- [ ] **Step 3.2: 加入 workspace**

Modify `Cargo.toml` (根):

```toml
[workspace]
members  = ["src-tauri", "crates/handy-core", "crates/handy-platform"]
resolver = "2"
```

- [ ] **Step 3.3: 写一个简单的 mock 测试,验证 trait 可实例化**

Create `crates/handy-platform/src/lib.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    struct CapturingSink(Mutex<Vec<(String, serde_json::Value)>>);

    impl EventSink for CapturingSink {
        fn emit_json(&self, event_name: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().push((event_name.to_string(), payload));
        }
    }

    #[test]
    fn event_sink_round_trip() {
        let sink = CapturingSink(Mutex::new(Vec::new()));
        event_sink::emit(&sink, "test/event", &json!({"k": 1}));
        let captured = sink.0.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "test/event");
        assert_eq!(captured[0].1, json!({"k": 1}));
    }
}
```

- [ ] **Step 3.4: 跑测试**

```powershell
cargo test -p handy-platform --lib
```

Expected: `test tests::event_sink_round_trip ... ok`,1 passed。

- [ ] **Step 3.5: 桌面端不受影响**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 3.6: Commit**

```powershell
git add crates/handy-platform Cargo.toml
git commit -m "feat(handy-platform): define platform abstraction traits"
```

---

## 阶段 1 - 按依赖顺序迁移模块到 handy-core

模块迁移顺序基于依赖关系 (叶子优先):
1. `audio_toolkit::text` (纯函数,零依赖)
2. `audio_toolkit::constants`
3. `audio_toolkit::audio::{utils, resampler, visualizer}` (无 cpal 依赖)
4. `audio_toolkit::vad` (依赖 vad-rs,无平台依赖)
5. `managers::history` (依赖 rusqlite)
6. `settings` (拆分: 共享部分迁出, 桌面部分留下)
7. `managers::model` (依赖 reqwest, transcribe-rs)
8. `managers::transcription` (最复杂,依赖前述全部 + tauri 解耦)

---

### 任务 4: 迁移 `audio_toolkit::text` 到 handy-core

**Files:**
- Create: `crates/handy-core/src/text/mod.rs`
- Modify: `crates/handy-core/Cargo.toml`
- Modify: `crates/handy-core/src/lib.rs`
- Modify: `src-tauri/src/audio_toolkit/text.rs` — 改为 re-export
- Modify: `src-tauri/src/audio_toolkit/mod.rs` — 修复 re-export
- Modify: `src-tauri/Cargo.toml` — 加 `handy-core` 依赖

- [ ] **Step 4.1: 读取并复制源文件**

Run:
```powershell
Copy-Item src-tauri/src/audio_toolkit/text.rs crates/handy-core/src/text/mod.rs
```

(目录 `crates/handy-core/src/text/` 需先创建)
```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/text" -Force
Copy-Item src-tauri/src/audio_toolkit/text.rs crates/handy-core/src/text/mod.rs -Force
```

- [ ] **Step 4.2: 修改 handy-core/Cargo.toml 加依赖**

Modify `crates/handy-core/Cargo.toml`,在 `[dependencies]` 下追加:

```toml
regex          = { workspace = true }
strsim         = { workspace = true }
natural        = { workspace = true }
ferrous-opencc = { workspace = true }
once_cell      = { workspace = true }
```

- [ ] **Step 4.3: 让 handy-core 导出 text 模块**

Modify `crates/handy-core/src/lib.rs`,在 sanity_check 上方追加:

```rust
pub mod text;
pub use text::{apply_custom_words, filter_transcription_output};
```

- [ ] **Step 4.4: 编译 handy-core 验证**

```powershell
cargo check -p handy-core
```

Expected: PASS。如有 import 路径问题,需修改 `crates/handy-core/src/text/mod.rs` 中的 `use crate::*` 路径。

- [ ] **Step 4.5: 把 src-tauri 端的 text.rs 改为薄 re-export**

Replace entire `src-tauri/src/audio_toolkit/text.rs` content with:

```rust
//! Re-export from handy-core for backward compatibility within src-tauri.
pub use handy_core::text::{apply_custom_words, filter_transcription_output};
```

- [ ] **Step 4.6: src-tauri/Cargo.toml 加 handy-core 依赖**

Modify `src-tauri/Cargo.toml`,在 `[dependencies]` 下追加:

```toml
handy-core = { path = "../crates/handy-core" }
```

- [ ] **Step 4.7: 编译 src-tauri 验证**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。

- [ ] **Step 4.8: 把 handy-core 的 text 测试移过来 (如果原文件里有)**

读 `crates/handy-core/src/text/mod.rs`,如果文件末尾 `#[cfg(test)]` 模块已经一起复制过来,直接跑:

```powershell
cargo test -p handy-core text::
```

Expected: 与原 src-tauri 跑同等测试时数量一致,全部通过。

- [ ] **Step 4.9: 在 src-tauri 跑全量测试零回归**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部通过 (数量与基线一致)。

- [ ] **Step 4.10: Commit**

```powershell
git add crates/handy-core src-tauri/src/audio_toolkit/text.rs src-tauri/Cargo.toml
git commit -m "refactor(handy-core): move audio_toolkit::text into handy-core"
```

---

### 任务 5: 迁移 `audio_toolkit::constants` 与 `audio_toolkit::utils` 到 handy-core

**Files:**
- Create: `crates/handy-core/src/audio/mod.rs`
- Create: `crates/handy-core/src/audio/constants.rs`
- Create: `crates/handy-core/src/audio/utils.rs`
- Modify: `src-tauri/src/audio_toolkit/constants.rs`、`utils.rs` — 改为 re-export

- [ ] **Step 5.1: 先看原 utils.rs 里有没有 cpal 依赖**

Read `src-tauri/src/audio_toolkit/utils.rs` 全文。如果引用了 `cpal::*`,**不要**整体搬,只搬不含 cpal 的纯函数 (如重采样辅助、wav 写入)。`get_cpal_host` 必须留在桌面端。

- [ ] **Step 5.2: 创建 handy-core 中 audio 目录与 mod**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/audio" -Force
```

Create `crates/handy-core/src/audio/mod.rs`:

```rust
pub mod constants;
pub mod utils;

pub use constants::*;
```

- [ ] **Step 5.3: 复制 constants.rs**

```powershell
Copy-Item src-tauri/src/audio_toolkit/constants.rs crates/handy-core/src/audio/constants.rs -Force
```

- [ ] **Step 5.4: 拆分 utils.rs**

将 `src-tauri/src/audio_toolkit/utils.rs` 内容**按依赖**拆分:
- **平台无关函数** (不引用 cpal 的) → 复制到 `crates/handy-core/src/audio/utils.rs`
- **依赖 cpal 的函数** (如 `get_cpal_host`) → 留在 src-tauri,原文件继续保留这些函数

如果整个 utils.rs 都依赖 cpal: 只在 handy-core 创建空的 `utils.rs`:
```rust
//! Reserved: platform-independent audio utilities will live here.
```
并跳过 Step 5.7-5.8 的 re-export 修改。

- [ ] **Step 5.5: handy-core/Cargo.toml 加音频依赖**

Modify `crates/handy-core/Cargo.toml` `[dependencies]`:

```toml
hound   = { workspace = true }
rubato  = { workspace = true }
rustfft = { workspace = true }
```

- [ ] **Step 5.6: 在 lib.rs 导出 audio**

Modify `crates/handy-core/src/lib.rs`:

```rust
pub mod audio;
pub mod text;

pub use text::{apply_custom_words, filter_transcription_output};
```

- [ ] **Step 5.7: src-tauri 端 constants.rs 改 re-export**

Replace `src-tauri/src/audio_toolkit/constants.rs`:

```rust
pub use handy_core::audio::constants::*;
```

- [ ] **Step 5.8: src-tauri 端 utils.rs 改混合 (re-export + cpal 部分保留)**

Modify `src-tauri/src/audio_toolkit/utils.rs`:
- 顶部加: `pub use handy_core::audio::utils::*;`
- 保留所有依赖 cpal 的函数

- [ ] **Step 5.9: 编译两边**

```powershell
cargo check -p handy-core
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: 两个都 PASS。

- [ ] **Step 5.10: 全量测试**

```powershell
cargo test -p handy-core --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部通过。

- [ ] **Step 5.11: Commit**

```powershell
git add crates/handy-core src-tauri/src/audio_toolkit/constants.rs src-tauri/src/audio_toolkit/utils.rs
git commit -m "refactor(handy-core): move audio constants & platform-independent utils"
```

---

### 任务 6: 迁移 audio 子模块的纯逻辑 (resampler / visualizer)

**Files:**
- Create: `crates/handy-core/src/audio/resampler.rs`
- Create: `crates/handy-core/src/audio/visualizer.rs`
- Modify: `src-tauri/src/audio_toolkit/audio/resampler.rs` `visualizer.rs` — re-export

- [ ] **Step 6.1: 检查源文件无 cpal 依赖**

Read `src-tauri/src/audio_toolkit/audio/resampler.rs` 与 `visualizer.rs`。验证只用 `rubato`、`rustfft` 等纯库。

如果发现 cpal 依赖: 停下来,把混合到的部分单独剥离再迁移 (类似 Step 5.4)。

- [ ] **Step 6.2: 复制源文件**

```powershell
Copy-Item src-tauri/src/audio_toolkit/audio/resampler.rs crates/handy-core/src/audio/resampler.rs -Force
Copy-Item src-tauri/src/audio_toolkit/audio/visualizer.rs crates/handy-core/src/audio/visualizer.rs -Force
```

- [ ] **Step 6.3: 更新 handy-core audio mod**

Modify `crates/handy-core/src/audio/mod.rs`:

```rust
pub mod constants;
pub mod resampler;
pub mod utils;
pub mod visualizer;

pub use constants::*;
```

- [ ] **Step 6.4: 修复 import 路径**

读新复制过来的 `resampler.rs` 与 `visualizer.rs`,把 `use crate::audio_toolkit::*` 改成 `use crate::audio::*` (在 handy-core 内).

- [ ] **Step 6.5: 编译 handy-core**

```powershell
cargo check -p handy-core
```

Expected: PASS。

- [ ] **Step 6.6: src-tauri 端改 re-export**

Replace `src-tauri/src/audio_toolkit/audio/resampler.rs`:

```rust
pub use handy_core::audio::resampler::*;
```

Replace `src-tauri/src/audio_toolkit/audio/visualizer.rs`:

```rust
pub use handy_core::audio::visualizer::*;
```

- [ ] **Step 6.7: 编译 src-tauri & 测试**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test -p handy-core --lib
```

Expected: 全部 PASS。

- [ ] **Step 6.8: Commit**

```powershell
git add crates/handy-core src-tauri/src/audio_toolkit/audio
git commit -m "refactor(handy-core): move resampler & visualizer"
```

---

### 任务 7: 迁移 VAD 整体

**Files:**
- Create: `crates/handy-core/src/vad/mod.rs`
- Create: `crates/handy-core/src/vad/silero.rs`
- Create: `crates/handy-core/src/vad/smoothed.rs`
- Modify: `src-tauri/src/audio_toolkit/vad/*.rs` — re-export

- [ ] **Step 7.1: 复制整个 vad 目录**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/vad" -Force
Copy-Item src-tauri/src/audio_toolkit/vad/mod.rs crates/handy-core/src/vad/mod.rs -Force
Copy-Item src-tauri/src/audio_toolkit/vad/silero.rs crates/handy-core/src/vad/silero.rs -Force
Copy-Item src-tauri/src/audio_toolkit/vad/smoothed.rs crates/handy-core/src/vad/smoothed.rs -Force
```

- [ ] **Step 7.2: 修复 imports**

读 3 个新文件,把 `use crate::audio_toolkit::*` 改为 `use crate::*`。

- [ ] **Step 7.3: handy-core/Cargo.toml 加 vad-rs**

```toml
vad-rs = { workspace = true }
```

- [ ] **Step 7.4: handy-core/src/lib.rs 导出**

```rust
pub mod audio;
pub mod text;
pub mod vad;

pub use text::{apply_custom_words, filter_transcription_output};
pub use vad::{SileroVad, VoiceActivityDetector};
```

- [ ] **Step 7.5: 编译 handy-core**

```powershell
cargo check -p handy-core
```

Expected: PASS。

- [ ] **Step 7.6: src-tauri 端三个 vad 文件改 re-export**

Replace each of `src-tauri/src/audio_toolkit/vad/{mod,silero,smoothed}.rs`:

`mod.rs`:
```rust
pub use handy_core::vad::*;
```

`silero.rs`:
```rust
pub use handy_core::vad::silero::*;
```

`smoothed.rs`:
```rust
pub use handy_core::vad::smoothed::*;
```

- [ ] **Step 7.7: 跑测试**

```powershell
cargo test -p handy-core --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部通过。

- [ ] **Step 7.8: Commit**

```powershell
git add crates/handy-core src-tauri/src/audio_toolkit/vad
git commit -m "refactor(handy-core): move VAD (silero, smoothed) into handy-core"
```

---

### 任务 8: 迁移 history (sqlite 历史记录)

**Files:**
- Create: `crates/handy-core/src/history/mod.rs`
- Modify: `src-tauri/src/managers/history.rs` — re-export (薄)
- Modify: `crates/handy-core/Cargo.toml` — 加 rusqlite

- [ ] **Step 8.1: 检查 history.rs 与 tauri 的耦合**

Read `src-tauri/src/managers/history.rs`。识别引用了 `tauri::*` 的部分 (通常是路径获取 `app.path().app_data_dir()`)。

- [ ] **Step 8.2: 复制并解耦**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/history" -Force
Copy-Item src-tauri/src/managers/history.rs crates/handy-core/src/history/mod.rs -Force
```

修改 `crates/handy-core/src/history/mod.rs`:
- 把所有依赖 `AppHandle` / `tauri::path` 的代码改为接受 `db_path: &Path` 参数 (通过构造函数注入)
- 公开构造: `pub fn new(db_path: &Path) -> Result<Self>`
- 移除所有 `use tauri::*`

- [ ] **Step 8.3: handy-core/Cargo.toml 加依赖**

```toml
rusqlite           = { workspace = true }
rusqlite_migration = { workspace = true }
chrono             = { workspace = true }
```

- [ ] **Step 8.4: handy-core/src/lib.rs 导出**

```rust
pub mod history;
pub use history::*;
```

(根据你的 history 模块导出什么,加合适的 `use`)

- [ ] **Step 8.5: 编译 handy-core**

```powershell
cargo check -p handy-core
```

Expected: PASS。

- [ ] **Step 8.6: src-tauri 端 history.rs 改薄封装 (注入路径)**

Replace `src-tauri/src/managers/history.rs` 内容为:

```rust
//! Thin desktop wrapper around `handy_core::history`. Resolves the sqlite
//! path via `tauri::path::PathResolver` and delegates everything else.

use anyhow::Result;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub use handy_core::history::*;

pub fn open_for_app(app: &AppHandle) -> Result<HistoryRepo> {
    let dir: PathBuf = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("app_data_dir: {e}"))?;
    std::fs::create_dir_all(&dir)?;
    HistoryRepo::new(&dir.join("history.db"))
}
```

(具体类型名 `HistoryRepo` 以源文件实际公开的为准)

- [ ] **Step 8.7: 改 callers 使用新 API**

Grep 桌面端引用 history 的位置:
```powershell
```

使用 Grep 工具搜索 `HistoryManager::new`、`HistoryRepo::new`、`history::` 在 `src-tauri/src/` 下的所有调用,改为通过 `open_for_app(&app)` 获取实例。

- [ ] **Step 8.8: 编译 src-tauri**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。如有调用点漏改,补齐。

- [ ] **Step 8.9: 跑测试**

```powershell
cargo test -p handy-core --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部通过。

- [ ] **Step 8.10: 跑桌面端手工 smoke test**

```powershell
cd src-tauri
. ../scripts/enter-build-env.ps1
cd ..
bun run tauri dev
```

手动验证: 录一段音、转录、看历史页面里出现新条目。
关闭 dev server。

- [ ] **Step 8.11: Commit**

```powershell
git add crates/handy-core src-tauri/src/managers/history.rs src-tauri/src
git commit -m "refactor(handy-core): move history repo, inject db path"
```

---

### 任务 9: 拆分 settings — 共享部分到 handy-core

**Files:**
- Create: `crates/handy-core/src/settings/mod.rs`
- Modify: `src-tauri/src/settings.rs` — 共享 struct re-export,桌面专属字段保留

- [ ] **Step 9.1: 读现状**

Read `src-tauri/src/settings.rs` 全文。把字段分类:
- **共享** (移动端也需要): `selected_model`, `language`, `vad_threshold`, `custom_words`, `output_mode_default`, `model_unload_timeout` 等
- **桌面专属**: `global_shortcut`, `autostart`, `tray_enabled`, `overlay_enabled`, `whisper_accelerator`, `ort_accelerator` 等

- [ ] **Step 9.2: 在 handy-core 建 settings 模块,只放共享字段**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/settings" -Force
```

Create `crates/handy-core/src/settings/mod.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoreSettings {
    pub selected_model_id: Option<String>,
    pub language: String,
    pub vad_threshold: f32,
    pub custom_words: Vec<String>,
    pub output_mode_default: String,
    pub model_unload_timeout_secs: u64,
}

impl CoreSettings {
    pub fn defaults() -> Self {
        Self {
            selected_model_id: None,
            language: "auto".into(),
            vad_threshold: 0.5,
            custom_words: Vec::new(),
            output_mode_default: "clipboard".into(),
            model_unload_timeout_secs: 300,
        }
    }
}
```

(字段名以源文件为准,这里只是骨架示意)

- [ ] **Step 9.3: 在 lib.rs 导出**

Modify `crates/handy-core/src/lib.rs`:

```rust
pub mod settings;
pub use settings::CoreSettings;
```

- [ ] **Step 9.4: 桌面端 settings 改为组合**

Modify `src-tauri/src/settings.rs`:

```rust
use serde::{Deserialize, Serialize};
pub use handy_core::settings::CoreSettings;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DesktopSettings {
    #[serde(flatten)]
    pub core: CoreSettings,

    // 桌面专属字段:
    pub global_shortcut: Option<String>,
    pub autostart_enabled: bool,
    pub tray_enabled: bool,
    pub overlay_enabled: bool,
    pub whisper_accelerator: WhisperAcceleratorSetting,
    pub ort_accelerator: OrtAcceleratorSetting,
    // ... 其余桌面专属字段
}

// 保留原 settings.rs 的 get_settings / set_settings 等函数,内部使用 DesktopSettings。
// 如果 callers 用了 `Settings` 类型,加一个 type alias 保兼容:
pub type Settings = DesktopSettings;
```

- [ ] **Step 9.5: 编译 + 测试 + smoke**

```powershell
cargo check -p handy-core
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部 PASS。

跑一次 `bun run tauri dev`,在设置页改一个值 (如语言)、重启,确认值持久化。

- [ ] **Step 9.6: Commit**

```powershell
git add crates/handy-core src-tauri/src/settings.rs
git commit -m "refactor(handy-core): extract shared CoreSettings, keep desktop-only fields in DesktopSettings"
```

---

### 任务 10: 迁移 model 管理 (含模型下载)

**Files:**
- Create: `crates/handy-core/src/model/mod.rs`
- Modify: `src-tauri/src/managers/model.rs` — 改薄封装
- Modify: `src-tauri/Cargo.toml` — 把 reqwest/transcribe-rs 等依赖也加到 handy-core

- [ ] **Step 10.1: 检查耦合点**

Read `src-tauri/src/managers/model.rs` 全文。识别:
- 路径获取: `app.path().app_data_dir()` → 注入 `models_dir: PathBuf`
- 事件发送: `app.emit("model/...", ...)` → 注入 `dyn EventSink`
- 引擎种类枚举 `EngineType` → 一起搬

- [ ] **Step 10.2: 复制并解耦**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/model" -Force
Copy-Item src-tauri/src/managers/model.rs crates/handy-core/src/model/mod.rs -Force
```

修改 `crates/handy-core/src/model/mod.rs`:
- 移除所有 `use tauri::*`、`tauri::AppHandle`
- `ModelManager::new(app: &AppHandle)` → `ModelManager::new(models_dir: PathBuf, event_sink: Arc<dyn handy_platform::EventSink>) -> Self`
- 所有 `app.emit("...", payload)` 改为 `event_sink.emit_json("...", serde_json::to_value(&payload).unwrap_or_default())`
- 用 `handy_platform::EventSink` 替代 `tauri::Emitter`

- [ ] **Step 10.3: handy-core/Cargo.toml 加依赖**

```toml
reqwest          = { workspace = true }
futures-util     = { workspace = true }
tokio            = { workspace = true }
transcribe-rs    = { workspace = true }
tar              = "0.4.44"
flate2           = "1.0"

[dependencies.handy-platform]
path = "../handy-platform"
```

- [ ] **Step 10.4: lib.rs 导出**

```rust
pub mod model;
pub use model::*;
```

(具体导出名以源文件为准)

- [ ] **Step 10.5: 编译 handy-core**

```powershell
cargo check -p handy-core
```

Expected: PASS。如失败,根据 cargo 报错逐个修 import / 类型。

- [ ] **Step 10.6: src-tauri 端 model.rs 改薄封装**

Replace `src-tauri/src/managers/model.rs`:

```rust
//! Desktop wrapper around `handy_core::model::ModelManager`.

use anyhow::Result;
use handy_platform::EventSink;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub use handy_core::model::*;

struct TauriEventSink(AppHandle);

impl EventSink for TauriEventSink {
    fn emit_json(&self, event_name: &str, payload: Value) {
        if let Err(e) = self.0.emit(event_name, payload) {
            log::warn!("emit {event_name}: {e}");
        }
    }
}

pub fn new_for_app(app: &AppHandle) -> Result<Arc<ModelManager>> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("app_data_dir: {e}"))?
        .join("models");
    std::fs::create_dir_all(&dir)?;
    let sink = Arc::new(TauriEventSink(app.clone()));
    Ok(Arc::new(ModelManager::new(dir, sink)))
}
```

- [ ] **Step 10.7: 改 callers**

Grep `ModelManager::new` 在 `src-tauri/src/` 全部出现,改为 `model::new_for_app(&app)?`。

- [ ] **Step 10.8: 编译并测试**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: PASS。

- [ ] **Step 10.9: 桌面端 smoke test**

`bun run tauri dev`,验证:
1. 模型列表能加载
2. 点击下载一个未安装的小模型,下载进度事件正常 (前端 UI 有反应)
3. 关闭 dev server

- [ ] **Step 10.10: Commit**

```powershell
git add crates/handy-core src-tauri/src/managers/model.rs src-tauri/src
git commit -m "refactor(handy-core): move ModelManager, inject models dir & event sink"
```

---

### 任务 11: 迁移 transcription manager (最复杂)

**Files:**
- Create: `crates/handy-core/src/transcription/mod.rs`
- Create: `crates/handy-core/src/transcription/engine.rs`
- Create: `crates/handy-core/src/transcription/events.rs`
- Modify: `src-tauri/src/managers/transcription.rs` — 薄封装

- [ ] **Step 11.1: 阅读全文,清点耦合点**

Read `src-tauri/src/managers/transcription.rs` 全文。耦合点清单:
- `AppHandle`、`tauri::Emitter` → 用 `EventSink` 替代
- `crate::settings::get_settings` → 改为接受 `Arc<dyn Fn() -> CoreSettings + Send + Sync>` 或直接传 `CoreSettings`
- `OrtAcceleratorSetting`、`WhisperAcceleratorSetting` 是桌面专属 → 抽象成 `EngineAcceleratorPreference` (枚举 Auto/Cpu/Gpu)

- [ ] **Step 11.2: 建空骨架文件**

```powershell
New-Item -ItemType Directory -Path "crates/handy-core/src/transcription" -Force
```

Create `crates/handy-core/src/transcription/events.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelStateEvent {
    pub event_type: String,
    pub model_id: Option<String>,
    pub model_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptionEvent {
    pub stage: String,
    pub text: Option<String>,
    pub partial: bool,
}
```

Create `crates/handy-core/src/transcription/engine.rs`:

```rust
//! Wraps transcribe-rs engines behind a single enum.
//! Moved verbatim from src-tauri/src/managers/transcription.rs::LoadedEngine

use anyhow::Result;
use transcribe_rs::{
    onnx::{
        canary::CanaryModel,
        gigaam::GigaAMModel,
        moonshine::{MoonshineModel, MoonshineVariant, StreamingModel},
        parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
        sense_voice::{SenseVoiceModel, SenseVoiceParams},
        Quantization,
    },
    whisper_cpp::{WhisperEngine, WhisperInferenceParams},
    SpeechModel, TranscribeOptions,
};

pub enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetModel),
    Moonshine(MoonshineModel),
    MoonshineStreaming(StreamingModel),
    SenseVoice(SenseVoiceModel),
    GigaAM(GigaAMModel),
    Canary(CanaryModel),
}

#[derive(Clone, Copy, Debug)]
pub enum EngineAcceleratorPreference {
    Auto,
    Cpu,
    Gpu,
}
```

Create `crates/handy-core/src/transcription/mod.rs`:

把原 `transcription.rs` 中除以下内容外的全部代码复制过来:
- `use tauri::*` 改为 `use handy_platform::EventSink;`
- 结构体 `TranscriptionManager` 改为 `TranscriptionService`,字段 `app_handle: AppHandle` 删除,新增 `event_sink: Arc<dyn EventSink>`
- `new(app_handle: &AppHandle, model_manager: Arc<ModelManager>)` 改为 `new(model_manager: Arc<ModelManager>, event_sink: Arc<dyn EventSink>) -> Result<Self>`
- 所有 `self.app_handle.emit("...", payload)` 改为 `handy_platform::event_sink::emit(&*self.event_sink, "...", &payload)`
- 设置查询: `get_settings()` 改为接受 `&CoreSettings`,通过参数传入相关字段;或者保存 `settings_snapshot: Arc<RwLock<CoreSettings>>`
- `OrtAcceleratorSetting` / `WhisperAcceleratorSetting` 替换为 `EngineAcceleratorPreference`

加 mod 声明在 mod.rs 顶部:
```rust
pub mod engine;
pub mod events;
pub use engine::{EngineAcceleratorPreference, LoadedEngine};
pub use events::{ModelStateEvent, TranscriptionEvent};
```

- [ ] **Step 11.3: handy-core/Cargo.toml 加 transcribe-rs**

确认 transcribe-rs 已在依赖 (任务 10 中应该已加)。如果还没有:

```toml
transcribe-rs = { workspace = true }
```

- [ ] **Step 11.4: handy-core/src/lib.rs 导出**

```rust
pub mod transcription;
pub use transcription::{TranscriptionService, ModelStateEvent, EngineAcceleratorPreference};
```

- [ ] **Step 11.5: 编译 handy-core**

```powershell
cargo check -p handy-core
```

Expected: PASS。这一步很可能首次失败,根据 cargo 报错逐步修。可能的修复:
- 缺少类型 import → 加 `use` 或在 events.rs / engine.rs 中重新定义
- `Settings` 字段访问 → 改为通过 `CoreSettings` 提供的字段
- 线程同步类型 → 直接复用 `std::sync::Arc / Mutex / Condvar` 即可

- [ ] **Step 11.6: src-tauri 端 transcription.rs 改薄封装**

Replace `src-tauri/src/managers/transcription.rs` 内容为:

```rust
//! Desktop wrapper. Provides app-handle-based factory + tauri::Emitter sink.

use anyhow::Result;
use handy_platform::EventSink;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub use handy_core::transcription::*;

struct TauriEventSink(AppHandle);

impl EventSink for TauriEventSink {
    fn emit_json(&self, event_name: &str, payload: Value) {
        if let Err(e) = self.0.emit(event_name, payload) {
            log::warn!("emit {event_name}: {e}");
        }
    }
}

pub fn new_for_app(
    app: &AppHandle,
    model_manager: Arc<crate::managers::model::ModelManager>,
) -> Result<TranscriptionService> {
    let sink = Arc::new(TauriEventSink(app.clone()));
    TranscriptionService::new(model_manager, sink)
}
```

- [ ] **Step 11.7: 更新 callers**

在 `src-tauri/src/lib.rs` 与 `src-tauri/src/commands/*` 中,找出 `TranscriptionManager::new(...)` 调用,改为 `transcription::new_for_app(&app, model_manager.clone())?`。

设置查询逻辑: 桌面端在调用 service 方法前,先把当前 `DesktopSettings.core` 传进去 (例如 `service.update_settings(settings.core.clone())`)。如果原代码每次都从 `get_settings()` 拿,可以加一个 `Arc<RwLock<CoreSettings>>` 共享给 service,由桌面端 settings 写入端负责更新。

- [ ] **Step 11.8: 编译 src-tauri**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS。如果有调用点漏改,补齐。

- [ ] **Step 11.9: 全量测试**

```powershell
cargo test -p handy-core --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: 全部通过。

- [ ] **Step 11.10: 桌面端 smoke test (关键)**

```powershell
. scripts/enter-build-env.ps1
bun run tauri dev
```

手工跑完整流程:
1. 选 Parakeet 模型,录一段 5 秒语音 → 转录成功 → 文本进剪贴板
2. 切到 Whisper 模型,录一段 → 转录成功
3. 查看历史页有 2 条记录
4. 改语言设置 → 重启 → 设置保留
5. 关闭 dev server

如果任何一步行为与抽取前不一致,**回滚最近 commit 并修复**:
```powershell
git revert HEAD --no-edit
```

- [ ] **Step 11.11: Commit**

```powershell
git add crates/handy-core src-tauri/src/managers/transcription.rs src-tauri/src
git commit -m "refactor(handy-core): extract TranscriptionService, decouple from tauri"
```

---

### 任务 12: 把 audio_toolkit/mod.rs 收拾干净 + 删除桌面端冗余 re-export

**Files:**
- Modify: `src-tauri/src/audio_toolkit/mod.rs`
- 可能删除: 那些已经成为单行 re-export 的文件 (text.rs, constants.rs, vad/*.rs, audio/{resampler,visualizer}.rs)

- [ ] **Step 12.1: 决定保留 re-export 还是删文件**

读 `src-tauri/src/audio_toolkit/mod.rs` 当前内容。

策略: **保留薄 re-export 文件**,这样如果将来桌面端要在中间插一层 (如埋点、日志),有地方可插。但要在每个文件顶部加注释:

例如 `src-tauri/src/audio_toolkit/text.rs`:
```rust
//! Re-export shim. Real implementation lives in `handy_core::text`.
//! Keep this file so we have a place to add desktop-only middleware later.
pub use handy_core::text::{apply_custom_words, filter_transcription_output};
```

为所有再导出文件加这个 doc comment。

- [ ] **Step 12.2: 确认 mod.rs 的 pub use 正确**

Modify `src-tauri/src/audio_toolkit/mod.rs`:

```rust
//! Desktop audio toolkit. Pure logic lives in handy-core; cpal-bound code stays here.

pub mod audio;
pub mod constants;
pub mod text;
pub mod utils;
pub mod vad;

pub use audio::{
    find_ai_mouse_microphone_name, is_microphone_access_denied, list_input_devices,
    list_output_devices, save_wav_file, AudioRecorder, CpalDeviceInfo,
};
pub use handy_core::text::{apply_custom_words, filter_transcription_output};
pub use handy_core::vad::{SileroVad, VoiceActivityDetector};
pub use utils::get_cpal_host;
```

- [ ] **Step 12.3: 编译并测试**

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Expected: PASS。

- [ ] **Step 12.4: Commit**

```powershell
git add src-tauri/src/audio_toolkit
git commit -m "refactor: clean up audio_toolkit re-exports, document shim files"
```

---

## 阶段 1 收尾: 验证 + GHA 跑通

### 任务 13: 完善 GitHub Actions workflow

**Files:**
- Modify: `.github/workflows/mobile-ci.yml`

- [ ] **Step 13.1: 把 workflow 升级为正式版**

Replace `.github/workflows/mobile-ci.yml`:

```yaml
name: Mobile CI

on:
  push:
    branches: [main, mobile/**]
  pull_request:
    branches: [main]
  workflow_dispatch:

jobs:
  core-tests:
    name: handy-core unit tests
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: handy-core
      - name: cargo test handy-core
        run: cargo test -p handy-core --lib --all-features
      - name: cargo test handy-platform
        run: cargo test -p handy-platform --lib --all-features

  android-cross-compile:
    name: Cross-compile handy-core to Android
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-linux-android,armv7-linux-androideabi
      - uses: nttld/setup-ndk@v1
        id: setup-ndk
        with:
          ndk-version: r26d
          local-cache: true
      - name: Configure cargo for NDK
        run: |
          mkdir -p ~/.cargo
          cat >> ~/.cargo/config.toml <<EOF
          [target.aarch64-linux-android]
          linker = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"
          ar     = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"

          [target.armv7-linux-androideabi]
          linker = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/armv7a-linux-androideabi24-clang"
          ar     = "${{ steps.setup-ndk.outputs.ndk-path }}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar"
          EOF
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: android-cross
      - name: cargo check handy-core for aarch64-linux-android
        env:
          ANDROID_NDK_HOME: ${{ steps.setup-ndk.outputs.ndk-path }}
        run: cargo check -p handy-core --target aarch64-linux-android

      - name: cargo check handy-platform for aarch64-linux-android
        env:
          ANDROID_NDK_HOME: ${{ steps.setup-ndk.outputs.ndk-path }}
        run: cargo check -p handy-platform --target aarch64-linux-android

  desktop-no-regression:
    name: Desktop build still works
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: desktop-${{ matrix.os }}
      - name: Install Linux deps
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libgtk-layer-shell-dev
      - name: cargo check src-tauri
        working-directory: src-tauri
        run: cargo check --lib
```

- [ ] **Step 13.2: Push 触发**

```powershell
git add .github/workflows/mobile-ci.yml
git commit -m "ci(mobile): full workflow — core tests, android cross-compile, desktop check"
git push
```

- [ ] **Step 13.3: 等 GHA 跑完**

打开 GitHub Actions 页面,等三个 job 全部跑完:
1. `core-tests (ubuntu)` 与 `core-tests (macos)` — 应当 PASS
2. `android-cross-compile` — **目标 PASS**。如果失败,查看错误:
   - 如果 handy-core 失败: 说明 core 里还残留了不能在 Android 上跑的依赖,需要回去检查 (常见: rusqlite 在 bundled feature 应该 OK;reqwest 默认 native-tls 在 Android 需要换 rustls-tls)
   - 修复方法: 给 handy-core 加 feature flag,Android target 时关掉相应依赖
3. `desktop-no-regression (ubuntu/windows/macos)` — 必须 PASS

- [ ] **Step 13.4: 如果 android-cross-compile 失败,修复**

最常见的修复 — reqwest 在 Android 上需要 rustls:

修改 `Cargo.toml` (根) `[workspace.dependencies]`:
```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
```

修改 `src-tauri/Cargo.toml` 显式打开 native-tls (如果桌面端确实需要):
```toml
reqwest = { workspace = true, features = ["native-tls"] }
```

如果是 rusqlite 失败: 检查 `bundled` feature 已开 (它包含 sqlite C 源码,Android NDK 能编译)。

如果是 transcribe-rs 失败: 这是更深的问题,需要在 `vendor/transcribe-rs` 中加 Android-specific 补丁或者排除某些 feature。把错误信息追加到 `docs/mobile/spike-findings.md` 并停下来与人讨论;不要在本任务里继续推进。

修复后:
```powershell
git add Cargo.toml src-tauri/Cargo.toml docs/mobile/spike-findings.md
git commit -m "build: fix Android cross-compile for handy-core"
git push
```

继续等 GHA 重跑。

- [ ] **Step 13.5: 三个 job 全 PASS 后 commit "milestone"**

```powershell
git tag mobile-phase-1-complete
git push origin mobile-phase-1-complete
```

---

### 任务 14: 文档与 PR

**Files:**
- Create: `docs/mobile/README.md`
- Modify: `docs/superpowers/specs/2026-05-26-handy-mobile-design.md` — 更新章节"变更记录"

- [ ] **Step 14.1: 写 docs/mobile/README.md**

Create `docs/mobile/README.md`:

```markdown
# Handy 移动端

> 状态: 阶段 1 完成 (Rust core 抽取),阶段 2 待启动 (Android UI + 前台录音)

## 文档索引

- [设计方案](../superpowers/specs/2026-05-26-handy-mobile-design.md) — 总体方案
- [阶段 0+1 实施计划](../superpowers/plans/2026-05-26-handy-mobile-phase-0-1.md) — 已完成
- [Spike 发现](./spike-findings.md) — Android 交叉编译初次尝试结论
- [Android 构建说明](./android-build.md) — 待写
- [输入法设计](./ime-design.md) — 待写

## 已完成 (阶段 0+1)

- [x] Cargo workspace 引入
- [x] `crates/handy-core` 创建并迁入: text / audio (无 cpal 部分) / vad / history / settings / model / transcription
- [x] `crates/handy-platform` 创建: AudioCapture / TextOutput / AppStorage / EventSink trait
- [x] 桌面端零回归 (CI + 手动 smoke 通过)
- [x] `handy-core` 可交叉编译到 aarch64-linux-android (GHA `android-cross-compile` 通过)

## 下一步 (阶段 2)

启动 `src-mobile/` Tauri 工程,实现 Android UI 与前台录音服务。参见后续实施计划 (待写)。
```

- [ ] **Step 14.2: 更新设计文档变更记录**

Modify `docs/superpowers/specs/2026-05-26-handy-mobile-design.md` 第 12 节追加一行:

```markdown
| 2026-05-26 | 阶段 0+1 完成: workspace 引入,handy-core 抽取,GHA 通过 | Claude Code + 唐海文 |
```

- [ ] **Step 14.3: Commit & push**

```powershell
git add docs/mobile/README.md docs/superpowers/specs/2026-05-26-handy-mobile-design.md
git commit -m "docs(mobile): mark phase 0+1 complete, point to next phase"
git push
```

- [ ] **Step 14.4: 开 PR**

```powershell
gh pr create --title "feat(mobile): phase 0+1 — workspace + handy-core extraction" --body @"
## Summary
- 引入 Cargo workspace,新增 crates/handy-core 与 crates/handy-platform
- 把所有平台无关的 Rust 逻辑从 src-tauri 迁到 handy-core (text, audio utils, vad, history, settings, model, transcription)
- 桌面端通过 trait 注入 EventSink/AppStorage,行为零回归
- handy-core 可交叉编译到 aarch64-linux-android

## 验证
- [x] cargo test -p handy-core 全部通过
- [x] cargo test --manifest-path src-tauri/Cargo.toml --lib 全部通过 (零回归)
- [x] GHA 三个 job 全 PASS: core-tests / android-cross-compile / desktop-no-regression
- [x] 手动 smoke test: 录音 → 转录 → 历史 → 设置 持久化 全部正常

## 不在本 PR 范围
- src-mobile/ Tauri 工程
- Android UI / IME / 前台录音
- iOS

参见 [设计文档](docs/superpowers/specs/2026-05-26-handy-mobile-design.md)
"@
```

完成。

---

## 完成判据 (阶段 0+1 全部任务)

1. **PR 已开**且 GitHub Actions 全绿
2. **桌面端零回归**: 现有 `cargo test` 全部通过,手动 smoke (录音/转录/历史/模型选择/语言切换) 行为与改动前一致
3. **handy-core 可独立编译**: `cargo check -p handy-core --target aarch64-linux-android` 通过
4. **trait 已就位**: handy-platform 中 `AudioCapture`、`TextOutput`、`AppStorage`、`EventSink` 已定义,桌面端有具体实现注入 handy-core
5. **文档已更新**: docs/mobile/README.md 反映当前状态,设计文档变更记录已追加

阶段 2 (src-mobile Tauri 工程 + Android UI + 前台录音) 将在另一份实施计划中处理。
