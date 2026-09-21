param(
    [string]$GameDir = (Join-Path ${env:ProgramFiles(x86)} "Steam\steamapps\common\Teamfight Manager2"),
    [switch]$Refresh
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$SdkRoot = Join-Path $RepoRoot "sdk"
$SdkSource = Join-Path $GameDir "mod-sdk-stable\mod-api-stable"
$SdkDestination = Join-Path $SdkRoot "mod-api-stable"

if (-not (Test-Path $SdkSource)) {
    throw "Stable SDK not found at '$SdkSource'. Pass the correct Teamfight Manager 2 folder with -GameDir."
}

if (Test-Path $SdkDestination) {
    if (-not $Refresh) {
        Write-Host "Stable SDK already exists at '$SdkDestination'."
        Write-Host "Pass -Refresh to replace it with the SDK from the installed game."
        exit 0
    }

    Remove-Item -LiteralPath $SdkDestination -Recurse -Force
}

New-Item -ItemType Directory -Path $SdkRoot -Force | Out-Null
Copy-Item -Path $SdkSource -Destination $SdkDestination -Recurse
Write-Host "Copied stable SDK to '$SdkDestination'."
