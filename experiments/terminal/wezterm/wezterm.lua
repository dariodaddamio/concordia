-- Minimal config for WezTerm 20240203 (portable).
-- Spawn the absolute exe path, never a bare `concordia`: a stale shim earlier on PATH could win.
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
local act = wezterm.action

config.default_prog = { 'powershell.exe', '-NoLogo' }

local function file_exists(path)
  local f = io.open(path, 'r')
  if f then
    f:close()
    return true
  end
  return false
end

local function read_first_line(path)
  local f = io.open(path, 'r')
  if not f then
    return nil
  end
  local line = f:read('*l')
  f:close()
  if line then
    line = line:gsub('%s+$', '')
  end
  if line and #line > 0 then
    return line
  end
  return nil
end

local function resolve_concordia()
  local localapp = os.getenv('LOCALAPPDATA') or ''
  -- /update + install scripts write this pointer — always prefer it.
  local current = read_first_line(localapp .. '\\concordia\\bin\\current')
  if current and file_exists(current) then
    return current
  end
  local from_env = os.getenv('CONCORDIA_EXE')
  if from_env and from_env ~= '' and file_exists(from_env) then
    return from_env
  end
  local installed = localapp .. '\\concordia\\bin\\concordia.exe'
  if file_exists(installed) then
    return installed
  end
  local here = debug.getinfo(1, 'S').source
  if here:sub(1, 1) == '@' then
    here = here:sub(2)
  end
  local root = here:gsub('[/\\]experiments[/\\]terminal[/\\]wezterm[/\\]wezterm%.lua$', '')
  local candidates = {
    root .. '\\target\\release\\concordia.exe',
    root .. '\\target\\concordia-update\\release\\concordia.exe',
    root .. '\\target\\concordia-fix\\release\\concordia.exe',
    root .. '\\target\\debug\\concordia.exe',
  }
  for _, p in ipairs(candidates) do
    if file_exists(p) then
      return p
    end
  end
  -- Last resort: still never bare `concordia` (Notepad). Fail visibly via missing exe.
  return installed
end

local concordia_exe = resolve_concordia()
local concordia_spawn = { args = { concordia_exe }, domain = 'CurrentPaneDomain' }

local function looks_like_shell_title(title)
  if not title or title == '' then
    return true
  end
  local t = title:lower()
  return t:find('powershell')
    or t:find('pwsh')
    or t:find('cmd%.exe')
    or t:find('windows terminal')
    or t == 'wezterm'
end

wezterm.on('format-tab-title', function(tab)
  local title = tab.active_pane.title or ''
  if looks_like_shell_title(title) then
    title = 'concordia'
  end
  return { { Text = ' ' .. title .. ' ' } }
end)

wezterm.on('format-window-title', function(tab)
  local title = tab.active_pane.title or ''
  if looks_like_shell_title(title) then
    return 'concordia'
  end
  return title
end)

config.keys = {
  {
    key = 'D',
    mods = 'CTRL|SHIFT',
    action = act.SplitHorizontal(concordia_spawn),
  },
  {
    key = 'E',
    mods = 'CTRL|SHIFT',
    action = act.SplitVertical(concordia_spawn),
  },
  {
    key = 'T',
    mods = 'CTRL|SHIFT',
    action = act.SpawnCommandInNewTab(concordia_spawn),
  },
  {
    key = 'LeftArrow',
    mods = 'CTRL|SHIFT',
    action = act.ActivatePaneDirection 'Left',
  },
  {
    key = 'RightArrow',
    mods = 'CTRL|SHIFT',
    action = act.ActivatePaneDirection 'Right',
  },
  {
    key = 'UpArrow',
    mods = 'CTRL|SHIFT',
    action = act.ActivatePaneDirection 'Up',
  },
  {
    key = 'DownArrow',
    mods = 'CTRL|SHIFT',
    action = act.ActivatePaneDirection 'Down',
  },
}

config.hide_tab_bar_if_only_one_tab = false
config.tab_bar_at_bottom = false

return config
