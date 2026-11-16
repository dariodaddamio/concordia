<#
Development loop: one Concordia window that follows your edits.

  scripts/dev-loop.ps1 [-Project <folder>] [-Fresh]

Starts a debug build of Concordia in its own state folder (a copy of your account registry, so
real sign-ins work) and watches crates/concordia. Each save rebuilds the debug binary (about 8 s
incrementally); the new binary is swapped in next to the running one and the window restarts
itself into it as soon as it is idle, keeping its tabs and drafts. A failed build leaves the
window running and prints the first errors.

The window listens on the test-control port written to <state>\dev-control.txt, so
scripts/dev-ctl.ps1 can read the screen, save screenshots and send keys while you watch.
Nothing here touches your installed Concordia or its state.
#>
param(
    [string]$Project = '',
    [int]$Port = 47321,
    [switch]$Fresh
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$dev = Join-Path $env:TEMP 'concordia-dev'
$state = Join-Path $dev 'state'
$bin = Join-Path $dev 'bin'
$exe = Join-Path $bin 'concordia.exe'
$log = Join-Path $dev 'dev-loop.log'
if (-not $Project) { $Project = Join-Path $dev 'project' }
function Note($m) { $line = '{0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; Add-Content $log $line; Write-Output $line }

if ($Fresh -and (Test-Path $state)) { Remove-Item $state -Recurse -Force }
New-Item -ItemType Directory -Force "$state\.ao", $bin, $Project | Out-Null
if (-not (Test-Path "$state\.ao\store.json")) {
    Copy-Item "$env:LOCALAPPDATA\concordia\.ao\store.json" "$state\.ao\store.json"
    foreach ($f in 'config.toml', 'compat.json') {
        if (Test-Path "$env:LOCALAPPDATA\concordia\$f") { Copy-Item "$env:LOCALAPPDATA\concordia\$f" "$state\$f" }
    }
}
$token = 'dev-' + [guid]::NewGuid().ToString('N')
"127.0.0.1:$Port;$token" | Set-Content (Join-Path $state 'dev-control.txt') -Encoding ascii

function Build {
    # Cargo reports progress on stderr; that is not a failure.
    $ErrorActionPreference = 'Continue'
    $out = & cargo build -p concordia --bin concordia --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) {
        Note 'build failed:'
        $out | Select-String -Pattern '^error' -Context 0, 6 | Select-Object -First 3 | ForEach-Object { Write-Output $_.ToString() }
        return $false
    }
    return $true
}

function Swap {
    # A running executable cannot be overwritten on Windows but can be renamed. The window
    # watches its own file and restarts into the replacement when idle.
    Get-ChildItem $bin -Filter 'concordia.old-*.exe' -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue }
    if (Test-Path $exe) { Rename-Item $exe ("concordia.old-{0}.exe" -f (Get-Date -Format 'HHmmssfff')) }
    Copy-Item (Join-Path $repo 'target\debug\concordia.exe') $exe
}

function Launch {
    $psi = [System.Diagnostics.ProcessStartInfo]::new($exe)
    $psi.WorkingDirectory = $Project
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['CONCORDIA_STATE_DIR'] = $state
    $psi.EnvironmentVariables['CONCORDIA_INSTALL_DIR'] = $bin
    $psi.EnvironmentVariables['CONCORDIA_TEST_CONTROL'] = "127.0.0.1:$Port;$token"
    $psi.EnvironmentVariables.Remove('NO_COLOR')
    $p = [System.Diagnostics.Process]::Start($psi)
    Note "window started (pid $($p.Id)); state $state"
}

function Running {
    @(Get-Process concordia -ErrorAction SilentlyContinue | Where-Object {
            try { $_.Path -and $_.Path.StartsWith($bin, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
        }).Count -gt 0
}

Note "dev loop for $repo"
# A window left by an earlier loop would hold the control port; close only windows run from
# this loop's own bin folder.
Get-Process concordia -ErrorAction SilentlyContinue | Where-Object {
    try { $_.Path -and $_.Path.StartsWith($bin, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
} | ForEach-Object { Stop-Process -Id $_.Id -Force; Note "closed earlier dev window (pid $($_.Id))" }
if (-not (Build)) { throw 'initial build failed' }
Swap
Launch

$watch = [System.IO.FileSystemWatcher]::new((Join-Path $repo 'crates\concordia'))
$watch.IncludeSubdirectories = $true
$watch.NotifyFilter = [System.IO.NotifyFilters]'LastWrite, FileName'
$watch.EnableRaisingEvents = $true
while ($true) {
    $change = $watch.WaitForChanged([System.IO.WatcherChangeTypes]::All, 2000)
    if ($change.TimedOut) {
        # A restart into a new build leaves a moment with no window; look again before ending.
        if (-not (Running)) {
            Start-Sleep -Seconds 5
            if (-not (Running)) { Note 'window closed; dev loop ends'; break }
        }
        continue
    }
    if ($change.Name -notmatch '\.(rs|toml|ico|manifest)$') { continue }
    # Let an editor finish writing a burst of files before building.
    do { $more = $watch.WaitForChanged([System.IO.WatcherChangeTypes]::All, 400) } until ($more.TimedOut)
    $started = Get-Date
    Note "change in $($change.Name); building"
    if (Build) {
        Swap
        Note ('swapped in after {0:n1} s; the window restarts when idle' -f ((Get-Date) - $started).TotalSeconds)
        if (-not (Running)) { Launch }
    }
}
