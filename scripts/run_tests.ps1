# scripts/run_tests.ps1
# This script builds, configures, and runs MASR backend Rust unit/integration tests on Windows.

Write-Host "1. Building library test executables..." -ForegroundColor Cyan
cd src-tauri
cargo test --no-run --lib
if ($LASTEXITCODE -ne 0) {
    Write-Error "Cargo compilation failed!"
    exit $LASTEXITCODE
}
cd ..

# Paths
$targetDebug = "D:\Downloads\target\debug"
$targetDeps = "$targetDebug\deps"

Write-Host "2. Copying DirectML.dll to deps..." -ForegroundColor Cyan
if (Test-Path "$targetDebug\DirectML.dll") {
    Copy-Item -Path "$targetDebug\DirectML.dll" -Destination "$targetDeps\DirectML.dll" -Force
} else {
    Write-Warning "DirectML.dll not found in $targetDebug. Hopefully it is not needed or already present."
}

Write-Host "3. Finding the compiled test executable..." -ForegroundColor Cyan
$testExes = Get-ChildItem -Path $targetDeps -Filter thegai_app_lib-*.exe
if ($testExes.Count -eq 0) {
    Write-Error "No compiled test executable found!"
    exit 1
}

# Use the latest modified test executable
$testExe = $testExes | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Host "Found test runner: $($testExe.FullName)" -ForegroundColor Green

Write-Host "4. Generating temporary app.manifest..." -ForegroundColor Cyan
$manifestPath = "$env:TEMP\app.manifest"
$manifestContent = @"
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"@
Set-Content -Path $manifestPath -Value $manifestContent

Write-Host "5. Finding mt.exe in Windows Kits..." -ForegroundColor Cyan
$mtExe = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\mt.exe"
if (-not (Test-Path $mtExe)) {
    # Fallback to another version if 26100 is not present
    $mtExes = Get-ChildItem -Path "C:\Program Files (x86)\Windows Kits" -Filter mt.exe -Recurse -ErrorAction SilentlyContinue
    if ($mtExes.Count -gt 0) {
        $mtExe = ($mtExes | Sort-Object FullName -Descending | Select-Object -First 1).FullName
    } else {
        Write-Error "mt.exe (Windows Manifest Tool) not found! Cannot embed manifest."
        Remove-Item -Path $manifestPath -ErrorAction SilentlyContinue
        exit 1
    }
}
Write-Host "Using Manifest Tool: $mtExe" -ForegroundColor Green

Write-Host "6. Embedding manifest into test runner..." -ForegroundColor Cyan
& $mtExe -manifest $manifestPath "-outputresource:$($testExe.FullName);#1"
if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to embed manifest!"
    Remove-Item -Path $manifestPath -ErrorAction SilentlyContinue
    exit $LASTEXITCODE
}
Remove-Item -Path $manifestPath -ErrorAction SilentlyContinue

Write-Host "7. Running tests..." -ForegroundColor Cyan
# Run the test binary with any arguments passed to the script, e.g. custom filters
& $testExe.FullName $args
exit $LASTEXITCODE
