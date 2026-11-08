# How Concordia works

Concordia is a local Windows application around the official Codex, Claude Code and Cursor CLIs.
It owns the interface, local conversation index, account routing, recovery records and worker
scheduler. The provider CLI still owns authentication, the native model session, tools and
approvals. Concordia does not proxy prompts through a hosted service and does not run a model to
summarize or route them.

This is the entry point for the implementation as it exists now. It deliberately cites source
files and function names so that a claim can be checked against code rather than inferred from a
roadmap document.

## The shortest end-to-end tour

1. `concordia` opens the native Windows window; `concordia --terminal` renders the same application
   in the current terminal. `concordia wezterm` opens WezTerm with a bundled config. Each tab or
   WezTerm pane is another Concordia process. See
   [`main.rs` (`run_default_chat`, command dispatch)](../crates/concordia/src/main.rs) and
   [`host/wezterm.rs` (`launch_window`, `split_concordia_pane`)](../crates/concordia/src/host/wezterm.rs).
2. One in-process application can also have several chat tabs and up to three internal file panes.
   Those are UI state, not extra provider agents. See
   [`tui/tabs.rs` (`TabSet`)](../crates/concordia/src/tui/tabs.rs) and
   [`tui/workspace.rs` (`MAX_PANES`, `Workspace`)](../crates/concordia/src/tui/workspace.rs).
3. Sending creates a provider/account/run owner, reserves the workspace, expands any `@chat`
   references, snapshots attachments and decides whether to resume the exact native session or
   start a new one with a bounded continuity prefix. See
   [`tui/run.rs` (the `turn_rx` dispatch loop)](../crates/concordia/src/tui/run.rs).
4. `provider::turn::plan_provider_turn` produces a program plus argument array and environment.
   The prompt is never interpolated into a shell command. Unsafe Windows shim arguments move to
   stdin or a verified launcher path. See
   [`provider/turn.rs` (`plan_provider_turn`)](../crates/concordia/src/provider/turn.rs) and
   [`provider/launch.rs` (`prepare`)](../crates/concordia/src/provider/launch.rs).
5. A tracked child process streams structured events. Concordia turns messages, reasoning and tool
   events into transcript rows and cards. Ctrl+C/Esc cancellation marks the owned run, wakes the
   reader, kills that provider process tree and reaps the direct child. See
   [`provider/turn.rs` (`RunningTurn::cancel`, `kill_process_tree`,
   `run_provider_turn_inner`)](../crates/concordia/src/provider/turn.rs).
6. The result remains in the provider's native session. Concordia records display/recovery state,
   token events and—when enabled—bounded references to original native history. It does not rewrite
   provider history. See
   [`native_memory.rs` (`Binding`, `index_bound`)](../crates/concordia/src/native_memory.rs),
   [`threads.rs` (`ThreadLog`)](../crates/concordia/src/threads.rs) and
   [`token_log.rs` (`record`, `load`)](../crates/concordia/src/token_log.rs).

The detailed tours are:

- [Provider processes, accounts, sessions and approvals](how-concordia-connects-providers.md)
- [Exactly what reaches the model, with size budgets](how-concordia-builds-prompts.md)
- [Memory, handoffs, workers and native subagents](how-concordia-remembers-and-delegates.md)
- [State, Harness integrations, MCP, usage, UI and security](how-concordia-runtime.md)

## Process and window model

The default executable contains the TUI and the native-window host. On Windows, no arguments call
`tui::run_chat_window`; `--terminal` calls `tui::run_chat_tui`. The native host is not a separate
daemon. Provider turns are child processes owned by the Concordia process that launched them.
Background preparation—history hydration, usage reads and provider stream readers—uses bounded
threads/tasks, but there is no resident inference service. Source:
[`main.rs` (`main`, `run_default_chat`)](../crates/concordia/src/main.rs),
[`tui/native_window.rs` (`run`)](../crates/concordia/src/tui/native_window.rs), and
[`provider/turn.rs` (`run_provider_turn_inner`)](../crates/concordia/src/provider/turn.rs).

There are three easily confused kinds of “pane”:

| Thing | What it is | Process boundary |
| --- | --- | --- |
| Native Concordia window | A Windows window rendering the ratatui application | One Concordia process |
| Chat tab | Another conversation inside that application | Same process; distinct conversation/run identity |
| Internal file pane | Read-only preview or raw editor beside the chat | Same process; belongs to that conversation workspace |
| WezTerm pane/tab | A terminal running `concordia --terminal` | Another Concordia process |

The native window has its own clipboard, file-drop and raster handling. Terminal mode relies on the
terminal host. Internal file panes are capped at three and files at 2 MiB/50,000 lines. Source:
[`tui/workspace.rs` (`MAX_PANES`, `MAX_BYTES`, `MAX_LINES`)](../crates/concordia/src/tui/workspace.rs)
and [`tui/native_window.rs`](../crates/concordia/src/tui/native_window.rs).

## State directory at a glance

`<state>` is `%LOCALAPPDATA%\concordia` unless `CONCORDIA_STATE_DIR` is set. The root is created
with owner-only permissions where the platform support succeeds. The durable settings store is
actually `<state>/.ao/store.json`, not `<state>/store.json`. Source:
[`store.rs` (`default_store_root`, `ensure_state_root`, `store_dir`,
`store_path`)](../crates/concordia/src/store.rs).

Important paths include:

```text
<state>/
  config.toml                  user-written configuration; Concordia reads it
  concordia.log[.1]            redacted event log, 1 MiB rotation
  homes/<account>/             Concordia-created provider login homes
  threads/<id>.jsonl           optional thread metadata/native references
  threads/<id>.meta.json       thread title/workspace metadata
  threads/<id>.pins.jsonl      bounded locally pinned source spans
  attachments/<sha>.*          immutable attachment snapshots
  usage-cache/*.json           account/home-bound quota cache
  token-log.jsonl              reported per-turn token events
  proposals/                   MCP proposals awaiting local review
  pane-recovery/.../           pane drafts, queue, unfinished run and worker ledger
  .ao/store.json               accounts, selections and saved settings
  .ao/native-input/*.json      per-account native transport preference
  .ao/context-policy/*.json    per-account context/compaction policy
  .ao/native-memory/*.json     content-addressed native-source indexes
```

Individual subsystems have more files and lock files. The path constructors—not this illustrative
tree—are authoritative: [`connect::homes_dir`](../crates/concordia/src/connect.rs),
[`threads::threads_dir`](../crates/concordia/src/threads.rs),
[`attachments::import`](../crates/concordia/src/attachments.rs),
[`usage::UsageIdentity::cache_path`](../crates/concordia/src/usage.rs),
[`delegate_recovery.rs` (`Ledger::save`)](../crates/concordia/src/delegate_recovery.rs), and
[`native_memory.rs` (`directory`)](../crates/concordia/src/native_memory.rs).

`config.toml` supplies defaults and switches; it is not the settings database. Saved choices such
as trust, workers and continuation opt-ins live in `.ao/store.json`. Writes use adjacent lock files
and atomic replacement where the subsystem requires it. Source:
[`config.rs` (`Config::parse`, `for_root`)](../crates/concordia/src/config.rs),
[`store.rs` (`modify_store`)](../crates/concordia/src/store.rs), and
[`state_io.rs` (`with_lock`, `write_atomic`)](../crates/concordia/src/state_io.rs).

## Identity is the organizing rule

A turn is not “whatever pane is focused.” Runtime and recovery records bind explicit conversation,
workspace, execution directory, run, attempt, provider, account, home and native-session ids.
Native interactive requests validate that owner before the UI can answer them. Exact-session locks
prevent two owned turns from concurrently mutating one native session. Source:
[`provider/interaction.rs` (`InteractionOwner::validate_for`,
`NativeRequest::is_owned_by`)](../crates/concordia/src/provider/interaction.rs),
[`provider/session_guard.rs` (`acquire`)](../crates/concordia/src/provider/session_guard.rs), and
[`tui/run.rs` (construction of `InteractionOwner`)](../crates/concordia/src/tui/run.rs).

That rule explains several design choices:

- Concordia never uses `resume --last`, Claude `--continue`, or Cursor's latest-chat resume.
- Cross-provider and cross-home switches clear the old native-session id.
- Pending approvals/questions are invalidated on cancel, disconnect or owner mismatch.
- Recovery preserves uncertainty instead of blindly replaying a tool-bearing request.

The enforcing functions are
[`provider/codex.rs` (`assert_safe_argv`)](../crates/concordia/src/provider/codex.rs),
[`provider/claude.rs` (`assert_safe_claude_argv`)](../crates/concordia/src/provider/claude.rs),
[`provider/cursor.rs` (`assert_safe_cursor_argv`)](../crates/concordia/src/provider/cursor.rs), and
[`tui/continuation.rs` (`prepare_usage_continuation`, `continue_usage`)](../crates/concordia/src/tui/continuation.rs).

## Changing soon, but not documented as shipped

Concurrent work is exploring an `ask-mcp` bridge for question popups, live usage refresh changes, a
`concordia feed` command and module-cycle cleanup. None of those names or behaviors is part of the
current contract on this page. Current questions are provider-native interaction requests or normal
follow-up UI choices; current usage behavior and current CLI commands are described in the focused
pages and in `main.rs`.

## Glossary

**Account** — a provider plus a stable Concordia account id and, normally, an explicit provider
home. It is not inferred from the focused window.

**Provider home** — the directory selected with `CODEX_HOME`, `CLAUDE_CONFIG_DIR`, or the
Cursor-account key/config binding. It is the authentication/session namespace.

**Conversation** — the local Concordia chat/workspace identity. It may cross providers through
handoffs.

**Native session** — a Codex thread id, Claude session id or Cursor chat/session id owned by the
provider CLI.

**Thread log** — Concordia's optional append-only metadata/reference journal under `<state>/threads`.
It is not the provider's native transcript.

**Map** — deterministic bounded text describing a thread and pointing to exact read commands. A
map is created locally without a model.

**Native-source index** — content-addressed metadata containing the provider/account/home/session,
file identity, byte ranges and hashes needed to re-read original provider history safely.

**Continuity pack** — the prefix on a fresh session after a switch: map v2, optionally native-source
pointers and workspace scope. It is omitted on exact native resume.

**Concordia worker** — a separate, user-started provider turn on another registered account,
scheduled by Concordia and returned as untrusted data.

**Native subagent** — a child agent created inside one provider's own turn (Codex collaboration
tools or Claude Agent/Task). Concordia displays provider-reported state but does not schedule it as
a cross-account worker.

**Harness** — a local integration described by `harness.json`; its explicit connect/verify/tool
commands are run only through reviewed user actions.

**MCP server** — the opt-in `concordia mcp` newline-delimited stdio server exposing bounded local
reads and review-only proposals.

**Trust mode** — Concordia's Plan, Ask, Workspace or Full choice, translated into provider-specific
flags or native transport policy. It is not a promise that all providers have identical approval
semantics.
