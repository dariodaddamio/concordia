# Memory relationships and provider interaction completion

Status: planned, 2026-11-02. Baseline: `c6d3b7c` / build `2026-11-02j`.
This plan adds relationships, evaluation and provider interaction to the existing native-source
memory manager. It does not reopen whole-history pagination, exact readback, lexical search,
observed handoffs, durable worker recovery or Codex proactive rollover as unbuilt features.
No production code or real provider turn is part of this planning change.

## Scope, assumptions and acceptance

The implementation remains local Rust. Native CLIs own tools, authentication, approvals and
session state. Concordia records bounded metadata and source pointers; it does not add transcript
copies, embeddings, a summarizer, a hosted index or model calls for discovery. Existing optional
thread logs remain an independent setting and are neither deleted nor enabled by migration.

Completion means a user can discover related work with an inspectable reason, verify every
retrieved excerpt against its original, and answer a supported native request for the exact main
or worker run. Unsupported provider capabilities must remain explicit. Fixture evidence,
installed-host evidence and authorized live-provider evidence are separate release gates.

Ownership: this document owns `M*` relationship/evaluation slices and `P*` interaction/capacity
slices. Scheduling, routing, worker attempt identity and accounting belong to
[the orchestration plan](orchestration-completion.md). Input payloads and plugin integration
belong to [the workspace/input plan](workspace-input-completion.md). Neither adjacent plan may
introduce an alternative approval broker, source reader or provider capability registry.

## Source audit

| Existing source | Implemented behavior | Remaining gap |
| --- | --- | --- |
| `src/native_memory.rs` | Provider/account/home/session bindings, immutable bounded source pages and hash-verified excerpts | Relationships need to reference these identities without creating another reader |
| `src/memory_retrieval.rs` (`native_memory::retrieval`) | Up to eight discovered source windows, twelve lexical hits, 512-byte queries, exact result pointers; explicit links and observed handoffs | Goal/file relationships, deterministic multi-signal ranking and related-chat groups |
| `src/tui/landing.rs` | Project/worktree-root session grouping; bounded goals stored in `goals.json` | Goals have no native-source links; project grouping is not content-based grouping |
| `src/capacity.rs`, `src/native_memory.rs::context_measurement` | Account/home policy, Codex-only native context measurement, reversible per-process compaction controls | Capability provenance and verified Claude/Cursor capacity capture |
| `src/provider/turn.rs::NativeRequest` | Method, raw parameters and a response channel that can answer once | No typed provider-independent request or explicit run/attempt owner envelope |
| `src/provider/turn_acp.rs` | Cursor image turns handle native permission and question requests with session checks | Ordinary Cursor text turns still use the print adapter; plan-approval extensions are unsupported |
| `src/tui/questions.rs`, `src/tui/mod.rs::apply_progress` | Main-run native ACP choices and separate conversational replies | Broader native request kinds and concurrent worker ownership |
| `src/delegate.rs::apply_progress` | Cancels native worker interaction requests because no foreground owner exists | A bounded worker waiting-input queue and dedicated review surface |
| `src/provider/compat.rs` | Bounded help/version probing and a legacy flag cache | Legacy `Caps::assume_all()` is not evidence for interactive protocols or capacity |

All paths in this table are relative to `crates/concordia/`. Existing evidence lives in
`tests/native_memory.rs`, `tests/images.rs`, `tests/mcp.rs`, `tests/tui_journeys.rs`,
`src/tui/questions.rs` and the capacity/retrieval module checks. Passing them does not prove the
planned additions.

## Provider capability evidence

Official documentation was opened on 2026-11-02. The matrix states a documented integration
path, not a claim about every installed version or a completed Concordia adapter.

| Provider | Native interaction path | Capacity/compaction evidence | Concordia completion condition |
| --- | --- | --- | --- |
| Codex | App-server documents bidirectional stdio, approval requests scoped by thread/turn/item, and resolution notifications | It documents thread usage updates and compaction events/methods; current Concordia uses the separate Codex native-log measurement | Add a version-matched app-server adapter behind explicit capability selection; retain existing source measurement until replacement semantics are proven |
| Claude Code | CLI documents `--permission-prompt-tool` for a loaded MCP permission tool; Agent SDK documents approval/question callbacks | Statusline documents current context fields, including a native window size and nullable usage; environment docs describe compaction controls | Verify a supported capture route for the actual `-p` transport and the permission-tool schema; SDK examples do not establish an arbitrary Rust wire protocol |
| Cursor | ACP documents session loading, permission requests, cancellation, questions and explicit plan requests | The inspected ACP page does not establish a current-context/window signal or a compaction-off control | Generalize the existing ACP adapter for text only after permission/session parity; keep proactive capacity unavailable until native evidence exists |

Sources for the corresponding claims: [Codex app-server](https://learn.chatgpt.com/docs/app-server),
[Claude CLI reference](https://code.claude.com/docs/en/cli-reference),
[Claude approval callbacks](https://code.claude.com/docs/en/agent-sdk/user-input),
[Claude statusline fields](https://code.claude.com/docs/en/statusline),
[Claude environment controls](https://code.claude.com/docs/en/env-vars), and
[Cursor ACP](https://cursor.com/docs/cli/acp).

The absence of a signal in the inspected Cursor page is a limit of this evidence, not proof that
no version can ever expose it. Likewise, documented Claude statusline output is a research lead;
this plan does not install a statusline command or change global provider configuration.

## Shared contracts before parallel implementation

### Source and relationship identity

Use native `Binding` plus immutable source-index/message references as the authoritative source
key. A relationship record has a schema version, stable relation ID, canonical workspace ID,
typed endpoints, kind, provenance and optional exact evidence pointer. Endpoints may be a native
source, an existing goal ID, a project-relative file identity or an explicit group ID.

Kinds remain distinct: user-confirmed association, observed Concordia handoff, source-observed
file reference, completed tool action and lexical/time suggestion. A requested edit is not a
completed action. A lexical match never writes a confirmed relation. Timestamps cannot establish
ancestry. No transitive suggestion is presented as an exact parent link.

Keep new metadata beside native-memory state in a versioned relationship store. Do not rewrite
`goals.json` merely to add a link. Preserve old `Link` and `Handoff` records; read them through one
compatibility boundary, with their original provenance, and leave originals intact. Unknown
schema versions or unreadable stores fail visibly rather than being overwritten with empty data.

### Interaction owner and capability evidence

Add a shared owner envelope containing conversation, workspace binding, run UUID, attempt identity,
provider, canonical account-home identity, native session/thread, native turn/item where supplied,
and provider request ID. The orchestration plan supplies a logical `TaskId`, existing UUID
`RunId`, and a new UUID `AttemptId` for each native process launch/failover; an explicit reconciled
retry creates a new run. Its `WorkspaceBinding` separates the owner workspace from execution
cwd/worktree. Reuse these identities and preserve the distinction between owner and execution
roots. Native request IDs
are unique only within their owning connection; a pane number or the currently focused chat is
never enough to answer one.

Separate native permission, native question, native plan approval and conversational follow-up
types. Preserve native option IDs and decision scopes. A transport adapter encodes the response;
the UI cannot invent an allow decision, broaden its scope or convert a question into permission.

Use `supported`, `unsupported` and `unknown` per capability with evidence: executable/version,
schema or negotiated capability, transport and observation time. Help/version results may prove
a flag exists, not that its runtime protocol works. New capabilities default to unknown even when
the legacy compatibility cache assumes old output flags. Probe failures never authorize fallback
to a more permissive trust mode.

Initial proposed bounds: eight unresolved native requests per run, thirty-two across the app,
256 KiB aggregate request display data per run, and existing transport frame/output limits.
Exceeding a limit cancels/declines through the native protocol and leaves an inspectable reason.
Pending prompts never auto-approve on a timer. Cancellation, lost transport, changed owner or
restart invalidates responses. Persist only a paused/interrupted marker and identity metadata;
never persist a response channel, an approval grant or raw request bodies for automatic replay.

## Memory implementation slices

### M0 — Explicit goal/source associations

First independent memory slice: introduce the versioned relationship model/store and explicit
associate/detach/read operations using an existing goal ID and verified source pointer. Suggested
new module: `src/memory_relations.rs`, exposed through the existing native-memory boundary.
Wire read-only relation display and explicit association into `src/tui/landing.rs`, the current
memory command path in `src/tui/mod.rs`, `src/chat_cli.rs` and `src/mcp.rs` where appropriate.
There must be one implementation for validation/readback across CLI, TUI and MCP.
MCP exposes relation discovery/readback only in this slice. A model-requested association can
later become a proposal for explicit user confirmation; a tool call cannot label its own link
user-confirmed.

Use atomic writes and a workspace lock. Start with a 2 MiB metadata store, at most 1024 relations
per workspace, and explicit full-store/invalid-source errors. These are proposed limits to verify
against fixture measurements, not existing product guarantees. Removing an association never
deletes a goal, source log, source page or earlier provenance event. Closing a goal retains links.

Acceptance: same native session ID in different homes remains distinct; attaching a stale pointer
fails; concurrent panes cannot lose unrelated links; duplicate attach is idempotent; detach affects
only the specified association; old goals and links still load unchanged; no transcript text is
written. Finish with same-change user documentation and build-stamp/changelog updates.

### M1 — File relationships and explainable ranking

Extract file references only from bounded, verified source records already being read. Store at
most 64 evidence-backed file facts per source page initially; report overflow. Normalize paths
against the recorded workspace, retain provenance and avoid opening arbitrary referenced files.
Reject paths escaping the workspace for workspace-file matching. Renames, deleted files and
unresolvable paths remain historical facts, not proof of current disk state. Do not add secret-file
content indexing or an implicit workspace crawl.

Extend `src/memory_retrieval.rs` with a deterministic rank result carrying individual reasons:
confirmed goal/source relation, explicit handoff, exact file association, lexical hit, and temporal
distance. Select and document tie-breaking by stable source ID/message ordinal. Rank only within
the declared count/byte budget; retain `incomplete` and stale/unavailable reasons. A higher score
cannot bypass source verification or promote a suggestion into a confirmed relation.

Acceptance: irrelevant keyword collisions rank below an exact confirmed association; same-named
files in different workspaces do not collide; stale evidence is withheld; ranking order is stable
for the same snapshot; every displayed reason can resolve its evidence; Unicode/timezone boundary
cases keep exact excerpts and do not fabricate dates.

### M2 — Related-chat groups and discovery UI

Add explicit stable group IDs and membership in the relationship store. Offer groups suggested by
shared goals/files/time separately from confirmed membership and observed handoff chains. Name a
group from the selected native title or existing deterministic title fallback; do not generate a
title with a model. Keep current project grouping in `src/tui/landing.rs`; add a related-work view
that preserves draft, selection and project context. Group confirmation/detachment must be a
deliberate user action. A stale or missing member remains visibly unavailable.

Dependency: M0 identity/storage and M1 ranking. Acceptance includes overlapping groups, cycle-safe
lineage display, deleted sources, worktree-root equivalence, cross-project suggestions labelled as
such, bounded pagination and late asynchronous results discarded after conversation changes.

### M3 — Retrieval and retention evaluation

Define a versioned synthetic corpus and expected results before tuning ranking. Include long logs,
same-session IDs in different homes, overlapping filenames, contradictory versions, compaction,
malformed/oversized records, deleted/rewritten sources and older history pages beyond discovery
limits. Reuse existing fake-provider/source fixtures; keep all material synthetic. No new unit or
integration tests are authorized by this plan; implementation uses existing suites, synthetic
fixtures and explicit integrity/contract matrices under project guidance.

Report precision/recall at the declared hit budget, exact-excerpt resolution, stale-source false
acceptance, coverage/incomplete rate, bytes read, elapsed time and peak retained metadata. Required
integrity gates are zero cross-account substitution, zero stale excerpt acceptance and zero silent
source truncation. Every seeded relevant item inside a fully searched declared scope must remain
reachable; partially searched scopes must be labelled. Benchmark recall improvements against the
current lexical baseline, with the same corpus and budgets, before changing ranking defaults.

Model constraint-retention/task-success comparisons are a different experiment. Prepare native
resume/compaction, fresh handoff, and compaction-plus-pointers arms, exact decision constraints and
side-effect reconciliation checks. Run no live arm until the owner supplies provider/task scope
and an explicit usage budget. Fixtures establish protocol behavior, not model recall superiority.

## Provider implementation slices

### P0 — Shared interaction/capability contract

First independent provider slice: add the owner-bound typed request and tri-state capabilities,
with conversion for the existing Cursor image path. Suggested new module:
`src/provider/interaction.rs`; integration points are `provider/turn.rs`, `provider/compat.rs`,
`provider/turn_acp.rs`, `tui/questions.rs`, `tui/mod.rs` and `delegate.rs`. Preserve the existing
oneshot single-response guarantee. Avoid a second question/approval queue in another subsystem.

P0 is an independently reviewable foundation, not completed worker approval support. The first
usable native-worker approval milestone is P0 plus P1 plus one verified transport adapter (P2 or
P3). The existing Cursor image adapter alone does not make text-only workers interactive.

Do not change a provider's default transport or trust mode in this slice. Existing conversational
digits/notes remain message text; native option shortcuts stay native. A malformed/unknown request
is shown as unsupported and receives the adapter's decline/cancel response, never a generic allow.
Acceptance includes duplicate response, response-after-cancel, same request ID on two connections,
wrong run/attempt/home/session, out-of-order resolution and old compatibility-cache migration.

### P1 — Foreground and worker interaction routing

Add a `waiting-input` worker state and a bounded owner-scoped pending queue. `src/delegate.rs`
must stop cancelling every request merely because the worker is in the background. Surface the
request in `src/tui/workers.rs` and the common interaction view without switching the main account,
launching a main-agent turn or overwriting its draft. Native decisions affect only the selected
worker attempt. Returning to chat leaves the pending request visible; cancellation acts on its
owner. Auto-collection and quota failover cannot run while approval/tool completion is uncertain.

A restart restores the worker paused, not a live approval. An old click cannot approve a retried
attempt even if native request IDs repeat. Ordinary worker output cannot create native requests.
Dependency: P0 and the orchestration attempt-identity contract, not the full dependency scheduler.

### P2 — Codex native approval adapter

Implement a local stdio app-server transport using the version-matched generated schema and
existing isolated home/launch/process guards. Map thread/turn/item IDs and native approval/resolution
events to P0. Use the provider's decisions as offered; policy amendments and session-wide grants
need a distinct scope label and explicit selection. Do not reuse the documentation's permissive
example settings. Keep default trust and locked-code enforcement authoritative.

Choose the adapter visibly per supported capability; switching transport between turns must bind
the exact existing native session and reconcile it before a prompt. A connection error after a
tool starts is uncertain work, not permission to retry through `codex exec`. A live connection
may survive consecutive turns, but there is only one active executor and its shutdown is tracked.
Do not add WebSocket hosting, remote authentication or provider credential handling.

### P3 — Cursor ACP parity for text and plans

Refactor the existing ACP path to accept text-only turns as well as images, without duplicating
`session/load` history reading. Add documented native plan approval alongside permission and
question types: the inspected [Cursor ACP extension reference](https://cursor.com/docs/cli/acp#cursor-extension-methods)
names `cursor/create_plan` as a blocking request. Preserve exact session/account/workspace bindings
and negotiated support.
Unsupported extensions fail visibly; no success response is fabricated. Compare text, image,
resume, permission denial, plan rejection, cancellation and end-of-turn semantics against the
existing fake provider before changing adapter selection. Do not assume ACP usage availability
from a JSON print-mode usage field.

### P4 — Claude native permission bridge

Use the documented CLI permission-tool boundary as the first Rust-compatible candidate: a
Concordia-owned local MCP permission tool for this exact launch, connected to P0's broker. First
verify the version-specific tool request/result schema and supported request kinds. SDK callback
examples are reference behavior, not evidence for undocumented `stream-json` control messages.
If no complete supported contract can be established, ship the capability as unavailable and keep
the existing adapter; do not add a Node/Python runtime or scrape an interactive terminal.

The permission bridge must accept messages only from its owned provider process/connection,
check the run/attempt/session binding and restrict IPC to the current user. Supply per-launch
configuration without modifying a user's existing MCP configuration or provider home. Never expose
a general approval tool to model-originated arbitrary calls. A named MCP tool may also be callable
by the model: process ownership or the tool name alone does not prove a native permission request.
Require documented, verifiable native request provenance/correlation before enabling the bridge.
If the supported contract cannot distinguish native permission callbacks from arbitrary model
tool calls, leave this route unavailable and record that specific blocker. The native permission
system still decides which requests may be approved; tool-specific restrictions remain authoritative.

### P5 — Capacity evidence and opt-in rollout

Extend the measurement contract to include exact session/turn/model, source revision or live
connection generation, transport, native window/usage fields, invalidation reason and evidence
version. Preserve the existing Codex reader as the current supported implementation. Unknown,
malformed, stale, cumulative-only or post-compaction-unrefreshed measurements never trigger a
rollover. Compare percentages with overflow-safe arithmetic; do not estimate a window from model
names, usage totals or character counts.

For Claude, first establish whether a documented per-launch interface can deliver current-context
and window fields for the headless adapter. Do not install a global statusline/hook to obtain them.
For Cursor, require an actually supported negotiated/reported signal in the selected CLI version;
an ACP schema containing a field is insufficient. If either path is unavailable, keep `/context`
explanations explicit and reactive capacity-error recovery usable. Missing upstream capacity is
not a blocker to M0–M3 or P0–P4.

Enable a new provider policy only after fixture replay proves signal meaning, invalidation and
account/model isolation; then conduct the separately authorized live parity run. Before-turn
rollover reuses current original-source verification, pinned-decision delivery, checkpoint and
cancellation handling. Never interrupt/replay in-flight work or silently disable native compaction.

## Contract matrix and delivery gates

| Scenario | Required observable outcome | Existing verification surface to reuse |
| --- | --- | --- |
| Source replaced, truncated, moved or wrong home | Relation/excerpt unavailable; no unrelated substitution | `tests/native_memory.rs`, native-memory checks |
| Legacy goal/link stores, duplicate writes, concurrent panes | Original data preserved; atomic idempotent relation update | Landing/retrieval checks and synthetic state integrity matrix |
| Bounded search misses an older page or hits a limit | Explicit incomplete coverage and continuation pointer | `tests/mcp.rs`, native-memory CLI/MCP fixtures |
| Native request races cancellation, retry or conversation switch | One response at most; never delivered to the new owner | `src/tui/questions.rs`, `tests/images.rs`, fake-provider fixtures |
| Worker waits while main draft/queue is open | Correct worker shown; main draft/queue untouched; no automatic grant | Worker checks and existing terminal journey surfaces |
| Provider pipe closes after tool execution begins | Paused/uncertain outcome; no transport fallback replay | Provider/delegate recovery checks |
| Usage is absent, cumulative-only, stale or invalidated | Context remains unknown; no proactive rollover | Capacity checks and proactive-rollover terminal journey |
| Same capability name, changed CLI schema/version | Evidence invalidated; explicit supported/unknown result | Compatibility and protocol fixture checks |

For each production slice: source review, contract/integrity cases, relevant existing fixtures,
`cargo fmt --all --check`, `cargo test -p concordia --tests --locked`, all-target clippy with
warnings denied, release/OSS gates and installed smoke where user-visible behavior changed.
Run affected terminal journeys for interaction/layout changes. Record exact source/build identity
and distinguish fixture versus installed versus live evidence. Bump build identity/changelog only
with the corresponding production change, not for this planning document.

## Order, unresolved evidence and handoff

M0 and P0 can proceed independently. M1 follows M0; M2 follows M1; M3 begins by defining the corpus
before rank tuning and finishes alongside M2. P1 follows P0 and worker attempt identity. P2, P3
and P4 may be separate transport branches after P0, each integrating through the same broker;
serialize edits to `provider/turn.rs`, `delegate.rs`, `tui/mod.rs` and `tui/questions.rs` through
their designated owner. P5 depends on verified signal capture, not on completing every other UI.

The parent campaign assigns three first-wave owners: orchestration D1, provider P0, and the cwd
search model. M0 is the next independent memory slice after an owner becomes available; it does
not expand that three-owner wave. A coordinator owns shared TUI wiring and integration. Worker
queues and transport adapters follow P0 integration. No broad rewrite is required to start these
bounded slices.

Open evidence: Claude permission-tool schema details and headless context capture; Cursor native
capacity/compaction control; installed-version native auth/approval compatibility; physical-host
input behavior; controlled model-retention results. These are explicit research/validation rows.
Only live model experiments require the separate provider/task budget; local implementation and
fixture work can proceed now. Publishing, signing identity, terms decisions and release claims
remain in the parent release plan.
