# Offline tests for scripts/install.ps1. Hermetic: no internet, no real install, no PATH change.
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
Set-Content -Path (Join-Path $zipSrc 'readme.txt') -Value 'synthetic test payload' -Encoding ascii
Add-Type -AssemblyName System.IO.Compression.FileSystem
$fakeZip = Join-Path $work 'fake.zip'
[IO.Compression.ZipFile]::CreateFromDirectory($zipSrc, $fakeZip)
$bodyB64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($fakeZip))

# ---- 1-4: offline, no feed needed. The API base points at a closed port, so any network call fails.
$dir1 = Join-Path $work 'dry1'
$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-Version', '0.0.1', '-ApiBase', 'http://127.0.0.1:9', '-Dir', $dir1)
Check ($r.Code -eq 0) "dry run with -Version exits 0 without network (exit $($r.Code))"
Check ($r.Out -match 'DRY RUN' -and $r.Out -match [regex]::Escape($assetZip) -and $r.Out -match 'releases/tags/v0\.0\.1') "dry run prints the plan and the tag URL"
Check (-not (Test-Path $dir1)) "dry run creates no install folder"

$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-ApiBase', 'http://127.0.0.1:9')
Check ($r.Code -eq 0 -and $r.Out -match 'releases\?per_page=30') "dry run without -Version prints the feed and exits 0 without network"

$r = Invoke-Installer @('-Repo', 'REPLACE-OWNER/concordia', '-ApiBase', 'http://127.0.0.1:9')
Check ($r.Code -ne 0 -and $r.Out -match 'placeholder') "placeholder repo is refused for a real install"

$r = Invoke-Installer @('-DryRun', '-Repo', 'test-owner/concordia', '-Version', '1.0; calc')
Check ($r.Code -ne 0 -and $r.Out -match '-Version must look like') "a hostile -Version is refused"

# ---- 5-7: a loopback fake feed in its own process.
$serverPs1 = Join-Path $work 'fake-feed.ps1'
@'
param([int]$Port, [string]$BodyB64, [string]$Zip)
$body = [Convert]::FromBase64String($BodyB64)
$hash = ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($body)) -replace '-', '').ToLower()
$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()
$base = "http://127.0.0.1:$Port"
while ($true) {
    $ctx = $listener.GetContext()
    $path = $ctx.Request.Url.AbsolutePath
    $bytes = $null
    if ($path -eq '/healthz') { $bytes = [Text.Encoding]::UTF8.GetBytes('ok') }
    elseif ($path -like '/repos/test-owner/*/releases*') {
        # Repo test-owner/bad publishes a wrong .sha256; every other repo publishes the right one.
        $bad = $path -like '/repos/test-owner/bad/*'
        $shaUrl = "$base/dl/zip.sha256"
        if ($bad) { $shaUrl = "$base/dl/bad/zip.sha256" }
        $rel = @{ tag_name = 'v0.0.1'; draft = $false; prerelease = $true; assets = @(
            @{ name = $Zip; browser_download_url = "$base/dl/zip" },
            @{ name = "$Zip.sha256"; browser_download_url = $shaUrl }) }
        $bytes = [Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject @($rel) -Depth 5))
    }
    elseif ($path -eq '/dl/zip') { $bytes = $body }
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
    -ArgumentList @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $serverPs1, '-Port', "$port", '-BodyB64', $bodyB64, '-Zip', $assetZip) `
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
}
finally {
    if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    Write-Host ("`n{0} failure(s)" -f $failures.Count)
    exit 1
}
Write-Host "`ntest-install PASS"
exit 0
