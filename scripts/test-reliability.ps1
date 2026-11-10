# Repeatable, bounded, provider-free reliability wall. Run from Windows PowerShell.
param([ValidateRange(1, 10)][int]$JourneyPasses = 3)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$evidence = Join-Path $root ('target/reliability-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ') + '-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$results = New-Object 'System.Collections.Generic.List[object]'

function Invoke-Check([string]$Name, [string]$Program, [string[]]$Arguments, [int]$TimeoutSeconds = 600, [string]$JourneyExe = '', [switch]$KeepTesting) {
    # All arguments below are fixed by this script, contain no spaces/quotes, and
    # go directly to an executable. No shell or prompt text is evaluated.
    if ($Arguments | Where-Object { $_ -match '[\s"]' }) { throw "unsupported check argument" }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = (Get-Command $Program -CommandType Application | Select-Object -First 1).Source
    $info.Arguments = $Arguments -join ' '
    $info.WorkingDirectory = $root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    if ($JourneyExe) { $info.EnvironmentVariables['CONCORDIA_JOURNEY_EXE'] = $JourneyExe }
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Host "RUN $Name"
    try {
        [void]$process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
        if ($timedOut) {
            # This PID belongs to this check; only its tracked process tree is stopped.
            & taskkill.exe /PID $process.Id /T /F 2>&1 | Out-Null
            $process.WaitForExit()
        }
        $text = $stdout.GetAwaiter().GetResult() + "`n" + $stderr.GetAwaiter().GetResult()
        $log = Join-Path $evidence "$Name.log"
        [System.IO.File]::WriteAllText($log, $text)
        $code = if ($timedOut) { 124 } else { $process.ExitCode }
        $row = [pscustomobject]@{ check = $Name; exitCode = $code; seconds = [math]::Round($watch.Elapsed.TotalSeconds, 2); log = "$Name.log" }
        $results.Add($row)
        [System.IO.File]::WriteAllText((Join-Path $evidence 'summary.json'), ($results.ToArray() | ConvertTo-Json -Depth 4))
        Write-Host "$Name exit=$code seconds=$($row.seconds)"
        if ($code -ne 0) {
            if (-not $KeepTesting) { throw "$Name failed; see $log" }
            Write-Host "Unresolved gate: $Name. Continuing independent local tests; final status remains failed."
        }
    } finally { $process.Dispose() }
}

Invoke-Check 'format' 'cargo.exe' @('fmt', '--all', '--check')
Invoke-Check 'clippy' 'cargo.exe' @('clippy', '-p', 'concordia', '--all-targets', '--locked', '--', '-D', 'warnings')
Invoke-Check 'all-tests' 'cargo.exe' @('test', '-p', 'concordia', '--tests', '--locked', '--quiet')
Invoke-Check 'audit' 'cargo.exe' @('audit', '--deny', 'warnings')
Invoke-Check 'dependency-policy' 'cargo.exe' @('deny', 'check')
Invoke-Check 'oss-gates' 'powershell.exe' @('-NoProfile', '-File', 'scripts/verify-oss-gates.ps1') -KeepTesting
Invoke-Check 'release-build' 'cargo.exe' @('build', '-p', 'concordia', '--release', '--locked')
Invoke-Check 'release-smoke' 'powershell.exe' @('-NoProfile', '-File', 'scripts/smoke-ci.ps1')
Invoke-Check 'input-latency' 'cargo.exe' @('test', '-p', 'concordia', '--release', '--lib', 'tui::performance', '--locked', '--', '--ignored', '--nocapture')
Invoke-Check 'draw-latency' 'cargo.exe' @('test', '-p', 'concordia', '--release', '--lib', 'bench_draw_ui', '--locked', '--', '--ignored', '--nocapture')

$candidate = Join-Path $evidence 'concordia-candidate.exe'
Copy-Item -LiteralPath (Join-Path $root 'target/release/concordia.exe') -Destination $candidate
$hash = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash
[System.IO.File]::WriteAllText((Join-Path $evidence 'candidate-sha256.txt'), $hash)
for ($pass = 1; $pass -le $JourneyPasses; $pass++) {
    Invoke-Check "release-journeys-$pass" 'cargo.exe' @('test', '-p', 'concordia', '--test', 'tui_journeys', '--locked', '--', '--ignored', '--test-threads=1', '--nocapture') 600 $candidate
    Invoke-Check "state-stress-$pass" 'cargo.exe' @('test', '-p', 'concordia', '--test', 'reliability', '--locked', '--quiet')
}
Invoke-Check 'screenshots' 'cargo.exe' @('test', '-p', 'concordia', '--lib', 'write_screenshots', '--locked', '--', '--ignored')
Invoke-Check 'synthetic-screen' 'cargo.exe' @('test', '-p', 'concordia', '--lib', 'show_screen', '--locked', '--', '--ignored', '--nocapture')
$failed = @($results | Where-Object { $_.exitCode -ne 0 })
if ($failed.Count -gt 0) { throw "Reliability checks completed with unresolved gates: $(($failed.check) -join ', '). Candidate SHA256 $hash" }
Write-Host "PASS reliability wall: $($results.Count) checks; candidate SHA256 $hash"
