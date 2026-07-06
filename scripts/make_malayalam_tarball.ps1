# scripts/make_malayalam_tarball.ps1
# This script normalizes the Malayalam ASR model files and bundles them into a .tar.gz archive.

$ErrorActionPreference = "Stop"

$asrProjectDir = "d:\Downloads\Projects\Asr malayalam"
$zipPath = Join-Path $asrProjectDir "indicconformer_ml_ctc_onnx.zip"
$extractedModelDir = Join-Path $asrProjectDir "model"
$masrRootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetTarballName = "thegav1.tar.gz"
$targetTarballPath = Join-Path $masrRootDir $targetTarballName

# Create a temporary directory
$tempDir = Join-Path $env:TEMP "masr_tarball_temp_$(Get-Random)"
$stageDir = Join-Path $tempDir "thegav1"
New-Item -ItemType Directory -Path $stageDir -Force | Out-Null

Write-Host "Staging model files..."
$copiedFromExtracted = $false

if (Test-Path $extractedModelDir) {
    # Check if the required files exist in the extracted model dir
    $reqFiles = @("model.onnx", "model.onnx.data", "vocab.txt", "config.json")
    $allExist = $true
    foreach ($file in $reqFiles) {
        if (-not (Test-Path (Join-Path $extractedModelDir $file))) {
            $allExist = $false
            break
        }
    }

    if ($allExist) {
        Write-Host "Copying files directly from already extracted model directory: $extractedModelDir"
        foreach ($file in $reqFiles) {
            Copy-Item (Join-Path $extractedModelDir $file) (Join-Path $stageDir $file)
        }
        $copiedFromExtracted = $true
    }
}

if (-not $copiedFromExtracted) {
    if (-not (Test-Path $zipPath)) {
        Write-Error "Could not find model zip at $zipPath or extracted files at $extractedModelDir"
    }

    Write-Host "Extracting $zipPath (this may take a minute)..."
    $zipTempDir = Join-Path $tempDir "zip_extract"
    New-Item -ItemType Directory -Path $zipTempDir -Force | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $zipTempDir

    # Try to find the files in the zip extract directory
    # Depending on how the zip is structured, they could be at the root or in a subfolder
    $onnxPath = Get-ChildItem -Path $zipTempDir -Filter "model.onnx" -Recurse | Select-Object -First 1
    if ($null -eq $onnxPath) {
        # Check for encoder.onnx and rename it if needed
        $onnxPath = Get-ChildItem -Path $zipTempDir -Filter "encoder.onnx" -Recurse | Select-Object -First 1
    }

    if ($null -eq $onnxPath) {
        Write-Error "Could not find model.onnx or encoder.onnx in the extracted zip archive."
    }

    $sourceDir = $onnxPath.DirectoryName
    Write-Host "Found model files at: $sourceDir"

    # Copy and normalize filenames
    # If the file is encoder.onnx, rename it to model.onnx
    if (Test-Path (Join-Path $sourceDir "encoder.onnx")) {
        Copy-Item (Join-Path $sourceDir "encoder.onnx") (Join-Path $stageDir "model.onnx")
    } else {
        Copy-Item (Join-Path $sourceDir "model.onnx") (Join-Path $stageDir "model.onnx")
    }

    if (Test-Path (Join-Path $sourceDir "encoder.onnx.data")) {
        Copy-Item (Join-Path $sourceDir "encoder.onnx.data") (Join-Path $stageDir "model.onnx.data")
    } else {
        Copy-Item (Join-Path $sourceDir "model.onnx.data") (Join-Path $stageDir "model.onnx.data")
    }

    Copy-Item (Join-Path $sourceDir "vocab.txt") (Join-Path $stageDir "vocab.txt")
    Copy-Item (Join-Path $sourceDir "config.json") (Join-Path $stageDir "config.json")
}

# Verify staging directory files
Write-Host "Verifying staged files:"
Get-ChildItem -Path $stageDir | Out-Host

# Compress using tar (which is standard on modern Windows 10/11)
Write-Host "Creating tarball: $targetTarballPath"
if (Test-Path $targetTarballPath) {
    Remove-Item $targetTarballPath -Force
}

# Run tar.exe. -C changes directory to $tempDir before compressing the folder thegav1
tar.exe -czf $targetTarballPath -C $tempDir thegav1

# Clean up temp directory
Write-Host "Cleaning up temporary files..."
Remove-Item -Recurse -Force $tempDir

# Calculate SHA256
if (Test-Path $targetTarballPath) {
    $hash = (Get-FileHash -Path $targetTarballPath -Algorithm SHA256).Hash
    Write-Host "`nTarball created successfully!"
    Write-Host "Location: $targetTarballPath"
    Write-Host "SHA256: $hash"
    Write-Host "Size: $((Get-Item $targetTarballPath).Length) bytes"
} else {
    Write-Error "Failed to create tarball."
}
