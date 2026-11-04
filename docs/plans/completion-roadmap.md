# Remaining feature implementation plan

Active dispatch: [six Luna briefs and coordinator queue](luna-completion/README.md), reconciled
against build `2026-11-03w`. Use that queue for new work; the original wave order below is historical.

Current implementation/capability disposition: [2026-11-03s campaign map](../quality/overnight-roadmap-disposition.md).
The planning baseline and proposed designs below are historical; shipped contracts are linked there.

Status: planned, 2026-11-02. Baseline: committed build `2026-11-02j` (`c6d3b7c`).
This was the execution plan for the remaining features, prepared through three parallel source
audits. It does not mark these features implemented or authorize provider usage or publication.

Progress: [wave one](../quality/completion-wave1.md) implements D1/P0/search W1;
[wave two](../quality/completion-wave2.md) connects approved shared-workspace dependencies to
normal dispatch and paused recovery. Other phases below remain planned unless evidence says otherwise.
The [goal/source slice](../quality/goal-source-associations.md) records M0 implementation separately.
The [accounting slice](../quality/worker-accounting.md) records U1 and bounded main/worker views;
[native source observations](../quality/native-usage-coverage.md) add separate U2 coverage and U3
attempt/outcome/workspace distinctions without double-counting overlapping snapshots.
The [file facts and ranking slice](../quality/memory-ranking.md) implements opt-in M1;
[related-chat groups](../quality/related-chat-groups.md) add M2 explicit membership and discovery.
[Synthetic retrieval evaluation](../quality/memory-evaluation.md) defines M3 corpus and contract
measurements. Controlled model-retention experiments remain unrun and separately budgeted.
The [routing slice](../quality/routing-policies.md) connects local eligibility, ordered overrides,
approval-bound sequences and worker/main inspection; workspace isolation remains separate.

[Managed lifecycle](../quality/managed-workspaces.md), [Codex native input](../quality/codex-native-input.md),
[capacity provenance](../quality/capacity-provenance.md), [project replacement](../quality/project-replacement.md),
[file actions](../quality/file-actions.md), [text documents and pasted paths](../images-and-diagrams.md)
and [integration discovery](../quality/integrations-catalog.md) record later engineering progress.
[Cross-worktree dependency delivery](../quality/dependency-delivery.md) adds reviewed receipts and
retirement references. [Goal proposals](../quality/goal-proposals.md) add exact revision-bound
local review and attribution. [Inline cell rasters](../images-and-diagrams.md) add bounded decoding
and explicit viewer fallback. [Typed file outputs](../quality/file-output-artifacts.md) add bounded
native-event records and current-file actions. Verified binary transports and
native/physical-host parity retain their separate contracts below.

## Baseline and scope

The baseline passed 960 enabled local tests, all 48 installed Windows terminal journeys,
clippy, formatting, release and installed smoke, dependency checks and privacy gates.
Remote CI run `41546832324` passed both Windows and dependency jobs. The Bincode maintenance
exception and 20 MiB artifact budget remain explicit reviewed policies, not upstream fixes.
See [dependency security](../quality/dependency-security.md) and [release readiness](../release-blockers.md).

Whole-history pages, exact Cursor ACP replay, bounded lexical retrieval, confirmed/observed
lineage, optional Codex proactive rollover, native compaction controls, durable worker recovery
and bounded worker quota failover already exist. Keep them working while extending them.

This campaign covers every remaining area from the feature audit, plus the validation and
release work required to substantiate claims. Windows remains the platform target. Linux/macOS,
hosted memory, resident inference, automatic summaries and model-based routing are outside scope.

## Area plans and ownership

| Area | Implementation plan | Required outcome |
| --- | --- | --- |
| Worker dependencies and workspace lifecycle | [Orchestration](orchestration-completion.md) | Durable dependency scheduling, explicit workspace ownership, isolated worktrees and safe retirement |
| Routing and fallback | [Orchestration](orchestration-completion.md) | Ordered account/model policies with visible eligibility and bounded attempts |
| Usage and cost accounting | [Orchestration](orchestration-completion.md) | Run/attempt-bound main and worker usage, deduplicated totals, explicitly unknown costs |
| Memory relationships and grouping | [Memory and providers](memory-provider-completion.md) | Goal/file relationships, discoverable related chats, verified provenance and measurable retrieval |
| Context and native interactions | [Memory and providers](memory-provider-completion.md) | Capability-checked capacity policy and owner-bound native requests across main/worker runs |
| Search/replace and file opening | [Workspace and inputs](workspace-input-completion.md) | Bounded cwd search, version-checked replacement previews and the accepted file-opening choices |
| Images, documents, artifacts and integrations | [Workspace and inputs](workspace-input-completion.md) | Capability-gated inputs/rendering and explicit tool/plugin ownership |
| Provider, host and distribution evidence | [Validation and release](validation-release-completion.md) | Reproducible evidence for the exact candidate; claims and publication gates remain explicit |

Each detailed plan identifies current code, changes by phase, resource bounds, failure states,
acceptance cases and the first independently shippable slice. Source paths in those plans are
repository-relative. A phase is complete only when its evidence exists, not when its design is written.

## Integration order

1. **Contracts and migrations.** Agree on task/run/attempt identity, versioned workspace bindings,
   native interaction ownership, capability records and accounting origins. Extend the existing
   recovery path instead of creating a competing executor. Define old-record migration and
   fail-closed behavior before adding dispatch. The plan-specific field names are proposals until
   the owning implementation slice establishes the canonical type.
2. **First independent slices.** Build worker dependency validation/recovery; add run-bound native
   interaction/capability plumbing; build a cancellable cwd search result model. These can proceed
   in parallel after module ownership is fixed. Main-loop and command integration has one owner.
3. **Core orchestration and memory.** Add isolated workspace dispatch, visible fallback evaluation,
   deduplicated usage events and goal/file relationships. Link scheduler/accounting/interaction
   events through the same run/attempt identity. Account changes never transfer approval state.
4. **Workspace and input expansion.** Add hash-bound selected replacements and the shared file-action
   resolver, then host/provider-gated thumbnails, drag/drop, documents and artifact presentation.
   Keep attachment capture, source retrieval and native provider tools as distinct operations.
5. **Provider integrations and evaluation.** Migrate supported native interactions one adapter at
   a time. Enable proactive capacity behavior only for a verified provider/version measurement.
   Run synthetic retrieval/retention and repeated lifecycle evaluations before any live claim.
6. **Candidate and release.** Use the owner's selected unsigned packaging, rehearse
   the exact candidate, and close supported-host/provider evidence. Publication remains a separate
   explicit action. A preview may state narrower support without claiming the whole roadmap complete.

No calendar completion date is inferred from this ordering. Slice size and protocol discovery,
not the number of agents, determine the critical path.

## Parallel implementation rules

Use at most three implementation owners concurrently, with root coordinating integration.
Assign each a concrete branch/worktree and an exclusive set of files before implementation.
The current planning agents own separate documents; they are not running provider tasks.

| Lane | First slice | Integration boundary |
| --- | --- | --- |
| Orchestration | `orchestration:D1` dependency graph and recovery foundation | Returns explicit state/events; no concurrent edits to shared TUI dispatch |
| Provider/memory | `memory-provider:P0` interaction/capability contract | Receives task/run/attempt identity; TUI ownership integrated serially |
| Workspace/input | `workspace-input:W1` read-only project search | Returns cancellable results bound to workspace and source revision |

Slice IDs are document-scoped. These three owners are the total first wave, not three owners
per plan. Goal/source associations (`M0`), route explanations (`R1`), worker accounting (`U1`)
and the integration catalog are independently schedulable next slices when an owner is free.
Foundations do not close user-facing features: dependency submission/dispatch requires D1+D2;
usable native approvals require P0 plus routing/UI and a verified adapter transport.

Root alone owns `PLAN.md`, the completion audit, shared command registration/TUI loop integration,
`BUILD_STAMP`, changelog and final checks. When a lane needs a shared file, return a concrete patch
or arrange a handover; never race edits in that file. New dependencies require license, advisory,
artifact-size and lifecycle review. Do not add package tooling to satisfy a host-only workflow.

## Acceptance and progress records

For every slice, record its baseline commit, owner, scope, changed paths, data migrations,
acceptance cases, actual commands/results and remaining limitations. Use states `planned`,
`implementing`, `fixture-verified`, `installed-verified`, `live-verified` and `release-qualified`
where applicable. These states are separate; a fixture result cannot close a live-provider gate.

Use existing fake providers and contract matrices for routine work. No new unit/integration
test files are required by this planning change. Run checks appropriate to implemented changes;
native Cargo/terminal checks apply to source slices. Documentation-only changes need link,
privacy and diff checks. Do not repeatedly rebuild unchanged source to validate a plan.

Live provider turns require the scope and usage budget specified in `AGENTS.md`; planning,
source inspection and local fixtures can continue independently. Certificate/account provisioning,
paid services, artifact uploads, tags, publication and repository visibility changes are not
implied by this plan. Unknown capacity, price, authentication or host capability remains unknown.

The [feature completion audit](../feature-completion-audit.md) remains the shipped-status record.
Update it when a slice lands, together with the relevant detailed plan and user documentation.
