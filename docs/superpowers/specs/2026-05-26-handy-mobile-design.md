# Handy 移动端设计文档 (Handy Mobile)

> **状态**: 设计稿,待评审
> **日期**: 2026-05-26
> **作者**: Claude Code (Opus 4.7) + 唐海文
> **关联**: [docs/system-requirements.md](../../system-requirements.md), [docs/bundled-models.md](../../bundled-models.md)

---

## 1. 背景与目标

### 1.1 背景
Handy 当前是基于 Tauri 2.x 的跨平台**桌面端**语音转文字应用,后端 Rust + 前端 React,使用 `transcribe-rs` 包装 whisper.cpp 与 ONNX (Parakeet) 双引擎。代码组织见 [CLAUDE.md](../../../CLAUDE.md)。

`src-tauri/Cargo.toml` 第 84–110 行已经为 `target_os = "android"` / `target_os = "ios"` 做了部分依赖排除,说明上游 Tauri 已具备移动端能力,但当前未启用任何移动 target。

### 1.2 目标
推出 Handy 的 **Android 移动端 MVP**,核心场景:
- 在手机上**离线**完成"录音 → 转录 → 输出文字"全流程
- 可在锁屏/后台继续录音
- 可作为**系统输入法**在任意 App 中语音输入

**iOS 列为第二阶段**,本文档涉及 iOS 部分仅作架构占位,不进 MVP 排期。

### 1.3 非目标 (Out of Scope)
- ❌ 桌面端与移动端的账号/历史云同步 (后续 RFC)
- ❌ 全局快捷键 (移动端用 IME 替代)
- ❌ 系统托盘、自启动 (移动端概念不适用)
- ❌ 后处理 AI (调用云端 LLM) — 二期再加
- ❌ iOS 首版发布

---

## 2. 高层架构

### 2.1 分层

| 层 | 桌面端 | 移动端 (Android) | 共享 |
|----|--------|------------------|------|
| UI 框架 | React + Tauri (`src/`) | React + Tauri Mobile (`src-mobile-ui/`) | 部分纯 UI 组件 |
| 应用入口 | `src-tauri/` | `src-mobile/` | — |
| 业务逻辑 | 经 `handy-core` | 经 `handy-core` | **`crates/handy-core`** |
| 推理引擎 | whisper.cpp + ONNX | whisper.cpp + ONNX | **同一个 `transcribe-rs`** |
| 音频采集 | `cpal` | Android: Oboe/AAudio (JNI) | `trait AudioCapture` |
| 文本输出 | `enigo` + `rdev` 模拟键入 | `InputMethodService.commitText()` | `trait TextOutput` |
| 持久化 | `tauri-plugin-store` + `rusqlite` | `rusqlite` (App 私有目录) | `handy-core::history`, `handy-core::settings` |
| 全局快捷键 | `tauri-plugin-global-shortcut` | ❌ (IME 替代) | — |
| 系统托盘 | `tauri::tray-icon` | ❌ (前台服务 + 通知替代) | — |

### 2.2 关键决策
1. **monorepo 双 Tauri 应用 + 共享 Rust 核心 crate** (方案 A)
   - 桌面端与移动端各有独立 Tauri 工程,共享 `handy-core` / `handy-platform`
   - 平台差异通过 trait 注入,而不是 cfg-guard 满天飞
2. **Android 优先,iOS 第二阶段**
3. **模型策略**: 默认 **Parakeet V3** (纯 CPU,~478MB),可选下载 **Whisper Small Q5** (~150MB 量化版);APK 首发不内置模型,首次启动下载
4. **手机独立运行**: MVP 不与桌面端做任何同步,完全本地

---

## 3. 目录结构

```
Handy/                                  # monorepo 根
├── Cargo.toml                          # 新增: workspace 根
├── package.json                        # 改: bun workspaces (src, src-mobile-ui)
│
├── src-tauri/                          # 桌面端 Tauri 工程 (改: 依赖 handy-core)
│   └── ...                             # 现状保持,逐步把逻辑迁到 handy-core
│
├── src-mobile/                         # 新增: 移动端 Tauri 工程
│   ├── src/
│   │   ├── lib.rs                      # mobile_entry_point
│   │   ├── commands/                   # 移动端 Tauri commands
│   │   └── platform/                   # Android/iOS 平台胶水
│   ├── gen/                            # cargo tauri android init 生成
│   │   ├── android/                    # Gradle 工程
│   │   │   └── app/src/main/java/...   # IME、ForegroundService Kotlin
│   │   └── apple/                      # Xcode 工程 (阶段二)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
│
├── crates/                             # 新增: 共享 Rust 库
│   ├── handy-core/                     # 纯逻辑,不依赖 tauri/cpal/enigo/gtk
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── transcription/         # 转录管线 (从 managers/transcription.rs 抽取)
│   │   │   ├── audio_toolkit/         # VAD、重采样 (从 src-tauri/src/audio_toolkit/ 抽取)
│   │   │   ├── model/                 # 模型下载与管理 (从 managers/model.rs 抽取)
│   │   │   ├── history/               # 历史 sqlite (从 managers/history.rs 抽取)
│   │   │   └── settings/              # 设置 schema (从 settings.rs 抽取共享部分)
│   │   └── Cargo.toml
│   │
│   ├── handy-platform/                 # 平台抽象 trait
│   │   ├── src/
│   │   │   ├── audio.rs                # trait AudioCapture
│   │   │   ├── text_output.rs          # trait TextOutput
│   │   │   ├── storage.rs              # trait AppStorage
│   │   │   └── notification.rs         # trait Notifier
│   │   └── Cargo.toml
│   │
│   └── handy-mobile/                   # 移动端独有 Rust 实现
│       ├── src/
│       │   ├── ime.rs                  # JNI 桥接 InputMethodService
│       │   ├── foreground_service.rs   # 前台录音服务桥接
│       │   └── android_audio.rs        # Oboe 绑定 (AudioCapture 实现)
│       └── Cargo.toml
│
├── src/                                # 桌面端 React (现状)
├── src-mobile-ui/                      # 新增: 移动端 React 工程
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json                   # 路径别名: @shared → ../src/components/shared
│   └── src/
│       ├── App.tsx                     # 移动专属布局
│       ├── pages/
│       │   ├── Record.tsx              # 大圆形录音按钮 + 实时波形
│       │   ├── History.tsx
│       │   ├── Models.tsx
│       │   └── Settings.tsx
│       ├── components/                 # 移动专属组件
│       └── i18n/                       # 共享 ../src/i18n/ 翻译源
│
├── scripts/
│   ├── enter-build-env.ps1             # 现状
│   └── enter-android-env.ps1           # 新增: 注入 ANDROID_HOME / NDK_HOME / JAVA_HOME
│
└── docs/
    ├── system-requirements.md          # 现状,新增"移动端配置"附录
    └── mobile/                         # 新增: 移动端文档
        ├── README.md                   # 入口
        ├── architecture.md             # 本文精炼版
        ├── android-build.md            # Windows + Android Studio 构建指引
        ├── ime-design.md               # 输入法设计细节
        └── ios-build.md                # 阶段二
```

### 3.1 边界规则 (CRITICAL)

`handy-core` **禁止依赖**:
- `tauri::*` 及任何 tauri-plugin-*
- `cpal`、`enigo`、`rdev`
- `gtk-*`、`tauri-nspanel`
- 任何平台特定 syscall (`windows`、`cocoa` 等)

`handy-core` **允许依赖**:
- `transcribe-rs`、`vad-rs`、`rubato`、`hound`、`rustfft`
- `rusqlite` (bundled feature)、`rusqlite_migration`
- `serde`、`tokio`、`reqwest`、`anyhow`、`log`、`chrono`、`regex`
- `ferrous-opencc` (中文繁简转换)

桌面与移动端各自:
- 实现 `handy-platform` 中定义的 trait
- 在自己的 Tauri command 层把 trait 注入 `handy-core` 的服务

---

## 4. 模块详细设计

### 4.1 `handy-core::transcription`

```rust
pub struct TranscriptionService<A: AudioCapture, O: TextOutput, S: AppStorage> {
    audio:   A,
    output:  O,
    storage: S,
    engine:  TranscribeEngine,        // whisper.cpp / Parakeet
    vad:     SileroVad,
    history: HistoryRepo,             // sqlite
}

impl<A, O, S> TranscriptionService<A, O, S> {
    pub async fn start_recording(&mut self) -> Result<RecordingHandle>;
    pub async fn stop_recording(&mut self, mode: OutputMode) -> Result<Transcript>;
    pub async fn cancel(&mut self) -> Result<()>;
}
```

桌面端与移动端的差异在于 `A` (cpal vs Oboe)、`O` (enigo vs IME)、`S` (`%APPDATA%` vs `context.filesDir`)。

### 4.2 `handy-platform`

```rust
// crates/handy-platform/src/audio.rs
#[async_trait]
pub trait AudioCapture: Send + Sync {
    async fn start(&mut self, config: AudioConfig) -> Result<()>;
    fn frames_rx(&mut self) -> mpsc::Receiver<AudioFrame>;  // 16kHz mono f32
    async fn stop(&mut self) -> Result<()>;
}

// crates/handy-platform/src/text_output.rs
#[async_trait]
pub trait TextOutput: Send + Sync {
    async fn deliver(&mut self, text: &str, mode: OutputMode) -> Result<()>;
}

pub enum OutputMode { Clipboard, Typed, ImeCommit }

// crates/handy-platform/src/storage.rs
pub trait AppStorage: Send + Sync {
    fn models_dir(&self) -> &Path;
    fn db_path(&self) -> &Path;
    fn settings_path(&self) -> &Path;
}
```

### 4.3 Android 独有: 前台录音服务

`src-mobile/gen/android/app/src/main/java/com/handy/mobile/HandyRecordingService.kt`:
- `Service` 子类,`startForeground()` + 常驻通知 (channel: `recording`)
- 通知含"停止"动作 → 通过 `LocalBroadcastManager` 或 Intent 通知 Rust 端 cancel
- Android 14+ 声明 `<service android:foregroundServiceType="microphone" />`
- 录音管线在 Rust 侧 (Oboe JNI),Kotlin 服务仅负责生命周期 + 通知 + 系统电源管理

需要的权限 (`AndroidManifest.xml`):
```xml
<uses-permission android:name="android.permission.RECORD_AUDIO" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
<uses-permission android:name="android.permission.FOREGROUND_SERVICE_MICROPHONE" />
<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
<uses-permission android:name="android.permission.INTERNET" />          <!-- 模型下载 -->
<uses-permission android:name="android.permission.WAKE_LOCK" />         <!-- 长录音防睡 -->
```

### 4.4 Android 独有: 系统输入法 (IME)

详见 [docs/mobile/ime-design.md](../../mobile/ime-design.md) (阶段 3 时落地)。核心要点:

`HandyInputMethodService.kt`:
- `InputMethodService` 子类
- `onCreateInputView()` 返回一个 Compose `View`: 60dp 高小键盘条,中央大按钮"按住说话",右侧设置图标
- 按钮按下 → bind 主 App 的 `HandyEngineService` (Android `Service`,通过 AIDL)
- 主 App 的 `HandyEngineService` 持有已加载的 transcribe-rs 引擎,IME 不重复加载模型 (避免每次切到 IME 就花 3s+ 加载)
- 松开按钮 → IME 拿到文本 → `currentInputConnection.commitText(text, 1)`

**MVP 简化**: 如果主 App 未运行,IME 显示"请先打开 Handy 主应用"提示。下一版考虑用 ContentProvider 或独立轻量引擎。

### 4.5 数据流

```
[麦克风]
   │ AudioCapture::frames_rx (16kHz mono f32, 30ms 帧)
   ▼
[VAD (Silero v4)] ── 静音段裁掉 ──┐
   │                              │
   ▼                              │
[transcribe-rs (Whisper / Parakeet)]
   │ 流式 + 终态 segments
   ▼
[文本后处理 (繁简/标点/敏感词)]
   │
   ▼
[TextOutput::deliver]
   ├── Clipboard (ClipboardManager.setPrimaryClip)
   └── IME commit (InputConnection.commitText)
   │
   ▼
[HistoryRepo.insert] → sqlite (App 私有目录)
```

---

## 5. 构建与开发流程

### 5.1 Cargo workspace (根 `Cargo.toml`,新增)

```toml
[workspace]
members  = ["src-tauri", "src-mobile", "crates/handy-core", "crates/handy-platform", "crates/handy-mobile"]
resolver = "2"

[workspace.dependencies]
transcribe-rs = { path = "src-tauri/vendor/transcribe-rs", features = ["whisper", "onnx"] }
vad-rs        = { git = "https://github.com/cjpais/vad-rs", default-features = false }
serde         = { version = "1", features = ["derive"] }
serde_json    = "1"
tokio         = "1.43"
anyhow        = "1"
log           = "0.4"
chrono        = "0.4"
rusqlite      = { version = "0.37", features = ["bundled"] }
rusqlite_migration = "2.3"
reqwest       = { version = "0.12", features = ["json", "stream"] }

[patch.crates-io]
# 沿用 src-tauri 现有 patch
tauri-runtime     = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-runtime-wry = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
tauri-utils       = { git = "https://github.com/cjpais/tauri.git", branch = "handy-2.10.2" }
```

### 5.2 前端工程组织

**现状**: 根 `package.json` 直接管理桌面端 React (`src/`),无嵌套 workspace。

**MVP 改动**: 不引入 bun workspaces (避免大改),改为:
- 根 `package.json` 保持现状,继续服务桌面端 (`src/`)
- 新增 `src-mobile-ui/package.json`,独立维护移动端 React 依赖,独立 `bun install`
- 根 `package.json` 新增脚本作为入口便捷调用:

```jsonc
// 根 package.json 新增的 scripts (仅追加,不动现有)
{
  "scripts": {
    "mobile:install":      "cd src-mobile-ui && bun install",
    "tauri:android:init":  "cd src-mobile && cargo tauri android init",
    "tauri:android:dev":   "cd src-mobile && cargo tauri android dev",
    "tauri:android:build": "cd src-mobile && cargo tauri android build --apk",
    "tauri:ios:dev":       "cd src-mobile && cargo tauri ios dev",
    "test:core":           "cargo test -p handy-core",
    "lint:rust":           "cargo clippy --workspace -- -D warnings",
    "lint:mobile":         "cd src-mobile-ui && eslint src"
  }
}
```

复用桌面 i18n 翻译源用 vite 的 `resolve.alias` 把 `@desktop-i18n` 指到 `../src/i18n/locales`,无需 npm link 或 workspace。

如果未来共享的 React 组件超过 ~5 个,再评估是否升级到 bun workspaces 或抽出 `packages/ui` 包。

### 5.3 Android 环境

- Android Studio (Hedgehog 或更新)
- NDK r26+ (`%LOCALAPPDATA%\Android\Sdk\ndk\26.x.xxxxx`)
- JDK 17 (Android Studio 内置)
- 新增 `scripts/enter-android-env.ps1`,职责类似 [scripts/enter-build-env.ps1](../../../scripts/enter-build-env.ps1):
  - 注入 `ANDROID_HOME` / `NDK_HOME` / `JAVA_HOME`
  - 设置短路径 `CARGO_TARGET_DIR_ANDROID` (规避 Windows 长路径问题,参考 [docs/README.md:6-17](../../README.md#L6-L17))
  - 添加 `aarch64-linux-android` / `armv7-linux-androideabi` 到 PATH

### 5.4 CI

新增 `.github/workflows/mobile-ci.yml`:
- `core-tests`: ubuntu + macOS 上跑 `cargo test -p handy-core`
- `android-build-check`: ubuntu 上 `cargo build -p handy-core --target aarch64-linux-android` (不打 APK,只验证交叉编译)
- `android-apk`: 手动触发或 `mobile/**` 分支推送,产 APK artifact

桌面端 CI 保持不变。

---

## 6. 模型策略

继承 [docs/bundled-models.md](../../bundled-models.md) 思路,但移动端特化:

| 模型 | 体积 | 默认 | 适用 |
|------|------|------|------|
| Parakeet V3 (q4) | ~478MB | ✅ 默认 | 大多数 Android 手机 |
| Whisper Small (q5_0) | ~150MB | 可选下载 | 中低端手机,多语言场景 |
| Whisper Medium (q5_0) | ~280MB | 可选下载 | 高端手机 (8GB+ RAM、骁龙 8 Gen 2+) |
| Whisper Turbo / Large | — | ❌ 不上架 | 体积/性能均不适合移动端 |

**首装策略**: APK 不内置模型 (保 APK < 30MB),首次启动引导用户:
1. 检测网络与剩余存储
2. 推荐 Parakeet V3 (大多数情况)
3. 用户确认后下载 (CDN: `blob.handy.computer`,与桌面端同源)

模型存储路径: `context.filesDir/models/<engine>/<name>.onnx|gguf`

---

## 7. 国际化

复用桌面端 i18next 翻译源 ([CLAUDE.md 中 i18n 章节](../../../CLAUDE.md)):
- `src-mobile-ui/` 通过 vite 路径别名复用 `src/i18n/locales/*/translation.json`
- 移动端新增 key 写入同一份 `translation.json`,桌面端忽略即可

ESLint 规则 (禁止硬编码字符串) 同样应用于 `src-mobile-ui/`。

---

## 8. 风险登记

| # | 风险 | 概率 | 影响 | 缓解 |
|---|------|------|------|------|
| 1 | whisper.cpp / `transcribe-rs` 在 NDK 下编译失败 | 中 | 高 | 阶段 0 技术 spike;必要时在 `vendor/transcribe-rs` 加补丁 (有先例) |
| 2 | 抽取 `handy-core` 时破坏桌面端行为 | 高 | 中 | 严格 TDD;每抽一个模块跑桌面 smoke test;阶段 1 内可用 feature flag 切新旧路径 |
| 3 | IME 通过 bound Service 跨进程访问主 App 模型复杂 | 中 | 中 | MVP 简化为"主 App 必须运行";下一版再做独立轻量模型加载 |
| 4 | Whisper Small 在低端 Android 太慢 | 中 | 中 | 默认 Parakeet,Whisper 仅可选;实测后调整 |
| 5 | Windows + Android Studio + NDK 配置门槛 | 高 | 低 | 一开始就写好 `enter-android-env.ps1` 与 [docs/mobile/android-build.md](../../mobile/android-build.md) |
| 6 | Android 各厂商麦克风权限 / 后台限制差异大 | 高 | 中 | 维护"已测设备矩阵";小米/华为/OPPO 提示用户手动加白名单 |
| 7 | tauri 上游 mobile 接口变动 | 中 | 中 | 锁 tauri 版本,跟随 cjpais fork (`patch.crates-io` 已锁) |

---

## 9. 里程碑与验收

### 阶段 0 — 技术 spike (1 周)
- [ ] `cargo tauri android init` 在 src-mobile/ 跑通
- [ ] APK 在真机/模拟器跑出 "Hello Handy" 页面
- [ ] `transcribe-rs` 能交叉编译到 `aarch64-linux-android` (可不在 APK 内调用)

### 阶段 1 — Rust core 抽取 (2 周)
- [ ] `crates/handy-core` 编译,所有现有 `cargo test` 通过
- [ ] 桌面端 `bun run tauri dev` 行为完全不变 (手工 smoke: 录音 / VAD / 转录 / 历史 / 设置)
- [ ] `handy-core` 在 host 与 android target 都能编译
- [ ] `handy-platform` trait 完整;桌面端 cpal/enigo 实现注入

### 阶段 2 — 移动端 MVP UI + 前台录音 (3 周)
- [ ] APK 安装后:录音 → 本地转录 → 复制到剪贴板 全流程通
- [ ] History/Models/Settings 三页可用
- [ ] 锁屏与后台仍能录音 (前台服务 + 通知)
- [ ] 切语言 (zh/en)、切模型 (Parakeet/Whisper-Small)
- [ ] APK 体积 < 100MB (模型按需下载,不内置)

### 阶段 3 — 输入法 (2 周)
- [ ] Handy IME 可在系统设置中启用
- [ ] 在微信/邮件/浏览器地址栏中可成功语音输入
- [ ] IME 与主 App 通过 bound Service 共享模型 (不重复加载)
- [ ] 主 App 未运行时给出友好提示

### 阶段 4 — 打包发布准备 (1 周)
- [ ] 签名 APK,Release 模式
- [ ] [docs/mobile/](../../mobile/) 文档齐备
- [ ] 新开发者按文档能在 2 小时内跑起本地构建

### 全局验收 ("Done" 标准)
1. **桌面端零回归**: 现有 `cargo test`、`bun run lint`、`bun run format:check` 全部通过;手工 smoke 不发现行为变化
2. **Android MVP**: APK 在至少 3 个不同厂商 (小米/三星/Pixel,Android 10–15) 设备上跑通基础录音流程
3. **文档**: 本设计文档已在 main 分支
4. **CI**: `mobile-ci.yml` 中 `core-tests` 与 `android-build-check` 通过

---

## 10. 未决问题 (Open Questions)

> ⚠️ 这些不阻塞 MVP 启动,但在对应阶段开始前需要决议:

1. **IME 与主 App 进程共享模型的 IPC 机制**: bound Service + AIDL 还是 ContentProvider?待阶段 3 spike
2. **Android Vulkan 加速是否开启**: MVP 默认 CPU,二期评估
3. **应用商店上架渠道**: Google Play vs F-Droid vs 自主分发?发布前确定
4. **App 签名密钥管理**: 谁持有?如何在 CI 中安全使用?发布前确定
5. **iOS 阶段排期**: MVP 完成后单独立项

---

## 11. 关联文档

- [docs/system-requirements.md](../../system-requirements.md) — 桌面端系统要求
- [docs/bundled-models.md](../../bundled-models.md) — 模型打包方案
- [docs/offline-translation.md](../../offline-translation.md) — 离线翻译方案 (二期可复用到移动端)
- [docs/handy-stt-ipc.md](../../handy-stt-ipc.md) — STT IPC 协议 (IME 跨进程时可参考)
- [CLAUDE.md](../../../CLAUDE.md) — 项目协作约定

---

## 12. 变更记录

| 日期 | 变更 | 作者 |
|------|------|------|
| 2026-05-26 | 初稿 | Claude Code + 唐海文 |
| 2026-05-26 | 阶段 0+1 完成 (本地): workspace 引入,handy-core/handy-platform 创建并迁入 text/audio/vad/history/settings/model-types;Task 11 转录管理器跳过迁移 (rationale 见 docs/mobile/README.md);GHA workflow 就位,等待 push 触发 | Claude Code + 唐海文 |
