[CmdletBinding()]
param(
    [switch]$Install,
    [switch]$CheckOnly,
    [hashtable]$ProbeOverrides
)

# Development bootstrap flow: probe prerequisites, optionally install missing items in order,
# then build and install Concordia. -CheckOnly probes and prints without changing the machine.
# -Install performs the ordered plan. ProbeOverrides supplies offline test probes.

$ErrorActionPreference = 'Stop'
$script:InstallRoot = Split-Path -Parent $PSScriptRoot
$script:CargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$script:ProgramFilesX86 = ${env:ProgramFiles(x86)}

function New-Result {
    param([string]$Name, [bool]$Ok, [string]$Detail, [string]$Fix)
    [pscustomobject]@{ Name = $Name; Ok = $Ok; Detail = $Detail; Fix = $Fix }
}

function Get-ProbeResult {
    param([string]$Name, [scriptblock]$Default)
    if ($ProbeOverrides -and $ProbeOverrides.ContainsKey($Name)) {
        $override = $ProbeOverrides[$Name]
        if ($override -is [scriptblock]) { return (& $override) }
        return $override
    }
    return (& $Default)
}

function Test-Rustup {
    $command = Get-RustupCommand
    $detail = if ($command) { "found $($command.Source)" } else { 'rustup was not found' }
    New-Result 'rustup' ([bool]$command) $detail 'winget install --id Rustlang.Rustup -e'
}

function Get-RustupCommand {
    $command = Get-Command rustup.exe -ErrorAction SilentlyContinue
    if (-not $command) {
        $candidate = Join-Path $script:CargoBin 'rustup.exe'
        if (Test-Path $candidate) { $command = Get-Item $candidate }
    }
    $command
}

function Get-PinnedToolchain {
    $path = Join-Path $script:InstallRoot 'rust-toolchain.toml'
    $match = [regex]::Match((Get-Content $path -Raw), '(?m)^channel\s*=\s*"([^"]+)"')
    $match.Groups[1].Value
}

function Test-Toolchain {
    $version = Get-PinnedToolchain
    $fix = "rustup toolchain install $version --component rustfmt,clippy"
    $rustupCommand = Get-RustupCommand
    if (-not $rustupCommand) {
        return (New-Result "toolchain ($version), rustfmt, clippy" $false 'needs rustup first' $fix)
    }
    $rustup = $rustupCommand.Source
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $list = & $rustup toolchain list 2>&1 | Out-String
    $listCode = $LASTEXITCODE
    $components = & $rustup component list --toolchain $version 2>&1 | Out-String
    $componentCode = $LASTEXITCODE
    $ErrorActionPreference = $oldPreference
    $installed = $list -match "(?m)^\s*$([regex]::Escape($version))(?:-|\s)"
    $hasComponents = $components -match '(?m)^rustfmt.*installed' -and $components -match '(?m)^clippy.*installed'
    $ok = $listCode -eq 0 -and $componentCode -eq 0 -and $installed -and $hasComponents
    $detail = if ($ok) { 'pinned toolchain and components are installed' } else { 'pinned toolchain or required components are missing' }
    New-Result "toolchain ($version), rustfmt, clippy" $ok $detail $fix
}

function Get-BuildToolsFix {
    'winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
}

function Test-Msvc {
    $vswhere = $null
    if ($script:ProgramFilesX86) { $vswhere = Join-Path $script:ProgramFilesX86 'Microsoft Visual Studio\Installer\vswhere.exe' }
    $ok = $false
    if ($vswhere -and (Test-Path $vswhere)) {
        $oldPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $output = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 2>$null
        $code = $LASTEXITCODE
        $ErrorActionPreference = $oldPreference
        $ok = $code -eq 0 -and [bool]$output
    }
    $detail = if ($ok) { 'Visual Studio C++ tools are installed' } else { 'Visual Studio C++ tools were not found' }
    New-Result 'MSVC' $ok $detail (Get-BuildToolsFix)
}

function Test-WindowsSdk {
    $ok = $false
    if ($script:ProgramFilesX86) {
        $libRoot = Join-Path $script:ProgramFilesX86 'Windows Kits\10\Lib'
        $matches = @(Get-ChildItem $libRoot -Filter DbgHelp.Lib -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match '\\um\\x64\\DbgHelp\.Lib$' })
        $ok = $matches.Count -gt 0
    }
    $detail = if ($ok) { 'DbgHelp.Lib is installed' } else { 'DbgHelp.Lib is not installed or is still installing' }
    New-Result 'Windows SDK' $ok $detail (Get-BuildToolsFix)
}

function Test-VsInstaller {
    $folder = $null
    if ($script:ProgramFilesX86) { $folder = Join-Path $script:ProgramFilesX86 'Microsoft Visual Studio\Installer' }
    $running = @(Get-Process setup, vs_installer, vs_BuildTools, vs_installershell -ErrorAction SilentlyContinue |
            Where-Object { $folder -and $_.Path -and $_.Path.StartsWith($folder, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    $detail = if ($running) { 'wait for the Visual Studio installer to finish' } else { 'not running' }
    New-Result 'VS installer' (-not $running) $detail ''
}

function Test-WezTerm {
    $command = Get-Command wezterm.exe -ErrorAction SilentlyContinue
    if (-not $command -and $env:ProgramFiles) {
        $candidate = Join-Path $env:ProgramFiles 'WezTerm\wezterm.exe'
        if (Test-Path $candidate) { $command = Get-Item $candidate }
    }
    $detail = if ($command) { 'found' } else { 'not found (warning only)' }
    New-Result 'WezTerm (optional)' ([bool]$command) $detail 'winget install --id wez.wezterm -e'
}

function Get-Preflight {
    @(
        (Get-ProbeResult 'Rustup' { Test-Rustup }),
        (Get-ProbeResult 'Toolchain' { Test-Toolchain }),
        (Get-ProbeResult 'Msvc' { Test-Msvc }),
        (Get-ProbeResult 'WindowsSdk' { Test-WindowsSdk }),
        (Get-ProbeResult 'VsInstaller' { Test-VsInstaller }),
        (Get-ProbeResult 'WezTerm' { Test-WezTerm })
    )
}

function Get-InstallPlan {
    param([object[]]$Items)
    $plan = @()
    $rustup = $Items | Where-Object { $_.Name -eq 'rustup' }
    $toolchain = $Items | Where-Object { $_.Name -like 'toolchain (*' }
    $msvc = $Items | Where-Object { $_.Name -eq 'MSVC' }
    $sdk = $Items | Where-Object { $_.Name -eq 'Windows SDK' }
    $installer = $Items | Where-Object { $_.Name -eq 'VS installer' }
    $wezterm = $Items | Where-Object { $_.Name -eq 'WezTerm (optional)' }
    if (-not $rustup.Ok) { $plan += [pscustomobject]@{ Type = 'Install'; File = 'winget.exe'; Arguments = @('install', '--id', 'Rustlang.Rustup', '-e'); Display = 'winget Rustlang.Rustup' } }
    if (-not $toolchain.Ok) {
        $version = Get-PinnedToolchain
        $plan += [pscustomobject]@{ Type = 'Install'; File = 'rustup.exe'; Arguments = @('toolchain', 'install', $version, '--component', 'rustfmt,clippy'); Display = "rustup toolchain install $version --component rustfmt,clippy" }
    }
    if (-not $msvc.Ok -or -not $sdk.Ok) {
        if (-not $installer.Ok) {
            $plan += [pscustomobject]@{ Type = 'WaitForInstaller'; File = ''; Arguments = @(); Display = 'wait for Visual Studio installer (poll every 10 seconds, report every minute, maximum 30 minutes)' }
        }
        $plan += [pscustomobject]@{ Type = 'Install'; File = 'winget.exe'; Arguments = @('install', '--id', 'Microsoft.VisualStudio.2022.BuildTools', '-e', '--override', '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'); Display = 'winget Microsoft.VisualStudio.2022.BuildTools' }
    }
    if (-not $wezterm.Ok) { $plan += [pscustomobject]@{ Type = 'Install'; File = 'winget.exe'; Arguments = @('install', '--id', 'wez.wezterm', '-e'); Display = 'winget wez.wezterm' } }
    $plan
}

function Show-Preflight {
    param([object[]]$Items)
    Write-Host ($Items | Format-Table Name, Ok, Detail, Fix -AutoSize | Out-String -Width 240)
    $Items
}

function Invoke-Native {
    param([string]$File, [string[]]$Arguments)
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $File @Arguments 2>&1 | ForEach-Object { Write-Host $_ }
    $code = $LASTEXITCODE
    $ErrorActionPreference = $oldPreference
    return [int]$code
}

function Wait-ForVsInstaller {
    param([int]$TimeoutSeconds = 1800)
    $elapsed = 0
    $lastReport = -60
    while ($elapsed -lt $TimeoutSeconds) {
        $state = Test-VsInstaller
        if ($state.Ok) { return $true }
        Start-Sleep -Seconds 10
        $elapsed += 10
        if ($elapsed - $lastReport -ge 60) {
            Write-Host "Still waiting for the Visual Studio installer ($elapsed seconds elapsed)..."
            $lastReport = $elapsed
        }
    }
    $false
}

function Invoke-InstallDev {
    $env:Path = "$script:CargoBin;$env:Path"
    $items = @(Get-Preflight)
    $required = @($items | Where-Object { $_.Name -ne 'WezTerm (optional)' })
    if ($CheckOnly) {
        Show-Preflight $items | Out-Null
        if (@($required | Where-Object { -not $_.Ok }).Count -gt 0) { return 1 }
        return 0
    }
    if ($Install) {
        foreach ($step in @(Get-InstallPlan $items)) {
            if ($step.Type -eq 'WaitForInstaller') {
                if (-not (Wait-ForVsInstaller)) { Write-Host 'Timed out waiting for the Visual Studio installer.'; return 1 }
                continue
            }
            if ($step.Display -eq 'winget Microsoft.VisualStudio.2022.BuildTools') {
                $msvc = Get-ProbeResult 'Msvc' { Test-Msvc }
                $sdk = Get-ProbeResult 'WindowsSdk' { Test-WindowsSdk }
                if ($msvc.Ok -and $sdk.Ok) { continue }
            }
            Write-Host "Installing $($step.Display)..."
            $nativeCode = Invoke-Native $step.File $step.Arguments
            if ($nativeCode -ne 0) { return $nativeCode }
            if ($step.File -eq 'winget.exe' -and $step.Display -like 'winget Rustlang.Rustup*') {
                # Refresh PATH because winget may have just created .cargo\bin.
                $env:Path = "$( [Environment]::GetEnvironmentVariable('Path', 'Machine'));$( [Environment]::GetEnvironmentVariable('Path', 'User'))"
            }
        }
        $items = @(Get-Preflight)
        if (@($items | Where-Object { $_.Name -ne 'WezTerm (optional)' -and -not $_.Ok }).Count -gt 0) { Show-Preflight $items | Out-Null; return 1 }
    } elseif (@($required | Where-Object { -not $_.Ok }).Count -gt 0) {
        Show-Preflight $items | Out-Null
        Write-Host 'Run the Fix command(s) above, or rerun with -Install.'
        return 1
    }
    Set-Location $script:InstallRoot
    if ((Invoke-Native 'cargo.exe' @('build', '-p', 'concordia', '--release')) -ne 0) { return $LASTEXITCODE }
    $binary = Join-Path $script:InstallRoot 'target\release\concordia.exe'
    if ((Invoke-Native $binary @('install')) -ne 0) { return $LASTEXITCODE }
    $help = & $binary --help 2>$null | Out-String
    if ($help -match '\bshortcut\b') { Invoke-Native $binary @('shortcut', 'install') | Out-Null }
    # Doctor is diagnostic: show its output even if it reports a problem.
    Invoke-Native $binary @('doctor') | Out-Null
    Write-Host 'Open a new terminal (or use the Start menu shortcut) and run: concordia'
    return 0
}

if ($MyInvocation.InvocationName -ne '.') { exit (Invoke-InstallDev) }
