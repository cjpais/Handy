Windows 开发环境（推荐启动方式）  

记录当前版本 mac
语音打字已经正常了，后续会继续优化和添加功能。

如果在 `npm run tauri dev` 阶段遇到 `whisper-rs-sys` 的 CMake 异常（例如 `0xc0000409`），请先在同一个 PowerShell 会话加载构建环境脚本，再启动 Tauri：

```powershell
Set-Location -LiteralPath "f:\@Haiwen\开源框架\AI开源项目\一个免费、开源且可扩展的语音转文字应用，完全离线运行\Handy-main\src-tauri"
. ..\scripts\enter-build-env.ps1
Set-Location ..
npm run tauri dev
```

说明：
- 脚本会优先使用 Kitware CMake，并注入短路径 `CARGO_TARGET_DIR`，降低 Windows 长路径导致的 CMake 崩溃概率。
- Windows 下本项目会以 Vulkan 特性编译 `whisper-rs`，必须安装 LunarG Vulkan SDK（包含 `Include\vulkan\vulkan.h`、`Lib\vulkan-1.lib`、`Bin\glslc.exe`）。

准备环境

Node.js（建议 LTS）
Rust 工具链（rustup + stable）
Tauri 所需系统依赖
Windows: 需要安装 Visual Studio Build Tools（含 “Desktop development with C++”）
macOS: 需要 Xcode Command Line Tools
Linux: 需要系统依赖（webkit2gtk/gtk 等）




 启动 Tauri 开发模式（会同时启动 Rust 后端）
 
 npm run tauri dev

下载 Caddy（Windows 版）：https://caddyserver.com/download
把 caddy.exe 放到服务器某个目录，比如 C:\caddy\

在同目录创建 Caddyfile，内容如下（把路径改成你的 dist）：

:8089

root * C:\Publish\Caddy\Antigravity-Manager-main\dist
file_server

@spa {
  header Accept text/html*
  not path /assets/* /favicon.ico /manifest.json /robots.txt /locales/*
}

rewrite @spa /index.html


# SPA 路由需要的话加这一行：
try_files {path} /index.html


C:\caddy\caddy.exe run

启动 Caddy 服务器，监听 80 端口，并把请求转发到你的 Tauri 应用

访问 http://localhost:80 即可看到你的 Tauri 应用

注意：Caddy 服务器默认会自动重启，如果修改了 Caddyfile 或 dist 目录下的文件，Caddy 服务器会自动重新加载配置。
C:\Publish\Caddy\caddy_windows_amd64.exe run
想做成服务常驻：
C:\Publish\Caddy\caddy_windows_amd64.exe service install
C:\Publish\Caddy\caddy_windows_amd64.exe service start

C:\Publish\Caddy\caddy_windows_amd64.exe run --config C:\Publish\Caddy\1.Caddyfile


iis 需要安装 URL Rewrite 模块，并配置 web.config 来实现类似的 SPA 路由重写：

```xml
可直接用的 web.config

bun run tauri build --bundles nsis

set HOST=0.0.0.0
set PORT=8089

set HOST=0.0.0.0
set PORT=8045

netstat -ano -p tcp | findstr :8089

netstat -ano -p tcp | findstr :8045


New-NetFirewallRule -DisplayName "AntigravityTools-8089" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8089

New-NetFirewallRule -DisplayName "AntigravityToolsAPI-8045" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8045
本文档详细介绍了 **Antigravity Tools** 暴露的 HTTP API 接口。



git remote add upstream https://github.com/haiwen619/Antigravity-Manager.git
合并上游主分支 开源项目的fork 
git remote remove upstream
git remote add upstream https://github.com/haiwen619/Antigravity-Manager.git
git fetch upstream
git merge upstream/main

git merge upstream/main --allow-unrelated-histories
