# Architecture

Concordia owns local coordination and presentation. Official provider CLIs own model execution,
authentication, native sessions and native approvals. [The request walkthrough](how-concordia-works.md)
explains the flow; this page identifies the code that owns each invariant.

## Ownership

| Concern | Canonical source owner | Contract |
| --- | --- | --- |
| Tabs and event loop | `tui/run.rs`, `tui/tabs.rs` | One App and channel set per tab; background work stays with its conversation |
| Submission and UI | `tui/input.rs`, `tui/input_dispatch.rs`, `tui/draw/` | Explicit input dispatch; open controls own their keys; drafts remain separate from sent requests |
| Provider execution | `provider/turn.rs`, native transport modules, `provider/launch.rs` | One tracked executor per turn; arguments/stdin; explicit cancellation and native ownership |
| Account selection | `accounts.rs`, `store.rs`, `connect.rs` | Explicit account/provider/home identity; isolated native authentication |
| Continuity | `context.rs` | Same-home exact resume when valid; bounded map for a new native session |
| Sources and retrieval | `native_memory.rs`, `native_references.rs`, `memory_*.rs` | Bounded reads, explicit provenance, source revision checks and honest unavailability |
| Optional thread record | `threads.rs` | Metadata/references plus bounded pinned spans; legacy compatibility |
| Main-turn retry | `tui/continuation.rs`, `routing.rs` | Explicit retry eligibility, configured targets, bounded attempts and review of uncertain work |
| Workers | `delegate.rs`, `delegate/`, `delegate_*.rs` | Per-run ownership, scheduling, result collection, recovery and native permission bounds |
| Workspace ownership | `workspaces.rs`, `workspace_paths.rs`, `managed_*.rs` | Cooperative reservations and explicit managed Git workspaces; not a replacement for native confinement |
| Files and attachments | `file_*.rs`, `attachments.rs`, `tui/workspace.rs`, `tui/raster.rs` | Explicit preview/edit/save and immutable attachment snapshots |
| Usage | `usage.rs`, `usage_codex.rs`, `cursor_usage.rs` | Identity-bound cached observations; no inference probes or invented remaining capacity |
| Configuration | `config.rs`, `store.rs`, `tui/appearance.rs` | Config supplies defaults; saved choices override them without rewriting user config |
| Extensions | `harness/`, `integration.rs`, `mcp.rs` | Explicit local adapters/tools; native permissions and locked-code receipts remain authoritative |

Paths are relative to [`crates/concordia/src`](../crates/concordia/src).

## State and recovery

Conversation, account/home, native session, run/attempt and workspace IDs are distinct. Never infer
ownership from the focused tab, the newest source file or `resume --last`.

The UI activity phase is presentation, not execution authority. Main continuation uses one retry
state: ready, deferred behind a local control, or requiring review. Request provenance and context
capacity remain separate because they determine whether a new native session is permitted.
[State ownership](tui-states.md) inventories explicit and implicit lifecycle owners, transition guards,
persistence, valid coupled-field combinations and remaining coverage limits, including clarification
replies and the per-tab local shell. It is not an exhaustive reachability proof.

Recovery binds drafts, queued prompts, unfinished requests and worker ledgers to their owning pane.
Restart cannot prove that a tool finished. Preserve the record, reconcile uncertain effects and
require explicit action instead of silently replaying them.

## Memory and delivery

Native provider history is authoritative. Concordia indexes source identity and serves bounded
excerpts; configured thread recording and local pins provide additional bounded retention.
A map is a table of contents with source pointers, not a model-generated summary or a complete
transfer of native memory. Native compaction still follows the selected provider/policy.

Context is prepared and delivered for a submitted message or authorized worker task. Account/model
selection alone does not dispatch it. Handoff, CLI and MCP use the same canonical map v2 formatter.
[Continuity](continuity.md), [map format](continuity-map-v2.md),
[storage/bounds](native-memory.md), [prompt composition](how-concordia-builds-prompts.md).

## Prompt queue

Queue entries are unsent requests. A clean completion may advance the queue through the normal
submission path. Failure/cancellation and recovery pause it. Editing a queued entry is separate
from composing a new message. Queue focus, edit and admission state live in `tui/queue.rs` and its
control modules; provider errors never authorize an uncertain replay.

## Boundaries

No hosted memory backend, inference for routing/summaries, credential collection or autonomous
plugin runtime. Read/write capability comes from the native provider's permission mode and any
verified locked-code receipt; cooperative reservations do not grant additional native permission.

Current capabilities belong in [PLAN.md](../PLAN.md), limits/evidence in
[verification status](e2e-status.md), and proposed work in the [backlog](plans/README.md).
Command/config reference details belong in [UI glossary](ui-glossary.md) and [configuration](config.md).
