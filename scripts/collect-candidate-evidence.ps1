# Record exact local candidate evidence. No build, install, provider turn, configuration or upload.
# Receipts are caller-supplied check results, bound to a source commit or exact executable hash.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-fA-F0-9]{40}$')][string]$BuildCommit,
    [string]$ChecksPath = ''
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$target = [IO.Path]::GetFullPath((Join-Path $root 'target'))
function Assert-Regular([string]$Path, [long]$Maximum) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt $Maximum) {
        throw 'Evidence input must be a bounded regular file.'
    }
}
function Target-File([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    # A worktree target can be the repository-owned shared-build junction. Inputs must remain
    # lexically beneath that named target, with no parent traversals or linked input file.
    if (-not $full.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Check receipts and logs must be under this checkout target directory.'
    }
    return $full
}
function Read-Version([string]$Program) {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $Program
    $start.Arguments = '--version'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Version reader did not start.' }
        # Only the hash-selected product executable and the installed compiler receive this
        # fixed read-only argument. Neither command reads native conversation bodies.
        $output = $process.StandardOutput.ReadToEndAsync()
        $errors = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'Version reader exceeded ten seconds.' }
        $text = $output.Result
        $errorText = $errors.Result
        if ($process.ExitCode -ne 0 -or $text.Length -gt 65536 -or $errorText.Length -gt 65536) {
            throw 'Version reader failed or exceeded its output bound.'
        }
        return (($text -split '\r?\n')[0]).Trim()
    } finally { $process.Dispose() }
}
$exePath = (Resolve-Path -LiteralPath $Exe).Path
# Match the measured artifact budget in smoke-ci.ps1 and docs/release-blockers.md.
$binaryBudgetBytes = 25260212
Assert-Regular $exePath $binaryBudgetBytes
$before = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($before -ne $ExpectedSha256.ToLowerInvariant()) { throw 'Candidate SHA-256 mismatch; no executable was started.' }
$head = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve checkout commit.' }
& git -C $root merge-base --is-ancestor $BuildCommit $head
if ($LASTEXITCODE -ne 0) { throw 'Build commit is not in this checkout history.' }
& git -C $root diff --quiet $BuildCommit -- Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates vendor
if ($LASTEXITCODE -ne 0) { throw 'Application source differs from the declared build commit.' }
$stampSource = & git -C $root show ($BuildCommit + ':crates/concordia/src/version.rs')
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve build identity source.' }
$stamp = [regex]::Match(($stampSource -join "`n"), 'BUILD_STAMP:\s*&str\s*=\s*"([^"]+)"').Groups[1].Value
$version = Read-Version $exePath
if (-not $stamp -or $version -notmatch ('^concordia \d+\.\d+\.\d+ \(' + [regex]::Escape($stamp) + '\)$')) {
    throw 'Executable version does not match the declared build source.'
}
$compiler = (Get-Command rustc.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$compilerVersion = Read-Version $compiler
$checks = @()
if ($ChecksPath) {
    $receiptPath = Target-File $ChecksPath
    Assert-Regular $receiptPath 65536
    $receipts = Get-Content -LiteralPath $receiptPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if (@($receipts).Count -gt 32) { throw 'At most 32 check receipts are accepted.' }
    foreach ($check in $receipts) {
        if ($check.name -notmatch '^[A-Za-z0-9 _()./-]{1,80}$' -or $check.scope -notin @('source', 'binary') -or
            ($check.exit_code -isnot [int] -and $check.exit_code -isnot [long]) -or $check.exit_code -lt 0 -or $check.exit_code -gt 255) {
            throw 'Invalid check receipt. Expected name, source/binary scope and integer exit_code.'
        }
        if (($check.scope -eq 'source' -and $check.build_commit -ne $BuildCommit) -or
            ($check.scope -eq 'binary' -and $check.artifact_sha256 -ne $before)) {
            throw 'Check receipt belongs to another source or candidate.'
        }
        $log = Target-File $check.log
        Assert-Regular $log 16777216
        $checks += [ordered]@{
            name = $check.name; scope = $check.scope; exit_code = $check.exit_code
            log = [IO.Path]::GetFileName($log)
            log_sha256 = (Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()
            result_origin = 'caller-supplied exit code; immutable log fingerprint'
        }
    }
}
Assert-Regular $exePath $binaryBudgetBytes
if ((Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $before) { throw 'Candidate changed during collection.' }
& git -C $root diff --quiet $BuildCommit -- Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates vendor
if ($LASTEXITCODE -ne 0 -or (& git -C $root rev-parse HEAD).Trim() -ne $head) { throw 'Source changed during collection.' }
$dirty = @(& git -C $root status --porcelain --untracked-files=no)
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect checkout state.' }
$record = [ordered]@{
    version = 1; collected_utc = [DateTime]::UtcNow.ToString('o')
    qualification = 'engineering evidence; not release qualification or a reproducible-build attestation'
    build_commit = $BuildCommit; checkout_commit = $head; tracked_changes = $dirty
    tracked_application_source_matches_build_commit = $true
    artifact = [ordered]@{ sha256 = $before; bytes = (Get-Item -LiteralPath $exePath).Length; version = $version }
    environment = [ordered]@{
        compiler = $compilerVersion; powershell = $PSVersionTable.PSVersion.ToString()
        os = [Environment]::OSVersion.VersionString
        terminal_hint = ([string]$env:TERM_PROGRAM).Substring(0, [Math]::Min(256, ([string]$env:TERM_PROGRAM).Length))
    }
    checks = $checks
    native_provider_versions = 'not collected by this read-only collector'
    live_provider_parity = 'not established'; physical_host_parity = 'not established'
    publication_authorized = $false
}
$dir = Join-Path $target 'candidate-evidence'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
if ((Get-Item -LiteralPath $dir -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Evidence output directory must not be linked.' }
$name = ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '-' + $before.Substring(0, 12) + '-' + [guid]::NewGuid().ToString('N') + '.json')
$destination = Join-Path $dir $name
$bytes = [Text.UTF8Encoding]::new($false).GetBytes(($record | ConvertTo-Json -Depth 8))
$stream = [IO.File]::Open($destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush() } finally { $stream.Dispose() }
Write-Host "Candidate evidence: $destination"
if (@($checks | Where-Object { $_.exit_code -ne 0 }).Count -gt 0) { exit 1 }
