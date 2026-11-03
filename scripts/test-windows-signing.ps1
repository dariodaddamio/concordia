# Exercise rejection paths without a signing key, trust-store changes, or model calls.
$ErrorActionPreference = 'Stop'
function Expect-Rejection([scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action }
    catch {
        if ($_.Exception.Message -notlike "*$Message*") { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "Expected rejection: $Message" }
    Write-Host "PASS rejected: $Message"
}
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("concordia-unsigned-" + [guid]::NewGuid().ToString('N') + '.ps1')
$previousPublisher = $env:CONCORDIA_RELEASE_PUBLISHER
try {
    $env:CONCORDIA_RELEASE_PUBLISHER = 'Synthetic Fixture Publisher'
    Set-Content -LiteralPath $fixture -Value "Write-Output 'unsigned signing-policy fixture'" -Encoding ascii
    $before = (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash
    Expect-Rejection { & (Join-Path $PSScriptRoot 'verify-signature.ps1') -Exe $fixture } 'Release signature is not trusted'
    Expect-Rejection { & (Join-Path $PSScriptRoot 'sign-release.ps1') -Exe $fixture -Thumbprint '' } 'Set CONCORDIA_SIGNING_THUMBPRINT'
    if ((Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash -ne $before) {
        throw 'A rejected signing request changed the file.'
    }
    $windowsExe = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    Expect-Rejection { & (Join-Path $PSScriptRoot 'verify-signature.ps1') -Exe $windowsExe } 'Unexpected signing publisher'
    $env:CONCORDIA_RELEASE_PUBLISHER = ''
    Expect-Rejection { & (Join-Path $PSScriptRoot 'verify-signature.ps1') -Exe $fixture } 'Set CONCORDIA_RELEASE_PUBLISHER'
} finally {
    $env:CONCORDIA_RELEASE_PUBLISHER = $previousPublisher
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture }
}
Write-Host 'PASS Windows signing rejection checks; no key or trust-store changes'
