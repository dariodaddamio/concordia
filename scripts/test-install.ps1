# Offline installer fixtures: loopback feed, isolated install, no persistent user PATH change.
#  - -DryRun prints a plan and never touches the network or the disk.
#  - A wrong SHA-256 fails closed before anything is extracted.
#  - A correct SHA-256 passes verification, then stops because the fake zip has no concordia.exe.
# The fake feed is an HttpListener on 127.0.0.1 in a separate hidden process (stopped by PID at the
# end). It is not a background job: a job's child process keeps this shell's stdout open.
# Usage: powershell -NoProfile -File scripts/test-install.ps1
$ErrorActionPreference = 'Stop'
$installer = Join-Path $PSScriptRoot 'install.ps1'
$assetZip = 'concordia-x86_64-pc-windows-msvc.zip'
$work = Join-Path ([IO.Path]::GetTempPath()) ('concordia-test-install-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$savedLocalAppData = $env:LOCALAPPDATA
$env:LOCALAPPDATA = Join-Path $work 'local-app-data'

$failures = @()
function Ok([string]$m) { Write-Host "OK   $m" }
function Fail([string]$m) { $script:failures += $m; Write-Host "FAIL $m" }
function Check([bool]$cond, [string]$m) { if ($cond) { Ok $m } else { Fail $m } }

function Get-FreePort {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
    $l.Start()
    $p = $l.LocalEndpoint.Port
    $l.Stop()
    return $p
}

# Runs install.ps1 in a child Windows PowerShell and returns exit code and combined output.
function Invoke-Installer([string[]]$CliArgs) {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $installer @CliArgs 2>&1 | Out-String
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $prev }
    return [pscustomobject]@{ Code = $code; Out = $out }
}

# A real zip with one non-concordia file: valid enough to extract, but with no concordia.exe.
$zipSrc = Join-Path $work 'zipsrc'
New-Item -ItemType Directory -Path $zipSrc | Out-Null
Set-Content -Path (Join-Path $zipSrc 'LICENSE') -Value 'synthetic test payload' -Encoding ascii
Add-Type -AssemblyName System.IO.Compression.FileSystem
$fakeZip = Join-Path $work 'fake.zip'
[IO.Compression.ZipFile]::CreateFromDirectory($zipSrc, $fakeZip)
$bodyB64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($fakeZip))
$releaseExe = Join-Path $PSScriptRoot '..\target\release\concordia.exe'
if (-not (Test-Path -LiteralPath $releaseExe)) { throw 'build the release executable before running installer fixtures' }
$happySrc = Join-Path $work 'happy-src'
New-Item -ItemType Directory -Path $happySrc | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $happySrc 'concordia.exe')
$happyZip = Join-Path $work 'happy.zip'
[IO.Compression.ZipFile]::CreateFromDirectory($happySrc, $happyZip)
$unsafeZip = Join-Path $work 'unsafe.zip'
$archive = [IO.Compression.ZipFile]::Open($unsafeZip, [IO.Compression.ZipArchiveMode]::Create)
try { $null = $archive.CreateEntry('../outside.exe') } finally { $archive.Dispose() }
$version = ((& $releaseExe --version | Select-Object -First 1) -split ' ')[1]

# ---- 1-4: offline, no feed needed. The API base points at a closed port, so any network call fails.
$dir1 = Join-Path $work 'dry1'
$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-Version', '0.0.1', '-ApiBase', 'http://127.0.0.1:9', '-Dir', $dir1)
Check ($r.Code -eq 0) "dry run with -Version exits 0 without network (exit $($r.Code))"
Check ($r.Out -match 'DRY RUN' -and $r.Out -match [regex]::Escape($assetZip) -and $r.Out -match 'releases/tags/v0\.0\.1') "dry run prints the plan and the tag URL"
Check (-not (Test-Path $dir1)) "dry run creates no install folder"

$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-ApiBase', 'http://127.0.0.1:9')
Check ($r.Code -eq 0 -and $r.Out -match 'releases\?per_page=30') "dry run without -Version prints the feed and exits 0 without network"

$r = Invoke-Installer @('-DryRun', '-ApiBase', 'http://127.0.0.1:9')
Check ($r.Code -eq 0 -and $r.Out -match 'dariodaddamio/concordia') "the official feed is configured by default"

$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-Version', '1.0; calc')
Check ($r.Code -ne 0 -and $r.Out -match '-Version must look like') "a hostile -Version is refused"

# ---- 5-7: a loopback fake feed in its own process.
$serverPs1 = Join-Path $work 'fake-feed.ps1'
@'
param([int]$Port, [string]$BodyB64, [string]$Zip, [string]$HappyPath, [string]$Version)
$body = [Convert]::FromBase64String($BodyB64)
$happy = [IO.File]::ReadAllBytes($HappyPath)
$happyHash = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($happy)) -replace '-', '').ToLower()
$unsafe = [IO.File]::ReadAllBytes((Join-Path (Split-Path $HappyPath) 'unsafe.zip'))
$unsafeHash = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($unsafe)) -replace '-', '').ToLower()
$hash = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($body)) -replace '-', '').ToLower()
$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()
$base = "http://127.0.0.1:$Port"
while ($true) {
    $ctx = $listener.GetContext()
    $path = $ctx.Request.Url.AbsolutePath
    [IO.File]::AppendAllText((Join-Path (Split-Path $HappyPath) 'requests.txt'), "$path`n")
    $bytes = $null
    if ($path -eq '/healthz') { $bytes = [Text.Encoding]::UTF8.GetBytes('ok') }
    elseif ($path -like '/repos/test-owner/*/releases*') {
        # Repo test-owner/bad publishes a wrong .sha256; every other repo publishes the right one.
        $bad = $path -like '/repos/test-owner/bad/*'
        $good = $path -like '/repos/test-owner/happy/*'
        $shaUrl = "$base/dl/zip.sha256"
        if ($bad) { $shaUrl = "$base/dl/bad/zip.sha256" }
        $zipUrl = "$base/dl/zip"
        if ($good) { $shaUrl = "$base/dl/happy.sha256"; $zipUrl = "$base/dl/happy" }
        if ($path -like '/repos/test-owner/unsafe/*') { $shaUrl = "$base/dl/unsafe.sha256"; $zipUrl = "$base/dl/unsafe" }
        $tag = "v$Version"
        if ($path -like '/repos/test-owner/wrong-version/*') { $tag = 'v99.99.99'; $shaUrl = "$base/dl/happy.sha256"; $zipUrl = "$base/dl/happy" }
        $rel = @{ tag_name = $tag; draft = $false; prerelease = $true; assets = @(
            @{ name = $Zip; browser_download_url = $zipUrl },
            @{ name = "$Zip.sha256"; browser_download_url = $shaUrl }) }
        $bytes = [Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject @($rel) -Depth 5))
    }
    elseif ($path -eq '/dl/zip') { $bytes = $body }
    elseif ($path -eq '/dl/happy') { $bytes = $happy }
    elseif ($path -eq '/dl/happy.sha256') { $bytes = [Text.Encoding]::UTF8.GetBytes("$happyHash  $Zip") }
    elseif ($path -eq '/dl/unsafe') { $bytes = $unsafe }
    elseif ($path -eq '/dl/unsafe.sha256') { $bytes = [Text.Encoding]::UTF8.GetBytes("$unsafeHash  $Zip") }
    elseif ($path -eq '/dl/zip.sha256') { $bytes = [Text.Encoding]::UTF8.GetBytes("$hash  $Zip") }
    elseif ($path -eq '/dl/bad/zip.sha256') { $bytes = [Text.Encoding]::UTF8.GetBytes(('0' * 64) + "  $Zip") }
    if ($null -eq $bytes) { $ctx.Response.StatusCode = 404; $bytes = [Text.Encoding]::UTF8.GetBytes('not found') }
    $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
    $ctx.Response.Close()
}
'@ | Set-Content -Path $serverPs1 -Encoding ascii

$port = Get-FreePort
$base = "http://127.0.0.1:$port"
$server = Start-Process -FilePath powershell.exe -PassThru -WindowStyle Hidden `
    -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $serverPs1, '-Port', "$port", '-BodyB64', $bodyB64, '-Zip', $assetZip, '-HappyPath', $happyZip, '-Version', $version) `
    -RedirectStandardOutput (Join-Path $work 'feed.out') -RedirectStandardError (Join-Path $work 'feed.err')

try {
    $ready = $false
    for ($i = 0; $i -lt 50 -and -not $ready; $i++) {
        try { $null = Invoke-WebRequest -UseBasicParsing -Uri "$base/healthz" -TimeoutSec 2; $ready = $true }
        catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $ready) { throw "fake feed did not start on port $port" }

    # 5. Wrong checksum: must fail closed, print the mismatch, and leave no install behind.
    $dir5 = Join-Path $work 'bad5'
    $r = Invoke-Installer @('-Repo', 'test-owner/bad', '-Version', '0.0.1', '-ApiBase', $base, '-Dir', $dir5)
    Check ($r.Code -ne 0 -and $r.Out -match 'checksum mismatch' -and $r.Out -match 'Nothing was installed') "wrong SHA-256 fails closed"
    Check (-not (Test-Path (Join-Path $dir5 'concordia.exe'))) "wrong SHA-256 installs nothing"
    Check ($r.Out -notmatch 'SHA-256 verified') "wrong SHA-256 never reports verified"

    # 6. Correct checksum: verification passes, then the fake zip (no concordia.exe) stops the run
    #    before `concordia.exe install` could ever run.
    $dir6 = Join-Path $work 'good6'
    $r = Invoke-Installer @('-Repo', 'test-owner/concordia', '-Version', '0.0.1', '-ApiBase', $base, '-Dir', $dir6)
    Check ($r.Out -match 'SHA-256 verified') "correct SHA-256 is verified"
    Check ($r.Code -ne 0 -and $r.Out -match 'no concordia.exe') "a zip without concordia.exe stops before install"
    Check (-not (Test-Path (Join-Path $dir6 'concordia.exe'))) "nothing is installed from the fake zip"

    $r = Invoke-Installer @('-Repo', 'test-owner/unsafe', '-ApiBase', $base, '-Dir', (Join-Path $work 'unsafe-install'), '-NoLaunch')
    Check ($r.Code -ne 0 -and $r.Out -match 'unexpected path') "verified archives with traversal paths are refused before extraction"
    $r = Invoke-Installer @('-Repo', 'test-owner/wrong-version', '-ApiBase', $base, '-Dir', (Join-Path $work 'version-install'), '-NoLaunch')
    Check ($r.Code -ne 0 -and $r.Out -match 'does not match release') "executable version mismatch is refused before installation"

    $dir7 = Join-Path $work 'happy7'
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $wrapper = Join-Path $work 'install-session.ps1'
    @'
param($Installer, $Api, $Dir)
$ErrorActionPreference = 'Stop'
& $Installer -Repo test-owner/happy -ApiBase $Api -Dir $Dir -NoLaunch
Write-Output ("SESSION_COMMAND=" + (Get-Command concordia.exe).Source)
'@ | Set-Content -LiteralPath $wrapper -Encoding ascii
    $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $wrapper -Installer $installer -Api $base -Dir $dir7 2>&1 | Out-String
    Check ($LASTEXITCODE -eq 0 -and $out -match 'Installed: concordia') "prebuilt install succeeds without a compiler or launch"
    Check ($out -match [regex]::Escape("SESSION_COMMAND=$dir7\concordia.exe")) "installed command is available in the same PowerShell session"
    Check ((Get-FileHash (Join-Path $dir7 'concordia.exe')).Hash -eq (Get-FileHash $releaseExe).Hash) "installed executable matches exact downloaded bytes"
    Check ([IO.File]::ReadAllText((Join-Path $dir7 'channel')).Trim() -eq 'release') "public installs select release updates even after old source installs"
    Check ([Environment]::GetEnvironmentVariable('Path', 'User') -eq $userPath) "isolated install leaves the real user PATH unchanged"

    $cacheRoot = Join-Path $env:LOCALAPPDATA 'concordia\cache\releases'
    $happyHash = (Get-FileHash -LiteralPath $happyZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $cachedZip = Join-Path $cacheRoot ($happyHash + '.zip')
    Check ((Test-Path -LiteralPath $cachedZip) -and (Test-Path -LiteralPath ($cachedZip + '.receipt.json'))) 'verified release is retained with an ownership receipt'
    $requestsPath = Join-Path $work 'requests.txt'
    $beforeZip = @([IO.File]::ReadAllLines($requestsPath) | Where-Object { $_ -eq '/dl/happy' }).Count
    $beforeSha = @([IO.File]::ReadAllLines($requestsPath) | Where-Object { $_ -eq '/dl/happy.sha256' }).Count
    $r = Invoke-Installer @('-Repo', 'test-owner/happy', '-ApiBase', $base, '-Dir', $dir7, '-NoLaunch')
    $after = [IO.File]::ReadAllLines($requestsPath)
    Check ($r.Code -eq 0 -and $r.Out -match 'Reusing SHA-256 verified cached' -and @($after | Where-Object { $_ -eq '/dl/happy' }).Count -eq $beforeZip) 'repeat installation rehashes and reuses cached bytes without downloading the zip'
    Check (@($after | Where-Object { $_ -eq '/dl/happy.sha256' }).Count -eq ($beforeSha + 1)) 'cache reuse still fetches the current release checksum'

    [IO.File]::WriteAllText($cachedZip, 'corrupt synthetic cache')
    $r = Invoke-Installer @('-Repo', 'test-owner/happy', '-ApiBase', $base, '-Dir', $dir7, '-NoLaunch')
    $after = [IO.File]::ReadAllLines($requestsPath)
    Check ($r.Code -eq 0 -and $r.Out -notmatch 'Reusing SHA-256 verified cached' -and @($after | Where-Object { $_ -eq '/dl/happy' }).Count -eq ($beforeZip + 1)) 'corrupt cache is refused and freshly verified download installs successfully'
    Check ([IO.File]::ReadAllText($cachedZip) -eq 'corrupt synthetic cache') 'corrupt cache files are never overwritten'
    [IO.File]::WriteAllBytes($cachedZip, [IO.File]::ReadAllBytes($happyZip))
    $receiptPath = $cachedZip + '.receipt.json'
    $receiptBytes = [IO.File]::ReadAllBytes($receiptPath)
    [IO.File]::WriteAllText($receiptPath, '{"owner":"someone-else"}')
    $r = Invoke-Installer @('-Repo', 'test-owner/happy', '-ApiBase', $base, '-Dir', $dir7, '-NoLaunch')
    Check ($r.Code -eq 0 -and $r.Out -notmatch 'Reusing SHA-256 verified cached' -and [IO.File]::ReadAllText($receiptPath) -eq '{"owner":"someone-else"}') 'unowned receipts cannot authorize reuse and are preserved'
    [IO.File]::WriteAllBytes($receiptPath, $receiptBytes)

    $oldOwned = @()
    for ($i = 0; $i -lt 5; $i++) {
        $payload = [Text.Encoding]::ASCII.GetBytes("owned-cache-$i")
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $digest = ([BitConverter]::ToString($sha.ComputeHash($payload)) -replace '-', '').ToLowerInvariant() } finally { $sha.Dispose() }
        $path = Join-Path $cacheRoot ($digest + '.zip')
        [IO.File]::WriteAllBytes($path, $payload)
        @{ owner = 'concordia-release-cache-v1'; sha256 = $digest; bytes = $payload.Length } | ConvertTo-Json -Compress | Set-Content -LiteralPath ($path + '.receipt.json') -Encoding ascii
        [IO.File]::SetLastWriteTimeUtc($path, [DateTime]::UtcNow.AddDays(-10).AddMinutes($i))
        $oldOwned += $path
    }
    $unknown = Join-Path $cacheRoot 'unknown-user-file.zip'
    [IO.File]::WriteAllText($unknown, 'preserve me')
    $r = Invoke-Installer @('-Repo', 'test-owner/happy', '-ApiBase', $base, '-Dir', $dir7, '-NoLaunch')
    $remaining = @($oldOwned | Where-Object { Test-Path -LiteralPath $_ })
    Check ($r.Code -eq 0 -and $remaining.Count -eq 1 -and $remaining[0] -eq $oldOwned[4] -and (Test-Path -LiteralPath $cachedZip)) 'bounded cleanup keeps the selected release and latest verified rollback'
    Check ((Test-Path -LiteralPath $unknown) -and @($oldOwned[0..3] | Where-Object { Test-Path -LiteralPath ($_ + '.receipt.json') }).Count -eq 0) 'cleanup removes only obsolete owned artifact pairs and preserves unknown files'
}
finally {
    $env:LOCALAPPDATA = $savedLocalAppData
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if ([IO.Path]::GetFullPath($work).StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($failures.Count -gt 0) {
    Write-Host ("`n{0} failure(s)" -f $failures.Count)
    exit 1
}
Write-Host "`ntest-install PASS"
exit 0
