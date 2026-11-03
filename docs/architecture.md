# Architecture: local memory and provider orchestration

Concordia coordinates memory/session continuity, account routing and worker runs around the
unmodified Codex, Claude Code and Cursor CLIs. Those CLIs retain native execution, authentication
and approvals. Concordia owns the index/map, orchestration policy, process lifecycle and UI.

Current source implements explicit resume, bounded maps, optional Concordia thread logs,
recent Codex/Claude native-source pointers and exact readback, unfinished-request recovery,
file-backed Harness pins and explicit capacity-error rollover. [Native memory](native-memory.md)
documents the identities, byte budgets and remaining limits. Whole-history indexing, related-chat
retrieval, broader worker/run policy and proactive rollover remain target features in the
[implementation plan](context-handoff-plan.md). This page describes the existing runtime unless
a section is explicitly marked planned.

## Principles

## Harness adapters

`/harness` is a user-invoked bridge for any local harness. A `harness.json` manifest declares
provider-specific connect/verify argv and optional named tools; Concordia expands only
`{provider}`, `{home}` and `{project}`, runs without a shell, and stores connection state under
the Concordia state directory. Tools are listed or run explicitly and never become a model-owned
runtime. A verify receipt is written only after a successful adapter denial report matching the
current provider, isolated home and policy revision; provider turns cannot create receipts.

1. **Pointers, not payloads.** When a model needs context from another thread, give it a small
   map and a way to fetch the part it needs. Never paste a transcript.
2. **No summariser model.** Maps are built deterministically from recorded events. They cost no
   tokens to make.
3. **The CLIs own skills, rules, tools and native session execution.** Concordia coordinates
   memory access and continuity between sessions; it does not import provider skills or hooks. They
   keep working because Concordia only points the CLI at an account home.
4. **Declarative extension only.** User customisation is config and files. There is no plugin
   runtime and no agent loop in the binary.
5. **Small and fast.** One binary and no resident inference model. Background provider/worker
   tasks, usage refresh and local reads are bounded and tracked. Budgets are checked in CI.
6. **Document retention explicitly.** Full Concordia thread logging is opt-in. Prompt history,
   drafts and queue recovery have separate persistence controls and may store sensitive text;
   they must not be described as all disabled by default.

## Layers

| Layer | What | Where |
| --- | --- | --- |
| Thread log | Append-only events of a conversation (opt-in) | `<state>/threads/<thread-id>.jsonl` |
| Map | Deterministic summary of a thread, <= 1600 chars | built on demand (`threads::build_map`) |
| Pointer | How a model fetches more: `concordia thread read <id>` | printed inside every map |
| Delivery | Map prepended on a switch or `@chat`; native resume otherwise | `context.rs` |
| Config | User-edited `config.toml` and `commands/*.md` | `<state>/config.toml`, `<state>/commands/` |
| MCP (opt-in) | `concordia mcp`: the same data as tools | same binary |

`<state>` is `%LOCALAPPDATA%\concordia` (override: `CONCORDIA_STATE_DIR`).

## Thread log

One file per conversation, created when a conversation starts and `threads.enabled = true`.
A conversation is the span between `/new` (or launch) and the next `/new`. Thread id: UUID v4.

One JSON object per line. Fields:

| Field | Meaning |
| --- | --- |
| `t` | unix seconds |
| `k` | `user`, `assistant`, `card`, `switch`, `note`, `usage` |
| `text` | message text (`user`, `assistant`, `note`), capped at 20000 chars per event |
| `provider`, `account`, `model` | who produced it (`assistant`, `card`, `usage`) |
| `card` | `{title, detail, status}` for `card` events (tool/command cards) |
| `usage` | `{input, output, cached, cost_usd}` (any may be absent) for `usage` events |

Rules: never log credentials, never log stderr; lines are written with one `write_all` (append
mode) so concurrent panes do not interleave; the file rotates nothing and is deleted by
`/threads delete <id>`. A header file `<id>.meta.json` holds `{id, started, title, cwd}` where
`title` is the first user prompt cut to 60 chars.

## Map

`threads::build_map(events, thread_id) -> String`, at most `map_chars` (default 1600) characters.
Plain text, fixed layout, every line optional except the first and last:

```
[concordia map] thread 3f9a... "fix the login redirect" - claude-1 -> codex-2 - 42 msgs - 18 min
asked: <first user prompt, 160 chars>
now: <latest user prompt, 160 chars>
files: src/auth.rs, src/login.rs, tests/login.rs (+4 more)
ran: cargo test; git diff --stat; rg -n redirect (+3 more)
errors: <up to 3 failed cards or provider errors, 80 chars each>
last reply: <tail of the last assistant message, up to 500 chars>
more: concordia thread read 3f9a... [--tail N | --grep TEXT | --range A-B]
```

`files` come from cards whose title is an edit/write/read tool and whose detail looks like a path.
`ran` comes from shell/command cards. No inference, no model. If the budget is tight, drop in
this order: `ran`, `files`, `errors`, then shorten `last reply`.

## The `thread` subcommand (the free pointer)

`concordia thread list [--json]`, `concordia thread map <id>`, and
`concordia thread read <id> [--tail N] [--grep TEXT] [--range A-B]` print to stdout, capped at
`--max-chars` (default 6000). Models with a shell tool can call it, so no MCP is needed for this.
It is read-only. A prefix of an id is enough if unambiguous.

## Delivery

* Same account, same provider, explicit session id: native resume. Nothing is prepended.
* Switch (account or provider): prepend the map of the current thread, not the transcript dump.
  If the thread log is off, build the map from the in-memory transcript (same layout, the `more:`
  line says the full text is not stored).
* `@chat`: in the prompt, `@` opens a completion list of recent threads (title, age, account).
  On send, `@<id-or-title-prefix>` is replaced by that thread's map. The user sees exactly what is
  sent (the expanded map is shown dimmed under the prompt).

## Config

`<state>/config.toml`, written by the user, read-only to Concordia. Missing file or key = default.
Unknown keys are reported by `concordia doctor`, not fatal. Full reference: [config.md](config.md).
Implemented in `config.rs`; wired by: `config::ui_token_line()`, `ui_tips()` (stage 1b),
`threads_enabled()`, `threads_map_chars()` (stage 1a), `default_mode()` and `delegation_protocol()`/
`max_workers()` (store/delegate, store value wins). A store value set by the user wins over config. Concordia-owned state stays in
`store.json`; config only supplies defaults and switches.

```toml
[ui]
token_line = true        # per-turn "in 12.3k out 1.4k 8.2s" under each reply
tips = true              # a rotating tip while idle

[threads]
enabled = false          # record conversations to <state>/threads (stores prompts)
map_chars = 1600

[mode]
default = "ask"          # plan | ask | workspace | full

[delegation]
protocol = false         # let the orchestrator propose delegations (user still approves)
max_workers = 3
```

## User commands

`<state>/commands/<name>.md` becomes `/<name>`. Front matter is optional:

```
---
description: review the staged diff
---
Review the staged diff for bugs and missing tests. Focus: $ARGS
```

Running `/<name> some text` sends the body with `$ARGS` replaced, as an ordinary prompt. Built-in
commands win over user commands on a name clash (doctor reports the clash). No code runs: it is a
prompt template. Implemented in `user_commands.rs`; see [config.md](config.md).

## Token line and tips

After each reply, one dim line: `in 12.3k - out 1.4k - cached 9.8k - 8.2s` (fields that the
provider reports; cost only if reported). Sources: Claude `result.usage` and `total_cost_usd`;
Codex `turn.completed.usage`. Cursor reports none, so the line shows time only. Session totals
appear in `/usage` under the limits. Tips are a static list in the binary, one per launch, shown
only while the transcript is empty or the prompt has been idle for 20 s.

## Awareness (one line, no per-turn cost)

For homes Concordia created, `/use add` writes a short block into that home's own instructions
file (`CLAUDE.md` under `CLAUDE_CONFIG_DIR`, `AGENTS.md` under `CODEX_HOME`): "You are running
inside Concordia. A `[concordia map]` block is a pointer to an earlier thread; run
`concordia thread read <id>` for details." About 40 tokens, loaded and cached by the CLI. Never
written to a user's own existing home.

## MCP server (opt-in)

`concordia mcp` (stdio) exposes `threads`, `thread_map`, `thread_read`, `usage` (read-only) and
`delegate` (propose only; the UI asks the user). Registered only in homes Concordia created.
Keep the tool list to five and the descriptions terse: every schema costs tokens each turn.

## Prompt queue

While a turn runs, the user can line up the next prompts.

* **Tab** with a non-empty prompt, while a turn is running (or other prompts are already queued),
  moves the prompt into the queue. When idle with an empty queue, Tab keeps completing slash
  commands. A prompt that starts with `/` is never queued; it runs now.
* The queue shows in a panel **above** the prompt box: `queued 1: <first 60 chars>`, newest last,
  at most 5 rows then `+N more`. The panel and the draft are separate: editing the queue never
  replaces what is being typed.
* **Ctrl+Up** focuses the queue while preserving a nonempty draft, caret and selection;
  **Ctrl+Down** returns after an upper edit is saved or cancelled. **Up** on the first prompt row
  with an empty draft and a non-empty queue also focuses the panel (with
  an empty queue, Up keeps walking history). In the panel: Up/Down select, **Enter** edits the
  selected item in a multiline upper editor (Enter saves, Esc cancels, Ctrl+Enter or Ctrl+J inserts
  a newline), **Alt+Up/Alt+Down** move it,
  **Delete** removes it, **Esc** returns to the prompt.
* Automatic sending waits while a queued item is being edited, preserving unsaved text; it
  continues after save/cancel. Conversation/provider/model switches wait for the active turn to
  finish or be cancelled, so streamed output stays bound to its original identity.
* When the running turn finishes (not when it is cancelled or fails), the first queued prompt is
  sent automatically through the normal submit path, so trust mode, the current account and
  provider, continuity packs and logging all apply at send time. Switching account while
  prompts are queued is allowed; the queue is provider-neutral text.
* Esc during a turn cancels the turn and **pauses** the queue. **Ctrl+R** or `/queue resume`
  explicitly resumes it while retaining the draft. Enter sends typed text without resuming a
  paused queue. A failed turn and restart also pause queued work.
* Draft, queue and pending upper edit use a pane-owned recovery record (atomic writes).
  `/queue clear` empties the committed queue; active upper edits require save/cancel first.

## Expandable activity

`Ctrl+O` toggles compact previews and full received tool input/output and reasoning, including
while a turn runs. Parsers retain text; only the compact rendering shortens it. Replies remain
fully scrollable. Provider-side omissions and the runner's bounded output-line limit still apply.
Click a card's gutter to expand it independently. `Ctrl+E` opens full activity detail with search,
match navigation and `Ctrl+C` copy; `Alt+Up/Down` chooses the previous/next card, and `Esc` closes
the detail view. Input belongs to that view while open,
preserving the main draft and queued edit underneath.

## Undo, redo and drafts

* `Ctrl+Z` undo, `Ctrl+Shift+Z` and `Ctrl+Y` redo, in the prompt editor. History is a stack of
  (text, cursor) snapshots, coalesced so that a burst of typing is one step and every cut, paste,
  delete-word and clear is its own step. Capped at 200 steps. Sending a prompt does not clear the
  stack (undo after send restores the sent text into the box, so a mis-sent prompt is never lost).
* Each pane holds an OS lock on its own record under `<state>/pane-recovery/<workspace-key>/`.
  Draft changes are debounced (about one second), with immediate save on exit. Active pane records
  are never claimed by another pane. One abandoned record can be restored at startup; several
  produce a chooser. `/recover` selects an abandoned record for the current workspace. Queued
  work stays paused after recovery. A failed turn restores its submitted prompt when the box is
  empty. Cancellation leaves the current prompt box unchanged; sent history supports explicit
  recall. Legacy shared `draft.json` and `queue.json` require explicit import via
  `/recover`; sent history remains shared.
* Pending upper edits are saved separately from committed queue items; recovery restores the
  editor and holds dispatch until explicit save/cancel. Save failures stay visible in the footer
  and retry at bounded intervals. `/recover` explains the error; `/recover retry` retries after
  local storage is repaired. Explicit legacy import preserves the current pane's saved record.
* Sent prompts persist to `<state>/history.jsonl` (cap 500, newest last) so Up still works after a
  restart. This stores prompts locally; `[ui] persist_history = false` turns it off (default on,
  same trust boundary as the provider CLIs' own history).

## Process stats

Shared goals and the token log use stable adjacent lock files through `state_io`.
Lock acquisition is bounded to two seconds. Goal saves merge additions and completion
by immutable id before atomic replacement, preserving unreadable originals. Token
writers append complete serialized records under the lock, so concurrent panes cannot
interleave JSON lines. Goals are bounded to 2 MiB on read/save; token counters saturate
instead of wrapping. The landing page wraps, scrolls and hit-tests goals using the same
visual rows, and displays the focused pane when the terminal is too narrow for two.

Optional footer segment `mem 41 MB - cpu 1%` for Concordia, and `+ agents 380 MB - 6%` for the
provider processes it started (their whole process trees). `[ui] stats = true` (default false).
Sampled every 2 s on a background thread using Win32 (`GetProcessMemoryInfo`, `GetProcessTimes`,
`CreateToolhelp32Snapshot`); the UI thread only reads the last sample. `concordia doctor` and
`docs/metrics.md` also report idle memory.

## Provider compatibility

Concordia never pins or updates provider CLIs. It uses **feature detection, not version pinning**:

* `concordia doctor` prints each provider's version (`--version`) next to a `TESTED` table in
  `provider/compat.rs` (the version each parser and flag set was last verified against) and warns,
  never fails, when the installed one is newer or older.
* A capability probe runs `--help` once per provider (cached in the state dir by the CLI's
  version string) and records which flags exist (`--output-format stream-json`,
  `--permission-mode`, `--mode plan`, `--include-partial-messages`, ...). Turn planning degrades
  when one is missing: stream-json falls back to plain text, plan mode falls back to ask. The
  footer shows one dim notice the first time it happens ("claude: no stream-json, showing text").
* Parsers already ignore unknown events and pass plain text through. Every provider format change
  is added as a fixture line in `tests/fixtures/provider-lines/<provider>/` first; the parser
  tests read that folder, so a new shape is one file.

## Locked-code launch gate

`provider::enforcement` decides, before a provider process starts, whether a turn may be
write-capable. A project opts in by carrying `docs/quality/locked-code.json` (found from the turn's
working directory upward). Installed hook files and capability flags do not count as enforcement.

The gate opens only for a receipt in Concordia's state directory, at
`enforcement/<project key>/<provider>.json`, with `result: "denied-live-probe"` whose provider,
isolated home, policy revision (SHA-256 of the policy file with normalized line endings), and
connected harness identity match the turn. File-backed harness manifests are re-hashed before the
gate opens. Receipts are never read from inside the project, so an agent that can only write the
project cannot forge one. The first time the gate sees a policy file it records that the project
was locked, so deleting the file does not unlock it. A provider home inside the project is not
isolated and never opens the gate. Any mismatch, missing file or read error closes it.

A closed gate lowers ask, workspace and full trust to plan and the turn shows a
`TurnProgress::Notice` explaining why. Plan turns are unchanged. Capability fallbacks can drop the
plan switch, so after planning the command line must still be provably read-only (Claude
`--permission-mode plan`, Cursor `--mode plan`, Codex `-s read-only`); if it is not, the turn is
refused instead of started. The Harness verifier writes the receipt; agents never do. A changed
connection or file-backed manifest closes the gate until verification runs again.

## Images and remaining attachment work

Build `2026-11-01e` implements clipboard/file image import, composer preview/remove chips,
content-addressed snapshots, durable draft/queue envelopes and native inputs for Codex/Claude/Cursor.
Cursor image turns use ACP and route permission/choice requests back to their original RPC ids.
Ordinary text turns retain their existing provider adapters. No vector embeddings are created.

[Images and diagrams](images-and-diagrams.md) records formats, budgets, lifecycle and fixture/live
capability evidence. Inline raster thumbnails, generic document/drop attachments and generated
artifact cards remain planned. Mermaid terminal diagrams render locally with bounded background
workers and a pinned native renderer; no browser/Node rendering service is launched.

## Budgets (checked in CI)

| Budget | Limit |
| --- | --- |
| Release binary | 4 MB |
| `concordia --version` cold start, median | 60 ms |
| `draw_ui` frame, busy transcript | 2 ms |
| Map | 1600 chars |
| Per-turn work on the UI thread | no file or process I/O |

## Production-readiness additions (2026-10-16)

Windows VT input uses a dedicated console reader plus the existing ANSI parser on the event
thread. Both readers are joined, input mode is ready before the first frame, and saved console
settings are restored at shutdown. Framed paste becomes one explicit paste event across read
chunks; action keys outside its frame remain actions. Bounded channels carry events and observed
quiet markers independently of drawing. Unsupported hosts fall back to the native key-record
reader; only this fallback and unframed input require quiet-gap inference. An empty application
channel alone never proves that a paste ended. Source scheduling and channel saturation remain
limitations for input that lacks framing.

`file_links` extracts bounded local references from rendered transcript text; explicit clicks
resolve against the workspace and `file_open` builds argument-array WezTerm launch plans.
External editors are configured through `[editor]`; `preview FILE` is a separate, read-only
UTF-8 Markdown/text terminal viewer. Selection drags never launch files. No shell evaluates
message text. `chat list|search|read` reads bounded native transcript files with explicit account
and session identity; search is metadata-only and unsupported formats report that limitation.

Usage fetches and caches bind account, provider and full normalized home. Old unbound/future
cache entries are not trusted; late results retain their originating identity. Low-limit warnings
above chat label freshness, and a reset does not imply confirmed available capacity. Tool cards
merge results into the original command and retain provider-supplied arguments/output/diffs.
See the production plan and individual audit documents for evidence and remaining gaps.

## Non-goals

A plugin runtime, an agent loop, skill or rule import, a global skill library, hosted services,
automatic summarisation, rotating accounts to get around a limit.

## Build order and ownership

| Stage | Work | Owns |
| --- | --- | --- |
| 1a | Thread log, map, `thread` subcommand, delivery on switch | `threads.rs`, `context.rs`, `main.rs` (subcommand only) |
| 1b | Token line and tips | `provider/stream_json.rs`, `provider/stream_cards.rs`, `provider/turn.rs`, token/tips drawing in `tui/mod.rs` |
| 1c | `config.toml`, user commands, doctor checks | `config.rs`, `commands.rs`, `doctor.rs` |
| 2a | `@chat` completion and expansion | `tui/mod.rs`, `tui/palette.rs` (needs 1a) |
| 2b | Awareness line, `concordia mcp` (needs 1a) | `mcp.rs`, `connect.rs` |
| 2c | Budgets in CI, docs | `scripts/`, `.github/`, README |
| 3a | Prompt queue, undo/redo, draft and history persistence | `tui/editor.rs`, `tui/queue.rs`, `tui/mod.rs` (input handling) |
| 3b | Process stats, provider compatibility, coloured `/usage` block | `sysstats.rs`, `provider/compat.rs`, `doctor.rs`, `usage.rs`, `tui/transcript.rs` |
| 3c | `@chat`, awareness line, `concordia mcp` | `mcp.rs`, `tui/palette.rs`, `connect.rs` |
| 4 | Attachments and artifact chips | `attachments.rs`, paste handling |
