# Architecture: a thin, configurable orchestrator

Concordia runs the unmodified Codex, Claude Code and Cursor CLIs under the user's own logins. It
adds accounts, limits, continuity and delegation on top, and nothing else. This page is the
contract the pieces are built against.

## Principles

1. **Pointers, not payloads.** When a model needs context from another thread, give it a small
   map and a way to fetch the part it needs. Never paste a transcript.
2. **No summariser model.** Maps are built deterministically from recorded events. They cost no
   tokens to make.
3. **The CLIs own skills, rules, tools and memory.** Concordia never parses or imports them. They
   keep working because Concordia only points the CLI at an account home.
4. **Declarative extension only.** User customisation is config and files. There is no plugin
   runtime and no agent loop in the binary.
5. **Small and fast.** One binary, no resident model, no background work beyond the usage cache.
   Budgets are checked in CI (see "Budgets").
6. **Local and opt-in.** Anything that stores prompts is off until the user turns it on.

## Layers

| Layer | What | Where |
| --- | --- | --- |
| Thread log | Append-only events of a conversation (opt-in) | `<state>/threads/<thread-id>.jsonl` |
| Map | Deterministic summary of a thread, <= 1600 chars | built on demand (`threads::build_map`) |
| Pointer | How a model fetches more: `concordia thread read <id>` | printed inside every map |
| Delivery | Map prepended on a switch or `@chat`; native resume otherwise | `context.rs` |
| Config | User-edited `config.toml` and `commands/*.md` | `<state>/config.toml`, `<state>/commands/` |
| MCP (later) | `concordia mcp`: the same data as tools | same binary |

`<state>` is `%LOCALAPPDATA%\concordia` (override: `CONCORDIA_STATE`).

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

## MCP server (later)

`concordia mcp` (stdio) exposes `threads`, `thread_map`, `thread_read`, `usage` (read-only) and
`delegate` (propose only; the UI asks the user). Registered only in homes Concordia created.
Keep the tool list to five and the descriptions terse: every schema costs tokens each turn.

## Budgets (checked in CI)

| Budget | Limit |
| --- | --- |
| Release binary | 4 MB |
| `concordia --version` cold start, median | 60 ms |
| `draw_ui` frame, busy transcript | 2 ms |
| Map | 1600 chars |
| Per-turn work on the UI thread | no file or process I/O |

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
