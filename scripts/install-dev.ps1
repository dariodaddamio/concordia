# Build concordia and install it as the `concordia` on your PATH (new terminals and panes get it).
# The install itself is `concordia install`: one implementation for source builds and downloads alike.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$env:Path = "$cargoBin;$env:Path"
$pf86 = ${env:ProgramFiles(x86)}
if ($pf86) {
    $vswhere = Join-Path $pf86 "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vs) {
            $devCmd = Join-Path $vs "Common7\Tools\VsDevCmd.bat"
            cmd /c "`"$devCmd`" -arch=x64 -host_arch=x64 && set" | ForEach-Object {
                if ($_ -match '^([^=]+)=(.*)$') {
                    [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
                }
            }
        }
    }
}

Set-Location $root
cargo build -p concordia --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$src = Join-Path $root "target\release\concordia.exe"
& $src install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# This product was called `orc` before the rename. Its npm shims (orc.cmd, orc.ps1, orc) can still
# answer to `orc` with a stale CLI or open in Notepad. Move them aside once (kept as *.bak-orc).
$npm = Join-Path $env:APPDATA "npm"
foreach ($name in @("orc.cmd", "orc.ps1", "orc")) {
    $p = Join-Path $npm $name
    if (Test-Path $p) {
        $bak = "$p.bak-orc"
        if (-not (Test-Path $bak)) { Move-Item -Force $p $bak } else { Remove-Item -Force $p }
        Write-Host "retired old shim $p"
    }
}

Write-Host ""
Write-Host "Open a NEW terminal (or WezTerm tab) and run: concordia --version"
& $src --version
exit 0
