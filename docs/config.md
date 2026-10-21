# Configuration and custom commands

## Terminal editor and Markdown preview

Click an underlined file reference in a sent user message or assistant reply to open it in a
separate WezTerm tab/window; a drag still selects text. Alt+click previews instead. Paths can
include a line/column (`src/main.rs:12:3`) or Markdown line fragment (`#L12`). Markdown labels
keep the explicit destination. `/open PATH` and `/preview PATH` are also available. Missing
paths or launch failures are shown in Concordia. References never run as shell commands.

Concordia finds an installed `nvim`, `hx` or `vim`, or uses your explicit executable and argv:

```toml
[editor]
program = "nvim"
args = ["+{line}", "--", "{file}"]
```

`{file}`, `{line}` and `{column}` expand within individual arguments; the executable is one
path, not a shell command string. If `{file}` is absent the file is appended as one argument.
`/preview` launches `concordia preview FILE`, a read-only Markdown/text viewer with scrolling,
Home/End and q/Esc to close, requiring no extra preview package. Input is UTF-8 and capped at
2 MiB. Separate terminals require WezTerm; `CONCORDIA_WEZTERM_BIN` can name its executable.

## Native chat access from the CLI

`concordia chat list --json` lists recent native chats across the user's discovered/registered
accounts, labeled with provider, account and session ID. Filter with `--account ID` or
`--provider codex|claude|cursor`; `--limit N` is capped at 1000. `chat search TEXT` searches
preview/ID metadata only. `chat read SESSION --account ID` reads Codex/Claude user/assistant
text with optional `--tail N`, `--grep TEXT`, and `--max-chars N`. It does not select an account,
resume a session, copy credentials, or transfer context to a provider. Explicit reads resolve in
the named account rather than the newest-list window. Discovery caps at 20,000 entries, 5,000
files and 32 accounts; listing summarizes at most 1,000 records, reading at most 64 KiB per summary.
Native UUID reads filter filenames before summarizing. A scan that cannot establish a unique
match reports its bound rather than claiming absence. List JSON preserves exact identity fields,
clips display metadata and caps output at 5 MiB. Reads use at most the last 8 MiB of a transcript. Tool records are
omitted. Cursor metadata can be listed; its native transcript reader is not implemented.

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
persist_history = true   # keep sent prompts in <state>/history.jsonl so Up works after a restart
stats = false            # footer: mem 41 MB - cpu 1% (and the provider processes)

[threads]
enabled = false          # record conversations to <state>/threads (stores prompts)
map_chars = 1600         # size of the [concordia map] block, 200..=8000
awareness = true         # tell the CLIs in Concordia-created homes about [concordia map] (docs/mcp.md)
mcp = false              # register `concordia mcp` in Concordia-created homes (opt-in, costs tokens)

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
| `ui.persist_history` | `true` | Stores sent prompts locally (last 500); `false` keeps history in memory only |
| `ui.stats` | `false` | Footer memory and CPU for Concordia and the agents it started (`/stats` toggles it) |
| `threads.enabled` | `false` | Opt-in recording; stores prompts on disk |
| `threads.map_chars` | `1600` | Out of range is clamped to 200..8000 |
| `threads.awareness` | `true` | One short block in `CLAUDE.md` / `AGENTS.md` of homes Concordia created; `false` removes it |
| `threads.mcp` | `false` | Registers the `concordia mcp` server in those homes; `false` removes the entry (see [mcp.md](mcp.md)) |
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
