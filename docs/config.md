# Configuration and custom commands

Concordia reads two optional things you write by hand, both under the state directory
(`%LOCALAPPDATA%\concordia`, or `CONCORDIA_STATE_DIR`). It never writes either of them.

## `config.toml`

`<state>\config.toml` (override the path with `CONCORDIA_CONFIG`). A missing file or key means the
default. A malformed file is ignored (defaults) and reported by `concordia doctor` with its line and
column; unknown keys and invalid values are warnings too (invalid values fall back to the default or are
clamped). Nothing here is ever fatal.

```toml
[ui]
token_line = true        # per-turn "in 12.3k out 1.4k 8.2s" under each reply
tips = true              # a rotating tip while idle

[threads]
enabled = false          # record conversations to <state>/threads (stores prompts)
map_chars = 1600         # size of the [concordia map] block, 200..=8000

[mode]
default = "ask"          # plan | ask | workspace | full

[delegation]
protocol = false         # let the orchestrator propose delegations (you still approve)
max_workers = 3          # 1..=3
```

| Key | Default | Notes |
| --- | --- | --- |
| `ui.token_line` | `true` | Dim usage line after each reply |
| `ui.tips` | `true` | Tips while the prompt is idle |
| `threads.enabled` | `false` | Opt-in recording; stores prompts on disk |
| `threads.map_chars` | `1600` | Out of range is clamped to 200..8000 |
| `mode.default` | `"ask"` | Anything else falls back to `ask` |
| `delegation.protocol` | `false` | Same switch as `/delegate protocol on|off` |
| `delegation.max_workers` | `3` | 0 becomes 1, above 3 becomes 3 |

### Precedence: store vs config

Config supplies defaults only. `store.json` holds what you did inside Concordia, and a value saved
there wins:

- Permission mode: `[mode] default` applies only until a mode has been saved (first launch). After
  that `/mode`, `/trust` and Shift+Tab own it.
- Delegation protocol: `/delegate protocol on|off` writes the store; until you use it, the config value applies.
- `max_workers` is a cap on `/workers add`; it is not stored.

Read the effective values with the `config` and `commands` rows of `concordia doctor`.

## Custom commands

`<state>\commands\<name>.md` becomes `/<name>`. Names are `[a-z0-9-]`, 1 to 32 characters; at most 64
files of at most 16 KB are read (others are skipped with a doctor warning). Front matter is optional:

```
---
description: review the staged diff
---
Review the staged diff for bugs and missing tests. Focus: $ARGS
```

`/review the parser` shows as typed and sends the body with `$ARGS` replaced by `the parser`, as an
ordinary prompt (history, trust mode and providers behave as for any message). If the template has no
`$ARGS`, typed text is appended as a new paragraph. Only `$ARGS` is replaced: no shell, no includes, no
environment expansion. Commands appear in the `/` palette with a dim `custom` marker. Built-in commands
always win a name clash; `concordia doctor` lists clashes. Files are scanned when Concordia starts.
