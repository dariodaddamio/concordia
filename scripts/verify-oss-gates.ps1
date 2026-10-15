# Ship-blocking OSS gates. Exit 0 only when every required gate passes.
# Scans every tracked text file, so nothing private or personal can ride along in docs or tests.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$failures = New-Object System.Collections.Generic.List[string]
$soft = New-Object System.Collections.Generic.List[string]

function Add-Need([string]$RelPath, [string]$Why) {
    if (-not (Test-Path (Join-Path $root $RelPath))) {
        [void]$failures.Add("MISSING $RelPath -- $Why")
    }
}

Add-Need "LICENSE" "MIT license at the root"
Add-Need "NOTICE" "third-party notice"
Add-Need "SECURITY.md" "vulnerability contact"
Add-Need "CHANGELOG.md" "user-facing change history"
Add-Need "docs/provider-terms.md" "what the providers' terms say about concordia"
Add-Need ".github/workflows" "CI"
Add-Need "rust-toolchain.toml" "pinned toolchain"

if (-not (Test-Path (Join-Path $root ".git"))) {
    [void]$failures.Add("MISSING .git -- not a git repository")
}

$crateToml = Get-Content (Join-Path $root "crates\concordia\Cargo.toml") -Raw
if ($crateToml -notmatch 'license\s*=\s*"MIT"') {
    [void]$failures.Add("crates/concordia license must be MIT to match LICENSE")
}
$lic = Get-Content (Join-Path $root "LICENSE") -Raw -ErrorAction SilentlyContinue
if ($lic -and ($lic -notmatch 'MIT License')) {
    [void]$failures.Add("LICENSE must be the MIT License text")
}
$gi = Get-Content (Join-Path $root ".gitignore") -Raw
if ($gi -notmatch '(?m)^target/') {
    [void]$failures.Add(".gitignore missing target/ -- release archives must not ship the build tree")
}

# Product surface is Rust only: no leftover JavaScript/TypeScript project.
foreach ($leftover in @("package.json", "tsconfig.json", "src")) {
    if (Test-Path (Join-Path $root $leftover)) {
        [void]$failures.Add("leftover non-Rust project file: $leftover")
    }
}

# --- content scan over every tracked text file ---------------------------------------------
# Needles are assembled from pieces so this script does not match its own deny-list, and the
# user's name is read from the environment so it never has to be written down anywhere.
$privateWords = @(('op' + 'ifex'), ('no' + 'ra'), ('bra' + 'in'))
$privateRegex = '(?i)\b(' + ($privateWords -join '|') + ')\b'
$personal = @($env:USERNAME, (Split-Path -Leaf $env:USERPROFILE)) |
    Where-Object { $_ -and $_.Length -ge 3 } | Select-Object -Unique
$personalRegex = if ($personal.Count -gt 0) { '(?i)(' + (($personal | ForEach-Object { [regex]::Escape($_) }) -join '|') + ')' } else { $null }

$binaryExt = '\.(png|jpg|jpeg|gif|ico|exe|dll|zip|gz|7z|pdf|woff2?)$'
$tracked = git ls-files | Where-Object { $_ -notmatch $binaryExt -and $_ -ne "Cargo.lock" }
$privateHits = New-Object System.Collections.Generic.List[string]
$personalHits = New-Object System.Collections.Generic.List[string]
foreach ($rel in $tracked) {
    $p = Join-Path $root $rel
    if (-not (Test-Path $p -PathType Leaf)) { continue }
    $t = Get-Content $p -Raw -ErrorAction SilentlyContinue
    if ($null -eq $t) { continue }
    if ($t -match $privateRegex) { [void]$privateHits.Add($rel) }
    if ($personalRegex -and ($t -match $personalRegex)) { [void]$personalHits.Add($rel) }
}
if ($privateHits.Count -gt 0) {
    [void]$failures.Add("mentions of private projects in: " + ($privateHits -join ", "))
}
if ($personalHits.Count -gt 0) {
    [void]$failures.Add("this machine's username appears in: " + ($personalHits -join ", "))
}

if (-not (Test-Path (Join-Path $root "CONTRIBUTING.md"))) {
    [void]$soft.Add("CONTRIBUTING.md")
}
if (-not (Test-Path (Join-Path $root "deny.toml"))) {
    [void]$soft.Add("deny.toml (cargo-deny hygiene)")
}

Write-Host "=== OSS gate report ==="
Write-Host ("scanned {0} tracked text files" -f @($tracked).Count)
if ($failures.Count -eq 0) {
    Write-Host "REQUIRED: PASS"
} else {
    Write-Host "REQUIRED: FAIL"
    foreach ($f in $failures) { Write-Host "  - $f" }
}
if ($soft.Count -gt 0) {
    Write-Host "SOFT:"
    foreach ($s in $soft) { Write-Host "  - $s" }
}
if ($failures.Count -gt 0) { exit 1 }
exit 0
