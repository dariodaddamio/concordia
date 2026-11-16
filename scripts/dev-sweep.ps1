<#
Open every command, settings tab and shortcut surface in the dev window and save each screen.

  scripts/dev-sweep.ps1 [-Out <folder>] [-Columns 120] [-Rows 36]

Requires scripts/dev-loop.ps1 to be running. Each surface is opened from a clean chat, captured
as text and PNG under <Out>, then closed with Esc. Use it to review the whole UI after a change.
#>
param([string]$Out = (Join-Path $env:TEMP 'concordia-dev\sweep'), [int]$Columns = 120, [int]$Rows = 36)
$ErrorActionPreference = 'Stop'
$ctl = Join-Path $PSScriptRoot 'dev-ctl.ps1'
New-Item -ItemType Directory -Force $Out | Out-Null
function C { & $ctl @args }
function Snap($name) {
    $text = C screen
    $text | Set-Content -Encoding UTF8 (Join-Path $Out "$name.txt")
    $png = C snap "sweep-$name"
    if ($png -and (Test-Path $png)) { Move-Item $png (Join-Path $Out "$name.png") -Force }
}
function Reset {
    foreach ($i in 1..3) { C key Esc; Start-Sleep -Milliseconds 150 }
    # A command that leaves its text in the composer would turn the next one into completion.
    foreach ($i in 1..24) { C key Backspace }
}

C resize $Columns $Rows | Out-Null
Start-Sleep 1
Reset
Snap '00-home'

$commands = [ordered]@{
    'terminal' = '/terminal'; 'use' = '/use'; 'model' = '/model'; 'effort' = '/effort';
    'rename' = '/rename'; 'config' = '/config'; 'mode' = '/mode'; 'usage' = '/usage';
    'resume' = '/resume'; 'harness' = '/harness'; 'skills' = '/skills'; 'hooks' = '/hooks';
    'mcp' = '/mcp'; 'open' = '/open'; 'copy' = '/copy'; 'doctor' = '/doctor'; 'update' = '/update'
}
$n = 1
foreach ($name in $commands.Keys) {
    Reset
    C send $commands[$name]
    Start-Sleep 2
    Snap ('{0:d2}-cmd-{1}' -f $n, $name)
    $n++
}

Reset
C text '/'
Start-Sleep 1
Snap '30-slash-completion'
Reset

C send '/config'
Start-Sleep 1
foreach ($i in 0..5) {
    Snap ('31-settings-tab{0}' -f $i)
    C key Right
    Start-Sleep -Milliseconds 300
}
Reset

foreach ($pair in @(@('F1', ''), @('k', 'CONTROL'), @('g', 'CONTROL'), @('o', 'CONTROL'))) {
    C key $pair[0] $pair[1]
    Start-Sleep 1
    Snap ('40-key-{0}{1}' -f $pair[1], $pair[0])
    Reset
}
Get-ChildItem $Out -Filter '*.txt' | Measure-Object | ForEach-Object { "captured $($_.Count) screens in $Out" }
