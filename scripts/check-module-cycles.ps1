$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..\crates\concordia\src'
$modules = Get-ChildItem $root -Filter '*.rs' -File | Where-Object Name -NotLike 'main.rs'
$edges = @{}
$allowlistPath = Join-Path $PSScriptRoot 'module-cycles-allowlist.txt'
$allowed = @{}
if (Test-Path $allowlistPath) {
    foreach ($line in Get-Content $allowlistPath) {
        if ($line -match '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(\S.*)$') { $allowed[$Matches[1]] = $Matches[2] }
    }
}
foreach ($module in $modules) {
    $name = [IO.Path]::GetFileNameWithoutExtension($module.Name)
    # Test-only imports are not production module dependencies and commonly point back to
    # the module under test. The guard intentionally analyzes the production portion only.
    $source = Get-Content $module.FullName
    $testStart = [Array]::IndexOf($source, '#[cfg(test)]')
    if ($testStart -ge 0) { $source = $source[0..($testStart - 1)] }
    $edges[$name] = @($source | Select-String -Pattern 'crate::([A-Za-z_][A-Za-z0-9_]*)' -AllMatches |
        ForEach-Object { $_.Matches } | ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ -ne $name } | Sort-Object -Unique)
}
$knownCycleStarts = [Collections.Generic.HashSet[string]]::new()
foreach ($start in $edges.Keys) {
    $seen = [Collections.Generic.HashSet[string]]::new()
    $queue = [Collections.Generic.Queue[string]]::new(); $queue.Enqueue($start)
    while ($queue.Count) { $current = $queue.Dequeue(); if (!$seen.Add($current)) { continue }
        foreach ($next in @($edges[$current])) {
            if ($next -eq $start) {
                if (!$allowed.ContainsKey($start)) { throw "module cycle detected: $start -> ... -> $start (add a named reason to $allowlistPath only for an intentional known cycle)" }
                [void]$knownCycleStarts.Add($start)
                continue
            }
            if ($edges.ContainsKey($next)) { $queue.Enqueue($next) }
        }
    }
}
Write-Output ('No unapproved cycles among direct top-level module references; {0} named starting modules have allowed cycles.' -f $knownCycleStarts.Count)
