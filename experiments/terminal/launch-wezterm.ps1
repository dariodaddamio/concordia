#Requires -Version 5.1
<#
.SYNOPSIS
  Minimal WezTerm launcher for P1 terminal spike (explicit pane IDs).

.DESCRIPTION
  Preferred host path for the thin orchestrator:
    - wezterm start / wezterm cli spawn|split-pane return pane IDs
    - subsequent splits use --pane-id (never focus heuristics)
    - set-tab-title for account/status labels without remapping Escape/Ctrl-C

  If wezterm is not installed, prints exact check/install steps and exits 2.
  Does not install software.
#>
[CmdletBinding()]
param(
  [string]$RepoRoot,
  [string]$MarkerDir,
  [string]$IdFile
)

$ErrorActionPreference = 'Stop'

$scriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $RepoRoot) {
  $RepoRoot = (Resolve-Path (Join-Path $scriptDir '..\..')).Path
}
if (-not $MarkerDir) {
  $MarkerDir = Join-Path $env:TEMP 'orch-terminal-spike\wezterm'
}
if (-not $IdFile) {
  $IdFile = Join-Path $env:TEMP 'orch-terminal-spike\wezterm-pane-ids.json'
}

function Find-WezTerm {
  $cmd = Get-Command wezterm -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\WezTerm\wezterm.exe'),
    (Join-Path $env:ProgramFiles 'WezTerm\wezterm.exe'),
    (Join-Path $env:USERPROFILE 'scoop\apps\wezterm\current\wezterm.exe')
  )
  foreach ($c in $candidates) {
    if (Test-Path -LiteralPath $c) { return $c }
  }
  return $null
}

function Write-InstallGuidance {
  Write-Host ''
  Write-Host 'WezTerm is not installed or not on PATH.'
  Write-Host 'User install steps (do not auto-install from agents):'
  Write-Host '  1. https://wezterm.org/install/windows.html'
  Write-Host '  2. Or: winget install wez.wezterm'
  Write-Host '  3. Or: scoop install wezterm'
  Write-Host 'Verify:'
  Write-Host '  where.exe wezterm'
  Write-Host '  wezterm --version'
  Write-Host '  wezterm cli --help'
  Write-Host 'Then re-run: .\launch-wezterm.ps1'
  Write-Host ''
  Write-Host 'CLI references:'
  Write-Host '  https://wezterm.org/cli/cli/index.html'
  Write-Host '  https://wezterm.org/multiplexing.html'
}

$wez = Find-WezTerm
if (-not $wez) {
  Write-InstallGuidance
  exit 2
}

$configFile = Join-Path $scriptDir 'wezterm\wezterm.lua'
$env:WEZTERM_CONFIG_FILE = $configFile

Write-Host "wezterm: $wez"
& $wez --version
Write-Host "config:  $configFile"
Write-Host "repo:    $RepoRoot"

New-Item -ItemType Directory -Force -Path $MarkerDir | Out-Null
Remove-Item -LiteralPath (Join-Path $MarkerDir '*') -Force -ErrorAction SilentlyContinue

# Start GUI with control pane (isolated config). Capture nothing from GUI; use cli after.
$markerControl = Join-Path $MarkerDir 'control.txt'
$controlProg = @(
  'powershell.exe', '-NoExit', '-Command',
  "Set-Content -LiteralPath '$markerControl' -Value ('control ' + (Get-Date -Format o)); Write-Host '[control] WezTerm pane ready'"
)

Write-Host 'Starting WezTerm window (control pane)...'
Start-Process -FilePath $wez -ArgumentList (@('start', '--cwd', $RepoRoot, '--') + $controlProg) | Out-Null

# Give the GUI a moment to create the mux client / pane.
$deadline = (Get-Date).AddSeconds(15)
$listJson = $null
do {
  Start-Sleep -Milliseconds 500
  try {
    $listJson = & $wez cli list --format json 2>$null
    if ($listJson) { break }
  } catch { }
} while ((Get-Date) -lt $deadline)

if (-not $listJson) {
  throw 'wezterm cli list returned no panes. Is the GUI running with this config?'
}

$panes = $listJson | ConvertFrom-Json
if (-not $panes -or $panes.Count -lt 1) {
  throw 'No panes listed after wezterm start.'
}

# Prefer the most recently created pane as control (highest pane_id).
$control = $panes | Sort-Object pane_id -Descending | Select-Object -First 1
$controlId = [int]$control.pane_id
Write-Host "control pane_id=$controlId"

& $wez cli set-tab-title --pane-id $controlId 'orch:control' | Out-Null

# Explicit split from control - returns new pane id on stdout.
$markerA = Join-Path $MarkerDir 'agent-a.txt'
$agentAOut = & $wez cli split-pane --pane-id $controlId --right --percent 50 --cwd $RepoRoot -- `
  powershell.exe -NoExit -Command "Set-Content -LiteralPath '$markerA' -Value ('agent-a ' + (Get-Date -Format o)); Write-Host '[agent-a] ready'"
$agentAId = [int]("$agentAOut".Trim())
Write-Host "agent-a pane_id=$agentAId (split from --pane-id $controlId)"
& $wez cli set-tab-title --pane-id $agentAId 'orch:agent-a idle' | Out-Null

# Explicit split from agent-a by id - not "whatever is focused".
$markerB = Join-Path $MarkerDir 'agent-b.txt'
$agentBOut = & $wez cli split-pane --pane-id $agentAId --bottom --percent 50 --cwd $RepoRoot -- `
  powershell.exe -NoExit -Command "Set-Content -LiteralPath '$markerB' -Value ('agent-b ' + (Get-Date -Format o)); Write-Host '[agent-b] ready'"
$agentBId = [int]("$agentBOut".Trim())
Write-Host "agent-b pane_id=$agentBId (split from --pane-id $agentAId)"
& $wez cli set-tab-title --pane-id $agentBId 'orch:agent-b idle' | Out-Null

$record = [ordered]@{
  host = 'wezterm'
  recorded_at = (Get-Date).ToString('o')
  control_pane_id = $controlId
  agent_a_pane_id = $agentAId
  agent_b_pane_id = $agentBId
  note = 'Ownership uses explicit pane ids from cli stdout / list - never focus heuristics.'
}
$record | ConvertTo-Json | Set-Content -LiteralPath $IdFile -Encoding UTF8
Write-Host "pane id record: $IdFile"

Start-Sleep -Seconds 1
Write-Host '--- markers ---'
Get-ChildItem -LiteralPath $MarkerDir -ErrorAction SilentlyContinue |
  ForEach-Object { Write-Host ("{0}: {1}" -f $_.Name, (Get-Content -LiteralPath $_.FullName -Raw).Trim()) }

Write-Host 'OK: explicit pane targeting demonstrated.'
Write-Host 'Persistence (manual): wezterm connect orch  # unix domain in wezterm/wezterm.lua'
Write-Host 'Complete the manual keyboard/mouse checklist in experiments/terminal/README.md.'
