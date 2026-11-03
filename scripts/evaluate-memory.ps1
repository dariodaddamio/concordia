# Synthetic retrieval evaluation through existing native-memory checks. No provider/model calls.
param([string]$Output = "")
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not $Output) {
    $Output = Join-Path $root ('target/memory-evaluation-' + [guid]::NewGuid().ToString('N') + '.json')
}
$artifactPath = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $artifactPath) { throw 'Evaluation output already exists; choose a new path.' }
$buildLog = Join-Path $root 'target/memory-evaluation-build.jsonl'
$buildErrors = Join-Path $root 'target/memory-evaluation-build.stderr.log'
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
& cargo test -p concordia --lib --locked --no-run --message-format=json 1> $buildLog 2> $buildErrors
$buildExit = $LASTEXITCODE
$ErrorActionPreference = $previousPreference
if ($buildExit -ne 0) { throw "Evaluation build failed; see $buildErrors" }
$testExe = Get-Content -LiteralPath $buildLog | ForEach-Object {
    $record = $_ | ConvertFrom-Json
    if ($record.reason -eq 'compiler-artifact' -and $record.target.name -eq 'concordia' -and $record.profile.test -and $record.executable) { $record.executable }
} | Select-Object -Last 1
if (-not $testExe) { throw 'Cargo did not identify the native library test executable.' }
$logPrefix = Join-Path $root ('target/memory-evaluation-run-' + [guid]::NewGuid().ToString('N'))
$run = Start-Process -FilePath $testExe -ArgumentList @('native_memory::tests', '--nocapture', '--test-threads=1') -WindowStyle Hidden -PassThru -RedirectStandardOutput "$logPrefix.stdout.log" -RedirectStandardError "$logPrefix.stderr.log"
# Retain the handle before polling so Windows PowerShell preserves the completed exit code.
$null = $run.Handle
$peakWorkingSet = 0L
$timer = [Diagnostics.Stopwatch]::StartNew()
while (-not $run.HasExited) {
    $run.Refresh()
    $peakWorkingSet = [Math]::Max($peakWorkingSet, $run.PeakWorkingSet64)
    if ($timer.Elapsed.TotalMinutes -gt 5) {
        $run.Kill()
        $run.WaitForExit()
        throw 'Synthetic memory evaluation exceeded five minutes.'
    }
    Start-Sleep -Milliseconds 50
}
$run.WaitForExit()
$timer.Stop()
$run.Refresh()
$peakWorkingSet = [Math]::Max($peakWorkingSet, $run.PeakWorkingSet64)
if ($run.ExitCode -ne 0) { throw "Existing native-memory suite failed; see $logPrefix.stdout.log" }
$rows = @(Get-Content -LiteralPath "$logPrefix.stdout.log" -Encoding UTF8 | ForEach-Object {
    if ($_ -match 'MEMORY_EVALUATION (\{.*\})') { $matches[1] | ConvertFrom-Json }
})
if ($rows.Count -ne 16) { throw 'Expected all 16 provider/corpus rows; output is incomplete.' }
$corpusPath = Join-Path $root 'crates/concordia/tests/fixtures/memory-evaluation-v1.json'
$corpus = Get-Content -LiteralPath $corpusPath -Raw -Encoding UTF8 | ConvertFrom-Json
$suiteOutput = Get-Content -LiteralPath "$logPrefix.stdout.log" -Raw -Encoding UTF8
foreach ($contract in $corpus.integrity_contracts) {
    if ($suiteOutput -notmatch ('test native_memory::tests::' + [regex]::Escape($contract.fixture) + ' \.\.\.')) {
        throw "Missing integrity fixture: $($contract.fixture)"
    }
}
$corpusHash = (Get-FileHash -LiteralPath $corpusPath -Algorithm SHA256).Hash.ToLowerInvariant()
if (@($rows | Where-Object { $_.corpus_sha256 -ne $corpusHash }).Count) { throw 'Corpus changed during evaluation.' }
$sourceCommit = (& git rev-parse HEAD | Out-String).Trim()
$sourceDirty = -not [string]::IsNullOrWhiteSpace((& git status --porcelain | Out-String))
$report = [ordered]@{
    version = 1
    source_commit = $sourceCommit
    source_dirty = $sourceDirty
    corpus_sha256 = $corpusHash
    rustc = (& rustc --version | Out-String).Trim()
    test_executable_sha256 = (Get-FileHash -LiteralPath $testExe -Algorithm SHA256).Hash.ToLowerInvariant()
    elapsed_ms = $timer.ElapsedMilliseconds
    process_peak_working_set_bytes = $peakWorkingSet
    peak_retained_serialized_reports_bytes = ($rows | Measure-Object -Property retained_serialized_report_bytes -Maximum).Maximum
    integrity_contracts = $corpus.integrity_contracts
    integrity_status = 'Existing native-memory suite passed, including cross-account, rewritten/deleted-source and bounded-history assertions.'
    limits = 'Synthetic native-log fixtures only. Read counters cover successful native read_at bytes during recall, including repeated verification, excluding metadata/index construction/discovery. Serialized metadata omits allocator overhead. Process working set includes the entire native-memory suite and is not a per-query heap peak. No model retention/task-success evidence.'
    rows = $rows
}
$parent = Split-Path -Parent $artifactPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$json = $report | ConvertTo-Json -Depth 15
[IO.File]::WriteAllText($artifactPath, $json, [Text.UTF8Encoding]::new($false))
Write-Host "Memory evaluation PASS: $($rows.Count) rows; $artifactPath"
