# Concordia one-line installer for Windows. No administrator rights needed.
#
#   irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1 | iex
#
# With options (script or scriptblock form):
#   & ([scriptblock]::Create((irm https://github.com/dariodaddamio/concordia/raw/main/scripts/install.ps1))) -Version 0.1.0
#
# What it does: reads the release feed, downloads the Windows zip and its .sha256, verifies the
# SHA-256 with .NET (fails closed on any mismatch, nothing is installed), extracts to a temp folder
# and reuses owned hash-verified downloads under %LOCALAPPDATA%\concordia\cache\releases.
# Each check fetches a fresh checksum; bounded cleanup keeps the current zip and one rollback.
# and runs `concordia.exe install`. That copies concordia to %LOCALAPPDATA%\concordia\bin, adds
# it to PATH and opens Concordia. No compiler or administrator access is needed.
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
    # Install without opening a window (automation and unattended installation).
    [switch]$NoLaunch,
    # Print the plan and exit. Makes no network calls and changes nothing.
    [switch]$DryRun,
    # GitHub API root. Only for mirrors and the offline tests.
    [string]$ApiBase = 'https://api.github.com'
)

$installerPreviousEap = $ErrorActionPreference
$installerPreviousProgress = $ProgressPreference
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

# Cache entries are owned only by a bounded receipt AND matching bytes. Unknown files stay put.
function Get-ReleaseCacheRoot {
    if (-not $env:LOCALAPPDATA) { return $null }
    $root = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'concordia\cache\releases'))
    $part = $root
    while ($part) {
        if (Test-Path -LiteralPath $part) {
            $item = Get-Item -LiteralPath $part -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $null }
        }
        $parent = [IO.Path]::GetDirectoryName($part)
        if ($parent -eq $part) { break }
        $part = $parent
    }
    return $root
}

function Get-OwnedCachedZip([string]$Root, [string]$Hash) {
    try {
        if (-not $Root -or (Get-ReleaseCacheRoot) -ne $Root -or $Hash -cnotmatch '^[0-9a-f]{64}$') { return $null }
        $path = Join-Path $Root ($Hash + '.zip')
        $receiptPath = $path + '.receipt.json'
        $file = Get-Item -LiteralPath $path -Force -ErrorAction Stop
        $receiptFile = Get-Item -LiteralPath $receiptPath -Force -ErrorAction Stop
        foreach ($item in @($file, $receiptFile)) {
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                [IO.Path]::GetDirectoryName($item.FullName) -ne $Root) { return $null }
        }
        if ($file.Length -gt 67108864 -or $receiptFile.Length -gt 4096) { return $null }
        $receipt = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
        if ($receipt.owner -cne 'concordia-release-cache-v1' -or $receipt.sha256 -cne $Hash -or
            ($receipt.bytes -isnot [int] -and $receipt.bytes -isnot [long]) -or $receipt.bytes -ne $file.Length -or
            (Get-Sha256Hex $path) -cne $Hash) { return $null }
        return $file
    } catch { return $null }
}

function Save-ReleaseCache([string]$Root, [string]$Hash, [string]$Zip) {
    if (-not $Root -or (Get-ReleaseCacheRoot) -ne $Root) { return }
    try {
        [IO.Directory]::CreateDirectory($Root) | Out-Null
        if ((Get-ReleaseCacheRoot) -ne $Root) { return }
        $path = Join-Path $Root ($Hash + '.zip')
        if (-not (Test-Path -LiteralPath $path) -and -not (Test-Path -LiteralPath ($path + '.receipt.json'))) {
            $inputFile = [IO.File]::OpenRead($Zip)
            try {
                $outputFile = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                try { $inputFile.CopyTo($outputFile) } finally { $outputFile.Dispose() }
            } finally { $inputFile.Dispose() }
            if ((Get-Sha256Hex $path) -cne $Hash) { return }
            $receipt = @{ owner = 'concordia-release-cache-v1'; sha256 = $Hash; bytes = (Get-Item -LiteralPath $Zip).Length } | ConvertTo-Json -Compress
            $outputFile = [IO.File]::Open(($path + '.receipt.json'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try {
                $bytes = [Text.Encoding]::UTF8.GetBytes($receipt)
                $outputFile.Write($bytes, 0, $bytes.Length)
            } finally { $outputFile.Dispose() }
        }
        # Scan at most 64 directory entries, keep the selected artifact and newest rollback.
        $owned = @()
        $seen = 0
        foreach ($entry in [IO.Directory]::EnumerateFiles($Root)) {
            $seen++
            if ($seen -gt 64) { break }
            if ([IO.Path]::GetFileName($entry) -cmatch '^([0-9a-f]{64})\.zip$') {
                $candidate = Get-OwnedCachedZip $Root $matches[1]
                if ($candidate) { $owned += $candidate }
            }
        }
        $obsolete = @($owned | Where-Object { $_.Name -ne ($Hash + '.zip') } | Sort-Object LastWriteTimeUtc -Descending | Select-Object -Skip 1 -First 4)
        foreach ($entry in $obsolete) {
            $oldHash = [IO.Path]::GetFileNameWithoutExtension($entry.Name)
            if (Get-OwnedCachedZip $Root $oldHash) {
                Remove-Item -LiteralPath $entry.FullName -Force
                Remove-Item -LiteralPath ($entry.FullName + '.receipt.json') -Force
            }
        }
    } catch { Write-Host 'Release cache unavailable; continuing with the verified temporary download.' }
}

function Assert-DownloadUrl([string]$Value, [uri]$Api) {
    $uri = $null
    if (-not [uri]::TryCreate($Value, [UriKind]::Absolute, [ref]$uri) -or $uri.UserInfo) {
        Stop-Install 'the release feed returned an invalid download URL'
    }
    if ($uri.Scheme -eq 'https') { return }
    if ($uri.Scheme -eq 'http' -and $Api.IsLoopback -and $uri.IsLoopback -and $uri.Authority -eq $Api.Authority) { return }
    Stop-Install 'release downloads require HTTPS'
}

function Expand-VerifiedRelease([string]$Zip, [string]$Stage) {
    if ((Get-Item -LiteralPath $Zip).Length -gt 67108864) { Stop-Install 'release zip exceeds 64 MiB' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        if ($archive.Entries.Count -gt 8) { Stop-Install 'release zip contains too many entries' }
        $names = @{}
        $total = 0L
        foreach ($entry in $archive.Entries) {
            if ($entry.FullName -notin @('concordia.exe', 'LICENSE', 'THIRD-PARTY-LICENSES.txt')) {
                Stop-Install "release zip contains an unexpected path: $($entry.FullName)"
            }
            if ($names.ContainsKey($entry.FullName)) { Stop-Install 'release zip contains duplicate paths' }
            $names[$entry.FullName] = $true
            $total += $entry.Length
            if ($entry.Length -gt 67108864 -or $total -gt 100663296) { Stop-Install 'release zip exceeds extraction limits' }
        }
        if (-not $names.ContainsKey('concordia.exe')) { Stop-Install 'the zip has no concordia.exe at its root' }
    } finally { $archive.Dispose() }
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Stage)
}

function Invoke-ReleaseExe([string]$Exe, [string]$Argument) {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $Exe
    $start.Arguments = $Argument
    $start.WorkingDirectory = Split-Path -Parent $Exe
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { Stop-Install "could not start concordia $Argument" }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(60000)) {
            $process.Kill()
            $process.WaitForExit()
            Stop-Install "concordia $Argument timed out"
        }
        $out = $stdout.GetAwaiter().GetResult()
        $err = $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { Stop-Install "concordia $Argument failed: $err" }
        return $out.TrimEnd()
    } finally { $process.Dispose() }
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
    if (-not $installDir -and $env:CONCORDIA_INSTALL_DIR) { $installDir = $env:CONCORDIA_INSTALL_DIR }
    if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'concordia\bin' }
    $installDir = [IO.Path]::GetFullPath($installDir)
    $channelPath = Join-Path $installDir 'channel'
    if (Test-Path -LiteralPath $channelPath) {
        $marker = Get-Item -LiteralPath $channelPath -Force
        if ($marker.PSIsContainer -or ($marker.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $marker.Length -gt 16 -or
            ([IO.File]::ReadAllText($channelPath).Trim() -notin @('source', 'release'))) {
            Stop-Install 'invalid install channel file; repair or remove the owned channel marker before reinstalling'
        }
    }
    $base = $ApiBase.TrimEnd('/')
    $api = $null
    if (-not [uri]::TryCreate($base, [UriKind]::Absolute, [ref]$api) -or $api.UserInfo -or
        ($api.Scheme -ne 'https' -and -not ($api.Scheme -eq 'http' -and $api.IsLoopback))) {
        Stop-Install '-ApiBase must use HTTPS (HTTP is allowed only for local fixtures)'
    }
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
        Write-Host 'Steps        : 1 read feed, 2 fetch checksum and reuse/download zip, 3 verify SHA-256 (fail closed),'
        Write-Host '               4 validate/extract, 5 install prebuilt executable, 6 verify version'
        if (-not $NoLaunch) { Write-Host '               7 open Concordia' }
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
    if ($chosen.Tag -notmatch '^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$') { Stop-Install 'release tag has an invalid version' }
    $expectedVersion = $matches[1]
    Assert-DownloadUrl $chosen.ZipUrl $api
    Assert-DownloadUrl $chosen.ShaUrl $api

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ('concordia-install-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $savedInstallDir = $env:CONCORDIA_INSTALL_DIR
    try {
        $zipPath = Join-Path $tmp $AssetZip
        $shaPath = Join-Path $tmp $AssetSha
        Invoke-WebRequest -UseBasicParsing -Uri $chosen.ShaUrl -Headers $Headers -OutFile $shaPath -TimeoutSec 60
        $expected = Read-ExpectedHash $shaPath $AssetZip
        $cacheRoot = Get-ReleaseCacheRoot
        $cached = Get-OwnedCachedZip $cacheRoot $expected
        if ($cached) {
            Write-Host "Reusing SHA-256 verified cached $AssetZip ..."
            [IO.File]::Copy($cached.FullName, $zipPath, $false)
        } else {
            Write-Host "Downloading $AssetZip ..."
            Invoke-WebRequest -UseBasicParsing -Uri $chosen.ZipUrl -Headers $Headers -OutFile $zipPath -TimeoutSec 300
        }
        $actual = Get-Sha256Hex $zipPath
        if ($actual -ne $expected) {
            Stop-Install "checksum mismatch for $AssetZip (expected $expected, got $actual). Nothing was installed."
        }
        Write-Host "SHA-256 verified ($actual)"

        $stage = Join-Path $tmp 'extract'
        Expand-VerifiedRelease $zipPath $stage
        Save-ReleaseCache $cacheRoot $expected $zipPath
        $exe = Join-Path $stage 'concordia.exe'
        if (-not (Test-Path -LiteralPath $exe)) { Stop-Install "the zip has no concordia.exe at its root" }

        # Only set for the child process; `concordia install` reads it to pick the folder.
        if ($Dir) { $env:CONCORDIA_INSTALL_DIR = $installDir }
        $versionLine = Invoke-ReleaseExe $exe '--version'
        if ($versionLine -notmatch ('^concordia ' + [regex]::Escape($expectedVersion) + ' \(')) {
            Stop-Install "downloaded executable does not match release $($chosen.Tag)"
        }
        $downloadedExeHash = Get-Sha256Hex $exe
        $out = Invoke-ReleaseExe $exe 'install'
        Write-Host (($out -split '\r?\n' | Where-Object { $_ -notmatch 'open a NEW terminal' }) -join [Environment]::NewLine)
    } finally {
        $env:CONCORDIA_INSTALL_DIR = $savedInstallDir
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
        if ([IO.Path]::GetFullPath($tmp).StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    $installed = Join-Path $installDir 'concordia.exe'
    if ((Get-Sha256Hex $installed) -ne $downloadedExeHash) { Stop-Install 'installed executable did not match the download' }
    $verLine = Invoke-ReleaseExe $installed '--version'
    [IO.File]::WriteAllText($channelPath, "release`n", (New-Object Text.UTF8Encoding($false)))
    # The child installer updates future terminals; this process also needs the new command now.
    $env:Path = "$installDir;" + (($env:Path -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ine $installDir.TrimEnd('\') }) -join ';')
    Write-Host ''
    Write-Host "Installed: $verLine"
    Write-Host 'Run concordia any time. Use /update inside the app for updates.'
    if (-not $NoLaunch) {
        Write-Host 'Opening Concordia...'
        Start-Process -FilePath $installed -WindowStyle Normal
    }
}

try { Invoke-Main } finally {
    $ErrorActionPreference = $installerPreviousEap
    $ProgressPreference = $installerPreviousProgress
}
