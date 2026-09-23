param(
    [string]$GameExe = "C:\Program Files (x86)\Steam\steamapps\common\Teamfight Manager2\TeamfightManager2.exe",
    [string]$OutputPath = (Join-Path $env:TEMP "TFM2-DirectControl-follow-action-xrefs.txt")
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Scanner = Join-Path $RepoRoot "tools\scan_camera_action_xrefs.py"

if (-not (Test-Path $GameExe)) {
    throw "Teamfight Manager 2 executable not found at '$GameExe'."
}

if (-not (Test-Path $Scanner)) {
    throw "Follow-action scanner not found at '$Scanner'."
}

$Python = Get-Command python -ErrorAction SilentlyContinue
if ($Python) {
    & $Python.Source $Scanner $GameExe --output $OutputPath
}
else {
    $Py = Get-Command py -ErrorAction SilentlyContinue
    if (-not $Py) {
        throw "Python was not found in PATH. Install Python or make 'python'/'py' available, then retry."
    }
    & $Py.Source -3 $Scanner $GameExe --output $OutputPath
}

if ($LASTEXITCODE -ne 0) {
    throw "Native follow-action scan failed with exit code $LASTEXITCODE."
}

if (-not (Test-Path $OutputPath)) {
    throw "Native follow-action scanner completed without producing '$OutputPath'."
}

Write-Host "Native follow-action scan complete."
Write-Host "Report: $OutputPath"
Write-Host "Upload that text file to the chat for analysis."
