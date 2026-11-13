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

# Exact public product identifiers are intentional metadata, not personal filesystem data.
# Scope exceptions to the files that need each identifier; all remaining text is still scanned.
$publicIdentifiers = @{
    'README.md' = @('dariodaddamio/concordia')
    'crates/concordia/Cargo.toml' = @('dariodaddamio/concordia')
    'crates/concordia/src/release.rs' = @('dariodaddamio/concordia')
    'docs/install.md' = @('dariodaddamio/concordia')
    'crates/concordia/src/harness/adapter.rs' = @('@dariodaddamio/harness')
    'scripts/install.ps1' = @('dariodaddamio/concordia')
    'scripts/test-install.ps1' = @('dariodaddamio/concordia')
    'scripts/verify-oss-gates.ps1' = @('dariodaddamio/concordia', '@dariodaddamio/harness')
}

# Owner-requested public theme name. Permit this exact brand token only in its implementing
# files/docs; personal paths and every other private-project token remain checked normally.
$publicThemeFiles = @(
    'CHANGELOG.md', 'PLAN.md', 'crates/concordia/src/config.rs',
    'crates/concordia/src/tui/appearance.rs', 'crates/concordia/src/tui/brand.rs',
    'crates/concordia/src/tui/theme.rs', 'crates/concordia/src/tui/tests.rs',
    'docs/config.md', 'docs/archive/brand-direction.md', 'docs/chat-rendering.md',
    'docs/e2e-status.md', 'docs/architecture.md', 'crates/concordia/src/theme_library.rs',
    ('crates/concordia/src/themes/' + ('op' + 'ifex') + '.toml'), 'crates/concordia/src/tui/raster.rs',
    'crates/concordia/src/tui/tests/draw.rs', 'crates/concordia/src/tui/tests/input_dispatch.rs',
    'crates/concordia/src/tui/ux_tests.rs'
)
$themeToken = ('op' + 'ifex')
$binaryExt = '\.(png|jpg|jpeg|gif|ico|mesh|exe|dll|zip|gz|7z|pdf|woff2?)$'
$tracked = git ls-files --cached --others --exclude-standard | Sort-Object -Unique | Where-Object { $_ -notmatch $binaryExt -and $_ -ne "Cargo.lock" }
if ($LASTEXITCODE -ne 0) { throw 'Could not enumerate distributable candidates; privacy coverage is unknown.' }
$privateHits = New-Object System.Collections.Generic.List[string]
$personalHits = New-Object System.Collections.Generic.List[string]
foreach ($rel in $tracked) {
    $p = Join-Path $root $rel
    if (-not (Test-Path $p -PathType Leaf)) { continue }
    $entry = Get-Item -LiteralPath $p -Force -ErrorAction Stop
    if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        [void]$failures.Add("linked distributable candidate was not read: $rel")
        continue
    }
    $t = Get-Content $p -Raw -ErrorAction SilentlyContinue
    if ($null -eq $t) { continue }
    $privateText = if ($publicThemeFiles -contains $rel) { $t -replace ('(?i)\b' + $themeToken + '\b'), 'theme' } else { $t }
    if ($privateText -match $privateRegex) { [void]$privateHits.Add($rel) }
    $personalText = $t
    foreach ($identifier in $publicIdentifiers[$rel]) {
        $pattern = '(?<![\w@.-])' + [regex]::Escape($identifier) + '(?![\w.-])'
        $personalText = [regex]::Replace($personalText, $pattern, '')
    }
    if ($personalRegex -and ($personalText -match $personalRegex)) { [void]$personalHits.Add($rel) }
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
Write-Host ("scanned {0} tracked and unignored text files" -f @($tracked).Count)
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
