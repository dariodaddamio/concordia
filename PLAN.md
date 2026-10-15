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
| Multi-session | WezTerm panes and tabs; each pane runs its own Concordia |
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

1. **Verify live** (needs the user): `/use add` login window, mouse and selection in a real WezTerm
   pane, a real multi-line turn through the stdin path.
2. **Context sharing**: explicit, bounded, visible-size handoff between providers and accounts
   (design in docs/provider-terms.md).
3. **Account handoff on exhaustion**: user-confirmed, explicit session bind; blocked on reading the
   providers' terms (see docs/provider-terms.md).
4. **Claude and Cursor parity**: session-id binding after turns, parity of cards and usage.
5. **Distribution**: name decision, signed releases, a shipped WezTerm config, first public tag.

Out of scope for v0.1: crew-style multi-agent delegation, hosted anything, Linux and macOS.

## Open decisions (the maintainer's)

- GitHub organization and repository name (the product name, Concordia, is decided; a trademark check is still to do).
- Whether and how automatic account handoff ships (terms review first).
- When to publish, and the repository destination.

## References

- [WezTerm CLI](https://wezterm.org/cli/cli/index.html) and [multiplexing](https://wezterm.org/multiplexing.html)
- [Claude Code programmatic use](https://code.claude.com/docs/en/headless)
- [Codex authentication](https://learn.chatgpt.com/docs/auth)
- [Cursor ACP](https://cursor.com/docs/cli/acp)
