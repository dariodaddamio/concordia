# Terminal host spike (P1)

Minimal launchers only. Host adapter surface for the orchestrator: spawn panes, record explicit IDs where the host returns them, set titles/labels. Does not remap Escape/Ctrl-C or replace the emulator.

## Prerequisites

| Host | This machine (2026-10-14) | Install (user step; not run by this spike) |
| --- | --- | --- |
| Windows Terminal (`wt`) | Present — Appx `Microsoft.WindowsTerminal` **1.24.12741.0** | Already installed |
| WezTerm (`wezterm`) | **Missing** | See below |

### Install WezTerm (required to validate preferred host)

Do **not** let agents install silently. Recommended user steps on Windows:

1. Download the Windows installer or portable zip from https://wezterm.org/install/windows.html  
   or install via a package manager you already trust, for example:
   - `winget install wez.wezterm` (when `winget` is available)
   - `scoop install wezterm` (when Scoop is available)
2. Confirm: `wezterm --version` and `wezterm cli --help`
3. Re-run `.\launch-wezterm.ps1` from this directory.

## Scripts

| Script | Host | What it demonstrates |
| --- | --- | --- |
| `launch-wt.ps1` | Windows Terminal | PowerShell default shell; multi-pane layout; tab titles as labels; sequential splits (no stable pane-id API) |
| `launch-wezterm.ps1` | WezTerm | Explicit `--pane-id` splits; records pane IDs from CLI stdout; tab titles; optional isolated config dir |
| `wezterm/wezterm.lua` | WezTerm | Isolated config: PowerShell default prog; unix domain named `orch` for persistence design |

## Manual checklist (keyboard / mouse)

After a launcher opens panes, verify in each agent pane (not the control pane):

1. Paste (Ctrl-V / right-click) inserts text into the shell, not the host chrome.
2. Escape reaches the running program (does **not** get swallowed by a host keymap — avoid agentmux-style Escape capture).
3. Ctrl-C interrupts the foreground process.
4. Mouse select / click-focus works; window resize redistributes panes.
5. Titles/labels remain visible without requiring key-stealing overlays.

## Evidence markers

Launchers write optional marker files under `%TEMP%\orch-terminal-spike\` so spawn success can be checked without reading TTY output.
