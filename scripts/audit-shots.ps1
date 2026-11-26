[CmdletBinding()]
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$output = 'D:\concordia-audit\shots'
$run = Join-Path $repo ('.ao\audit-shots\' + [guid]::NewGuid().ToString('N'))
$scratch = Join-Path $run 'temp'
$bin = Join-Path $repo 'target\debug'
$exe = Join-Path $run 'concordia.exe'
$fake = Join-Path $run 'concordia-fake-provider.exe'
$utf8 = [System.Text.UTF8Encoding]::new($false)
$sizes = @(@(160, 45), @(120, 36), @(80, 24))
$shots = [System.Collections.Generic.List[string]]::new()
$unreachable = [System.Collections.Generic.List[string]]::new()
$script:session = $null
$script:sequence = 0

function Write-Utf8($Path, $Text) {
    [IO.File]::WriteAllText($Path, $Text, $utf8)
}

function Write-Json($Path, $Value) {
    Write-Utf8 $Path (ConvertTo-Json -InputObject $Value -Depth 15 -Compress)
}

New-Item -ItemType Directory -Force $scratch, $output | Out-Null
$savedTemp, $savedTmp = $env:TEMP, $env:TMP
try {
    $env:TEMP = $scratch
    $env:TMP = $scratch
    if (-not $SkipBuild) {
        $savedTarget = $env:CARGO_TARGET_DIR
        try {
            $env:CARGO_TARGET_DIR = Join-Path $repo 'target'
            & cargo build --offline --locked --manifest-path (Join-Path $repo 'Cargo.toml') -p concordia --bin concordia --bin concordia-fake-provider
            if ($LASTEXITCODE -ne 0) { throw 'Offline build failed.' }
        } finally { $env:CARGO_TARGET_DIR = $savedTarget }
    }
    foreach ($destination in $exe, $fake) {
        $binary = Join-Path $bin (Split-Path -Leaf $destination)
        Copy-Item -LiteralPath $binary -Destination $destination
    }
    if (-not ('ConcordiaAuditWindow' -as [type])) {
        Add-Type -ReferencedAssemblies System.Drawing @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class ConcordiaAuditWindow {
    private delegate bool EnumProc(IntPtr window, IntPtr data);
    [StructLayout(LayoutKind.Sequential)] private struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] private static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] private static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    public static IntPtr Find(uint pid) {
        SetProcessDPIAware();
        IntPtr best = IntPtr.Zero;
        long area = 0;
        EnumWindows((window, data) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            Rect rect;
            if (owner == pid && IsWindowVisible(window) && GetClientRect(window, out rect)) {
                long candidate = (long)(rect.Right - rect.Left) * (rect.Bottom - rect.Top);
                if (candidate > area) { best = window; area = candidate; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }

    public static void Save(IntPtr window, string path) {
        Rect rect;
        if (!GetClientRect(window, out rect) || rect.Right <= 0 || rect.Bottom <= 0 || IsIconic(window))
            throw new InvalidOperationException("The audit window has no visible client area.");
        using (var bitmap = new Bitmap(rect.Right, rect.Bottom)) {
            using (var graphics = Graphics.FromImage(bitmap)) {
                IntPtr dc = graphics.GetHdc();
                try {
                    if (!PrintWindow(window, dc, 1)) throw new InvalidOperationException("PrintWindow failed.");
                } finally { graphics.ReleaseHdc(dc); }
            }
            int first = bitmap.GetPixel(0, 0).ToArgb();
            for (int y = 0; y < bitmap.Height; y += 3) {
                for (int x = 0; x < bitmap.Width; x += 3) {
                    if (bitmap.GetPixel(x, y).ToArgb() != first) {
                        bitmap.Save(path, ImageFormat.Png);
                        return;
                    }
                }
            }
            throw new InvalidOperationException("PrintWindow returned a blank client area.");
        }
    }
}
'@
    }
} finally {
    $env:TEMP = $savedTemp
    $env:TMP = $savedTmp
}

function New-Fixture($Name, $Provider, [switch]$Question) {
    $root = Join-Path $run $Name
    $state = Join-Path $root 'state'
    $profile = Join-Path $root 'profile'
    $workspace = Join-Path $root 'parser-demo'
    $install = Join-Path $root 'install'
    New-Item -ItemType Directory -Force $state, $profile, $workspace, $install | Out-Null
    Write-Utf8 (Join-Path $install 'source') $repo
    Write-Utf8 (Join-Path $install 'shortcut-start-removed') 'removed'
    Write-Utf8 (Join-Path $install 'shortcut-desktop-removed') 'removed'
    Write-Utf8 (Join-Path $state 'config.toml') "[threads]`nlogging = 'on'`n[ui]`ntips = false`n[updates]`ninstall_on_launch = false`n[turn]`nidle_timeout_minutes = 1`n"
    if ($Provider) {
        $markers = @{
            codex = @('auth.json', '{}')
            claude = @('.credentials.json', 'synthetic-login-existence-marker')
            cursor = @('cli-config.json', '{"authInfo":{"email":"fake@example.invalid"}}')
            grok = @('auth.json', 'synthetic-login-existence-marker')
            opencode = @('auth.json', 'synthetic-login-existence-marker')
        }
        $accounts = foreach ($id in 'codex', 'claude', 'cursor', 'grok', 'opencode') {
            $accountHome = Join-Path $profile ".$id"
            New-Item -ItemType Directory -Force $accountHome | Out-Null
            Write-Utf8 (Join-Path $accountHome $markers[$id][0]) $markers[$id][1]
            @{ id = $id; provider = $id; homePath = $accountHome; label = $id; createdAt = '0' }
        }
        Write-Utf8 (Join-Path $profile '.cursor\concordia-api-key') 'synthetic-cursor-api-key'
        Write-Utf8 (Join-Path $profile '.grok\config.toml') "default_model = 'synthetic-model'"
        Write-Utf8 (Join-Path $profile '.codex\models_cache.json') '{"models":[{"slug":"gpt-5.6-luna","display_name":"GPT-5.6 Luna","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"}]},{"slug":"fixture-model","display_name":"Fixture model","visibility":"list"}]}'
        $model, $effort = if ($Provider -eq 'codex') { 'gpt-5.6-luna', 'low' } else { 'haiku', $null }
        $selection = @{
            provider = $Provider; account = $Provider; homePath = Join-Path $profile ".$Provider"
            model = $model; effort = $effort; updatedAt = '0'
        }
        New-Item -ItemType Directory -Force (Join-Path $state '.ao') | Out-Null
        Write-Json (Join-Path $state '.ao\store.json') @{ version = 1; selection = $selection; accounts = @($accounts) }
        $usage = Join-Path $profile '.codex\tmp'
        $sessions = Join-Path $profile '.codex\sessions'
        New-Item -ItemType Directory -Force $usage, $sessions | Out-Null
        Write-Json (Join-Path $usage 'rate_limits.json') @{
            primary = @{ used_percent = 23; window_minutes = 300 }
            secondary = @{ used_percent = 61; window_minutes = 10080 }
        }
        foreach ($chat in @(
            @('parser', $workspace, 'Review parser edge cases'),
            @('docs', $workspace, 'Draft the parser guide'),
            @('scratch', (Join-Path $root 'concordia-test-scratch'), 'Check a scratch example')
        )) {
            $history = @(
                @{ type = 'session_meta'; payload = @{ id = "audit-$($chat[0])"; cwd = $chat[1] } },
                @{ type = 'response_item'; payload = @{ type = 'message'; role = 'user'; content = @(@{ type = 'input_text'; text = $chat[2] }) } },
                @{ type = 'response_item'; payload = @{ type = 'message'; role = 'assistant'; content = @(@{ type = 'output_text'; text = 'The sample is ready for review.' }) } }
            )
            $lines = $history | ForEach-Object { ConvertTo-Json -InputObject $_ -Depth 10 -Compress }
            Write-Utf8 (Join-Path $sessions "$($chat[0]).jsonl") (($lines -join "`n") + "`n")
        }
        Write-Utf8 (Join-Path $workspace 'parser.rs') "pub fn parse(input: &str) -> Vec<&str> {`n    input.split_whitespace().collect()`n}`n"
    }
    if ($Question) {
        Write-Json (Join-Path $workspace 'appserver-fixture.json') @{ question = $true; native_history = $true }
    }
    @{
        Root = $root; State = $state; Profile = $profile; Workspace = $workspace
        Install = $install; Question = [bool]$Question; Provider = $Provider
    }
}

function Isolated-ProcessInfo($Program, $Fixture) {
    $start = [Diagnostics.ProcessStartInfo]::new($Program)
    $start.UseShellExecute = $false
    $start.WorkingDirectory = $Fixture.Workspace
    $start.EnvironmentVariables.Clear()
    $system = Join-Path $env:SystemRoot 'System32'
    $git = (Get-Command git.exe -CommandType Application).Source
    $start.EnvironmentVariables['PATH'] = @((Split-Path -Parent $git), $system, "$system\WindowsPowerShell\v1.0") -join ';'
    foreach ($name in 'SystemRoot', 'WINDIR', 'PATHEXT', 'OS', 'PROCESSOR_ARCHITECTURE', 'NUMBER_OF_PROCESSORS') {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ($value) { $start.EnvironmentVariables[$name] = $value }
    }
    foreach ($name in 'HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'CONCORDIA_TEST_HOME', 'CONCORDIA_CODEX_HOME_BASE') {
        $start.EnvironmentVariables[$name] = $Fixture.Profile
    }
    foreach ($name in 'CONCORDIA_CODEX_BIN', 'CONCORDIA_CLAUDE_BIN', 'CONCORDIA_AGENT_BIN', 'CONCORDIA_GROK_BIN', 'CONCORDIA_OPENCODE_BIN') {
        $start.EnvironmentVariables[$name] = $fake
    }
    $isolated = @{
        COMSPEC = "$system\cmd.exe"; TEMP = $scratch; TMP = $scratch
        GIT_CONFIG_NOSYSTEM = '1'; GIT_CONFIG_GLOBAL = 'NUL'; GIT_OPTIONAL_LOCKS = '0'
        GIT_CEILING_DIRECTORIES = $Fixture.Root
        CONCORDIA_TEST_GUARD = '1'; CONCORDIA_STATE_DIR = $Fixture.State
        CONCORDIA_INSTALL_DIR = $Fixture.Install; CONCORDIA_UPDATE_REPO = 'offline'
        FAKE_PROVIDER_MODE = 'interaction'; FAKE_PROVIDER_AUDIT = '1'; FAKE_PROVIDER_GATE = Join-Path $Fixture.Root 'gate'
        FAKE_PROVIDER_JOURNAL = Join-Path $Fixture.Root 'received.jsonl'
        FAKE_FILE_LAUNCH_JOURNAL = Join-Path $Fixture.Root 'file-launch.json'
    }
    if ($Fixture.Question) { $isolated.FAKE_PROVIDER_NATIVE_CODEX = '1' }
    foreach ($entry in $isolated.GetEnumerator()) { $start.EnvironmentVariables[$entry.Key] = $entry.Value }
    $start
}

function Initialize-WorkerRepo($Fixture) {
    $git = (Get-Command git.exe -CommandType Application).Source
    foreach ($arguments in @('init --quiet', 'add -- parser.rs', '-c user.name=Audit -c user.email=audit@example.invalid -c commit.gpgSign=false commit --quiet -m fixture')) {
        $start = Isolated-ProcessInfo $git $Fixture
        $start.Arguments = $arguments
        $start.CreateNoWindow = $true
        $process = [Diagnostics.Process]::Start($start)
        try {
            if (-not $process.WaitForExit(10000)) { $process.Kill(); throw "Fixture git $arguments timed out." }
            if ($process.ExitCode -ne 0) { throw "Fixture git $arguments failed." }
        } finally { $process.Dispose() }
    }
}

function Start-AuditWindow($Fixture) {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    $listener.Stop()
    $token = 'audit-' + [guid]::NewGuid().ToString('N')
    $start = Isolated-ProcessInfo $exe $Fixture
    $start.EnvironmentVariables['CONCORDIA_TEST_CONTROL'] = "127.0.0.1:$port;$token"
    $script:session = @{
        Process = [Diagnostics.Process]::Start($start); Window = [IntPtr]::Zero
        Port = $port; Token = $token; Fixture = $Fixture; Client = $null
    }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt 10) {
        if ($session.Process.HasExited) { throw "Audit window exited with code $($session.Process.ExitCode)." }
        $client = [Net.Sockets.TcpClient]::new()
        try { $client.Connect([Net.IPAddress]::Loopback, $port) }
        catch { $client.Dispose(); Start-Sleep -Milliseconds 100; continue }
        $client.ReceiveTimeout = 4000
        $session.Client = $client
        $stream = $client.GetStream()
        $session.Writer = [IO.StreamWriter]::new($stream, $utf8)
        $session.Writer.NewLine = "`n"
        $session.Writer.AutoFlush = $true
        $session.Reader = [IO.StreamReader]::new($stream, $utf8)
        $session.Window = [ConcordiaAuditWindow]::Find([uint32]$session.Process.Id)
        if ($session.Window -eq [IntPtr]::Zero) { throw 'The control listener has no visible native window.' }
        Wait-Grid 'concordia' | Out-Null
        return
    }
    throw 'The native control listener did not start.'
}

function Stop-AuditWindow {
    if (-not $session) { return }
    try {
        if ($session.Client) { $session.Client.Dispose() }
        if (-not $session.Process.HasExited) {
            if ($session.Window -ne [IntPtr]::Zero) {
                [void][ConcordiaAuditWindow]::PostMessage($session.Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
            }
            if (-not $session.Process.WaitForExit(1000)) { $session.Process.Kill(); $session.Process.WaitForExit() }
        }
    } finally {
        $session.Process.Dispose()
        $script:session = $null
    }
}

function Request($Body) {
    $Body.token = $session.Token
    $session.Writer.WriteLine((ConvertTo-Json -InputObject $Body -Compress))
    $line = $session.Reader.ReadLine()
    if (-not $line) { throw 'The native control connection closed.' }
    $result = ConvertFrom-Json -InputObject $line
    if (-not $result.ok) { throw "Native control $($Body.op) failed: $($result.error)" }
    $result
}

function Snapshot { Request @{ op = 'snapshot' } }

function Wait-Grid($Pattern, [int]$Seconds = 8) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Snapshot
        $evidence = @($snapshot.grid) + @($snapshot.overlays | ForEach-Object { "Overlay: $_" })
        if (($evidence -join "`n") -match $Pattern) { return $snapshot }
        Start-Sleep -Milliseconds 70
    } while ($timer.Elapsed.TotalSeconds -lt $Seconds)
    Write-Utf8 (Join-Path $scratch ('miss-{0:D2}-{1}x{2}.txt' -f $sequence, $snapshot.columns, $snapshot.rows)) ($evidence -join "`n")
    throw "Grid never showed /$Pattern/."
}

function Press($Code, [string[]]$Modifiers = @()) {
    Request @{ op = 'key'; code = $Code; modifiers = $Modifiers } | Out-Null
    Start-Sleep -Milliseconds 75
}

function Type-Text($Value) {
    Request @{ op = 'text'; text = $Value } | Out-Null
    Start-Sleep -Milliseconds 100
}

function Submit($Value) { Type-Text $Value; Press Enter }

function Click($X, $Y) {
    foreach ($kind in 'down', 'up') { Request @{ op = 'mouse'; kind = $kind; x = $X; y = $Y } | Out-Null }
    Start-Sleep -Milliseconds 100
}

function Click-Grid($Pattern, [switch]$Last) {
    $grid = (Snapshot).grid
    $rows = @(0..($grid.Count - 1))
    if ($Last) { [Array]::Reverse($rows) }
    foreach ($y in $rows) {
        $hit = [regex]::Match($grid[$y], $Pattern)
        if ($hit.Success) { Click $hit.Index $y; return }
    }
    throw "Click target /$Pattern/ is absent from the grid."
}

function Resize-Grid($Columns, $Rows) {
    Request @{ op = 'resize'; columns = $Columns; rows = $Rows } | Out-Null
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Snapshot
        if ($snapshot.columns -eq $Columns -and $snapshot.rows -eq $Rows -and $snapshot.grid[-1].Trim()) { return }
        Start-Sleep -Milliseconds 70
    } while ($timer.Elapsed.TotalSeconds -lt 8)
    throw "No fully drawn ${Columns}x${Rows} grid. Native buffer is $($snapshot.columns)x$($snapshot.rows)."
}

function Save-Shot($Number, $Name, $Columns, $Rows, $Route, $Pattern) {
    $base = '{0:D2}-{1}-{2}x{3}' -f $Number, $Name, $Columns, $Rows
    $stagedPng = Join-Path $scratch "$base.png"
    foreach ($attempt in 1..3) {
        $snapshot = Wait-Grid $Pattern
        if ($snapshot.columns -ne $Columns -or $snapshot.rows -ne $Rows) { throw 'Grid size changed before capture.' }
        try { [ConcordiaAuditWindow]::Save($session.Window, $stagedPng); break }
        catch { if ($attempt -eq 3) { throw }; Start-Sleep -Milliseconds 100 }
    }
    Write-Utf8 (Join-Path $output "$base.txt") (($snapshot.grid -join "`n") + "`n")
    Move-Item -LiteralPath $stagedPng -Destination (Join-Path $output "$base.png") -Force
    $shots.Add(('- [{0}.png]({0}.png) / [grid]({0}.txt). {1}' -f $base, $Route))
}

function Add-Unreachable($Name, $Size, $Reason) {
    $unreachable.Add("- **$Name ($Size)** $Reason")
    Write-Warning "$Name ($Size) $Reason"
}

function Capture($Name, $Route, $Pattern, [scriptblock]$Reach = {}, [scriptblock]$Leave = {}) {
    $script:sequence++
    Write-Host "Capturing $Name"
    try {
        Resize-Grid 160 45
        & $Reach
        foreach ($size in $sizes) {
            $columns, $rows = $size
            try {
                Resize-Grid $columns $rows
                Save-Shot $sequence $Name $columns $rows $Route $Pattern
            } catch { Add-Unreachable $Name "${columns}x${rows}" $_.Exception.Message }
        }
    } catch { Add-Unreachable $Name 'all sizes' $_.Exception.Message }
    finally {
        try { & $Leave }
        catch { Write-Warning "Could not leave $Name cleanly. $($_.Exception.Message)" }
    }
}

function Picker($Name, $Command, $Pattern) {
    Capture $Name ('Type `{0}`, then Enter.' -f $Command) $Pattern { Submit $Command } { Press Esc }
}

function Window-State($Minimized) {
    $command = if ($Minimized) { 6 } else { 9 }
    if (-not [ConcordiaAuditWindow]::ShowWindowAsync($session.Window, $command)) { throw 'ShowWindowAsync failed.' }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        if ([ConcordiaAuditWindow]::IsIconic($session.Window) -eq $Minimized) { return }
        Start-Sleep -Milliseconds 50
    } while ($timer.Elapsed.TotalSeconds -lt 3)
    throw "The window's minimized state did not become $Minimized."
}

try {
    Start-AuditWindow (New-Fixture 'welcome' '')
    Capture 'home-welcome' 'Launch Concordia with an empty account registry.' 'Welcome to Concordia'
    Stop-AuditWindow

    Start-AuditWindow (New-Fixture 'chat' 'codex')
    Capture 'home-chats-usage' 'Launch with the seeded account registry and chats. Press Left to focus Home.' '(?s)(?=.*5h)(?=.*Review parser edge cases)' { Press Left }
    Capture 'home-hidden-chats' 'On Home, press H to show hidden chats, click the scratch group and press Enter to expand it.' 'Check a scratch example' {
        Press h
        Click-Grid 'concordia-test-scratch'
        Press Enter
    }
    Press Right
    Capture 'chat-transcript' 'Press Right from Home. Type `Audit the parser sample` and press Enter.' '(?s)(?=.*Audit the parser sample)(?=.*Checking empty input)(?=.*Ran command)(?=.*Edit File parser\.rs)(?=.*Parser review)(?=.*parser guide)(?=.*let tokens)' {
        Submit 'Audit the parser sample'
        Wait-Grid 'Parser review' | Out-Null
    }

    Capture 'header-account' 'Click the Codex account chip.' '(?m)^Overlay: providers and accounts$' { Click-Grid '\bCodex\b' } { Press Esc }
    Capture 'header-model' 'Click the GPT-5.6 Luna model chip.' 'Fixture model' { Click-Grid 'GPT-5\.6 Luna' } { Press Esc }
    Capture 'header-effort' 'Click the Effort chip.' '(?m)^Overlay: .*Choose effort$' { Click-Grid 'Effort low' } { Press Esc }
    Capture 'footer-permissions' 'Click the permissions chip at the bottom right.' '(?m)^Overlay: permissions' { $frame = Snapshot; Click ($frame.columns - 4) ($frame.rows - 1) } { Press Esc }

    Picker 'picker-use' '/use' '(?m)^Overlay: (accounts|/use)'
    Picker 'picker-use-add' '/use add' '(?m)^Overlay: add account'
    Picker 'picker-use-remove' '/use remove' '(?m)^Overlay: remove which account'
    Picker 'picker-usage' '/usage' 'Chat input is paused'
    Picker 'picker-resume' '/resume' 'native session|Review parser edge cases'
    Picker 'picker-model' '/model' 'Fixture model'
    Picker 'picker-mode' '/mode' '(?m)^Overlay: permissions'
    Picker 'picker-doctor' '/doctor' '(?m)^Overlay: doctor'
    Picker 'picker-update' '/update' 'source checkout|newest build|up to date|same build'
    Picker 'picker-harness' '/harness' 'No integrations connected|New local integration|Manage'
    Picker 'picker-open' '/open' '(?m)^Overlay: /open|^Overlay: Open'

    foreach ($missing in @(
        @('/workers', 'This branch removes the slash command. The live worker sidebar is reached through managed-worktree delegation.'),
        @('/crew', 'This branch removes the slash command. Ctrl+G opens the Agents list.'),
        @('/memory', 'The memory handler has no slash-command route in this build.'),
        @('/context', 'The command is absent from the Concordia picker registry.'),
        @('/integrations', 'The integrations picker is registered as /harness and captured there.'),
        @('/search', 'The slash picker is absent. Activity detail offers its own search and is captured below.'),
        @('/preview', 'The file preview picker is registered as /open and captured there.')
    )) {
        Add-Unreachable $missing[0] 'all sizes' $missing[1]
    }

    Capture 'config-general' 'Type `/config` and press Enter.' 'Response usage line' { Submit '/config' }
    Capture 'config-permissions' 'On Settings > General, press Enter on Permissions.' '(?m)^Overlay: permissions' { Press Enter } { Press Esc }
    Capture 'config-filter' 'On Settings > General, type `permissions`.' 'Filter +permissions' {
        foreach ($letter in 'permissions'.ToCharArray()) { Press ([string]$letter) }
    } { Press Esc }
    foreach ($tab in @(
        @('appearance', 'Appearance', 'Theme and colors'),
        @('models-and-roles', 'Models and roles', 'Fallback model per role'),
        @('accounts', 'Accounts', 'When a limit is reached'),
        @('updates', 'Updates', 'New versions'),
        @('advanced', 'Advanced', 'Native provider controls')
    )) {
        Capture "config-$($tab[0])" ('In `/config`, press Right to reach {0}.' -f $tab[1]) $tab[2] { Press Right }
    }
    Press Esc

    Capture 'palette-slash' 'Type `/` in the composer.' '(?s)(?=.*commands)(?=.*> /[ ]*[^\S\n]*[^\w/])' { Press '/' }
    Capture 'palette-us' 'With the slash palette open, type `us`.' '(?s)(?=.*commands)(?=.*> /us\W)' { Press u; Press s } { Press a @('CTRL'); Press Backspace }
    Capture 'shortcuts' 'Press F1.' 'Shortcuts' { Press F1 } { Press Esc }
    Capture 'paused-composer' 'Type `/usage` and press Enter.' 'message \(paused\)|Chat input is paused' { Submit '/usage' } { Press Esc }
    Capture 'activity-search' 'Press Ctrl+E on the transcript, then type `parser` in activity detail.' 'Typing searches output' { Press e @('CTRL'); Type-Text 'parser' } { Press Esc }
    Capture 'file-pane' 'Type `/open parser.rs` and press Enter.' 'input\.split_whitespace' { Submit '/open parser.rs' } { Press Esc; Submit '/close 1' }
    Capture 'terminal-pane' 'Type `/terminal open` and press Enter. At the PowerShell prompt, type `Write-Output AUDIT_TERMINAL` and press Enter.' '(?m)^AUDIT_TERMINAL\s*$' {
        Submit '/terminal open'
        Wait-Grid 'PS [A-Z]:|F11 chat' | Out-Null
        Submit 'Write-Output AUDIT_TERMINAL'
    } { Press w @('CTRL', 'SHIFT') }

    Capture 'agents-panel' 'Type `[agents] [agents-six] Review the parser` and press Enter. After the fake reply, press Ctrl+G.' '(?m)^Overlay: Agents$' {
        Submit '[agents] [agents-six] Review the parser'
        Wait-Grid 'Ctrl\+G agents' 15 | Out-Null
        Press g @('CTRL')
    } { Press Esc }
    Capture 'tabs' 'Press Ctrl+T twice to create two more tabs.' '(?m)^.*\b1\b.*\b2\b.*\b3\b' { Press t @('CTRL'); Press t @('CTRL') }
    Capture 'tab-being-renamed' 'Type `/rename`, press Enter, choose Set a title, then type `Parser follow-up`.' 'Parser follow-up' {
        Submit '/rename'
        Wait-Grid 'Set a title' | Out-Null
        Press Enter
        Type-Text 'Parser follow-up'
    } { Press Esc }

    Capture 'window-restored' 'Minimize the Concordia window; verify it is minimized; restore it and capture its client area.' 'concordia' {
        Window-State $true
        Window-State $false
    }
    foreach ($step in @(@('start', 160, 45), @('shrink', 80, 24), @('expand', 160, 45))) {
        $sequence++
        $label, $columns, $rows = $step
        try {
            Resize-Grid $columns $rows
            Save-Shot $sequence "window-resize-$label" $columns $rows "Resize the restored native window to ${columns}x${rows} in the 160x45, 80x24, 160x45 sequence." 'concordia'
        } catch { Add-Unreachable "window-resize-$label" "${columns}x${rows}" $_.Exception.Message }
    }
    Stop-AuditWindow

    $worker = New-Fixture 'worker' 'codex'
    try {
        Initialize-WorkerRepo $worker
        Start-AuditWindow $worker
        Capture 'worker-side-panel' 'Run `/native claude off`, `/worktree create HEAD`, then `/worktree delegate ID claude [wait] Review the parser` with the new workspace ID.' 'live workers' {
            Submit '/native claude off'
            Submit '/worktree create HEAD'
            $frame = Wait-Grid 'Managed workspace \S+ revision \d+: Ready' 10
            $id = [regex]::Match(($frame.grid -join "`n"), 'Managed workspace (\S+) revision').Groups[1].Value
            Submit "/worktree delegate $id claude [wait] Review the parser"
            Wait-Grid 'live workers' 10 | Out-Null
        }
    } catch { Add-Unreachable 'worker-side-panel' 'all sizes' $_.Exception.Message }
    finally { Stop-AuditWindow }

    Start-AuditWindow (New-Fixture 'question' 'codex' -Question)
    Capture 'ask-question' 'Run `/native codex on`, then send `Inspect the synthetic app-server request` to the fake Codex app-server.' '1\. Direct|Choose the route' {
        Submit '/native codex on'
        Submit 'Inspect the synthetic app-server request'
        Wait-Grid 'Choose the route' 8 | Out-Null
    }
    Stop-AuditWindow

    Start-AuditWindow (New-Fixture 'queue' 'claude')
    Capture 'busy-turn' 'Run `/native claude off`, then send `[wait] [fail] deliberate failure` to fake Claude.' 'Run command|Tab queue|Esc cancel' {
        Submit '/native claude off'
        Submit '[wait] [fail] deliberate failure'
        Wait-Grid 'Run command' | Out-Null
    }
    Capture 'queued-messages' 'While the turn is busy, type `Review parser tests` and press Tab. Repeat with `Draft release notes`.' '(?s)(?=.*Review parser tests)(?=.*Draft release notes)' {
        foreach ($message in 'Review parser tests', 'Draft release notes') { Type-Text $message; Press Tab }
    }
    Capture 'ctrl-k-options' 'Wait for the one-minute quiet-turn notice from the gated fake turn, then press Ctrl+K.' '(?m)^ *\u250c turn is quiet ' {
        Wait-Grid 'No output from' 75 | Out-Null
        Press k @('CTRL')
    } { Press Esc }
    Capture 'failed-turn' 'Release the fake provider gate to finish the active turn with exit code 3.' 'Turn failed\.' {
        Write-Utf8 (Join-Path $session.Fixture.Root 'gate') ''
        Wait-Grid 'Turn failed\.' | Out-Null
    }
    Capture 'paused-queue' 'Keep the queued items after the fake turn exits with code 3.' 'queue paused'
} finally {
    Stop-AuditWindow
    $index = @('# Concordia native-window audit', '', 'PNG files are native client-area captures. Each grid comes from the same window at the indicated size.', '')
    $index += $shots.ToArray()
    $index += @('', '## Unreachable', '')
    if ($unreachable.Count) { $index += $unreachable.ToArray() } else { $index += 'None.' }
    Write-Utf8 (Join-Path $output 'index.md') (($index -join "`n") + "`n")
    Write-Host "$($shots.Count) PNGs written."
}
