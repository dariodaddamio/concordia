$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\crates\concordia\src')).Path
& node (Join-Path $PSScriptRoot 'lib\rust-module-graph.mjs') --check $root (Join-Path $PSScriptRoot 'module-cycles-allowlist.txt')
if ($LASTEXITCODE -ne 0) { throw 'Static source-reference cycle gate failed; inspect the exact edge diff before changing its baseline.' }
