# Everyday MVP A smoke - no provider model spend.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$failures = @()
$stampSource = Get-Content (Join-Path $root "crates\concordia\src\version.rs") -Raw
$expectedStamp = [regex]::Match($stampSource, 'BUILD_STAMP:\s*&str\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $expectedStamp) { throw "BUILD_STAMP missing from version.rs" }

function Fail([string]$msg) { $script:failures += $msg; Write-Host "FAIL $msg" -ForegroundColor Red }
function Ok([string]$msg) { Write-Host "OK   $msg" -ForegroundColor Green }

# Windows PowerShell can turn normal native stderr (Cargo's progress diagnostics) into
# terminating errors when the caller captures the smoke output. Use the process exit code.
function Invoke-Cargo([string[]]$CargoArgs) {
    $ErrorActionPreference = "Continue"
    & cargo @CargoArgs
    $script:cargoExit = $LASTEXITCODE
}

$binDir = Join-Path $env:LOCALAPPDATA "concordia\bin"
$currentFile = Join-Path $binDir "current"
if (-not (Test-Path $currentFile)) { Fail "missing $currentFile - run scripts\install-dev.ps1" }
else {
    $active = (Get-Content $currentFile -Raw).Trim()
    if (-not (Test-Path $active)) { Fail "current pointer dead: $active" }
    else {
        Ok "current -> $active"
        $ver = & $active --version 2>&1 | Out-String
        if ($ver -notmatch [regex]::Escape($expectedStamp)) { Fail "expected build $expectedStamp in: $ver" }
        else { Ok "version: $($ver.Trim().Split("`n")[0])" }
        $help = & $active --help 2>&1 | Out-String
        foreach ($needle in @('/update', '/version', '/usage', '/copy', '/use [add]', 'Connect:', 'concordia doctor', '/doctor')) {
            if ($help -notmatch [regex]::Escape($needle)) { Fail "help missing $needle" }
            else { Ok "help has $needle" }
        }
    }
}

Invoke-Cargo @('test', '-p', 'concordia', '--tests', '--quiet')
if ($script:cargoExit -ne 0) { Fail "cargo test -p concordia" } else { Ok "cargo test -p concordia" }

# Native usage parsing is covered by the existing control-plane and library checks above.
# Keep this smoke read-only with respect to provider homes and checked-out test sources.

if ($failures.Count -gt 0) {
    Write-Host "`n$($failures.Count) failure(s)" -ForegroundColor Red
    exit 1
}
Write-Host "`nsmoke-e2e-everyday PASS" -ForegroundColor Green
exit 0
