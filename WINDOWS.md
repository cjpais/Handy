# Windows Setup

This document summarizes the steps we used to get this repo running on Windows with Rust, Bun, Tauri, and the native dependencies needed by whisper-rs.

## Prereqs

- Windows 10/11
- Git
- VS Code

## 1) Install Rust (MSVC)

- Install via rustup: https://rustup.rs/
- Open a new PowerShell and verify:

```powershell
rustc --version
cargo --version
```

If `rustc` is not found, add Cargo to PATH (user):

```powershell
[Environment]::SetEnvironmentVariable("Path", "$env:USERPROFILE\.cargo\bin;" + [Environment]::GetEnvironmentVariable("Path","User"), "User")
```

Close and reopen PowerShell, then re-check `rustc --version`.

## 2) Install Bun

```powershell
powershell -c "irm bun.sh/install.ps1|iex"
```

Close and reopen PowerShell, then verify:

```powershell
bun --version
```

## 3) Install Visual Studio Build Tools (C++)

Install Visual Studio Build Tools and select **Desktop development with C++**.
Required components:

- MSVC v143 (or v142) C++ build tools
- Windows 10/11 SDK
- C++ CMake tools (optional, but fine to include)

## 4) Install CMake

```powershell
winget install --id Kitware.CMake -e --source winget --accept-source-agreements --accept-package-agreements
```

Ensure CMake is on PATH (user) if needed:

```powershell
$cmakeBin = "C:\Program Files\CMake\bin"
[Environment]::SetEnvironmentVariable("Path", "$cmakeBin;" + [Environment]::GetEnvironmentVariable("Path","User"), "User")
```

## 5) Install LLVM 18.1.8 (libclang)

Bindgen broke with LLVM 22, so use LLVM 18.1.8.

```powershell
winget install --id LLVM.LLVM -e --version 18.1.8 --source winget --accept-source-agreements --accept-package-agreements
```

Verify:

```powershell
& "C:\Program Files\LLVM\bin\clang.exe" --version
```

## 6) Install Vulkan SDK

```powershell
winget install --id KhronosGroup.VulkanSDK -e --source winget --accept-source-agreements --accept-package-agreements
```

This sets `VULKAN_SDK` to a folder like `C:\VulkanSDK\1.4.335.0`.

## 7) Repo setup

From the repo root:

```powershell
bun install
```

## 8) Required model

```powershell
mkdir src-tauri\resources\models
curl -o src-tauri\resources\models\silero_vad_v4.onnx https://blob.handy.computer/silero_vad_v4.onnx
```

## 9) Run dev build (foreground)

Use these env vars to ensure bindgen targets Windows correctly:

```powershell
$cmakeBin = "C:\Program Files\CMake\bin"
$env:PATH = "$cmakeBin;$env:PATH"
$env:CMAKE = "$cmakeBin\cmake.exe"
$env:VULKAN_SDK = "C:\VulkanSDK\1.4.335.0"
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:BINDGEN_EXTRA_CLANG_ARGS = "--target=x86_64-pc-windows-msvc"
Set-Item -Path Env:BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc -Value "--target=x86_64-pc-windows-msvc"
Set-Item -Path Env:BINDGEN_EXTRA_CLANG_ARGS_x86_64-pc-windows-msvc -Value "--target=x86_64-pc-windows-msvc"

# If you previously forced bundled bindings, clear it:
Remove-Item Env:WHISPER_DONT_GENERATE_BINDINGS -ErrorAction SilentlyContinue

bun run tauri dev
```

## 10) Common issues

### Port 1420 already in use

```powershell
$p = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty OwningProcess
if ($p) { Stop-Process -Id $p -Force }
```

### Rust build says `link.exe` not found

Install Visual Studio Build Tools with **Desktop development with C++**.

### Bindgen errors like "unknown field `_address`"

Use LLVM 18.1.8 and the bindgen target env vars above. LLVM 22 caused opaque structs.

### Vulkan SDK missing

Install Vulkan SDK and ensure `VULKAN_SDK` is set.
