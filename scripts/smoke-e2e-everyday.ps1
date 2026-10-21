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

cargo test -p concordia --tests --quiet
if ($LASTEXITCODE -ne 0) { Fail "cargo test -p concordia" } else { Ok "cargo test -p concordia" }

$exe = Join-Path $root "target\release\concordia.exe"
if (-not (Test-Path $exe)) {
    cargo build -p concordia --release --quiet
}
# usage reader against real home if present (read-only)
$codex = Join-Path $env:USERPROFILE ".codex"
if (Test-Path (Join-Path $codex "sessions")) {
    $tmpTest = @"
#[test]
fn smoke_live_usage() {
    let home = dirs::home_dir().unwrap().join(".codex");
    let snap = concordia::usage::read_codex_usage(&home);
    assert!(!snap.summary.is_empty());
    eprintln!("usage={}", snap.summary);
}
"@
    $path = Join-Path $root "crates\concordia\tests\_smoke_usage.rs"
    Set-Content $path $tmpTest -Encoding utf8
    cargo test -p concordia --test _smoke_usage -- --nocapture
    $code = $LASTEXITCODE
    Remove-Item -Force $path -EA SilentlyContinue
    if ($code -ne 0) { Fail "live usage read" } else { Ok "live usage read" }
} else {
    Ok "skip live usage (no ~/.codex/sessions)"
}

if ($failures.Count -gt 0) {
    Write-Host "`n$($failures.Count) failure(s)" -ForegroundColor Red
    exit 1
}
Write-Host "`nsmoke-e2e-everyday PASS" -ForegroundColor Green
exit 0
