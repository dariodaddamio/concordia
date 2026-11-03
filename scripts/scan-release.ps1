# Scan an exact artifact with Defender. No exclusions or protection changes.
param([Parameter(Mandatory = $true)][string]$Exe)
$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$status = Get-MpComputerStatus
if (-not $status.AntivirusEnabled -or -not $status.RealTimeProtectionEnabled) {
    throw 'Defender antivirus and real-time protection must be enabled for release validation.'
}
if (-not $status.AntivirusSignatureLastUpdated -or $status.AntivirusSignatureLastUpdated -lt (Get-Date).AddDays(-2)) {
    throw 'Defender security intelligence is stale. Run Update-MpSignature and retry.'
}
$platform = Join-Path $env:ProgramData 'Microsoft\Windows Defender\Platform'
$scanner = Get-ChildItem -LiteralPath $platform -Directory |
    Where-Object { $_.Name -match '^\d' } | Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'MpCmdRun.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $scanner) { $scanner = Join-Path $env:ProgramFiles 'Windows Defender\MpCmdRun.exe' }
$before = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash
Write-Host "Defender signatures: $($status.AntivirusSignatureVersion)"
# This custom-scan option ignores exclusions and reports detections without remediating
# them, so exit 0 cannot mean 'a threat was removed successfully'. Real-time protection
# stays enabled. Any threat or scan error fails the release gate.
& $scanner -Scan -ScanType 3 -File $exePath -DisableRemediation
if ($LASTEXITCODE -ne 0) { throw "Defender scan failed or detected a threat (exit $LASTEXITCODE)." }
if (-not (Test-Path -LiteralPath $exePath) -or (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash -ne $before) {
    throw 'Artifact disappeared or changed during its scan.'
}
Write-Host "PASS Defender scan; SHA256 $before"
