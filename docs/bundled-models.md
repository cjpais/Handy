# 内置模型打包说明

把语音模型直接打包进 Windows 安装包，用户安装完即可使用，无需联网下载。

## 工作原理

```
bundled-models.json  ──► prepare-bundled-models.mjs ──► src-tauri/resources/models/
       │                       (构建前自动运行)                       │
       │                                                              ▼
       │                                                       Tauri 打包进 NSIS 安装包
       │                                                              │
       └──── include_str! 编译进 binary                                ▼
                          │                                    用户首次启动
                          ▼                                           │
              migrate_bundled_models() ◄──────────────────────────────┘
                          │
                          ▼
            复制到用户模型目录 (AppData\models 或自定义路径)
```

三个环节配合：
1. **构建前** — [scripts/prepare-bundled-models.mjs](../scripts/prepare-bundled-models.mjs) 从 `sourceDir` 增量复制到 `src-tauri/resources/models/`
2. **打包时** — [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json) 的 `"resources": ["resources/**/*"]` 将这些文件打入安装包
3. **运行时** — [src-tauri/src/managers/model.rs](../src-tauri/src/managers/model.rs) 的 `migrate_bundled_models()` 把资源复制到用户模型目录

## 配置文件

[src-tauri/bundled-models.json](../src-tauri/bundled-models.json)：

```json
{
  "sourceDir": "G:/离线语音模型",
  "models": [
    { "filename": "sense-voice-int8", "type": "directory" }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `sourceDir` | 本机模型源目录（构建机上）。绝对路径，正斜杠或双反斜杠 |
| `models[].filename` | 必须与 [model.rs](../src-tauri/src/managers/model.rs) 中 `ModelInfo.filename` 完全一致 |
| `models[].type` | `"directory"`（目录型，如 ONNX 模型文件夹）或 `"file"`（单文件，如 .bin） |

构建脚本会校验类型与磁盘实际情况一致；不一致时报错退出。

## 常用场景

### 场景 1：只打包 SenseVoice（默认推荐）

约 +228 MB。中英日韩粤五语支持，速度最快。

```json
{
  "sourceDir": "G:/离线语音模型",
  "models": [
    { "filename": "sense-voice-int8", "type": "directory" }
  ]
}
```

### 场景 2：不打包任何模型（保持原版下载行为）

```json
{
  "sourceDir": "G:/离线语音模型",
  "models": []
}
```

### 场景 3：全量打包（约 +9 GB）

```json
{
  "sourceDir": "G:/离线语音模型",
  "models": [
    { "filename": "sense-voice-int8",              "type": "directory" },
    { "filename": "ggml-small.bin",                "type": "file" },
    { "filename": "whisper-medium-q4_1.bin",       "type": "file" },
    { "filename": "ggml-large-v3-turbo.bin",       "type": "file" },
    { "filename": "ggml-large-v3-q5_0.bin",        "type": "file" },
    { "filename": "breeze-asr-q5_k.bin",           "type": "file" },
    { "filename": "parakeet-tdt-0.6b-v2-int8",     "type": "directory" },
    { "filename": "parakeet-tdt-0.6b-v3-int8",     "type": "directory" },
    { "filename": "moonshine-base",                "type": "directory" },
    { "filename": "moonshine-tiny-streaming-en",   "type": "directory" },
    { "filename": "moonshine-small-streaming-en",  "type": "directory" },
    { "filename": "moonshine-medium-streaming-en", "type": "directory" },
    { "filename": "giga-am-v3.int8.onnx",          "type": "file" },
    { "filename": "canary-180m-flash",             "type": "directory" },
    { "filename": "canary-1b-v2",                  "type": "directory" }
  ]
}
```

## 完整模型清单

| filename | type | 大小 | 说明 |
|----------|------|------|------|
| `sense-voice-int8` | directory | 160 MB | 中英日韩粤，**推荐** |
| `ggml-small.bin` | file | 487 MB | Whisper Small |
| `whisper-medium-q4_1.bin` | file | 492 MB | Whisper Medium |
| `ggml-large-v3-turbo.bin` | file | 1.6 GB | Whisper Turbo |
| `ggml-large-v3-q5_0.bin` | file | 1.1 GB | Whisper Large |
| `breeze-asr-q5_k.bin` | file | 1.1 GB | 台湾国语优化 |
| `parakeet-tdt-0.6b-v2-int8` | directory | 473 MB | Parakeet V2（英语） |
| `parakeet-tdt-0.6b-v3-int8` | directory | 478 MB | Parakeet V3（25 欧语） |
| `moonshine-base` | directory | 58 MB | Moonshine Base |
| `moonshine-tiny-streaming-en` | directory | 31 MB | Moonshine Tiny |
| `moonshine-small-streaming-en` | directory | 100 MB | Moonshine Small |
| `moonshine-medium-streaming-en` | directory | 192 MB | Moonshine Medium |
| `giga-am-v3.int8.onnx` | file | 225 MB | GigaAM v3（俄语） |
| `canary-180m-flash` | directory | 146 MB | Canary 180M Flash |
| `canary-1b-v2` | directory | 692 MB | Canary 1B v2 |

## 操作流程

### 1. 准备源目录

确保 `bundled-models.json` 里 `sourceDir` 指向的目录存在，且包含所有要打包的模型，命名与 `filename` 一致。

可以直接用 Handy 内的下载器把要打包的模型先下到这个目录，或从已有用户目录复制过来。

### 2. 修改配置

编辑 [src-tauri/bundled-models.json](../src-tauri/bundled-models.json)，按需增删 `models` 数组的条目。

### 3. 构建

```bash
bun run tauri build
```

构建脚本会自动：
- 调用 `prepare-bundled-models.mjs` 检查并增量复制模型（已存在且大小一致则跳过）
- 让 Tauri 把模型作为 resource 打进安装包
- 把 `bundled-models.json` 通过 `include_str!` 嵌入到 Rust binary
- **自动选择 bundler**：NSIS 或 MSI（见下方）

输出示例：
```
[prepare-bundled-models] SKIP sense-voice-int8 (already up-to-date)
[prepare-bundled-models] Done. 1 bundled model(s) ready in F:\...\src-tauri\resources\models
```

构建产物位于：
- NSIS：`src-tauri/target/release/bundle/nsis/*.exe`
- MSI：`src-tauri/target/release/bundle/msi/*.msi`

### Windows Bundler 自动选择（NSIS vs MSI）

Tauri 在 Windows 上支持两种安装包格式：

| 格式 | 文件 | 大小限制 | 何时使用 |
|------|------|---------|---------|
| **NSIS** | `.exe` | 总安装包 **~2 GB** | 默认；轻量；支持自定义脚本（已有 `nsis/installer.nsi`） |
| **MSI (WiX v3)** | `.msi` | 单 CAB **2 GB**，总包 4 GB | 资源在 2-4 GB 之间；企业部署友好（GPO 静默安装） |

> **重要硬限制：** MSI 默认把所有资源压成**单个 CAB 文件**，WiX v3 的 `light.exe` 在 CAB > 2 GB 时会抛 `LGHT0001 / E_UNEXPECTED`。即便资源 > 4 GB 想拆多个 CAB，MSI 自身也有 4 GB 上限（Windows Installer 5.0+ 才能扩到 ~8 GB）。
>
> **超过 4 GB 资源的建议：** 不要尝试 installer 全量打包，改为只打核心模型（如 `sense-voice-int8` + `ggml-small.bin` 共 ~650 MB），其他走应用内下载。

NSIS 一旦总包超过约 2GB 会报错：
```
Internal compiler error #12345: error mmapping file ... is out of range.
```

**自动检测：** [scripts/tauri-wrapper.mjs](../scripts/tauri-wrapper.mjs) 会在构建前扫描 `src-tauri/resources/` 总大小，若超过 **1.6 GB** 安全阈值，自动切换到 MSI bundler。判断输出：
```
[tauri-wrapper] resources/ exceeds NSIS ~2GB limit, defaulting to MSI (WiX) bundler.
```

**手动指定：** 也可以用 `--bundles` 显式选择，跳过自动检测：

```bash
# 强制只打 NSIS
bun run tauri build --bundles nsis

# 强制只打 MSI（资源很大或要 MSI 分发时）
bun run tauri build --bundles msi

# 同时打两种格式（资源在 NSIS 阈值内才可行）
bun run tauri build --bundles nsis,msi

# 所有支持的目标
bun run tauri build --bundles all
```

**注意：**
- 当前 NSIS 用自定义模板 [src-tauri/nsis/installer.nsi](../src-tauri/nsis/installer.nsi)，定制行为（如安装目录、快捷方式）只在 NSIS 中生效
- MSI 走 WiX 默认布局，配置在 [tauri.conf.json](../src-tauri/tauri.conf.json) 的 `bundle.windows.wix`
- 首次打 MSI 时 Tauri 会自动下载 WiX 工具链（约 30MB），需要联网

#### MSI 临时空间需求

WiX 的 `light.exe` 在打包时会把所有资源**先压缩到 CAB 临时文件**，临时空间需求通常是 **源资源体积的 2-3 倍**。例如 8 GB 资源可能需要 16-24 GB 可用临时空间。

如果 `%TEMP%`（Windows 默认在 C 盘）空间不足，会看到：
```
light.exe : error LGHT0297 : An error (ERROR_DISK_FULL) was returned while creating a CAB file.
```

[scripts/tauri-wrapper.mjs](../scripts/tauri-wrapper.mjs) 在选择 MSI bundler 时会**自动把 `TEMP`/`TMP` 重定向到** `src-tauri/target/wix-temp/`，避免 C 盘耗尽。判断输出：
```
[tauri-wrapper] redirected TEMP/TMP -> F:\...\src-tauri\target\wix-temp (for WiX light.exe CAB temp)
```

构建机的项目所在盘需要预留 **资源体积 × 3 倍** 的可用空间（CAB 临时 + 最终 MSI + WiX 中间产物）。构建完成后可手动清理：
```bash
rm -rf src-tauri/target/wix-temp
```

### 4. 用户安装

用户运行安装包后，首次启动应用时：
- `ModelManager::new()` 调用 `migrate_bundled_models()`
- 按配置把每个内置模型从安装目录复制到用户模型目录（默认 `%APPDATA%\com.pais.handy\models`，或用户在设置中指定的路径）
- 已存在的模型不会覆盖
- `auto_select_model_if_needed()` 自动选中（SenseVoice 的 `is_recommended: true` 让它优先成为默认）

## 日志验证

应用启动后查看日志，期望看到：
```
INFO  Migrating bundled model directory sense-voice-int8
INFO  Successfully migrated sense-voice-int8
INFO  Auto-selecting model: sense-voice-int8 (SenseVoice)
```

如果跳过（用户已有该模型）：日志里没有上述行，是正常行为。

## Git 与文件管理

[.gitignore](../.gitignore) 已配置：

```gitignore
/src-tauri/resources/models/*
!/src-tauri/resources/models/.gitkeep
!/src-tauri/resources/models/silero_vad_v4.onnx
!/src-tauri/resources/models/gigaam_vocab.txt
```

- 打包脚本拷贝进来的模型文件**不会**进版本库（避免提交几个 GB）
- VAD 与 GigaAM 词表是必要小文件，照常跟踪
- `bundled-models.json` 本身**会**进版本库 —— 这是构建配置

每位开发者 / CI 机器需要在本机有 `sourceDir`，或把 `sourceDir` 改成共享的网络路径 / 构建产物目录。

## 故障排查

| 现象 | 原因 | 处理 |
|------|------|------|
| `ERROR: sourceDir does not exist` | 配置里的源路径在构建机上找不到 | 修改 `sourceDir` 或准备源文件 |
| `ERROR: source not found: ...` | `sourceDir` 下缺少声明的 `filename` | 把对应模型下载到源目录，或从清单中移除 |
| `ERROR: type mismatch` | 配置说 `directory` 但磁盘上是文件（或反之） | 改 `type`，或调整源目录结构 |
| 安装包仍然提示要下载 | 用户已有同名模型且文件存在 | 这是预期行为：已有版本优先；删除后重启可触发迁移 |
| 安装包过大 | `models` 列表过多 | 减少条目，或考虑提供"精简版/全量版"两个安装包 |

## 提供多个安装包变体

如果想同时发"精简版"和"全量版"，可在 CI 里维护两份配置文件 `bundled-models.lite.json` / `bundled-models.full.json`，构建前用脚本复制为 `bundled-models.json` 再打包，最后产物分别上传。
