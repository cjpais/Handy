# handy-stt（WPF IPC 集成）

这个仓库新增了一个独立的 Rust 进程：`handy-stt`。

- 通讯方式：stdin/stdout JSON Lines（JSONL）
- 一行一个 JSON
- 支持：列模型、下载模型（带进度事件）、加载模型、会话式推送音频 + 完成转写

## 1. 构建/运行

在 `src-tauri` 下构建：

```powershell
cargo build --bin handy-stt
```

运行（指定模型目录）：

```powershell
.\target\debug\handy-stt.exe --models-dir "C:\YourApp\models"
```

也可以用环境变量：

- `HANDY_MODELS_DIR=C:\YourApp\models`

### Windows 编译前置（常见坑）

本仓库依赖 `whisper-rs-sys` 等 crate，会触发 `bindgen`，需要 `libclang.dll`：

- 安装 LLVM（包含 clang），并设置 `LIBCLANG_PATH` 指向包含 `libclang.dll` 的目录
- 安装 VS C++ Build Tools（MSVC）

另外，在 Windows 上 `transcribe-rs` 会让 `whisper-rs` 以 `vulkan` 特性编译（用于 GPU 加速），因此构建阶段还需要 Vulkan SDK：

- 安装 Vulkan SDK（LunarG）并确保 `VULKAN_SDK` 环境变量已设置

可选：某些依赖会尝试调用 `rustfmt`，没有也不致命，但建议安装：

- `rustup component add rustfmt`

如果你看到类似 “Unable to find libclang… set LIBCLANG_PATH” 的错误，就是这里没配置。

如果你看到类似 “Please install Vulkan SDK and ensure that VULKAN_SDK env variable is set”，就是 Vulkan SDK 未安装或未配置。

#### 快速验证（PowerShell）

```powershell
clang --version
cl
echo $env:LIBCLANG_PATH
echo $env:VULKAN_SDK
Test-Path (Join-Path $env:VULKAN_SDK 'Include\vulkan\vulkan.h')
Test-Path (Join-Path $env:VULKAN_SDK 'Lib\vulkan-1.lib')
```

说明：`vulkaninfo.exe` 不存在通常不影响构建；`whisper-rs-sys` 构建主要依赖 Vulkan SDK 的头文件与 `vulkan-1.lib`。

#### 一键加载构建环境（推荐）

如果你在普通 PowerShell/VSCode 终端里运行 `cargo`，经常会出现 `cl.exe` 返回错误但看不到标准头文件（因为 `INCLUDE/LIB` 没有被 vcvars 初始化）。

仓库提供了一个辅助脚本，会在当前 PowerShell 会话里：

- 自动加载 VS C++ 环境（VsDevCmd/vcvars）
- 确保 `cmake` 在 PATH 上
- 设置 `LIBCLANG_PATH/CLANG_PATH`（如果 LLVM 安装在默认位置）
- 注入 `BINDGEN_EXTRA_CLANG_ARGS`，避免 `stdbool.h file not found`

在 `src-tauri` 目录执行：

```powershell
Set-Location -LiteralPath "f:\@Haiwen\开源框架\AI开源项目\一个免费、开源且可扩展的语音转文字应用，完全离线运行\Handy-main\src-tauri"
powershell -ExecutionPolicy Bypass -File "..\scripts\enter-build-env.ps1"
cargo check --bin handy-stt
```

## 2. JSONL 协议

### 2.1 请求

请求格式：

```json
{ "id": 1, "cmd": "list_models", "args": {} }
```

- `id`：请求序号（WPF 自己生成，用于匹配 response）
- `cmd`：命令
- `args`：参数对象

### 2.2 响应

```json
{ "type": "response", "id": 1, "ok": true, "result": { ... } }
```

失败：

```json
{ "type": "response", "id": 1, "ok": false, "error": "..." }
```

### 2.3 事件（异步）

下载进度/完成会通过 event 推送：

```json
{ "type": "event", "event": "download_progress", "payload": { ... } }
```

## 3. 命令列表

### ping

```json
{ "id": 1, "cmd": "ping", "args": {} }
```

### list_models

```json
{ "id": 2, "cmd": "list_models", "args": {} }
```

### download_model

```json
{ "id": 3, "cmd": "download_model", "args": { "model_id": "parakeet-tdt-0.6b-v3" } }
```

事件：

- `download_progress`：`payload = { model_id, downloaded, total?, percentage? }`
- `download_completed`：`payload = { model_id }`
- `download_failed`：`payload = { model_id, error }`

### load_model

```json
{ "id": 4, "cmd": "load_model", "args": { "model_id": "small" } }
```

### set_language / set_translate

```json
{ "id": 5, "cmd": "set_language", "args": { "language": "zh" } }
{ "id": 6, "cmd": "set_translate", "args": { "translate": false } }
```

- `language`：`"auto"` 或 ISO 语言码；`zh-Hans/zh-Hant` 会归一化为 `zh`

### start_session / push_audio / finish_transcribe

适合你现在 WPF 的“按键说话、持续推流、松开结束”的工作流。

```json
{ "id": 10, "cmd": "start_session", "args": { "sample_rate": 48000 } }
```

把 PCM 16-bit little-endian 的字节流 base64 后推送：

```json
{ "id": 11, "cmd": "push_audio", "args": { "session_id": 1, "encoding": "pcm_s16le", "data": "...base64..." } }
```

结束并获取文本：

```json
{ "id": 12, "cmd": "finish_transcribe", "args": { "session_id": 1 } }
```

注：服务端会自动把非 16kHz 的音频重采样到 16kHz。

### transcribe_wav

```json
{ "id": 20, "cmd": "transcribe_wav", "args": { "path": "C:\\temp\\a.wav" } }
```

## 4. WPF（C#）调用示例（最小化）

下面示例展示：启动进程、发送 JSONL、读取 response/event。

```csharp
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

public sealed class HandySttClient : IDisposable
{
    private readonly Process _p;
    private readonly StreamWriter _stdin;
    private long _nextId = 1;

    public event Action<JsonElement>? OnEvent;

    public HandySttClient(string exePath, string modelsDir)
    {
        _p = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = exePath,
                Arguments = $"--models-dir \"{modelsDir}\"",
                UseShellExecute = false,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                CreateNoWindow = true,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8,
            }
        };
        _p.Start();

        _stdin = _p.StandardInput;
        _stdin.NewLine = "\n";
        _stdin.AutoFlush = true;

        _ = Task.Run(ReadLoop);
    }

    private async Task ReadLoop()
    {
        while (!_p.HasExited)
        {
            var line = await _p.StandardOutput.ReadLineAsync();
            if (string.IsNullOrWhiteSpace(line)) continue;

            using var doc = JsonDocument.Parse(line);
            var root = doc.RootElement;
            if (root.TryGetProperty("type", out var t) && t.GetString() == "event")
            {
                OnEvent?.Invoke(root);
            }
            // response 的匹配这里省略：你可以用 ConcurrentDictionary<long, TaskCompletionSource<...>>
        }
    }

    public async Task SendAsync(string cmd, object args, CancellationToken ct = default)
    {
        var id = Interlocked.Increment(ref _nextId);
        var payload = new { id, cmd, args };
        var json = JsonSerializer.Serialize(payload);
        await _stdin.WriteLineAsync(json);
    }

    public void Dispose()
    {
        try { _stdin.Close(); } catch { }
        try { if (!_p.HasExited) _p.Kill(true); } catch { }
        _p.Dispose();
    }
}
```

你现有的音频采集是“实时拿到 PCM 帧”，把每帧 bytes base64 后调用 `push_audio` 即可；松开按键时调用 `finish_transcribe`。



$env:VULKAN_SDK = "C:\VulkanSDK\1.4.335.0"
[Environment]::SetEnvironmentVariable("VULKAN_SDK", "C:\VulkanSDK\1.4.335.0", "User")



