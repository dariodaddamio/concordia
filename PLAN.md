# Concordia plan

Concordia is a local Windows interface for the official Codex, Claude Code and Cursor CLIs.
It keeps conversations, account switches, source references and workers in one Rust application.
Providers own authentication, native sessions, tools and approvals. Concordia owns the interface
and local coordination. It makes no inference calls for routing, indexing or summaries.

Status: private, pre-release. The public release is held until the owner resumes publication
after the pending GitHub Student approval. No scheduled release is authorized.

## What works today

| Area | Implemented behavior | Limits |
| --- | --- | --- |
| Chat and accounts | Native window or terminal UI; independent tabs; isolated provider accounts; model selection; exact-session resume; usage where reported | Compatibility depends on the installed CLI; unknown usage remains unknown |
| Memory | Native-source indexing, bounded history/search, explicit chat references and deterministic context maps across providers | Source files must remain available; maps do not transfer native tool or approval state |
| Workers | Explicit delegation, queues, result collection, durable recovery, bounded quota failover and cooperative path reservations | Broader live approval/host parity and managed worktree integration/retirement remain open |
| Workspace | File discovery, content search, previews, raw editing with explicit save, image/PDF input and local Mermaid rendering | Native permissions still apply; transport support and rendering proof vary by provider |
| Customization | Settings, themes, scoped skills/hooks inspection and optional local adapter integrations | Integrations are explicit local connections, not a hosted memory service |

Current repair evidence is in [chat rendering](docs/chat-rendering.md) and
[E2E status](docs/e2e-status.md). Detailed technical references are
[architecture](docs/architecture.md), [native memory](docs/native-memory.md),
[delegation](docs/delegation.md) and [workspace panes](docs/workspace-panes.md).
These documents describe implementation; proposed behavior belongs in the
[backlog](docs/plans/README.md).

## Product direction and claims

The owner's working direction is a fast, inexpensive TUI IDE across provider accounts.
The current useful distinction is one local workspace for conversations, original-source
retrieval and workers across those providers. Rust is the implementation choice; it does not
by itself prove speed, safety of spawned tools or a competitive advantage.

Do not claim proven memory superiority, token savings, complete native parity, arbitrary
provider support or a finished IDE. Existing small trials establish scoped behavior, not
broad effectiveness. Hermes integration and additional providers remain proposals.
Pointers improve navigation and retrieval; they do not replace or disable native compaction.
Chat groups support explicit membership; automatic organization of repetitive chats is not
established by lexical related-chat suggestions. Local adapter connections are explicit local
integrations, not an autonomous shared-memory service.
The whole-repository 08g audit and 08h continuity follow-through are complete. The owner postponed
product direction (step 7) and requested removal of outdated implementations, silent fallbacks,
obsolete builds/worktrees/branches and redundant backups. Keep one current implementation per
feature, report unsupported capabilities explicitly, and preserve user data and current tab recovery.
The audience, distribution destination and remaining feature priorities are owner decisions.
The owner subsequently requested a Photoshop-style dockable workspace with configurable runtime
panels. This is a new planned UI feature; its requirements live in the [canonical backlog](docs/plans/README.md#dockable-workspace-and-runtime-panels).
The broader product/support discussion remains postponed. Existing closeout work is still open:
retained-snapshot inspection, Git checkpoint/candidate qualification, native hook/provider/host
verification, missing native signals and documentation consolidation.

## Execution order

1. Make daily chat reliable: input, streaming, history, drafts, recovery, account/model controls
   and accurate context delivery. Reproduce reported failures through actual input paths.
2. Preserve the completed 08g [repository audit](docs/agent/cleanup-review.md), its repairs and
   evidence. Work continues here; the earlier desktop transition was cancelled. Keep source identity, recovery and approval
   contracts intact; preserve records and pending work.
3. Qualify the selected provider/host scope. Track missing native data separately from bugs:
   Cursor capacity/external usage and omitted numeric credit balances remain known gaps.
4. Complete the current feature/artifact cleanup, verify and install the result, and reconcile
   the source work with Git. Product direction remains postponed until the owner resumes step 7.
   No publication is implied.

The 09a source cleanup is verified and installed. Three old worktrees/build trees and redundant
archives are removed. The owner subsequently removed the remaining 7.75 GB main build tree;
absence was verified before 09b work. 09b is verified and installed: local shell mode, tab interaction
improvements, noninterrupting clarification replies and the expanded implicit state-machine
inventory. The single temporary release build tree was removed after verification and installation.
09c is verified and installed: one chat owner across tabs/windows, orphan interrupted-request
restoration and checkpoint authority before first native-session delivery. Twenty-one focused
checks and six exact-binary native-window recovery/navigation scenarios pass, with no model calls.
09d moves tab renaming into an independent inline editor and is verified and installed. The native
window check covers save/cancel, both drafts, restart and single-tab editing without model calls.
09e fixes large Codex app-server frame handling and distinguishes confirmed provider exit from
unconfirmed tool completion. Real-process and native-window regressions pass; one tiny direct
native CLI live smoke passes. The installed executable matches the verified candidate hash.
09f fixes restored-tab email refresh, adds recovery command completion and makes normal startup
quiet. Exact native-window account/history/recovery checks pass without model calls.
09g repairs setup command discovery, shared skill/project setup and identical native request
retransmissions. Focused process, terminal and native protocol checks pass. The optimized executable
passes the exact-binary terminal replay and is installed with a matching checksum. Version 2 setup
storage preserves legacy records and prevents older writers erasing shared intent. Native hook
trust remains a provider-client review step. The 1.8 GiB verification cache and unused installer
image are removed; the older image still used by the existing window is preserved.
Current source changes remain local and uncommitted. Product direction (step 7) remains postponed.

## This slice

### Commands and shared setup - anti-slop gate

| Paths | Tendency check | Ownership and deletions |
| --- | --- | --- |
| `provider/interaction.rs`, native transports | 2, 3, 4, 10 | Broker owns request identity; replace unconditional duplicate failure with exact retransmission handling. |
| Commands, palette, slash, settings, integrations UI | 1, 4, 7 | Register skills/hooks; keep one cancellable setup execution path. |
| `harness/`, accounts, provider turn entry | 4, 5, 6 | Existing store owns shared intent and receipts; account discovery includes hidden homes for cleanup. |
| README, changelog, build stamp | 9, 10 | Record verified behavior and remaining native trust requirements. |
| Existing native-interaction test callers, process fixtures | 3, 9 | Adapt Option admission; verify reported retransmission and setup cases. |

Acceptance: commands appear in completion and perform their stated actions; shared setup reaches
supported provider homes; identical native retransmissions keep one pending answer. Changed
request payloads, source revisions, cancellation and native consent remain explicit boundaries.

## Engineering contracts

- Bind work to explicit workspace, conversation, account, native-session, run and pane IDs.
  Never recover through `resume --last`, newest-chat or focused-pane heuristics.
- Preserve native authentication and approvals. Never overwrite provider homes, collect
  credentials or silently change provider compaction policy. Pass prompts by arguments/stdin.
- Prepare context locally; dispatch only an explicitly submitted turn or authorized worker.
  Unknown usage never means free capacity. Keep failover bounded and eligibility visible.
- Preserve drafts, queues, original sources and uncertain actions. One owner captures/reconciles
  each run. Do not blindly replay tools or reset, delete, merge or clean user work.
- Keep reads bounded and detect missing, replaced or stale source files. Never substitute an
  unrelated chat. No hosted memory backend, resident model, quota inference probes or Docker.
- Keep implemented behavior separate from target design in
  [context handoff](docs/plans/context-handoff-plan.md). Preserve locked-code enforcement and receipts.
- Follow AGENTS.md's scoped end-to-end development loop. Broad checks belong to explicitly
  requested delivery qualification. Live verification follows its small-model budget; report
  fixture, live-provider and physical-host evidence separately.
- Bump the build stamp for visible changes and record them in CHANGELOG.md. Running windows
  retain their executable until reopened. Publishing remains a separate owner action.

The [completion audit](docs/feature-completion-audit.md) holds detailed outstanding evidence;
[release blockers](docs/release-blockers.md) holds packaging, privacy, compatibility and release
gates. Historical build changes belong in CHANGELOG.md, not this plan.
