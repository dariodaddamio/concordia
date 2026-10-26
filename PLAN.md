# Concordia plan

**Concordia is the provider orchestration layer: use all your AI coding subscriptions from one terminal
app.** It is a fast, native Rust TUI for Windows that drives the official Codex, Claude Code and
Cursor CLIs under your own logins. Providers are backends; Concordia draws the interface.

Status: pre-release, private. Product first (everyday chat that works end to end), then an
MIT-licensed, stranger-downloadable Windows binary. Live status: [docs/e2e-status.md](docs/e2e-status.md).
What blocks a public release: [docs/release-blockers.md](docs/release-blockers.md).

## Product

| Decision | Choice |
| --- | --- |
| Interface | Concordia's own TUI (`ratatui`), inspired by modern agent clients; clean-room |
| Chat, tool cards, command previews | Rendered by Concordia from the providers' structured output |
| Setup and control | Slash commands in the same input as chat (`/use`, `/use add`, `/model`, `/usage`, `/resume`, `/trust`, `/update`, `/doctor`, `/copy`) |
| Backends | Programmatic CLIs: `codex exec`, `claude -p`, `agent --print`. Never a scraped hidden TUI |
| Accounts | Several per provider, each in an isolated home; sign-in is the provider's own flow |
| Context across switches | One Concordia thread; on a switch, a bounded pack of Concordia's own recorded conversation goes to the next provider |
| Workspace | Installed build `2026-10-21d`: `/open` and `/preview` create internal read-only split panes, `/close` closes them, `/edit` launches an external editor. Editing/recovery stages: [workspace requirements](docs/workspace-panes.md) |
| Multi-session | Separate conversations can still use WezTerm panes and tabs; internal file panes belong to one conversation workspace |
| Install | One binary: `concordia install` puts it where `concordia` on PATH finds it; `/update` keeps it current |

## Principles (contracts)

**Identity and execution.** Every operation is bound to explicit ids: workspace, conversation,
account, native provider session, run, pane. Never recover through `resume --last`, the focused
pane, or newest-chat heuristics. Each account has its own authentication environment; credentials
are never rotated underneath running agents. One executor owns a conversation at a time.

**Run states.** idle, starting, running, waiting for approval, interrupting, exhausted, switching,
finished, failed, uncertain. Completed tool events are recorded separately from requested work. On
an uncertain completion, reconcile or ask; never blindly retry. Recovery cannot cycle forever.

**Cost.** No inference calls for routing, account selection, status polling, indexing or
summaries. No hosted service, no resident model, no Docker. Only explicitly started provider turns
consume provider usage. Unknown usage is shown as unknown, never inferred as free capacity.

**Safety.** Arguments travel as arrays or stdin, never shell strings; Windows `.cmd` shims are
handled explicitly. Cancellation kills the provider's whole process tree and only that. Nothing
resets, cleans, merges or deletes user work. Native approvals stay authoritative; headless bypass
flags are not equivalent to interactive approval and are never adopted silently.

**Terms.** Concordia runs the unmodified provider CLIs under the user's own logins and never collects,
stores or shares credentials. See [docs/provider-terms.md](docs/provider-terms.md).

## Architecture

| Module | Role |
| --- | --- |
| `tui/` | Screen, input editor, mouse selection, transcript rows (cached), clipboard |
| `provider/` | Per-provider argv planning, `launch` (stdin vs argv, shim handling), `turn` runner (stream, cancel, stderr), stream cards |
| `accounts`, `connect`, `store` | Account discovery, Concordia-owned homes and login launch, atomic locked state |
| `context`, `sessions`, `usage`, `catalog`, `identity` | Bounded context pack, resumable sessions, limits and models read from provider-written files |
| `update`, `release` | Install (`concordia.exe` swap that works while running), `concordia install`, source and release updates |
| `doctor`, `commands`, `host` | Diagnostics, slash commands, WezTerm integration |

Provider state stays native: usage, sessions and models are read from the files the CLIs already
write, never re-implemented.

## Roadmap

1. **Reconcile readiness**: retain recorded installed fake-provider verification and distinguish
   outstanding human host checks and versioned real-provider checks. See [E2E status](docs/e2e-status.md).
2. **Internal workspace**: bounded file preview panes, then explicit editing, recovery and cwd
   search/replacement. Prove focus, mouse and command behavior before expanding; acceptance is in
   [workspace requirements](docs/workspace-panes.md).
3. **Reliable self-development**: complete a bounded Concordia change through the stable installed
   app, including interruption and recovery. Build a candidate separately; a failed candidate must
   not remove the working interface.
4. **Thin delegation routing**: evolve existing explicit workers beyond first-idle selection using
   Harness-owned eligibility and task policy, with visible reasons and bounded handoffs. No extra
   inference for routing. Worktree lifecycle requires explicit ownership and preservation of unique
   or active work; existing user work is never automatically cleaned.
5. **Provider parity**: verify Claude/Cursor session binding, native approvals and cancellation
   against supported CLI versions. Account exhaustion requires explicit session binding and no
   uncertain replay; terms review remains a release prerequisite.
6. **Distribution**: name checks, signed releases and first public tag after everyday reliability.

Locked-code launch gate: implemented (`provider::enforcement`, see [architecture](docs/architecture.md)).
A write-capable turn in a project with `docs/quality/locked-code.json` runs read-only unless a verified
receipt matches its provider, isolated home and policy revision. Still open: the Harness verifier that
writes the receipt from a live denial probe, and runtime-version binding.

Bounded context handoff and explicit delegation already exist; see [delegation](docs/delegation.md).
Internal preview panes are implemented in source; editing/recovery, policy-based routing and
managed worktree cleanup remain requirements. Hosted services, resident inference and Linux/macOS
support remain outside current scope. Local installed build is `2026-10-21d`; earlier running
windows retain their existing executable until reopened.

## Open decisions (the maintainer's)

- GitHub organization and repository name (the product name, Concordia, is decided; a trademark check is still to do).
- Public-release terms review for automatic account handoff. Local opt-in handoff is configured under `[continuation]`; see [configuration](docs/config.md).
- When to publish, and the repository destination.

## References

- [WezTerm CLI](https://wezterm.org/cli/cli/index.html) and [multiplexing](https://wezterm.org/multiplexing.html)
- [Claude Code programmatic use](https://code.claude.com/docs/en/headless)
- [Codex authentication](https://learn.chatgpt.com/docs/auth)
- [Cursor ACP](https://cursor.com/docs/cli/acp)
