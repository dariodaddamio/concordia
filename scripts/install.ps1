# Concordia one-line installer for Windows. No administrator rights needed.
#
#   irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1 | iex
#
# With options (script or scriptblock form):
#   & ([scriptblock]::Create((irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1))) -Version 0.1.0
#
# What it does: reads the release feed, downloads the Windows zip and its .sha256, verifies the
# SHA-256 with .NET (fails closed on any mismatch, nothing is installed), extracts to a temp folder
# and runs `concordia.exe install`. That copies concordia to %LOCALAPPDATA%\concordia\bin and adds
# that folder to your USER PATH. Nothing else on the machine is changed.
#
# Windows PowerShell 5.1 compatible: keep this file ASCII-only and avoid newer syntax.
param(
    # The GitHub owner/name that publishes concordia releases.
    [string]$Repo = 'dariodaddamio/concordia',
    # Install this version (0.1.0 or v0.1.0). Default: the newest published release.
    [string]$Version = '',
    # Install into this folder instead of %LOCALAPPDATA%\concordia\bin. The user PATH is then not
    # changed; add the folder to PATH yourself.
    [string]$Dir = '',
    # Print the plan and exit. Makes no network calls and changes nothing.
    [switch]$DryRun,
    # GitHub API root. Only for mirrors and the offline tests.
    [string]$ApiBase = 'https://api.github.com'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch { }

$AssetZip = 'concordia-x86_64-pc-windows-msvc.zip'
$AssetSha = "$AssetZip.sha256"
$Headers = @{ 'User-Agent' = 'concordia-installer'; 'Accept' = 'application/vnd.github+json' }

function Stop-Install([string]$Message) {
    throw "concordia install: $Message"
}

function Get-Sha256Hex([string]$Path) {
    $fs = [IO.File]::OpenRead($Path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha.ComputeHash($fs)) -replace '-', '').ToLower() }
        finally { $sha.Dispose() }
    } finally { $fs.Dispose() }
}

function Read-ExpectedHash([string]$Path, [string]$Name) {
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        if ($line.Trim() -match '^([0-9A-Fa-f]{64})\s+\*?(.+)$') {
            if ($matches[2].Trim() -eq $Name) { return $matches[1].ToLower() }
        }
    }
    Stop-Install "$AssetSha has no SHA-256 entry for $Name"
}

function Invoke-Main {
    if ($Repo -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
        Stop-Install "-Repo must look like owner/name, got '$Repo'"
    }
    if ($Repo -like 'REPLACE-*') {
        if (-not $DryRun) {
            Stop-Install "this installer still has the placeholder repo '$Repo'. Pass -Repo owner/name or fix the constant."
        }
        Write-Host "WARNING: placeholder repo '$Repo' (the real one is not configured yet)"
    }
    $Version = $Version.Trim() -replace '^[vV]', ''
    if ($Version -and $Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
        Stop-Install "-Version must look like 0.1.0, got '$Version'"
    }

    $installDir = $Dir
    if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'concordia\bin' }
    $base = $ApiBase.TrimEnd('/')
    if ($Version) {
        $feed = "$base/repos/$Repo/releases/tags/v$Version"
    } else {
        # Lists newest first and includes pre-releases (0.x releases are marked pre-release).
        $feed = "$base/repos/$Repo/releases?per_page=30"
    }

    if ($DryRun) {
        Write-Host 'DRY RUN: nothing is downloaded, installed or changed.'
        Write-Host "Release feed : $feed"
        if ($Version) { Write-Host "Version      : v$Version" } else { Write-Host 'Version      : newest published release (drafts skipped)' }
        Write-Host "Assets       : $AssetZip and $AssetSha"
        Write-Host "Install dir  : $installDir"
        Write-Host 'Steps        : 1 read feed, 2 download zip and .sha256, 3 verify SHA-256 (fail closed),'
        Write-Host '               4 extract to a temp folder, 5 run concordia.exe install, 6 print version'
        if ($Dir) { Write-Host 'PATH         : not changed (-Dir given)' } else { Write-Host 'PATH         : install folder added to the USER PATH if missing' }
        return
    }

    Write-Host "Reading release feed for $Repo ..."
    $body = Invoke-RestMethod -Uri $feed -Headers $Headers -TimeoutSec 30
    $chosen = $null
    foreach ($rel in @($body)) {
        if ($rel.draft) { continue }
        $zip = @($rel.assets | Where-Object { $_.name -eq $AssetZip }) | Select-Object -First 1
        $sha = @($rel.assets | Where-Object { $_.name -eq $AssetSha }) | Select-Object -First 1
        if ($zip -and $sha) {
            $chosen = [pscustomobject]@{ Tag = [string]$rel.tag_name; ZipUrl = [string]$zip.browser_download_url; ShaUrl = [string]$sha.browser_download_url }
            break
        }
    }
    if (-not $chosen) {
        if ($Version) { Stop-Install "release v$Version has no $AssetZip" }
        Stop-Install "no published release with $AssetZip found in $Repo"
    }
    Write-Host "Found $($chosen.Tag)"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('concordia-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $savedInstallDir = $env:CONCORDIA_INSTALL_DIR
    try {
        $zipPath = Join-Path $tmp $AssetZip
        $shaPath = Join-Path $tmp $AssetSha
        Write-Host "Downloading $AssetZip ..."
        Invoke-WebRequest -UseBasicParsing -Uri $chosen.ZipUrl -Headers $Headers -OutFile $zipPath -TimeoutSec 300
        Invoke-WebRequest -UseBasicParsing -Uri $chosen.ShaUrl -Headers $Headers -OutFile $shaPath -TimeoutSec 60

        $expected = Read-ExpectedHash $shaPath $AssetZip
        $actual = Get-Sha256Hex $zipPath
        if ($actual -ne $expected) {
            Stop-Install "checksum mismatch for $AssetZip (expected $expected, got $actual). Nothing was installed."
        }
        Write-Host "SHA-256 verified ($actual)"

        $stage = Join-Path $tmp 'extract'
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::ExtractToDirectory($zipPath, $stage)
        $exe = Join-Path $stage 'concordia.exe'
        if (-not (Test-Path -LiteralPath $exe)) { Stop-Install "the zip has no concordia.exe at its root" }

        # Only set for the child process; `concordia install` reads it to pick the folder.
        if ($Dir) { $env:CONCORDIA_INSTALL_DIR = $installDir }
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $out = & $exe install 2>&1 | Out-String
            $code = $LASTEXITCODE
        } finally { $ErrorActionPreference = $prevEap }
        Write-Host $out.TrimEnd()
        if ($code -ne 0) { Stop-Install "concordia.exe install exited with code $code" }
    } finally {
        $env:CONCORDIA_INSTALL_DIR = $savedInstallDir
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    $installed = Join-Path $installDir 'concordia.exe'
    $verLine = & $installed --version | Select-Object -First 1
    Write-Host ''
    Write-Host "Installed: $verLine"
    if ($Dir) {
        Write-Host "Add $installDir to your PATH to run concordia from any terminal."
    } else {
        Write-Host 'Open a NEW terminal, then run: concordia --version'
    }
}

Invoke-Main
