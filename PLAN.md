# Concordia plan

**Concordia is a local memory-management and orchestration layer for AI coding CLIs.**
It keeps tasks navigable across provider accounts, sessions and worker runs. Its native Rust TUI
for Windows drives the official Codex, Claude Code and Cursor CLIs under your own logins. Providers are backends; Concordia draws the interface.

Status: pre-release, private. Memory management and orchestration are the core product; native chat
and a reliable UI are their execution surface. Scope was reaffirmed by the owner on 2026-10-31.
Bounded continuity maps, explicit delegation, recent native-source pointers/readback (Codex/Claude),
pane-owned unfinished-request recovery and one-attempt explicit capacity-error rollover are implemented.
Whole-history pages, bounded lexical retrieval/lineage, supported Cursor ACP readback, opt-in Codex
proactive rollover/native compaction controls and durable worker recovery/failover are implemented
with fixture evidence. Unsupported provider capacity and live parity remain open.
Current bounds and controls: [native memory](docs/native-memory.md).
Native image inputs for Codex/Claude/Cursor and local Mermaid rendering are implemented;
[image/diagram limits](docs/images-and-diagrams.md) distinguish fixture proof from live-provider validation.
Current UI behavior and installed-host pointer limits: [chat rendering](docs/chat-rendering.md).
The owner's implementation-first campaign gates and X/LinkedIn drafts are in the
[launch plan](docs/launch-plan.md); unfinished roadmap features are not launch claims.
The [completion audit](docs/feature-completion-audit.md) records what is closed and what remains;
the two draft GIFs demonstrate completed UI behavior only.
Product first, then an MIT-licensed Windows release with documented limits. Live status: [docs/e2e-status.md](docs/e2e-status.md).
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
| Workspace | `/open` and `/preview` create internal splits; Markdown/JSON default to formatted, read-only view. Raw mode edits with syntax colors, explicit save, conflict checks and recovery. `/edit` launches an external editor. Remaining stages: [workspace requirements](docs/workspace-panes.md) |
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
| `context`, `sessions`, `usage`, `catalog`, `identity` | Bounded context pack, resumable sessions, limits from read-only native interfaces; models from provider-written files |
| `update`, `release` | Install (`concordia.exe` swap that works while running), `concordia install`, source and release updates |
| `doctor`, `commands`, `host` | Diagnostics, slash commands, WezTerm integration |

Provider state stays native: sessions/models use provider-written files, and Codex usage also
uses its read-only quota RPC. Native execution, authentication and approvals remain authoritative.

## Roadmap

The [remaining-feature execution plan](docs/plans/completion-roadmap.md) breaks all open areas
into implementation slices, parallel ownership, dependencies and observable acceptance criteria.
Its detailed plans are proposed work; shipped status remains in the feature completion audit.

Worker models, bounded automatic result return and remaining orchestration gaps:
[orchestration and latency](docs/orchestration-and-latency.md).

1. **Native-source memory index.** Recent-window source identity/readback is implemented for
   Codex/Claude; whole-history pages and supported Cursor ACP readback are also implemented. Bind pointers to provider, account home, session, file revision
   and exact range. Index session sources only, preserve existing logs, and detect missing,
   compacted or replaced originals. An unknown or stale source must not yield an unrelated chat.
2. **Bounded retrieval and lineage.** Lexical page retrieval, workspace filters, explicit confirmed
   links and observed handoffs are implemented. Richer goal/file relationships remain future work.
   Link explicit handoffs, projects, touched files and time.
   Return exact excerpts with source identity and bounded read budgets. Keep lexical suggestions
   distinct from confirmed relationships. Evaluate recall, stale-source handling and task success
   using synthetic histories before choosing more complex retrieval.
3. **Durable orchestration policy.** Unfinished request/run/source recovery and file-backed
   Harness pinned-decision delivery, pane-owned worker ledgers and bounded worker quota failover
   are implemented. Richer dependency/workspace isolation and eligibility explanations remain.
   Persist ordered account/model fallbacks and eligibility
   reasons, distinguish quota exhaustion from context capacity, and bind queues, completed actions,
   worker ownership and recovery to explicit run ids. Preserve drafts and stop uncertain replay.
4. **Context-capacity handoff.** Explicit capacity-error recovery and optional Codex proactive
   rollover are implemented. Codex threshold/Claude automatic-compaction controls use documented
   process-local settings. Verify live behavior and unsupported capacity signals per
   provider/version. Unknown capacity stays unknown. Introduce rollover only after pointer readback,
   lineage and pinned decision delivery work; keep the policy visible and reversible.
5. **Provider and host parity.** Record fake-provider checks separately from real-provider and
   physical-host evidence. Live provider turns require an explicit scope and usage budget.
6. **OSS distribution.** Resolve security, terms, license/attribution and reproducible packaging
   gates in docs/release-blockers.md. Documentation must state shipped limits; publishing is a
   separate authorized action. Editing features and managed worktree cleanup follow core memory
   and orchestration rather than displacing them.

Acceptance details and research: [context handoff plan](docs/context-handoff-plan.md).

Locked-code launch gate: implemented (`provider::enforcement`, see [architecture](docs/architecture.md)).
A write-capable turn in a project with `docs/quality/locked-code.json` runs read-only unless a verified
receipt matches its provider, isolated home, policy revision and connected Harness identity. The
verifier writes that identity from the live manifest; file-backed manifests are re-hashed before a
protected launch, so a changed manifest closes the gate until verification runs again.

Bounded context handoff and explicit delegation already exist; see [delegation](docs/delegation.md).
Internal preview panes, raw editing/recovery, bounded file discovery and explicit quota fallback
are implemented. Broader policy-based routing and managed worktree cleanup remain requirements. Hosted services, resident inference and Linux/macOS
support remain outside current scope. Use `concordia --version` to identify an installed build; running windows retain their existing
executable until reopened.

## Open decisions (the maintainer's)

- GitHub organization and repository name (the product name, Concordia, is decided; a trademark check is still to do).
- Public-release terms review for automatic account handoff. Local opt-in handoff is configured under `[continuation]`; see [configuration](docs/config.md).
- When to publish, and the repository destination.

## References

- [WezTerm CLI](https://wezterm.org/cli/cli/index.html) and [multiplexing](https://wezterm.org/multiplexing.html)
- [Claude Code programmatic use](https://code.claude.com/docs/en/headless)
- [Codex authentication](https://learn.chatgpt.com/docs/auth)
- [Cursor ACP](https://cursor.com/docs/cli/acp)
