param(
    [string]$GameDir = "C:\Program Files (x86)\Steam\steamapps\common\Teamfight Manager2",
    [string]$OutputPath = (Join-Path $env:TEMP "TFM2-DirectControl-camera-strings.txt")
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Scanner = Join-Path $RepoRoot "tools\scan_camera_strings.py"

if (-not (Test-Path $GameDir)) {
    throw "Teamfight Manager 2 folder not found at '$GameDir'."
}

if (-not (Test-Path $Scanner)) {
    throw "Camera string scanner not found at '$Scanner'."
}

$Python = Get-Command python -ErrorAction SilentlyContinue
if ($Python) {
    & $Python.Source $Scanner --game-dir $GameDir --output $OutputPath
}
else {
    $Py = Get-Command py -ErrorAction SilentlyContinue
    if (-not $Py) {
        throw "Python was not found in PATH. Install Python or make 'python'/'py' available, then retry."
    }
    & $Py.Source -3 $Scanner --game-dir $GameDir --output $OutputPath
}

if ($LASTEXITCODE -ne 0) {
    throw "Camera string scan failed with exit code $LASTEXITCODE."
}

if (-not (Test-Path $OutputPath)) {
    throw "Camera string scanner completed without producing '$OutputPath'."
}

Write-Host "Camera string scan complete."
Write-Host "Report: $OutputPath"
Write-Host "Upload that text file to the chat for analysis."
