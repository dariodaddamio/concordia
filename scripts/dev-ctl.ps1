<#
Read and drive the window started by scripts/dev-loop.ps1.

  dev-ctl.ps1 screen                      print the current screen as text
  dev-ctl.ps1 snap <name>                 save <name>.txt and <name>.png under the dev folder
  dev-ctl.ps1 text "<text>"               type text (no Enter)
  dev-ctl.ps1 send "<text>"               type text and press Enter
  dev-ctl.ps1 key <Key> [CONTROL,SHIFT]   press a key, e.g. key o CONTROL
  dev-ctl.ps1 click <x> <y>               left click at a cell
  dev-ctl.ps1 resize <columns> <rows>     resize the window grid
  dev-ctl.ps1 timing                      frame and paint timings since the last call

Each command reconnects, so it keeps working across the window's automatic restarts.
#>
param([Parameter(Mandatory)][string]$Command, [string]$A = '', [string]$B = '')
$ErrorActionPreference = 'Stop'
$dev = Join-Path $env:TEMP 'concordia-dev'
$spec = (Get-Content (Join-Path $dev 'state\dev-control.txt') -Raw).Trim()
$address, $token = $spec -split ';', 2
$hostName, $port = $address -split ':'
$client = $null
foreach ($i in 1..100) { try { $client = [System.Net.Sockets.TcpClient]::new($hostName, [int]$port); break } catch { Start-Sleep -Milliseconds 100 } }
if (-not $client) { throw 'dev window is not listening; is scripts/dev-loop.ps1 running?' }
$stream = $client.GetStream()
$writer = [System.IO.StreamWriter]::new($stream); $writer.NewLine = "`n"; $writer.AutoFlush = $true
$reader = [System.IO.StreamReader]::new($stream)
function Ask($o) { $o.token = $token; $writer.WriteLine(($o | ConvertTo-Json -Compress)); $reader.ReadLine() | ConvertFrom-Json }
function Screen { ((Ask @{ op = 'snapshot' }).grid) -join "`n" }

Add-Type -ReferencedAssemblies System.Drawing @'
using System; using System.Runtime.InteropServices; using System.Drawing;
public static class DevCap {
  [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static IntPtr Find(uint pid) { SetProcessDPIAware(); IntPtr best = IntPtr.Zero; int area = 0;
    EnumWindows((h, l) => { uint q; GetWindowThreadProcessId(h, out q);
      if (q == pid) { RECT r; GetClientRect(h, out r); int a = (r.R - r.L) * (r.B - r.T); if (a > area) { area = a; best = h; } } return true; }, IntPtr.Zero);
    return best; }
  public static bool Save(IntPtr h, string path) { RECT r; GetClientRect(h, out r); int w = r.R - r.L, ht = r.B - r.T; if (w <= 0 || ht <= 0) return false;
    using (var bmp = new Bitmap(w, ht)) { using (var g = Graphics.FromImage(bmp)) { IntPtr dc = g.GetHdc(); bool ok = PrintWindow(h, dc, 3); g.ReleaseHdc(dc); if (!ok) return false; }
      bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png); } return true; }
}
'@

try {
    switch ($Command) {
        'screen' { Screen }
        'snap' {
            $out = Join-Path $dev 'snaps'; New-Item -ItemType Directory -Force $out | Out-Null
            Screen | Set-Content -Encoding UTF8 (Join-Path $out "$A.txt")
            $bin = Join-Path $dev 'bin'
            $p = Get-Process concordia -ErrorAction SilentlyContinue | Where-Object { try { $_.Path.StartsWith($bin, [StringComparison]::OrdinalIgnoreCase) } catch { $false } } | Select-Object -First 1
            if ($p) { [void][DevCap]::Save([DevCap]::Find([uint32]$p.Id), (Join-Path $out "$A.png")) }
            Join-Path $out "$A.png"
        }
        'text' { Ask @{ op = 'text'; text = $A } | Out-Null }
        'send' { Ask @{ op = 'text'; text = $A } | Out-Null; Start-Sleep -Milliseconds 200; Ask @{ op = 'key'; code = 'Enter' } | Out-Null }
        'key' { $o = @{ op = 'key'; code = $A }; if ($B) { $o.modifiers = @($B -split ',') }; Ask $o | Out-Null }
        'click' { Ask @{ op = 'mouse'; kind = 'down'; x = [int]$A; y = [int]$B } | Out-Null; Ask @{ op = 'mouse'; kind = 'up'; x = [int]$A; y = [int]$B } | Out-Null }
        'resize' { Ask @{ op = 'resize'; columns = [int]$A; rows = [int]$B } | ConvertTo-Json -Compress }
        'timing' { Ask @{ op = 'timing' } | ConvertTo-Json -Compress -Depth 4 }
        default { throw "unknown command $Command" }
    }
} finally { $client.Close() }
