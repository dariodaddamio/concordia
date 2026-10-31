# Configuration and custom commands

## Usage-limit continuation

The usage handoff row in `/use` pauses or automatically continues when usage runs out. A paused
request offers an account picker. Persistent defaults:

```toml
[continuation]
mode = "manual" # manual | automatic
accounts = ["codex-2", "claude-1"] # choose ids shown by /use, in preferred order
```

Only a provider error explicitly reporting an exhausted usage limit triggers handoff; ordinary
errors, cancellation and model prose do not. Each account is tried once, with at most three
automatic handoffs per request. No configured target means pause. Unknown capacity is not
treated as free capacity: a configured account may itself report a limit and exhaust its one
attempt. A draft, open picker, background job, queued edit or uncertain tool completion blocks
automatic continuation. Inspect uncertain results before continuing.

Handoff keeps the conversation and title, starts a new native session in the selected account's
home and sends the bounded conversation map with an instruction to reconcile completed actions.
Native tool/approval state is not transferred. Queued prompts remain paused for explicit resume.
There are no quota probes or summarizer calls. Runtime mode changes apply to this window;
edit config.toml to choose the default for future launches.

## Titles, resume and agent reports

Session titles are generated deterministically from the first substantive request, without a
model call. They are shown in the header, terminal title and resume picker, and saved under
`<state>/.ao/session-titles/` independently of `[threads].enabled`. Each record is bound to the
provider, account, normalized home and native session id; provider-owned files are not rewritten.

`/resume` loads up to 200 recent Codex/Claude user/assistant messages within 1 MiB of visible
text, reading at most an 8 MiB native tail. Loading runs off the input/render thread; the draft
is retained and sending waits for completion. A truncated view is labeled; Cursor transcript
reading is explicitly unavailable. Loaded scrollback is for display and is not replayed into
the selected native session. Native tools and approvals are not restored from this text.

Ctrl+G lists reported native child-agent activity and Concordia worker jobs. Enter
opens the report; no model call is made by inspection. Native report detail depends on provider
events. Cross-account worker configuration remains under `/workers` (unlisted) and `/delegate`.

Cancellation leaves the prompt box unchanged. Use sent history to recall a submitted request
explicitly; queued work stays paused until resumed.

## Internal file panes and terminal editor

Click an underlined file reference in a sent user message or assistant reply to open it in an
internal read-only pane; a drag still selects transcript text. Alt+click also previews. Paths can
include a line/column (`src/main.rs:12:3`) or Markdown line fragment (`#L12`). Markdown labels
keep the explicit destination. `/open PATH` opens up to three file panes beside
chat; reopening a file focuses its existing pane. Click to focus, F6 cycles focus, Esc returns to
chat, arrows/PageUp/PageDown/Home/End scroll, and Left/Right pan. The title shows a stable pane id.
Click X or press Ctrl+W in a focused file. Drag the main divider or
the bottom border between file panes to resize. Files are read asynchronously, limited to 2 MiB
and 50,000 lines each, and must be UTF-8 text. File focus consumes typing/Enter and rejects paste;
these panes do not edit files. Small terminals hide panes without losing their contents and restore
them on enlargement. Layout is not restored after restart yet.

`/edit PATH` (unlisted) opens a separate configured terminal editor. Missing paths, preview errors or
launch failures are shown in Concordia. References never run as shell commands.

Concordia finds an installed `nvim`, `hx` or `vim`, or uses your explicit executable and argv:

```toml
[editor]
program = "nvim"
args = ["+{line}", "--", "{file}"]
```

`{file}`, `{line}` and `{column}` expand within individual arguments; the executable is one
path, not a shell command string. If `{file}` is absent the file is appended as one argument.
The standalone CLI `concordia preview FILE` remains a read-only Markdown/text viewer with scrolling,
Home/End and q/Esc to close, requiring no extra preview package. Input is UTF-8 and capped at
2 MiB. `/edit` and separate terminals require WezTerm; internal file panes do not.
`CONCORDIA_WEZTERM_BIN` can name its executable. See [workspace requirements](workspace-panes.md)
for the editing/recovery and search/replacement stages.

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
  that `/mode` and Shift+Tab own it.
- Delegation protocol: `/delegate protocol on|off` writes the store; until you use it, the config value applies.
- `max_workers` is a cap on worker enrollment inside `/delegate`; it is not stored.

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
