# Shared local/release policy reader. Publisher identity is configured outside source control.
function Get-ReleaseSigningPolicy {
    param([string]$PolicyPath = '')
    if (-not $PolicyPath) { $PolicyPath = Join-Path $PSScriptRoot 'release-signing.json' }
    $file = Get-Item -LiteralPath $PolicyPath -ErrorAction Stop
    if ($file.PSIsContainer -or $file.Length -gt 16384) { throw 'Signing policy must be a JSON file of at most 16 KiB.' }
    $policy = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    $expected = $env:CONCORDIA_RELEASE_PUBLISHER
    if (-not $expected) { $expected = [string]$policy.publisher }
    if (-not $expected -or $expected.Length -gt 256 -or $expected -match '[\x00-\x1f\x7f]') {
        throw 'Set CONCORDIA_RELEASE_PUBLISHER to the verified legal publisher, or pass an explicit local signing policy. Keep personal identity out of source control.'
    }
    $timestamp = $null
    if (-not [Uri]::TryCreate([string]$policy.timestampUrl, [UriKind]::Absolute, [ref]$timestamp) -or
        $timestamp.Scheme -notin @('http', 'https') -or $timestamp.UserInfo) {
        throw 'Signing policy needs an absolute HTTP(S) RFC 3161 timestamp endpoint without credentials.'
    }
    [pscustomobject]@{ publisher = $expected; timestampUrl = $timestamp.AbsoluteUri }
}
