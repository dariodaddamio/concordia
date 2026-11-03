# Local signing with an existing identity-verified certificate in CurrentUser\My.
# Never creates certificates, exports private keys, or changes certificate trust.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Thumbprint = $env:CONCORDIA_SIGNING_THUMBPRINT,
    [string]$SignTool = '',
    [string]$PolicyPath = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-signing-policy.ps1')
$policy = Get-ReleaseSigningPolicy -PolicyPath $PolicyPath
if (-not $Thumbprint -or $Thumbprint -notmatch '^[0-9a-fA-F]{40}$') {
    throw 'Set CONCORDIA_SIGNING_THUMBPRINT to an existing trusted code-signing certificate thumbprint. See docs/windows-signing.md.'
}
$cert = Get-Item -LiteralPath "Cert:\CurrentUser\My\$Thumbprint"
$publisher = $cert.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
if ($publisher -cne $policy.publisher -or -not $cert.HasPrivateKey -or $cert.NotAfter -le (Get-Date) -or $cert.NotBefore -gt (Get-Date)) {
    throw 'Signing certificate must be current, have a private key, and match the configured publisher.'
}
if (-not ($cert.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq '1.3.6.1.5.5.7.3.3' })) {
    throw 'Certificate does not have the Code Signing extended key usage.'
}
if (-not $cert.Verify()) { throw 'Signing certificate chain is not trusted by Windows.' }
if (-not $SignTool) {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) { $SignTool = $command.Source }
    else {
        $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $SignTool = Get-ChildItem -LiteralPath $sdk -Directory |
            Where-Object { $_.Name -match '^10\.' } | Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
            Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
}
if (-not $SignTool) { throw 'Install the Windows SDK signing tools first.' }
$exePath = (Resolve-Path -LiteralPath $Exe).Path
& $SignTool sign /sha1 $Thumbprint /fd SHA256 /tr $policy.timestampUrl /td SHA256 /d Concordia $exePath
if ($LASTEXITCODE -ne 0) { throw "SignTool failed ($LASTEXITCODE)." }
& (Join-Path $PSScriptRoot 'verify-signature.ps1') -Exe $exePath -PolicyPath $PolicyPath
