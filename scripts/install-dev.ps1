param(
    [string]$GameDir = (Join-Path ${env:ProgramFiles(x86)} "Steam\steamapps\common\Teamfight Manager2"),
    [switch]$ReplayTrace
)

$ErrorActionPreference = "Stop"

$ModId = "tfm2_direct_control"
$RepoRoot = Split-Path $PSScriptRoot -Parent
$SdkBootstrap = Join-Path $PSScriptRoot "bootstrap-sdk.ps1"
$ModDir = Join-Path $GameDir "mods\$ModId"
$DllSource = Join-Path $RepoRoot "target\release\$ModId.dll"
$ModInfoSource = Join-Path $RepoRoot "mod.mod_info"

& $SdkBootstrap -GameDir $GameDir -Refresh

Push-Location $RepoRoot
try {
    $cargoArgs = @('build', '--release')
    if ($ReplayTrace) {
        $cargoArgs += @('--features', 'replay-native-trace')
        Write-Host 'WARNING: installing temporary diagnostic native replay-action tracer.'
    }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path $DllSource)) {
    throw "Build completed but '$DllSource' was not found."
}

New-Item -ItemType Directory -Path $ModDir -Force | Out-Null
Copy-Item -Path $DllSource -Destination (Join-Path $ModDir "$ModId.dll") -Force
Copy-Item -Path $ModInfoSource -Destination (Join-Path $ModDir "mod.mod_info") -Force

Write-Host "Installed development build to '$ModDir'."
