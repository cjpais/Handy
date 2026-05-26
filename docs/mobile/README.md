# Handy 移动端

> 状态: 阶段 1 已在本地完成 (Rust core 抽取),GHA workflow 已就位但等待 push 触发。阶段 2 待启动 (Android Tauri Mobile 工程 + UI + 前台录音)。

## 文档索引

- [设计方案](../superpowers/specs/2026-05-26-handy-mobile-design.md) — 总体方案
- [阶段 0+1 实施计划](../superpowers/plans/2026-05-26-handy-mobile-phase-0-1.md) — 见下方完成情况
- [Spike 发现](./spike-findings.md) — Android 交叉编译初次尝试结论 (等 GHA 跑完后填实)
- 阶段 2 计划 — 待写

## 阶段 0+1 完成情况

### 已完成

| Task | 内容 | Commit | 备注 |
|------|------|--------|------|
| 0 | GHA spike workflow + spike-findings 占位 | `a34575a`, `57cabc4`, `44b48f5` | 在 `mobile/phase-0-spike` 分支 |
| 1 | Cargo workspace,根 manifest,hoist patch & profile | `63912ff`, `5217d67` | 含 Cargo.lock 搬迁到 workspace 根 |
| 2 | 空 `handy-core` crate + sanity_check 测试 | `65fdff7` | |
| 3 | `handy-platform` crate: `AudioCapture` / `TextOutput` / `AppStorage` / `EventSink` traits | `b6e1e26` | |
| 4 | 迁移 `audio_toolkit::text` → `handy_core::text` | `30fd582` | 30 个 text 测试全部通过 |
| 5 | 迁移 `audio_toolkit::constants` → `handy_core::audio::constants` | `e3b6d34` | utils 是纯 cpal 代码,留在 src-tauri |
| 6 | 迁移 `audio_toolkit::audio::{resampler, visualizer}` | `3309636` | |
| 7 | 迁移 `audio_toolkit::vad` → `handy_core::vad` | `0de6079` | |
| 8 | 迁移 `managers::history` → `handy_core::history` | `3a7a382` | 含 `RecordingRetentionPeriod` + `save_wav_file`;通过 `EventSink` 解耦 tauri |
| 9 | `handy_core::settings::CoreSettings` 空骨架 | `a140f78` | 字段按需添加,未做大规模 settings 拆分 |
| 10 | 抽取共享 model 类型 (`EngineType`/`ModelInfo`/`DownloadProgress`) | `cc25d15` | `ModelManager` 留在 src-tauri (见下方"范围调整") |
| 12 | 清理 `audio_toolkit/mod.rs` re-export 路径 | `635780d` | text/vad 直接从 `handy_core::` re-export |
| 13 | GHA mobile-ci 完整 workflow (core-tests / android-cross-compile / desktop-no-regression) | `3d3b913` | 等网络恢复后 push 触发 |
| 14 | docs/mobile/README.md (本文件) 与设计文档变更记录 | `f09756d` | |
| post-review | 加 core→desktop RetentionPeriod From,GHA 加 armv7 check | (next commit) | 来自 final review 反馈 |

### 范围调整 (scope changes)

**Task 11 — `TranscriptionManager` 全量迁移: 跳过**

原计划要把 `src-tauri/src/managers/transcription.rs` 整体抽到 handy-core。实际评估后认为收益不足以匹配风险:

- `TranscriptionManager` 持有 `LoadedEngine` 多变体 (`Whisper`/`Parakeet`/`Moonshine`/`SenseVoice`/`GigaAM`/`Canary`),并维护自己的后台 watcher 线程 + 一组 `Arc<Mutex/Condvar>` 状态机
- 它从 `crate::settings::get_settings(app)` 直接读取全部桌面设置 (`OrtAcceleratorSetting`, `WhisperAcceleratorSetting`,以及加速器偏好等)
- 它发射多个 `model/...` 与 `transcription/...` 事件,这些事件的 schema 已经在桌面前端被 `bindings.ts` 锁定

如果搬到 handy-core,要么得连带把 settings 全部抽出来 (Task 9 已明确按 YAGNI 推迟),要么得在 core 端接受一份巨大的"设置快照"结构,这两条都不能干净落地。

替代方案: 移动端将来自己组装 transcription 服务,使用以下已迁移到 handy-core 的可复用部分:
- `handy_core::text::*` (文本后处理)
- `handy_core::vad::*` (语音活动检测)
- `handy_core::audio::{resampler, visualizer, utils, constants}` (信号处理)
- `handy_core::model::{EngineType, ModelInfo, DownloadProgress}` (模型元数据)
- `handy_core::history::HistoryManager` (录音历史)
- `transcribe-rs` (直接依赖,与桌面共享)

实际上桌面 `TranscriptionManager` 里**真正复杂的纯逻辑** (文本归一化/VAD) 都已经迁过去了;剩下的"manager pattern" 是 host runtime 适配,各平台单独写更合适。

**Task 10 — `ModelManager` 全量迁移: 部分**

只搬数据类型 (`EngineType` 等),`ModelManager` 留在 src-tauri。原因:
- 1677 行,33 处 tauri 引用
- bundled 模型路径解析依赖 `tauri::path::BaseDirectory::Resource` (桌面专属)
- 模型偏好回写 (`write_settings`) 是桌面端职责

移动端将来用同一份 `EngineType` / `ModelInfo` 词汇,自己写下载/管理逻辑 (Android Foreground Service 友好的 API)。

**Task 9 — Settings 拆分: 最小骨架**

`CoreSettings` 当前为空 struct。原计划要把 ~50 个字段做共享/桌面专属分类;但桌面 `Settings` 大多数字段是桌面专属 (overlay/tray/全局快捷键/Vulkan/Metal 偏好/AI 通道代理),硬拆会触动 specta 绑定与持久化 schema。按 YAGNI,改成"消费者驱动",未来阶段 2 真正需要某个字段在 core 端用时再加。

### 关键边界规则验证

`handy-core` 在本阶段结束时:
- ✅ 不依赖 `tauri::*` / `cpal` / `enigo` / `rdev` / `gtk-*` / `specta`
- ✅ 全部 34 个单元测试通过
- ⏳ Android 交叉编译验证: 等待 GHA 跑出结果 (workflow 已就位)
- ✅ 桌面端 `cargo check -p handy-core` 通过 (跑过多次)
- ⏳ 桌面端 `bun run tauri dev` 端到端 smoke test: 需手动跑 (本机 MSVC GBK 代码页问题阻塞自动跑;需先 source `scripts/enter-build-env.ps1`)

### 已知 follow-up

1. **`HistoryManager` 的 `RetentionConfig` 快照**
   `open_for_app(&app)` 在构造时一次性读取 retention 配置。如果用户在运行时改了 `recording_retention_period` 设置,旧 manager 会用过期快照做清理。修复方案: 把 `RetentionConfig` 放进 `Arc<RwLock<_>>` 或在设置变更时重新调用 `open_for_app`。
   触发条件:`commands/history.rs:97` 在设置变更后立即调用 `cleanup_old_entries`。
   优先级:中。

2. **Android 交叉编译实测**
   GHA push 后,如果 `android-cross-compile` 失败,常见原因:
   - `reqwest` 在 Android 上默认 `native-tls` 不可用 → 换 `rustls-tls`
   - `rusqlite` 的 `bundled` feature 一般 OK (它包含 sqlite C 源码)
   - `transcribe-rs` 本身可能不能在 Android 交叉编译 (whisper-rs-sys cmake) — 但 handy-core 本身**没有**直接依赖 transcribe-rs (Task 8 加 hound 但不加 transcribe-rs),所以应该不会触发
   修复指南: 见设计文档 §8 风险登记表第 1 项与 spike-findings.md

3. **Push 与 PR**
   阶段 0 与阶段 1 的所有 commit 都在本地,尚未 push:
   - 分支 `mobile/phase-0-spike`: 3 个 commit (`a34575a`, `57cabc4`, `44b48f5`)
   - 分支 `mobile/phase-1-workspace`: 13 个 commit (`63912ff` 起到 `3d3b913`)
   网络恢复后批量 push 并 `gh pr create` 开 PR 合并到 main。

## 下一步 (阶段 2)

待 PR 合并 + GHA 全绿后启动:

1. `cargo tauri android init` 在 `src-mobile/` 创建移动端 Tauri 工程
2. 编写 [docs/mobile/android-build.md](./android-build.md) (Windows + Android Studio 构建指引)
3. 实现移动端 UI (`src-mobile-ui/`):Record / History / Models / Settings 四个页
4. 实现 Android 前台录音服务 (`HandyRecordingService.kt` + Rust JNI 桥)
5. 实现 Android `AudioCapture` (基于 Oboe 或 AAudio)

阶段 2 详细计划将单独成文。
