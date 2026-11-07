param([string]$Output = 'bom.json')
$ErrorActionPreference = 'Stop'
$env:NO_COLOR = $null
$metadataPreference = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$metadataText = @(& cargo metadata --format-version 1 --locked 2>&1)
$complete = $LASTEXITCODE -eq 0
if (-not $complete) {
    # Offline developer sandboxes may have the build cache but not every registry index entry.
    # CI uses the first query and therefore records the complete locked dependency graph.
    $metadataText = @(& cargo metadata --format-version 1 --locked --no-deps 2>&1)
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed in both complete and --no-deps modes' }
}
$ErrorActionPreference = $metadataPreference
$metadata = ($metadataText -join "`n") | ConvertFrom-Json
$components = @(
    foreach ($package in $metadata.packages) {
        [ordered]@{
            type = 'library'
            name = [string]$package.name
            version = [string]$package.version
            purl = "pkg:cargo/$($package.name)@$($package.version)"
            scope = if ($package.name -eq 'concordia') { 'required' } else { 'optional' }
        }
    }
)
$application = @($metadata.packages | Where-Object { $_.name -eq 'concordia' })[0]
if (-not $application) { throw 'cargo metadata did not contain the concordia package' }
$bom = [ordered]@{
    bomFormat = 'CycloneDX'
    specVersion = '1.5'
    serialNumber = 'urn:uuid:00000000-0000-0000-0000-000000000001'
    version = 1
    metadata = [ordered]@{
        timestamp = [DateTime]::UtcNow.ToString('o')
        tools = @([ordered]@{ vendor = 'Concordia contributors'; name = 'generate-sbom.ps1'; version = '1' })
        component = [ordered]@{ type = 'application'; name = 'concordia'; version = [string]$application.version }
    }
    components = $components
    properties = @([ordered]@{ name = 'concordia:dependency-resolution'; value = if ($complete) { 'complete' } else { 'package metadata only; offline fallback' } })
}
$json = $bom | ConvertTo-Json -Depth 10
[IO.File]::WriteAllText((Join-Path (Get-Location) $Output), $json, [Text.UTF8Encoding]::new($false))
Write-Host "CycloneDX SBOM: $Output ($($components.Count) components)"
