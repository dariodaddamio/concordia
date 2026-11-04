# CI smoke for the release binary. No provider CLIs, no network, no install pointer needed.
# Usage: pwsh scripts/smoke-ci.ps1 [-Exe path\to\concordia.exe]
param([string]$Exe = "")
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $root "target\release\concordia.exe" }

$failures = @()
function Ok([string]$m) { Write-Host "OK   $m" }
function Fail([string]$m) { $script:failures += $m; Write-Host "FAIL $m" }

if (-not (Test-Path $Exe)) { Write-Host "FAIL missing $Exe"; exit 1 }
Ok "binary: $Exe"

# 1. Reviewed distribution budget: native Mermaid, syntax assets, image support and memory
# orchestration increased the release from the original thin UI. See docs/release-blockers.md.
# Compare exact bytes so rounding cannot let an oversized artifact through.
$sizeBytes = (Get-Item $Exe).Length
$mb = [math]::Round($sizeBytes / 1MB, 2)
if ($sizeBytes -gt 23MB) { Fail "binary is $mb MiB (budget 23 MiB)" } else { Ok "size $mb MiB (budget 23 MiB)" }

# 2. --version: stamp format, and read-only (must not create or change the install pointer).
$pointer = Join-Path $env:LOCALAPPDATA "concordia\bin\current"
$before = if (Test-Path $pointer) { (Get-Item $pointer).LastWriteTimeUtc.Ticks } else { $null }
$ver = (& $Exe --version | Select-Object -First 1)
if ($ver -notmatch '^concordia \d+\.\d+\.\d+ \(\d{4}-\d{2}-\d{2}[a-z]?\)$') { Fail "bad version line: $ver" } else { Ok $ver }
$after = if (Test-Path $pointer) { (Get-Item $pointer).LastWriteTimeUtc.Ticks } else { $null }
if ($before -ne $after) { Fail "--version changed the install pointer" } else { Ok "--version is read-only" }

# 3. --help lists the product surface.
$help = & $Exe --help | Out-String
foreach ($needle in @('/use [add]', '/copy', '/doctor', 'concordia doctor', 'concordia wezterm', 'Connect:', 'Mouse:', 'resume --last')) {
    if ($help -notmatch [regex]::Escape($needle)) { Fail "help missing: $needle" } else { Ok "help has $needle" }
}

# 4. doctor runs against an empty state dir and is read-only. On a bare runner it is allowed to
#    report problems (no provider CLIs), so only the report shape and exit-code contract are checked.
$state = Join-Path ([System.IO.Path]::GetTempPath()) ("concordia-ci-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $state | Out-Null
$env:CONCORDIA_STATE_DIR = $state
$report = & $Exe doctor | Out-String
$code = $LASTEXITCODE
Remove-Item Env:\CONCORDIA_STATE_DIR
if ($report -notmatch '^concordia doctor') { Fail "doctor report malformed" } else { Ok "doctor prints a report" }
$hasFail = $report -match '(?m)^FAIL'
if (($hasFail -and $code -ne 1) -or ((-not $hasFail) -and $code -ne 0)) { Fail "doctor exit code $code does not match its findings" } else { Ok "doctor exit code ($code) matches findings" }
if (@(Get-ChildItem $state -Force).Count -ne 0) { Fail "doctor left files in the state dir" } else { Ok "doctor left no files behind" }
Remove-Item -Recurse -Force $state

# 5. Unknown arguments fail with exit code 2.
# (Windows PowerShell 5.1 throws on native stderr under -Stop, so relax it for this one call.)
$prev = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& $Exe --definitely-not-a-flag 2>&1 | Out-Null
$badArgCode = $LASTEXITCODE
$ErrorActionPreference = $prev
if ($badArgCode -ne 2) { Fail "unknown arg exit code $badArgCode (want 2)" } else { Ok "unknown arg exits 2" }

if ($failures.Count -gt 0) {
    Write-Host ("`n{0} failure(s)" -f $failures.Count)
    exit 1
}
Write-Host "`nsmoke-ci PASS"
exit 0
