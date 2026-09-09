param(
    [string]$GameDir = "C:\Program Files (x86)\Steam\steamapps\common\Teamfight Manager2"
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ProbeDir = Join-Path $RepoRoot "tools\tfm2_camera_probe"
$SdkDir = Join-Path $GameDir "mod-sdk"
$BuildScript = Join-Path $SdkDir "build_mod.bat"
$ProbeDll = Join-Path $ProbeDir "tfm2_camera_probe.dll"
$InstallDir = Join-Path $GameDir "mods\tfm2_camera_probe"

if (-not (Test-Path $BuildScript)) {
    throw "Classic 0.5 Mod SDK not found at '$SdkDir'. Expected '$BuildScript'."
}

if (-not (Test-Path $ProbeDir)) {
    throw "Camera probe source not found at '$ProbeDir'."
}

Write-Host "Building disposable classic camera probe with TFM2's matching 0.5 SDK..."
Push-Location $SdkDir
try {
    & $BuildScript $ProbeDir
    if ($LASTEXITCODE -ne 0) {
        throw "Classic SDK build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path $ProbeDll)) {
    throw "Classic SDK reported success, but '$ProbeDll' was not produced."
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $ProbeDll (Join-Path $InstallDir "tfm2_camera_probe.dll") -Force
Copy-Item (Join-Path $ProbeDir "mod.mod_info") (Join-Path $InstallDir "mod.mod_info") -Force

Write-Host "Installed disposable camera probe to '$InstallDir'."
Write-Host "This probe is separate from tfm2_direct_control; enable it only for camera discovery."
Write-Host "If it loads, its runtime dump will be written to:"
Write-Host "  $env:TEMP\TFM2-DirectControl-classic-camera-probe.txt"
