# One entry point for Harbinger developer installs and existing-item Workshop packages.
# Source of truth: this repository. target/, Steam mods/, and dist/ are generated copies.
param(
    [ValidateSet('Dev', 'Workshop')]
    [string]$Target = 'Dev',
    [string]$GameDir = (Join-Path ${env:ProgramFiles(x86)} 'Steam\steamapps\common\Teamfight Manager2')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$ModId = 'tfm2_direct_control'
$ExpectedWorkshopId = '3807134574'
$Package = Join-Path $RepoRoot "dist\workshop\$ModId"

function Invoke-Checked {
    param(
        [string]$Command,
        [string[]]$Arguments
    )
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
    }
}

function Get-WorkshopItemId {
    param([string]$File)
    $data = Get-Content -LiteralPath $File -Raw | ConvertFrom-Json
    if ($data.PSObject.Properties.Name -notcontains 'published_file_id') {
        throw "'$File' is missing published_file_id."
    }
    return [string]$data.published_file_id
}

if ($Target -eq 'Dev') {
    Write-Host 'DEV: Building the checked-out branch and installing into the game.'
    & (Join-Path $PSScriptRoot 'install-dev.ps1') -GameDir $GameDir
    if (-not $?) {
        throw 'Development installation failed.'
    }
    Write-Warning 'Do not load this local copy simultaneously with a Steam Workshop installation.'
    return
}

# GitHub Desktop bundles Git without necessarily placing it on the user's PATH.
# Discover the bundled copy when the standalone Git for Windows CLI is absent.
if (-not (Get-Command 'git' -ErrorAction SilentlyContinue)) {
    $gitCandidates = @(
        (Join-Path $env:ProgramFiles 'Git\cmd\git.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Git\cmd\git.exe')
    )
    if ($env:LOCALAPPDATA) {
        $desktopDir = Join-Path $env:LOCALAPPDATA 'GitHubDesktop'
        if (Test-Path -LiteralPath $desktopDir) {
            foreach ($install in (Get-ChildItem -LiteralPath $desktopDir -Directory -Filter 'app-*' |
                Sort-Object Name -Descending)) {
                $gitCandidates += (Join-Path $install.FullName 'resources\app\git\cmd\git.exe')
                $gitCandidates += (Join-Path $install.FullName 'resources\app\git\bin\git.exe')
            }
        }
    }
    $foundGit = $gitCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
        Select-Object -First 1
    if (-not $foundGit) {
        throw 'Git CLI not found. Install Git for Windows or use the Git bundled with GitHub Desktop.'
    }
    $env:Path = (Split-Path $foundGit -Parent) + ';' + $env:Path
    Write-Host "Using Git at $foundGit"
}

# A release is never built from an ambiguous working tree, the wrong branch,
# an out-of-date local main, or a leftover tracing-feature release artifact.
Push-Location $RepoRoot
try {
    $branch = (& git branch --show-current).Trim()
    if ($LASTEXITCODE -ne 0 -or $branch -ne 'main') {
        throw 'Workshop packaging requires main. Switch to main in GitHub Desktop, fetch and pull.'
    }

    $pending = @(& git status --porcelain)
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not check Git working-tree status.'
    }
    if ($pending.Count -gt 0) {
        throw 'Uncommitted files exist. Commit or discard them before preparing a Workshop release.'
    }

    $commit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not determine the release commit.'
    }
    $originMain = (& git rev-parse --verify refs/remotes/origin/main 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $originMain) {
        throw 'Missing origin/main. Fetch origin and pull main with GitHub Desktop.'
    }
    $originMain = $originMain.Trim()
    if ($commit -ne $originMain) {
        throw 'Local main differs from origin/main. Fetch and pull in GitHub Desktop before packaging.'
    }

    if (-not (Test-Path -LiteralPath $Package -PathType Container)) {
        throw "Original publishing folder missing: $Package. Restore it from backup; do not create a new Workshop item."
    }

    $idFile = Join-Path $Package 'mod.workshop_id'
    $preview = Join-Path $Package 'preview.png'
    if (-not (Test-Path -LiteralPath $idFile -PathType Leaf)) {
        throw "Missing original mod.workshop_id in $Package. Restore it; never publish while uploader says New item."
    }
    if (-not (Test-Path -LiteralPath $preview -PathType Leaf)) {
        throw "Missing original preview.png in $Package. Restore the existing Workshop preview before packaging."
    }
    $itemId = Get-WorkshopItemId $idFile
    if ($itemId -ne $ExpectedWorkshopId) {
        throw "Workshop ID mismatch: found $itemId but expected $ExpectedWorkshopId. Refusing to package."
    }

    # Keep exactly one intentional, established publish directory. Unexpected
    # files are stopped rather than silently shipping local test artifacts.
    $allowed = @('mod.workshop_id', 'preview.png', 'mod.mod_info', "$ModId.dll")
    $unexpected = @(Get-ChildItem -LiteralPath $Package -Force |
        Where-Object { $_.Name -notin $allowed })
    if ($unexpected.Count -gt 0) {
        $names = ($unexpected | ForEach-Object { $_.Name }) -join ', '
        throw "Unexpected content in publishing folder: $names. Review/remove it before packaging."
    }

    # The publishing folder is gitignored, so protect its irreplaceable
    # publishing identity and existing artwork outside the generated directories.
    $documents = [Environment]::GetFolderPath('MyDocuments')
    if ([string]::IsNullOrWhiteSpace($documents)) {
        throw 'Could not locate Documents for the one-time Workshop identity backup.'
    }
    $backup = Join-Path $documents 'Harbinger-Publishing-Backup'
    New-Item -ItemType Directory -Path $backup -Force | Out-Null
    $backupId = Join-Path $backup 'mod.workshop_id'
    $backupPreview = Join-Path $backup 'preview.png'
    if (Test-Path -LiteralPath $backupId) {
        if ((Get-WorkshopItemId $backupId) -ne $ExpectedWorkshopId) {
            throw "Existing Workshop identity backup does not match $ExpectedWorkshopId: $backupId"
        }
    } else {
        Copy-Item -LiteralPath $idFile -Destination $backupId
    }
    if (-not (Test-Path -LiteralPath $backupPreview)) {
        Copy-Item -LiteralPath $preview -Destination $backupPreview
    }

    $metadata = Get-Content -LiteralPath (Join-Path $RepoRoot 'mod.mod_info') -Raw | ConvertFrom-Json
    $cargo = Get-Content -LiteralPath (Join-Path $RepoRoot 'Cargo.toml') -Raw
    $matchVersion = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $matchVersion.Success -or [string]$metadata.version -ne $matchVersion.Groups[1].Value) {
        throw "Cargo.toml and mod.mod_info disagree on version. Fix and commit metadata before packaging."
    }

    Write-Host "WORKSHOP: Validated original item $itemId; building main commit $commit"
    & (Join-Path $PSScriptRoot 'bootstrap-sdk.ps1') -GameDir $GameDir -Refresh
    if (-not $?) {
        throw 'SDK bootstrap failed.'
    }
    Invoke-Checked -Command 'cargo' -Arguments @('fmt', '--check')
    Invoke-Checked -Command 'cargo' -Arguments @('test')
    # Do not risk copying an old feature-enabled/native-tracing DLL.
    Invoke-Checked -Command 'cargo' -Arguments @('clean', '--release')
    Invoke-Checked -Command 'cargo' -Arguments @('build', '--release')

    $compiled = Join-Path $RepoRoot "target\release\$ModId.dll"
    if (-not (Test-Path -LiteralPath $compiled -PathType Leaf)) {
        throw "Missing freshly compiled DLL at $compiled."
    }
    $stagedDll = Join-Path $Package "$ModId.dll"
    Copy-Item -LiteralPath $compiled -Destination $stagedDll -Force
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'mod.mod_info') -Destination (Join-Path $Package 'mod.mod_info') -Force
    $sourceHash = (Get-FileHash -LiteralPath $compiled -Algorithm SHA256).Hash
    $packageHash = (Get-FileHash -LiteralPath $stagedDll -Algorithm SHA256).Hash
    if ($sourceHash -ne $packageHash) {
        throw 'Staged DLL differs from the compiled release DLL.'
    }

    # This manifest is OUTSIDE the upload directory; the uploader should stage
    # only the runtime DLL, metadata and selected existing artwork.
    $manifest = Join-Path (Split-Path $Package -Parent) 'RELEASE-MANIFEST.txt'
    @(
        "Harbinger v$($metadata.version) — staged, NOT uploaded",
        "Git commit: $commit",
        "Workshop item: $itemId",
        "DLL SHA256: $sourceHash",
        "Staged DLL: $stagedDll",
        "Identity/artwork backup: $backup",
        "Prepared at: $([DateTimeOffset]::Now.ToString('o'))"
    ) | Set-Content -LiteralPath $manifest

    Write-Host ''
    Write-Host "SUCCESS: Prepared existing Workshop item $itemId, version $($metadata.version)"
    Write-Host "Package: $Package"
    Write-Host "Build manifest: $manifest"
    Write-Host 'NEXT: In TFM2ModUploader.exe select THIS package, click Refresh, verify'
    Write-Host "'Workshop item' shows $itemId, then Build Only and Update Workshop Item."
    Write-Host 'Do not choose the Steam mods directory or the repository root.'
}
finally {
    Pop-Location
}
