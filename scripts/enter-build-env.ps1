param(
  [ValidateSet('x64')]
  [string]$Arch = 'x64'
)

$ErrorActionPreference = 'Stop'

function Import-CmdEnv {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Cmd
  )

  # Use cmd.exe to run a batch script and print environment via `set`.
  # IMPORTANT: The command string here must be a single argument to cmd.exe.
  $full = "$Cmd & set"
  $comspec = $env:ComSpec
  if ([string]::IsNullOrWhiteSpace($comspec)) {
    $comspec = 'C:\Windows\System32\cmd.exe'
  }
  $lines = & $comspec /c $full 2>$null
  foreach ($line in $lines) {
    if ($line -match '^(.*?)=(.*)$') {
      [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
  }
}

function Add-ToPathIfExists {
  param([string]$Dir)
  if ([string]::IsNullOrWhiteSpace($Dir)) { return }
  if (Test-Path $Dir) {
    $parts = @($env:PATH -split ';')
    $parts = $parts | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    $parts = $parts | Where-Object { $_ -ne $Dir }
    $env:PATH = (@($Dir) + $parts) -join ';'
  }
}

function Get-ShortPath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path
  )
  if (-not (Test-Path $Path)) { return $null }

  # Many Rust build scripts split BINDGEN_EXTRA_CLANG_ARGS by whitespace.
  # Using a short-path avoids breaking "C:\Program Files\..." into multiple args.
  $cmd = 'for %I in ("' + $Path + '") do @echo %~sI'
  $comspec = $env:ComSpec
  if ([string]::IsNullOrWhiteSpace($comspec)) {
    $comspec = 'C:\Windows\System32\cmd.exe'
  }
  $out = & $comspec /c $cmd 2>$null
  if (-not $out) { return $null }
  return $out.Trim()
}

$isDotSourced = $MyInvocation.InvocationName -eq '.'
if (-not $isDotSourced) {
  Write-Warning "This script sets environment variables for the current PowerShell session. Dot-source it to persist changes: . ..\\scripts\\enter-build-env.ps1"
}

# 1) Ensure MSVC env is loaded (INCLUDE/LIB etc)
$vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path $vswhere) {
  $inst = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if ($inst) {
    $vsDevCmd = Join-Path $inst 'Common7\Tools\VsDevCmd.bat'
    if (Test-Path $vsDevCmd) {
      # `call` ensures cmd runs the batch file correctly.
      $cmd = 'call "' + $vsDevCmd + '" -no_logo -arch=' + $Arch + ' -host_arch=' + $Arch
      Import-CmdEnv $cmd
    }
  }
}

# VsDevCmd replaces PATH; make sure core Windows directories remain available.
$winDir = $env:WINDIR
if ([string]::IsNullOrWhiteSpace($winDir)) {
  $winDir = 'C:\Windows'
}
Add-ToPathIfExists $winDir
Add-ToPathIfExists (Join-Path $winDir 'System32')

# 1.1) Use a short Cargo target directory on Windows to avoid deep-path cmake crashes.
#
# IMPORTANT: do NOT put this under $env:TEMP. MSBuild's MSB8029 warning
# ("中间目录或输出目录无法驻留在临时目录下") is actually fatal in some
# whisper-rs-sys build invocations — MSBuild treats temp-resident inputs as
# clean-able and wipes out generated whisper.cpp/CMakeLists.txt mid-build,
# producing "The source directory ... does not appear to contain CMakeLists.txt".
# Use a stable short path on a real drive instead.
if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  $candidates = @('C:\handy-build', 'D:\handy-build', 'F:\handy-build')
  foreach ($c in $candidates) {
    $root = Split-Path $c -Qualifier
    if (Test-Path $root) {
      if (-not (Test-Path $c)) {
        try { New-Item -ItemType Directory -Path $c -Force | Out-Null } catch { continue }
      }
      $env:CARGO_TARGET_DIR = $c
      break
    }
  }
  if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    # Fallback: keep prior behaviour if no fixed drive root is writable.
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'handy-cargo-target'
    Write-Warning "Could not allocate a non-temp CARGO_TARGET_DIR; falling back to $env:CARGO_TARGET_DIR. whisper-rs-sys MSBuild may fail."
  }
}

# 2) Ensure CMake is callable (whisper-rs-sys build script shells out to `cmake`)
$vs2022CmakeExe = 'C:\Program Files\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$vs2019CmakeExe = 'C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$kitwareCmakeExe = 'C:\Program Files\CMake\bin\cmake.exe'

# Prefer Kitware CMake first; older VS-bundled CMake versions may crash in whisper-rs-sys builds.
if (Test-Path $kitwareCmakeExe) {
  Add-ToPathIfExists (Split-Path $kitwareCmakeExe -Parent)
} elseif (Test-Path $vs2022CmakeExe) {
  Add-ToPathIfExists (Split-Path $vs2022CmakeExe -Parent)
} elseif (Test-Path $vs2019CmakeExe) {
  Add-ToPathIfExists (Split-Path $vs2019CmakeExe -Parent)
}

$cmakeCmd = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if ($cmakeCmd) {
  # Let cmake-rs use this exact executable instead of a stale PATH entry.
  $env:CMAKE = $cmakeCmd
}

# 3) Help bindgen find standard headers reliably
$llvmBin = 'C:\Program Files\LLVM\bin'
if (Test-Path (Join-Path $llvmBin 'libclang.dll')) {
  $env:LIBCLANG_PATH = $llvmBin
}
if (Test-Path (Join-Path $llvmBin 'clang.exe')) {
  $env:CLANG_PATH = (Join-Path $llvmBin 'clang.exe')
  Add-ToPathIfExists $llvmBin
}

$clangLibRoot = 'C:\Program Files\LLVM\lib\clang'
$clangIncludeDir = $null
if (Test-Path $clangLibRoot) {
  $ver = Get-ChildItem $clangLibRoot -Directory | Sort-Object { [int]$_.Name } -Descending | Select-Object -First 1
  if ($ver) {
    $candidate = Join-Path $ver.FullName 'include'
    if (Test-Path $candidate) {
      $clangIncludeDir = $candidate
    }
  }
}

if ($clangIncludeDir) {
  $shortInclude = Get-ShortPath $clangIncludeDir
  if (-not $shortInclude) {
    $shortInclude = $clangIncludeDir
  }

  $extra = @(
    "--target=x86_64-pc-windows-msvc",
    "-I$shortInclude"
  ) -join ' '

  # Preserve user-provided args if any
  if ([string]::IsNullOrWhiteSpace($env:BINDGEN_EXTRA_CLANG_ARGS)) {
    $env:BINDGEN_EXTRA_CLANG_ARGS = $extra
  } elseif ($env:BINDGEN_EXTRA_CLANG_ARGS -notmatch [Regex]::Escape("-I$shortInclude")) {
    $env:BINDGEN_EXTRA_CLANG_ARGS = "$env:BINDGEN_EXTRA_CLANG_ARGS $extra"
  }
}

# 4) Make cc-rs prefer the active environment's compiler toolchain
# (avoids cc-rs picking a different Visual Studio instance).
$env:CC = 'cl.exe'
$env:CXX = 'cl.exe'

# Ensure MSVC parses source files as UTF-8. whisper.cpp contains symbols like ♪ ♫ etc.
if ([string]::IsNullOrWhiteSpace($env:CL)) {
  $env:CL = '/utf-8'
} elseif ($env:CL -notmatch '(^|\s)/utf-8(\s|$)') {
  $env:CL = "/utf-8 $env:CL"
}

# Help cmake-rs pick the same generator as the active MSVC toolchain.
$clPath = (Get-Command cl -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
if ($clPath -match '\\2019\\') {
  $env:CMAKE_GENERATOR = 'Visual Studio 16 2019'
} elseif ($clPath -match '\\2022\\') {
  $env:CMAKE_GENERATOR = 'Visual Studio 17 2022'
}

Write-Host "Loaded build env:" -ForegroundColor Cyan
Write-Host "  LIBCLANG_PATH=$env:LIBCLANG_PATH"
Write-Host "  CLANG_PATH=$env:CLANG_PATH"
Write-Host "  BINDGEN_EXTRA_CLANG_ARGS=$env:BINDGEN_EXTRA_CLANG_ARGS"
Write-Host "  CC=$env:CC"
Write-Host "  CXX=$env:CXX"
Write-Host "  CMAKE_GENERATOR=$env:CMAKE_GENERATOR"
Write-Host "  CMAKE=$env:CMAKE"
Write-Host "  CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
Write-Host "  CMake=" -NoNewline; (Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source) | Write-Host
Write-Host "  cl=" -NoNewline; (Get-Command cl -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source) | Write-Host
