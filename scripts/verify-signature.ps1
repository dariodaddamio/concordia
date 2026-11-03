# Verify the publisher and Windows trust on the exact executable that will ship.
param([Parameter(Mandatory = $true)][string]$Exe, [string]$PolicyPath = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-signing-policy.ps1')
$policy = Get-ReleaseSigningPolicy -PolicyPath $PolicyPath
$before = (Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash
$signature = Get-AuthenticodeSignature -LiteralPath (Resolve-Path -LiteralPath $Exe).Path
if ($signature.Status -ne 'Valid') {
    throw "Release signature is not trusted: $($signature.Status). $($signature.StatusMessage)"
}
$publisher = $signature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
if ($publisher -cne $policy.publisher) {
    throw "Unexpected signing publisher '$publisher'; expected '$($policy.publisher)'."
}
if (-not $signature.TimeStamperCertificate) {
    throw 'Release signature has no trusted timestamp.'
}
if ((Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash -ne $before) { throw 'Artifact changed during signature verification.' }
Write-Host "PASS trusted, timestamped signature: $publisher"
