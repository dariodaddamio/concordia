param()

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$source = Join-Path $root 'crates\concordia\src'
$violations = @()

Get-ChildItem $source -Recurse -Filter '*.rs' | ForEach-Object {
    $path = $_.FullName
    $relative = $path.Substring($root.Length + 1).Replace('\', '/')
    $lineNumber = 0
    Get-Content -LiteralPath $path | ForEach-Object {
        $lineNumber++
        if ($_ -match 'Command::new\s*\(') {
            $allowed = $relative -eq 'crates/concordia/src/process_ui.rs' -or
                ($relative -eq 'crates/concordia/src/connect.rs') -or
                ($_ -match 'explorer\.exe')
            if (-not $allowed) {
                $violations += "${relative}:${lineNumber}: $_"
            }
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Error ("Unrouted child process constructors found:`n" + ($violations -join "`n"))
}
Write-Host 'hidden-process-spawn gate PASS'
