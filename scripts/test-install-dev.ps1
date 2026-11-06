# Offline tests for install-dev.ps1. No installer, rustup, VS installer, or install script runs.
$ErrorActionPreference = 'Stop'
$failures = @()

function Check {
    param([bool]$Condition, [string]$Message)
    if ($Condition) { Write-Host "OK   $Message" } else { $script:failures += $Message; Write-Host "FAIL $Message" }
}

$scriptPath = Join-Path $PSScriptRoot 'install-dev.ps1'
. $scriptPath

Check ($null -ne (Get-Command Get-InstallPlan -ErrorAction SilentlyContinue)) 'Get-InstallPlan is available'
Check ($null -ne (Get-Command Invoke-Native -ErrorAction SilentlyContinue)) 'Invoke-Native is available'

$version = Get-PinnedToolchain
function New-Items {
    param([string[]]$Missing, [switch]$InstallerRunning)
    $names = @('rustup', "toolchain ($version), rustfmt, clippy", 'MSVC', 'Windows SDK', 'VS installer', 'WezTerm (optional)')
    @($names | ForEach-Object {
        $ok = $_ -notin $Missing -and -not ($_.StartsWith('toolchain (') -and $Missing -contains 'toolchain')
        if ($_ -eq 'VS installer') { $ok = -not $InstallerRunning }
        [pscustomobject]@{ Name = $_; Ok = $ok }
    })
}
$missing = New-Items -Missing 'rustup','toolchain','MSVC','Windows SDK','WezTerm (optional)'
$plan = @(Get-InstallPlan $missing)
Check ($plan.Count -eq 4) 'missing everything yields four ordered steps'
Check ($plan[0].File -eq 'winget.exe' -and $plan[0].Arguments -contains 'Rustlang.Rustup' -and $plan[0].Arguments -notcontains '--override') 'rustup winget step has no VS override'
Check ($plan[1].File -eq 'rustup.exe' -and ($plan[1].Arguments -join ' ') -eq "toolchain install $version --component rustfmt,clippy") 'toolchain step is rustup with pinned components'
Check ($plan[2].Arguments -contains 'Microsoft.VisualStudio.2022.BuildTools' -and $plan[2].Arguments -contains '--override') 'one Build Tools step carries the VS override'
Check ($plan[3].Arguments -contains 'wez.wezterm' -and $plan[3].Arguments -notcontains '--override') 'WezTerm step has no VS override'

$onlyToolchain = New-Items -Missing 'toolchain'
$toolchainPlan = @(Get-InstallPlan $onlyToolchain)
Check ($toolchainPlan.Count -eq 1 -and $toolchainPlan[0].File -eq 'rustup.exe') 'only missing toolchain yields only the rustup command'

$running = New-Items -Missing 'MSVC','Windows SDK' -InstallerRunning
$runningPlan = @(Get-InstallPlan $running)
Check ($running[4].Ok -eq $false) 'running VS installer fixture uses Ok false'
$buildIndex = @($runningPlan | ForEach-Object { if ($_.Arguments -contains 'Microsoft.VisualStudio.2022.BuildTools') { [array]::IndexOf($runningPlan, $_) } })[0]
$waitIndex = @($runningPlan | ForEach-Object { if ($_.Type -eq 'WaitForInstaller') { [array]::IndexOf($runningPlan, $_) } })[0]
Check ($waitIndex -ge 0 -and $buildIndex -eq $waitIndex + 1) 'running VS installer yields wait followed by Build Tools'
$cleanPlan = @(Get-InstallPlan (New-Items -Missing 'MSVC'))
Check (@($cleanPlan | Where-Object Type -eq 'WaitForInstaller').Count -eq 0 -and @($cleanPlan | Where-Object { $_.Arguments -contains 'Microsoft.VisualStudio.2022.BuildTools' }).Count -eq 1) 'missing MSVC with idle installer installs Build Tools without waiting'

$nativeZero = Invoke-Native 'powershell.exe' @('-NoProfile', '-NonInteractive', '-Command', 'Write-Output line-one; Write-Output line-two; exit 0')
$nativeThree = Invoke-Native 'powershell.exe' @('-NoProfile', '-NonInteractive', '-Command', 'Write-Output line-three; exit 3')
Check ($nativeZero -is [int] -and $nativeZero -eq 0) 'native helper returns integer zero with output'
Check ($nativeThree -is [int] -and $nativeThree -eq 3) 'native helper returns integer nonzero with output'
$redirected = $true
try { $null = Invoke-Native 'powershell.exe' @('-NoProfile', '-NonInteractive', '-Command', 'Write-Output redirected') *> $null } catch { $redirected = $false }
Check $redirected 'native helper remains safe when all output is redirected'

Check ((Get-Content $scriptPath -Raw) -match 'if \(\$CheckOnly\)') 'CheckOnly path remains present'

if ($failures.Count -gt 0) { Write-Host "$($failures.Count) failure(s)"; exit 1 }
Write-Host 'test-install-dev PASS'
exit 0
